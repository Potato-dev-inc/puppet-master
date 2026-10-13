//! Plain-language activity line: "what is my agent doing right now?" in one short sentence.
//!
//! `TuiProfile::activity` is the per-agent hook. This module is its generic default: it combines
//! the pane status, the last tool-activity row on screen (`is_tool_activity_line`, which Cursor
//! and declarative profiles teach) and the parsed approval prompt into phrases like
//! "Running npm test", "Waiting: allow `npm test`?", "Idle". Agents that teach nothing get
//! status-level phrases ("Working"). No agent names appear here.

use super::TuiProfile;
use crate::pty::status::PaneStatus;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Idle,
    Working,
    Waiting,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Activity {
    pub kind: ActivityKind,
    /// One short sentence, at most `MAX_LEN` characters.
    pub text: String,
}

impl Activity {
    pub fn new(kind: ActivityKind, text: impl Into<String>) -> Self {
        Self { kind, text: clip(&text.into()) }
    }
}

pub const MAX_LEN: usize = 72;

/// Past/imperative verbs a tool row may start with, mapped to the present participle.
const VERBS: &[(&str, &str)] = &[
    ("ran", "Running"),
    ("run", "Running"),
    ("running", "Running"),
    ("executed", "Running"),
    ("edited", "Editing"),
    ("edit", "Editing"),
    ("editing", "Editing"),
    ("updated", "Editing"),
    ("update", "Editing"),
    ("modified", "Editing"),
    ("wrote", "Writing"),
    ("write", "Writing"),
    ("writing", "Writing"),
    ("created", "Creating"),
    ("create", "Creating"),
    ("added", "Adding"),
    ("deleted", "Deleting"),
    ("read", "Reading"),
    ("reading", "Reading"),
    ("viewed", "Reading"),
    ("opened", "Reading"),
    ("searched", "Searching"),
    ("search", "Searching"),
    ("grepped", "Searching"),
    ("grep", "Searching"),
    ("globbed", "Searching"),
    ("glob", "Searching"),
    ("listed", "Listing"),
    ("fetched", "Fetching"),
    ("fetch", "Fetching"),
];

/// Decoration a TUI draws in front of a tool row (bullets, tree glyphs, box edges, prompts).
fn is_decoration(c: char) -> bool {
    c.is_whitespace() || (!c.is_alphanumeric() && !matches!(c, '$' | '>' | '.' | '/' | '\\' | '~' | '"' | '\'' | '`'))
}

/// A tool row as a short present-tense phrase: "Grepped "def " in ." -> "Searching "def " in .",
/// "$ python -m pytest -q 9.3s" -> "Running python -m pytest -q".
pub fn humanize_tool_line(line: &str) -> String {
    let mut text = line.trim().trim_start_matches(is_decoration).trim().to_string();
    // A shell row: "$ cmd" or "> cmd".
    if let Some(rest) = text.strip_prefix('$').or_else(|| text.strip_prefix("> ")) {
        let command = strip_trailing_timer(rest.trim());
        return clip(&format!("Running {command}"));
    }
    // "(esc to interrupt)" style hints are chrome, not part of the phrase.
    if let Some(at) = text.to_ascii_lowercase().find(" (") {
        if text[at..].to_ascii_lowercase().contains("esc") {
            text.truncate(at);
        }
    }
    let (first, rest) = match text.split_once(' ') {
        Some((a, b)) => (a, b.trim()),
        None => (text.as_str(), ""),
    };
    let lower = first.trim_end_matches(':').to_ascii_lowercase();
    if let Some((_, verb)) = VERBS.iter().find(|(word, _)| *word == lower) {
        return clip(&if rest.is_empty() { (*verb).to_string() } else { format!("{verb} {rest}") });
    }
    clip(&capitalize(&text))
}

/// "python -m pytest -q 9.3s" -> "python -m pytest -q" (a trailing elapsed-time token).
fn strip_trailing_timer(command: &str) -> &str {
    match command.rsplit_once(' ') {
        Some((head, tail))
            if tail.len() <= 8
                && tail.chars().next().is_some_and(|c| c.is_ascii_digit())
                && tail.ends_with(['s', 'm', 'h']) =>
        {
            head.trim_end()
        }
        _ => command,
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// At most `MAX_LEN` characters, one line.
fn clip(text: &str) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > MAX_LEN {
        format!("{}...", one_line.chars().take(MAX_LEN - 3).collect::<String>().trim_end())
    } else {
        one_line
    }
}

/// The last non-empty visible line that `is_match` accepts, newest first.
pub fn last_matching_line<'a>(tail: &'a str, is_match: &dyn Fn(&str) -> bool) -> Option<&'a str> {
    tail.lines()
        .rev()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(24)
        .find(|line| is_match(line))
}

/// Generic activity for any profile (see module docs). `tail` is the visible screen tail with
/// ANSI already stripped; `status` is the pane's classified status.
pub fn default_activity<P: TuiProfile + ?Sized>(
    profile: &P,
    tail: &str,
    status: PaneStatus,
) -> Option<Activity> {
    Some(match status {
        PaneStatus::Error => {
            let line = last_matching_line(tail, &|l| {
                let lower = l.to_ascii_lowercase();
                lower.contains("error") || lower.contains("failed") || lower.contains("exception")
            });
            match line {
                Some(line) => Activity::new(ActivityKind::Error, format!("Error: {}", strip_error_label(line))),
                None => Activity::new(ActivityKind::Error, "Error"),
            }
        }
        PaneStatus::WaitingInput => Activity::new(ActivityKind::Waiting, waiting_phrase(profile, tail)),
        PaneStatus::Running => {
            let tool = last_matching_line(tail, &|l| profile.is_tool_activity_line(&l.to_ascii_lowercase()));
            match tool {
                Some(line) => Activity::new(ActivityKind::Working, humanize_tool_line(line)),
                None => Activity::new(ActivityKind::Working, "Working"),
            }
        }
        PaneStatus::Idle => Activity::new(ActivityKind::Idle, "Idle"),
    })
}

fn strip_error_label(line: &str) -> String {
    let trimmed = line.trim().trim_start_matches(is_decoration);
    let lower = trimmed.to_ascii_lowercase();
    for label in ["error:", "error -", "error "] {
        if lower.starts_with(label) {
            return trimmed[label.len()..].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn waiting_phrase<P: TuiProfile + ?Sized>(profile: &P, tail: &str) -> String {
    let Some(prompt) = profile.detect_prompt(tail, false) else {
        return "Waiting for you".into();
    };
    if let Some(command) = prompt.get("command").and_then(|v| v.as_str()).filter(|c| !c.trim().is_empty()) {
        return format!("Waiting: allow `{}`?", command.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let kind = prompt.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_ascii_lowercase();
    if kind.contains("trust") {
        "Waiting: trust this folder?".into()
    } else if kind.contains("command") || kind.contains("approval") {
        "Waiting: allow this command?".into()
    } else {
        "Waiting for you".into()
    }
}

#[cfg(test)]
mod tests;
