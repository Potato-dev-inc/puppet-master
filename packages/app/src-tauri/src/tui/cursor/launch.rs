//! Headless launch of `cursor-agent --print` and lookup of its configured default model.

use crate::tui::{HeadlessLaunch, LaunchSpec};
use std::path::PathBuf;

/// `--print --output-format json` plus the opt-in flags. `--trust` and `--force` are only ever
/// added when the caller explicitly asked; `--yolo`/`-f` are never passed.
fn cli_args(launch: &HeadlessLaunch) -> Vec<String> {
    let mut args = vec!["--print", "--output-format", "json"];
    if launch.trust_workspace {
        args.push("--trust");
    }
    if launch.allow_shell {
        args.push("--force");
    }
    if launch.read_only {
        args.extend(["--mode", "plan"]);
    }
    args.push(launch.task);
    args.into_iter().map(str::to_string).collect()
}

pub(super) fn headless_command(launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    let args = cli_args(launch);
    // Cursor Agent installs a PowerShell shim on Windows; run it through PowerShell rather than
    // asking ConPTY to execute the `.ps1` directly.
    #[cfg(windows)]
    {
        let shim = crate::tui::find_path_file("cursor-agent.ps1")
            .ok_or_else(|| "cursor-agent.ps1 was not found on PATH".to_string())?;
        let mut full = vec![
            "-NoProfile".to_string(),
            "-File".to_string(),
            shim.to_string_lossy().into_owned(),
        ];
        full.extend(args);
        Ok(LaunchSpec {
            program: "powershell.exe".into(),
            args: full,
            cwd: PathBuf::from(launch.project),
            read_only_enforced: launch.read_only,
        })
    }
    #[cfg(not(windows))]
    Ok(LaunchSpec {
        program: "cursor-agent".into(),
        args,
        cwd: PathBuf::from(launch.project),
        read_only_enforced: launch.read_only,
    })
}

/// Pull the model out of `cursor-agent about` output (ANSI escapes ignored).
pub fn parse_default_model(about: &str) -> Option<String> {
    let mut plain = String::new();
    let mut chars = about.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(ch);
        }
    }
    // Real output is aligned columns (`Model               Composer 2.5`); older/other builds
    // may print `Default Model: X`. Take the first column as the key, split on a colon or a run
    // of two or more spaces.
    plain.lines().find_map(|line| {
        let line = line.trim();
        let split = line.find(':').or_else(|| line.find("  "))?;
        let (key, value) = line.split_at(split);
        let key = key.trim();
        let value = value.trim_start_matches(':').trim();
        ((key.eq_ignore_ascii_case("model") || key.eq_ignore_ascii_case("default model"))
            && !value.is_empty())
        .then(|| value.to_string())
    })
}

/// The model Cursor uses when no `--model` is given, from `cursor-agent about`. Headless runs do
/// not report which model ran, so this is the best truthful answer; looked up once per process.
pub(super) fn default_model() -> Option<String> {
    static CACHE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut command;
            #[cfg(windows)]
            {
                let shim = crate::tui::find_path_file("cursor-agent.ps1")?;
                command = std::process::Command::new("powershell.exe");
                command.args(["-NoProfile", "-File"]).arg(shim).arg("about");
            }
            #[cfg(not(windows))]
            {
                command = std::process::Command::new("cursor-agent");
                command.arg("about");
            }
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            let mut child = command.spawn().ok()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    _ => {
                        let _ = child.kill();
                        return None;
                    }
                }
            }
            let output = child.wait_with_output().ok()?;
            parse_default_model(&String::from_utf8_lossy(&output.stdout))
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{headless_launch, profile_for};

    fn launch(read_only: bool) -> LaunchSpec {
        headless_launch(
            "cursor_agent",
            &HeadlessLaunch {
                read_only,
                ..HeadlessLaunch::new(".", "inspect")
            },
        )
        .unwrap()
    }

    #[test]
    fn trust_flag_is_added_only_on_explicit_opt_in() {
        let plain = headless_launch("cursor_agent", &HeadlessLaunch::new(".", "t")).unwrap();
        assert!(!plain.args.iter().any(|arg| arg == "--trust"));
        let trusted = headless_launch(
            "cursor_agent",
            &HeadlessLaunch {
                trust_workspace: true,
                ..HeadlessLaunch::new(".", "t")
            },
        )
        .unwrap();
        assert!(trusted.args.iter().any(|arg| arg == "--trust"));
        assert_eq!(trusted.args.last().map(String::as_str), Some("t"));
    }

    #[test]
    fn allow_shell_adds_force_only_when_asked_and_never_with_read_only() {
        let plain = headless_launch("cursor_agent", &HeadlessLaunch::new(".", "t")).unwrap();
        assert!(!plain.args.iter().any(|arg| arg == "--force"));
        let forced = headless_launch(
            "cursor_agent",
            &HeadlessLaunch {
                allow_shell: true,
                ..HeadlessLaunch::new(".", "t")
            },
        )
        .unwrap();
        assert!(forced.args.iter().any(|arg| arg == "--force"));
        assert!(headless_launch(
            "cursor_agent",
            &HeadlessLaunch {
                read_only: true,
                allow_shell: true,
                ..HeadlessLaunch::new(".", "t")
            },
        )
        .is_err());
    }

    #[test]
    fn launches_without_ide_and_uses_plan_for_read_only() {
        let spec = launch(true);
        #[cfg(windows)]
        {
            assert_eq!(spec.program, "powershell.exe");
            assert_eq!(spec.args[0], "-NoProfile");
            assert_eq!(spec.args[1], "-File");
            assert!(spec.args[2].ends_with("cursor-agent.ps1"));
        }
        #[cfg(not(windows))]
        assert_eq!(spec.program, "cursor-agent");
        assert!(spec.args.windows(2).any(|pair| pair == ["--mode", "plan"]));
        assert!(spec.read_only_enforced);
    }

    #[test]
    fn headless_launch_never_passes_trust_bypass_flags() {
        for read_only in [true, false] {
            let spec = launch(read_only);
            assert!(!spec
                .args
                .iter()
                .any(|arg| matches!(arg.as_str(), "--trust" | "--yolo" | "--force" | "-f")));
        }
    }

    #[test]
    fn the_ide_launcher_cannot_run_headless() {
        assert!(profile_for("cursor").headless_command(&HeadlessLaunch::new(".", "t")).is_err());
    }

    #[test]
    fn default_model_is_parsed_from_about_output() {
        let about = "About Cursor CLI\n\nCLI Version         2026.10.01-e373342\nDefault Model:      \u{1b}[1mComposer 2.5\u{1b}[0m\nSubscription Tier   Pro+\n";
        assert_eq!(parse_default_model(about).as_deref(), Some("Composer 2.5"));
        assert!(parse_default_model("Version: 1").is_none());
    }

    #[test]
    fn default_model_is_parsed_from_real_about_columns() {
        let about = "About Cursor CLI\n\nCLI Version         2026.10.01-e373342\nLatest              2026.10.01-e373342 (up to date)\nModel               Composer 2.5\nSubscription Tier   Pro+\n";
        assert_eq!(parse_default_model(about).as_deref(), Some("Composer 2.5"));
    }
}
