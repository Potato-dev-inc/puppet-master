//! Keeps one reader thread per open pane. The thread finds the pane's session file, follows it, turns
//! new lines into chat items and hands them to a `Sink` (the Tauri event in the app, a fake in tests).
//! Chat text is never logged here; only pane ids and counts are.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use parking_lot::Mutex;

use super::api::{ApiEndpoint, ApiFollower, ChatFrom, SessionApi};
use super::claims::Claims;
use super::items::read_lines;
use super::model::ChatItem;
use super::screen::{ScreenFollower, ScreenRead};
use super::screen_first::ScreenFirstFollower;
use super::source::{Located, PaneFacts, SessionSource};
use super::store::{Delta, Holders, Items};
use super::tail::{Polled, Tail};

pub use super::store::Hold;

/// Most items kept per pane. Older ones are dropped, so a long session shows its tail.
pub const MAX_ITEMS: usize = 3000;

/// Where batches of items go. `reset`: drop everything shown before and show `items` instead.
pub trait Sink: Send + Sync {
    fn send(&self, pane_id: &str, items: Vec<ChatItem>, reset: bool);
    /// What a pane with nothing to show is waiting for changed (`None`: it is not waiting any more).
    fn waiting(&self, _pane_id: &str, _waiting_for: Option<&str>) {}
    /// Where a pane's chat is read from changed (`ChatFrom::name`): the API stopped answering, or answers again.
    fn source(&self, _pane_id: &str, _source: &str) {}
}

/// Sees every batch of items a pane's reader puts in its list (also the first look, which is not sent to the
/// `Sink`). It runs under the pane's items lock: it must only hand the items on, never wait on anything slow.
/// The chat search index is one (`chat_index`).
pub trait ItemObserver: Send + Sync {
    fn observe(&self, pane_id: &str, items: &[ChatItem]);
}

/// What the watcher asks about a pane: its facts, or `None` when the pane is gone.
pub trait PaneLookup: Send + Sync {
    fn facts(&self, pane_id: &str) -> Option<PaneFacts>;
    /// What the pane's screen shows, for a chat read from the screen (`None`: the pane is gone). `known`: the output
    /// position of the text read last; when the pane has printed nothing since, the answer carries no text.
    fn screen(&self, _pane_id: &str, _known: Option<u64>) -> Option<ScreenRead> {
        None
    }
    /// Where the pane's agent serves its session API (`None`: it does not, or not yet).
    fn api(&self, _pane_id: &str) -> Option<ApiEndpoint> {
        None
    }
    /// The agent session ids every other pane is pinned to (from their hooks): a file named after one of them is
    /// that pane's conversation, never this pane's.
    fn other_sessions(&self, _pane_id: &str) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    /// How often the file is read.
    pub poll: Duration,
    /// How often a pane's screen is looked at (the chat read from the screen).
    pub screen_poll: Duration,
    /// How often the agent's session API is asked (the whole conversation, so not as often as a file).
    pub api_poll: Duration,
    /// How often the pane's file is looked for again (to find it, and to follow a switch).
    pub relocate: Duration,
    pub max_items: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { poll: Duration::from_millis(400), screen_poll: Duration::from_millis(700), api_poll: Duration::from_secs(1), relocate: Duration::from_secs(1), max_items: MAX_ITEMS }
    }
}

/// What one look at the file changed.
#[derive(Debug, PartialEq)]
pub enum Change {
    Nothing,
    Append(Vec<ChatItem>),
    /// Items to add, or to put in place of the shown items with the same id (a screen read changes its items).
    Upsert(Vec<ChatItem>),
    /// The file was replaced or truncated: these items are everything there is.
    Reset(Vec<ChatItem>),
}

/// Finds a pane's file and reads what was added since the last step. No threads, no sleeping.
pub(super) struct Follower {
    source: &'static dyn SessionSource,
    file: Option<PathBuf>,
    tail: Option<Tail>,
    last_locate: Option<Instant>,
    /// The last answer was `Ambiguous`.
    ambiguous: bool,
    /// The pane's id and the table of who follows what (`Watchers` gives it; a bare follower in a test has none).
    claim: Option<(String, Arc<Claims>)>,
    /// The session ids other panes are pinned to (`PaneLookup::other_sessions`), as of the last look.
    others: Vec<String>,
}

impl Drop for Follower {
    fn drop(&mut self) {
        if let Some((pane, claims)) = &self.claim {
            claims.forget(pane);
        }
    }
}

impl Follower {
    /// The file this follower reads now (`None`: none found yet).
    pub(super) fn file(&self) -> Option<&std::path::Path> {
        self.file.as_deref()
    }

    pub(super) fn new(source: &'static dyn SessionSource) -> Self {
        Follower { source, file: None, tail: None, last_locate: None, ambiguous: false, claim: None, others: Vec::new() }
    }

    /// A follower that tells `claims` which file it follows, and leaves alone the file a sibling follows.
    pub(super) fn claiming(source: &'static dyn SessionSource, pane_id: &str, claims: Arc<Claims>) -> Self {
        let mut follower = Follower::new(source);
        follower.claim = Some((pane_id.to_string(), claims));
        follower
    }

    /// The session ids the other panes are pinned to, for the next `locate`.
    pub(super) fn set_others(&mut self, others: Vec<String>) {
        self.others = others;
    }

    /// Whether a file the source picked may be this pane's. A file this pane already follows, or one named after this
    /// pane's own session id, is. A file named after another pane's session id is not. A file another pane follows is
    /// not either; a pane that follows a file does not move to another while a sibling runs in its folder; and where a
    /// missing file leaves the screen to be read (`screen_until_found`) no file is taken for a pane without a session
    /// id while a sibling is still looking for its own: nothing tells whose it is. Such a pick is answered as
    /// `Ambiguous`, which keeps what the pane shows and asks again.
    fn vet(&self, located: Located, facts: &PaneFacts) -> Located {
        let Located::Found(path) = &located else { return located };
        if self.file.as_ref() == Some(path) {
            return located;
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let id = facts.provider_session_id.as_deref().filter(|id| !id.is_empty());
        if id.is_some_and(|id| name.contains(id)) {
            return located;
        }
        let foreign = self.others.iter().any(|other| other.len() >= 8 && id != Some(other.as_str()) && name.contains(other.as_str()));
        let claimed = self.claim.as_ref().is_some_and(|(pane, claims)| {
            let source = source_key(self.source);
            claims.held_by_other(pane, source, &facts.cwd, path)
                // Following one file, a newer one in a folder shared with a sibling is as likely the sibling's as a new session.
                || (self.file.is_some() && claims.has_sibling(pane, source, &facts.cwd))
                || (self.file.is_none() && id.is_none() && self.source.screen_until_found() && claims.rival_searching(pane, source, &facts.cwd))
        });
        if foreign || claimed {
            return Located::Ambiguous(vec![path.clone()]);
        }
        located
    }

    fn note_claim(&self, facts: &PaneFacts) {
        if let Some((pane, claims)) = &self.claim {
            claims.note(pane, source_key(self.source), &facts.cwd, self.file.as_deref());
        }
    }

    pub(super) fn step(&mut self, facts: &PaneFacts, now: Instant, relocate: Duration) -> Change {
        let mut switched = false;
        if !matches!(self.last_locate, Some(at) if now.duration_since(at) < relocate) {
            self.last_locate = Some(now);
            // Ambiguous and NotYet keep the file already followed; only a different file replaces it.
            let located = self.source.locate(facts);
            let located = self.vet(located, facts);
            self.ambiguous = matches!(located, Located::Ambiguous(_));
            if let Located::Found(path) = located {
                if self.file.as_ref() != Some(&path) {
                    switched = self.file.is_some();
                    self.tail = Some(Tail::new(path.clone()).with_cap(self.source.max_read_per_poll()));
                    self.file = Some(path);
                }
            }
        }
        self.note_claim(facts);
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

/// Which agent's source this is, for telling panes of one agent from panes of another.
fn source_key(source: &'static dyn SessionSource) -> usize {
    source as *const dyn SessionSource as *const () as usize
}

/// A folder as the person reads it: the home folder is written `~`, so a sentence on the screen (or in a shared
/// screenshot) does not carry the Windows user name.
fn tilde(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

impl Follower {
    /// In words, what a pane with nothing to show is waiting for. For the person, one short sentence.
    pub(super) fn waiting(&self, facts: &PaneFacts) -> String {
        let since: DateTime<Local> = facts.started_at.into();
        let since = since.format("%H:%M:%S");
        match &self.file {
            None if self.ambiguous => "Several files could be this pane's; waiting for one to stand out.".to_string(),
            None => match self.source.looking_in(facts) {
                Some(place) => format!("Looking in {} for a file written after {since}.", tilde(&place, crate::project_path::home_dir().as_deref())),
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

/// What a pane's chat is read from: its session file, its screen, or the agent's session API (the screen while the
/// API does not answer).
enum Reader {
    File(Follower),
    Screen(ScreenFollower),
    Api(ApiFollower),
    /// The screen until the session file has something to show, then the file.
    ScreenFirst(ScreenFirstFollower),
}

impl Reader {
    /// One look at the pane; `None` when the pane is gone.
    fn step(&mut self, panes: &dyn PaneLookup, pane_id: &str, now: Instant, relocate: Duration) -> Option<Change> {
        match self {
            Reader::File(follower) => {
                let facts = panes.facts(pane_id)?;
                follower.set_others(panes.other_sessions(pane_id));
                Some(follower.step(&facts, now, relocate))
            }
            Reader::Screen(follower) => follower.step(panes, pane_id),
            Reader::Api(follower) => follower.step(panes, pane_id),
            Reader::ScreenFirst(follower) => follower.step(panes, pane_id, now, relocate),
        }
    }

    /// The session file the items come from now (`None`: not read from a file, or no file found yet).
    fn followed_file(&self) -> Option<PathBuf> {
        match self {
            Reader::File(follower) => follower.file().map(PathBuf::from),
            Reader::ScreenFirst(follower) => follower.file_now().map(PathBuf::from),
            Reader::Screen(_) | Reader::Api(_) => None,
        }
    }

    /// Where the items come from now: `"file"`, `"screen"` or `"api"`.
    fn source(&self) -> &'static str {
        match self {
            Reader::File(_) => "file",
            Reader::Screen(_) => ChatFrom::Screen.name(),
            Reader::Api(follower) => follower.from().name(),
            Reader::ScreenFirst(follower) if follower.on_file() => "file",
            Reader::ScreenFirst(_) => ChatFrom::Screen.name(),
        }
    }

    fn waiting(&self, panes: &dyn PaneLookup, pane_id: &str) -> Option<String> {
        match self {
            Reader::File(follower) => Some(follower.waiting(&panes.facts(pane_id)?)),
            Reader::Screen(follower) => Some(follower.waiting()),
            Reader::Api(follower) => Some(follower.waiting()),
            Reader::ScreenFirst(follower) => follower.waiting(panes, pane_id),
        }
    }

    fn poll(&self, tuning: &Tuning) -> Duration {
        match self {
            Reader::File(_) => tuning.poll,
            Reader::Screen(_) => tuning.screen_poll,
            Reader::Api(follower) => match follower.from() {
                ChatFrom::Api => tuning.api_poll,
                ChatFrom::Screen => tuning.screen_poll,
            },
            Reader::ScreenFirst(follower) if follower.on_file() => tuning.poll,
            Reader::ScreenFirst(_) => tuning.screen_poll,
        }
    }
}

/// The items one pane shows, and the way to stop its thread.
struct Watch {
    items: Mutex<Items>,
    /// Who keeps the reader running; it ends when none is left.
    holders: Mutex<Holders>,
    /// What the pane is waiting for while it has no items (see `Follower::waiting`).
    waiting: Mutex<Option<String>>,
    /// Where the items come from (`Reader::source`); it changes for a reader that can fall back to the screen.
    source: Mutex<&'static str>,
    stop: AtomicBool,
    finished: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Watch {
    fn new() -> Self {
        Watch {
            items: Mutex::new(Items::new()),
            holders: Mutex::new(Holders::default()),
            waiting: Mutex::new(None),
            source: Mutex::new("file"),
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> Vec<ChatItem> {
        self.items.lock().all()
    }

    /// Adds (or renews) a holder. False when the reader already ended: the caller starts a new one.
    fn hold(&self, hold: Hold) -> bool {
        let mut holders = self.holders.lock();
        if self.finished.load(Ordering::SeqCst) {
            return false;
        }
        holders.add(hold, Instant::now());
        true
    }

    /// Lets a holder go; true when none is left.
    fn release(&self, holder: &str) -> bool {
        let mut holders = self.holders.lock();
        holders.remove(holder);
        holders.none_left(Instant::now())
    }

    /// Asked by the reader between looks: when no holder is left the reader ends (and a new holder starts another).
    fn unheld(&self) -> bool {
        let mut holders = self.holders.lock();
        let none = holders.none_left(Instant::now());
        if none {
            self.finished.store(true, Ordering::SeqCst);
        }
        none
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
    /// Which pane follows which file (see `claims`).
    claims: Arc<Claims>,
    /// Told of every item batch (the chat search index).
    observer: Option<Arc<dyn ItemObserver>>,
}

impl Watchers {
    pub fn new(sink: Arc<dyn Sink>, panes: Arc<dyn PaneLookup>) -> Self {
        Self::with_tuning(sink, panes, Tuning::default())
    }

    pub fn with_tuning(sink: Arc<dyn Sink>, panes: Arc<dyn PaneLookup>, tuning: Tuning) -> Self {
        Watchers { sink, panes, tuning, open: Mutex::new(HashMap::new()), alive: Arc::new(AtomicUsize::new(0)), claims: Arc::new(Claims::default()), observer: None }
    }

    /// Tells `observer` of every item the readers read from now on.
    pub fn with_observer(mut self, observer: Arc<dyn ItemObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Starts following the pane (once) and returns the items so far, read before this returns when the
    /// file is already there. Opening an open pane returns its current items and starts nothing.
    pub fn open<'a>(&self, pane_id: &str, hold: impl Into<Hold<'a>>, source: &'static dyn SessionSource) -> Vec<ChatItem> {
        self.open_reader(pane_id, hold.into(), Reader::File(Follower::claiming(source, pane_id, self.claims.clone())))
    }

    /// As `open`, for an agent that serves its session over an API: the chat is read from there, and from the
    /// pane's screen while the API does not answer.
    pub fn open_api<'a>(&self, pane_id: &str, hold: impl Into<Hold<'a>>, api: &'static dyn SessionApi, agent_type: &str) -> Vec<ChatItem> {
        self.open_reader(pane_id, hold.into(), Reader::Api(ApiFollower::new(api, agent_type)))
    }

    /// As `open`, for an agent whose file may never be found: the chat is read from the pane's screen until the file
    /// has something to show, and from the file after that.
    pub fn open_screen_first<'a>(&self, pane_id: &str, hold: impl Into<Hold<'a>>, source: &'static dyn SessionSource, agent_type: &str) -> Vec<ChatItem> {
        self.open_reader(pane_id, hold.into(), Reader::ScreenFirst(ScreenFirstFollower::new(Follower::claiming(source, pane_id, self.claims.clone()), agent_type)))
    }

    /// As `open`, for an agent with no session file: the chat is read from the pane's screen.
    pub fn open_screen<'a>(&self, pane_id: &str, hold: impl Into<Hold<'a>>, agent_type: &str) -> Vec<ChatItem> {
        self.open_reader(pane_id, hold.into(), Reader::Screen(ScreenFollower::new(agent_type)))
    }

    fn open_reader(&self, pane_id: &str, hold: Hold, mut reader: Reader) -> Vec<ChatItem> {
        let mut open = self.open.lock();
        if let Some(watch) = open.get(pane_id) {
            if watch.hold(hold) {
                return watch.snapshot();
            }
            open.remove(pane_id);
        }
        let watch = Arc::new(Watch::new());
        watch.hold(hold);
        // The first look happens here, so the caller gets the items in its answer and not as an event.
        let Some(first) = reader.step(&*self.panes, pane_id, Instant::now(), self.tuning.relocate) else {
            return Vec::new();
        };
        apply(&watch, first, reader.followed_file(), self.tuning.max_items, None, self.observer.as_deref().map(|o| (o, pane_id)));
        let snapshot = watch.snapshot();
        *watch.waiting.lock() = waiting_of(&watch, &reader, &*self.panes, pane_id);
        *watch.source.lock() = reader.source();

        let worker = Worker {
            pane_id: pane_id.to_string(),
            watch: watch.clone(),
            reader,
            sink: self.sink.clone(),
            panes: self.panes.clone(),
            tuning: self.tuning,
            alive: self.alive.clone(),
            observer: self.observer.clone(),
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

    /// Where the open pane's items come from now (`"file"`, `"screen"` or `"api"`); `None` when it is not open.
    pub fn source_of(&self, pane_id: &str) -> Option<&'static str> {
        self.open.lock().get(pane_id).map(|watch| *watch.source.lock())
    }

    /// What the pane is waiting for while it has nothing to show; `None` once it has items (or is not open).
    pub fn waiting_for(&self, pane_id: &str) -> Option<String> {
        self.open.lock().get(pane_id).and_then(|watch| watch.waiting.lock().clone())
    }

    /// What changed in the pane's items since revision `since` (`None`: the newest `cap`), `None` when the pane is
    /// not open. Asking does not keep the reader running; opening it with a holder does.
    pub fn since(&self, pane_id: &str, since: Option<u64>, cap: usize) -> Option<Delta> {
        let watch = self.open.lock().get(pane_id).cloned()?;
        let delta = watch.items.lock().since(since, cap);
        Some(delta)
    }

    /// `holder` lets go of the pane. The reader stops when nobody holds it any more (its thread ends within one
    /// poll; nothing waits for it). A holder that never held the pane changes nothing.
    pub fn close(&self, pane_id: &str, holder: &str) {
        let mut open = self.open.lock();
        if !open.get(pane_id).is_some_and(|watch| watch.release(holder)) {
            return;
        }
        if let Some(watch) = open.remove(pane_id) {
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
    reader: Reader,
    sink: Arc<dyn Sink>,
    panes: Arc<dyn PaneLookup>,
    tuning: Tuning,
    alive: Arc<AtomicUsize>,
    observer: Option<Arc<dyn ItemObserver>>,
}

impl Worker {
    fn run(mut self) {
        tracing::debug!(pane = %self.pane_id, "session chat: reader started");
        loop {
            std::thread::park_timeout(self.reader.poll(&self.tuning));
            if self.watch.stop.load(Ordering::SeqCst) || self.watch.unheld() {
                break;
            }
            let Some(change) = self.reader.step(&*self.panes, &self.pane_id, Instant::now(), self.tuning.relocate) else {
                break;
            };
            apply(&self.watch, change, self.reader.followed_file(), self.tuning.max_items, Some((&*self.sink, &self.pane_id)), self.observer.as_deref().map(|o| (o, self.pane_id.as_str())));
            let source = self.reader.source();
            {
                let mut known = self.watch.source.lock();
                if *known != source {
                    *known = source;
                    self.sink.source(&self.pane_id, source);
                }
            }
            let waiting = waiting_of(&self.watch, &self.reader, &*self.panes, &self.pane_id);
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
fn waiting_of(watch: &Watch, reader: &Reader, panes: &dyn PaneLookup, pane_id: &str) -> Option<String> {
    if watch.items.lock().is_empty() {
        reader.waiting(panes, pane_id)
    } else {
        None
    }
}

/// Puts a change into the pane's items (dropping the oldest past `max`) and sends it to the sink.
/// Sending happens under the items lock so a snapshot never misses or repeats a batch.
/// `file` is the session file the reader follows after this step; it is noted with the items, under the same lock.
fn apply(watch: &Watch, change: Change, file: Option<PathBuf>, max: usize, out: Option<(&dyn Sink, &str)>, observer: Option<(&dyn ItemObserver, &str)>) {
    let mut items = watch.items.lock();
    let upsert = matches!(change, Change::Upsert(_));
    let (batch, mut reset) = match change {
        Change::Nothing => return,
        Change::Append(new) | Change::Upsert(new) => (new, false),
        Change::Reset(new) => {
            items.clear();
            (new, true)
        }
    };
    items.set_file(file);
    let count = batch.len();
    if let Some((observer, pane_id)) = observer {
        observer.observe(pane_id, &batch);
    }
    for item in &batch {
        if upsert {
            // An item sent again under its id replaces the old copy where it stands.
            items.upsert(item);
        } else {
            items.push(item.clone());
        }
    }
    let dropped = items.len().saturating_sub(max);
    if dropped > 0 {
        items.drop_oldest(dropped);
    }
    // A batch bigger than the cap would leave the viewer with a gap before it: it gets the capped list instead.
    if count > max {
        reset = true;
    }
    let send = if reset { items.all() } else { batch };
    if let Some((sink, pane_id)) = out {
        tracing::debug!(pane = %pane_id, count, dropped, reset, "session chat: items");
        sink.send(pane_id, send, reset);
    }
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
