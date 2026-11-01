//! Tauri commands for a terminal chat's questions (`terminal_chat::prompt`). Desktop only: no bridge route, no MCP
//! tool, no phone path. The answer needs the person's click (`UserGesture`).

use crate::terminal_chat::prompt::{self, PromptView};
use super::AppState;
use tauri::{AppHandle, State};

/// The question the pane shows now, as a card; `None` when nothing recognisable is on the screen.
#[tauri::command]
pub async fn terminal_chat_prompt(state: State<'_, AppState>, pane: String) -> Result<Option<PromptView>, String> {
    Ok(prompt::view(&state.registry, &pane))
}

/// The person's pick. Errors start with a stable code: `TERMINAL_CHAT_PROMPT_STALE` (read again),
/// `TERMINAL_CHAT_UNKNOWN_OPTION`, `ANSWER_NEEDS_A_PERSON`, then the answer route's own codes.
#[tauri::command]
pub async fn terminal_chat_answer(
    app: AppHandle,
    state: State<'_, AppState>,
    pane: String,
    prompt_id: String,
    option_id: String,
    gesture: Option<crate::pane_prompt::answers::UserGesture>,
) -> Result<(), String> {
    prompt::answer(&state.registry, &app, &pane, &prompt_id, &option_id, gesture.as_ref()).map_err(|e| e.wire())
}
