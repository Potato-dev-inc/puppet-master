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

/// Why `prepare_project_path_coded` refused a folder.
#[derive(Debug)]
pub enum PrepareError {
    /// The folder's path is longer than Windows can start a program in (`path_limit`).
    TooLong(crate::path_limit::PathTooLong, PathBuf),
    /// Anything else: not a folder, cannot be created, no storage.
    Invalid(String),
}

impl PrepareError {
    pub fn message(&self) -> String {
        match self {
            Self::TooLong(found, _) => found.message(),
            Self::Invalid(text) => text.clone(),
        }
    }

    /// The MCP/bridge error: `PATH_TOO_LONG` or `INVALID_PROJECT_PATH`.
    pub fn into_operation_error(self) -> crate::operations::OperationError {
        match self {
            Self::TooLong(found, folder) => found.into_error(&folder),
            Self::Invalid(text) => crate::operations::OperationError::new("INVALID_PROJECT_PATH", text, false),
        }
    }
}

/// Absolute, existing project directory with `.puppet-master` storage ready.
pub fn prepare_project_path(path: &Path) -> Result<PathBuf, String> {
    prepare_project_path_coded(path).map_err(|error| error.message())
}

/// `prepare_project_path` that keeps the reason apart, so a caller can answer with the right code.
/// A folder longer than Windows can start a program in is refused before anything is created.
pub fn prepare_project_path_coded(path: &Path) -> Result<PathBuf, PrepareError> {
    let expanded = normalize_project_path(path).map_err(PrepareError::Invalid)?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|err| {
                PrepareError::Invalid(format!(
                    "could not resolve relative project path {}: current directory unavailable: {err}",
                    expanded.display()
                ))
            })?
            .join(expanded)
    };
    crate::path_limit::check(&absolute).map_err(|found| PrepareError::TooLong(found, absolute.clone()))?;
    if absolute.exists() && !absolute.is_dir() {
        return Err(PrepareError::Invalid(format!(
            "project path is not a directory: {}",
            absolute.display()
        )));
    }
    if !absolute.exists() {
        std::fs::create_dir_all(&absolute).map_err(|err| {
            PrepareError::Invalid(format!(
                "could not create project directory {}: {err}",
                absolute.display()
            ))
        })?;
    }
    let storage = crate::event_log::project_storage_dir(&absolute);
    std::fs::create_dir_all(&storage).map_err(|err| {
        PrepareError::Invalid(format!(
            "could not create project storage at {}: {err}",
            storage.display()
        ))
    })?;
    Ok(absolute)
}

pub fn comparable_project_path(path: &Path) -> String {
    let normalized = normalize_project_path(path).unwrap_or_else(|_| path.to_path_buf());
    let canon = std::fs::canonicalize(&normalized).unwrap_or(normalized);
    let text = canon.to_string_lossy().replace('\\', "/");
    // `\\?\C:\x` (extended-length) and `\\?\UNC\host\share` name the same places as `C:\x` and
    // `\\host\share`; a trailing separator names the same directory as none.
    let text = if let Some(rest) = text.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = text.strip_prefix("//?/").or_else(|| text.strip_prefix("//./")) {
        rest.to_string()
    } else {
        text
    };
    let trimmed = text.trim_end_matches('/');
    let text = if trimmed.is_empty() || trimmed.ends_with(':') {
        text // a filesystem root keeps its separator
    } else {
        trimmed.to_string()
    };
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
    let normalized = normalize_project_path(Path::new(&candidate))?;
    crate::path_limit::check(&normalized).map_err(|found| found.message())?;
    Ok(normalized)
}


/// Make `raw` the active project folder AND save it as the `project_path` setting.
///
/// The registry (what new panes use) and `settings.json` (what Settings, the "+" menu and the
/// next launch read) used to be written by two different callers; the bridge route only did the
/// first, so `POST /project-path` answered ok and the setting never changed. `persist` is the
/// atomic settings patch (it also tells every window with `settings://changed`).
pub fn set_active_and_persist(
    registry: &parking_lot::Mutex<crate::pty::PaneRegistry>,
    raw: &str,
    persist: impl FnOnce(&serde_json::Value) -> Result<serde_json::Value, String>,
) -> Result<PathBuf, (u16, String)> {
    let normalized = normalize_project_path(Path::new(raw)).map_err(|e| (400, e))?;
    // A relative or drive-relative path (`reports`, `C:Users`) would be resolved against whatever
    // the app's working directory is: runs would create folders there (inside the source tree in
    // a dev build) and the bad value would be saved and loaded again at every start.
    if !normalized.is_absolute() {
        return Err((
            400,
            format!("The project folder must be a full path (like C:/work/app), not `{}`", raw.trim()),
        ));
    }
    let text = normalized.to_string_lossy().into_owned();
    crate::pty::registry_set_project_path(registry, text.clone());
    crate::event_log::set_active_project_path(Some(normalized.clone()));
    persist(&serde_json::json!({ "project_path": text }))
        .map_err(|e| (500, format!("The folder is active but could not be saved: {e}")))?;
    Ok(normalized)
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
    fn setting_the_project_folder_refuses_a_path_that_is_not_absolute() {
        let registry = parking_lot::Mutex::new(crate::pty::PaneRegistry::default());
        for raw in ["relative/dir", "reports", "C:Users", "./x"] {
            let mut saved = false;
            let result = set_active_and_persist(&registry, raw, |_| {
                saved = true;
                Ok(serde_json::json!({}))
            });
            let (status, message) = result.expect_err(raw);
            assert_eq!(status, 400, "{raw}");
            assert!(message.contains("full path"), "{raw}: {message}");
            assert!(!saved, "{raw} must not be saved");
        }
    }

    fn long_folder(under: &Path, len: usize) -> PathBuf {
        let mut text = under.to_string_lossy().into_owned();
        while text.len() < len {
            text.push_str("\\segment-of-the-long-path");
        }
        text.truncate(len);
        if text.ends_with('\\') {
            text.pop();
            text.push('x');
        }
        PathBuf::from(text)
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_too_long_for_windows_is_refused_before_anything_is_created() {
        let base = std::env::temp_dir().join(format!("pm-long-{}", uuid::Uuid::new_v4().simple()));
        let long = long_folder(&base, 283);
        assert_eq!(long.to_string_lossy().len(), 283);
        let error = prepare_project_path_coded(&long).unwrap_err();
        assert!(matches!(error, PrepareError::TooLong(..)), "{error:?}");
        let coded = error.into_operation_error();
        assert_eq!(coded.code, "PATH_TOO_LONG");
        assert!(coded.message.starts_with("This folder's path is 283 characters."), "{}", coded.message);
        // The String flavour says the same thing, and nothing was made on disk.
        let text = prepare_project_path(&long).unwrap_err();
        assert!(text.contains("283 characters") && text.contains("259"), "{text}");
        assert!(!base.exists(), "a refused folder must not be created");
        // A spawn cwd is judged by the same rule.
        let spawn = resolve_spawn_cwd(Some(long.to_string_lossy().into_owned()), "C:/".into()).unwrap_err();
        assert!(spawn.contains("283 characters"), "{spawn}");
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_at_the_limit_still_prepares() {
        let base = std::env::temp_dir().join(format!("pm-edge-{}", uuid::Uuid::new_v4().simple()));
        let edge = long_folder(&base, 259);
        match prepare_project_path_coded(&edge) {
            Ok(path) => assert!(path.is_dir()),
            // The temp folder itself may already be deep on a build machine.
            Err(PrepareError::TooLong(found, _)) => panic!("259 characters is allowed, got {found:?}"),
            Err(other) => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&base);
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

#[cfg(test)]
mod persist_tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn set_active_and_persist_writes_the_setting_and_tells_windows() {
        let dir = std::env::temp_dir().join(format!("pm-projpath-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(&file, r#"{"settings":{"project_path":"C:/old","theme":"dark"}}"#).unwrap();
        let project = dir.join("repo");
        std::fs::create_dir_all(&project).unwrap();

        let registry = parking_lot::Mutex::new(crate::pty::PaneRegistry::new());
        let events: std::cell::RefCell<Vec<(String, Value)>> = Default::default();
        let out = set_active_and_persist(&registry, &project.to_string_lossy(), |patch| {
            crate::settings_patch::apply_core(
                patch,
                false,
                |clean| crate::settings_patch::patch_file_locked(&file, clean),
                |_| {},
                |name, payload| events.borrow_mut().push((name.to_string(), payload)),
            )
        })
        .expect("persisted");

        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["settings"]["project_path"], json!(out.to_string_lossy()), "settings.json changed");
        assert_eq!(saved["settings"]["theme"], "dark", "other settings are untouched");
        assert_eq!(registry.lock().project_path, out.to_string_lossy());
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "settings://changed");
        assert_eq!(events[0].1["project_path"], json!(out.to_string_lossy()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_path_is_a_400_and_a_failed_save_is_reported() {
        let registry = parking_lot::Mutex::new(crate::pty::PaneRegistry::new());
        let before = registry.lock().project_path.clone();
        let err = set_active_and_persist(&registry, "/", |_| panic!("must not persist")).unwrap_err();
        assert_eq!(err.0, 400);
        assert_eq!(registry.lock().project_path, before);
        let err = set_active_and_persist(&registry, std::env::temp_dir().to_str().unwrap(), |_| Err("disk full".into())).unwrap_err();
        assert_eq!(err.0, 500);
        assert!(err.1.contains("could not be saved") && err.1.contains("disk full"), "{}", err.1);
    }
}
