//! Tauri commands of the chat view: it follows the session file an agent writes for a pane, or reads the pane's
//! screen when the agent has no file Puppet Master can read (`crate::session_chat`), and sends the items to the
//! desktop windows as `session-chat-items` events.
//! Desktop only: nothing here is reachable through the bridge, the phone or MCP.

use super::AppState;
use crate::pty::PaneRegistry;
use crate::chat_index::{Answer, ChatIndex, ContextLookup, Facets, Filters, PaneContext};
use crate::session_chat::{ApiEndpoint, ChatItem, ItemObserver, Opening, PaneFacts, PaneLookup, ScreenRead, Sink, Watchers};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

const NO_SOURCE: &str = "Chat is not available for this pane: it has no agent to read a conversation from.";

/// The readers of the panes whose chat view is open. Built on the first open.
#[derive(Default)]
pub struct SessionChatState {
    watchers: OnceCell<Arc<Watchers>>,
    /// The search index of every chat read (`None`: it could not be opened, search is off).
    index: OnceCell<Option<Arc<ChatIndex>>>,
    /// The saved-page exports not yet written (see `session_chat::export_save`).
    pages: crate::session_chat::export_save::Pages,
}

impl SessionChatState {
    /// The readers shared by the desktop windows and the phone route (built on first use). Every item they read
    /// goes to the search index.
    pub fn watchers(&self, app: &AppHandle, registry: &Arc<Mutex<PaneRegistry>>) -> Arc<Watchers> {
        self.watchers
            .get_or_init(|| {
                let watchers = Watchers::new(Arc::new(EventSink(app.clone())), Arc::new(RegistryPanes(registry.clone())));
                Arc::new(match self.index(registry) {
                    Some(index) => watchers.with_observer(index as Arc<dyn ItemObserver>),
                    None => watchers,
                })
            })
            .clone()
    }

    /// The search index of this profile (opened on first use).
    pub fn index(&self, registry: &Arc<Mutex<PaneRegistry>>) -> Option<Arc<ChatIndex>> {
        self.index.get_or_init(|| crate::chat_index::open_default(Arc::new(RegistryPanes(registry.clone())))).clone()
    }

    /// The chat pages made by "Save as page" and waiting to be written.
    pub fn pages(&self) -> &crate::session_chat::export_save::Pages {
        &self.pages
    }

    /// The readers, if a chat view has already built them. Reading them never opens a reader.
    pub fn existing(&self) -> Option<Arc<Watchers>> {
        self.watchers.get().cloned()
    }

    /// Stops every reader (the app is exiting).
    pub fn shutdown(&self) {
        if let Some(watchers) = self.watchers.get() {
            watchers.close_all();
        }
    }
}

/// Answer of `session_chat_open`. `waiting_for`: while there are no items, what the reader is waiting for.
/// `source`: where the items come from, `"file"` (the agent's session file), `"api"` (the agent's local session API)
/// or `"screen"` (read off the terminal).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChatOpened {
    available: bool,
    reason: Option<String>,
    items: Vec<ChatItem>,
    waiting_for: Option<String>,
    source: Option<&'static str>,
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
struct SourceEvent<'a> {
    pane_id: &'a str,
    source: &'a str,
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

    fn source(&self, pane_id: &str, source: &str) {
        let _ = self.0.emit("session-chat-source", SourceEvent { pane_id, source });
    }
}

/// A pane's facts from the registry, the hook ledger (agent session id) and the accounts store.
struct RegistryPanes(Arc<Mutex<PaneRegistry>>);

impl PaneLookup for RegistryPanes {
    fn facts(&self, pane_id: &str) -> Option<PaneFacts> {
        let (info, others) = {
            let registry = self.0.lock();
            let info = registry.get(pane_id)?.info.clone();
            let others: Vec<(String, String)> = registry
                .panes
                .iter()
                .filter(|(id, pane)| id.as_str() != pane_id && !*pane.exited.lock())
                .map(|(_, pane)| (pane.info.agent_type.clone(), pane.info.cwd.clone()))
                .collect();
            (info, others)
        };
        let account = info.account.as_deref();
        // A hook-reported id (it follows `/clear` and resumes) wins; until one arrives, the id the pane was launched with.
        let hooked = crate::agent_hooks::ledger::pinned_session(pane_id);
        let launched = crate::pty::launch_session::get(pane_id);
        let from_launch = hooked.is_none() && launched.is_some();
        Some(PaneFacts {
            cwd: PathBuf::from(&info.cwd),
            started_at: UNIX_EPOCH + Duration::from_millis(info.created_at.max(0) as u64),
            provider_session_id: hooked.or(launched),
            launch: crate::session_chat::LaunchFacts::of(
                from_launch,
                (&info.agent_type, &info.cwd),
                others.iter().map(|(agent, cwd)| (agent.as_str(), cwd.as_str())),
            ),
            home: account
                .and_then(|id| crate::accounts::Store::default_location().get(id))
                .map(|account| PathBuf::from(account.home)),
        })
    }

    fn other_sessions(&self, pane_id: &str) -> Vec<String> {
        let mut ids = crate::agent_hooks::ledger::pinned_sessions_except(pane_id);
        ids.extend(crate::pty::launch_session::all_except(pane_id));
        ids
    }

    fn api(&self, pane_id: &str) -> Option<ApiEndpoint> {
        let registry = self.0.lock();
        let pane = registry.get(pane_id)?;
        crate::tui::profile_for(&pane.info.agent_type).session_api_endpoint(pane)
    }

    fn screen(&self, pane_id: &str, known: Option<u64>) -> Option<ScreenRead> {
        crate::session_chat::read_pane(&self.0, pane_id, known)
    }
}

/// Where an item came from, for the search: the pane's agent, the name of its folder and its session id.
impl ContextLookup for RegistryPanes {
    fn context(&self, pane_id: &str) -> Option<PaneContext> {
        let info = self.0.lock().get(pane_id)?.info.clone();
        let session = crate::agent_hooks::ledger::pinned_session(pane_id).or_else(|| crate::pty::launch_session::get(pane_id));
        Some(PaneContext::of(&info.agent_type, &info.cwd, session.as_deref()))
    }
}

/// Search across every chat that was read: the items whose text has all the typed words, newest first, with a
/// snippet and the ranges to highlight. `filters` (optional) narrows it to an agent, a folder, a start time and who
/// spoke. `enabled` is false when the person turned the index off (then no hits). Empty when nothing was typed and
/// no filter is set; with filters only, the newest items that pass them.
#[tauri::command]
pub async fn chat_search(
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    query: String,
    limit: Option<usize>,
    filters: Option<Filters>,
) -> Result<Answer, String> {
    let Some(index) = chat.index(&state.registry) else { return Ok(Answer { enabled: crate::chat_index::enabled(), hits: Vec::new() }) };
    let filters = filters.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || index.search_filtered(&query, limit.unwrap_or(30), &filters))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// The agents and folders the index holds, for the "Search all chats" filter menus.
#[tauri::command]
pub async fn chat_search_facets(state: State<'_, AppState>, chat: State<'_, SessionChatState>) -> Result<Facets, String> {
    let Some(index) = chat.index(&state.registry) else { return Ok(Facets::default()) };
    tauri::async_runtime::spawn_blocking(move || index.facets())
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// Start following the pane's session file, or its screen. `{ available, reason, items, source }`; the items so far
/// come back here, later ones as `session-chat-items` events until `session_chat_close`.
#[tauri::command]
pub async fn session_chat_open(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    window: tauri::Window,
    pane_id: String,
    surface: Option<String>,
) -> Result<SessionChatOpened, String> {
    let agent_type = state
        .registry
        .lock()
        .get(&pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    // An agent with no session file Puppet Master can read is read off its screen, unless it is a plain shell.
    let Some(opening) = Opening::for_agent(&agent_type) else {
        return Ok(SessionChatOpened { available: false, reason: Some(NO_SOURCE.into()), items: Vec::new(), waiting_for: None, source: None });
    };
    let watchers = chat.watchers(&app, &state.registry);
    let holder = holder_id(window.label(), surface.as_deref());
    // The first read of a long session file happens in this call.
    let (items, waiting_for, source) = tauri::async_runtime::spawn_blocking(move || {
        let items = opening.open(&watchers, &pane_id, holder.as_str().into());
        (items, watchers.waiting_for(&pane_id), opening.source_now(&watchers, &pane_id))
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?;
    Ok(SessionChatOpened { available: true, reason: None, items, waiting_for, source: Some(source) })
}

/// One holder per chat surface: two views of one pane in a window (the cockpit and a pane view) each hold the
/// reader, and it stops when the last lets go. `surface` is the view's own id.
fn holder_id(window: &str, surface: Option<&str>) -> String {
    format!("{window}#{}", surface.unwrap_or_default())
}

/// This surface stops following the pane's session file or screen; the reader stops when no other surface (or
/// phone) is following it.
#[tauri::command]
pub async fn session_chat_close(
    chat: State<'_, SessionChatState>,
    window: tauri::Window,
    pane_id: String,
    surface: Option<String>,
) -> Result<(), String> {
    if let Some(watchers) = chat.watchers.get() {
        watchers.close(&pane_id, &holder_id(window.label(), surface.as_deref()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_agent_without_a_session_source_is_read_off_its_screen_unless_it_is_a_shell() {
        let generic = crate::tui::profile_for("no-such-agent");
        assert!(generic.session_chat().is_none());
        assert!(generic.screen_chat());
        let shell = crate::tui::profile_for("powershell");
        assert!(shell.session_chat().is_none() && !shell.screen_chat());
        // The two agents with a session file keep it.
        assert!(crate::tui::profile_for("claude").session_chat().is_some());
        assert!(crate::tui::profile_for("codex").session_chat().is_some());
    }
}
