//! The import methods of `ChatIndex` (BC-7): what they write, what they answer, and that they respect the switch.
//! Fixtures are synthetic; no real agent folder is read.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::entry::{Entry, MAX_IMPORTED_TEXT_CHARS};
use super::imported::{import_pane_id, ImportedChatRow};
use super::store::Store;
use super::{entry_of_with, ChatIndex, ContextLookup, PaneContext, Strength};
use crate::session_chat::{ChatItem, ItemObserver};

const AGENT: &str = "agent-x";

/// No pane is open: every live item is written with empty labels.
struct NoPanes;

impl ContextLookup for NoPanes {
    fn context(&self, _pane_id: &str) -> Option<PaneContext> {
        None
    }
}

/// An index over an in-memory store with its own switch (not the process-wide one, so tests do not interfere).
fn index_with(switch: &Arc<AtomicBool>) -> Arc<ChatIndex> {
    ChatIndex::start_switched(Store::open_memory(1000).unwrap(), Arc::new(NoPanes), switch.clone())
}

fn chat(session: &str, stamp: &str, items: usize) -> ImportedChatRow {
    ImportedChatRow {
        agent: AGENT.into(),
        session: session.into(),
        folder: "shop".into(),
        cwd: "~/projects/shop".into(),
        title: "first words".into(),
        updated_ms: 500,
        stamp: stamp.into(),
        items,
        cut_short: false,
        imported_ms: 600,
    }
}

/// Old items of one chat, built the way the importer builds them (the import pane, the imported flag).
fn old_items(session: &str, count: usize, at_ms: i64) -> Vec<Entry> {
    (0..count)
        .map(|n| Entry {
            pane_id: import_pane_id(AGENT),
            session: session.into(),
            item_id: format!("{n}:0"),
            kind: "assistant",
            agent: AGENT.into(),
            folder: "shop".into(),
            at_ms: at_ms + n as i64,
            text: format!("old words {session} {n}"),
            imported: true,
        })
        .collect()
}

fn live_user(id: &str, text: &str) -> ChatItem {
    ChatItem::User { id: id.into(), at: None, text: text.into() }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn with_the_switch_off_an_import_is_refused_and_nothing_is_written() {
    let switch = Arc::new(AtomicBool::new(false));
    let index = index_with(&switch);
    let refused = index.import_chat(&chat("s1", "10:20", 2), &old_items("s1", 2, 10));
    assert_eq!(refused, Err("INDEX_OFF".to_string()));
    let status = index.status().unwrap();
    assert!(!status.enabled);
    assert_eq!((status.imported_chats, status.imported_items), (0, 0));
    assert_eq!(index.imported_stamp(AGENT, "s1").unwrap(), None);

    // Back on: the same import is accepted.
    switch.store(true, Ordering::SeqCst);
    index.import_chat(&chat("s1", "10:20", 2), &old_items("s1", 2, 10)).unwrap();
    assert_eq!(index.imported_stamp(AGENT, "s1").unwrap().as_deref(), Some("10:20"));
}

#[test]
fn status_counts_the_old_chats_and_their_items_apart_from_the_live_ones() {
    let switch = Arc::new(AtomicBool::new(true));
    let index = index_with(&switch);
    index.import_chat(&chat("s1", "a", 2), &old_items("s1", 2, 10)).unwrap();
    index.import_chat(&chat("s2", "b", 1), &old_items("s2", 1, 50)).unwrap();
    let status = index.status().unwrap();
    assert_eq!((status.imported_chats, status.imported_items), (2, 3));
    assert_eq!(status.items, 0, "old items are not counted as live items");

    // A chat read again with a new stamp keeps one record and its new item count.
    index.import_chat(&chat("s1", "c", 1), &old_items("s1", 1, 10)).unwrap();
    let status = index.status().unwrap();
    assert_eq!((status.imported_chats, status.imported_items), (2, 2));
}

#[test]
fn the_answers_read_back_what_the_import_wrote() {
    let switch = Arc::new(AtomicBool::new(true));
    let index = index_with(&switch);
    assert_eq!(index.imported_chat(AGENT, "s1").unwrap(), None);
    index.import_chat(&chat("s1", "10:20", 2), &old_items("s1", 2, 10)).unwrap();
    let record = index.imported_chat(AGENT, "s1").unwrap().expect("the record");
    assert_eq!((record.stamp.as_str(), record.items, record.folder.as_str()), ("10:20", 2, "shop"));
    assert!(!index.has_live(AGENT, "s1").unwrap());
    assert_eq!(index.imported_chat(AGENT, "other").unwrap(), None);
}

#[test]
fn clear_also_clears_the_old_chats() {
    let switch = Arc::new(AtomicBool::new(true));
    let index = index_with(&switch);
    index.import_chat(&chat("s1", "a", 2), &old_items("s1", 2, 10)).unwrap();
    index.clear().unwrap();
    let status = index.status().unwrap();
    assert_eq!((status.items, status.imported_chats, status.imported_items), (0, 0, 0));
    assert_eq!(index.imported_stamp(AGENT, "s1").unwrap(), None);
}

#[test]
fn clear_imported_removes_only_the_old_chats_and_answers_with_the_status() {
    let switch = Arc::new(AtomicBool::new(true));
    let index = index_with(&switch);
    index.observe("pane-live-1", &[live_user("1:0", "live words")]);
    wait_until("the live item", || index.count() == 1);
    index.import_chat(&chat("s1", "a", 2), &old_items("s1", 2, 10)).unwrap();
    let status = index.clear_imported().unwrap();
    assert_eq!((status.items, status.imported_chats, status.imported_items), (1, 0, 0));
    assert_eq!(index.imported_stamp(AGENT, "s1").unwrap(), None);
}

#[test]
fn a_live_writer_and_an_importer_on_two_threads_both_finish_without_error() {
    let switch = Arc::new(AtomicBool::new(true));
    let index = index_with(&switch);
    let live = index.clone();
    let writer = std::thread::spawn(move || {
        for n in 0..40 {
            live.observe("pane-live-1", &[live_user(&format!("{n}:0"), &format!("live words {n}"))]);
        }
    });
    let importer = index.clone();
    let reader = std::thread::spawn(move || {
        for n in 0..50 {
            let session = format!("s{n:02}");
            importer.import_chat(&chat(&session, "stamp", 2), &old_items(&session, 2, 10)).expect("import");
        }
    });
    writer.join().unwrap();
    reader.join().unwrap();
    // 40 live items and 50 chats of 2 old items each (the writer hands its batches to a thread, so wait for them).
    wait_until("the live items", || index.count() == 140);
    let status = index.status().unwrap();
    assert_eq!((status.items, status.imported_chats, status.imported_items), (40, 50, 100));
}

#[test]
fn an_old_chat_text_goes_through_the_strict_entry_before_it_is_written() {
    // The importer builds its rows with `entry_of_with` (re-exported): the strict pass runs before the write.
    let switch = Arc::new(AtomicBool::new(true));
    let index = index_with(&switch);
    let ctx = PaneContext::of(AGENT, r"C:\Users\you\projects\shop", Some("s1"));
    let item = ChatItem::User { id: "1:0".into(), at: None, text: "mail someone@example.com about it".into() };
    let entry = entry_of_with(&import_pane_id(AGENT), &ctx, &item, 1, Strength::Strict, true, MAX_IMPORTED_TEXT_CHARS).expect("kept");
    index.import_chat(&chat("s1", "a", 1), &[entry]).unwrap();
    let hits = index.search("someone", 5).unwrap();
    assert!(hits.hits.is_empty(), "the e-mail address is masked before it is searchable");
}
