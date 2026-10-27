//! The screen reader inside the watcher: one thread per open pane, items put in place by id, nothing read when
//! the pane printed nothing.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::tests::{register_probes, ECHO};
use super::ScreenRead;
use crate::session_chat::model::ChatItem;
use crate::session_chat::source::PaneFacts;
use crate::session_chat::watch::{PaneLookup, Sink, Tuning, Watchers};

/// A pane whose text and status the test sets. The output position is the length of the text.
#[derive(Default)]
struct Screens {
    text: Mutex<String>,
    status: Mutex<String>,
    texts_read: AtomicUsize,
    gone: AtomicBool,
}

impl PaneLookup for Screens {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        None
    }

    fn screen(&self, _pane_id: &str, known: Option<u64>) -> Option<ScreenRead> {
        if self.gone.load(Ordering::SeqCst) {
            return None;
        }
        let text = self.text.lock().clone();
        let position = Some(text.len() as u64);
        let text = if known == position {
            None
        } else {
            self.texts_read.fetch_add(1, Ordering::SeqCst);
            Some(text)
        };
        Some(ScreenRead { status: self.status.lock().clone(), position, text })
    }
}

#[derive(Default)]
struct Batches {
    sent: Mutex<Vec<(Vec<ChatItem>, bool)>>,
    waits: Mutex<Vec<Option<String>>>,
}

impl Sink for Batches {
    fn send(&self, _pane_id: &str, items: Vec<ChatItem>, reset: bool) {
        self.sent.lock().push((items, reset));
    }

    fn waiting(&self, _pane_id: &str, waiting_for: Option<&str>) {
        self.waits.lock().push(waiting_for.map(str::to_string));
    }
}

fn rig() -> (Watchers, Arc<Screens>, Arc<Batches>) {
    register_probes();
    let panes = Arc::new(Screens::default());
    *panes.status.lock() = "idle".into();
    let sink = Arc::new(Batches::default());
    let tuning = Tuning { screen_poll: Duration::from_millis(10), ..Tuning::default() };
    (Watchers::with_tuning(sink.clone(), panes.clone(), tuning), panes, sink)
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

const BANNER: &str = "Probe CLI 1.4  model: small-1\n";

#[test]
fn the_first_look_is_the_answer_to_open_and_later_changes_replace_items_by_id() {
    let (watchers, panes, sink) = rig();
    *panes.text.lock() = format!("{BANNER}> say hi\nhi");
    *panes.status.lock() = "running".into();

    let first = watchers.open_screen("p1", "w", ECHO);
    let ids: Vec<&str> = first.iter().map(|item| item.id()).collect();
    assert_eq!(ids, ["screen:session", "screen:1:user", "screen:1:reply"]);
    assert!(sink.sent.lock().is_empty());

    *panes.text.lock() = format!("{BANNER}> say hi\nhi there, how are you");
    wait_until("the grown reply", || !sink.sent.lock().is_empty());
    let sent = sink.sent.lock().clone();
    assert_eq!(sent.len(), 1);
    let (items, reset) = &sent[0];
    assert!(!reset);
    assert_eq!(items.len(), 1, "only the reply changed: {items:?}");
    assert!(matches!(&items[0], ChatItem::Assistant { id, text, .. } if id == "screen:1:reply" && text == "hi there, how are you"), "{items:?}");
    // The watcher's own copy holds one reply, the new one.
    let held = watchers.open_screen("p1", "w", ECHO);
    assert_eq!(held.iter().filter(|item| item.id() == "screen:1:reply").count(), 1);
    assert!(matches!(held.last(), Some(ChatItem::Assistant { text, .. }) if text == "hi there, how are you"));
    assert_eq!(watchers.running(), 1, "opening an open pane starts no second reader");
}

#[test]
fn a_pane_that_prints_nothing_is_not_read_again() {
    let (watchers, panes, sink) = rig();
    *panes.text.lock() = format!("{BANNER}> say hi\nhi");
    watchers.open_screen("p1", "w", ECHO);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(panes.texts_read.load(Ordering::SeqCst), 1, "the text was read once and not again while it stood still");
    // The status alone moving on (the turn ended) still reaches the viewer, without reading the text again.
    wait_until("the end of the turn", || sink.sent.lock().iter().any(|(items, _)| items.iter().any(|item| matches!(item, ChatItem::TurnEnd { .. }))));
    assert_eq!(panes.texts_read.load(Ordering::SeqCst), 1);
}

#[test]
fn a_chat_with_nothing_to_show_says_what_it_waits_for_and_a_gone_pane_stops_the_reader() {
    let (watchers, panes, sink) = rig();
    let first = watchers.open_screen("p1", "w", ECHO);
    assert!(first.is_empty());
    let waiting = watchers.waiting_for("p1").expect("a chat with nothing to show says what it waits for");
    assert!(waiting.contains("terminal"), "{waiting}");
    panes.gone.store(true, Ordering::SeqCst);
    wait_until("the reader to stop", || watchers.running() == 0);
    assert!(sink.sent.lock().is_empty());
}

#[test]
fn a_reply_that_grows_on_the_screen_is_a_change_since_the_revision_a_viewer_saw() {
    let (watchers, panes, sink) = rig();
    *panes.text.lock() = format!("{BANNER}> say hi\nhi");
    *panes.status.lock() = "running".into();
    watchers.open_screen("p1", "w", ECHO);
    let seen = watchers.since("p1", None, 300).unwrap();
    assert!(watchers.since("p1", Some(seen.rev), 300).unwrap().items.is_empty());

    *panes.text.lock() = format!("{BANNER}> say hi\nhi there, how are you");
    wait_until("the grown reply", || !sink.sent.lock().is_empty());
    let delta = watchers.since("p1", Some(seen.rev), 300).unwrap();
    assert!(!delta.reset);
    assert_eq!(delta.items.len(), 1, "only the reply changed: {:?}", delta.items);
    assert!(matches!(&delta.items[0], ChatItem::Assistant { id, text, .. } if id == "screen:1:reply" && text == "hi there, how are you"));
}
