//! The interactive Codex CLI TUI: what its screen shows about whether a turn is running, a dialog is
//! up, or the composer is empty.

use crate::tui::{ComposerState, StandbyScreen, StartupGate};

/// The questions Codex asks before it trusts a folder.
const TRUST_PHRASES: [&str; 3] = [
    "do you trust the files in this folder",
    "do you trust the contents of this directory",
    "allow codex to work in this folder",
];

/// The first-run login choices.
const LOGIN_PHRASES: [&str; 2] = ["sign in with chatgpt", "provide your own api key"];

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

/// The text after the composer's `\u{203a}`: the last such glyph on the screen, up to the end of its
/// line. Submitted messages are drawn with the same glyph above it, and a multi-line composer
/// continues on unmarked rows below its first line.
///
/// The glyph is searched for anywhere in the text, not at the start of a line: Codex fills a row to
/// the last column, the emulator marks it wrapped, and `contents()` then glues it to the next row,
/// so on a pane with history the composer sits at the END of one long line that starts with an old
/// message (`fixtures/submitted_live.txt`). Reading the first row that starts with the glyph took that
/// old message for the composer.
fn composer_row(screen: &str) -> Option<&str> {
    let at = screen.rfind('\u{203a}')?;
    screen[at + '\u{203a}'.len_utf8()..].lines().next()
}

/// What the composer holds. A turn in flight (`esc to interrupt`) wins: Codex keeps the composer
/// visible under the status row, and text typed there is queued, not waiting to be submitted.
/// `Unknown` when no composer row is on screen (startup, a menu, a full-screen view).
pub(super) fn composer_state(screen: &str) -> ComposerState {
    // A startup menu marks its selected option with the composer's glyph; it is not an input box.
    if startup_gate(screen).is_some() {
        return ComposerState::Unknown;
    }
    if shows_work_in_progress(screen) {
        return ComposerState::Working;
    }
    match composer_row(screen) {
        None => ComposerState::Unknown,
        Some(rest) => {
            let rest = rest.trim().trim_end_matches('\u{258c}').trim().to_ascii_lowercase();
            if rest.is_empty() || COMPOSER_PLACEHOLDERS.contains(&rest.as_str()) {
                ComposerState::Empty
            } else {
                ComposerState::HasText
            }
        }
    }
}

/// Ctrl+U sixteen times. Ctrl+U clears the line before the cursor and does nothing in an empty
/// composer; on a blank line it joins it to the one above, so an N-line draft needs about 2N
/// presses. Checked live on 0.159.3: one press cleared a single line, sixteen in one write cleared
/// a four-line draft and a "[Pasted Content N chars]" placeholder, and an empty composer stayed
/// empty. Ctrl+C is not used: on an empty composer it starts Codex's quit confirmation.
pub(super) const CLEAR_COMPOSER: &str = "\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}";

/// Esc: the update menu's own footer says `esc skip`. It skips this launch's notice and nothing
/// else ("Update now" runs an installer, "Skip until next version" writes a setting).
const SKIP_KEY: &str = "\u{1b}";

/// Screens Codex shows before its composer takes input. The update MENU (options "Update now",
/// "Skip", "Skip until next version") is one; the boxed "Update available!" banner that stays
/// above a ready composer is not (`fixtures/standby_ready_live.txt`). Approvals of a command or an
/// edit are not startup screens either: they are prompts (`pane_prompt`).
pub(super) fn startup_gate(screen: &str) -> Option<StartupGate> {
    let lower = screen.to_ascii_lowercase();
    if lower.contains("update now") && lower.contains("skip until next version") {
        return Some(StartupGate {
            id: "update_available",
            summary: "Codex is offering an update",
            dismiss: Some(SKIP_KEY),
        });
    }
    if TRUST_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return Some(StartupGate {
            id: "workspace_trust",
            summary: "Codex asks whether to trust this folder",
            dismiss: None,
        });
    }
    if LOGIN_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return Some(StartupGate {
            id: "login",
            summary: "Codex needs you to sign in",
            dismiss: None,
        });
    }
    if lower.contains("press enter to continue") {
        return Some(StartupGate {
            id: "onboarding",
            summary: "Codex is waiting for you to continue",
            dismiss: None,
        });
    }
    None
}

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
    let composer = composer_row(screen);
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

    // Real captures from a live Codex 0.159.3 pane in D:/pm-demo (Windows ConPTY, 120 columns).
    // The panes had history, so rows that reach the last column are glued to the next one by the
    // emulator: the composer is at the END of one long line (see `composer_row`).
    const LIVE_UNSENT: &str = include_str!("fixtures/composer_unsent_live.txt");
    const LIVE_UNSENT_GLUED: &str = include_str!("fixtures/composer_unsent_glued_live.txt");
    const LIVE_SUBMITTED: &str = include_str!("fixtures/submitted_live.txt");
    const LIVE_WORKING: &str = include_str!("fixtures/working_live.txt");
    const LIVE_PASTED: &str = include_str!("fixtures/composer_pasted_live.txt");
    const LIVE_UPDATE_MENU: &str = include_str!("fixtures/update_prompt_live.txt");
    const LIVE_READY_BANNER: &str = include_str!("fixtures/standby_ready_live.txt");

    #[test]
    fn live_composer_with_text_that_enter_did_not_submit_is_has_text() {
        // Bug #3: the task was typed, the Enter became a line break, the pane sat idle.
        assert_eq!(composer_state(LIVE_UNSENT), ComposerState::HasText);
        assert_eq!(composer_state(LIVE_UNSENT_GLUED), ComposerState::HasText);
        assert_eq!(composer_state(LIVE_PASTED), ComposerState::HasText, "a [Pasted Content N chars] placeholder is text");
    }

    #[test]
    fn live_screens_after_a_submit_read_as_a_started_turn() {
        assert_eq!(composer_state(LIVE_WORKING), ComposerState::Working);
        // The reply is in, the composer is empty again. The old message at the START of the glued
        // line begins with the composer glyph and must not be taken for the composer.
        assert_eq!(composer_state(LIVE_SUBMITTED), ComposerState::Empty);
        assert_eq!(composer_state(LIVE_READY_BANNER), ComposerState::Empty);
    }

    #[test]
    fn a_startup_menu_is_not_a_composer() {
        assert_eq!(composer_state(LIVE_UPDATE_MENU), ComposerState::Unknown);
        assert_eq!(composer_state(""), ComposerState::Unknown);
    }

    #[test]
    fn the_standby_check_reads_the_composer_on_a_glued_screen_too() {
        // Before: the first row starting with the glyph was the old message, so a pane with
        // history never read as Ready.
        assert_eq!(standby_screen(LIVE_SUBMITTED), StandbyScreen::Ready);
        assert_eq!(standby_screen(LIVE_UNSENT_GLUED), StandbyScreen::NotReady);
    }

    #[test]
    fn live_update_menu_is_a_dismissible_startup_gate() {
        // Bug #4: this screen was taken for a run's answer.
        let gate = startup_gate(LIVE_UPDATE_MENU).expect("the update menu is a startup gate");
        assert_eq!(gate.id, "update_available");
        // Esc is the menu's own "esc skip": never Enter (that runs "Update now") and never the
        // "Skip until next version" row (that writes a setting).
        assert_eq!(gate.dismiss, Some("\u{1b}"));
    }

    #[test]
    fn the_banner_above_a_ready_composer_and_ordinary_screens_are_not_gates() {
        for screen in [
            LIVE_READY_BANNER, LIVE_UNSENT, LIVE_UNSENT_GLUED, LIVE_SUBMITTED, LIVE_WORKING, LIVE_PASTED,
            READY, WORKING, UPDATE, TYPED, SPLASH,
        ] {
            assert_eq!(startup_gate(screen), None, "{screen:.80}");
        }
    }

    #[test]
    fn trust_login_and_onboarding_screens_are_gates_nobody_dismisses_for_the_user() {
        for (screen, id) in [
            (TRUST_DIALOG, "workspace_trust"),
            (LOGIN, "login"),
            ("Welcome to Codex\n\n  Press Enter to continue\n", "onboarding"),
        ] {
            let gate = startup_gate(screen).unwrap_or_else(|| panic!("{screen}"));
            assert_eq!(gate.id, id);
            assert_eq!(gate.dismiss, None, "{id} needs the user");
        }
        // A command approval is a prompt (`pane_prompt`), not a startup screen.
        assert_eq!(startup_gate(APPROVAL), None);
    }

    #[test]
    fn the_clear_sequence_is_only_ctrl_u() {
        assert!(CLEAR_COMPOSER.chars().all(|ch| ch == '\u{15}'));
        assert!(CLEAR_COMPOSER.len() >= 8, "a multi-line draft needs about two presses per line");
    }

    #[test]
    fn the_status_row_is_work_and_an_idle_composer_is_not() {
        assert!(shows_work_in_progress(WORKING));
        assert!(!shows_work_in_progress(READY));
    }
}
