//! "Remember for all agents": one line added to the project memory from a selection in a chat.
//!
//! The selection is cleaned to one line (control characters out, whitespace collapsed), refused when it is
//! empty or holds a secret, capped at [`MAX_CHARS`], and added as `- <text>` unless the memory already has
//! that line (case-insensitive). [`forget_with`] removes that same line again (the toast's Undo).
//!
//! The error text starts with a stable code (`CODE: message`), the same shape the other commands use, so the
//! UI can explain each case.

use std::path::Path;

use crate::redact::{self, RedactContext};

use super::{project_enabled, read_memory, write_memory};

/// The longest line one selection adds to the memory.
pub const MAX_CHARS: usize = 300;

pub const EMPTY: &str = "EMPTY";
pub const SECRET: &str = "SECRET";
pub const MEMORY_OFF_APP: &str = "MEMORY_OFF_APP";
pub const MEMORY_OFF_PROJECT: &str = "MEMORY_OFF_PROJECT";

/// What [`remember_with`] did: the line as stored, and `added: false` when the memory already had it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    pub text: String,
    pub added: bool,
}

/// One line of plain text: control characters dropped, every run of whitespace a single space, leading
/// list markers (`- `) dropped. Not capped and not redacted (see [`remember_with`]).
pub fn clean(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .filter_map(|c| if c.is_whitespace() { Some(' ') } else if c.is_control() { None } else { Some(c) })
        .collect();
    let mut line = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    while line.starts_with("- ") {
        line = line[2..].trim_start().to_string();
    }
    line
}

/// Adds `raw` to the project memory as one `- ` line. `global` is the app-wide switch.
///
/// Redaction runs before the cap, so a secret cut at 300 characters cannot leak its start: a selection
/// that changes under redaction is refused, never stored half-masked.
pub fn remember_with(project: &Path, raw: &str, global: bool, ctx: &RedactContext) -> Result<Remembered, String> {
    if !global {
        return Err(format!("{MEMORY_OFF_APP}: Shared project memory is off in Settings."));
    }
    if !project_enabled(project) {
        return Err(format!("{MEMORY_OFF_PROJECT}: Shared memory is off for this project."));
    }
    let cleaned = clean(raw);
    if cleaned.is_empty() {
        return Err(format!("{EMPTY}: Select some text first."));
    }
    if redact::text_with(&cleaned, ctx).as_str() != cleaned {
        return Err(format!("{SECRET}: That text looks like a secret, so it was not saved."));
    }
    let text: String = cleaned.chars().take(MAX_CHARS).collect::<String>().trim_end().to_string();

    let memory = read_memory(project);
    if memory.lines().any(|line| same_line(line, &text)) {
        return Ok(Remembered { text, added: false });
    }
    // The memory's own bytes stay as they are; the new line goes after its last line break.
    let mut next = memory;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str("- ");
    next.push_str(&text);
    next.push('\n');
    write_memory(project, &next).map_err(|e| format!("SAVE_FAILED: Could not save the memory: {e}"))?;
    Ok(Remembered { text, added: true })
}

/// Removes the first line that is `- <text>` (case-insensitive). Every other byte of the memory stays.
/// Returns whether a line was removed.
pub fn forget_with(project: &Path, raw: &str) -> Result<bool, String> {
    let text = clean(raw);
    if text.is_empty() {
        return Ok(false);
    }
    let memory = read_memory(project);
    let mut removed = false;
    let mut kept = String::with_capacity(memory.len());
    for chunk in memory.split_inclusive('\n') {
        if !removed && same_line(chunk.trim_end_matches(['\r', '\n']), &text) {
            removed = true;
            continue;
        }
        kept.push_str(chunk);
    }
    if removed {
        write_memory(project, &kept).map_err(|e| format!("SAVE_FAILED: Could not save the memory: {e}"))?;
    }
    Ok(removed)
}

/// True when a memory line is `text`, with or without its `- ` marker, ignoring case and outer spaces.
fn same_line(line: &str, text: &str) -> bool {
    let line = line.trim();
    let line = line.strip_prefix("- ").unwrap_or(line).trim();
    line.to_lowercase() == text.to_lowercase()
}
