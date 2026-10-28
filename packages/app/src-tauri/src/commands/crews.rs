//! Tauri commands for crews (`crate::crews`): list, draft from a live pane, save, remove, start.
//! Errors that are not a crew problem read `CODE: message` like the worktree commands.

use super::AppState;
use crate::crews::{agents, live, model::Crew, start, starters, store, CrewError};
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::{AppHandle, State};

#[derive(Serialize)]
pub struct SaveOutcome {
    pub slug: Option<String>,
    pub errors: Vec<CrewError>,
}

fn text(error: CrewError) -> String {
    error.to_string()
}

fn internal(error: impl std::fmt::Display) -> String {
    format!("INTERNAL: {error}")
}

#[tauri::command]
pub async fn crews_list() -> Result<Vec<store::Summary>, String> {
    tauri::async_runtime::spawn_blocking(|| store::list(&store::dir(), &live::agent_state)).await.map_err(internal)
}

/// The built-in starter crews (never written to app data; copy one to My crews to keep it).
#[tauri::command]
pub async fn crews_starters() -> Result<Vec<store::Summary>, String> {
    tauri::async_runtime::spawn_blocking(|| starters::summaries(&live::agent_state)).await.map_err(internal)
}

/// A saved crew or a starter, by slug.
#[tauri::command]
pub async fn crews_get(slug: String) -> Result<Crew, String> {
    tauri::async_runtime::spawn_blocking(move || store::load_any(&store::dir(), &slug)).await.map_err(internal)?.map_err(text)
}

/// The agents a role can run, and the one `{default}` stands for (null when none is set up).
#[tauri::command]
pub async fn crews_agents() -> Result<agents::Choices, String> {
    tauri::async_runtime::spawn_blocking(live::choices).await.map_err(internal)
}

/// The draft of "Save this team" for a pane and the helpers under it. Nothing is saved yet.
#[tauri::command]
pub async fn crews_draft(state: State<'_, AppState>, pane_id: String) -> Result<crate::crews::draft::Draft, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || live::draft_for_pane(&registry, &pane_id)).await.map_err(internal)?.map_err(text)
}

/// Save a crew. Problems come back in `errors` (nothing is written then), each naming its role. A crew
/// with the same name is refused (`CREW_NAME_TAKEN`) unless `replace` is true.
#[tauri::command]
pub async fn crews_save(crew: Crew, replace: Option<bool>) -> Result<SaveOutcome, String> {
    let replace = replace.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || match store::save(&store::dir(), &crew, replace, &live::agent_state) {
        Ok(slug) => SaveOutcome { slug: Some(slug), errors: Vec::new() },
        Err(errors) => SaveOutcome { slug: None, errors },
    })
    .await
    .map_err(internal)
}

#[tauri::command]
pub async fn crews_remove(slug: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || store::remove(&store::dir(), &slug)).await.map_err(internal)?.map_err(text)
}

/// Start a crew in `folder`, one role at a time. `agents` maps role names to the agent the Start sheet chose
/// for them (roles not named keep theirs); `{default}` roles take the default agent. Returns who started,
/// and which role failed (if one did). A second start of the same crew while one runs is refused
/// (`CREW_STARTING`), so a double click does not start two sets of panes.
#[tauri::command]
pub async fn crews_start(
    app: AppHandle,
    state: State<'_, AppState>,
    slug: String,
    folder: String,
    agents: Option<BTreeMap<String, String>>,
) -> Result<start::Outcome, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _starting = start::StartGuard::claim(&slug).map_err(text)?;
        let stored = store::load_any(&store::dir(), &slug).map_err(text)?;
        let chosen = agents::with_agents(&stored, &agents.unwrap_or_default());
        let crew = agents::resolve_defaults(&chosen, live::choices().default.as_deref()).map_err(text)?;
        let problems = crate::crews::validate::validate(&crew, &live::agent_state);
        if let Some(first) = problems.into_iter().next() {
            return Err(text(first));
        }
        live::check_folder(&folder).map_err(text)?;
        crate::worktrees::folder::remember(&crate::app_paths::app_data_dir(), &folder);
        Ok(start::run(&crew, &folder, &mut live::LiveStarter::new(registry, app)))
    })
    .await
    .map_err(internal)?
}
