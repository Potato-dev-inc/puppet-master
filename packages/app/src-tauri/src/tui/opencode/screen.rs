//! The interactive OpenCode TUI: menus and selectors that block on the user.

use crate::pty::ansi::strip_ansi;
use once_cell::sync::Lazy;
use regex::Regex;

/// OpenCode TUI menus (numbered yes/no, submit hints).
static MENU_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)enter submit",
        r"(?i)esc dismiss",
        r"(?i)↑↓ select",
        r"(?i)\d+\.\s*yes\b",
        r"(?i)\d+\.\s*no\b",
        r"(?i)type your own answer",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid opencode tui regex"))
    .collect()
});

/// True when one of the last 12 non-empty lines shows an open menu.
pub(super) fn looks_like_menu(text: &str) -> bool {
    let clean = strip_ansi(text);
    let tail: String = clean
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    MENU_PATTERNS.iter().any(|re| re.is_match(&tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_numbered_yes_no_menu() {
        let text = "1. Yes\n2. No\n3. Type your own answer\n↑↓ select  enter submit  esc dismiss";
        assert!(looks_like_menu(text));
        assert!(!looks_like_menu("just some output"));
    }
}
