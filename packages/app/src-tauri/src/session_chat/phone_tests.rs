use super::*;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use parking_lot::Mutex;

use crate::session_chat::model::SessionFormat;
use crate::session_chat::source::{Located, PaneFacts, SessionSource};
use crate::session_chat::watch::{PaneLookup, Sink, Tuning};

/// A token-shaped string; nothing real.
const SECRET: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

/// Lines: `u:` the person, `a:` the agent, `t:` a tool call, `o:` its output.
struct Lines;

impl SessionFormat for Lines {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        let id = format!("{offset}:0");
        let (kind, text) = line.split_once(':').unwrap_or(("", ""));
        let text = text.to_string();
        match kind {
            "u" => vec![ChatItem::User { id, at: None, text }],
            "a" => vec![ChatItem::Assistant { id, at: None, text, model: None, final_answer: false }],
            "t" => vec![ChatItem::ToolCall { id, at: None, call_id: format!("c{offset}"), name: "shell".into(), summary: text, detail: Default::default() }],
            "o" => vec![ChatItem::ToolResult { id, at: None, call_id: "c0".into(), ok: true, output: text }],
            _ => Vec::new(),
        }
    }
}

struct Source {
    located: Mutex<Located>,
}

impl SessionSource for Source {
    fn format(&self) -> &dyn SessionFormat {
        &Lines
    }

    fn locate(&self, _pane: &PaneFacts) -> Located {
        self.located.lock().clone()
    }

    fn looking_in(&self, _pane: &PaneFacts) -> Option<PathBuf> {
        Some(PathBuf::from(r"C:\Users\you\sessions\proj"))
    }
}

struct Quiet;

impl Sink for Quiet {
    fn send(&self, _pane_id: &str, _items: Vec<ChatItem>, _reset: bool) {}
}

struct Pane;

impl PaneLookup for Pane {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        Some(PaneFacts { cwd: PathBuf::from(r"C:\Users\you\proj"), started_at: SystemTime::now(), provider_session_id: None, home: None, launch: Default::default() })
    }
}

struct Rig {
    watchers: Watchers,
    source: &'static Source,
    opening: Opening,
    dir: PathBuf,
}

impl Rig {
    fn new(located: Located) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("pm-phone-chat-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tuning = Tuning { poll: Duration::from_millis(10), screen_poll: Duration::from_millis(10), api_poll: Duration::from_millis(10), relocate: Duration::from_millis(10), max_items: 3000 };
        let source: &'static Source = Box::leak(Box::new(Source { located: Mutex::new(located) }));
        Rig { watchers: Watchers::with_tuning(Arc::new(Quiet), Arc::new(Pane), tuning), source, opening: Opening::File(source), dir }
    }

    fn with_file(lines: &str) -> (Self, PathBuf) {
        let rig = Rig::new(Located::NotYet);
        let path = rig.dir.join("a.jsonl");
        append(&path, lines);
        *rig.source.located.lock() = Located::Found(path.clone());
        (rig, path)
    }

    fn read(&self, since: Option<u64>) -> Value {
        read(&self.watchers, "p1", Some(&self.opening), since)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.watchers.close_all();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn append(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn texts(reply: &Value) -> Vec<String> {
    reply["items"].as_array().unwrap().iter().map(|item| item["text"].as_str().unwrap_or_default().to_string()).collect()
}

#[test]
fn the_first_answer_has_the_items_a_revision_and_no_reset() {
    let (rig, _) = Rig::with_file("u:hello\na:hi there\n");
    let reply = rig.read(None);
    assert_eq!(reply["available"], true);
    assert_eq!(reply["source"], "file");
    assert_eq!(reply["reset"], false);
    assert_eq!(texts(&reply), ["hello", "hi there"]);
    assert!(reply["rev"].as_u64().unwrap() > 0);
    assert_eq!(reply["items"][0]["kind"], "user");
}

#[test]
fn the_first_answer_has_at_most_the_last_300_items() {
    let lines: String = (0..350).map(|n| format!("u:m{n}\n")).collect();
    let (rig, _) = Rig::with_file(&lines);
    let reply = rig.read(None);
    let texts = texts(&reply);
    assert_eq!(texts.len(), MAX_ITEMS);
    assert_eq!(texts.first().unwrap(), "m50");
    assert_eq!(texts.last().unwrap(), "m349");
    assert_eq!(reply["reset"], false);
}

#[test]
fn since_returns_only_what_is_new_and_nothing_when_nothing_is() {
    let (rig, path) = Rig::with_file("u:one\n");
    let first = rig.read(None);
    let rev = first["rev"].as_u64().unwrap();
    let quiet = rig.read(Some(rev));
    assert!(texts(&quiet).is_empty());
    assert_eq!(quiet["rev"].as_u64().unwrap(), rev);
    assert_eq!(quiet["reset"], false);

    append(&path, "u:two\nu:three\n");
    wait_until("the new lines", || texts(&rig.read(Some(rev))) == ["two", "three"]);
    let next = rig.read(Some(rev));
    assert!(next["rev"].as_u64().unwrap() > rev);
    assert_eq!(next["reset"], false);
    // Asking again from the new revision gives nothing.
    assert!(texts(&rig.read(next["rev"].as_u64())).is_empty());
}

#[test]
fn a_replaced_file_is_a_reset_with_the_new_items() {
    let (rig, _) = Rig::with_file("u:old one\nu:old two\n");
    let rev = rig.read(None)["rev"].as_u64().unwrap();
    let other = rig.dir.join("b.jsonl");
    append(&other, "u:fresh\n");
    *rig.source.located.lock() = Located::Found(other);
    wait_until("the reset", || rig.read(Some(rev))["reset"] == true);
    let reply = rig.read(Some(rev));
    assert_eq!(texts(&reply), ["fresh"]);
    // A phone that saw the new list is not told to reset again.
    assert_eq!(rig.read(reply["rev"].as_u64())["reset"], false);
}

#[test]
fn a_revision_from_before_the_reader_started_is_a_reset() {
    let (rig, _) = Rig::with_file("u:one\n");
    let reply = rig.read(Some(5));
    assert_eq!(reply["reset"], true);
    assert_eq!(texts(&reply), ["one"]);
}

#[test]
fn every_text_that_leaves_is_redacted() {
    let lines = format!("u:my key is {SECRET}\na:use {SECRET} carefully\nt:curl -H {SECRET}\no:token={SECRET}\n");
    let (rig, _) = Rig::with_file(&lines);
    let reply = rig.read(None);
    let all = reply.to_string();
    assert!(!all.contains(SECRET), "a token-shaped secret left in: {all}");
    assert!(!all.contains("abcdefghijklmnopqrstuvwxyz0123456789"), "{all}");
    assert_eq!(reply["items"].as_array().unwrap().len(), 4);
    assert!(texts(&reply)[0].starts_with("my key is "));
}

#[test]
fn a_chat_with_nothing_yet_says_so_in_a_plain_sentence_without_folders() {
    let rig = Rig::new(Located::NotYet);
    let reply = rig.read(None);
    assert_eq!(reply["available"], true);
    assert_eq!(reply["waitingFor"], WAITING);
    let all = reply.to_string();
    assert!(!all.contains("sessions") && !all.contains("Users"), "{all}");
    // Once there are items it waits for nothing.
    let path = rig.dir.join("late.jsonl");
    append(&path, "u:hi\n");
    *rig.source.located.lock() = Located::Found(path);
    wait_until("the first item", || !texts(&rig.read(None)).is_empty());
    assert!(rig.read(None)["waitingFor"].is_null());
}

#[test]
fn a_pane_without_an_agent_gets_a_refusal_and_no_reader() {
    let rig = Rig::new(Located::NotYet);
    let reply = read(&rig.watchers, "p1", None, None);
    assert_eq!(reply["available"], false);
    assert_eq!(reply["reason"], NOT_AGENT);
    assert_eq!(reply["items"].as_array().unwrap().len(), 0);
    assert_eq!(rig.watchers.running(), 0);
    assert!(Opening::for_agent("powershell").is_none());
}

#[test]
fn a_pane_that_is_gone_is_unavailable() {
    struct Nobody;
    impl PaneLookup for Nobody {
        fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
            None
        }
    }
    let rig = Rig::new(Located::NotYet);
    let watchers = Watchers::new(Arc::new(Quiet), Arc::new(Nobody));
    let reply = read(&watchers, "p1", Some(&rig.opening), None);
    assert_eq!(reply["available"], false);
    assert_eq!(reply["reason"], GONE);
    assert_eq!(watchers.running(), 0);
}

#[test]
fn the_reader_ends_when_the_phone_stops_asking_and_stays_while_it_asks() {
    let (rig, _) = Rig::with_file("u:one\n");
    // While the phone asks, the lease is far longer than any pause a busy machine puts between two requests (a
    // 150 ms lease with a request every 60 ms ran out between two asks when the test was starved). How a renewal
    // moves the end of a lease is checked on a clock the test sets, in `store_tests`.
    let lasting = Duration::from_secs(120);
    read_with(&rig.watchers, "p1", Some(&rig.opening), None, lasting);
    assert_eq!(rig.watchers.running(), 1);
    // Asking again keeps the one reader (it does not start a second).
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(60));
        read_with(&rig.watchers, "p1", Some(&rig.opening), None, lasting);
        assert_eq!(rig.watchers.running(), 1);
    }
    // The last ask: a lease that runs out soon (an ask replaces the one before it), then nobody asks again.
    let lease = Duration::from_millis(150);
    read_with(&rig.watchers, "p1", Some(&rig.opening), None, lease);
    wait_until("the lease to run out", || rig.watchers.running() == 0);
    // The next request starts it again, with the same items.
    let reply = read_with(&rig.watchers, "p1", Some(&rig.opening), None, lasting);
    assert_eq!(texts(&reply), ["one"]);
    assert_eq!(rig.watchers.running(), 1);
    // A revision from before the restarted reader is a reset.
    let again = read_with(&rig.watchers, "p1", Some(&rig.opening), Some(1), lasting);
    assert_eq!(again["reset"], true);
}

#[test]
fn lean_drops_a_tool_calls_input_and_keeps_its_files() {
    let call = ChatItem::ToolCall {
        id: "0:0".into(),
        at: None,
        call_id: "c".into(),
        name: "Edit".into(),
        summary: "a.rs".into(),
        detail: crate::session_chat::ToolDetail {
            input: Some(serde_json::json!({"old_string": "a", "new_string": "b"})),
            paths: vec!["a.rs".into()],
            writes: true,
            helper: false,
        },
    };
    let value = serde_json::to_value(lean(call)).unwrap();
    assert!(value.get("input").is_none());
    assert_eq!(value["paths"], serde_json::json!(["a.rs"]));
    assert_eq!(value["writes"], true);
    let user = ChatItem::User { id: "1:0".into(), at: None, text: "hi".into() };
    assert_eq!(lean(user.clone()), user);
}
