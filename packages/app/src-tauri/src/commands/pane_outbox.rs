//! Tauri commands of the Chat tab's message box: send a message to the agent in a pane (now when it is ready,
//! later when its turn ends), cancel one that has not started, list a pane's messages. The list changes arrive as
//! `pane-outbox` events (`{ paneId, messages }`). Desktop commands; a phone uses the bridge routes
//! (`bridge/routes/panes.rs`), which share the same outbox.

use super::AppState;
use crate::pane_outbox::{self, OutboxMessage};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn pane_message_send(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    text: String,
) -> Result<OutboxMessage, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || pane_outbox::global(&registry, &app).send(&pane_id, &text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn pane_message_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    message_id: String,
) -> Result<OutboxMessage, String> {
    pane_outbox::global(&state.registry, &app).cancel(&pane_id, &message_id)
}

#[tauri::command]
pub async fn pane_messages_list(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
) -> Result<Vec<OutboxMessage>, String> {
    Ok(pane_outbox::global(&state.registry, &app).list(&pane_id))
}
