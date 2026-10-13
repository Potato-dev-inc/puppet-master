//! In-memory public settings mirror for the HTTP bridge (synced from desktop).

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;

const PATCHABLE_KEYS: &[&str] = &[
    "orchestrator_backend",
    "default_provider",
    "default_model",
    "opencode_model_provider",
    "opencode_model_id",
    "mobile_input_delay_ms",
    "mobile_input_visible",
    "mobile_keyboard_resizes_agent",
    "developer_use_rust_mcp",
    "librarian_indexer_path",
    "checkpoints_enabled",
    "checkpoints_keep",
];

pub fn default_public_settings() -> Value {
    json!({
        "orchestrator_backend": "api",
        "default_provider": "anthropic",
        "default_model": "claude-sonnet-4-6",
        "opencode_model_provider": null,
        "opencode_model_id": null,
        "mobile_input_delay_ms": 60,
        "mobile_input_visible": true,
        "mobile_keyboard_resizes_agent": false,
        "developer_use_rust_mcp": false,
        "librarian_indexer_path": null,
        "checkpoints_enabled": true,
        "checkpoints_keep": 50,
    })
}

pub fn merge_public_settings(base: &Value, patch: &Value) -> Value {
    let mut merged = default_public_settings();
    if let Some(obj) = base.as_object() {
        if let Some(target) = merged.as_object_mut() {
            for (key, value) in obj {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(obj) = patch.as_object() {
        if let Some(target) = merged.as_object_mut() {
            for key in PATCHABLE_KEYS {
                if let Some(value) = obj.get(*key) {
                    target.insert((*key).to_string(), value.clone());
                }
            }
        }
    }
    merged
}

pub fn read_public_settings(app: &AppHandle) -> Value {
    app.try_state::<AppState>()
        .map(|state| state.public_settings.lock().clone())
        .unwrap_or_else(default_public_settings)
}

pub fn patch_public_settings(app: &AppHandle, patch: Value) -> Value {
    let current = read_public_settings(app);
    let merged = merge_public_settings(&current, &patch);
    if let Some(state) = app.try_state::<AppState>() {
        *state.public_settings.lock() = merged.clone();
    }

    let _ = app.emit("settings://apply", patch);
    let _ = app.emit("settings://changed", merged.clone());
    crate::bridge::push_settings_sse(&merged);

    if let Some(backend) = merged.get("orchestrator_backend").and_then(Value::as_str) {
        if backend != "api" {
            let _ = app.emit("orchestrator://ensure", json!({ "backend": backend }));
        }
    }

    merged
}

/// Public settings as saved in `settings.json` (`{"settings": {...}}`, written by the frontend
/// store plugin). Panes-first has no main window to push them at startup, so Rust loads them.
pub fn public_settings_from_store_file(file: &Value) -> Value {
    let patch = file.get("settings").cloned().unwrap_or(Value::Null);
    let cleaned: serde_json::Map<String, Value> = patch
        .as_object()
        .map(|o| o.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    merge_public_settings(&default_public_settings(), &Value::Object(cleaned))
}

/// Load the saved public settings into `AppState` (no-op when the file is missing or unreadable).
pub fn load_saved_public_settings(app: &AppHandle) {
    let path = crate::app_paths::app_data_dir().join("settings.json");
    let Some(file) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        return;
    };
    let merged = public_settings_from_store_file(&file);
    if let Some(state) = app.try_state::<AppState>() {
        *state.public_settings.lock() = merged.clone();
    }
    crate::bridge::push_settings_sse(&merged);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_settings_override_defaults_and_ignore_unknown_or_null_keys() {
        let file = json!({"settings": {
            "mobile_input_delay_ms": 400, "default_model": "m1", "opencode_model_id": null,
            "api_key": "secret", "pane_window_position": {"x": 1, "y": 2}
        }});
        let out = public_settings_from_store_file(&file);
        assert_eq!(out["mobile_input_delay_ms"], 400);
        assert_eq!(out["default_model"], "m1");
        assert_eq!(out["default_provider"], "anthropic");
        assert_eq!(out["mobile_keyboard_resizes_agent"], false);
        assert!(out.get("api_key").is_none() && out.get("pane_window_position").is_none());
        assert_eq!(public_settings_from_store_file(&json!({})), default_public_settings());
    }
}
