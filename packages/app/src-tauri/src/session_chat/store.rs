//! What one open pane holds: its chat items, each with the revision it last changed at (so a viewer can ask for
//! what is new since the revision it saw), and the holders that keep its reader running.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use super::model::ChatItem;

/// Who keeps a pane's reader running. The reader stops when no holder is left. A desktop surface holds until it
/// lets go; a phone holds a lease (`ttl`) that each request renews and that ends by itself when they stop.
#[derive(Debug, Clone, Copy)]
pub struct Hold<'a> {
    pub id: &'a str,
    pub ttl: Option<Duration>,
}

impl<'a> Hold<'a> {
    pub fn lease(id: &'a str, ttl: Duration) -> Self {
        Hold { id, ttl: Some(ttl) }
    }
}

impl<'a> From<&'a str> for Hold<'a> {
    fn from(id: &'a str) -> Self {
        Hold { id, ttl: None }
    }
}

/// Revisions count every change to any pane's items and never go back, also across app runs (they start at the
/// clock in milliseconds), so a viewer holding an old revision can always tell that its list is stale.
static REVISION: Lazy<AtomicU64> = Lazy::new(|| {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    AtomicU64::new(now)
});

fn next_rev() -> u64 {
    REVISION.fetch_add(1, Ordering::SeqCst) + 1
}

/// The holders of a pane: id -> when its lease ends (`None`: until it lets go).
#[derive(Default)]
pub struct Holders(HashMap<String, Option<Instant>>);

impl Holders {
    pub fn add(&mut self, hold: Hold, now: Instant) {
        self.0.insert(hold.id.to_string(), hold.ttl.map(|ttl| now + ttl));
    }

    pub fn remove(&mut self, id: &str) {
        self.0.remove(id);
    }

    /// True when no holder is left: leases that ran out do not count.
    pub fn none_left(&mut self, now: Instant) -> bool {
        self.0.retain(|_, until| until.map_or(true, |until| until > now));
        self.0.is_empty()
    }
}

/// A pane's items, each with the revision it last changed at.
pub struct Items {
    list: VecDeque<ChatItem>,
    /// `revs[i]` is the revision of `list[i]`.
    revs: VecDeque<u64>,
    /// The revision the list was started (or replaced) at: a viewer older than this must drop what it has.
    reset_rev: u64,
    /// The newest revision of the list.
    last_rev: u64,
    /// The session file the items were read from (`None`: not from a file, or no file yet). Set together with the
    /// items, so a reader of a delta knows which file its items belong to.
    file: Option<PathBuf>,
}

/// What a viewer that has seen everything up to some revision is missing.
#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    /// The newest revision: the viewer asks for what is newer than this next time.
    pub rev: u64,
    /// The viewer's list is out of date as a whole: it drops it and shows `items` instead.
    pub reset: bool,
    pub items: Vec<ChatItem>,
    /// The session file `items` come from (`None`: not read from a file, or none found yet).
    pub file: Option<PathBuf>,
}

impl Items {
    pub fn new() -> Self {
        let rev = next_rev();
        Items { list: VecDeque::new(), revs: VecDeque::new(), reset_rev: rev, last_rev: rev, file: None }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn all(&self) -> Vec<ChatItem> {
        self.list.iter().cloned().collect()
    }

    /// Starts the list over: viewers from before see a reset.
    pub fn clear(&mut self) {
        self.list.clear();
        self.revs.clear();
        self.reset_rev = next_rev();
        self.last_rev = self.reset_rev;
    }

    pub fn push(&mut self, item: ChatItem) {
        self.last_rev = next_rev();
        self.list.push_back(item);
        self.revs.push_back(self.last_rev);
    }

    /// Puts `item` where the item with its id stands (nothing changes when it is the same), else at the end.
    pub fn upsert(&mut self, item: &ChatItem) {
        match self.list.iter().rposition(|known| known.id() == item.id()) {
            Some(at) if self.list[at] == *item => {}
            Some(at) => {
                self.last_rev = next_rev();
                self.list[at] = item.clone();
                self.revs[at] = self.last_rev;
            }
            None => self.push(item.clone()),
        }
    }

    /// Notes which session file the items come from now.
    pub fn set_file(&mut self, file: Option<PathBuf>) {
        self.file = file;
    }

    pub fn drop_oldest(&mut self, count: usize) {
        self.list.drain(..count);
        self.revs.drain(..count);
    }

    /// What a viewer that has seen everything up to revision `since` is missing, at most `cap` items.
    /// `None` (the viewer has nothing): the newest `cap` items. A `since` the list does not know (older than its
    /// start, or from the future), or more than `cap` changes since, is a reset: the viewer gets the newest `cap`
    /// items to show instead of what it has.
    pub fn since(&self, since: Option<u64>, cap: usize) -> Delta {
        let stale = since.is_some_and(|rev| rev < self.reset_rev || rev > self.last_rev);
        let from = match since {
            Some(rev) if !stale => self.revs.iter().position(|item_rev| *item_rev > rev).unwrap_or(self.list.len()),
            _ => 0,
        };
        let missing = self.list.len() - from;
        let gap = since.is_some() && !stale && missing > cap;
        let skip = from + missing.saturating_sub(cap);
        Delta { rev: self.last_rev, reset: stale || gap, items: self.list.iter().skip(skip).cloned().collect(), file: self.file.clone() }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
