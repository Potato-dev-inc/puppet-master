//! Tauri commands for the global Ask bar. Rules and wiring live in `crate::askbar`.

use super::*;
use crate::askbar::{self, glue, AgentEntry, AskRequest, AskResult, Clip, Notice, Settings};

#[tauri::command]
pub fn askbar_settings_get() -> Settings {
    askbar::settings()
}

/// Save the settings, then register or drop the shortcut and create or hide the window.
#[tauri::command]
pub fn askbar_settings_set(app: AppHandle, settings: Settings) -> Settings {
    let applied = askbar::set_settings(settings);
    glue::apply(&app, &applied);
    applied
}

/// Agents that can answer a headless read-only question and are installed.
#[tauri::command]
pub fn askbar_agents() -> Vec<AgentEntry> {
    let labels: std::collections::HashMap<&str, String> =
        crate::tui::descriptors().into_iter().map(|d| (d.id, d.label)).collect();
    let capable: Vec<(String, String)> = crate::tui::read_only_backends()
        .into_iter()
        .filter(|id| crate::tui::is_agent_backend(id))
        .map(|id| (id.to_string(), labels.get(id).cloned().unwrap_or_else(|| id.to_string())))
        .collect();
    askbar::list_agents(&capable, &|id| crate::tui::profile_for(id).is_installed(), &crate::standby::ready_agents())
}

/// The clipboard text, cut for attaching (`null` when empty or not text).
#[tauri::command]
pub async fn askbar_clipboard() -> Option<Clip> {
    glue::read_clipboard().as_deref().and_then(askbar::clip_text)
}

#[tauri::command]
pub fn askbar_hide(app: AppHandle) {
    glue::hide(&app);
}

#[tauri::command]
pub fn askbar_resize(app: AppHandle, expanded: bool) {
    glue::resize(&app, expanded);
}

pub(super) fn label_for(agent: &str) -> String {
    crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id.eq_ignore_ascii_case(agent))
        .map(|d| d.label)
        .unwrap_or_else(|| agent.to_string())
}

/// Ask one question through the read-only `quick_ask` path. The answer is returned to the bar; if
/// the bar was hidden meanwhile it also becomes an Inbox notice.
#[tauri::command]
pub async fn askbar_ask(app: AppHandle, state: State<'_, AppState>, request: AskRequest) -> Result<AskResult, String> {
    askbar::remember_agent(&request.agent);
    let registry = state.registry.clone();
    let project = crate::standby::default_project(&registry);
    let label = label_for(&request.agent);
    let run_app = app.clone();
    let req = request.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        askbar::run_ask(&req, &label, Some(project), &|quick| {
            crate::agent_runs::quick::quick_ask(quick, registry.clone(), run_app.clone(), None)
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    if askbar::needs_notice(glue::bar_visible(&app)) {
        glue::publish_notice(&app, &result);
    }
    Ok(result)
}

/// "Continue in a pane": a visible pane of the same agent whose first task is the question.
#[tauri::command]
pub async fn askbar_continue(
    app: AppHandle,
    state: State<'_, AppState>,
    agent: String,
    question: String,
    clipboard: Option<String>,
) -> Result<(), String> {
    let registry = state.registry.clone();
    let project = crate::standby::default_project(&registry);
    let body = askbar::continue_request(&agent, &question, clipboard.as_deref(), Some(project));
    let run: crate::agent_runs::AgentRunRequest = serde_json::from_value(body).map_err(|e| e.to_string())?;
    let run_app = app.clone();
    let view = tauri::async_runtime::spawn_blocking(move || crate::agent_runs::run_agent(run, registry, run_app, None))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.message)?;
    glue::hide(&app);
    crate::cockpit::open_cockpit(&app, view.pane_id.as_deref());
    Ok(())
}

#[tauri::command]
pub fn askbar_notices() -> Vec<Notice> {
    askbar::notices()
}

#[tauri::command]
pub fn askbar_notice_dismiss(app: AppHandle, id: String) -> bool {
    glue::remove_notice(&app, &id).is_some()
}

#[tauri::command]
pub fn askbar_notice_open(app: AppHandle, id: String) -> bool {
    glue::open_notice(&app, &id)
}
