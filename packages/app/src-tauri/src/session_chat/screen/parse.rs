//! Cuts a pane's text into turns and reads each turn's tool rows and reply with the profile's own tables
//! (chrome, thinking, summary, tool activity, the transcript echo of a sent message) and the same reply cleaning
//! a delegated run's result goes through (`agent_runs::completion`). Pure: text in, turns out.

use std::collections::HashMap;

use crate::agent_runs::completion::{clean_extracted_result_in_turn, is_tui_chrome_line, tui_turn_evidence};
use crate::tui::TuiProfile;

/// Longest tool row kept, in characters.
const MAX_TOOL_CHARS: usize = 200;

/// One exchange as the screen shows it right now.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Parsed {
    /// The person's message. `None` for rows above the first message on screen, or when the TUI echoes none.
    pub user: Option<String>,
    /// Tool-activity rows, in screen order, as the agent printed them (trimmed).
    pub tools: Vec<String>,
    /// What the agent said after its last tool row, cleaned.
    pub reply: Option<String>,
}

impl Parsed {
    pub fn has_body(&self) -> bool {
        !self.tools.is_empty() || self.reply.is_some()
    }
}

/// The turns on `text`, oldest first. A turn starts at a row where the profile reads the person's message and
/// runs to the next such row. Rows above the first message form a turn of their own (no `user`) when they hold
/// anything but chrome. A profile that declares no echo gives one such turn: use `read_rows` for those.
pub(super) fn parse_turns(text: &str, profile: &dyn TuiProfile, agent_type: &str) -> Vec<Parsed> {
    let text = profile.strip_dialogs(text);
    // (message parts, rows from the first message row on); the first entry holds the rows above any message.
    let mut cuts: Vec<(Vec<String>, Vec<&str>)> = vec![(Vec::new(), Vec::new())];
    let mut in_message = false;
    for row in text.split('\n').map(|row| row.trim_end_matches('\r')) {
        let chrome = is_tui_chrome_line(row, agent_type);
        match (!chrome).then(|| profile.user_message_row(row)).flatten() {
            Some(part) if in_message => {
                let (parts, rows) = cuts.last_mut().expect("a cut exists");
                parts.push(part);
                rows.push(row);
            }
            Some(part) => {
                cuts.push((vec![part], vec![row]));
                in_message = true;
            }
            None => {
                cuts.last_mut().expect("a cut exists").1.push(row);
                // Blank and chrome rows (the empty rows of a boxed message) do not end a message.
                in_message &= chrome;
            }
        }
    }
    cuts.into_iter()
        .filter_map(|(parts, rows)| {
            let user = (!parts.is_empty()).then(|| parts.join(" "));
            let turn = read_rows(&rows, user.as_deref(), agent_type, profile);
            (user.is_some() || turn.has_body()).then_some(turn)
        })
        .collect()
}

/// One turn's tool rows and reply from its rows (`user` is the message echoed on the first of them, if any).
pub(super) fn read_rows(rows: &[&str], user: Option<&str>, agent_type: &str, profile: &dyn TuiProfile) -> Parsed {
    // The rows that echo the message are the message, not the reply: a short one would not be told from text.
    let body: Vec<&str> = rows.iter().copied().filter(|row| profile.user_message_row(row).is_none()).collect();
    let tools: Vec<String> =
        body.iter().filter(|row| profile.is_tool_activity_line(&row.trim().to_ascii_lowercase())).map(|row| tool_summary(row)).collect();
    // The reply is read the way a run's result is: after the last tool row, chrome and echoes of the message dropped.
    let reply = tui_turn_evidence(&body.join("\n"), "", agent_type)
        .and_then(|raw| clean_extracted_result_in_turn(&raw, user.unwrap_or(""), "", agent_type, user.is_some() && !tools.is_empty()));
    Parsed { user: user.map(str::to_string), tools, reply }
}

/// The rows of `text` that are new since `baseline`, in order, counting repeats (a row printed twice counts as
/// new again when the baseline had it once). Blank rows are not kept.
pub(super) fn rows_since<'a>(text: &'a str, baseline: &str) -> Vec<&'a str> {
    let mut before: HashMap<&str, usize> = HashMap::new();
    for row in baseline.lines().map(str::trim).filter(|row| !row.is_empty()) {
        *before.entry(row).or_insert(0) += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    text.lines()
        .filter(|row| !row.trim().is_empty())
        .filter(|row| {
            let count = seen.entry(row.trim()).or_insert(0);
            *count += 1;
            *count > *before.get(row.trim()).unwrap_or(&0)
        })
        .collect()
}

/// A tool row as one short line: the spinner glyph a running tool wears is dropped, so the row does not change
/// with every frame.
fn tool_summary(row: &str) -> String {
    let bare = row.trim().trim_start_matches(|ch: char| ch.is_whitespace() || ('\u{2800}'..='\u{28ff}').contains(&ch));
    bare.chars().take(MAX_TOOL_CHARS).collect()
}
