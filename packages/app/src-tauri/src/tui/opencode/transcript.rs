//! How the OpenCode TUI draws a conversation, read off the screen (the chat view's screen reader, and the reply
//! cleaning a delegated run shares with it). From real captures of OpenCode 1.19 (`fixtures/screen_*.txt`):
//!
//! - the person's message is a block of rows with a `┃` bar in front (an empty `┃` row above and below it); a shell
//!   command and its output, and the agent's thinking, sit in blocks with the same bar, told apart by their first row;
//! - a tool call is one row with a glyph (`→ Read notes.txt`, `✱ Glob ...`, `← Edit ...`, `$ command`), a pending one
//!   starts with `~`; its output sits indented under it;
//! - the answer is plain text, and a `▣  Build · <model> · 12.7s` row closes the turn;
//! - the input box is a block of `┃` rows ending in a `╹▀▀▀` edge, with the footer under it;
//! - a scrollbar of `█` runs down the right edge of the transcript, and in a pane wider than 120 columns a sidebar
//!   (context, tokens, MCP) sits to the right of it.

use once_cell::sync::Lazy;
use regex::Regex;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid opencode transcript regex")
}

/// The logo a fresh pane shows before any message.
static HOME_LOGO: Lazy<Regex> = Lazy::new(|| re(r"^\s*█▀▀█ █▀▀█ █▀▀█"));
/// The bottom edge of the input box.
static BOX_EDGE: Lazy<Regex> = Lazy::new(|| re(r"^\s*╹▀+\s*$"));
/// A row of the input box (and of a message, which has the same bar).
static BAR_ROW: Lazy<Regex> = Lazy::new(|| re(r"^\s*┃"));
/// The scrollbar and, beyond it, the sidebar: everything from the first `█` that has spaces in front of it.
static SCROLLBAR: Lazy<Regex> = Lazy::new(|| re(r"\s+█.*$"));
/// The first row of a bar block that is a shell command (`$ node -v`, with or without a `# title` row above it)...
static SHELL_BLOCK: Lazy<Regex> = Lazy::new(|| re(r"^(?:\$\s|#\s)"));
/// ... or the agent's thought, neither of which is the person's message.
static THOUGHT_BLOCK: Lazy<Regex> = Lazy::new(|| re(r"(?i)^(?:_?thinking:|\+\s*thought\b)"));
static MESSAGE_ROW: Lazy<Regex> = Lazy::new(|| re(r"^\s*┃\s+(\S.*?)\s*$"));
static MODEL_ROW: Lazy<Regex> = Lazy::new(|| re(r"^▣\s+\S+\s+·\s+(?P<model>[^·]+?)(?:\s+·\s+\d.*)?$"));

static CHROME: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"^┃\s*$",
        r"^╹▀+$",
        r"^/update\b",
        r"^● tip\b",
        r"^tab agents\b",
        r"ctrl\+p commands",
        r"^[■⬝\s]*esc (to )?interrupt",
        r"^permission required\b",
        r"^△",
    ]
    .into_iter()
    .map(re)
    .collect()
});

static TOOL_ROWS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [r"^[→←✱%◇◈⚙]\s\S", r"^\$\s\S", r"^~\s\S", r"^[\u{2800}-\u{28ff}]\s\S"].into_iter().map(re).collect()
});

static THINKING: Lazy<Vec<Regex>> = Lazy::new(|| [r"^_?thinking\b", r"^[+_]\s*thought\b"].into_iter().map(re).collect());

/// The conversation part of the screen: no sidebar or scrollbar, no input box, no footer. A fresh pane (the logo,
/// an empty input box) has none.
pub(super) fn transcript_text(text: &str) -> String {
    let rows: Vec<&str> = text.split('\n').map(|row| row.trim_end_matches('\r')).collect();
    if rows.iter().any(|row| HOME_LOGO.is_match(row)) {
        return String::new();
    }
    // The input box: the bar rows right above the last bottom edge, and everything below.
    let mut end = rows.iter().rposition(|row| BOX_EDGE.is_match(row)).unwrap_or(rows.len());
    while end > 0 && BAR_ROW.is_match(rows[end - 1]) {
        end -= 1;
    }
    let rows: Vec<String> = rows[..end].iter().map(|row| SCROLLBAR.replace(row, "").into_owned()).collect();
    let mut kept: Vec<String> = Vec::with_capacity(rows.len());
    let mut at = 0;
    while at < rows.len() {
        if !BAR_ROW.is_match(&rows[at]) {
            kept.push(rows[at].clone());
            at += 1;
            continue;
        }
        let block_end = (at..rows.len()).find(|&row| !BAR_ROW.is_match(&rows[row])).unwrap_or(rows.len());
        let block = &rows[at..block_end];
        let first = block.iter().map(|row| row.trim().trim_start_matches('┃').trim()).find(|text| !text.is_empty());
        match first {
            // A shell command: its `$` row stays as a tool row, its output (the rest of the box) is not conversation.
            Some(text) if SHELL_BLOCK.is_match(text) => {
                let command = block.iter().map(|row| row.trim().trim_start_matches('┃').trim()).find(|text| text.starts_with("$ "));
                kept.extend(command.map(|command| format!("     {command}")));
            }
            // A thought is not conversation.
            Some(text) if THOUGHT_BLOCK.is_match(text) => {}
            _ => kept.extend(block.iter().cloned()),
        }
        at = block_end;
    }
    kept.join("\n")
}

/// The person's message on one row of the transcript (`┃  text`).
pub(super) fn user_message_row(row: &str) -> Option<String> {
    MESSAGE_ROW.captures(row).map(|found| found[1].to_string())
}

pub(super) fn is_chrome_line(lower: &str) -> bool {
    CHROME.iter().any(|re| re.is_match(lower))
}

pub(super) fn is_tool_activity_line(lower: &str) -> bool {
    TOOL_ROWS.iter().any(|re| re.is_match(lower))
}

pub(super) fn is_thinking_line(lower: &str) -> bool {
    THINKING.iter().any(|re| re.is_match(lower))
}

/// The row that closes a turn (`▣  Build · <model> · 12.7s`).
pub(super) fn is_summary_line(lower: &str) -> bool {
    lower.starts_with('▣')
}

/// The model named on the newest `▣` row of `buffer`.
pub(super) fn footer_model(buffer: &str) -> Option<String> {
    buffer
        .lines()
        .rev()
        .map(|row| SCROLLBAR.replace(row, ""))
        .filter_map(|row| MODEL_ROW.captures(row.trim()).map(|found| found["model"].trim().to_string()))
        .find(|model| !model.is_empty())
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
