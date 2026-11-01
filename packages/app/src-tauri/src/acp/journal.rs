//! The chat journal (P-29, "never lose a word"): an append-only file per chat conversation, written as each
//! chat item arrives, so a crash or a kill in the middle of a turn loses (almost) nothing.
//!
//! Before this, a chat lived only in memory (`session_chat::Feed`); after an app start it was rebuilt from the
//! agent's own session file by `session/load`, which only knows what the agent had finished writing. The record
//! (`record.rs`) keeps a pointer, never words.
//!
//! * **Format.** `<app data>/acp/journals/<conversation id>.jsonl`, one JSON object per line:
//!   `{"v":1,"at":<ms>,"item":<ChatItem>}`, or `{"v":1,"reset":true}` (the chat started over). The same item id
//!   appears many times while a message grows; the last line of an id wins and keeps the item's place.
//! * **Safe to read after a kill.** A line that does not parse (a half-written last line, a line of a newer
//!   build) is skipped, never fatal. Appending after such a line starts on a fresh line.
//! * **Cheap.** Lines are coalesced for 100 ms (a growing message is written about ten times a second, not on
//!   every chunk), written at once with the OS (a killed process loses nothing it handed over), and `fsync`ed at
//!   most every 250 ms and at the end of a turn. What a killed process can lose: the last 100 ms of text.
//! * **Bounded.** Above [`MAX_BYTES`] the file is compacted (latest line per id, newest [`MAX_ITEMS`] items) by
//!   an atomic replace.
//! * **Never secrets.** Each item goes through `redact::json` before it is written, like the phone's answer.
//! * **Never lost to a failure.** Nothing here deletes a journal except [`Journal::remove`], which only the
//!   discard of a conversation that never started calls. A failed resume or restart does not touch it.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::session_chat::ChatItem;

/// Lines are held back at most this long (a growing message is written about ten times a second).
pub const COALESCE: Duration = Duration::from_millis(100);
/// `fsync` at most this often (and always when a turn ends).
pub const SYNC_EVERY: Duration = Duration::from_millis(250);
/// The file is compacted above this size.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Items kept by a compaction and by a load (the same cap as the live feed).
pub const MAX_ITEMS: usize = crate::session_chat::pushed::MAX_FEED_ITEMS;
const VERSION: u64 = 1;

/// What a journal file says.
#[derive(Debug, Default, PartialEq)]
pub struct Loaded {
    /// The items in the order they stand, the latest copy of each.
    pub items: Vec<ChatItem>,
    /// Lines that could not be read and were skipped.
    pub skipped: usize,
}

/// The items in the journal at `path`. A missing file is an empty list; a bad line is skipped.
pub fn load(path: &Path) -> Loaded {
    let Ok(bytes) = std::fs::read(path) else { return Loaded::default() };
    let text = String::from_utf8_lossy(&bytes);
    let mut order: Vec<String> = Vec::new();
    let mut by_id: HashMap<String, ChatItem> = HashMap::new();
    let mut skipped = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            skipped += 1;
            continue;
        };
        if value.get("reset").and_then(Value::as_bool) == Some(true) {
            order.clear();
            by_id.clear();
            continue;
        }
        let Some(item) = value.get("item").and_then(|i| serde_json::from_value::<ChatItem>(i.clone()).ok()) else {
            skipped += 1;
            continue;
        };
        let id = item.id().to_string();
        if by_id.insert(id.clone(), item).is_none() {
            order.push(id);
        }
    }
    let drop = order.len().saturating_sub(MAX_ITEMS);
    let items = order.into_iter().skip(drop).filter_map(|id| by_id.remove(&id)).collect();
    Loaded { items, skipped }
}

struct State {
    file: Option<File>,
    /// Lines waiting to be written, in order, one per item id (the latest copy replaces the earlier).
    pending: Vec<(String, String)>,
    last_write: Option<Instant>,
    last_sync: Option<Instant>,
    /// A timer thread will flush `pending`.
    armed: bool,
    bytes: u64,
    /// Said once that the disk refused, so a full disk does not flood the log.
    warned: bool,
}

struct Shared {
    path: PathBuf,
    state: Mutex<State>,
}

/// The writing side of one conversation's journal. Cheap to clone.
#[derive(Clone)]
pub struct Journal(Arc<Shared>);

impl Journal {
    /// A journal at `path`. Nothing is created until the first item.
    pub fn at(path: impl Into<PathBuf>) -> Journal {
        let path = path.into();
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Journal(Arc::new(Shared { path, state: Mutex::new(State { file: None, pending: Vec::new(), last_write: None, last_sync: None, armed: false, bytes, warned: false }) }))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }

    /// What the journal holds now (pending lines are written first).
    pub fn load(&self) -> Loaded {
        self.flush();
        load(&self.0.path)
    }

    /// Records `item` (redacted). Returns at once; the file is written within [`COALESCE`] (immediately when the
    /// item ends a turn).
    pub fn append(&self, item: &ChatItem) {
        let Some(line) = line_of(item) else { return };
        let ends_turn = matches!(item, ChatItem::TurnEnd { .. });
        let id = item.id().to_string();
        let mut st = self.0.state.lock();
        match st.pending.iter_mut().find(|(known, _)| *known == id) {
            Some(slot) => slot.1 = line,
            None => st.pending.push((id, line)),
        }
        let due = st.last_write.is_none_or(|at| at.elapsed() >= COALESCE);
        if ends_turn || due {
            self.flush_locked(&mut st, ends_turn);
        } else if !st.armed {
            st.armed = true;
            let shared = self.0.clone();
            std::thread::spawn(move || {
                std::thread::sleep(COALESCE);
                let journal = Journal(shared);
                let mut st = journal.0.state.lock();
                st.armed = false;
                journal.flush_locked(&mut st, true);
            });
        }
    }

    /// The chat started over (its items are about to be rebuilt): earlier lines no longer count. The earlier
    /// file is kept next to it as `.prev`, so a rebuild that fails still has the words.
    pub fn reset(&self) {
        let mut st = self.0.state.lock();
        self.flush_locked(&mut st, true);
        st.file = None;
        let prev = self.0.path.with_extension("prev.jsonl");
        if self.0.path.exists() && load(&self.0.path).items.len() > 0 {
            let _ = std::fs::rename(&self.0.path, &prev);
        }
        st.bytes = 0;
        let line = format!("{}\n", json!({ "v": VERSION, "reset": true }));
        self.write_locked(&mut st, line.as_bytes(), true);
    }

    /// The items to start from: this journal, or, when it holds none because the chat was just started over, the
    /// file kept by [`Journal::reset`].
    pub fn load_or_previous(&self) -> Loaded {
        let now = self.load();
        if !now.items.is_empty() {
            return now;
        }
        let prev = load(&self.0.path.with_extension("prev.jsonl"));
        if prev.items.is_empty() { now } else { prev }
    }

    /// Writes what is pending and syncs it. For a clean shutdown and for tests.
    pub fn flush(&self) {
        let mut st = self.0.state.lock();
        self.flush_locked(&mut st, true);
    }

    /// Deletes the file. Only for a conversation that never got going.
    pub fn remove(&self) {
        let mut st = self.0.state.lock();
        st.pending.clear();
        st.file = None;
        st.bytes = 0;
        let _ = std::fs::remove_file(&self.0.path);
        let _ = std::fs::remove_file(self.0.path.with_extension("prev.jsonl"));
    }

    fn flush_locked(&self, st: &mut State, sync_now: bool) {
        if st.pending.is_empty() {
            if sync_now {
                self.sync_locked(st);
            }
            return;
        }
        let mut buf = String::new();
        for (_, line) in st.pending.drain(..) {
            buf.push_str(&line);
            buf.push('\n');
        }
        let sync = sync_now || st.last_sync.is_none_or(|at| at.elapsed() >= SYNC_EVERY);
        self.write_locked(st, buf.as_bytes(), sync);
        if st.bytes > MAX_BYTES {
            self.compact_locked(st);
        }
    }

    fn sync_locked(&self, st: &mut State) {
        if let Some(file) = st.file.as_ref() {
            if file.sync_data().is_ok() {
                st.last_sync = Some(Instant::now());
            }
        }
    }

    fn write_locked(&self, st: &mut State, bytes: &[u8], sync: bool) {
        if st.file.is_none() {
            match open_for_append(&self.0.path) {
                Ok((file, len)) => {
                    st.file = Some(file);
                    st.bytes = len;
                }
                Err(e) => return self.warn(st, &e),
            }
        }
        let result = st.file.as_mut().map(|f| f.write_all(bytes));
        match result {
            Some(Ok(())) => {
                st.bytes += bytes.len() as u64;
                st.last_write = Some(Instant::now());
                if sync {
                    self.sync_locked(st);
                }
            }
            Some(Err(e)) => {
                // The handle may be bad (the folder went away): open again next time.
                st.file = None;
                self.warn(st, &e);
            }
            None => {}
        }
    }

    fn warn(&self, st: &mut State, error: &std::io::Error) {
        if !st.warned {
            st.warned = true;
            tracing::warn!("acp journal: could not write the chat journal: {error}");
        }
    }

    /// Rewrites the file with the latest line of each id (atomic replace), then appends go to the new file.
    fn compact_locked(&self, st: &mut State) {
        let loaded = load(&self.0.path);
        let mut out = String::new();
        for item in &loaded.items {
            if let Some(line) = line_of(item) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        st.file = None;
        match crate::settings_patch::write_atomic(&self.0.path, out.as_bytes()) {
            Ok(()) => st.bytes = out.len() as u64,
            Err(e) => self.warn(st, &e),
        }
    }
}

fn line_of(item: &ChatItem) -> Option<String> {
    let value = serde_json::to_value(item).ok()?;
    let value = crate::redact::json(value).into_inner();
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    serde_json::to_string(&json!({ "v": VERSION, "at": at, "item": value })).ok()
}

/// Opens the file for appending. A file whose last line was cut short by a kill gets a line break first, so
/// the next line is not glued to the broken one.
fn open_for_append(path: &Path) -> std::io::Result<(File, u64)> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().create(true).read(true).append(true).open(path)?;
    let len = file.metadata()?.len();
    if len > 0 {
        file.seek(SeekFrom::Start(len - 1))?;
        let mut last = [0u8; 1];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            file.write_all(b"\n")?;
            return Ok((file, len + 1));
        }
    }
    Ok((file, len))
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
