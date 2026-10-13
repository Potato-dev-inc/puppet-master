//! Byte positions of a pane's output stream, so a viewer can join a snapshot to live output
//! without losing or repeating a byte.
//!
//! The PTY reader numbers every chunk it emits (`terminal-data` carries the offset where the chunk
//! starts, counted in bytes since the pane started). `processed` is how far the screen emulator
//! has consumed the stream: it is stored while the emulator lock is held, so a screen replay read
//! under that lock gets a snapshot and the offset it was taken at that agree. A viewer writes the
//! snapshot, then only the bytes at or after that offset.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct StreamPos {
    /// Bytes handed to `terminal-data` so far (what a `pty://resized` event is placed after).
    pub emitted: AtomicU64,
    /// Bytes the screen emulator has processed.
    pub processed: AtomicU64,
}

static POS: Lazy<Mutex<HashMap<String, Arc<StreamPos>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// A fresh counter for a pane that is being (re)spawned.
pub fn start(pane_id: &str) -> Arc<StreamPos> {
    let pos = Arc::new(StreamPos::default());
    POS.lock().insert(pane_id.to_string(), pos.clone());
    pos
}

pub fn get(pane_id: &str) -> Option<Arc<StreamPos>> {
    POS.lock().get(pane_id).cloned()
}

/// Where the stream is now, for placing a resize between chunks.
pub fn emitted(pane_id: &str) -> Option<u64> {
    get(pane_id).map(|p| p.emitted.load(Ordering::SeqCst))
}
