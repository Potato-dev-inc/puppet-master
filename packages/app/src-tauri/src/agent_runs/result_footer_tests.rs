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
