//! "Save chat as page" (the chat's header menu): the pane's chat as one self-contained HTML file. The page is shown
//! in a preview and written where the person picks. Desktop only; nothing is uploaded, and nothing here is reachable
//! through the bridge, the phone or MCP.

use super::session_chat::SessionChatState;
use super::AppState;
use crate::session_chat::export::{self, Meta, TurnRange};
use crate::session_chat::{Hold, Opening};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, State};

const NO_SOURCE: &str = "Chat is not available for this pane: it has no agent to read a conversation from.";
/// The export holds the pane's reader only while it reads the items; it lets go at once.
const HOLDER: &str = "chat-export";
const HOLD_FOR: Duration = Duration::from_secs(30);

/// The chat of a pane as one HTML page (see `session_chat::export`). `from_turn` / `to_turn` keep those turns
/// (inclusive, numbered by the person's messages); both optional.
#[tauri::command]
pub async fn chat_export_html(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    pane_id: String,
    from_turn: Option<u32>,
    to_turn: Option<u32>,
) -> Result<String, String> {
    let range = TurnRange { from: from_turn, to: to_turn };
    range.check()?;
    let (agent_type, cwd) = {
        let registry = state.registry.lock();
        let pane = registry.get(&pane_id).ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        (pane.info.agent_type.clone(), pane.info.cwd.clone())
    };
    let opening = Opening::for_agent(&agent_type).ok_or_else(|| NO_SOURCE.to_string())?;
    let watchers = chat.watchers(&app, &state.registry);
    let items = tauri::async_runtime::spawn_blocking(move || {
        opening.open(&watchers, &pane_id, Hold::lease(HOLDER, HOLD_FOR));
        let items = watchers.since(&pane_id, None, usize::MAX).map(|delta| delta.items).unwrap_or_default();
        watchers.close(&pane_id, HOLDER);
        items
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?;
    let ctx = crate::redact::known::context();
    let meta = Meta {
        agent: crate::ambient::agent_label(&agent_type),
        folder: export::folder_of(&cwd, ctx.home.as_deref()),
        date: chrono::Local::now().format("%Y-%m-%d").to_string(),
    };
    Ok(export::page(&items, &meta, range, &ctx))
}

/// Writes the page the preview showed to `path` (a file name ending in .html). Returns the path written.
#[tauri::command]
pub async fn chat_export_save(path: String, html: String) -> Result<String, String> {
    let dest = PathBuf::from(path.trim());
    let is_html = dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    if dest.as_os_str().is_empty() || !is_html {
        return Err("Choose a file name that ends in .html.".into());
    }
    std::fs::write(&dest, html.as_bytes()).map_err(|e| format!("Could not write the file: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}
