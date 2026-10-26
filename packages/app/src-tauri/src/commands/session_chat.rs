//! Tauri commands of the chat view: it follows the session file an agent writes for a pane
//! (`crate::session_chat`) and sends the items to the desktop windows as `session-chat-items` events.
//! Desktop only: nothing here is reachable through the bridge, the phone or MCP.

use super::AppState;
use crate::pty::PaneRegistry;
use crate::session_chat::{ChatItem, PaneFacts, PaneLookup, Sink, Watchers};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

const NO_SOURCE: &str = "This agent does not write a session file Puppet Master can read yet.";

/// The readers of the panes whose chat view is open. Built on the first open.
#[derive(Default)]
pub struct SessionChatState {
    watchers: OnceCell<Arc<Watchers>>,
}

impl SessionChatState {
    /// Stops every reader (the app is exiting).
    pub fn shutdown(&self) {
        if let Some(watchers) = self.watchers.get() {
            watchers.close_all();
        }
    }
}

/// Answer of `session_chat_open`. `waiting_for`: while there are no items, what the reader is waiting for.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChatOpened {
    available: bool,
    reason: Option<String>,
    items: Vec<ChatItem>,
    waiting_for: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemsEvent<'a> {
    pane_id: &'a str,
    items: Vec<ChatItem>,
    reset: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusEvent<'a> {
    pane_id: &'a str,
    waiting_for: Option<&'a str>,
}

struct EventSink(AppHandle);

impl Sink for EventSink {
    fn send(&self, pane_id: &str, items: Vec<ChatItem>, reset: bool) {
        let _ = self.0.emit("session-chat-items", ItemsEvent { pane_id, items, reset });
    }

    fn waiting(&self, pane_id: &str, waiting_for: Option<&str>) {
        let _ = self.0.emit("session-chat-status", StatusEvent { pane_id, waiting_for });
    }
}

/// A pane's facts from the registry, the hook ledger (agent session id) and the accounts store.
struct RegistryPanes(Arc<Mutex<PaneRegistry>>);

impl PaneLookup for RegistryPanes {
    fn facts(&self, pane_id: &str) -> Option<PaneFacts> {
        let info = self.0.lock().get(pane_id)?.info.clone();
        let account = info.account.as_deref();
        Some(PaneFacts {
            cwd: PathBuf::from(&info.cwd),
            started_at: UNIX_EPOCH + Duration::from_millis(info.created_at.max(0) as u64),
            provider_session_id: crate::agent_hooks::ledger::pinned_session(pane_id),
            home: account
                .and_then(|id| crate::accounts::Store::default_location().get(id))
                .map(|account| PathBuf::from(account.home)),
        })
    }
}

/// Start following the pane's session file. `{ available, reason, items }`; the items so far come back
/// here, later ones as `session-chat-items` events until `session_chat_close`.
#[tauri::command]
pub async fn session_chat_open(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    pane_id: String,
) -> Result<SessionChatOpened, String> {
    let agent_type = state
        .registry
        .lock()
        .get(&pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let Some(source) = crate::tui::profile_for(&agent_type).session_chat() else {
        return Ok(SessionChatOpened { available: false, reason: Some(NO_SOURCE.into()), items: Vec::new(), waiting_for: None });
    };
    let registry = state.registry.clone();
    let watchers = chat
        .watchers
        .get_or_init(|| Arc::new(Watchers::new(Arc::new(EventSink(app)), Arc::new(RegistryPanes(registry)))))
        .clone();
    // The first read of a long session file happens in this call.
    let (items, waiting_for) = tauri::async_runtime::spawn_blocking(move || {
        let items = watchers.open(&pane_id, source);
        (items, watchers.waiting_for(&pane_id))
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?;
    Ok(SessionChatOpened { available: true, reason: None, items, waiting_for })
}

/// Stop following the pane's session file.
#[tauri::command]
pub async fn session_chat_close(chat: State<'_, SessionChatState>, pane_id: String) -> Result<(), String> {
    if let Some(watchers) = chat.watchers.get() {
        watchers.close(&pane_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_agent_without_a_session_source_gets_no_chat_view() {
        assert!(crate::tui::profile_for("no-such-agent").session_chat().is_none());
    }
}
