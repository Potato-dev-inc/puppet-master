//! The interactive OpenCode TUI: menus and selectors that block on the user.

use crate::pty::ansi::strip_ansi;
use crate::tui::StandbyScreen;
use once_cell::sync::Lazy;
use regex::Regex;

/// Permission and provider prompts OpenCode draws over the input. A standby never answers one.
const DIALOG_PHRASES: [&str; 3] = ["permission required", "allow once", "allow always"];

/// Provider and update screens the user has to deal with.
const ATTENTION_PHRASES: [&str; 5] = [
    "connect a provider",
    "select a provider",
    "enter your api key",
    "update available",
    "new version available",
];

/// Rows OpenCode draws while a turn runs.
const BUSY_PHRASES: [&str; 2] = ["esc interrupt", "esc to interrupt"];

/// Placeholder OpenCode shows in an empty input. Typed text replaces it, so a row that starts with
/// it is an empty input.
const INPUT_PLACEHOLDER: &str = "ask anything";

/// Whether the visible rows show OpenCode working.
pub(super) fn shows_work_in_progress(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    BUSY_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

/// Whether a pre-launched OpenCode pane is at an empty input with no menu, dialog or provider
/// screen on top. Anything unrecognised is NotReady.
pub(super) fn standby_screen(text: &str) -> StandbyScreen {
    let lower = text.to_ascii_lowercase();
    if looks_like_menu(text) || DIALOG_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::Dialog;
    }
    if ATTENTION_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::NeedsAttention;
    }
    if shows_work_in_progress(text) {
        return StandbyScreen::NotReady;
    }
    // The empty input shows its placeholder; typing replaces it. Matched anywhere on the screen:
    // OpenCode paints every cell of every row, so the headless screen's text comes back as one
    // long line (live capture `fixtures/standby_ready_live.txt`), not one line per row.
    let input_is_empty = strip_ansi(text).to_ascii_lowercase().contains(INPUT_PLACEHOLDER);
    if input_is_empty {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
}

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

    // Hand-written from the OpenCode layout, not a live capture: the input row with its placeholder
    // sits above a footer.
    const READY: &str = "  opencode v1.0.0\n\n  ┃ Ask anything... \"What is the tech stack of this project?\"\n  ~/project                    tab agents  ctrl+p commands\n";
    // Hand-written: text typed into the input replaces the placeholder.
    const TYPED: &str = "  opencode v1.0.0\n\n  ┃ fix the failing test in parser.rs\n  ~/project\n";
    // Hand-written: a turn in progress shows the interrupt hint in the footer.
    const WORKING: &str = "  ▣ Build · claude-sonnet\n  ⠹ Thinking...\n\n  ┃ Ask anything...\n  esc interrupt\n";
    // Hand-written: a permission prompt over the input.
    const PERMISSION: &str = "  Permission required\n  Run shell command: cargo test\n\n  Allow once   Allow always   Reject\n";
    // Hand-written: the provider screen of a fresh install.
    const PROVIDER: &str = "  Welcome to OpenCode\n\n  Connect a provider to get started\n  Run /connect\n";
    // Hand-written: the startup splash before the input is drawn.
    const SPLASH: &str = "   ___                   ___          _\n  |     opencode loading...\n";

    #[test]
    fn standby_screen_is_ready_only_at_an_empty_input() {
        assert_eq!(standby_screen(READY), StandbyScreen::Ready);
        assert_eq!(standby_screen(TYPED), StandbyScreen::NotReady, "typed text in the input");
        assert_eq!(standby_screen(WORKING), StandbyScreen::NotReady, "a turn in progress");
        assert_eq!(standby_screen(SPLASH), StandbyScreen::NotReady, "startup splash");
        assert_eq!(standby_screen(""), StandbyScreen::NotReady);
    }

    #[test]
    fn a_live_capture_of_the_empty_input_is_ready() {
        // Captured from a real OpenCode 1.19 pane (100x30) right after start: one glued line.
        let live = include_str!("fixtures/standby_ready_live.txt");
        assert_eq!(standby_screen(live), StandbyScreen::Ready);
    }

    #[test]
    fn menus_dialogs_and_provider_screens_are_not_ready() {
        assert_eq!(standby_screen(PERMISSION), StandbyScreen::Dialog);
        assert_eq!(standby_screen("1. Yes\n2. No\n↑↓ select  enter submit"), StandbyScreen::Dialog);
        assert_eq!(standby_screen(PROVIDER), StandbyScreen::NeedsAttention);
    }
}
