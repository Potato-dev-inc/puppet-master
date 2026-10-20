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
    scrub_host_session_env();
}

/// Variables a launching Claude Code session exports to its children. A pane or headless `claude`
/// that inherits them believes it is a nested child session: it turns transcript saving off, shows
/// a warning row instead of its input hint and repaints the prompt box, which breaks screen-based
/// prompt delivery and completion capture. Auth and config variables are deliberately not listed.
const HOST_SESSION_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_DESKTOP_APP_VERSION",
    "CLAUDE_CODE_TERMINAL_MCP_TOOLS",
    "CLAUDE_AGENT_SDK_VERSION",
    "CLAUDE_PID",
];

/// Remove [`HOST_SESSION_ENV`] from this process so every child (panes, headless runs, opencode
/// serve) starts as a top-level session no matter what launched the app.
pub fn scrub_host_session_env() {
    for name in HOST_SESSION_ENV {
        std::env::remove_var(name);
    }
}

#[cfg(test)]
mod scrub_tests {
    use super::*;

    #[test]
    fn host_session_markers_are_removed_and_auth_is_kept() {
        std::env::set_var("CLAUDE_CODE_CHILD_SESSION", "1");
        std::env::set_var("CLAUDECODE", "1");
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "keep-me");
        scrub_host_session_env();
        assert!(std::env::var("CLAUDE_CODE_CHILD_SESSION").is_err());
        assert!(std::env::var("CLAUDECODE").is_err());
        assert_eq!(std::env::var("CLAUDE_CODE_OAUTH_TOKEN").as_deref(), Ok("keep-me"));
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
    }
}

/// Resolve the OpenCode binary for `std::process::Command` / PTY spawn.
///
/// On Windows, npm installs `opencode` as a `.ps1` shim which `CreateProcess`
/// cannot execute directly — use the real `opencode.exe` under the npm package.
pub fn resolve_opencode_executable() -> Result<String, String> {
    resolve_opencode_in(std::env::var_os("APPDATA").as_deref(), path_for_spawn())
}

/// [`resolve_opencode_executable`] with the npm roaming dir (`%APPDATA%`) and the `PATH` string
/// passed in, so a test can point both at a temp dir.
fn resolve_opencode_in(appdata: Option<&std::ffi::OsStr>, path: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        if let Some(appdata) = appdata {
            let candidate = PathBuf::from(appdata)
                .join("npm")
                .join("node_modules")
                .join("opencode-ai")
                .join("bin")
                .join("opencode.exe");
            if candidate.is_file() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
        if let Some(path) = which_on_path("opencode.exe", path) {
            return Ok(path);
        }
        return Err(
            "opencode.exe not found (install: npm i -g opencode-ai, or ensure npm global bin is on PATH)"
                .into(),
        );
    }
    #[cfg(not(windows))]
    {
        let _ = appdata;
        if which_on_path("opencode", path).is_some() {
            return Ok("opencode".into());
        }
        Err("opencode not found on PATH (install: npm i -g opencode-ai)".into())
    }
}

fn which_on_path(name: &str, path: &str) -> Option<String> {
    #[cfg(windows)]
    let sep = ';';
    #[cfg(not(windows))]
    let sep = ':';

    for dir in path.split(sep) {
        let candidate = PathBuf::from(dir.trim()).join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
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
    let output = crate::proc_util::hidden_command(&shell)
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
    let output = crate::proc_util::hidden_command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$u=[Environment]::GetEnvironmentVariable('Path','User');$m=[Environment]::GetEnvironmentVariable('Path','Machine');if($u-and$m){\"$u;$m\"}elseif($u){$u}else{$m}",
        ])
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
        let expected =
            path_separator_join(&["/opt/homebrew/bin".to_string(), "/usr/bin".to_string()]);
        assert_eq!(dedupe_path(&duplicate), expected);
    }

    #[test]
    fn fallback_includes_homebrew_on_macos() {
        let path = build_fallback_path();
        #[cfg(target_os = "macos")]
        assert!(path.contains("/opt/homebrew/bin"));
        assert!(path.contains("/usr/local/bin"));
    }

    /// A throw-away directory that stands in for `%APPDATA%` / a `PATH` entry.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-opencode-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opencode_is_found_in_the_npm_package_dir_then_on_path_and_else_is_an_error() {
        let exe = if cfg!(windows) { "opencode.exe" } else { "opencode" };
        let on_path = temp_dir("path");
        let empty = temp_dir("empty");
        let path = on_path.to_string_lossy().into_owned();

        // Nothing installed anywhere: a clear error that names the fix.
        let missing = resolve_opencode_in(Some(empty.as_os_str()), &empty.to_string_lossy());
        assert!(missing.unwrap_err().contains("npm i -g opencode-ai"));

        // On PATH.
        std::fs::write(on_path.join(exe), "stub").unwrap();
        let found = resolve_opencode_in(Some(empty.as_os_str()), &path).unwrap();
        #[cfg(windows)]
        assert!(found.ends_with("opencode.exe"), "expected opencode.exe, got {found:?}");
        #[cfg(not(windows))]
        assert_eq!(found, "opencode");

        // Windows: the npm package dir under %APPDATA% wins over PATH.
        #[cfg(windows)]
        {
            let bin = empty.join("npm").join("node_modules").join("opencode-ai").join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("opencode.exe"), "stub").unwrap();
            let found = resolve_opencode_in(Some(empty.as_os_str()), "").unwrap();
            assert!(found.starts_with(&*empty.to_string_lossy()), "{found}");
        }
        let _ = (std::fs::remove_dir_all(on_path), std::fs::remove_dir_all(empty));
    }
}
