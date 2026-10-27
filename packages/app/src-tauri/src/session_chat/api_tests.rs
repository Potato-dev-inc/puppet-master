//! The API reader: the whole conversation each look, only the changes sent, the screen read while the API does not
//! answer, and the way back. The API and the pane are made up; nothing here names a real CLI.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;

use super::*;
use crate::session_chat::screen::tests::{register_probes, ECHO};
use crate::session_chat::screen::ScreenRead;
use crate::session_chat::watch::{Sink, Tuning, Watchers};

/// An API that answers what the test queued, then the last answer again.
struct Scripted {
    answers: Mutex<VecDeque<Result<Vec<ChatItem>, String>>>,
    last: Mutex<Result<Vec<ChatItem>, String>>,
}

impl Scripted {
    fn leak(answers: Vec<Result<Vec<ChatItem>, String>>) -> &'static Scripted {
        Box::leak(Box::new(Scripted { answers: Mutex::new(answers.into()), last: Mutex::new(Err("not asked".into())) }))
    }

    fn then(&self, answer: Result<Vec<ChatItem>, String>) {
        self.answers.lock().push_back(answer);
    }
}

impl SessionApi for Scripted {
    fn fetch(&self, _pane: &PaneFacts, _endpoint: &ApiEndpoint) -> Result<Vec<ChatItem>, String> {
        let mut answers = self.answers.lock();
        if let Some(next) = answers.pop_front() {
            *self.last.lock() = next;
        }
        self.last.lock().clone()
    }
}

/// A pane with an API endpoint (or not) and a screen the test sets.
struct Pane {
    endpoint: Mutex<Option<ApiEndpoint>>,
    text: Mutex<String>,
    status: Mutex<String>,
    gone: AtomicBool,
}

impl Pane {
    fn new(with_api: bool) -> Arc<Self> {
        Arc::new(Pane {
            endpoint: Mutex::new(with_api.then(|| ApiEndpoint { base_url: "http://127.0.0.1:1".into(), session_id: Some("s".into()) })),
            text: Mutex::new(String::new()),
            status: Mutex::new("idle".into()),
            gone: AtomicBool::new(false),
        })
    }
}

impl PaneLookup for Pane {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        (!self.gone.load(Ordering::SeqCst))
            .then(|| PaneFacts { cwd: PathBuf::from(r"C:\Users\you\proj"), started_at: SystemTime::now(), provider_session_id: None, home: None })
    }

    fn api(&self, _pane_id: &str) -> Option<ApiEndpoint> {
        self.endpoint.lock().clone()
    }

    fn screen(&self, _pane_id: &str, _known: Option<u64>) -> Option<ScreenRead> {
        if self.gone.load(Ordering::SeqCst) {
            return None;
        }
        Some(ScreenRead { status: self.status.lock().clone(), position: None, text: Some(self.text.lock().clone()) })
    }
}

fn user(id: &str, text: &str) -> ChatItem {
    ChatItem::User { id: id.into(), at: None, text: text.into() }
}

fn reply(id: &str, text: &str) -> ChatItem {
    ChatItem::Assistant { id: id.into(), at: None, text: text.into(), model: None, final_answer: false }
}

const BANNER: &str = "Probe CLI 1.4  model: small-1\n";

#[test]
fn the_first_answer_replaces_whatever_the_screen_showed_and_later_ones_send_only_the_changes() {
    register_probes();
    let api = Scripted::leak(vec![Ok(vec![user("a:user", "hi"), reply("a:0", "he")])]);
    let pane = Pane::new(true);
    let mut follower = ApiFollower::new(api, ECHO);
    assert_eq!(follower.from(), ChatFrom::Screen, "nothing is known before the first look");

    let first = follower.step(&*pane, "p").unwrap();
    assert_eq!(first, Change::Reset(vec![user("a:user", "hi"), reply("a:0", "he")]));
    assert_eq!(follower.from(), ChatFrom::Api);

    // Nothing changed.
    assert_eq!(follower.step(&*pane, "p").unwrap(), Change::Nothing);
    // The reply grew and a tool row was added: the grown one is sent again under its id, and the new one with it.
    api.then(Ok(vec![user("a:user", "hi"), reply("a:0", "hello"), reply("a:1", "!")]));
    assert_eq!(follower.step(&*pane, "p").unwrap(), Change::Upsert(vec![reply("a:0", "hello"), reply("a:1", "!")]));
    // An item that went away means the viewer starts over.
    api.then(Ok(vec![user("b:user", "again")]));
    assert_eq!(follower.step(&*pane, "p").unwrap(), Change::Reset(vec![user("b:user", "again")]));
}

#[test]
fn a_pane_with_no_api_is_read_off_its_screen() {
    register_probes();
    let api = Scripted::leak(vec![]);
    let pane = Pane::new(false);
    *pane.text.lock() = format!("{BANNER}> say hi\nhi");
    let mut follower = ApiFollower::new(api, ECHO);
    let (Change::Upsert(items) | Change::Reset(items)) = follower.step(&*pane, "p").unwrap() else { panic!("nothing read") };
    assert!(items.iter().any(|item| matches!(item, ChatItem::User { id, .. } if id.starts_with("screen:"))), "{items:?}");
    assert_eq!(follower.from(), ChatFrom::Screen);
}

#[test]
fn an_api_that_stops_answering_hands_the_chat_to_the_screen_after_a_few_looks_and_takes_it_back() {
    register_probes();
    let api = Scripted::leak(vec![Ok(vec![user("a:user", "hi")])]);
    let pane = Pane::new(true);
    *pane.text.lock() = format!("{BANNER}> say hi\nhi");
    let mut follower = ApiFollower::new(api, ECHO);
    follower.step(&*pane, "p").unwrap();
    assert_eq!(follower.from(), ChatFrom::Api);

    api.then(Err("connection refused".into()));
    // One failed look, then another: what is shown stays.
    assert_eq!(follower.step(&*pane, "p").unwrap(), Change::Nothing);
    assert_eq!(follower.step(&*pane, "p").unwrap(), Change::Nothing);
    assert_eq!(follower.from(), ChatFrom::Api);
    // The third one gives the viewer what the screen shows, in place of everything.
    let Change::Reset(items) = follower.step(&*pane, "p").unwrap() else { panic!("expected a reset") };
    assert_eq!(follower.from(), ChatFrom::Screen);
    assert!(items.iter().any(|item| matches!(item, ChatItem::User { id, .. } if id.starts_with("screen:"))), "{items:?}");

    // The API answers again: everything the viewer has is replaced by the conversation.
    api.then(Ok(vec![user("a:user", "hi"), reply("a:0", "hello")]));
    assert_eq!(follower.step(&*pane, "p").unwrap(), Change::Reset(vec![user("a:user", "hi"), reply("a:0", "hello")]));
    assert_eq!(follower.from(), ChatFrom::Api);
}

#[test]
fn a_gone_pane_ends_the_reader() {
    register_probes();
    let api = Scripted::leak(vec![Err("no session yet".into())]);
    let pane = Pane::new(true);
    let mut follower = ApiFollower::new(api, ECHO);
    pane.gone.store(true, Ordering::SeqCst);
    assert!(follower.step(&*pane, "p").is_none());
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

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_watcher_reads_from_the_api_says_so_and_tells_the_viewer_when_it_falls_back() {
    register_probes();
    let api = Scripted::leak(vec![Ok(vec![user("a:user", "hi")])]);
    let pane = Pane::new(true);
    *pane.text.lock() = format!("{BANNER}> say hi\nhi");
    let sink = Arc::new(Batches::default());
    let tuning = Tuning { api_poll: Duration::from_millis(10), screen_poll: Duration::from_millis(10), ..Tuning::default() };
    let watchers = Watchers::with_tuning(sink.clone(), pane.clone(), tuning);

    let first = watchers.open_api("p1", "w", api, ECHO);
    assert_eq!(first, vec![user("a:user", "hi")]);
    assert_eq!(watchers.source_of("p1"), Some("api"));
    assert!(sink.sent.lock().is_empty(), "the first look is the answer to open, not an event");

    api.then(Ok(vec![user("a:user", "hi"), reply("a:0", "hello")]));
    wait_until("the new reply", || !sink.sent.lock().is_empty());
    assert_eq!(sink.sent.lock()[0], (vec![reply("a:0", "hello")], false));

    api.then(Err("connection refused".into()));
    wait_until("the fall back", || watchers.source_of("p1") == Some("screen"));
    assert_eq!(sink.sources.lock().last().map(String::as_str), Some("screen"));

    api.then(Ok(vec![user("a:user", "hi"), reply("a:0", "hello")]));
    wait_until("the way back", || watchers.source_of("p1") == Some("api"));
    assert_eq!(sink.sources.lock().as_slice(), ["screen", "api"]);
    watchers.close_all();
}
