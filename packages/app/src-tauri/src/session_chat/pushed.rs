//! A chat that is pushed to us instead of read: a producer (the ACP lane) puts items into a [`Feed`] as it
//! learns of them, and the pane's reader only asks what is new. There is no file to find and no screen to read,
//! so the viewers (desktop, phone, `GET /panes/{id}/chat?since=`) see the same `ChatItem`s and the same
//! revisions as for any other chat: the revisions belong to the watcher's item list (`store.rs`), not to the
//! producer.
//!
//! Item ids are stable: a producer sends an item again under its id and it replaces the old copy where it
//! stands (a growing assistant message, a tool call whose title arrives late).
//! Chat text is never logged here.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;

use super::model::ChatItem;

/// Most items a feed keeps. Older ones are dropped, as in every pane's list (`watch::MAX_ITEMS`).
pub const MAX_FEED_ITEMS: usize = 4000;

/// Where a reader stands in a feed: `generation` changes when the feed is started over, `seq` counts changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushedPos {
    pub generation: u64,
    pub seq: u64,
}

/// What changed since a position.
#[derive(Debug, Clone, PartialEq)]
pub struct PushedChanges {
    pub pos: PushedPos,
    /// The reader drops what it has and shows `items` (the whole list) instead.
    pub reset: bool,
    pub items: Vec<ChatItem>,
}

/// A pane's pushed chat, as the watcher sees it.
pub trait PushedSource: Send + Sync {
    /// What changed since `known` (`None`: the reader has nothing, it gets the whole list as a reset).
    /// `None` back: the source is gone, the reader ends.
    fn changes(&self, known: Option<PushedPos>) -> Option<PushedChanges>;
    /// For a chat with nothing to show: what it waits for.
    fn waiting(&self) -> String;
    /// Extra facts for the viewer about the session behind the chat (phase, queue...), when there are any.
    fn state(&self) -> Option<Value> {
        None
    }
}

struct FeedInner {
    generation: u64,
    seq: u64,
    /// `(seq it last changed at, item)`, in the order the items stand.
    entries: VecDeque<(u64, ChatItem)>,
}

/// The ordered list a producer pushes into. Cheap to read: a reader holds the lock only to copy what changed.
pub struct Feed {
    inner: Mutex<FeedInner>,
    closed: AtomicBool,
}

impl Default for Feed {
    fn default() -> Self {
        Self::new()
    }
}

impl Feed {
    pub fn new() -> Self {
        Feed { inner: Mutex::new(FeedInner { generation: 1, seq: 0, entries: VecDeque::new() }), closed: AtomicBool::new(false) }
    }

    /// Puts `item` where the item with its id stands (nothing changes when it is the same), else at the end.
    pub fn upsert(&self, item: ChatItem) {
        let mut inner = self.inner.lock();
        match inner.entries.iter().rposition(|(_, known)| known.id() == item.id()) {
            Some(at) if inner.entries[at].1 == item => {}
            Some(at) => {
                inner.seq += 1;
                let seq = inner.seq;
                inner.entries[at] = (seq, item);
            }
            None => {
                inner.seq += 1;
                let seq = inner.seq;
                inner.entries.push_back((seq, item));
                while inner.entries.len() > MAX_FEED_ITEMS {
                    inner.entries.pop_front();
                }
            }
        }
    }

    /// Starts the list over: readers from before get a reset. (Used by the resume path, A3.)
    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.generation += 1;
        inner.seq = 0;
        inner.entries.clear();
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().entries.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    /// Every item, in order.
    pub fn all(&self) -> Vec<ChatItem> {
        self.inner.lock().entries.iter().map(|(_, item)| item.clone()).collect()
    }

    /// The item with this id, if the feed has it.
    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<ChatItem> {
        self.inner.lock().entries.iter().rev().find(|(_, item)| item.id() == id).map(|(_, item)| item.clone())
    }

    /// The producer is done: readers end.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// What changed since `known`. A position from another generation (or from the future) is a reset.
    pub fn changes_since(&self, known: Option<PushedPos>) -> PushedChanges {
        let inner = self.inner.lock();
        let pos = PushedPos { generation: inner.generation, seq: inner.seq };
        match known {
            Some(known) if known.generation == inner.generation && known.seq <= inner.seq => PushedChanges {
                pos,
                reset: false,
                items: inner.entries.iter().filter(|(seq, _)| *seq > known.seq).map(|(_, item)| item.clone()).collect(),
            },
            _ => PushedChanges { pos, reset: true, items: inner.entries.iter().map(|(_, item)| item.clone()).collect() },
        }
    }
}

static SOURCES: Lazy<Mutex<HashMap<String, Arc<dyn PushedSource>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Tells the chat readers that `pane_id`'s chat is pushed by `source` (so `Opening::for_pane` finds it).
pub fn register(pane_id: &str, source: Arc<dyn PushedSource>) {
    SOURCES.lock().insert(pane_id.to_string(), source);
}

pub fn unregister(pane_id: &str) {
    SOURCES.lock().remove(pane_id);
}

/// The pushed source of a pane, if its chat is pushed.
pub fn lookup(pane_id: &str) -> Option<Arc<dyn PushedSource>> {
    SOURCES.lock().get(pane_id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, text: &str) -> ChatItem {
        ChatItem::User { id: id.into(), at: None, text: text.into() }
    }

    #[test]
    fn a_new_reader_gets_everything_as_a_reset_and_then_only_what_changed() {
        let feed = Feed::new();
        feed.upsert(user("a", "one"));
        feed.upsert(user("b", "two"));
        let first = feed.changes_since(None);
        assert!(first.reset);
        assert_eq!(first.items.len(), 2);
        let quiet = feed.changes_since(Some(first.pos));
        assert!(!quiet.reset && quiet.items.is_empty());
        feed.upsert(user("c", "three"));
        let next = feed.changes_since(Some(first.pos));
        assert!(!next.reset);
        assert_eq!(next.items, vec![user("c", "three")]);
    }

    #[test]
    fn an_item_sent_again_replaces_the_old_copy_where_it_stands() {
        let feed = Feed::new();
        feed.upsert(user("a", "he"));
        feed.upsert(user("b", "other"));
        let seen = feed.changes_since(None).pos;
        feed.upsert(user("a", "hello"));
        assert_eq!(feed.all(), vec![user("a", "hello"), user("b", "other")]);
        let changes = feed.changes_since(Some(seen));
        assert_eq!(changes.items, vec![user("a", "hello")]);
        // The same item again is no change at all.
        let after = changes.pos;
        feed.upsert(user("a", "hello"));
        assert!(feed.changes_since(Some(after)).items.is_empty());
    }

    #[test]
    fn starting_over_makes_every_reader_reset() {
        let feed = Feed::new();
        feed.upsert(user("a", "one"));
        let seen = feed.changes_since(None).pos;
        feed.clear();
        feed.upsert(user("z", "new"));
        let changes = feed.changes_since(Some(seen));
        assert!(changes.reset);
        assert_eq!(changes.items, vec![user("z", "new")]);
    }

    #[test]
    fn the_feed_keeps_the_newest_items_when_it_is_full() {
        let feed = Feed::new();
        for n in 0..(MAX_FEED_ITEMS + 5) {
            feed.upsert(user(&format!("i{n}"), "x"));
        }
        assert_eq!(feed.len(), MAX_FEED_ITEMS);
        assert_eq!(feed.all().first().map(|i| i.id().to_string()), Some("i5".to_string()));
    }

    #[test]
    fn the_registry_finds_a_source_by_pane() {
        struct Gone;
        impl PushedSource for Gone {
            fn changes(&self, _: Option<PushedPos>) -> Option<PushedChanges> {
                None
            }
            fn waiting(&self) -> String {
                String::new()
            }
        }
        register("pushed-test-pane", Arc::new(Gone));
        assert!(lookup("pushed-test-pane").is_some());
        unregister("pushed-test-pane");
        assert!(lookup("pushed-test-pane").is_none());
    }
}
