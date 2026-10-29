//! Test doubles for the importer: a fake agent store, a fake catalog, and an index on an in-memory database.
//! Nothing here touches a folder.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::{Catalog, Emitter, Importer, Phase, Progress, Source};
use crate::chat_index::store::Store;
use crate::chat_index::{ChatIndex, ContextLookup, PaneContext};
use crate::session_chat::history::{
    HistoryChat, HistoryEnv, ListLimits, Listing, ReadError, ReadLimits, ReadOutcome, SessionHistory,
};
use crate::session_chat::{ChatItem, ToolDetail};

/// Called with the session id when a read of it begins.
type ReadHook = Box<dyn Fn(&str) + Send + Sync>;

/// An agent store in memory. It counts what is asked of it, so a test can prove what was (not) opened.
pub(super) struct FakeHistory {
    pub chats: Mutex<Vec<HistoryChat>>,
    pub items: Mutex<HashMap<String, Vec<ChatItem>>>,
    pub errors: Mutex<HashMap<String, ReadError>>,
    pub panics: Mutex<Vec<String>>,
    pub lists: AtomicUsize,
    pub reads: AtomicUsize,
    pub read_sessions: Mutex<Vec<String>>,
    pub last_limits: Mutex<Option<ListLimits>>,
    /// Items per chunk handed to the sink.
    pub chunk: usize,
    /// Sleep before each chunk (a slow store).
    pub delay: Duration,
    pub on_read: Mutex<Option<ReadHook>>,
    pub resume: bool,
}

impl FakeHistory {
    pub fn new() -> Self {
        FakeHistory {
            chats: Mutex::new(Vec::new()),
            items: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            panics: Mutex::new(Vec::new()),
            lists: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            read_sessions: Mutex::new(Vec::new()),
            last_limits: Mutex::new(None),
            chunk: 2,
            delay: Duration::ZERO,
            on_read: Mutex::new(None),
            resume: true,
        }
    }

    /// Adds a chat with a conversation about `words`.
    pub fn add(&self, session: &str, stamp: &str, updated_ms: i64, words: &str) {
        self.chats.lock().push(chat(session, stamp, updated_ms));
        self.items.lock().insert(session.to_string(), conversation(words));
    }

    pub fn set_stamp(&self, session: &str, stamp: &str) {
        for chat in self.chats.lock().iter_mut().filter(|chat| chat.session == session) {
            chat.stamp = stamp.to_string();
        }
    }

    pub fn leak(self) -> &'static FakeHistory {
        Box::leak(Box::new(self))
    }

    pub fn read_count(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl SessionHistory for FakeHistory {
    fn list(&self, _env: &HistoryEnv, limits: &ListLimits) -> Listing {
        self.lists.fetch_add(1, Ordering::SeqCst);
        *self.last_limits.lock() = Some(limits.clone());
        let mut chats = self.chats.lock().clone();
        chats.sort_by_key(|chat| std::cmp::Reverse(chat.updated_ms));
        Listing { chats, ..Listing::default() }
    }

    fn read(
        &self,
        _env: &HistoryEnv,
        session: &str,
        _limits: &ReadLimits,
        sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
    ) -> Result<ReadOutcome, ReadError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.read_sessions.lock().push(session.to_string());
        if let Some(hook) = self.on_read.lock().as_ref() {
            hook(session);
        }
        if self.panics.lock().iter().any(|id| id == session) {
            panic!("fake store blew up");
        }
        if let Some(error) = self.errors.lock().get(session) {
            return Err(*error);
        }
        let items = self.items.lock().get(session).cloned().ok_or(ReadError::Missing)?;
        let mut sent = 0;
        for part in items.chunks(self.chunk.max(1)) {
            std::thread::sleep(self.delay);
            sent += part.len();
            if !sink(part.to_vec()) {
                return Ok(ReadOutcome { items: sent, cut_short: true });
            }
        }
        Ok(ReadOutcome { items: sent, cut_short: false })
    }

    fn resume_args(&self, session: &str) -> Option<Vec<String>> {
        self.resume.then(|| vec!["--resume".to_string(), session.to_string()])
    }

    fn places(&self, _env: &HistoryEnv) -> Vec<String> {
        vec!["~/.fake/chats".to_string()]
    }
}

pub(super) fn chat(session: &str, stamp: &str, updated_ms: i64) -> HistoryChat {
    HistoryChat { session: session.into(), cwd: Some("~/projects/shop".into()), updated_ms, bytes: 100, stamp: stamp.into() }
}

/// A user question, an assistant answer, a tool call (kept) and its result (not kept), about `words`.
pub(super) fn conversation(words: &str) -> Vec<ChatItem> {
    vec![
        ChatItem::User { id: "0:0".into(), at: Some("2026-08-14T10:00:00Z".into()), text: format!("question about {words}") },
        ChatItem::Assistant {
            id: "10:0".into(),
            at: Some("2026-08-14T10:00:01Z".into()),
            text: format!("answer about {words}"),
            model: None,
            final_answer: true,
        },
        ChatItem::ToolCall {
            id: "20:0".into(),
            at: None,
            call_id: "c1".into(),
            name: "Search".into(),
            summary: format!("look for {words}"),
            detail: ToolDetail::default(),
        },
        ChatItem::ToolResult { id: "30:0".into(), at: None, call_id: "c1".into(), ok: true, output: "TOOLOUTPUTMARKER".into() },
    ]
}

pub(super) struct FakeCatalog(pub Vec<Source>);

impl Catalog for FakeCatalog {
    fn sources(&self) -> Vec<Source> {
        self.0.clone()
    }
}

pub(super) fn source(id: &str, from_definition: bool, history: &'static FakeHistory) -> Source {
    Source { id: id.into(), label: id.into(), from_definition, history }
}

/// Every pane is unknown: live items are written with empty labels.
pub(super) struct NoPanes;

impl ContextLookup for NoPanes {
    fn context(&self, _pane_id: &str) -> Option<PaneContext> {
        None
    }
}

/// An index over an in-memory store with its own switch (not the process-wide one, so tests do not interfere).
pub(super) fn memory_index() -> (Arc<ChatIndex>, Arc<AtomicBool>) {
    let switch = Arc::new(AtomicBool::new(true));
    (ChatIndex::start_switched(Store::open_memory(1000).unwrap(), Arc::new(NoPanes), switch.clone()), switch)
}

/// An importer over `sources` and `index`, and the list of everything it emitted.
pub(super) fn importer(sources: Vec<Source>, index: &Arc<ChatIndex>) -> (Arc<Importer>, Arc<Mutex<Vec<Progress>>>) {
    let seen: Arc<Mutex<Vec<Progress>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let emit: Emitter = Arc::new(move |progress: &Progress| sink.lock().push(progress.clone()));
    (Arc::new(Importer::new(Arc::new(FakeCatalog(sources)), index.clone(), emit)), seen)
}

/// Like `importer`, but the progress throttle reads `clock` instead of the machine's time.
pub(super) fn importer_on_clock(sources: Vec<Source>, index: &Arc<ChatIndex>, clock: super::progress::Clock) -> (Arc<Importer>, Arc<Mutex<Vec<Progress>>>) {
    let seen: Arc<Mutex<Vec<Progress>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let emit: Emitter = Arc::new(move |progress: &Progress| sink.lock().push(progress.clone()));
    let reporter = super::progress::Reporter::with_clock(emit, clock);
    (Arc::new(Importer::with_reporter(Arc::new(FakeCatalog(sources)), index.clone(), reporter)), seen)
}

/// A clock that only moves when it is read: each look at it is `tick` later than the one before.
pub(super) fn ticking_clock(tick: Duration) -> super::progress::Clock {
    let start = Instant::now();
    let reads = std::sync::atomic::AtomicU32::new(0);
    Arc::new(move || start + tick * reads.fetch_add(1, Ordering::SeqCst))
}

/// A hang detector, not a time limit: on a machine running the whole suite an import of a few chats took 20 s
/// (measured pinned to one core among 12 busy processes).
pub(super) fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Waits for the run to end and returns how it ended.
pub(super) fn finished(importer: &Importer) -> Progress {
    wait_until("the import to end", || importer.progress().phase.is_finished() && !importer.shared.running.load(Ordering::SeqCst));
    importer.progress()
}

pub(super) fn is_reading(importer: &Importer) -> bool {
    importer.progress().phase == Phase::Reading
}
