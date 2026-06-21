//! Resolve a writable project directory for PTY cwd and MCP install.

use std::path::{Path, PathBuf};

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Expand `~` and `~/…` so paths typed in settings resolve outside `src-tauri` during dev.
pub fn expand_user_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

/// GUI macOS apps launched from a DMG often have current_dir `/` — never use that as a project root.
pub fn is_valid_project_path(path: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    if path == Path::new("/") {
        return false;
    }
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if s.len() == 2 && s.ends_with(':') {
            return false;
        }
    }
    true
}

pub fn default_project_path() -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if is_valid_project_path(&cwd) {
            return cwd.to_string_lossy().to_string();
        }
    }
    home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".into())
}

pub fn normalize_project_path(path: &Path) -> Result<PathBuf, String> {
    let expanded = expand_user_path(path);
    if !is_valid_project_path(&expanded) {
        return Err(
            "Pick a project folder in the header before using Claude, Codex, or OpenCode orchestrators (cannot use /)"
                .into(),
        );
    }
    Ok(expanded)
}

/// PTY spawn cwd: explicit path when valid, otherwise registry default (also validated).
pub fn resolve_spawn_cwd(
    cwd: Option<String>,
    registry_fallback: String,
) -> Result<PathBuf, String> {
    let candidate = cwd
        .filter(|value| is_valid_project_path(Path::new(value)))
        .unwrap_or(registry_fallback);
    normalize_project_path(Path::new(&candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_root() {
        assert!(!is_valid_project_path(Path::new("/")));
    }

    #[test]
    fn default_is_not_root() {
        let path = default_project_path();
        assert!(is_valid_project_path(Path::new(&path)));
    }

    #[test]
    fn expand_tilde_project_path() {
        let Some(home) = home_dir() else {
            return;
        };
        let expanded = expand_user_path(Path::new("~/work/puppet-master"));
        assert_eq!(expanded, home.join("work/puppet-master"));
    }

    #[test]
    fn resolve_spawn_cwd_rejects_root_override() {
        let err = resolve_spawn_cwd(Some("/".into()), "/".into()).unwrap_err();
        assert!(err.contains("Claude, Codex, or OpenCode"));
    }
}
