//! The writer behind the index: the chat readers hand it their items (`ItemObserver`), a background thread
//! redacts them, adds the pane's agent and folder, and writes them. A reader thread never waits on the disk, and
//! an index that cannot be written only loses search, never the chat.
//!
//! The on/off switch (`Settings > General`, "Keep a search index of my chats") is an `AtomicBool` the index reads:
//! when it is off nothing is copied, written or searched. Turning it off keeps what is already there; `clear`
//! removes it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;

use super::entry::{entries_of, is_kept, Entry, PaneContext};
use super::filter::{Facets, Filters};
use super::imported::ImportedChatRow;
use super::store::{Hit, Store};
use crate::session_chat::{ChatItem, ItemObserver};

/// What the app knows about a pane (`None`: the pane is gone).
pub trait ContextLookup: Send + Sync {
    fn context(&self, pane_id: &str) -> Option<PaneContext>;
}

/// The answer to a search. `enabled` is false when the index is off: then `hits` is empty and nothing was searched.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub enabled: bool,
    pub hits: Vec<Hit>,
}

/// What the index holds now: whether it is on, how many items, how many bytes the file takes, and how many old copies
/// of a damaged index are left on disk. `imported_chats` and `imported_items` count the old chats read in (not in
/// `items`, which counts what the panes said).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub items: usize,
    pub bytes: u64,
    /// Old copies of a damaged index (they hold old chat text; "Clear search index" removes them).
    pub old_copies: usize,
    pub imported_chats: usize,
    pub imported_items: usize,
}

struct Job {
    pane_id: String,
    items: Vec<ChatItem>,
}

/// The index and the thread that feeds it.
pub struct ChatIndex {
    store: Arc<Mutex<Store>>,
    tx: Mutex<Sender<Job>>,
    on: Arc<AtomicBool>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl ChatIndex {
    /// Feeds `store` while the index is on; `lookup` names the pane an item came from.
    pub fn start(store: Store, lookup: Arc<dyn ContextLookup>) -> Arc<ChatIndex> {
        Self::start_switched(store, lookup, Arc::new(AtomicBool::new(true)))
    }

    /// Like `start`, with the on/off switch the app shares (`chat_index::switch`).
    pub fn start_switched(store: Store, lookup: Arc<dyn ContextLookup>, on: Arc<AtomicBool>) -> Arc<ChatIndex> {
        let store = Arc::new(Mutex::new(store));
        let (tx, rx) = channel();
        let writer = store.clone();
        let switch = on.clone();
        let spawned = std::thread::Builder::new().name("chat-index".into()).spawn(move || write_loop(rx, writer, lookup, switch));
        if let Err(error) = spawned {
            tracing::warn!(%error, "chat index: could not start the writer");
        }
        Arc::new(ChatIndex { store, tx: Mutex::new(tx), on })
    }

    /// Whether the index is on.
    pub fn enabled(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    /// The items whose text has every typed word, newest first (none when the index is off). Every row, old chats
    /// included: only the desktop window may use it. A phone, a tool or a companion uses `search_live`.
    pub fn search(&self, query: &str, limit: usize) -> Result<Answer, String> {
        self.search_filtered(query, limit, &Filters::default())
    }

    /// Like `search`, over live rows only (what the panes said). Old chats are never in the answer. This is the
    /// search for every surface that is not the desktop window (the phone's `/chats/search`). None when the index is off.
    pub fn search_live(&self, query: &str, limit: usize) -> Result<Answer, String> {
        if !self.enabled() {
            return Ok(Answer { enabled: false, hits: Vec::new() });
        }
        let hits = self.store.lock().search_live(query, limit).map_err(|error| format!("INTERNAL: chat search: {error}"))?;
        Ok(Answer { enabled: true, hits })
    }

    /// Like `search`, with the chips' filters (`Filters`). None when the index is off.
    pub fn search_filtered(&self, query: &str, limit: usize, filters: &Filters) -> Result<Answer, String> {
        if !self.enabled() {
            return Ok(Answer { enabled: false, hits: Vec::new() });
        }
        let hits = self.store.lock().search_filtered(query, limit, filters).map_err(|error| format!("INTERNAL: chat search: {error}"))?;
        Ok(Answer { enabled: true, hits })
    }

    /// The agents and folders in the index, for the filter menus. Nothing when the index is off.
    pub fn facets(&self) -> Result<Facets, String> {
        if !self.enabled() {
            return Ok(Facets::default());
        }
        self.store.lock().facets().map_err(|error| format!("INTERNAL: chat search: {error}"))
    }

    /// What was said (the person's and the agent's messages) in the chats of `panes` only, newest first. Nothing
    /// outside those panes is returned, and no pane means no hit. Nothing is searched when the index is off.
    pub fn search_in(&self, query: &str, limit: usize, panes: &[String]) -> Result<Answer, String> {
        if !self.enabled() {
            return Ok(Answer { enabled: false, hits: Vec::new() });
        }
        let hits = self.store.lock().search_in(query, limit, panes).map_err(|error| format!("INTERNAL: chat search: {error}"))?;
        Ok(Answer { enabled: true, hits })
    }

    /// The items kept, the bytes the file takes and whether the index is on.
    pub fn status(&self) -> Result<Status, String> {
        let store = self.store.lock();
        let (items, bytes) = store.usage().map_err(|error| format!("INTERNAL: chat index: {error}"))?;
        let (imported_chats, imported_items) = store.imported_usage().map_err(|error| format!("INTERNAL: chat index: {error}"))?;
        Ok(Status { enabled: self.enabled(), items, bytes, old_copies: store.old_copies(), imported_chats, imported_items })
    }

    /// Writes one old chat whole (`Store::replace_chat`): its earlier copy is replaced, and a killed app or an error
    /// leaves the chat as it was. Refused with `INDEX_OFF` when the index is off, so nothing is written then.
    pub fn import_chat(&self, row: &ImportedChatRow, entries: &[Entry]) -> Result<(), String> {
        if !self.enabled() {
            return Err("INDEX_OFF".into());
        }
        self.store.lock().replace_chat(row, entries).map_err(|error| format!("INTERNAL: chat import: {error}"))
    }

    /// The stamp an old chat was read with (`None` when it was never imported). Equal stamp = not changed.
    pub fn imported_stamp(&self, agent: &str, session: &str) -> Result<Option<String>, String> {
        self.store.lock().imported_stamp(agent, session).map_err(|error| format!("INTERNAL: chat import: {error}"))
    }

    /// Whether a live chat (the panes said it) is in the index under this agent and session.
    pub fn has_live(&self, agent: &str, session: &str) -> Result<bool, String> {
        self.store.lock().has_live(agent, session).map_err(|error| format!("INTERNAL: chat import: {error}"))
    }

    /// The record of one old chat, if it was imported.
    pub fn imported_chat(&self, agent: &str, session: &str) -> Result<Option<ImportedChatRow>, String> {
        self.store.lock().imported_chat(agent, session).map_err(|error| format!("INTERNAL: chat import: {error}"))
    }

    /// Removes only the old chats (their text is erased from the file). Live items stay. Returns the status after it.
    pub fn clear_imported(&self) -> Result<Status, String> {
        self.store.lock().clear_imported().map_err(|error| format!("INTERNAL: chat import: {error}"))?;
        self.status()
    }

    /// Removes every item and its text from the file (see `Store::clear`), and the old copies of a damaged index
    /// next to it. The index stays on or off as it was.
    pub fn clear(&self) -> Result<(), String> {
        let mut store = self.store.lock();
        store.clear().map_err(|error| format!("INTERNAL: chat index: {error}"))?;
        store.remove_old_copies();
        Ok(())
    }

    /// Items written so far (tests wait on it).
    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.store.lock().count().unwrap_or(0)
    }

    /// The hits of a search, without the switch's answer (tests).
    #[cfg(test)]
    pub fn search_hits(&self, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
        self.search(query, limit).map(|answer| answer.hits)
    }
}

impl ItemObserver for ChatIndex {
    fn observe(&self, pane_id: &str, items: &[ChatItem]) {
        // Off: nothing is copied, nothing is sent to the writer.
        if !self.enabled() {
            return;
        }
        // Only what the index keeps is copied (never a tool's output).
        let items: Vec<ChatItem> = items.iter().filter(|item| is_kept(item)).cloned().collect();
        if !items.is_empty() {
            let _ = self.tx.lock().send(Job { pane_id: pane_id.to_string(), items });
        }
    }
}

/// The labels last seen per pane: a batch that arrives after its pane closed still gets them. A pane that the app no
/// longer knows is forgotten after its second miss, so the cache does not grow by one entry per pane for the whole run.
#[derive(Default)]
pub(super) struct Labels {
    known: HashMap<String, (PaneContext, u8)>,
}

/// Misses (lookups that found no pane) after which a pane's labels are dropped.
const FORGET_AFTER: u8 = 2;

impl Labels {
    /// Panes whose labels are kept (tests).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.known.len()
    }

    /// The labels for `pane_id` now. A session id that was known before stays when the lookup has none (yet).
    pub(super) fn of(&mut self, lookup: &dyn ContextLookup, pane_id: &str) -> PaneContext {
        match lookup.context(pane_id) {
            Some(mut ctx) => {
                if let Some((before, _)) = self.known.get(pane_id) {
                    if ctx.session.is_empty() {
                        ctx.session = before.session.clone();
                    }
                }
                self.known.insert(pane_id.to_string(), (ctx.clone(), 0));
                ctx
            }
            None => {
                let Some((ctx, misses)) = self.known.get_mut(pane_id) else { return PaneContext::default() };
                *misses += 1;
                let ctx = ctx.clone();
                if *misses >= FORGET_AFTER {
                    self.known.remove(pane_id);
                }
                ctx
            }
        }
    }
}

fn write_loop(rx: Receiver<Job>, store: Arc<Mutex<Store>>, lookup: Arc<dyn ContextLookup>, on: Arc<AtomicBool>) {
    let mut labels = Labels::default();
    while let Ok(first) = rx.recv() {
        let mut jobs = vec![first];
        while let Ok(more) = rx.try_recv() {
            jobs.push(more);
        }
        // Batches queued before the switch went off are dropped, not written.
        if !on.load(Ordering::SeqCst) {
            continue;
        }
        let now = now_ms();
        let mut entries = Vec::new();
        for job in jobs {
            let ctx = labels.of(lookup.as_ref(), &job.pane_id);
            entries.extend(entries_of(&job.pane_id, &ctx, &job.items, now));
        }
        if let Err(error) = store.lock().insert(&entries) {
            tracing::warn!(%error, "chat index: could not write");
        }
    }
}

/// Opens the index file at `path`. A file that cannot be opened (damaged, or not an index) is moved aside, not
/// deleted, and a new one is made.
pub fn open_store(path: &Path, cap: usize) -> Result<Store, String> {
    match Store::open(path, cap) {
        Ok(store) => Ok(store),
        Err(first) => {
            // The log and shared-memory files go with it: left behind they would be replayed into the new file.
            super::aside::move_aside(path, now_ms()).map_err(|_| format!("chat index: {first}"))?;
            Store::open(path, cap).map_err(|error| format!("chat index: {error}"))
        }
    }
}
