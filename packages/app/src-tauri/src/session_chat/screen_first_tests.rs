//! The chat of an agent whose file may never come: the screen until the file has something to show, then the file.
//! The source and the pane are made up; nothing here names a real CLI.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;

use super::*;
use crate::session_chat::model::SessionFormat;
use crate::session_chat::screen::tests::{register_probes, ECHO};
use crate::session_chat::screen::ScreenRead;
use crate::session_chat::source::{Located, PaneFacts, SessionSource};
use crate::session_chat::watch::{Sink, Tuning, Watchers};

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

/// A source whose answer the test sets.
struct Source {
    located: Mutex<Located>,
}

impl SessionSource for Source {
    fn format(&self) -> &dyn SessionFormat {
        &LineFormat
    }

    fn locate(&self, _pane: &PaneFacts) -> Located {
        self.located.lock().clone()
    }

    fn screen_until_found(&self) -> bool {
        true
    }
}

fn source(located: Located) -> &'static Source {
    Box::leak(Box::new(Source { located: Mutex::new(located) }))
}

struct Pane {
    text: Mutex<String>,
    gone: AtomicBool,
}

impl Pane {
    fn new(text: &str) -> Arc<Self> {
        Arc::new(Pane { text: Mutex::new(text.to_string()), gone: AtomicBool::new(false) })
    }
}

impl PaneLookup for Pane {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        (!self.gone.load(Ordering::SeqCst))
            .then(|| PaneFacts { cwd: PathBuf::from(r"C:\Users\you\proj"), started_at: SystemTime::now(), provider_session_id: None, home: None, launch: Default::default() })
    }

    fn screen(&self, _pane_id: &str, _known: Option<u64>) -> Option<ScreenRead> {
        if self.gone.load(Ordering::SeqCst) {
            return None;
        }
        Some(ScreenRead { status: "idle".into(), position: None, text: Some(self.text.lock().clone()) })
    }
}

#[derive(Default)]
struct Batches {
    sent: Mutex<Vec<(Vec<ChatItem>, bool)>>,
    sources: Mutex<Vec<String>>,
}

impl Sink for Batches {
    fn send(&self, _pane_id: &str, items: Vec<ChatItem>, reset: bool) {
        self.sent.lock().push((items, reset));
    }

    fn source(&self, _pane_id: &str, source: &str) {
        self.sources.lock().push(source.to_string());
    }
}

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("pm-screen-first-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn append(path: &Path, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

const BANNER: &str = "Probe CLI 1.4  model: small-1\n";

fn fast() -> Tuning {
    Tuning { poll: Duration::from_millis(10), screen_poll: Duration::from_millis(10), api_poll: Duration::from_millis(10), relocate: Duration::from_millis(10), ..Tuning::default() }
}

#[test]
fn with_no_file_the_chat_is_the_screen_and_it_stays_the_screen() {
    register_probes();
    let pane = Pane::new(&format!("{BANNER}> say hi\nhi"));
    let sink = Arc::new(Batches::default());
    let watchers = Watchers::with_tuning(sink.clone(), pane, fast());
    let src = source(Located::NotYet);

    let items = watchers.open_screen_first("p1", "w", src, ECHO);
    assert!(items.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text == "say hi")), "{items:?}");
    assert_eq!(watchers.source_of("p1"), Some("screen"));

    // The file is never found (NotYet, or Ambiguous): the screen keeps being the chat, and no source change is sent.
    std::thread::sleep(Duration::from_millis(80));
    *src.located.lock() = Located::Ambiguous(vec![PathBuf::from("a.jsonl"), PathBuf::from("b.jsonl")]);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(watchers.source_of("p1"), Some("screen"));
    assert!(sink.sources.lock().is_empty());
    watchers.close_all();
}

#[test]
fn a_file_that_is_found_but_still_empty_does_not_replace_the_screen() {
    register_probes();
    let folder = Folder::new();
    let path = folder.file("empty.jsonl");
    append(&path, "noise that is not a message\n");
    let pane = Pane::new(&format!("{BANNER}> say hi\nhi"));
    let sink = Arc::new(Batches::default());
    let watchers = Watchers::with_tuning(sink.clone(), pane, fast());
    let src = source(Located::Found(path));
    watchers.open_screen_first("p1", "w", src, ECHO);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(watchers.source_of("p1"), Some("screen"));
    assert!(sink.sources.lock().is_empty());
    watchers.close_all();
}

#[test]
fn when_the_file_appears_the_chat_switches_to_it_and_the_viewer_is_told() {
    register_probes();
    let folder = Folder::new();
    let path = folder.file("late.jsonl");
    let pane = Pane::new(&format!("{BANNER}> say hi\nhi"));
    let sink = Arc::new(Batches::default());
    let watchers = Watchers::with_tuning(sink.clone(), pane, fast());
    let src = source(Located::NotYet);

    let first = watchers.open_screen_first("p1", "w", src, ECHO);
    assert!(!first.is_empty());
    assert_eq!(watchers.source_of("p1"), Some("screen"));

    append(&path, "u:from the file\nu:and another\n");
    *src.located.lock() = Located::Found(path.clone());
    wait_until("the switch to the file", || watchers.source_of("p1") == Some("file"));
    assert_eq!(sink.sources.lock().as_slice(), ["file"]);
    let sent = sink.sent.lock();
    let (items, reset) = sent.last().unwrap();
    assert!(*reset, "the screen's items are replaced, not added to");
    let texts: Vec<&str> = items.iter().map(|item| if let ChatItem::User { text, .. } = item { text.as_str() } else { "?" }).collect();
    assert_eq!(texts, ["from the file", "and another"]);
    drop(sent);

    // From then on lines arrive as appends, and the screen is not looked at again.
    append(&path, "u:third\n");
    wait_until("the appended line", || sink.sent.lock().last().is_some_and(|(items, reset)| !reset && items.len() == 1));
    assert_eq!(watchers.source_of("p1"), Some("file"));
    assert_eq!(sink.sources.lock().as_slice(), ["file"]);
    watchers.close_all();
}

#[test]
fn a_file_that_is_there_when_the_chat_opens_is_the_chat_at_once() {
    register_probes();
    let folder = Folder::new();
    let path = folder.file("now.jsonl");
    append(&path, "u:already here\n");
    let pane = Pane::new(&format!("{BANNER}> say hi\nhi"));
    let watchers = Watchers::with_tuning(Arc::new(Batches::default()), pane, fast());
    let items = watchers.open_screen_first("p1", "w", source(Located::Found(path)), ECHO);
    assert!(matches!(items.as_slice(), [ChatItem::User { text, .. }] if text == "already here"));
    assert_eq!(watchers.source_of("p1"), Some("file"));
    watchers.close_all();
}
