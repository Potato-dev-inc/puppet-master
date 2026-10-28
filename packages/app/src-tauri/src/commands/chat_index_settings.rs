//! Tauri commands of the search index settings (`Settings > General`): what the index holds, its on/off switch, and
//! clearing it. Desktop only: nothing here is on the bridge, the phone or MCP.

use serde_json::json;
use tauri::{AppHandle, State};

use super::{AppState, SessionChatState};
use crate::chat_index::{self, Status};

/// The status when the index could not be opened (it holds nothing, search is off for the chats).
fn nothing_kept() -> Status {
    Status { enabled: chat_index::enabled(), items: 0, bytes: 0 }
}

/// What the index holds and whether it is on: `{ enabled, items, bytes }`.
#[tauri::command]
pub async fn chat_index_status(state: State<'_, AppState>, chat: State<'_, SessionChatState>) -> Result<Status, String> {
    let Some(index) = chat.index(&state.registry) else { return Ok(nothing_kept()) };
    tauri::async_runtime::spawn_blocking(move || index.status())
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// "Keep a search index of my chats". Saved with the other settings in `settings.json` (through `settings_patch`,
/// which also tells the index), so it survives a restart. When off, no chat is copied and nothing is searched; what
/// is already there stays until `chat_index_clear`.
#[tauri::command]
pub fn chat_index_set_enabled(app: AppHandle, state: State<'_, AppState>, chat: State<'_, SessionChatState>, enabled: bool) -> Result<Status, String> {
    crate::settings_patch::apply(&app, &json!({ (chat_index::SETTING_KEY): enabled }))?;
    chat_index::set_enabled(enabled);
    match chat.index(&state.registry) {
        Some(index) => index.status(),
        None => Ok(nothing_kept()),
    }
}

/// "Clear search index": removes the saved search text (rows, text index, free pages and the write-ahead log).
/// Chats themselves are not touched. Returns the status after the clear.
#[tauri::command]
pub async fn chat_index_clear(state: State<'_, AppState>, chat: State<'_, SessionChatState>) -> Result<Status, String> {
    let Some(index) = chat.index(&state.registry) else { return Ok(nothing_kept()) };
    tauri::async_runtime::spawn_blocking(move || {
        index.clear()?;
        index.status()
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}
