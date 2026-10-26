//! Keeps one reader thread per open pane. The thread finds the pane's session file, follows it, turns
//! new lines into chat items and hands them to a `Sink` (the Tauri event in the app, a fake in tests).
//! Chat text is never logged here; only pane ids and counts are.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use parking_lot::Mutex;

use super::items::read_lines;
use super::model::ChatItem;
use super::source::{Located, PaneFacts, SessionSource};
use super::tail::{Polled, Tail};

/// Most items kept per pane. Older ones are dropped, so a long session shows its tail.
pub const MAX_ITEMS: usize = 3000;

/// Where batches of items go. `reset`: drop everything shown before and show `items` instead.
pub trait Sink: Send + Sync {
    fn send(&self, pane_id: &str, items: Vec<ChatItem>, reset: bool);
    /// What a pane with nothing to show is waiting for changed (`None`: it is not waiting any more).
    fn waiting(&self, _pane_id: &str, _waiting_for: Option<&str>) {}
}

/// What the watcher asks about a pane: its facts, or `None` when the pane is gone.
pub trait PaneLookup: Send + Sync {
    fn facts(&self, pane_id: &str) -> Option<PaneFacts>;
}

#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    /// How often the file is read.
    pub poll: Duration,
    /// How often the pane's file is looked for again (to find it, and to follow a switch).
    pub relocate: Duration,
    pub max_items: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { poll: Duration::from_millis(400), relocate: Duration::from_secs(1), max_items: MAX_ITEMS }
    }
}

/// What one look at the file changed.
#[derive(Debug, PartialEq)]
enum Change {
    Nothing,
    Append(Vec<ChatItem>),
    /// The file was replaced or truncated: these items are everything there is.
    Reset(Vec<ChatItem>),
}

/// Finds a pane's file and reads what was added since the last step. No threads, no sleeping.
struct Follower {
    source: &'static dyn SessionSource,
    file: Option<PathBuf>,
    tail: Option<Tail>,
    last_locate: Option<Instant>,
    /// The last answer was `Ambiguous`.
    ambiguous: bool,
}

impl Follower {
    fn new(source: &'static dyn SessionSource) -> Self {
        Follower { source, file: None, tail: None, last_locate: None, ambiguous: false }
    }

    fn step(&mut self, facts: &PaneFacts, now: Instant, relocate: Duration) -> Change {
        let mut switched = false;
        if !matches!(self.last_locate, Some(at) if now.duration_since(at) < relocate) {
            self.last_locate = Some(now);
            // Ambiguous and NotYet keep the file already followed; only a different file replaces it.
            let located = self.source.locate(facts);
            self.ambiguous = matches!(located, Located::Ambiguous(_));
            if let Located::Found(path) = located {
                if self.file.as_ref() != Some(&path) {
                    switched = self.file.is_some();
                    self.tail = Some(Tail::new(path.clone()));
                    self.file = Some(path);
                }
            }
        }
        let Some(tail) = self.tail.as_mut() else {
            return Change::Nothing;
        };
        let format = self.source.format();
        match tail.poll() {
            Ok(Polled::Reset(lines)) => Change::Reset(read_lines(format, &lines)),
            Ok(Polled::Lines(lines)) => {
                let items = read_lines(format, &lines);
                if switched {
                    Change::Reset(items)
                } else if items.is_empty() {
                    Change::Nothing
                } else {
                    Change::Append(items)
                }
            }
            Err(_) => Change::Nothing,
        }
    }
}

impl Follower {
    /// In words, what a pane with nothing to show is waiting for. For the person, one short sentence.
    fn waiting(&self, facts: &PaneFacts) -> String {
        let since: DateTime<Local> = facts.started_at.into();
        let since = since.format("%H:%M:%S");
        match &self.file {
            None if self.ambiguous => "Several files could be this pane's; waiting for one to stand out.".to_string(),
            None => match self.source.looking_in(facts) {
                Some(place) => format!("Looking in {} for a file written after {since}.", place.display()),
                None => format!("Looking for a file written after {since}."),
            },
            Some(path) => {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                if path.is_file() {
                    format!("Reading {name}: nothing to show in it yet.")
                } else {
                    format!("Waiting for {name} to be written.")
                }
            }
        }
    }
}

/// The items one pane shows, and the way to stop its thread.
struct Watch {
    items: Mutex<VecDeque<ChatItem>>,
    /// What the pane is waiting for while it has no items (see `Follower::waiting`).
    waiting: Mutex<Option<String>>,
    stop: AtomicBool,
    finished: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Watch {
    fn new() -> Self {
        Watch {
            items: Mutex::new(VecDeque::new()),
            waiting: Mutex::new(None),
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> Vec<ChatItem> {
        self.items.lock().iter().cloned().collect()
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.lock().as_ref() {
            thread.thread().unpark();
        }
    }
}

pub struct Watchers {
    sink: Arc<dyn Sink>,
    panes: Arc<dyn PaneLookup>,
    tuning: Tuning,
    open: Mutex<HashMap<String, Arc<Watch>>>,
    alive: Arc<AtomicUsize>,
}

impl Watchers {
    pub fn new(sink: Arc<dyn Sink>, panes: Arc<dyn PaneLookup>) -> Self {
        Self::with_tuning(sink, panes, Tuning::default())
    }

    pub fn with_tuning(sink: Arc<dyn Sink>, panes: Arc<dyn PaneLookup>, tuning: Tuning) -> Self {
        Watchers { sink, panes, tuning, open: Mutex::new(HashMap::new()), alive: Arc::new(AtomicUsize::new(0)) }
    }

    /// Starts following the pane (once) and returns the items so far, read before this returns when the
    /// file is already there. Opening an open pane returns its current items and starts nothing.
    pub fn open(&self, pane_id: &str, source: &'static dyn SessionSource) -> Vec<ChatItem> {
        let mut open = self.open.lock();
        if let Some(watch) = open.get(pane_id) {
            if !watch.finished.load(Ordering::SeqCst) {
                return watch.snapshot();
            }
            open.remove(pane_id);
        }
        let Some(facts) = self.panes.facts(pane_id) else {
            return Vec::new();
        };
        let mut follower = Follower::new(source);
        let watch = Arc::new(Watch::new());
        // The first look happens here, so the caller gets the items in its answer and not as an event.
        let first = follower.step(&facts, Instant::now(), self.tuning.relocate);
        apply(&watch, first, self.tuning.max_items, None);
        let snapshot = watch.snapshot();
        *watch.waiting.lock() = waiting_of(&watch, &follower, &facts);

        let worker = Worker {
            pane_id: pane_id.to_string(),
            watch: watch.clone(),
            follower,
            sink: self.sink.clone(),
            panes: self.panes.clone(),
            tuning: self.tuning,
            alive: self.alive.clone(),
        };
        self.alive.fetch_add(1, Ordering::SeqCst);
        match std::thread::Builder::new().name("session-chat".into()).spawn(move || worker.run()) {
            Ok(handle) => {
                *watch.thread.lock() = Some(handle);
                open.insert(pane_id.to_string(), watch);
            }
            Err(error) => {
                self.alive.fetch_sub(1, Ordering::SeqCst);
                tracing::warn!(pane = %pane_id, %error, "session chat: could not start the reader");
            }
        }
        snapshot
    }

    /// What the pane is waiting for while it has nothing to show; `None` once it has items (or is not open).
    pub fn waiting_for(&self, pane_id: &str) -> Option<String> {
        self.open.lock().get(pane_id).and_then(|watch| watch.waiting.lock().clone())
    }

    /// Stops following the pane. Its thread ends within one poll; nothing waits for it.
    pub fn close(&self, pane_id: &str) {
        if let Some(watch) = self.open.lock().remove(pane_id) {
            watch.stop();
        }
    }

    pub fn close_all(&self) {
        for (_, watch) in self.open.lock().drain() {
            watch.stop();
        }
    }

    /// Reader threads still running.
    #[cfg(test)]
    pub fn running(&self) -> usize {
        self.alive.load(Ordering::SeqCst)
    }
}

impl Drop for Watchers {
    fn drop(&mut self) {
        self.close_all();
    }
}

struct Worker {
    pane_id: String,
    watch: Arc<Watch>,
    follower: Follower,
    sink: Arc<dyn Sink>,
    panes: Arc<dyn PaneLookup>,
    tuning: Tuning,
    alive: Arc<AtomicUsize>,
}

impl Worker {
    fn run(mut self) {
        tracing::debug!(pane = %self.pane_id, "session chat: reader started");
        loop {
            std::thread::park_timeout(self.tuning.poll);
            if self.watch.stop.load(Ordering::SeqCst) {
                break;
            }
            let Some(facts) = self.panes.facts(&self.pane_id) else {
                break;
            };
            let change = self.follower.step(&facts, Instant::now(), self.tuning.relocate);
            apply(&self.watch, change, self.tuning.max_items, Some((&*self.sink, &self.pane_id)));
            let waiting = waiting_of(&self.watch, &self.follower, &facts);
            let mut known = self.watch.waiting.lock();
            if *known != waiting {
                self.sink.waiting(&self.pane_id, waiting.as_deref());
                *known = waiting;
            }
        }
        self.watch.finished.store(true, Ordering::SeqCst);
        self.alive.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!(pane = %self.pane_id, "session chat: reader stopped");
    }
}

/// What the pane is waiting for: only while it has nothing to show.
fn waiting_of(watch: &Watch, follower: &Follower, facts: &PaneFacts) -> Option<String> {
    watch.items.lock().is_empty().then(|| follower.waiting(facts))
}

/// Puts a change into the pane's items (dropping the oldest past `max`) and sends it to the sink.
/// Sending happens under the items lock so a snapshot never misses or repeats a batch.
fn apply(watch: &Watch, change: Change, max: usize, out: Option<(&dyn Sink, &str)>) {
    let mut items = watch.items.lock();
    let (batch, mut reset) = match change {
        Change::Nothing => return,
        Change::Append(new) => (new, false),
        Change::Reset(new) => {
            items.clear();
            (new, true)
        }
    };
    let count = batch.len();
    items.extend(batch.iter().cloned());
    let dropped = items.len().saturating_sub(max);
    if dropped > 0 {
        items.drain(..dropped);
    }
    // A batch bigger than the cap would leave the viewer with a gap before it: it gets the capped list instead.
    if count > max {
        reset = true;
    }
    let send = if reset { items.iter().cloned().collect() } else { batch };
    if let Some((sink, pane_id)) = out {
        tracing::debug!(pane = %pane_id, count, dropped, reset, "session chat: items");
        sink.send(pane_id, send, reset);
    }
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
