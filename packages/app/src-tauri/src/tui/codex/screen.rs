//! The interactive Codex CLI TUI: what its screen shows about whether a turn is running, a dialog is
//! up, or the composer is empty.

use crate::tui::StandbyScreen;

/// Trust and approval prompts Codex draws in the pane. A standby never answers one.
const DIALOG_PHRASES: [&str; 6] = [
    "do you trust the files in this folder",
    "do you trust the contents of this directory",
    "allow codex to work in this folder",
    "would you like to run the following command",
    "would you like to make the following edits",
    "yes, proceed",
];

/// Login and update screens the user has to deal with.
/// Blocking screens only. The "Update available!" banner Codex keeps above a ready composer is
/// not one (live capture `fixtures/standby_ready_live.txt`); the update menu shown first is
/// (`fixtures/update_prompt_live.txt`: "1. Update now ... 3. Skip until next version").
const ATTENTION_PHRASES: [&str; 5] = [
    "sign in with chatgpt",
    "provide your own api key",
    "update now",
    "skip until next version",
    "press enter to continue",
];

/// Placeholder text Codex shows in an empty composer. The composer keeps no separate state for
/// it, so a row matching one of these exactly is taken as empty; anything else is typed text.
/// Codex rotates its hints between releases: an unlisted hint reads as NotReady, never Ready.
const COMPOSER_PLACEHOLDERS: [&str; 5] = [
    "ask codex to do anything",
    "explain this codebase",
    "find and fix a bug in @filename",
    "implement {feature}",
    "write tests for @filename",
];

/// Whether the screen shows Codex working: the status row `Working (12s \u{2022} esc to interrupt)`.
pub(super) fn shows_work_in_progress(screen: &str) -> bool {
    screen.to_ascii_lowercase().contains("esc to interrupt")
}

/// Whether a pre-launched Codex pane is at an empty composer with no dialog or login screen on top.
/// Anything unrecognised is NotReady.
pub(super) fn standby_screen(screen: &str) -> StandbyScreen {
    let lower = screen.to_ascii_lowercase();
    if DIALOG_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::Dialog;
    }
    if ATTENTION_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::NeedsAttention;
    }
    if shows_work_in_progress(screen) {
        return StandbyScreen::NotReady;
    }
    // The composer is the last `\u{203a}` row: submitted messages are drawn with the same glyph above it.
    let composer = screen
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix('\u{203a}'));
    let empty = composer.is_some_and(|rest| {
        // The block cursor may sit in an empty composer.
        let rest = rest.trim().trim_end_matches('\u{258c}').trim().to_ascii_lowercase();
        rest.is_empty() || COMPOSER_PLACEHOLDERS.contains(&rest.as_str())
    });
    if empty && lower.contains("codex") {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-written from the Codex CLI's layout, not a live capture: banner, empty composer with a
    // placeholder hint, and the footer row.
    const READY: &str = ">_ OpenAI Codex (v0.1.0)\n\n  model: gpt-5-codex   directory: ~/project\n\n\u{203a} Ask Codex to do anything\n\n  ? for shortcuts\n";
    const READY_CURSOR: &str = ">_ OpenAI Codex (v0.1.0)\n\n\u{203a} \u{258c}\n\n  ? for shortcuts\n";
    // Hand-written: a turn in flight shows the status row above the composer.
    const WORKING: &str = ">_ OpenAI Codex (v0.1.0)\n\n\u{2022} Working (12s \u{2022} esc to interrupt)\n\n\u{203a} Ask Codex to do anything\n";
    // Hand-written: the trust dialog on first launch in a folder.
    const TRUST_DIALOG: &str = "> You are running Codex in ~/sandbox\n\n  Do you trust the files in this folder?\n\n\u{203a} 1. Yes, continue\n  2. No, quit\n\n  Press enter to continue\n";
    // Hand-written: an approval request for a shell command.
    const APPROVAL: &str = "  Would you like to run the following command?\n\n    $ cargo test\n\n\u{203a} 1. Yes, proceed (y)\n  2. No, and tell Codex what to do differently (esc)\n";
    // Hand-written: the login screen of a fresh install.
    const LOGIN: &str = ">_ Welcome to Codex\n\n  1. Sign in with ChatGPT\n  2. Provide your own API key\n";
    // Hand-written: the update notice shown above the composer.
    const UPDATE: &str = "Update available! 0.1.0 -> 0.2.0\nRun npm install -g @openai/codex to update.\n\u{203a} Ask Codex to do anything\n";
    // Hand-written: the text typed into the composer is not a placeholder.
    const TYPED: &str = ">_ OpenAI Codex (v0.1.0)\n\n\u{203a} fix the failing test in parser.rs\n";
    // Hand-written: the startup splash before the composer is drawn.
    const SPLASH: &str = ">_ OpenAI Codex (v0.1.0)\n\n  loading session...\n";

    #[test]
    fn a_live_capture_of_the_empty_composer_under_the_update_banner_is_ready() {
        // Real Codex 0.159 pane after skipping the update menu: banner on top, empty composer.
        let live = include_str!("fixtures/standby_ready_live.txt");
        assert_eq!(standby_screen(live), StandbyScreen::Ready);
    }

    #[test]
    fn a_live_capture_of_the_update_prompt_needs_attention() {
        // Captured from a real Codex 0.159 pane right after start: the update menu comes first.
        let live = include_str!("fixtures/update_prompt_live.txt");
        assert_eq!(standby_screen(live), StandbyScreen::NeedsAttention);
    }

    #[test]
    fn standby_screen_is_ready_only_at_an_empty_composer() {
        assert_eq!(standby_screen(READY), StandbyScreen::Ready);
        assert_eq!(standby_screen(READY_CURSOR), StandbyScreen::Ready, "a block cursor in an empty box");
        assert_eq!(standby_screen(TYPED), StandbyScreen::NotReady, "typed text in the box");
        assert_eq!(standby_screen(WORKING), StandbyScreen::NotReady, "a turn in progress");
        assert_eq!(standby_screen(SPLASH), StandbyScreen::NotReady, "startup splash");
        assert_eq!(standby_screen(""), StandbyScreen::NotReady);
    }

    #[test]
    fn dialogs_and_login_screens_are_not_ready() {
        assert_eq!(standby_screen(TRUST_DIALOG), StandbyScreen::Dialog);
        assert_eq!(standby_screen(APPROVAL), StandbyScreen::Dialog);
        assert_eq!(standby_screen(LOGIN), StandbyScreen::NeedsAttention);
        // The update banner above an empty composer is not blocking (live capture agrees).
        assert_eq!(standby_screen(UPDATE), StandbyScreen::Ready);
    }

    #[test]
    fn the_status_row_is_work_and_an_idle_composer_is_not() {
        assert!(shows_work_in_progress(WORKING));
        assert!(!shows_work_in_progress(READY));
    }
}
