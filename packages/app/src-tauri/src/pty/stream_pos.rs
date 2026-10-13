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

impl StreamPos {
    /// One PTY reader step for a chunk of `bytes`: `emit(offset)` hands the chunk out (to
    /// `terminal-data`, and from there to the bridge's `terminal` events) numbered with the offset it
    /// starts at, `total` advances past it, and the screen emulator consumes it with `processed`
    /// stored under the emulator lock, so `read_screen_replay_at` gets a snapshot and the offset it
    /// was taken at that agree. Returns the screen text after the chunk.
    pub fn feed(
        &self,
        screen: &Mutex<vt100::Parser>,
        total: &mut u64,
        bytes: &[u8],
        emit: impl FnOnce(u64),
    ) -> String {
        emit(*total);
        *total += bytes.len() as u64;
        self.emitted.store(*total, Ordering::SeqCst);
        let mut parser = screen.lock();
        parser.process(bytes);
        self.processed.store(*total, Ordering::SeqCst);
        parser.screen().contents()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::registry::PaneRegistry;
    use std::sync::atomic::AtomicBool;

    /// What the screen shows after `bytes` fed to a fresh 3x10 emulator.
    fn reference_screen(bytes: &[u8]) -> String {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(bytes);
        parser.screen().contents()
    }

    fn registry_with(id: &str) -> Mutex<PaneRegistry> {
        let mut registry = PaneRegistry::new();
        registry.panes.insert(id.to_string(), PaneRegistry::test_pane_stub(id));
        Mutex::new(registry)
    }

    #[test]
    fn feed_numbers_chunks_by_their_start_and_advances_both_counters() {
        let pos = StreamPos::default();
        let screen = Mutex::new(vt100::Parser::new(3, 10, 100));
        let mut total = 0;
        let mut starts = Vec::new();
        for chunk in [&b"abc"[..], b"defgh\r\n", b"i"] {
            pos.feed(&screen, &mut total, chunk, |offset| starts.push(offset));
        }
        assert_eq!(starts, vec![0, 3, 10]);
        assert_eq!(total, 11);
        assert_eq!(pos.emitted.load(Ordering::SeqCst), 11);
        assert_eq!(pos.processed.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn a_snapshot_offset_is_the_exact_point_the_snapshot_was_taken_at() {
        // A reader thread feeds numbered chunks while this thread takes screen replays the way
        // `GET /panes/{id}/screen` does. Every replay, applied to a fresh emulator, must show exactly
        // what the first `offset` bytes of the stream draw: the offset is read under the emulator lock.
        let id = "stream-pos-lock-test";
        let registry = registry_with(id);
        let pos = start(id);
        let screen = registry.lock().panes.get(id).unwrap().screen.clone();

        let stream: Vec<u8> = (0..400).flat_map(|n| format!("row {n}\r\n").into_bytes()).collect();
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (stream, done, pos, screen) = (stream.clone(), done.clone(), pos.clone(), screen.clone());
            std::thread::spawn(move || {
                let mut total = 0;
                // Chunk boundaries fall mid-line on purpose.
                for chunk in stream.chunks(7) {
                    pos.feed(&screen, &mut total, chunk, |_| {});
                    std::thread::yield_now();
                }
                done.store(true, Ordering::SeqCst);
            })
        };

        let mut taken = 0;
        while !done.load(Ordering::SeqCst) || taken < 3 {
            let (replay, cols, rows, offset) =
                crate::pty::registry::read_screen_replay_at(&registry, id).expect("replay");
            let offset = offset.expect("offset") as usize;
            let mut viewer = vt100::Parser::new(rows, cols, 100);
            viewer.process(&replay);
            assert_eq!(
                viewer.screen().contents(),
                reference_screen(&stream[..offset]),
                "snapshot at offset {offset} does not show the first {offset} bytes"
            );
            taken += 1;
        }
        reader.join().unwrap();
        let (_, _, _, offset) = crate::pty::registry::read_screen_replay_at(&registry, id).unwrap();
        assert_eq!(offset, Some(stream.len() as u64));
        assert!(taken >= 3);
    }
}
