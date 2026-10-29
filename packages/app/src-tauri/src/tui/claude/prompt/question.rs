//! Claude Code's own question to the person: the AskUserQuestion menu.
//!
//! ```text
//! ──────────────────────────────── ☐ Colour
//! Pick a colour
//!
//! ❯ 1. Red
//!      Red
//!   2. Blue
//!      Blue
//!   3. Type something.
//! ──────────────────────────────────────
//!   4. Chat about this
//!
//! Enter to select · ↑/↓ to navigate · Esc to cancel
//! ```
//!
//! The menu is a question, not an approval: the card is kind `question`, one choice per option with its number as the id
//! (`"1"`, `"2"`) and the option's own words as the label. Nothing answers it but the person. Two rows are not options
//! to pick from a card: `Type something.` opens a free-text box and `Chat about this` leaves the menu; they stay in the
//! terminal (`terminal_only`). A multi-select question (rows with a checkbox) has no card choices at all: Enter only
//! toggles a box there, so it is answered in the terminal.
//!
//! Answering moves the pointer with arrow keys (the route adds the Enter), exactly like the permission menus. A number
//! key would select and submit by itself, and the Enter after it could answer the NEXT question of a multi-question form.

use serde_json::{json, Value};

use super::{alnum, generic, is_rule_row, rows, tail, POINTER};

/// Footer Claude draws under this menu.
const FOOTER: &str = "enter to select";
/// Rows that are never offered as a normal choice (lowercase, without the trailing full stop).
const TERMINAL_ONLY: [&str; 2] = ["type something", "chat about this"];
/// Most options a card lists.
const MAX_OPTIONS: usize = 9;
/// Most rows of the question above the options.
const MAX_QUESTION_ROWS: usize = 3;

pub(super) struct Row {
    pub(super) n: usize,
    pub(super) label: String,
    pub(super) pointer: bool,
}

/// `❯ 2. Blue` / `  2. Blue` -> (pointer, 2, "Blue").
pub(super) fn option_row(row: &str) -> Option<Row> {
    let trimmed = row.trim();
    let (pointer, rest) = match trimmed.strip_prefix(POINTER) {
        Some(rest) => (true, rest.trim_start()),
        None => (false, trimmed),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let label = rest[digits.len()..].strip_prefix('.')?;
    if !label.starts_with(char::is_whitespace) {
        return None;
    }
    let label = label.trim();
    (!label.is_empty()).then(|| Row { n: digits.parse().ok().unwrap_or(0), label: label.to_string(), pointer })
}

/// A row that is the tab strip of a multi-question form or the rule above the header (`──── ☐ Colour`).
fn is_header_row(row: &str) -> bool {
    let row = row.trim();
    is_rule_row(row) || row.contains('\u{2610}') || row.contains('\u{2612}') || row.contains('\u{2714}') || row.chars().filter(|c| *c == '\u{2500}').count() >= 8
}

fn is_terminal_only(label: &str) -> bool {
    let norm = label.trim().trim_end_matches('.').to_lowercase();
    TERMINAL_ONLY.contains(&norm.as_str())
}

fn is_checkbox(label: &str) -> bool {
    let label = label.trim_start();
    ["[ ]", "[x]", "[X]", "[\u{2714}]", "[\u{2713}]", "\u{2610}", "\u{2611}", "\u{2612}"].iter().any(|mark| label.starts_with(mark))
}

/// The AskUserQuestion menu at the end of `screen` as a card, or `None`.
pub(super) fn question_menu(agent_type: &str, screen: &str) -> Option<Value> {
    let all = rows(tail(screen));
    // The footer is the last row: a menu with anything under it was answered long ago.
    let footer = all.iter().rposition(|row| row.to_lowercase().contains(FOOTER))?;
    if all.len() - footer > 2 {
        return None;
    }
    // Options, bottom to top, down to the one numbered 1. Rows that are no option (descriptions, the rule inside the
    // menu) are skipped on the way.
    let mut found: Vec<Row> = Vec::new();
    let mut first_row = None;
    for index in (0..footer).rev() {
        if footer - index > 40 {
            return None;
        }
        if let Some(row) = option_row(&all[index]) {
            let expected = found.last().map_or(row.n, |last| last.n.saturating_sub(1));
            if row.n != expected || row.n == 0 {
                return None;
            }
            let is_first = row.n == 1;
            found.push(row);
            if is_first {
                first_row = Some(index);
                break;
            }
        }
    }
    let first_row = first_row?;
    found.reverse();
    if found.len() < 2 || found.len() > MAX_OPTIONS || !found.iter().any(|row| row.pointer) {
        return None;
    }
    // The question: the rows right above option 1, down to the form's header.
    let mut question: Vec<&str> = Vec::new();
    for row in all[..first_row].iter().rev() {
        if question.len() >= MAX_QUESTION_ROWS || is_header_row(row) || row.trim_start().starts_with(POINTER) {
            break;
        }
        question.push(row.trim());
    }
    question.reverse();
    let question = question.join(" ");
    if question.trim().is_empty() {
        return None;
    }

    let multi = found.iter().any(|row| is_checkbox(&row.label));
    let selected = found.iter().find(|row| row.pointer).map_or(1, |row| row.n);
    let mut choices: Vec<String> = Vec::new();
    let mut labels = serde_json::Map::new();
    let mut terminal_only: Vec<String> = Vec::new();
    let mut menu_options: Vec<Value> = Vec::new();
    for row in &found {
        let offered = !multi && !is_terminal_only(&row.label);
        let id = row.n.to_string();
        if offered {
            choices.push(id.clone());
            labels.insert(id.clone(), Value::String(row.label.clone()));
        } else {
            terminal_only.push(row.label.clone());
        }
        menu_options.push(json!({"n": row.n, "label": row.label, "choice": offered.then_some(id)}));
    }
    let summary = crate::pane_prompt::summary::clean(&question);
    let shown: Vec<&str> = found.iter().filter(|row| !multi && !is_terminal_only(&row.label)).map(|row| row.label.as_str()).collect();
    let mut prompt = generic::prompt_json(agent_type, "question", &format!("{question}\n{}", shown.join("\n")), choices);
    prompt["labels"] = Value::Object(labels);
    prompt["menu"] = json!({"selected": selected, "options": menu_options});
    prompt["summary"] = Value::String(summary);
    // The question's own words and options make the id: a redraw keeps it, the next question is another card.
    prompt["identity"] = Value::String(format!("question|{}|{}", alnum(&question), found.iter().map(|r| alnum(&r.label)).collect::<Vec<_>>().join("|")));
    if !terminal_only.is_empty() {
        prompt["terminal_only"] = json!(terminal_only);
    }
    if multi {
        prompt["multi_select"] = Value::Bool(true);
    }
    Some(prompt)
}

/// Whether the AskUserQuestion menu is on screen (the pane is waiting for the person).
pub(super) fn shows_question_menu(screen: &str) -> bool {
    question_menu("claude", screen).is_some()
}
