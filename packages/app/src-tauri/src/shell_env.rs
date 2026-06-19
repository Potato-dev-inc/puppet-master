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
    std::env::set_var("PATH", path_for_spawn());
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

    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
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

    if let Ok(existing) = std::env::var("PATH") {
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
        let path = dedupe_path("/opt/homebrew/bin:/usr/bin:/opt/homebrew/bin");
        assert_eq!(path, "/opt/homebrew/bin:/usr/bin");
    }

    #[test]
    fn fallback_includes_homebrew_on_macos() {
        let path = build_fallback_path();
        #[cfg(target_os = "macos")]
        assert!(path.contains("/opt/homebrew/bin"));
        assert!(path.contains("/usr/local/bin"));
    }
}
