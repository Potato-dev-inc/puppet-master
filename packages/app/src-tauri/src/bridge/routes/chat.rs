//! `GET /panes/{id}/chat?since=<rev>`: the read-only chat of an agent pane for the phone. The answer is built (and
//! redacted) by `session_chat::phone`; this only finds the pane and the shared readers.

use serde_json::json;
use tauri::Manager;

use super::{Ctx, RouteResult};
use crate::bridge::http::query_param;
use crate::commands::SessionChatState;
use crate::session_chat::{phone, Opening};

pub(super) fn get(cx: &Ctx, pane_id: &str) -> RouteResult {
    let agent_type = cx
        .registry
        .lock()
        .get(pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| (404, json!({ "error": crate::pty::registry::unknown_pane_message(pane_id) })))?;
    let since = query_param(cx.query, "since").and_then(|value| value.parse::<u64>().ok());
    let opening = Opening::for_agent(&agent_type);
    let watchers = cx.app.state::<SessionChatState>().watchers(cx.app, cx.registry);
    Ok((200, phone::read(&watchers, pane_id, opening.as_ref(), since)))
}
