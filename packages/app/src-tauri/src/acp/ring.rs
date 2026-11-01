//! A capped ring of the adapter's stderr lines, kept for diagnostics ("why did it die?").
//!
//! Bounded in lines AND bytes, each line cut short and stripped of control characters, so a chatty or
//! hostile adapter can neither grow memory nor inject terminal escapes into a log or a card.

use std::collections::VecDeque;

use parking_lot::Mutex;

pub const DEFAULT_MAX_LINES: usize = 200;
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024;
const MAX_LINE_CHARS: usize = 2048;

#[derive(Debug)]
pub struct StderrRing {
    max_lines: usize,
    max_bytes: usize,
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    lines: VecDeque<String>,
    bytes: usize,
    total_bytes: u64,
    dropped_lines: u64,
}

impl Default for StderrRing {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES)
    }
}

impl StderrRing {
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        Self { max_lines: max_lines.max(1), max_bytes: max_bytes.max(64), inner: Mutex::new(Inner::default()) }
    }

    pub fn push_line(&self, raw: &[u8]) {
        let cleaned = clean_line(raw);
        let mut inner = self.inner.lock();
        inner.total_bytes += raw.len() as u64 + 1;
        inner.bytes += cleaned.len();
        inner.lines.push_back(cleaned);
        while inner.lines.len() > self.max_lines || inner.bytes > self.max_bytes {
            match inner.lines.pop_front() {
                Some(old) => {
                    inner.bytes = inner.bytes.saturating_sub(old.len());
                    inner.dropped_lines += 1;
                }
                None => break,
            }
        }
    }

    /// The last `n` lines, oldest first.
    pub fn tail(&self, n: usize) -> Vec<String> {
        let inner = self.inner.lock();
        let skip = inner.lines.len().saturating_sub(n);
        inner.lines.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bytes_held(&self) -> usize {
        self.inner.lock().bytes
    }

    pub fn total_bytes(&self) -> u64 {
        self.inner.lock().total_bytes
    }

    pub fn dropped_lines(&self) -> u64 {
        self.inner.lock().dropped_lines
    }
}

fn clean_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out: String = text.chars().filter(|c| !c.is_control() || *c == '\t').take(MAX_LINE_CHARS).collect();
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keeps_only_the_newest_lines() {
        let ring = StderrRing::new(3, 10_000);
        for n in 0..10 {
            ring.push_line(format!("line {n}").as_bytes());
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.tail(10), vec!["line 7", "line 8", "line 9"]);
        assert_eq!(ring.tail(1), vec!["line 9"]);
        assert_eq!(ring.dropped_lines(), 7);
    }

    #[test]
    fn it_stays_under_the_byte_cap_however_chatty_the_adapter_is() {
        let ring = StderrRing::new(10_000, 1024);
        for _ in 0..5000 {
            ring.push_line(&[b'a'; 100]);
        }
        assert!(ring.bytes_held() <= 1024, "held {}", ring.bytes_held());
        assert_eq!(ring.total_bytes(), 5000 * 101);
    }

    #[test]
    fn control_characters_and_long_lines_are_cleaned() {
        let ring = StderrRing::default();
        ring.push_line(b"\x1b[31mred\x1b[0m\r");
        ring.push_line("é".repeat(5000).as_bytes());
        ring.push_line(&[0xff, b'o', b'k']);
        let tail = ring.tail(3);
        assert_eq!(tail[0], "[31mred[0m");
        assert!(!tail[0].contains('\x1b'));
        assert_eq!(tail[1].chars().count(), 2048);
        assert!(tail[2].ends_with("ok"));
    }
}
