//! The importer: brings the old chats of every agent that can list them into the search index. Generic, it
//! never names an agent. It asks a `Catalog` for the agents that have a `SessionHistory` (every registered profile,
//! built-in and from a TOML definition), lists what each has (`scan`, no full read) and, when the person chose,
//! reads the chats on a worker thread (`start`).
//!
//! - `plan`: the catalog over the profile registry, `scan`, and how a chat becomes an index row (the one place
//!   that builds rows, always at `Strict` strength).
//! - `run`: the worker thread: skips, reads in chunks, writes one chat per transaction, yields, stops.
//! - `progress`: what the screen shows and the throttled event that carries it.
//! - `prefs`: the person's choices, `<app data>/chat_import.json`.
//!
//! Nothing here reads a folder until `scan` or `start` is called with a `HistoryEnv`, and only the agents the person
//! chose are read. Counts and agent ids are the only things logged or sent: no path and no chat text.

mod plan;
mod prefs;
mod progress;
mod run;
#[cfg(test)]
mod fakes;
#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ChatIndex;
use crate::session_chat::history::{HistoryEnv, SessionHistory};

pub use plan::RegistryCatalog;
pub use prefs::{ImportPrefs, DEFAULT_SINCE_DAYS};
pub use progress::{Emitter, Phase, Progress, EMIT_EVERY};

/// Error codes of `Importer::start`.
pub const IMPORT_RUNNING: &str = "IMPORT_RUNNING";
pub const INDEX_OFF: &str = "INDEX_OFF";
pub const IMPORT_NOTHING_CHOSEN: &str = "IMPORT_NOTHING_CHOSEN";

/// The Tauri event the command layer sends the `Progress` on.
pub const PROGRESS_EVENT: &str = "chat-import-progress";
/// The importer sleeps this long after every chunk of items, so it never starves the screen or the live writer.
pub const YIELD_EVERY_CHUNK: Duration = Duration::from_millis(2);
/// The message of a run that stopped because chat search was turned off.
pub const MESSAGE_INDEX_OFF: &str = "Chat search was turned off";

/// The agents whose old chats can be read.
pub trait Catalog: Send + Sync {
    fn sources(&self) -> Vec<Source>;
}

/// One agent that can list its old chats. `id` is the agent id the app lists it under; `from_definition` is true for an
/// agent that a TOML definition added (it starts switched off on the screen).
#[derive(Clone)]
pub struct Source {
    pub id: String,
    pub label: String,
    pub from_definition: bool,
    pub history: &'static dyn SessionHistory,
}

/// What `scan` found for one agent: names, sizes and times only, nothing was read in full.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub id: String,
    pub label: String,
    pub from_definition: bool,
    pub chats: usize,
    pub bytes: u64,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
    /// Chats that are in the index already with the same stamp (a new run would not read them again).
    pub already: usize,
    pub too_big: usize,
    /// Where it looked, with the home folder as `~` (for the screen).
    pub places: Vec<String>,
    /// Whether the agent declares a resume option ("Continue in a pane" works for its chats).
    pub can_continue: bool,
}

/// What the person chose: the agents (by id) and how far back (`None` = all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRequest {
    pub agents: Vec<String>,
    pub since_days: Option<u32>,
}

/// State shared between the importer and its worker thread.
pub(super) struct Shared {
    pub(super) reporter: progress::Reporter,
    pub(super) cancel: AtomicBool,
    pub(super) running: AtomicBool,
}

/// Lists and imports the old chats of the agents in a `Catalog` into a `ChatIndex`. One import runs at a time.
pub struct Importer {
    catalog: Arc<dyn Catalog>,
    index: Arc<ChatIndex>,
    shared: Arc<Shared>,
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl Importer {
    /// `emit` gets the progress at most four times a second (and at every phase change); the command layer passes the
    /// Tauri emit.
    pub fn new(catalog: Arc<dyn Catalog>, index: Arc<ChatIndex>, emit: Emitter) -> Importer {
        let shared = Shared {
            reporter: progress::Reporter::new(emit),
            cancel: AtomicBool::new(false),
            running: AtomicBool::new(false),
        };
        Importer { catalog, index, shared: Arc::new(shared) }
    }

    /// The agents that can be read (what the screen lists), in registry order.
    pub fn sources(&self) -> Vec<Source> {
        self.catalog.sources()
    }

    /// What each agent has, from `list` only (each within its time budget). Reads no chat. Call it only after the
    /// person pressed "Find my chats".
    pub fn scan(&self, env: &HistoryEnv, since_days: Option<u32>) -> Vec<Found> {
        plan::scan(self.catalog.as_ref(), &self.index, env, since_days, now_ms())
    }

    /// Starts the worker for the chosen agents. Errors: `IMPORT_RUNNING` (one is running), `INDEX_OFF` (chat search is
    /// off), `IMPORT_NOTHING_CHOSEN` (no chosen agent is in the catalog). Ids that are not in the catalog are ignored.
    pub fn start(&self, req: ImportRequest, env: HistoryEnv) -> Result<(), String> {
        if self.shared.running.load(Ordering::SeqCst) {
            return Err(IMPORT_RUNNING.into());
        }
        if !self.index.enabled() {
            return Err(INDEX_OFF.into());
        }
        let sources = plan::chosen(self.catalog.sources(), &req.agents);
        if sources.is_empty() {
            return Err(IMPORT_NOTHING_CHOSEN.into());
        }
        if self.shared.running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(IMPORT_RUNNING.into());
        }
        self.shared.cancel.store(false, Ordering::SeqCst);
        self.shared.reporter.reset(Progress { phase: Phase::Looking, ..Progress::idle() });
        let job = run::Job { sources, since_ms: plan::since_ms(req.since_days, now_ms()), env };
        let (index, shared) = (self.index.clone(), self.shared.clone());
        let spawned = std::thread::Builder::new().name("chat-import".into()).spawn(move || run::run(&index, &shared, job));
        if spawned.is_err() {
            self.shared.running.store(false, Ordering::SeqCst);
            self.shared.reporter.reset(Progress {
                phase: Phase::Failed,
                message: Some("Could not start reading".into()),
                ..Progress::idle()
            });
            return Err("INTERNAL: chat import: could not start the worker".into());
        }
        Ok(())
    }

    /// Asks the running import to stop. It stops at the next chunk; the chat it was reading is dropped, chats already
    /// written stay whole. Does nothing when no import runs.
    pub fn cancel(&self) {
        if self.shared.running.load(Ordering::SeqCst) {
            self.shared.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Where the import is now (`idle` before the first one; the end state stays until the next `start`).
    pub fn progress(&self) -> Progress {
        self.shared.reporter.get()
    }
}
