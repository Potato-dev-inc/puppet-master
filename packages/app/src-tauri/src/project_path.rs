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

/// Absolute, existing project directory with `.puppet-master` storage ready.
pub fn prepare_project_path(path: &Path) -> Result<PathBuf, String> {
    let expanded = normalize_project_path(path)?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|err| {
                format!(
                    "could not resolve relative project path {}: current directory unavailable: {err}",
                    expanded.display()
                )
            })?
            .join(expanded)
    };
    if absolute.exists() && !absolute.is_dir() {
        return Err(format!(
            "project path is not a directory: {}",
            absolute.display()
        ));
    }
    if !absolute.exists() {
        std::fs::create_dir_all(&absolute).map_err(|err| {
            format!(
                "could not create project directory {}: {err}",
                absolute.display()
            )
        })?;
    }
    let storage = crate::event_log::project_storage_dir(&absolute);
    std::fs::create_dir_all(&storage).map_err(|err| {
        format!(
            "could not create project storage at {}: {err}",
            storage.display()
        )
    })?;
    Ok(absolute)
}

pub fn comparable_project_path(path: &Path) -> String {
    let normalized = normalize_project_path(path).unwrap_or_else(|_| path.to_path_buf());
    let canon = std::fs::canonicalize(&normalized).unwrap_or(normalized);
    let text = canon.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

/// True when `supplied` is the worker directory or an ancestor of it.
pub fn workspace_covers(supplied: &Path, worker: &Path) -> bool {
    let supplied = comparable_project_path(supplied);
    let worker = comparable_project_path(worker);
    supplied == worker || worker.starts_with(&(supplied.clone() + "/"))
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
    fn prepare_errors_name_the_path_and_step() {
        let base = std::env::temp_dir().join(format!("pm-prep-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("blocker");
        std::fs::write(&file, "x").unwrap();
        let err = prepare_project_path(&file.join("child")).unwrap_err();
        assert!(err.contains("could not create project directory"), "{err}");
        assert!(err.contains("blocker"), "{err}");
        let err = prepare_project_path(&file).unwrap_err();
        assert!(err.contains("not a directory") && err.contains("blocker"), "{err}");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn prepare_accepts_trailing_separator_and_nested_missing_dirs() {
        let base = std::env::temp_dir().join(format!("pm-prep2-{}", uuid::Uuid::new_v4()));
        let nested = base.join("Roaming").join("scratch ws");
        let with_sep = format!("{}{}", nested.display(), std::path::MAIN_SEPARATOR);
        let out = prepare_project_path(Path::new(&with_sep)).unwrap();
        assert!(out.is_dir());
        let _ = std::fs::remove_dir_all(base);
    }

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

    #[test]
    fn prepare_project_path_creates_storage_dir() {
        let root = std::env::temp_dir().join(format!(
            "pm-prepare-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let prepared = prepare_project_path(&root).expect("absolute path should work");
        assert!(prepared.is_absolute());
        assert!(crate::event_log::project_storage_dir(&prepared).is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_covers_parent_and_rejects_unrelated() {
        let root = std::env::temp_dir().join(format!(
            "pm-workspace-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let child = root.join("packages").join("app");
        let sibling = root.join("other");
        let _ = std::fs::create_dir_all(&child);
        let _ = std::fs::create_dir_all(&sibling);
        assert!(workspace_covers(&root, &child));
        assert!(workspace_covers(&child, &child));
        assert!(!workspace_covers(&sibling, &child));
        let _ = std::fs::remove_dir_all(root);
    }
}
