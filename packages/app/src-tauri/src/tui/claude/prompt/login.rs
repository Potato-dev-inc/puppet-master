//! Claude Code's own sign-in menu (`/login`, or a "Login expired" message that asks for it).
//!
//! ```text
//! Login
//! Claude Code can be used with your Claude subscription or billed based on API usage through your Console account.
//! Select login method:
//!  ❯ 1. Claude account with subscription · Pro, Max, Team, or Enterprise
//!    2. Anthropic Console account · API usage billing
//!    3. 3rd-party platform · Amazon Bedrock, Microsoft Foundry, Google Vertex AI
//! Esc to cancel
//! ```
//!
//! The session file has nothing of it, so Chat showed an empty pane while the menu waited in the terminal. The card is
//! kind `question` (the agent's own words on the buttons), one choice per option with its number as the id, plus
//! `cancel` (Esc). Picking an option moves the pointer with arrow keys and the route adds the Enter, like every other
//! Claude menu; the CLI then opens the browser by itself. Only the end of the screen counts: once anything else is
//! drawn under the menu it is gone.

use serde_json::{json, Value};

use super::question::option_row;
use super::{alnum, generic, rows, tail};

/// The line that names this menu (lowercase).
const ANCHOR: &str = "select login method";
const FOOTER: &str = "esc to cancel";
const MAX_OPTIONS: usize = 6;
/// What Esc sends to leave the menu.
const ESC: &str = "\u{1b}";

/// A button reads the first part of the row (`Claude account with subscription`), not the small print after ` · `.
fn short(label: &str) -> String {
    label.split(" \u{b7} ").next().unwrap_or(label).trim().to_string()
}

/// The sign-in menu at the end of `screen` as a card, or `None`.
pub(super) fn login_menu(agent_type: &str, screen: &str) -> Option<Value> {
    let all = rows(tail(screen));
    let anchor = all.iter().rposition(|row| row.to_lowercase().contains(ANCHOR))?;
    // Numbered options 1..N right under the anchor, then at most the footer.
    let mut found = Vec::new();
    let mut footer = false;
    for row in &all[anchor + 1..] {
        if let Some(option) = option_row(row) {
            if footer || option.n != found.len() + 1 {
                return None;
            }
            found.push(option);
        } else if row.to_lowercase().contains(FOOTER) {
            footer = true;
        } else {
            return None;
        }
    }
    if found.len() < 2 || found.len() > MAX_OPTIONS || !found.iter().any(|option| option.pointer) {
        return None;
    }
    let selected = found.iter().find(|option| option.pointer).map_or(1, |option| option.n);
    let mut choices: Vec<String> = Vec::new();
    let mut labels = serde_json::Map::new();
    let mut menu_options: Vec<Value> = Vec::new();
    for option in &found {
        let id = option.n.to_string();
        choices.push(id.clone());
        labels.insert(id.clone(), Value::String(short(&option.label)));
        menu_options.push(json!({"n": option.n, "label": option.label, "choice": id}));
    }
    choices.push("cancel".into());
    labels.insert("cancel".into(), Value::String("Cancel".into()));

    let mut prompt = generic::prompt_json(agent_type, "question", &all[anchor..].join("\n"), choices);
    prompt["labels"] = Value::Object(labels);
    prompt["menu"] = json!({"selected": selected, "options": menu_options});
    prompt["summary"] = Value::String("Claude Code needs you to sign in".into());
    prompt["note"] = Value::String("Pick how you sign in. A browser window opens.".into());
    prompt["cancel_key"] = Value::String(ESC.into());
    prompt["identity"] = Value::String(format!("sign_in|{}", found.iter().map(|option| alnum(&option.label)).collect::<Vec<_>>().join("|")));
    Some(prompt)
}

/// Whether the sign-in menu is on screen (the pane is waiting for the person).
pub(super) fn shows_login_menu(screen: &str) -> bool {
    login_menu("claude", screen).is_some()
}
