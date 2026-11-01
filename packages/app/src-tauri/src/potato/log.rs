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

// ---- why it stopped ---------------------------------------------------------------------------------------------

/// What the app itself writes into the ring when a brain ends or does not start. They end one attempt.
pub const MARKERS: [&str; 3] = ["the brain exited", "the brain did not say hello", "could not start the brain"];

/// The longest reason shown in a note.
const REASON_CHARS: usize = 160;

fn is_marker(line: &str) -> bool {
    MARKERS.iter().any(|m| line.starts_with(m))
}

/// A line that says what went wrong, or `None` for a stack frame, a source excerpt or a status line.
fn error_text(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("at ") || line.starts_with("Node.js v") || line.starts_with('^') || line.starts_with('{') || line.starts_with('}') {
        return None;
    }
    let brain_says = line.strip_prefix("potato-brain:").map(str::trim);
    let text = match brain_says {
        // The brain's own words: its refusals and failures, not its greeting and not a single failed turn.
        Some(rest) if !rest.is_empty() && !rest.contains(": ready") && !rest.starts_with("a turn failed") && !rest.starts_with("an unhandled error") => rest,
        Some(_) => return None,
        None if line.contains("Error:") || line.contains("Error [") => line,
        None => return None,
    };
    let text: String = text.chars().take(REASON_CHARS).collect();
    Some(text.trim_end_matches(['.', ' ']).to_string())
}

/// Why the brain stopped, in a few words, from the (already cleaned) ring: the last error line it printed since it
/// last got going (a line saying it is ready ends the search, so an old turn's trouble is not blamed), else what the
/// app saw last (the exit code, no hello, no start). `None` when the ring says nothing. A line the brain printed as
/// it died can reach the ring just after the app's own marker, so the search does not stop at a marker.
pub fn last_reason(lines: &[String]) -> Option<String> {
    if let Some(error) = lines.iter().rev().take_while(|l| !l.contains(": ready")).find_map(|l| error_text(l)) {
        return Some(error);
    }
    let marker = lines.iter().rev().find(|l| is_marker(l))?;
    Some(marker.chars().take(REASON_CHARS).collect::<String>().trim_end_matches(['.', ' ']).to_string())
}

/// The note for a brain that stopped for good: with the reason when the ring has one.
pub fn stopped_note(name: &str, lines: &[String]) -> String {
    match last_reason(lines) {
        Some(reason) => format!("{name}'s brain stopped: {reason}. Press Start again."),
        None => format!("{name}'s brain stopped. Press Start again."),
    }
}
