//! Tauri commands for Settings > Agents > "How agent runs work". Only the app's own webview can
//! call these; the bridge has no route for them, so a phone or an agent cannot change how an
//! agent runs.

use crate::agent_run_mode::{self, RunMode};
use serde_json::Value;

/// `{agents: [{id, label, headless_supported, headless_verified, headless_verified_reason, run_mode}]}`
/// for every custom (definition-based) agent.
#[tauri::command]
pub fn agent_run_mode_get() -> Value {
    agent_run_mode::settings_view()
}

/// Saves one agent's choice (`visible` | `one_task`). `one_task` is refused with
/// `RUN_MODE_NOT_VERIFIED` unless the agent's headless mode is verified. Returns the new view.
#[tauri::command]
pub fn agent_run_mode_set(id: String, mode: String) -> Result<Value, String> {
    let mode = RunMode::parse(&mode).ok_or_else(|| format!("INVALID_ARGUMENT: unknown run mode {mode}"))?;
    agent_run_mode::set_checked(&id, mode)?;
    Ok(agent_run_mode::settings_view())
}
