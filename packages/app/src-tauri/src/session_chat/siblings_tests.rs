//! Two panes of one agent in one folder (safety net): each Chat shows only its own conversation.
//! The agent, its files and its words are made up; nothing here names a real CLI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;

use super::super::model::{ChatItem, SessionFormat};
use super::super::screen::tests::{register_probes, ECHO};
use super::super::screen::ScreenRead;
use super::super::source::{Located, PaneFacts, SessionSource};
use super::super::watch::{PaneLookup, Sink, Tuning, Watchers};
use super::super::{DeclaredSource, SessionChatTable};

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

/// What every agent's source does at heart: the newest `*.jsonl` of the folder. `screen_first`: a file that may
/// never come (a definition someone wrote), so the screen is read until it has something.
struct Newest {
    dir: PathBuf,
    screen_first: bool,
}

impl SessionSource for Newest {
    fn format(&self) -> &dyn SessionFormat {
        &LineFormat
    }

    fn locate(&self, _pane: &PaneFacts) -> Located {
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

    fn screen_until_found(&self) -> bool {
        self.screen_first
    }
}

fn newest(dir: &Path, screen_first: bool) -> &'static Newest {
    Box::leak(Box::new(Newest { dir: dir.to_path_buf(), screen_first }))
}

struct PaneSpec {
    cwd: PathBuf,
    pin: Option<String>,
    screen: String,
}

/// Several panes, each with its own facts and its own screen.
#[derive(Default)]
struct Panes {
    list: Mutex<HashMap<String, PaneSpec>>,
}

impl Panes {
    fn add(&self, id: &str, cwd: &Path, pin: Option<&str>, screen: &str) {
        self.list.lock().insert(id.to_string(), PaneSpec { cwd: cwd.to_path_buf(), pin: pin.map(str::to_string), screen: screen.to_string() });
    }

    fn pin(&self, id: &str, pin: &str) {
        self.list.lock().get_mut(id).unwrap().pin = Some(pin.to_string());
    }
}

impl PaneLookup for Panes {
    fn facts(&self, pane_id: &str) -> Option<PaneFacts> {
        let list = self.list.lock();
        let spec = list.get(pane_id)?;
        Some(PaneFacts { cwd: spec.cwd.clone(), started_at: SystemTime::now() - Duration::from_secs(30), provider_session_id: spec.pin.clone(), home: None })
    }

    fn screen(&self, pane_id: &str, _known: Option<u64>) -> Option<ScreenRead> {
        let text = self.list.lock().get(pane_id)?.screen.clone();
        Some(ScreenRead { status: "idle".into(), position: None, text: Some(text) })
    }

    fn other_sessions(&self, pane_id: &str) -> Vec<String> {
        self.list.lock().iter().filter(|(id, _)| id.as_str() != pane_id).filter_map(|(_, spec)| spec.pin.clone()).collect()
    }
}

struct Quiet;

impl Sink for Quiet {
    fn send(&self, _pane_id: &str, _items: Vec<ChatItem>, _reset: bool) {}
}

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("pm-siblings-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }

    /// Appends one user line to `name`, a moment after the last write (file times must differ).
    fn say(&self, name: &str, text: &str) {
        use std::io::Write;
        std::thread::sleep(Duration::from_millis(30));
        let mut file = std::fs::OpenOptions::new().append(true).create(true).open(self.0.join(name)).unwrap();
        writeln!(file, "u:{text}").unwrap();
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fast() -> Tuning {
    Tuning { poll: Duration::from_millis(10), screen_poll: Duration::from_millis(10), api_poll: Duration::from_millis(10), relocate: Duration::from_millis(10), ..Tuning::default() }
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The words of a pane's chat, in order.
fn said(watchers: &Watchers, pane: &str) -> Vec<String> {
    watchers
        .since(pane, None, 100)
        .map(|delta| delta.items)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| if let ChatItem::User { text, .. } = item { Some(text) } else { None })
        .collect()
}

fn rig(panes: Arc<Panes>) -> Watchers {
    register_probes();
    Watchers::with_tuning(Arc::new(Quiet), panes, fast())
}

fn screen_of(said: &str) -> String {
    format!("Probe CLI 1.4  model: small-1\n> {said}\nok\n")
}

fn settle() {
    std::thread::sleep(Duration::from_millis(150));
}

// ---- a declared agent (a definition someone wrote): the screen until the file is surely this pane's ----

const DECLARED: &str = r#"
format = "jsonl"
dir = "{cwd}/.sessions"

[map]
role = "/role"
text = "/text"
"#;

#[test]
fn a_declared_agent_alone_in_its_folder_moves_from_the_screen_to_its_file() {
    let folder = Folder::new();
    let table: SessionChatTable = toml::from_str(DECLARED).unwrap();
    let source = DeclaredSource::shared(&table).unwrap();
    std::fs::create_dir_all(folder.0.join(".sessions")).unwrap();
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, None, &screen_of("alpha on screen"));
    let watchers = rig(panes);
    watchers.open_screen_first("a", "w", source, ECHO);
    assert_eq!(watchers.source_of("a"), Some("screen"));
    std::fs::write(folder.0.join(".sessions").join("one.jsonl"), "{\"role\":\"user\",\"text\":\"alpha from file\"}\n").unwrap();
    wait_until("the lone pane moves to its file", || watchers.source_of("a") == Some("file"));
    assert_eq!(said(&watchers, "a"), ["alpha from file"]);
    watchers.close_all();
}

#[test]
fn two_declared_panes_in_one_folder_read_their_own_screens_not_the_file_one_of_them_wrote() {
    let folder = Folder::new();
    let table: SessionChatTable = toml::from_str(DECLARED).unwrap();
    let source = DeclaredSource::shared(&table).unwrap();
    let sessions = folder.0.join(".sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, None, &screen_of("alpha only for pane A"));
    panes.add("b", &folder.0, None, &screen_of("bravo only for pane B"));
    let watchers = rig(panes);
    watchers.open_screen_first("a", "w", source, ECHO);
    watchers.open_screen_first("b", "w", source, ECHO);

    // Pane A's agent writes its transcript. Nothing says whose it is: both panes keep reading their screens.
    std::fs::write(sessions.join("alpha.jsonl"), "{\"role\":\"user\",\"text\":\"alpha only for pane A\"}\n").unwrap();
    settle();
    assert_eq!(watchers.source_of("a"), Some("screen"));
    assert_eq!(watchers.source_of("b"), Some("screen"), "B must not switch to a file that may be A's");
    assert_eq!(said(&watchers, "a"), ["alpha only for pane A"]);
    assert_eq!(said(&watchers, "b"), ["bravo only for pane B"], "B shows B's words, never A's");
    watchers.close_all();
}

#[test]
fn a_declared_pane_opened_next_to_a_pane_on_its_file_keeps_to_its_screen_until_it_has_a_file_of_its_own() {
    let folder = Folder::new();
    let table: SessionChatTable = toml::from_str(DECLARED).unwrap();
    let source = DeclaredSource::shared(&table).unwrap();
    let sessions = folder.0.join(".sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, None, &screen_of("alpha only for pane A"));
    let watchers = rig(panes.clone());

    // A is alone: its file is its chat.
    std::fs::write(sessions.join("alpha.jsonl"), "{\"role\":\"user\",\"text\":\"alpha only for pane A\"}\n").unwrap();
    watchers.open_screen_first("a", "w", source, ECHO);
    assert_eq!(watchers.source_of("a"), Some("file"));

    // B opens in the same folder: the newest file is A's, which A follows. B stays on its screen.
    panes.add("b", &folder.0, None, &screen_of("bravo only for pane B"));
    watchers.open_screen_first("b", "w", source, ECHO);
    settle();
    assert_eq!(watchers.source_of("b"), Some("screen"));
    assert_eq!(said(&watchers, "b"), ["bravo only for pane B"]);

    // A keeps writing: still A's. B's agent writes its own file (now the newest): B takes that one.
    std::thread::sleep(Duration::from_millis(30));
    std::fs::write(sessions.join("bravo.jsonl"), "{\"role\":\"user\",\"text\":\"bravo from file\"}\n").unwrap();
    wait_until("B moves to its own file", || watchers.source_of("b") == Some("file"));
    assert_eq!(said(&watchers, "b"), ["bravo from file"]);
    assert_eq!(said(&watchers, "a"), ["alpha only for pane A"], "A's chat did not change");
    watchers.close_all();
}

#[test]
fn when_the_pane_next_to_it_closes_the_one_left_takes_the_file() {
    let folder = Folder::new();
    let table: SessionChatTable = toml::from_str(DECLARED).unwrap();
    let source = DeclaredSource::shared(&table).unwrap();
    let sessions = folder.0.join(".sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, None, &screen_of("alpha only for pane A"));
    panes.add("b", &folder.0, None, &screen_of("bravo only for pane B"));
    let watchers = rig(panes);
    watchers.open_screen_first("a", "w", source, ECHO);
    watchers.open_screen_first("b", "w", source, ECHO);
    std::fs::write(sessions.join("alpha.jsonl"), "{\"role\":\"user\",\"text\":\"alpha only for pane A\"}\n").unwrap();
    settle();
    assert_eq!(watchers.source_of("a"), Some("screen"));
    watchers.close("b", "w");
    wait_until("A, alone again, moves to its file", || watchers.source_of("a") == Some("file"));
    assert_eq!(said(&watchers, "a"), ["alpha only for pane A"]);
    watchers.close_all();
}

// ---- an agent whose pick is the newest file: a sibling's file is never taken ----

#[test]
fn a_pane_pinned_to_a_session_that_has_no_file_yet_does_not_show_the_file_of_the_pane_next_to_it() {
    let folder = Folder::new();
    let source = newest(&folder.0, false);
    folder.say("aaaaaaaa-1111.jsonl", "alpha in A's file");
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, Some("aaaaaaaa-1111"), "");
    panes.add("b", &folder.0, Some("bbbbbbbb-2222"), "");
    let watchers = rig(panes);
    watchers.open("a", "w", source);
    watchers.open("b", "w", source);
    settle();
    assert_eq!(said(&watchers, "a"), ["alpha in A's file"]);
    assert_eq!(said(&watchers, "b"), Vec::<String>::new(), "B has said nothing: its chat is empty, not A's");

    // B's agent writes its file: B shows it, A is unchanged.
    folder.say("bbbbbbbb-2222.jsonl", "bravo in B's file");
    wait_until("B switches to its own file", || said(&watchers, "b") == ["bravo in B's file"]);
    assert_eq!(said(&watchers, "a"), ["alpha in A's file"]);
    watchers.close_all();
}

#[test]
fn a_pane_without_a_session_id_does_not_take_the_file_another_open_chat_follows() {
    let folder = Folder::new();
    let source = newest(&folder.0, false);
    folder.say("a.jsonl", "alpha in A's file");
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, None, "");
    let watchers = rig(panes.clone());
    watchers.open("a", "w", source);
    assert_eq!(said(&watchers, "a"), ["alpha in A's file"]);

    panes.add("b", &folder.0, None, "");
    watchers.open("b", "w", source);
    settle();
    assert_eq!(said(&watchers, "b"), Vec::<String>::new(), "A's file is A's");
    folder.say("b.jsonl", "bravo in B's file");
    wait_until("B finds its own file", || said(&watchers, "b") == ["bravo in B's file"]);
    assert_eq!(said(&watchers, "a"), ["alpha in A's file"]);
    // When the agent reports B's session id later, nothing changes for either.
    panes.pin("b", "bbbbbbbb-2222");
    settle();
    assert_eq!(said(&watchers, "b"), ["bravo in B's file"]);
    watchers.close_all();
}

#[test]
fn a_pane_of_another_agent_in_the_same_folder_is_not_a_sibling() {
    let folder = Folder::new();
    let mine = newest(&folder.0, true);
    folder.say("one.jsonl", "from the file");
    let panes = Arc::new(Panes::default());
    panes.add("a", &folder.0, None, &screen_of("alpha on screen"));
    panes.add("b", &folder.0, None, &screen_of("bravo on screen"));
    let watchers = rig(panes);
    // `b` runs another agent (another source) that has not found a file: it is no rival for `a`.
    let empty = tempdir_without_files();
    let theirs = newest(&empty.0, true);
    watchers.open_screen_first("b", "w", theirs, ECHO);
    watchers.open_screen_first("a", "w", mine, ECHO);
    wait_until("A moves to its file", || watchers.source_of("a") == Some("file"));
    assert_eq!(said(&watchers, "a"), ["from the file"]);
    watchers.close_all();
}

fn tempdir_without_files() -> Folder {
    let dir = std::env::temp_dir().join(format!("pm-siblings-empty-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Folder(dir)
}
