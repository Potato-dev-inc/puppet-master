//! In-memory public settings mirror for the HTTP bridge (synced from desktop).

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;

const PATCHABLE_KEYS: &[&str] = &[
    "orchestrator_backend",
    "default_provider",
    "default_model",
    "mobile_input_delay_ms",
    "mobile_input_visible",
    "developer_use_rust_mcp",
];

pub fn default_public_settings() -> Value {
    json!({
        "orchestrator_backend": "api",
        "default_provider": "anthropic",
        "default_model": "claude-sonnet-4-6",
        "mobile_input_delay_ms": 250,
        "mobile_input_visible": true,
        "developer_use_rust_mcp": false,
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
