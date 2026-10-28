//! "Save chat as page" and "Save as Markdown" (the chat's header menu): the pane's chat as one self-contained HTML
//! file, or as one Markdown file. The text is shown in a preview and written where the person picks. Desktop only;
//! nothing is uploaded, and nothing here is reachable through the bridge, the phone or MCP.

use super::session_chat::SessionChatState;
use super::AppState;
use crate::redact::RedactContext;
use crate::session_chat::export::{self, Meta, TurnRange};
use crate::session_chat::export_save::PageKind;
use crate::session_chat::model::ChatItem;
use crate::session_chat::{Hold, Opening};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, State};

const NO_SOURCE: &str = "Chat is not available for this pane: it has no agent to read a conversation from.";
/// The export holds the pane's reader only while it reads the items; it lets go at once.
const HOLDER: &str = "chat-export";
const HOLD_FOR: Duration = Duration::from_secs(30);

/// A chat page for the preview, and the one-time token that saves exactly this page (`chat_export_save`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPage {
    pub html: String,
    pub token: String,
}

/// A chat as Markdown for the preview, and the one-time token that saves exactly this file (`chat_export_save`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMarkdown {
    pub markdown: String,
    pub token: String,
}

/// The chat of a pane, read once: its items, the header facts and the redaction context they were made with.
async fn gather(
    app: &AppHandle,
    state: &AppState,
    chat: &SessionChatState,
    pane_id: &str,
) -> Result<(Vec<ChatItem>, Meta, Arc<RedactContext>), String> {
    let (agent_type, cwd) = {
        let registry = state.registry.lock();
        let pane = registry.get(pane_id).ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        (pane.info.agent_type.clone(), pane.info.cwd.clone())
    };
    let opening = Opening::for_agent(&agent_type).ok_or_else(|| NO_SOURCE.to_string())?;
    let watchers = chat.watchers(app, &state.registry);
    let owned_pane = pane_id.to_string();
    let items = tauri::async_runtime::spawn_blocking(move || {
        opening.open(&watchers, &owned_pane, Hold::lease(HOLDER, HOLD_FOR));
        let items = watchers.since(&owned_pane, None, usize::MAX).map(|delta| delta.items).unwrap_or_default();
        watchers.close(&owned_pane, HOLDER);
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
    Ok((items, meta, ctx))
}

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
) -> Result<ExportPage, String> {
    let range = TurnRange { from: from_turn, to: to_turn };
    range.check()?;
    let (items, meta, ctx) = gather(&app, &state, &chat, &pane_id).await?;
    let html = export::page(&items, &meta, range, &ctx);
    let token = chat.pages().keep(&pane_id, PageKind::Html, &html)?;
    Ok(ExportPage { html, token })
}

/// The chat of a pane as one Markdown file (see `session_chat::export_md`). Same turn range as the page.
#[tauri::command]
pub async fn chat_export_markdown(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    pane_id: String,
    from_turn: Option<u32>,
    to_turn: Option<u32>,
) -> Result<ExportMarkdown, String> {
    let range = TurnRange { from: from_turn, to: to_turn };
    range.check()?;
    let (items, meta, ctx) = gather(&app, &state, &chat, &pane_id).await?;
    let markdown = crate::session_chat::export_md::markdown(&items, &meta, range, &ctx);
    let token = chat.pages().keep(&pane_id, PageKind::Markdown, &markdown)?;
    Ok(ExportMarkdown { markdown, token })
}

/// Writes the page or Markdown file the preview showed to `path`, a place the save dialog returned. The text is not
/// sent back by the window: it is the one Rust made and kept under `token` (see `session_chat::export_save`), so only
/// that text can be written, once, under a name of its own kind. Returns the path written.
#[tauri::command]
pub async fn chat_export_save(chat: State<'_, SessionChatState>, token: String, path: String) -> Result<String, String> {
    chat.pages().save(&token, &path).map(|dest| dest.to_string_lossy().to_string())
}
