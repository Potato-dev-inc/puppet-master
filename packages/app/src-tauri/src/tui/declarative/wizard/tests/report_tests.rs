//! The error format for an AI (`report`, `hints`) and the request preparation for the local drafter.

use crate::tui::declarative::wizard::hints::fix_for;
use crate::tui::declarative::wizard::report::{excerpt, render, CheckError, Source, EXCERPT_LINES, EXCERPT_LINE_CHARS};


#[test]
fn nothing_to_fix_renders_as_empty_text() {
    assert_eq!(render(&[], Source::Verify), "");
}

#[test]
fn the_text_numbers_every_problem_and_gives_problem_and_fix() {
    let errors = vec![
        CheckError::new("validate", "AGENT_DEF_INCONSISTENT", "headless.output.result.path", "this selector needs a `path`").at(Some(24), Some(1)),
        CheckError::new("fixture", "FIXTURE_FAILED", "headless_resume.jsonl", "result: expected \"pong2\", got \"\"")
            .with_excerpt("{\"a\":1}\n\n{\"b\":2}\n"),
        CheckError::new("wizard", "WIZARD_NO_HEADLESS", "", "no headless"),
    ];
    let text = render(&errors, Source::Verify);
    let expected_start = "The checker found 3 problems. Fix every one and reply with the complete corrected file in one ```toml block.\n";
    assert!(text.starts_with(expected_start), "{text}");
    assert!(text.contains("1. [validate] AGENT_DEF_INCONSISTENT at `headless.output.result.path` (line 24, column 1)\n   Problem: this selector needs a `path`\n   Fix: "), "{text}");
    assert!(text.contains("2. [fixture] FIXTURE_FAILED at `headless_resume.jsonl`\n"), "{text}");
    assert!(text.contains("   Real output (first lines):\n   {\"a\":1}\n   {\"b\":2}\n"), "blank lines are dropped: {text}");
    assert!(text.contains("3. [wizard] WIZARD_NO_HEADLESS\n   Problem: no headless\n"), "no path, no 'at': {text}");
    let live = render(&errors[..1], Source::LiveTest);
    assert!(live.starts_with("The live test ran your file against the real program and found 1 problem."), "{live}");
}

#[test]
fn excerpts_are_limited_in_lines_and_width() {
    let many: String = (0..100).map(|i| format!("line {i}\n")).collect();
    assert_eq!(excerpt(&many).lines().count(), EXCERPT_LINES);
    let wide = "x".repeat(2_000);
    let cut = excerpt(&wide);
    assert_eq!(cut.chars().count(), EXCERPT_LINE_CHARS + 3);
    assert!(cut.ends_with("..."));
    assert_eq!(excerpt(" \n\n"), "");
}

#[test]
fn every_error_code_the_wizard_produces_has_advice() {
    for code in [
        "AGENT_DEF_INVALID_TOML",
        "AGENT_DEF_UNKNOWN_FIELD",
        "AGENT_DEF_INCONSISTENT",
        "AGENT_DEF_BAD_PLACEHOLDER",
        "AGENT_DEF_BAD_REGEX",
        "AGENT_DEF_TOO_LARGE",
        "AGENT_DEF_BAD_ID",
        "AGENT_DEF_RESERVED_ID",
        "AGENT_DEF_BAD_BINARY",
        "AGENT_DEF_SHELL_INTERPRETER",
        "WIZARD_NO_HEADLESS",
        "WIZARD_ID_TAKEN",
        "WIZARD_BINARY_MISMATCH",
        "WIZARD_BINARY_UNRESOLVED",
        "WIZARD_KIND",
        "FIXTURE_FAILED",
        "LIVETEST_NO_OUTPUT",
        "LIVETEST_TIMEOUT",
        "LIVETEST_SPAWN_FAILED",
        "LIVETEST_PONG_FAILED",
        "LIVETEST_RESUME_FAILED",
        "LIVETEST_SENTENCE_FAILED",
        "WIZARD_TEST_WORD",
        "LIVETEST_BAD_MODEL_FAILED",
        "CONFORMANCE_EXTRACT_RESULT_REJECTS_GARBAGE_AND_FAILED_EXITS",
        "CONFORMANCE_SOMETHING_NEW",
    ] {
        assert!(!fix_for(code, "").is_empty(), "{code}");
    }
    assert!(fix_for("AGENT_DEF_INCONSISTENT", "headless.output.result.path").contains("path = "));
    assert!(fix_for("AGENT_DEF_INCONSISTENT", "headless.output").contains("[headless.output]"));
    assert_eq!(fix_for("AGENT_DEF_INCONSISTENT", "x"), fix_for("AGENT_DEF_INCONSISTENT", "x"), "same mistake, same advice");
}
