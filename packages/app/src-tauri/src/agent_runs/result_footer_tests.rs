//! NEW #5 (live VERIFY run, 2026-10-14): a visible run's result ended with the agent's own footer
//! rows (Claude: the permission-mode row and the effort indicator; Codex: the model/folder footer and
//! the `Worked for` separator). The inputs are the real captured results (fixtures under
//! `tui/<agent>/fixtures/`), cleaned the way a run's result is.

use super::completion::clean_extracted_result;

const CLAUDE_MANUAL: &str = include_str!("../tui/claude/fixtures/result_footer_manual_mode_live.txt");
const CLAUDE_EDIT: &str = include_str!("../tui/claude/fixtures/result_footer_edit_live.txt");
const CODEX: &str = include_str!("../tui/codex/fixtures/result_footer_live.txt");

#[test]
fn a_claude_result_has_no_permission_mode_or_effort_footer() {
    let cleaned = clean_extracted_result(CLAUDE_MANUAL, "Run npm test and tell me the result in one sentence.", "claude").expect("a reply");
    assert!(cleaned.contains("npm test passed"), "{cleaned}");
    for chrome in ["manual mode", "/effort", "for shortcuts", "for agents"] {
        assert!(!cleaned.contains(chrome), "{chrome:?} in {cleaned:?}");
    }
    let edited = clean_extracted_result(
        CLAUDE_EDIT,
        "Add a one-line comment at the very top of sum.js saying \"// verified\". Edit only sum.js. Do not run anything.",
        "claude",
    )
    .expect("a reply");
    assert!(edited.contains("I added // verified"), "{edited}");
    for chrome in ["manual mode", "/effort", "for shortcuts"] {
        assert!(!edited.contains(chrome), "{chrome:?} in {edited:?}");
    }
}

#[test]
fn a_codex_result_has_no_model_footer_or_worked_for_row() {
    let cleaned =
        clean_extracted_result(CODEX, "Read README.md and say one sentence about it. Don't edit files.", "codex").expect("a reply");
    assert!(cleaned.contains("README.md describes a tiny Node.js demo"), "{cleaned}");
    for chrome in ["GPT-6-Luna", "Worked for", "for shortcuts"] {
        assert!(!cleaned.contains(chrome), "{chrome:?} in {cleaned:?}");
    }
}

#[test]
fn the_same_rows_for_an_agent_without_those_footers_are_left_alone() {
    // Chrome is the profile's data: a profile that does not declare these rows keeps them.
    let cleaned = clean_extracted_result(CLAUDE_MANUAL, "Run npm test", "powershell");
    let text = cleaned.unwrap_or_default();
    assert!(text.contains("manual mode"), "{text}");
}

// ---- the update notice box and the banner at the start of a Codex result (VERIFY-R7 finding C) ----

/// The real final screen of the verify run (op 3f82886d, Codex 0.159.3, update pending) and the
/// screen right before its task was typed.
const CODEX_NOTICE_SCREEN: &str = include_str!("../tui/codex/fixtures/result_update_notice_screen_live.txt");
const CODEX_NOTICE_BASELINE: &str = include_str!("../tui/codex/fixtures/result_update_notice_baseline_live.txt");

#[test]
fn a_codex_result_does_not_start_with_the_update_notice_box_or_the_banner() {
    let task = "Read README.md and say one sentence about it. Don't edit files.";
    let evidence =
        super::completion::tui_turn_evidence(CODEX_NOTICE_SCREEN, CODEX_NOTICE_BASELINE, "codex").expect("new text on the screen");
    let cleaned = clean_extracted_result(&evidence, task, "codex").expect("a reply");
    for chrome in ["See full release notes", "github.com/openai/codex/releases", "OpenAI Codex (v", "\u{256f}", "\u{2502}"] {
        assert!(!cleaned.contains(chrome), "{chrome:?} in {cleaned:?}");
    }
    assert!(cleaned.contains("README.md describes a tiny Node project"), "{cleaned}");
    assert!(cleaned.starts_with("\u{2022} I\u{2019}ll read the README"), "the narration is the first row: {cleaned:?}");
}

#[test]
fn another_agents_result_keeps_rows_that_only_codex_declares_as_chrome() {
    let cleaned = clean_extracted_result(CODEX_NOTICE_SCREEN, "x", "powershell").unwrap_or_default();
    assert!(cleaned.contains("See full release notes"), "{cleaned}");
}

// ---- the folder row under the Codex banner (the e2e result that began with the worktree path) ----

/// The task of the live captures above. A screen laid out as they are, after the first message: the
/// banner, the folder on its OWN row (`fixtures/composer_unsent_live.txt`, `standby_ready_live.txt`),
/// the message and the reply, with a baseline taken before the banner was on the screen.
const CODEX_OWN_ROW_TASK: &str = "Reply with the single word OK. Do not use any tools.";
const CODEX_OWN_ROW_BASELINE: &str = "\r\n\u{203a} Ask Codex to do anything\r\n\r\n  GPT-6-Luna medium \u{b7} D:\\pm-demo\r\n  ? for shortcuts\r\n";
const CODEX_OWN_ROW_SCREEN: &str = "\r\n  >_ OpenAI Codex (v0.159.3)\r\n     D:\\pm-demo\r\n\r\n\r\n\u{203a} Reply with the single word OK. Do not use any tools.\r\n\r\n\r\n\u{2022} OK\r\n\r\n  Worked for 2s \u{2022} 9:43 AM\r\n\r\n\u{203a} Ask Codex to do anything\r\n\r\n  GPT-6-Luna medium \u{b7} D:\\pm-demo\r\n  ? for shortcuts\r\n";

#[test]
fn a_codex_result_does_not_start_with_the_folder_row_under_the_banner() {
    let evidence = super::completion::tui_turn_evidence(CODEX_OWN_ROW_SCREEN, CODEX_OWN_ROW_BASELINE, "codex").expect("new text on the screen");
    let cleaned = clean_extracted_result(&evidence, CODEX_OWN_ROW_TASK, "codex").expect("a reply");
    assert!(!cleaned.contains("pm-demo"), "the folder row leaked into {cleaned:?}");
    assert_eq!(cleaned, "\u{2022} OK");
}

#[test]
fn a_codex_reply_that_is_only_a_path_survives_when_no_banner_sits_above_it() {
    let baseline = "\r\n\u{203a} Ask Codex to do anything\r\n\r\n  ? for shortcuts\r\n";
    let screen = "\u{203a} Which folder are you in? Answer with the path only.\r\n\r\n\u{2022} D:\\pm-demo\r\n\r\n  Worked for 3s \u{2022} 9:44 AM\r\n\r\n\u{203a} Ask Codex to do anything\r\n\r\n  ? for shortcuts\r\n";
    let task = "Which folder are you in? Answer with the path only.";
    let evidence = super::completion::tui_turn_evidence(screen, baseline, "codex").expect("new text on the screen");
    let cleaned = clean_extracted_result(&evidence, task, "codex").expect("a reply");
    assert!(cleaned.contains("D:\\pm-demo"), "{cleaned:?}");
    // And a bare path row with nothing above it at all, as a TUI that drew no bullet would show it.
    let bare = super::completion::tui_turn_evidence("  D:\\pm-demo\r\n", "", "codex").expect("new text on the screen");
    assert_eq!(bare, "D:\\pm-demo");
}
