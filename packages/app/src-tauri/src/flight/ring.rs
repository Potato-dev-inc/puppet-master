//! The per-pane recording buffer: a byte ring of about 2 MB plus a list of timestamped events that
//! point into it.
//!
//! Built for the PTY reader's hot path: one lock, one `Instant` read, a copy of the chunk into the
//! ring (two `memcpy`s at most) and one fixed-size event record. There is no allocation per byte or
//! per chunk once the ring is warm: the byte buffer grows to its cap once, and the event queue is
//! bounded (`MAX_EVENTS`) and reuses its storage.

use super::InputTag;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Bytes of PTY output and input kept per pane.
pub const CAPACITY: usize = 2 * 1024 * 1024;
/// Older events are dropped past this count even when their bytes still fit (a stream of single
/// keystrokes would otherwise grow the event list without bound).
pub const MAX_EVENTS: usize = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Bytes the PTY produced.
    Output,
    /// Bytes written into the PTY; `aux` is the [`InputTag`].
    Input,
    /// The PTY grid changed; `aux` is `cols << 16 | rows`.
    Resize,
    /// A decision or fact written by Puppet Master (UTF-8 text).
    Marker,
}

/// One fixed-size record. `off`/`len` locate its bytes in the stream (a running byte count since the
/// ring started); the bytes themselves live in the ring.
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub t_us: u64,
    pub off: u64,
    pub len: u32,
    pub aux: u32,
    pub kind: Kind,
}

struct Inner {
    /// Grows to `CAPACITY` once, then is written round-robin.
    buf: Vec<u8>,
    /// Bytes ever written (the stream offset of the next byte).
    total: u64,
    events: VecDeque<Event>,
    cols: u16,
    rows: u16,
    /// The grid at the start of the retained window (the cast header's size).
    base_cols: u16,
    base_rows: u16,
    /// Events were dropped from the front.
    wrapped: bool,
}

pub struct Ring {
    inner: Mutex<Inner>,
    started: Instant,
    start_unix_ms: i64,
}

/// A consistent copy of the ring, taken under the lock and used outside it.
pub struct Snapshot {
    pub start_unix_ms: i64,
    pub base_cols: u16,
    pub base_rows: u16,
    pub events: Vec<Event>,
    /// The bytes of the window, starting at stream offset `first_off`.
    pub bytes: Vec<u8>,
    pub first_off: u64,
    /// Older activity was dropped to stay inside the size cap.
    pub truncated: bool,
}

impl Snapshot {
    pub fn data(&self, event: &Event) -> &[u8] {
        let from = (event.off - self.first_off) as usize;
        &self.bytes[from..from + event.len as usize]
    }
}

impl Ring {
    pub fn new(cols: u16, rows: u16) -> Self {
        let start_unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        Self {
            inner: Mutex::new(Inner {
                buf: Vec::new(),
                total: 0,
                events: VecDeque::new(),
                cols,
                rows,
                base_cols: cols,
                base_rows: rows,
                wrapped: false,
            }),
            started: Instant::now(),
            start_unix_ms,
        }
    }

    fn now_us(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    /// PTY output. Called from the reader thread for every chunk.
    pub fn output(&self, bytes: &[u8]) {
        let t = self.now_us();
        self.inner.lock().push(t, Kind::Output, 0, bytes);
    }

    /// Bytes written into the PTY, with who wrote them.
    pub fn input(&self, tag: InputTag, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let t = self.now_us();
        self.inner.lock().push(t, Kind::Input, tag as u32, bytes);
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let t = self.now_us();
        let mut g = self.inner.lock();
        if (g.cols, g.rows) == (cols, rows) {
            return;
        }
        g.cols = cols;
        g.rows = rows;
        g.push(t, Kind::Resize, (u32::from(cols) << 16) | u32::from(rows), &[]);
    }

    /// A decision or fact ("status=waiting_input", "typed task 412 bytes").
    pub fn mark(&self, text: &str) {
        let t = self.now_us();
        self.inner.lock().push(t, Kind::Marker, 0, text.as_bytes());
    }

    pub fn snapshot(&self) -> Snapshot {
        let g = self.inner.lock();
        let events: Vec<Event> = g.events.iter().copied().collect();
        let first_off = events.first().map(|e| e.off).unwrap_or(g.total);
        let bytes = g.read(first_off, (g.total - first_off) as usize);
        Snapshot {
            start_unix_ms: self.start_unix_ms,
            base_cols: g.base_cols,
            base_rows: g.base_rows,
            events,
            bytes,
            first_off,
            truncated: g.wrapped,
        }
    }

    /// Bytes currently held and events kept (for the health view and tests).
    pub fn stats(&self) -> (usize, usize) {
        let g = self.inner.lock();
        let held = g.events.front().map(|e| (g.total - e.off) as usize).unwrap_or(0);
        (held, g.events.len())
    }

    #[cfg(test)]
    pub(crate) fn buffer_capacity(&self) -> usize {
        self.inner.lock().buf.capacity()
    }
}

impl Inner {
    fn push(&mut self, t_us: u64, kind: Kind, aux: u32, bytes: &[u8]) {
        // A chunk bigger than the whole ring keeps its tail (its head would be overwritten at once).
        let kept = if bytes.len() > CAPACITY { &bytes[bytes.len() - CAPACITY..] } else { bytes };
        let skipped = (bytes.len() - kept.len()) as u64;
        if skipped > 0 {
            // The ring is entirely overwritten by this chunk: make sure it exists at full size.
            self.buf.resize(CAPACITY, 0);
        }
        self.total += skipped;
        let off = self.total;
        self.write(kept);
        self.events.push_back(Event { t_us, off, len: kept.len() as u32, aux, kind });
        self.evict();
    }

    /// Copy `data` at the current stream position, growing the buffer until it reaches its cap and
    /// writing round-robin after that.
    fn write(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let idx = (self.total % CAPACITY as u64) as usize;
            let take = data.len().min(CAPACITY - idx);
            if idx == self.buf.len() {
                // Still filling the first lap: the write position is the end of the buffer.
                self.buf.extend_from_slice(&data[..take]);
            } else {
                self.buf[idx..idx + take].copy_from_slice(&data[..take]);
            }
            self.total += take as u64;
            data = &data[take..];
        }
    }

    /// Drop events whose bytes were overwritten, and the oldest ones past `MAX_EVENTS`.
    fn evict(&mut self) {
        let oldest_valid = self.total.saturating_sub(CAPACITY as u64);
        while let Some(front) = self.events.front() {
            let overwritten = front.off < oldest_valid;
            if !overwritten && self.events.len() <= MAX_EVENTS {
                break;
            }
            let gone = self.events.pop_front().expect("front exists");
            self.wrapped = true;
            if gone.kind == Kind::Resize {
                self.base_cols = (gone.aux >> 16) as u16;
                self.base_rows = (gone.aux & 0xffff) as u16;
            }
        }
    }

    /// `len` bytes from stream offset `from` (must still be in the ring).
    fn read(&self, from: u64, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut at = from;
        let mut left = len;
        while left > 0 {
            let idx = (at % CAPACITY as u64) as usize;
            let take = left.min(CAPACITY - idx).min(self.buf.len().saturating_sub(idx));
            if take == 0 {
                break;
            }
            out.extend_from_slice(&self.buf[idx..idx + take]);
            at += take as u64;
            left -= take;
        }
        out
    }
}
