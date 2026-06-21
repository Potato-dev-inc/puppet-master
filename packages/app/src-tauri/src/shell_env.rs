//! Interactive shell PATH for GUI-launched apps.
//!
//! macOS `.app` / DMG launches inherit a minimal PATH that omits Homebrew,
//! npm globals, nvm, cargo, etc. Dev (`npm run tauri -- dev`) works because the
//! process is started from a terminal with a full PATH. We mirror the login
//! shell PATH so CLI presets (`codex`, `claude`, `opencode`, …) resolve the
//! same way as in Terminal.

use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::PathBuf;

static INTERACTIVE_PATH: Lazy<String> = Lazy::new(discover_interactive_path);

/// PATH string to pass to spawned PTY children.
pub fn path_for_spawn() -> &'static str {
    &INTERACTIVE_PATH
}

/// Apply the interactive PATH to the current process (bridge, MCP helpers, etc.).
pub fn apply_to_process() {
    let path = path_for_spawn();
    std::env::set_var("PATH", path);
    #[cfg(windows)]
    std::env::set_var("Path", path);
}

fn discover_interactive_path() -> String {
    if let Some(path) = read_login_shell_path() {
        if !path.is_empty() {
            return dedupe_path(&path);
        }
    }
    dedupe_path(&build_fallback_path())
}

#[cfg(unix)]
fn read_login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = std::process::Command::new(&shell)
        .args(["-l", "-c", "echo -n \"$PATH\""])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(not(unix))]
fn read_login_shell_path() -> Option<String> {
    read_windows_path()
}

#[cfg(windows)]
fn read_windows_path() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$u=[Environment]::GetEnvironmentVariable('Path','User');$m=[Environment]::GetEnvironmentVariable('Path','Machine');if($u-and$m){\"$u;$m\"}elseif($u){$u}else{$m}",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(all(not(unix), not(windows)))]
fn read_windows_path() -> Option<String> {
    None
}

fn build_fallback_path() -> String {
    let mut segments: Vec<String> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        segments.push("/opt/homebrew/bin".into());
        segments.push("/opt/homebrew/sbin".into());
    }

    segments.push("/usr/local/bin".into());
    segments.push("/usr/bin".into());
    segments.push("/bin".into());
    segments.push("/usr/sbin".into());
    segments.push("/sbin".into());

    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let home = PathBuf::from(home);
        #[cfg(windows)]
        {
            if let Ok(pf) = std::env::var("ProgramFiles") {
                segments.push(format!(r"{pf}\nodejs"));
            }
            if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
                segments.push(format!(r"{pf86}\nodejs"));
            }
            segments.push(
                home.join("AppData")
                    .join("Roaming")
                    .join("npm")
                    .to_string_lossy()
                    .into_owned(),
            );
            segments.push(
                home.join("AppData")
                    .join("Local")
                    .join("fnm")
                    .to_string_lossy()
                    .into_owned(),
            );
            segments.push(
                home.join(".cargo")
                    .join("bin")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        #[cfg(not(windows))]
        {
            for sub in [
                ".local/bin",
                ".cargo/bin",
                ".npm-global/bin",
                "bin",
                ".volta/bin",
                ".fnm/current/bin",
            ] {
                segments.push(home.join(sub).to_string_lossy().into_owned());
            }
        }
    }

    if let Ok(existing) = std::env::var("PATH").or_else(|_| std::env::var("Path")) {
        segments.push(existing);
    }

    path_separator_join(&segments)
}

fn path_separator_join(segments: &[String]) -> String {
    #[cfg(windows)]
    let sep = ';';
    #[cfg(not(windows))]
    let sep = ':';
    segments.join(&sep.to_string())
}

fn dedupe_path(path: &str) -> String {
    #[cfg(windows)]
    let sep = ';';
    #[cfg(not(windows))]
    let sep = ':';

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in path.split(sep) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if seen.insert(part.to_string()) {
            out.push(part);
        }
    }
    out.join(&sep.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_keeps_first_occurrence() {
        let duplicate = path_separator_join(&[
            "/opt/homebrew/bin".to_string(),
            "/usr/bin".to_string(),
            "/opt/homebrew/bin".to_string(),
        ]);
        let expected = path_separator_join(&[
            "/opt/homebrew/bin".to_string(),
            "/usr/bin".to_string(),
        ]);
        assert_eq!(dedupe_path(&duplicate), expected);
    }

    #[test]
    fn fallback_includes_homebrew_on_macos() {
        let path = build_fallback_path();
        #[cfg(target_os = "macos")]
        assert!(path.contains("/opt/homebrew/bin"));
        assert!(path.contains("/usr/local/bin"));
    }
}
