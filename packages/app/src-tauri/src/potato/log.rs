//! The brain's log: a 200-line ring per bot (spec section 6.4). The brain's stdout and stderr are read
//! into it line by line. Each line is cleaned in this order:
//!
//! 1. the exact secret values (the model key, the bridge token) become `[hidden]`;
//! 2. `redact::text` (token shapes, known secrets);
//! 3. the line is cut to 300 characters.
//!
//! Lines go to the app log at debug level only, after the cleaning. The brain itself never logs message
//! text, environment variables or headers, but this file does not rely on that.

use std::collections::VecDeque;

pub const RING_LINES: usize = 200;
pub const LINE_CHARS: usize = 300;
pub const HIDDEN: &str = "[hidden]";

/// A secret shorter than this is not worth matching by value (it would also hide ordinary words).
const MIN_SECRET_CHARS: usize = 6;

/// Cleans one raw line. `secrets` are the exact values to hide.
pub fn clean(raw: &str, secrets: &[String]) -> String {
    let mut line = raw.trim_end_matches(['\r', '\n']).to_string();
    for secret in secrets {
        if secret.chars().count() >= MIN_SECRET_CHARS {
            line = line.replace(secret.as_str(), HIDDEN);
        }
    }
    let line = crate::redact::text(&line).into_inner();
    line.chars().take(LINE_CHARS).collect()
}

#[derive(Debug, Default)]
pub struct LogRing {
    lines: VecDeque<String>,
}

impl LogRing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cleans and keeps one line; the oldest line goes when the ring is full. Returns the cleaned line.
    pub fn push(&mut self, raw: &str, secrets: &[String]) -> String {
        let line = clean(raw, secrets);
        if self.lines.len() >= RING_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line.clone());
        line
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
