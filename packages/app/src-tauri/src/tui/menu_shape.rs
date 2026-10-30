//! "The pane is asking", read from the SHAPE of the screen.
//!
//! A profile that does not describe a dialog (no `classify_screen` for it, no `[[tui.prompt]]` for it, or a table the
//! wizard never wrote) used to leave the pane `running` and then `idle` behind the question, and the Inbox said
//! "Nothing needs you". Two generic rules close that gap (`pty::status::classify_agent_observation`):
//!
//! 1. the profile's own prompt tables match the visible tail (`TuiProfile::declares_prompt`);
//! 2. a menu is the LAST thing on the screen: a cursor row (`❯` / `>` / `›`) among listed options, and a menu hint
//!    footer under it: a short row that names a key (`enter`, `esc`, an arrow, `ctrl+`; see `names_a_key`), one of the
//!    known hint phrases (`enter to select`, `navigate`, ...), or the profile's own `[tui.menu]`.
//!
//! The second rule is deliberately strict, because the screens it must NOT take are common: an idle input box (`> `
//! with a hint line), a finished reply that contains a numbered list, a draft, a spinner. Each of those lacks the
//! cursor row among options, or the hint footer, or has more text under the menu than a footer.
//!
//! The menu itself is read by the Agent Wizard's reader (`parse_menu`), not by a second parser.

use super::TuiProfile;
use crate::tui::declarative::wizard::visible::answer::{parse_menu, MenuKind};

/// Words a menu footer uses to say how to move and choose. Lowercase; a profile adds its own with
/// `TuiProfile::menu_hints` (or `[tui.menu] regex`, which `looks_like_menu` reads).
pub const DEFAULT_MENU_HINTS: &[&str] = &[
    "enter to select",
    "enter to confirm",
    "enter to choose",
    "enter select",
    "enter confirm",
    "\u{2191}/\u{2193}",
    "\u{2191}\u{2193}",
    "up/down",
    "navigate",
    "esc to cancel",
    "esc to go back",
    "arrow keys to",
];

/// Rows under the last option that may still belong to the menu (a footer, a rule, a second hint line).
const MAX_ROWS_AFTER_MENU: usize = 3;

fn is_rule_row(row: &str) -> bool {
    let rule = |ch: char| matches!(ch, '\u{2500}'..='\u{257f}' | '-' | '=' | '_');
    row.chars().count() >= 3 && row.chars().all(rule)
}

/// The longest row (trimmed, in characters) that can be a footer: a footer is a few words, prose is not.
const MAX_FOOTER_CHARS: usize = 100;

/// Whole words that name a key.
const KEY_WORDS: &[&str] = &["enter", "return", "esc", "escape", "space", "tab"];

/// Whether a short row names a key a hand would press: a whole word (`enter`, `esc`, ...), an arrow glyph, or `ctrl+`.
/// Any agent's footer says it, so no list of exact phrases is needed.
fn names_a_key(row: &str) -> bool {
    let row = row.trim();
    if row.chars().count() > MAX_FOOTER_CHARS {
        return false;
    }
    if row.contains(['\u{2191}', '\u{2193}', '\u{2190}', '\u{2192}']) {
        return true;
    }
    let lower = row.to_lowercase();
    if lower.contains("ctrl+") || lower.contains("ctrl-") {
        return true;
    }
    lower.split(|ch: char| !ch.is_alphanumeric()).any(|word| KEY_WORDS.contains(&word))
}

fn says_hint(row: &str, hints: &[&str]) -> bool {
    let lower = row.to_lowercase();
    hints.iter().any(|hint| lower.contains(hint)) || names_a_key(row)
}

/// Whether the last rows of `visible_tail` (ANSI-stripped, the last 12 non-empty rows) are a menu waiting for a pick.
pub fn asks(profile: &dyn TuiProfile, visible_tail: &str) -> bool {
    // A pane that shows work in progress is not at a question, whatever list it printed.
    if profile.busy_marker(visible_tail) == Some(true) {
        return false;
    }
    let rows: Vec<&str> = visible_tail.lines().map(str::trim_end).filter(|r| !r.trim().is_empty()).collect();
    if rows.len() < 3 {
        return false;
    }
    let Some(menu) = parse_menu(visible_tail) else { return false };
    if !matches!(menu.kind, MenuKind::Arrow | MenuKind::Numbered) || menu.options.len() < 2 || menu.highlighted().is_none() {
        return false;
    }
    let Some(last) = menu.options.last() else { return false };
    let Some(at) = rows.iter().rposition(|row| row.contains(last.text.trim())) else { return false };
    let after = &rows[at + 1..];
    if after.is_empty() || after.len() > MAX_ROWS_AFTER_MENU {
        return false;
    }
    let hints = profile.menu_hints();
    let tail_menu_hint = profile.looks_like_menu(&after.join("\n"));
    let footer = after.iter().any(|row| says_hint(row, hints)) || tail_menu_hint;
    // Under the footer there is nothing but hints and rules: a menu with an input box under it was answered long ago.
    let only_chrome = after.iter().all(|row| is_rule_row(row.trim()) || says_hint(row, hints) || tail_menu_hint);
    footer && only_chrome
}

#[cfg(test)]
#[path = "menu_shape_tests.rs"]
mod tests;
