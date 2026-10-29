//! Claude Code's first-run screens: what a brand-new install asks before the input box shows.
//!
//! Real captures of v2.1.312 (a fresh config folder, a hidden terminal; `fixtures/first_run_*_2_1_312.txt`):
//!
//! ```text
//! Choose the text style that looks best with your terminal        Detected a custom API key in your environment
//! To change this later, run /theme
//!     Auto (match terminal)                                       ANTHROPIC_API_KEY: sk-ant-...
//!  ❯ ✔ Dark mode                                                  Do you want to use this API key?
//!     Light mode                                                    Yes
//!     ...                                                         ❯ No (recommended)
//!  ╌╌╌╌╌╌╌╌ (a code preview under the menu)                       Enter to confirm · Esc to cancel
//!
//! Select login method:                                            Security notes:
//!  ❯ 1. Claude account with subscription · ...                    1. Claude can make mistakes. ...
//!    2. Anthropic Console account · API usage billing             Press Enter to continue…
//! ```
//!
//! None of them is an approval and none is in the session file, so the pane read as idle (or running) while it waited
//! for a hand, and a message typed in Chat queued behind it for ever. Each is a card of kind `question` here, the same
//! shape as the `/login` menu (`login.rs`): one choice per row with its position as the id, the screen's own words on
//! the buttons, and the pointer moved with arrow keys (the route adds the Enter). The sign-in menu of a first run has
//! no title and no footer; `login.rs` reads it as it is (see the fixture test).
//!
//! Only text that was really seen is matched. The "Login successful. Press Enter to continue" and "Use Claude Code's
//! terminal setup?" screens were not reachable without signing in, so they are not named here; the first is still caught by
//! its footer, the second by the generic Chat card.

use serde_json::{json, Value};

use super::{alnum, generic, is_rule_row, rows, tail, POINTER};

/// What Esc sends (the API-key question says "Esc to cancel").
const ESC: &str = "\u{1b}";
/// Most rows a first-run menu has.
const MAX_OPTIONS: usize = 8;
/// A row longer than this is text, not a menu row.
const MAX_ROW: usize = 70;
/// Rows under the theme menu: the rule and the code preview.
const MAX_PREVIEW_ROWS: usize = 14;
/// Footer rows that end a menu (lowercase).
const FOOTERS: [&str; 2] = ["enter to confirm", "esc to cancel"];

const THEME_ANCHOR: &str = "choose the text style";
const THEME_HINT: &str = "to change this later";
const KEY_ANCHOR: &str = "do you want to use this api key";
const CONTINUE_FOOTER: &str = "press enter to continue";

/// The menu rows read from the rows after a question.
struct Menu {
    labels: Vec<String>,
    /// 1-based position of the pointer.
    selected: usize,
    /// How many rows the menu took.
    used: usize,
}

/// A check mark or bullet in front of a row's words (`✔ Dark mode`).
fn without_mark(text: &str) -> &str {
    let text = text.trim_start();
    ['\u{2714}', '\u{2713}', '\u{25cf}', '\u{25c9}', '\u{25cb}']
        .iter()
        .find_map(|mark| text.strip_prefix(*mark))
        .map_or(text, str::trim_start)
}

/// The rows of a menu with a pointer and no numbers, up to a rule or a footer.
fn pointer_menu(rows: &[String]) -> Option<Menu> {
    let mut labels: Vec<String> = Vec::new();
    let mut selected = None;
    let mut used = 0;
    for row in rows {
        let lower = row.to_lowercase();
        if is_rule_row(row) || FOOTERS.iter().any(|footer| lower.contains(footer)) {
            break;
        }
        let (pointer, text) = match row.strip_prefix(POINTER) {
            Some(rest) => (true, rest),
            None => (false, row.as_str()),
        };
        let label = without_mark(text);
        if label.is_empty() || label.chars().count() > MAX_ROW || labels.len() >= MAX_OPTIONS {
            return None;
        }
        if pointer {
            if selected.is_some() {
                return None;
            }
            selected = Some(labels.len() + 1);
        }
        labels.push(label.to_string());
        used += 1;
    }
    let selected = selected?;
    (labels.len() >= 2).then_some(Menu { labels, selected, used })
}

/// A button reads the first part of the row (`Dark mode`), not the small print after ` · `.
fn short(label: &str) -> String {
    label.split(" \u{b7} ").next().unwrap_or(label).trim().to_string()
}

struct Words<'a> {
    summary: &'a str,
    note: &'a str,
    /// Which screen (part of the id).
    name: &'a str,
}

fn menu_card(agent_type: &str, text: &str, menu: &Menu, words: &Words, cancel: bool) -> Value {
    let mut choices: Vec<String> = Vec::new();
    let mut labels = serde_json::Map::new();
    let mut options: Vec<Value> = Vec::new();
    for (index, label) in menu.labels.iter().enumerate() {
        let id = (index + 1).to_string();
        choices.push(id.clone());
        labels.insert(id.clone(), Value::String(short(label)));
        options.push(json!({"n": index + 1, "label": label, "choice": id}));
    }
    if cancel {
        choices.push("cancel".into());
        labels.insert("cancel".into(), Value::String("Cancel".into()));
    }
    let mut prompt = generic::prompt_json(agent_type, "question", text, choices);
    prompt["labels"] = Value::Object(labels);
    prompt["menu"] = json!({"selected": menu.selected, "options": options});
    prompt["summary"] = Value::String(words.summary.into());
    prompt["note"] = Value::String(words.note.into());
    if cancel {
        prompt["cancel_key"] = Value::String(ESC.into());
    }
    // The words of the rows make the id, not the pointer: moving it keeps the card.
    prompt["identity"] = Value::String(format!("first_run|{}|{}", words.name, menu.labels.iter().map(|label| alnum(label)).collect::<Vec<_>>().join("|")));
    prompt
}

/// "Choose the text style that looks best with your terminal" and its list of themes.
fn theme_picker(agent_type: &str, all: &[String]) -> Option<Value> {
    let anchor = all.iter().rposition(|row| row.to_lowercase().contains(THEME_ANCHOR))?;
    let mut start = anchor + 1;
    while all.get(start).is_some_and(|row| row.to_lowercase().contains(THEME_HINT)) {
        start += 1;
    }
    let menu = pointer_menu(&all[start..])?;
    // Under the menu there is only the rule and the code preview.
    if all.len() - (start + menu.used) > MAX_PREVIEW_ROWS {
        return None;
    }
    let text = all[anchor..start + menu.used].join("\n");
    let words = Words { summary: "Claude Code: pick a text style", note: "Pick how Claude Code looks in the terminal. You can change it later with /theme.", name: "theme" };
    Some(menu_card(agent_type, &text, &menu, &words, false))
}

/// "Do you want to use this API key?" with Yes and No rows.
fn api_key_question(agent_type: &str, all: &[String]) -> Option<Value> {
    let anchor = all.iter().rposition(|row| row.to_lowercase().contains(KEY_ANCHOR))?;
    let menu = pointer_menu(&all[anchor + 1..])?;
    // Only the footer may follow the menu.
    let after = &all[anchor + 1 + menu.used..];
    if after.len() > 2 || after.iter().any(|row| !FOOTERS.iter().any(|footer| row.to_lowercase().contains(footer))) {
        return None;
    }
    // The key itself is not copied into the card.
    let text = format!("{}\n{}", all[anchor], menu.labels.join("\n"));
    let words = Words { summary: "Claude Code found an API key in your environment", note: "Pick whether Claude Code uses the key it found.", name: "api_key" };
    Some(menu_card(agent_type, &text, &menu, &words, true))
}

/// A screen that ends with "Press Enter to continue…" (the security notes, a sign-in result).
fn press_enter(agent_type: &str, all: &[String]) -> Option<Value> {
    let last = all.last()?;
    if !last.to_lowercase().contains(CONTINUE_FOOTER) {
        return None;
    }
    // A menu with a pointer has its own card.
    if all[all.len().saturating_sub(8)..].iter().any(|row| row.starts_with(POINTER)) {
        return None;
    }
    let from = all.len().saturating_sub(12);
    let heading = all[from..]
        .iter()
        .position(|row| {
            let lower = row.to_lowercase();
            lower.starts_with("security notes") || lower.starts_with("login successful") || lower.starts_with("logged in")
        })
        .map(|at| from + at);
    let text = all[heading.unwrap_or(all.len().saturating_sub(6))..].join("\n");
    let lower = text.to_lowercase();
    let (summary, name) = if lower.contains("security notes") {
        ("Claude Code shows its security notes", "security_notes")
    } else if lower.contains("login successful") || lower.contains("logged in") {
        ("Claude Code signed you in", "signed_in")
    } else {
        ("Claude Code waits for you to continue", "continue")
    };
    let menu = Menu { labels: vec!["Continue".into()], selected: 1, used: 1 };
    let words = Words { summary, note: "Read the screen, then continue.", name };
    let mut prompt = menu_card(agent_type, &text, &menu, &words, false);
    // One button, with a plain id.
    prompt["choices"] = json!(["continue"]);
    prompt["labels"] = json!({"continue": "Continue"});
    prompt["menu"] = json!({"selected": 1, "options": [{"n": 1, "label": "Continue", "choice": "continue"}]});
    prompt["identity"] = Value::String(format!("first_run|{}|{}", name, alnum(&text)));
    Some(prompt)
}

/// `rows` cuts a row at three spaces, which is where the pointer sits on a row whose check-mark slot is empty
/// (the pointer, three spaces, `Light mode`): the lone pointer is put back in front of the words it belongs to.
fn rows_with_pointers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in rows(text) {
        match out.last_mut() {
            Some(last) if last.chars().eq(std::iter::once(POINTER)) => *last = format!("{POINTER} {row}"),
            _ => out.push(row),
        }
    }
    out
}

/// The first-run screen at the end of `screen` as a card, or `None`.
pub(super) fn first_run(agent_type: &str, screen: &str) -> Option<Value> {
    let all = rows_with_pointers(tail(screen));
    theme_picker(agent_type, &all).or_else(|| api_key_question(agent_type, &all)).or_else(|| press_enter(agent_type, &all))
}

/// Whether a first-run screen is on (the pane is waiting for the person).
pub(super) fn shows_first_run(screen: &str) -> bool {
    first_run("claude", screen).is_some()
}
