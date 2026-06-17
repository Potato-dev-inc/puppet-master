//! Bounded scrollback buffer storing raw bytes per line.
//!
//! Design principles (rewritten from scratch to fix rendering bugs):
//! - Stores **raw bytes**, never decodes to String during accumulation.
//!   This avoids UTF-8 corruption at chunk boundaries (the old code used
//!   `String::from_utf8_lossy` which replaced partial multi-byte sequences
//!   with U+FFFD).
//! - Splits lines on `\n` only. Preserves `\r` inside lines so that
//!   `tail_raw_bytes()` can be replayed into xterm.js with correct
//!   carriage-return semantics.
//! - `tail_text()` produces clean human-readable text for MCP/LLM reads:
//!   strips ANSI CSI/OSC escapes and lone `\r`, joins lines with `\n`.
//! - Cap is in **lines**, not bytes. Default 10 000 lines.

use std::collections::VecDeque;

pub struct Scrollback {
    /// Completed lines, each ending with `\n` (raw bytes, escapes preserved).
    lines: VecDeque<Vec<u8>>,
    /// Current line being assembled (no trailing `\n` yet).
    current: Vec<u8>,
    cap: usize,
}

#[allow(dead_code)]
impl Scrollback {
    pub fn new(cap: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(cap.min(1024)),
            current: Vec::with_capacity(512),
            cap,
        }
    }

    /// Append a chunk of raw PTY bytes. Splits on `\n`; preserves `\r`.
    pub fn push_chunk(&mut self, chunk: &[u8]) {
        for &b in chunk {
            if b == b'\n' {
                // Finalize current line with the \n included.
                self.current.push(b'\n');
                let line = std::mem::take(&mut self.current);
                self.lines.push_back(line);
                while self.lines.len() > self.cap {
                    self.lines.pop_front();
                }
            } else {
                self.current.push(b);
            }
        }
    }

    /// Raw bytes of the last `n` lines, suitable for replaying into xterm.
    /// Includes `\r` and ANSI escape sequences. Lines are `\n`-terminated.
    pub fn tail_raw_bytes(&self, n: usize) -> Vec<u8> {
        let start = self.lines.len().saturating_sub(n);
        let mut out: Vec<u8> = Vec::new();
        for line in self.lines.iter().skip(start) {
            out.extend_from_slice(line);
        }
        // Append the in-progress partial line (if any) without a trailing \n.
        out.extend_from_slice(&self.current);
        out
    }

    /// Clean text of the last `n` lines, suitable for MCP/LLM consumption.
    /// Strips ANSI CSI/OSC sequences and lone `\r`. Lines joined by `\n`.
    pub fn tail_text(&self, n: usize) -> String {
        let raw = self.tail_raw_bytes(n);
        let text = String::from_utf8_lossy(&raw);
        strip_ansi_and_cr(&text)
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.current.clear();
    }
}

/// Strip ANSI CSI and OSC escape sequences plus lone `\r` from a string.
///
/// - CSI: `ESC [` ... final byte (0x40–0x7E)
/// - OSC: `ESC ]` ... `BEL` or `ST` (`ESC \`)
/// - Other common two-byte escapes: `ESC 7`, `ESC 8`, `ESC =`, `ESC >`, etc.
/// - `\r` is removed (carriage returns are positioning, not content)
fn strip_ansi_and_cr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            // ESC — start of an escape sequence
            if i + 1 >= bytes.len() {
                break; // dangling ESC at end
            }
            let next = bytes[i + 1];
            if next == b'[' {
                // CSI: skip until a final byte in 0x40..=0x7E
                i += 2;
                while i < bytes.len() && !(bytes[i] >= 0x40 && bytes[i] <= 0x7e) {
                    i += 1;
                }
                i += 1; // consume the final byte
            } else if next == b']' {
                // OSC: skip until BEL (0x07) or ST (ESC \)
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else {
                // Other two-byte escape (ESC 7, ESC 8, ESC =, ESC M, etc.)
                i += 2;
            }
        } else if b == b'\r' {
            // Drop carriage returns — they're cursor positioning, not content.
            i += 1;
        } else {
            // Keep the byte as-is.
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_newlines_raw() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"hello\nworld\n");
        let raw = sb.tail_raw_bytes(10);
        assert_eq!(raw, b"hello\nworld\n");
    }

    #[test]
    fn coalesces_partial_chunks() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"hel");
        sb.push_chunk(b"lo\nworld\n");
        let raw = sb.tail_raw_bytes(10);
        assert_eq!(raw, b"hello\nworld\n");
    }

    #[test]
    fn preserves_carriage_returns_in_raw() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"hello\r\nworld\r\n");
        let raw = sb.tail_raw_bytes(10);
        assert_eq!(raw, b"hello\r\nworld\r\n");
    }

    #[test]
    fn tail_text_strips_cr_and_ansi() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"\x1b[32mhello\r\n\x1b[0mworld\r\n");
        let text = sb.tail_text(10);
        assert_eq!(text, "hello\nworld\n");
    }

    #[test]
    fn tail_text_strips_osc() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"\x1b]0;title\x07hello\n");
        let text = sb.tail_text(10);
        assert_eq!(text, "hello\n");
    }

    #[test]
    fn caps_old_lines() {
        let mut sb = Scrollback::new(3);
        sb.push_chunk(b"a\nb\nc\nd\n");
        let raw = sb.tail_raw_bytes(10);
        assert_eq!(raw, b"b\nc\nd\n");
    }

    #[test]
    fn partial_line_at_end() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"hello\nworld");
        let raw = sb.tail_raw_bytes(10);
        assert_eq!(raw, b"hello\nworld");
        let text = sb.tail_text(10);
        assert_eq!(text, "hello\nworld");
    }

    #[test]
    fn multiline_ansi_with_text() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"\x1b[1;31mError:\x1b[0m something broke\nNext line\n");
        let text = sb.tail_text(10);
        assert_eq!(text, "Error: something broke\nNext line\n");
    }

    #[test]
    fn utf8_preserved_across_chunks() {
        let mut sb = Scrollback::new(100);
        // "café" in UTF-8: 63 61 66 c3 a9 — split the multi-byte é
        sb.push_chunk(&[b'c', b'a', b'f', 0xc3]);
        sb.push_chunk(&[0xa9, b'\n']);
        let text = sb.tail_text(10);
        assert_eq!(text, "café\n");
    }

    #[test]
    fn clear_resets() {
        let mut sb = Scrollback::new(100);
        sb.push_chunk(b"a\nb\n");
        sb.clear();
        assert_eq!(sb.line_count(), 0);
        assert_eq!(sb.tail_text(10), "");
    }
}
