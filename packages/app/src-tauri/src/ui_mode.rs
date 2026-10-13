//! Cockpit UI mode (global, persisted in `settings.json` as `settings.ui_mode`).
//!
//! * `orchestration` (default): every pane is shown in the single `cockpit` window.
//! * `individual`: every pane gets its own `detached-pane-<id>` pop-out window.
//!
//! Exactly one window owns a pane for a given mode, and only the owner may resize the PTY (two
//! viewers with different grids garble ConPTY output). The pure rules live here; the window
//! choreography is in `cockpit.rs`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

pub const COCKPIT_LABEL: &str = "cockpit";
/// The standalone worker host (`--worker`) is a single-terminal window that always owns its pane.
const WORKER_HOST_LABEL: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiMode {
    #[default]
    Orchestration,
    Individual,
}

impl UiMode {
    pub fn as_str(self) -> &'static str {
        match self {
            UiMode::Orchestration => "orchestration",
            UiMode::Individual => "individual",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "orchestration" => Some(UiMode::Orchestration),
            "individual" => Some(UiMode::Individual),
            _ => None,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> UiMode {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        UiMode::Individual
    } else {
        UiMode::Orchestration
    }
}

pub fn set_current(mode: UiMode) {
    CURRENT.store(u8::from(mode == UiMode::Individual), Ordering::Relaxed);
}

/// `settings.ui_mode` from the parsed `settings.json` (`{"settings": {...}}`); anything missing or
/// unknown reads as the default (orchestration).
pub fn mode_from_store_file(file: &Value) -> UiMode {
    file.get("settings")
        .and_then(|s| s.get("ui_mode"))
        .and_then(Value::as_str)
        .and_then(UiMode::parse)
        .unwrap_or_default()
}

/// The same file with `settings.ui_mode` set; every other key is kept.
pub fn store_file_with_mode(file: &Value, mode: UiMode) -> Value {
    let mut out = if file.is_object() { file.clone() } else { json!({}) };
    if !out.get("settings").is_some_and(Value::is_object) {
        out["settings"] = json!({});
    }
    out["settings"]["ui_mode"] = Value::String(mode.as_str().into());
    out
}

pub fn load_from_path(path: &Path) -> UiMode {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|file| mode_from_store_file(&file))
        .unwrap_or_default()
}

/// Read-modify-write of one key in the settings file (fallback when the store plugin is unusable).
pub fn save_to_path(path: &Path, mode: UiMode) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let next = store_file_with_mode(&existing.unwrap_or_else(|| json!({})), mode);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&next)?)
}

/// Label of the window that owns `pane_id` in `mode`.
pub fn owner_label(mode: UiMode, pane_id: &str) -> String {
    match mode {
        UiMode::Orchestration => COCKPIT_LABEL.to_string(),
        UiMode::Individual => crate::pane_windows::window_label(pane_id),
    }
}

/// May the window `caller` resize the PTY of `pane_id` in `mode`?
pub fn is_owner(mode: UiMode, caller: &str, pane_id: &str) -> bool {
    caller == WORKER_HOST_LABEL || caller == owner_label(mode, pane_id)
}

mod runtime {
    use super::*;
    use tauri::AppHandle;
    use tauri_plugin_store::StoreExt;

    const FILE: &str = "settings.json";

    fn settings_path() -> std::path::PathBuf {
        crate::app_paths::app_data_dir().join(FILE)
    }

    /// Load the saved mode into the process-wide current mode (startup).
    pub fn load_saved() -> UiMode {
        let mode = load_from_path(&settings_path());
        set_current(mode);
        mode
    }

    /// Persist the mode through the store plugin (so the frontend's `LazyStore` sees it), falling
    /// back to editing the file directly.
    pub fn persist(app: &AppHandle, mode: UiMode) -> Result<(), String> {
        let via_store = (|| -> Result<(), String> {
            let store = app.store(FILE).map_err(|e| e.to_string())?;
            let current = store.get("settings").unwrap_or_else(|| json!({}));
            let mut next = if current.is_object() { current } else { json!({}) };
            next["ui_mode"] = Value::String(mode.as_str().into());
            store.set("settings", next);
            store.save().map_err(|e| e.to_string())
        })();
        match via_store {
            Ok(()) => Ok(()),
            Err(store_err) => {
                tracing::warn!(error = %store_err, "ui_mode: store plugin save failed, writing the file directly");
                save_to_path(&settings_path(), mode).map_err(|e| format!("could not save ui_mode: {e}"))
            }
        }
    }
}

pub use runtime::{load_saved, persist};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_unknown_ui_mode_reads_as_orchestration() {
        assert_eq!(mode_from_store_file(&json!({})), UiMode::Orchestration);
        assert_eq!(mode_from_store_file(&json!({"settings": {}})), UiMode::Orchestration);
        assert_eq!(mode_from_store_file(&json!({"settings": {"ui_mode": "grid"}})), UiMode::Orchestration);
        assert_eq!(mode_from_store_file(&json!({"settings": {"ui_mode": 3}})), UiMode::Orchestration);
        assert_eq!(UiMode::default(), UiMode::Orchestration);
    }

    #[test]
    fn mode_round_trips_through_the_settings_file_and_keeps_other_keys() {
        let dir = std::env::temp_dir().join(format!("pm-ui-mode-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_from_path(&path), UiMode::Orchestration, "no file: default");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"settings":{"theme":"light"},"other":1}"#).unwrap();
        save_to_path(&path, UiMode::Individual).unwrap();
        assert_eq!(load_from_path(&path), UiMode::Individual);
        let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(file["settings"]["theme"], "light");
        assert_eq!(file["other"], 1);
        save_to_path(&path, UiMode::Orchestration).unwrap();
        assert_eq!(load_from_path(&path), UiMode::Orchestration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mode_serializes_lowercase() {
        assert_eq!(serde_json::to_value(UiMode::Individual).unwrap(), json!("individual"));
        assert_eq!(serde_json::from_value::<UiMode>(json!("orchestration")).unwrap(), UiMode::Orchestration);
        assert!(serde_json::from_value::<UiMode>(json!("grid")).is_err());
    }

    #[test]
    fn only_the_owner_for_the_mode_may_resize() {
        let pane = "aaaa-1111";
        let pop = crate::pane_windows::window_label(pane);
        // Orchestration: the cockpit owns every pane; a pop-out does not.
        assert!(is_owner(UiMode::Orchestration, "cockpit", pane));
        assert!(!is_owner(UiMode::Orchestration, &pop, pane));
        // Individual: only that pane's own pop-out.
        assert!(is_owner(UiMode::Individual, &pop, pane));
        assert!(!is_owner(UiMode::Individual, "cockpit", pane));
        assert!(!is_owner(UiMode::Individual, &crate::pane_windows::window_label("bbbb-2222"), pane));
        // Settings never owns; the --worker host always does.
        assert!(!is_owner(UiMode::Orchestration, "settings", pane));
        assert!(is_owner(UiMode::Individual, "main", pane));
    }
}
