//! Tauri commands for "Second opinion". Rules and wiring live in `crate::second_opinion`.
//! Errors read `CODE: message` like the worktree commands.

use super::*;
use crate::operations::OperationError;
use crate::second_opinion::route::Sent;
use crate::second_opinion::service::{self, Poll, ReviewerEntry, StartInfo};

fn text(error: OperationError) -> String {
    format!("{}: {}", error.code, error.message)
}

/// Agents that can review (headless, read-only, installed), with the author's own agent marked.
#[tauri::command]
pub async fn second_opinion_agents(state: State<'_, AppState>, pane_id: String) -> Result<Vec<ReviewerEntry>, String> {
    let author = service::author_agent_of(&state.registry, &pane_id);
    Ok(service::reviewers(author.as_deref()))
}

/// Pick the diff, build the prompt and start the read-only reviewer run. Returns at once with
/// the handle and the diff that was sent; the answer is read with `second_opinion_poll`.
#[tauri::command]
pub async fn second_opinion_start(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    reviewer: String,
) -> Result<StartInfo, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || service::start(&pane_id, &reviewer, registry, app))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
        .map_err(text)
}

#[tauri::command]
pub async fn second_opinion_poll(project: String, handle: String) -> Result<Poll, String> {
    tauri::async_runtime::spawn_blocking(move || service::poll(&project, &handle))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
        .map_err(text)
}

#[tauri::command]
pub async fn second_opinion_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    project: String,
    handle: String,
) -> Result<(), String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || service::cancel(&project, &handle, registry, app))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
        .map_err(text)
}

/// Send a finding (or a free comment on a diff line) to the agent that wrote the code.
#[tauri::command]
pub async fn second_opinion_send(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    file: String,
    line: Option<u32>,
    text: String,
) -> Result<Sent, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || service::send_to_author(&pane_id, &file, line, &text, registry, app))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
        .map_err(self::text)
}
