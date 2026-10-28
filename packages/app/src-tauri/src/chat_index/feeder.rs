//! The writer behind the index: the chat readers hand it their items (`ItemObserver`), a background thread
//! redacts them, adds the pane's agent and folder, and writes them. A reader thread never waits on the disk, and
//! an index that cannot be written only loses search, never the chat.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use super::entry::{entries_of, is_kept, PaneContext};
use super::store::{Hit, Store};
use crate::session_chat::{ChatItem, ItemObserver};

/// What the app knows about a pane (`None`: the pane is gone).
pub trait ContextLookup: Send + Sync {
    fn context(&self, pane_id: &str) -> Option<PaneContext>;
}

struct Job {
    pane_id: String,
    items: Vec<ChatItem>,
}

/// The index and the thread that feeds it.
pub struct ChatIndex {
    store: Arc<Mutex<Store>>,
    tx: Mutex<Sender<Job>>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl ChatIndex {
    /// Feeds `store`; `lookup` names the pane an item came from.
    pub fn start(store: Store, lookup: Arc<dyn ContextLookup>) -> Arc<ChatIndex> {
        let store = Arc::new(Mutex::new(store));
        let (tx, rx) = channel();
        let writer = store.clone();
        let spawned = std::thread::Builder::new().name("chat-index".into()).spawn(move || write_loop(rx, writer, lookup));
        if let Err(error) = spawned {
            tracing::warn!(%error, "chat index: could not start the writer");
        }
        Arc::new(ChatIndex { store, tx: Mutex::new(tx) })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
        self.store.lock().search(query, limit).map_err(|error| format!("INTERNAL: chat search: {error}"))
    }

    /// Items written so far (tests wait on it).
    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.store.lock().count().unwrap_or(0)
    }
}

impl ItemObserver for ChatIndex {
    fn observe(&self, pane_id: &str, items: &[ChatItem]) {
        // Only what the index keeps is copied (never a tool's output).
        let items: Vec<ChatItem> = items.iter().filter(|item| is_kept(item)).cloned().collect();
        if !items.is_empty() {
            let _ = self.tx.lock().send(Job { pane_id: pane_id.to_string(), items });
        }
    }
}

fn write_loop(rx: Receiver<Job>, store: Arc<Mutex<Store>>, lookup: Arc<dyn ContextLookup>) {
    // The last context seen per pane: a batch that arrives after its pane closed still gets its labels.
    let mut known: HashMap<String, PaneContext> = HashMap::new();
    while let Ok(first) = rx.recv() {
        let mut jobs = vec![first];
        while let Ok(more) = rx.try_recv() {
            jobs.push(more);
        }
        let now = now_ms();
        let mut entries = Vec::new();
        for job in jobs {
            if let Some(ctx) = lookup.context(&job.pane_id) {
                known.insert(job.pane_id.clone(), ctx);
            }
            let ctx = known.get(&job.pane_id).cloned().unwrap_or_default();
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
            let aside = path.with_extension(format!("bad-{}", now_ms()));
            std::fs::rename(path, &aside).map_err(|_| format!("chat index: {first}"))?;
            Store::open(path, cap).map_err(|error| format!("chat index: {error}"))
        }
    }
}
