//! In-memory public settings mirror for the HTTP bridge (synced from desktop).

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;

/// The Labs switches a development build shows as on (see `dev_labs_override_active`). `pane_mcp_enabled` is not
/// one of them: it is on by default and lives in Settings › Agents.
pub const DEV_LABS_KEYS: &[&str] = &[
    "missions_enabled",
    "night_shift_enabled",
    "potato_bot_enabled",
    "potato_sandbox_enabled",
    "acp_chat_enabled",
];

/// Tests turn the override on through this; it is off in every test unless a test sets it.
#[cfg(test)]
pub(crate) static TEST_DEV_LABS: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
fn debug_build_allows_override() -> bool {
    TEST_DEV_LABS.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn debug_build_allows_override() -> bool {
    true
}

/// The rule for the development override, in one place: a debug build, not switched off by `PM_DEV_LABS=0`,
/// and allowed by the build (always, except under `cargo test`, where a test must turn it on).
pub fn dev_labs_gate(debug_build: bool, env: Option<&str>, build_allows: bool) -> bool {
    debug_build && env != Some("0") && build_allows
}

/// Whether every Labs switch reads as on: a debug build (`npm run dev`, the e2e build) unless `PM_DEV_LABS=0`.
/// A release build is never affected. Applied at read time only: `settings.json` is never written with it.
pub fn dev_labs_override_active() -> bool {
    dev_labs_gate(cfg!(debug_assertions), std::env::var("PM_DEV_LABS").ok().as_deref(), debug_build_allows_override())
}

/// The settings the app acts on: `stored` with every Labs switch on when `active`. Pure; `stored` is not changed.
pub fn with_dev_labs(stored: &Value, active: bool) -> Value {
    let mut out = stored.clone();
    if active {
        if let Some(obj) = out.as_object_mut() {
            for key in DEV_LABS_KEYS {
                obj.insert((*key).to_string(), Value::Bool(true));
            }
        }
    }
    out
}

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
    "confirm_close_running_pane",
    "checkpoints_enabled",
    "checkpoints_keep",
    "away_minutes",
    "budget_default_turns",
    "budget_default_minutes",
    "agent_hooks_mode",
    "agent_hooks_agents",
    "pane_mcp_enabled",
    "missions_enabled",
    "night_shift_enabled",
    "potato_bot_enabled",
    "potato_sandbox_enabled",
    "acp_chat_enabled",
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
        "confirm_close_running_pane": true,
        "checkpoints_enabled": true,
        "checkpoints_keep": 50,
        "away_minutes": 5,
        // Budgets: 0 = off, which is today's behaviour.
        "budget_default_turns": 0,
        "budget_default_minutes": 0,
        // Hook Lane (founder decision 2026-10-14): watch-only hooks for agents that declare them.
        "agent_hooks_mode": "shadow",
        "agent_hooks_agents": {},
        // Puppet Master's helper tools ready inside every agent pane (on unless the person turns it off).
        "pane_mcp_enabled": true,
        // Missions (Preview): off until the person turns them on in Settings › General › Labs.
        "missions_enabled": false,
        // Night Shift (Labs): hours of unattended work, so off until the person turns it on.
        "night_shift_enabled": false,
        // Potato Bot (Labs): a chat friend with its own brain that costs money per message, so off until chosen.
        // It is also the kill switch: off stops every brain (`potato::live::apply_settings`).
        "potato_bot_enabled": false,
        // Potato Bot sandbox (Labs, needs Potato Bot): a small Linux computer the bot can use. Off until chosen.
        "potato_sandbox_enabled": false,
        // Chat over the Agent Client Protocol (Labs): agents driven through a protocol instead of a terminal
        // screen. Off until chosen; it is also the kill switch (off closes every ACP session). Rust: acp.
        "acp_chat_enabled": false,
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

/// The public settings as stored (the mirror), without the development override. Only the code that saves or
/// mirrors settings reads this; everything that acts on a setting reads `read_public_settings`.
fn stored_public_settings(app: &AppHandle) -> Value {
    app.try_state::<AppState>()
        .map(|state| state.public_settings.lock().clone())
        .unwrap_or_else(default_public_settings)
}

/// The effective public settings: the stored mirror with the development Labs override applied (if active).
pub fn read_public_settings(app: &AppHandle) -> Value {
    with_dev_labs(&stored_public_settings(app), dev_labs_override_active())
}

/// Tells the bridge SSE and each Labs owner the settings. Always called with the effective settings, so the
/// override reaches every subsystem.
fn apply_public_settings(effective: &Value) {
    crate::bridge::push_settings_sse(effective);
    crate::agent_hooks::apply_settings(effective);
    crate::pane_mcp::apply_settings(effective);
    crate::potato::live::apply_settings(effective);
    crate::acp::apply_settings(effective);
}

/// Merge `patch` into the public mirror (bridge state + mobile SSE) without any window event.
/// `settings_patch` uses this and emits its own `settings://changed`. Returns the effective settings.
pub fn mirror_public_patch(app: &AppHandle, patch: &Value) -> Value {
    let current = stored_public_settings(app);
    let merged = merge_public_settings(&current, patch);
    if let Some(state) = app.try_state::<AppState>() {
        *state.public_settings.lock() = merged.clone();
    }
    let effective = with_dev_labs(&merged, dev_labs_override_active());
    apply_public_settings(&effective);

    if let Some(backend) = effective.get("orchestrator_backend").and_then(Value::as_str) {
        if backend != "api" {
            let _ = app.emit("orchestrator://ensure", json!({ "backend": backend }));
        }
    }

    effective
}

pub fn patch_public_settings(app: &AppHandle, patch: Value) -> Value {
    let merged = mirror_public_patch(app, &patch);
    let _ = app.emit("settings://apply", patch);
    let _ = app.emit("settings://changed", merged.clone());
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
    // The search index's switch is not in the public mirror (desktop-only setting): read it from the file.
    crate::chat_index::apply_setting(file.get("settings").unwrap_or(&Value::Null));
    apply_public_settings(&with_dev_labs(&merged, dev_labs_override_active()));
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

    #[test]
    fn night_shift_is_off_by_default_and_the_person_can_turn_it_on() {
        let defaults = default_public_settings();
        assert_eq!(defaults["night_shift_enabled"], false);
        assert_eq!(defaults["missions_enabled"], false);
        let on = merge_public_settings(&defaults, &json!({"night_shift_enabled": true}));
        assert_eq!(on["night_shift_enabled"], true);
        // The switch is its own: the missions switch does not turn it on.
        let missions_only = merge_public_settings(&defaults, &json!({"missions_enabled": true}));
        assert_eq!(missions_only["night_shift_enabled"], false);
        let from_file = public_settings_from_store_file(&json!({"settings": {"night_shift_enabled": true}}));
        assert_eq!(from_file["night_shift_enabled"], true);
    }

    #[test]
    fn dev_override_turns_every_labs_switch_on_and_nothing_else() {
        let stored = default_public_settings();
        let effective = with_dev_labs(&stored, true);
        for key in DEV_LABS_KEYS {
            assert_eq!(effective[*key], true, "{key} should read as on");
        }
        // Not a Labs switch: unchanged (on by default, Settings › Agents).
        assert_eq!(effective["pane_mcp_enabled"], true);
        assert_eq!(effective["checkpoints_enabled"], stored["checkpoints_enabled"]);
        assert_eq!(effective["default_model"], stored["default_model"]);
        // Inactive: the stored value passes through untouched.
        assert_eq!(with_dev_labs(&stored, false), stored);
        assert_eq!(with_dev_labs(&stored, false)["potato_bot_enabled"], false);
    }

    #[test]
    fn dev_override_never_changes_the_stored_file() {
        let file = json!({"settings": {"potato_bot_enabled": false, "missions_enabled": false}});
        let before = file.clone();
        let stored = public_settings_from_store_file(&file);
        let effective = with_dev_labs(&stored, true);
        assert_eq!(effective["potato_bot_enabled"], true);
        assert_eq!(effective["missions_enabled"], true);
        // The mirror (what is saved) and the file are as they were.
        assert_eq!(stored["potato_bot_enabled"], false);
        assert_eq!(stored["missions_enabled"], false);
        assert_eq!(file, before);
    }

    #[test]
    fn dev_gate_is_on_only_in_a_debug_build_unless_pm_dev_labs_is_zero() {
        // A debug build with no switch-off: on.
        assert!(dev_labs_gate(true, None, true));
        assert!(dev_labs_gate(true, Some("1"), true));
        assert!(dev_labs_gate(true, Some(""), true));
        // PM_DEV_LABS=0 turns it off.
        assert!(!dev_labs_gate(true, Some("0"), true));
        // A release build is never affected, whatever the variable says.
        assert!(!dev_labs_gate(false, None, true));
        assert!(!dev_labs_gate(false, Some("1"), true));
        // Under cargo test the build itself must allow it (tests are off by default).
        assert!(!dev_labs_gate(true, None, false));
    }

    #[test]
    fn dev_override_is_off_in_tests_until_a_test_turns_it_on() {
        assert!(!dev_labs_override_active(), "tests start with every Labs flag as stored");
        TEST_DEV_LABS.store(true, Ordering::SeqCst);
        let on = dev_labs_override_active();
        TEST_DEV_LABS.store(false, Ordering::SeqCst);
        assert_eq!(on, std::env::var("PM_DEV_LABS").ok().as_deref() != Some("0"));
        assert!(!dev_labs_override_active());
    }
}
