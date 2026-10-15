//! Turns the bytes a terminal sends into keys. Pure, so it is unit-tested without a console.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Tab,
    ShiftTab,
    Backspace,
    CtrlC,
    CtrlU,
    /// A bracketed paste (`ESC[200~ ... ESC[201~`).
    Paste(String),
    /// Anything else (function keys, unknown sequences): ignored by the engine, kept for the log.
    Other(String),
}

/// A lone ESC at the end of the bytes is Esc, but only once nothing follows it for this long (it may
/// be the first byte of an arrow key whose rest has not arrived yet).
pub const ESC_WAIT_MS: u64 = 30;

#[derive(Default)]
pub struct KeyParser {
    buf: Vec<u8>,
    /// Inside a bracketed paste: the text so far.
    paste: Option<String>,
    esc_since: Option<u64>,
}

impl KeyParser {
    /// Feed bytes that arrived at `now_ms`; returns the keys they complete.
    pub fn feed(&mut self, bytes: &[u8], now_ms: u64) -> Vec<Key> {
        self.buf.extend_from_slice(bytes);
        self.esc_since = None;
        self.drain(now_ms)
    }

    /// Call with the clock now and then: releases a trailing lone ESC as the Esc key.
    pub fn poll(&mut self, now_ms: u64) -> Vec<Key> {
        if self.buf == [0x1b] {
            let since = *self.esc_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= ESC_WAIT_MS {
                self.buf.clear();
                self.esc_since = None;
                return vec![Key::Esc];
            }
        }
        Vec::new()
    }

    fn drain(&mut self, now_ms: u64) -> Vec<Key> {
        let mut out = Vec::new();
        loop {
            if let Some(text) = self.paste.as_mut() {
                const END: &[u8] = b"\x1b[201~";
                if let Some(at) = find(&self.buf, END) {
                    text.push_str(&String::from_utf8_lossy(&self.buf[..at]));
                    self.buf.drain(..at + END.len());
                    out.push(Key::Paste(self.paste.take().unwrap_or_default()));
                    continue;
                }
                // Keep a possible partial terminator; take the rest as paste text.
                let keep = END.len() - 1;
                if self.buf.len() > keep {
                    let take = self.buf.len() - keep;
                    text.push_str(&String::from_utf8_lossy(&self.buf[..take]));
                    self.buf.drain(..take);
                }
                break;
            }
            let Some(&first) = self.buf.first() else { break };
            match first {
                0x1b => {
                    if self.buf.len() == 1 {
                        self.esc_since = Some(now_ms);
                        break;
                    }
                    if self.buf[1] == b'[' || self.buf[1] == b'O' {
                        // CSI / SS3: parameters, then a final byte in 0x40..=0x7e.
                        let end = self.buf[2..].iter().position(|b| (0x40..=0x7e).contains(b)).map(|p| p + 3);
                        let Some(end) = end else { break };
                        let seq: Vec<u8> = self.buf.drain(..end).collect();
                        match seq.as_slice() {
                            b"\x1b[200~" => self.paste = Some(String::new()),
                            b"\x1b[A" | b"\x1bOA" => out.push(Key::Up),
                            b"\x1b[B" | b"\x1bOB" => out.push(Key::Down),
                            b"\x1b[C" | b"\x1bOC" => out.push(Key::Right),
                            b"\x1b[D" | b"\x1bOD" => out.push(Key::Left),
                            b"\x1b[Z" => out.push(Key::ShiftTab),
                            other => out.push(Key::Other(String::from_utf8_lossy(other).into_owned())),
                        }
                    } else {
                        // ESC followed by another key (Alt+key): report the Esc, keep the key.
                        self.buf.remove(0);
                        out.push(Key::Esc);
                    }
                }
                b'\r' | b'\n' => {
                    self.buf.remove(0);
                    // CRLF is one Enter.
                    if first == b'\r' && self.buf.first() == Some(&b'\n') {
                        self.buf.remove(0);
                    }
                    out.push(Key::Enter);
                }
                b'\t' => {
                    self.buf.remove(0);
                    out.push(Key::Tab);
                }
                0x7f | 0x08 => {
                    self.buf.remove(0);
                    out.push(Key::Backspace);
                }
                0x03 => {
                    self.buf.remove(0);
                    out.push(Key::CtrlC);
                }
                0x15 => {
                    self.buf.remove(0);
                    out.push(Key::CtrlU);
                }
                b if b < 0x20 => {
                    self.buf.remove(0);
                    out.push(Key::Other(format!("^{}", (b + 0x40) as char)));
                }
                _ => {
                    let len = utf8_len(first);
                    if self.buf.len() < len {
                        break;
                    }
                    let bytes: Vec<u8> = self.buf.drain(..len).collect();
                    out.extend(String::from_utf8_lossy(&bytes).chars().map(Key::Char));
                }
            }
        }
        out
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_enter_and_arrows() {
        let mut p = KeyParser::default();
        assert_eq!(p.feed(b"hi\r", 0), vec![Key::Char('h'), Key::Char('i'), Key::Enter]);
        assert_eq!(p.feed(b"\x1b[B\x1b[A", 0), vec![Key::Down, Key::Up]);
        assert_eq!(p.feed("\u{276f}".as_bytes(), 0), vec![Key::Char('\u{276f}')]);
    }

    #[test]
    fn a_lone_escape_waits_for_the_rest_then_is_esc() {
        let mut p = KeyParser::default();
        assert!(p.feed(b"\x1b", 100).is_empty());
        assert!(p.poll(110).is_empty(), "an arrow key may still be arriving");
        assert_eq!(p.poll(100 + ESC_WAIT_MS), vec![Key::Esc]);
        // an arrow split across two reads is still an arrow
        assert!(p.feed(b"\x1b", 200).is_empty());
        assert_eq!(p.feed(b"[A", 205), vec![Key::Up]);
    }

    #[test]
    fn bracketed_paste_is_one_key() {
        let mut p = KeyParser::default();
        assert!(p.feed(b"\x1b[200~line one\nline", 0).is_empty());
        assert_eq!(p.feed(b" two\x1b[201~x", 0), vec![Key::Paste("line one\nline two".into()), Key::Char('x')]);
    }

    #[test]
    fn control_keys() {
        let mut p = KeyParser::default();
        assert_eq!(p.feed(&[0x15, 0x03, 0x7f, b'\t'], 0), vec![Key::CtrlU, Key::CtrlC, Key::Backspace, Key::Tab]);
    }
}
