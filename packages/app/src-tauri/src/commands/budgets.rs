//! Tauri commands for the header budget chip and its popover (`crate::budgets`).

use super::*;
use serde_json::Value;

/// The budget of the run bound to a pane, or `null` when the pane has no run or the run has no
/// budget (the default).
#[tauri::command]
pub async fn budget_for_pane(state: State<'_, AppState>, pane_id: String) -> Result<Option<Value>, String> {
    let Some((project, handle)) = crate::agent_runs::pane_api::run_of_pane(&state.registry, &pane_id) else {
        return Ok(None);
    };
    Ok(crate::budgets::view(&project, &handle))
}

/// "add_turns" | "add_minutes" | "stop" from the popover: the same step the Inbox card offers.
/// Returns the plain sentence to show.
#[tauri::command]
pub async fn budget_adjust(state: State<'_, AppState>, pane_id: String, action: String) -> Result<String, String> {
    let Some((project, handle)) = crate::agent_runs::pane_api::run_of_pane(&state.registry, &pane_id) else {
        return Err("BUDGET_NOT_FOUND: this pane has no budget".into());
    };
    crate::budgets::adjust_by_step(&project, &handle, &action).map_err(|e| format!("{}: {}", e.code, e.message))
}
