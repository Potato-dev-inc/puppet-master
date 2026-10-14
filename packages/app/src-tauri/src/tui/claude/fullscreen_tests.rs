//! Claude Code's "fullscreen renderer" (`/tui fullscreen`, or `CLAUDE_CODE_NO_FLICKER=1`).
//!
//! It draws on the terminal's alternate screen buffer and pins the input box to the bottom rows
//! instead of letting it follow the output. These are REAL captures (Claude Code 2.1.300, a
//! scratch `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_NO_FLICKER=1`, a ConPTY, 2026-10-15), the raw PTY
//! stream run through the emulator the way the app does. The conversations were driven by a local
//! stand-in for the Messages API (no network, no account), so the UI is the real one. How they were
//! made: `docs/features/claude-fullscreen-renderer.md`.
//!
//! What differs from the classic screen, and what readiness has to cope with:
//! - the whole screen is the alternate buffer: banner at the top, then the conversation, then a
//!   run of blank rows, then the pinned input box (rule, `❯` row, rule) and its footer;
//! - the effort indicator (`◐ medium · /effort`) is drawn on its own row ABOVE the box, not in
//!   the footer;
//! - the `/` command list is drawn ABOVE the box (its selected row starts with an indented `❯`);
//! - a dialog replaces the input box for as long as it is open.

use super::screen::{input_box, shows_work_in_progress, standby_screen, without_suggestion};
use crate::tui::{ComposerState, StandbyScreen, TuiProfile};

const EMPTY: &str = include_str!("fixtures/fullscreen_2_1_300_live_empty_120x30.ansi");
const EMPTY_NARROW: &str = include_str!("fixtures/fullscreen_2_1_300_live_empty_52x40.ansi");
const DRAFT: &str = include_str!("fixtures/fullscreen_2_1_300_live_draft_120x30.ansi");
const SLASH_MENU: &str = include_str!("fixtures/fullscreen_2_1_300_live_slash_menu_120x30.ansi");
const WORKING: &str = include_str!("fixtures/fullscreen_2_1_300_live_working_120x30.ansi");
const SHELL_RUNNING: &str = include_str!("fixtures/fullscreen_2_1_300_live_shell_running_120x30.ansi");
const PERMISSION: &str = include_str!("fixtures/fullscreen_2_1_300_live_permission_dialog_120x30.ansi");
const MODEL_DIALOG: &str = include_str!("fixtures/fullscreen_2_1_300_live_model_dialog_120x30.ansi");
const AFTER_TURN: &str = include_str!("fixtures/fullscreen_2_1_300_live_after_turn_120x30.ansi");
const MULTILINE_DRAFT: &str = include_str!("fixtures/fullscreen_2_1_300_live_multiline_draft_120x30.ansi");
const TRANSCRIPT_MODE: &str = include_str!("fixtures/fullscreen_2_1_300_live_transcript_mode_120x30.ansi");
const DIFF_PANEL: &str = include_str!("fixtures/fullscreen_2_1_300_live_diff_panel_120x30.ansi");
const INVALID_KEY_TURN: &str = include_str!("fixtures/fullscreen_2_1_300_live_invalid_key_turn_120x30.ansi");

/// The screen text readiness reads for a capture (`TuiProfile::input_screen`).
fn read(raw: &str, rows: u16, cols: u16) -> (vt100::Parser, String) {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(raw.as_bytes());
    let text = without_suggestion(parser.screen());
    (parser, text)
}

fn composer(raw: &str, rows: u16, cols: u16) -> ComposerState {
    let (_, text) = read(raw, rows, cols);
    input_box(&text)
}

#[test]
fn the_captures_really_are_the_fullscreen_layout() {
    for raw in [EMPTY, DRAFT, WORKING, PERMISSION] {
        let (parser, _) = read(raw, 30, 120);
        assert!(parser.screen().alternate_screen(), "the alternate screen buffer is on");
    }
    // The input box is pinned to the bottom rows, with blank rows between it and the banner.
    let (parser, _) = read(EMPTY, 30, 120);
    let rows: Vec<String> = parser.screen().rows(0, 120).collect();
    assert!(rows[1].contains("Claude Code v2.1.300"));
    assert!(rows[27].starts_with('\u{276f}') && rows[26].starts_with('\u{2500}') && rows[28].starts_with('\u{2500}'), "{rows:#?}");
    assert!(rows[25].contains("/effort"), "the effort indicator sits above the box: {:?}", rows[25]);
}

#[test]
fn an_empty_pinned_box_is_empty_at_wide_and_phone_width() {
    assert_eq!(composer(EMPTY, 30, 120), ComposerState::Empty);
    assert_eq!(composer(EMPTY_NARROW, 40, 52), ComposerState::Empty);
}

#[test]
fn typed_text_in_the_pinned_box_is_a_draft_even_under_the_slash_menu() {
    assert_eq!(composer(DRAFT, 30, 120), ComposerState::HasText);
    // The command list is drawn above the box; its selected row (`  ❯ /model`) is not the box.
    assert_eq!(composer(SLASH_MENU, 30, 120), ComposerState::HasText);
}

#[test]
fn a_turn_in_progress_is_working_whatever_row_the_hint_is_on() {
    assert_eq!(composer(WORKING, 30, 120), ComposerState::Working, "spinner row `Doing…` above the box");
    assert_eq!(composer(SHELL_RUNNING, 30, 120), ComposerState::Working, "`esc to interrupt` in the footer");
    let (_, text) = read(WORKING, 30, 120);
    assert!(shows_work_in_progress(&text));
}

#[test]
fn a_dialog_in_place_of_the_box_is_never_an_input_box_to_type_into() {
    assert_eq!(composer(PERMISSION, 30, 120), ComposerState::Unknown, "the permission menu");
    assert_eq!(composer(MODEL_DIALOG, 30, 120), ComposerState::Unknown, "the /model picker");
    // And the prompt reader still sees the permission request in this layout.
    let (parser, _) = read(PERMISSION, 30, 120);
    let screen = parser.screen().contents();
    let prompt = crate::tui::profile_for("claude").detect_prompt(&screen, false).expect("a permission prompt in the fullscreen layout");
    assert_eq!(prompt["kind"], "command_approval", "{prompt}");
    assert_eq!(prompt["command"], "echo hello > fake-out.txt", "{prompt}");
}

#[test]
fn a_failed_turn_leaves_an_empty_box_and_the_error_row_is_not_the_box() {
    // `Invalid API key` is in the conversation above the box: readiness reads the box, so it is
    // Empty, while a pre-launched standby with that text on screen still needs attention.
    assert_eq!(composer(INVALID_KEY_TURN, 30, 120), ComposerState::Empty);
    let (_, text) = read(INVALID_KEY_TURN, 30, 120);
    assert_eq!(standby_screen(&text), StandbyScreen::NeedsAttention);
}

#[test]
fn a_fresh_fullscreen_session_is_a_ready_standby() {
    let (_, text) = read(EMPTY, 30, 120);
    assert_eq!(standby_screen(&text), StandbyScreen::Ready);
}

#[test]
fn a_finished_turn_a_multi_line_draft_and_the_diff_panel_beside_the_chat() {
    // After a turn (the `done 0:43` summary above the box): empty, ready for the next task.
    assert_eq!(composer(AFTER_TURN, 30, 120), ComposerState::Empty);
    // A draft of three rows grows the box upward from the pinned bottom: still a draft.
    assert_eq!(composer(MULTILINE_DRAFT, 30, 120), ComposerState::HasText);
    // `/diff` opens a panel beside the conversation (fullscreen only). It stops above the box, so
    // the box keeps the full width and reads as it always does.
    assert_eq!(composer(DIFF_PANEL, 30, 120), ComposerState::Empty);
}

#[test]
fn transcript_mode_draws_no_input_box_so_nothing_is_typed_into_it() {
    // Ctrl+O: the whole screen is the transcript, a rule and the key hints. No box: Unknown, so a
    // task is parked rather than typed into a viewer that would take the keys as shortcuts.
    assert_eq!(composer(TRANSCRIPT_MODE, 30, 120), ComposerState::Unknown);
}

#[test]
fn a_dimmed_next_prompt_suggestion_in_the_pinned_box_is_not_a_draft() {
    // SYNTHETIC: the live capture above, plus Claude's faint (SGR 2) next-prompt suggestion drawn
    // after the `❯` of the pinned box (row 28, column 3), the way the classic layout draws it
    // (see `ghost_suggestion_2_1_300_live.ansi`). A real fullscreen suggestion needs a real model
    // turn; this one is spliced in to pin the layout-independent faint-cell rule.
    let with_suggestion = format!("{EMPTY}\u{1b}[28;3H\u{1b}[2mcommit this\u{1b}[m");
    let (parser, _) = read(&with_suggestion, 30, 120);
    assert!(parser.screen().contents().contains("commit this"), "the emulator sees the words");
    assert_eq!(composer(&with_suggestion, 30, 120), ComposerState::Empty, "readiness reads around faint cells");
    // The same words in normal intensity are a draft.
    let typed = format!("{EMPTY}\u{1b}[28;3Hcommit this");
    assert_eq!(composer(&typed, 30, 120), ComposerState::HasText);
}
