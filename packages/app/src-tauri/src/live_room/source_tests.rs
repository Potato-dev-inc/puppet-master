//! Which session's chat a Live Room may publish (SPEC-n32 4 #3). Two sessions of one agent write into one folder; the
//! agent, its files and its words are made up. The reader is the real one (`Watchers`), shared with the Chat tab.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;

use super::*;
use crate::session_chat::model::SessionFormat;
use crate::session_chat::watch::Tuning;
use crate::session_chat::{Located, PaneFacts, PaneLookup, SessionSource};

const A: &str = "aaaaaaaa-1111-4111-8111-111111111111";
const B: &str = "bbbbbbbb-2222-4222-8222-222222222222";
const PANE: &str = "pane-1";

/// Reads lines like `u:hello` as a user message.
struct LineFormat;

impl SessionFormat for LineFormat {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        match line.strip_prefix("u:") {
            Some(text) => vec![ChatItem::User { id: format!("{offset}:0"), at: None, text: text.to_string() }],
            None => Vec::new(),
        }
    }
}

/// What every agent's source does at heart: the file named after the pane's session id when there is one and it
/// exists, else the newest `*.jsonl` of the folder. `by_id` false: the worst case, the newest file even with an id.
struct Folderwise {
    dir: PathBuf,
    by_id: bool,
}

impl SessionSource for Folderwise {
    fn format(&self) -> &dyn SessionFormat {
        &LineFormat
    }

    fn locate(&self, pane: &PaneFacts) -> Located {
        if let Some(id) = pane.provider_session_id.as_deref().filter(|_| self.by_id) {
            let own = self.dir.join(format!("{id}.jsonl"));
            if own.is_file() {
                return Located::Found(own);
            }
        }
        let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        files.sort();
        files.pop().map_or(Located::NotYet, |(_, path)| Located::Found(path))
    }
}

fn source(dir: &Path, by_id: bool) -> &'static Folderwise {
    Box::leak(Box::new(Folderwise { dir: dir.to_path_buf(), by_id }))
}

/// The one pane: its folder, and the session id its hooks reported (the test sets it).
struct Pane {
    cwd: PathBuf,
    session: Mutex<Option<String>>,
}

impl PaneLookup for Pane {
    fn facts(&self, pane_id: &str) -> Option<PaneFacts> {
        (pane_id == PANE).then(|| PaneFacts {
            cwd: self.cwd.clone(),
            started_at: SystemTime::now() - Duration::from_secs(30),
            provider_session_id: self.session.lock().clone(),
            home: None,
            launch: Default::default(),
        })
    }
}

struct Quiet;

impl crate::session_chat::Sink for Quiet {
    fn send(&self, _pane_id: &str, _items: Vec<ChatItem>, _reset: bool) {}
}

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-room-pin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }

    /// Appends a user line to `<session>.jsonl`, a moment after the last write (file times must differ).
    fn say(&self, session: &str, text: &str) {
        use std::io::Write;
        std::thread::sleep(Duration::from_millis(30));
        let mut file = std::fs::OpenOptions::new().append(true).create(true).open(self.0.join(format!("{session}.jsonl"))).unwrap();
        writeln!(file, "u:{text}").unwrap();
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Rig {
    watchers: Watchers,
    pane: Arc<Pane>,
    pins: Pins,
    _folder: Folder,
    opening_source: &'static Folderwise,
}

impl Rig {
    fn new(name: &str, by_id: bool, session: Option<&str>) -> Self {
        let folder = Folder::new(name);
        let pane = Arc::new(Pane { cwd: folder.0.clone(), session: Mutex::new(session.map(str::to_string)) });
        let tuning = Tuning {
            poll: Duration::from_millis(10),
            screen_poll: Duration::from_millis(10),
            api_poll: Duration::from_millis(10),
            relocate: Duration::from_millis(10),
            ..Tuning::default()
        };
        let watchers = Watchers::with_tuning(Arc::new(Quiet), pane.clone(), tuning);
        let opening_source = source(&folder.0, by_id);
        Rig { watchers, pane, pins: Pins::default(), _folder: folder, opening_source }
    }

    fn folder(&self) -> &Folder {
        &self._folder
    }

    /// Opens the pane's reader the way the Chat tab does (a shared reader).
    fn open(&self) {
        self.watchers.open(PANE, "chat-tab", self.opening_source);
    }

    fn set_session(&self, id: Option<&str>) {
        *self.pane.session.lock() = id.map(str::to_string);
    }

    /// What a room would take now: the share verdict, and the words of the delta.
    fn room_reads(&mut self) -> (ChatShare, Vec<String>) {
        let session = self.pane.session.lock().clone();
        let (share, delta) = read_chat(&mut self.pins, &self.watchers, PANE, session.as_deref(), true, None, 400);
        (share, delta.map(|d| words(&d.items)).unwrap_or_default())
    }
}

fn words(items: &[ChatItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            ChatItem::User { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What the Chat tab shows for the pane.
fn chat_tab(rig: &Rig) -> Vec<String> {
    rig.watchers.since(PANE, None, 400).map(|d| words(&d.items)).unwrap_or_default()
}

#[test]
fn a_pane_pinned_to_one_session_shares_only_that_session_even_when_another_writes_later() {
    let mut rig = Rig::new("pinned", true, Some(A));
    rig.folder().say(A, "mine one");
    rig.open();
    let (share, seen) = rig.room_reads();
    assert_eq!(share, ChatShare::Shared);
    assert_eq!(seen, vec!["mine one"]);

    // Another session of the same agent starts writing in the same folder, later.
    rig.folder().say(B, "someone else, private");
    rig.folder().say(A, "mine two");
    wait_until("the reader sees both of mine", || chat_tab(&rig).contains(&"mine two".to_string()));
    let (share, seen) = rig.room_reads();
    assert_eq!(share, ChatShare::Shared);
    assert_eq!(seen, vec!["mine one", "mine two"]);
    assert!(!seen.iter().any(|text| text.contains("someone else")));
}

#[test]
fn a_reader_that_follows_the_other_sessions_file_shares_nothing() {
    // The worst case: the reader picks the newest file whatever the pane's id says (an agent whose source cannot
    // tell). The room still publishes nothing, because the file is not named after the pane's session.
    let mut rig = Rig::new("worst", false, Some(A));
    rig.folder().say(A, "mine");
    rig.folder().say(B, "someone else, private");
    rig.open();
    assert!(chat_tab(&rig).iter().any(|text| text.contains("someone else")), "the reader did follow the newest file");
    let (share, seen) = rig.room_reads();
    assert_eq!(share, ChatShare::NotPinned);
    assert!(seen.is_empty(), "nothing of the other session: {seen:?}");
}

#[test]
fn a_pane_with_no_known_session_shares_no_chat_even_though_its_chat_tab_shows_one() {
    let mut rig = Rig::new("unpinned", true, None);
    rig.folder().say(A, "older session");
    rig.folder().say(B, "newest session, not this pane's");
    rig.open();
    assert!(!chat_tab(&rig).is_empty(), "the Chat tab follows the newest file of the folder");
    for _ in 0..3 {
        let (share, seen) = rig.room_reads();
        assert_eq!(share, ChatShare::NotPinned);
        assert!(seen.is_empty(), "{seen:?}");
    }
}

#[test]
fn a_pane_pinned_late_shares_its_own_session_and_never_what_the_reader_followed_before() {
    let mut rig = Rig::new("late", true, None);
    rig.folder().say(A, "mine");
    rig.folder().say(B, "someone else, private");
    rig.open();
    assert_eq!(rig.room_reads().0, ChatShare::NotPinned);

    // The pane's hook reports its session: the reader moves to that file.
    rig.set_session(Some(A));
    wait_until("the reader moves to the pinned file", || chat_tab(&rig) == vec!["mine"]);
    let (share, seen) = rig.room_reads();
    assert_eq!(share, ChatShare::Shared);
    assert_eq!(seen, vec!["mine"]);
}

#[test]
fn when_the_session_changes_the_room_stops_sharing_that_pane_for_good() {
    let mut rig = Rig::new("changed", true, Some(A));
    rig.folder().say(A, "mine");
    rig.open();
    assert_eq!(rig.room_reads(), (ChatShare::Shared, vec!["mine".to_string()]));

    // The pane starts a new session (a clear): its hook reports another id, and its new file fills.
    rig.folder().say(B, "after the clear");
    rig.set_session(Some(B));
    wait_until("the reader moves on", || chat_tab(&rig) == vec!["after the clear"]);
    for _ in 0..3 {
        let (share, seen) = rig.room_reads();
        assert_eq!(share, ChatShare::SessionChanged);
        assert!(seen.is_empty(), "{seen:?}");
    }
    // Even if the id comes back, this room does not share the pane again.
    rig.set_session(Some(A));
    assert_eq!(rig.room_reads().0, ChatShare::SessionChanged);
}

#[test]
fn a_pane_that_is_not_open_yet_is_judged_by_its_id_alone() {
    let mut rig = Rig::new("notopen", true, None);
    assert_eq!(rig.room_reads().0, ChatShare::NotPinned);
    let mut rig = Rig::new("notopen2", true, Some(A));
    assert_eq!(rig.room_reads(), (ChatShare::Shared, Vec::new()));
}
