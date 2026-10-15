//! Tauri commands of the first-run welcome (`crate::first_run`). The UI never chooses where the
//! first task runs or whether it may change files: `first_run_start_task` always asks for a
//! separate copy, read-only wherever the agent can enforce it.

use super::AppState;
use crate::first_run;
use crate::plain_mode::guard;
use serde_json::{json, Value};
use tauri::{AppHandle, State};

/// `{ show, auto, step }`: should the welcome be on screen, and where did the person leave it.
#[tauri::command]
pub async fn first_run_state(app: AppHandle) -> Result<Value, String> {
    Ok(first_run::state(&crate::settings_patch::current_settings(&app)))
}

/// Start the first task on `project` with `agent`. Returns `{ project, handle, pane_id, enforced }`.
/// Errors read `CODE: message` (`FOLDER_TOO_BROAD`, `FOLDER_NEEDS_GIT`, ...).
#[tauri::command]
pub async fn first_run_start_task(
    app: AppHandle,
    state: State<'_, AppState>,
    project: String,
    agent: String,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let enforced = crate::tui::profile_for(&agent).features().read_only_headless;
        let known = guard::KnownDirs::from_env();
        let plan = first_run::plan_task(&project, &agent, enforced, &known, crate::worktrees::folder::git_state)
            .map_err(|r| r.to_string())?;
        let run: crate::agent_runs::AgentRunRequest =
            serde_json::from_value(plan.request.clone()).map_err(|e| format!("INTERNAL: {e}"))?;
        let view = crate::agent_runs::run_agent(run, registry, app, None).map_err(|e| format!("{}: {}", e.code, e.message))?;
        crate::worktrees::folder::remember(&crate::app_paths::app_data_dir(), &plan.origin.to_string_lossy());
        Ok::<Value, String>(json!({
            "project": plan.origin.to_string_lossy(),
            "handle": view.handle,
            "pane_id": view.pane_id,
            "enforced": plan.enforced,
        }))
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}

/// Where the first task is: `{ status, result, error, receipt, pane_id }`. When the run is over and the
/// agent changed nothing, its copy is removed (a read-only task leaves nothing behind).
#[tauri::command]
pub async fn first_run_task_status(project: String, handle: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = crate::operations::latest_agent_run(&project, &handle).map_err(|e| format!("{}: {}", e.code, e.message))?;
        let view = serde_json::to_value(crate::agent_runs::AgentRunView::from(snapshot)).map_err(|e| format!("INTERNAL: {e}"))?;
        let out = first_run::summarize(&view);
        let status = out["status"].as_str().unwrap_or_default();
        if first_run::is_final(status) {
            if let Some(copy) = crate::worktrees::find_by_handle(&project, &handle) {
                crate::worktrees::discard_if_pristine(&copy);
            }
        }
        Ok::<Value, String>(out)
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}
