//! Tauri commands for the agent doctor's Install button (see `crate::agent_install`). Tauri
//! commands are the webview's own channel: no bridge route or MCP tool reaches these.

use super::*;

/// The exact commands Install would run for `agent_id`, in order, with where each comes from.
/// Runs nothing.
#[tauri::command(async)]
pub fn agents_install_plan(agent_id: String) -> Result<Value, String> {
    crate::agent_install::plan(&agent_id)
}

/// Runs one step the person confirmed, in a visible PowerShell pane. `shown_command` must equal
/// the command text of that step in a freshly built plan.
#[tauri::command(async)]
pub fn agents_install_run(
    agent_id: String,
    step: usize,
    shown_command: String,
    acknowledged_risk: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    crate::agent_install::start(&app, &state.registry, &agent_id, step, &shown_command, acknowledged_risk)
}

/// Has the install pane's command finished, and with which exit code?
#[tauri::command]
pub fn agents_install_status(pane_id: String, state: State<'_, AppState>) -> Value {
    crate::agent_install::status(&state.registry, &pane_id)
}

/// After an install: refresh PATH for this process and check the agent again.
#[tauri::command(async)]
pub fn agents_install_finish(agent_id: String) -> Result<Value, String> {
    crate::agent_install::finish(&agent_id)
}
