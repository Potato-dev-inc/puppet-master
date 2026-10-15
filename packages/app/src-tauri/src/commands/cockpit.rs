//! Tauri commands for the desktop cockpit (see `docs/design/cockpit-contract.md`). Errors are
//! plain strings; a missing run is `null`, not an error.

use super::*;
use crate::pane_prompt::PanePrompt;
use crate::ui_mode::UiMode;

#[tauri::command]
pub async fn get_ui_mode() -> Result<UiMode, String> {
    Ok(crate::ui_mode::current())
}

#[tauri::command]
pub async fn set_ui_mode(app: AppHandle, mode: UiMode) -> Result<(), String> {
    crate::cockpit::set_mode(&app, mode)
}

#[tauri::command]
pub async fn open_cockpit(app: AppHandle, pane_id: Option<String>) -> Result<(), String> {
    crate::cockpit::open_cockpit(&app, pane_id.as_deref());
    Ok(())
}

#[tauri::command]
pub async fn pane_prompt(state: State<'_, AppState>, pane_id: String) -> Result<Option<PanePrompt>, String> {
    Ok(crate::pane_prompt::detect_for_ui(&state.registry, &pane_id))
}

/// Same validation as `POST /agents/answer` for a bare pane (stale and replay guards). Never opts
/// into broad approvals.
#[tauri::command]
pub async fn answer_pane_prompt(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    prompt_id: String,
    choice: String,
) -> Result<(), String> {
    crate::agent_runs::answer_for_pane(&state.registry, &app, &pane_id, &prompt_id, &choice)
        .map(|_| crate::attention::note_desktop_answer(&pane_id, &choice))
        .map_err(|e| format!("{}: {}", e.code, e.message))
}

#[tauri::command]
pub async fn interrupt_pane(app: AppHandle, state: State<'_, AppState>, pane_id: String) -> Result<(), String> {
    registry_write_input(&state.registry, &app, &pane_id, "\u{3}", false, false, None, crate::pty::WriteOrigin::User)
}

#[tauri::command]
pub async fn new_pane(
    app: AppHandle,
    agent_id: String,
    account_id: Option<String>,
    isolation: Option<String>,
    project_path: Option<String>,
) -> Result<String, String> {
    let isolated = match isolation.as_deref() {
        None | Some("") | Some("none") => false,
        Some("worktree") => true,
        Some(other) => return Err(format!("INVALID_ARGUMENT: isolation must be \"none\" or \"worktree\", got {other:?}")),
    };
    let started = crate::tray::new_pane_in(
        &app,
        &agent_id,
        crate::cockpit::Origin::CockpitUi,
        account_id.as_deref(),
        isolated,
        project_path.as_deref(),
    );
    if started.is_ok() {
        if let Some(project) = project_path.as_deref().filter(|p| !p.trim().is_empty()) {
            crate::worktrees::folder::remember(&crate::app_paths::app_data_dir(), project);
        }
    }
    started
}

#[tauri::command]
pub async fn pane_run_info(state: State<'_, AppState>, pane_id: String) -> Result<Option<Value>, String> {
    crate::agent_runs::pane_api::pane_run_info(&state.registry, &pane_id)
}

#[tauri::command]
pub async fn pane_transcript(state: State<'_, AppState>, pane_id: String) -> Result<Option<Value>, String> {
    crate::agent_runs::pane_api::pane_transcript(&state.registry, &pane_id)
}

/// Show Settings, optionally on a tab: inside the cockpit in Orchestration mode, the Settings window (shown
/// or created) in Individual mode.
#[tauri::command]
pub async fn open_settings(app: AppHandle, tab: Option<String>) -> Result<(), String> {
    crate::cockpit::open_settings(&app, tab.as_deref());
    Ok(())
}
