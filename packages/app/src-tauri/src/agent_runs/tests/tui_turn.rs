//! Reading a pane turn: evidence, idle detection, echo and chrome filtering.

use crate::agent_runs::completion::{
    clean_extracted_result, observe_tui_turn, tui_turn_evidence, TuiTurnDecision, TuiTurnWatch,
    NOT_SUBMITTED_TICKS,
};
use crate::operations::ResultCapture;

#[test]
fn tui_turn_evidence_ignores_unchanged_idle_chrome() {
    let idle = "Add follow-up >";
    assert!(tui_turn_evidence(idle, idle).is_none());
}

#[test]
fn tui_turn_evidence_returns_new_reply_after_idle_baseline() {
    let before = "previous chatter\nAdd follow-up >";
    let after = "previous chatter\nPUPPET_MASTER_AGENT_MODE_OK\n\nAdd follow-up >";
    assert_eq!(
        tui_turn_evidence(after, before).as_deref(),
        Some("PUPPET_MASTER_AGENT_MODE_OK")
    );
}

#[test]
fn tui_turn_evidence_ignores_chrome_only_delta() {
    let before = "hello\nAdd follow-up >";
    let after = "hello\n→ Add follow-up";
    assert!(tui_turn_evidence(after, before).is_none());
}

fn tick_until_idle(
    watch: &mut TuiTurnWatch,
    status: &str,
    screen: &str,
    has_prompt: bool,
    times: u8,
) -> TuiTurnDecision {
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..times {
        last = observe_tui_turn(watch, status, screen, has_prompt);
    }
    last
}

#[test]
fn fast_reply_never_observed_busy_completes_with_result_not_timeout() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    let screen = "The connection fails because the token expired.\nAdd follow-up >";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    match decision {
        TuiTurnDecision::Complete { result, capture } => {
            assert!(result.contains("token expired"));
            assert_eq!(capture, ResultCapture::Inferred);
        }
        other => panic!("expected complete, got {other:?}"),
    }
}

#[test]
fn blank_baseline_banner_text_with_no_work_is_not_a_completed_turn() {
    // The baseline snapshot raced the ConPTY screen and came back blank, so the banner, model and
    // working directory that are already on screen look like "new output".
    let mut watch = TuiTurnWatch::from_baseline("").with_task("do X");
    let screen = "Cursor Agent v2026.10.01-abc\n~/proj\nAdd a follow-up";
    for tick in 0..6 {
        let decision = observe_tui_turn(&mut watch, "idle", screen, false);
        assert!(
            !matches!(decision, TuiTurnDecision::Complete { .. }),
            "tick {tick}: banner text must not complete a turn that did nothing: {decision:?}"
        );
    }
}

#[test]
fn blank_baseline_startup_furniture_beyond_the_known_chrome_is_not_a_completed_turn() {
    // Same race, but the idle screen carries a bare non-Composer model footer, a trust-folder
    // line and an update notice: none are in the chrome list, so they read as "new output".
    let mut watch = TuiTurnWatch::from_baseline("").with_task("do X");
    let screen =
        "Cursor Agent v2026.10.01-abc\n~/proj\nGPT-5\nUpdate available: run agent update\nAdd a follow-up";
    for tick in 0..6 {
        let decision = observe_tui_turn(&mut watch, "idle", screen, false);
        assert!(
            !matches!(decision, TuiTurnDecision::Complete { .. }),
            "tick {tick}: startup furniture must not complete a turn that did nothing: {decision:?}"
        );
    }
}

#[test]
fn blank_baseline_turn_completes_once_activity_was_observed() {
    let mut watch = TuiTurnWatch::from_baseline("").with_task("do X");
    let _ = observe_tui_turn(&mut watch, "running", "Cursor Agent v1\nworking", false);
    let screen = "Cursor Agent v1\nThe answer is 42.\nAdd a follow-up";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    assert!(
        matches!(decision, TuiTurnDecision::Complete { .. }),
        "{decision:?}"
    );
}

#[test]
fn blank_baseline_turn_completes_once_the_task_echo_is_on_screen() {
    let mut watch = TuiTurnWatch::from_baseline("").with_task("summarize the readme file");
    let echoing = "Cursor Agent v1\n> summarize the readme file\nAdd a follow-up";
    // Echo seen on a busy-looking-idle tick, then the reply lands.
    let _ = observe_tui_turn(&mut watch, "idle", echoing, false);
    let screen =
        "Cursor Agent v1\n> summarize the readme file\nIt documents the CLI.\nAdd a follow-up";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    assert!(
        matches!(decision, TuiTurnDecision::Complete { .. }),
        "{decision:?}"
    );
}

#[test]
fn repeated_identical_answers_still_count_as_a_new_turn_result() {
    let previous = "PUPPET_MASTER_AGENT_MODE_OK\nAdd follow-up >";
    let mut watch = TuiTurnWatch::from_baseline(previous);
    let screen = "PUPPET_MASTER_AGENT_MODE_OK\nPUPPET_MASTER_AGENT_MODE_OK\nAdd follow-up >";
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 2);
    match decision {
        TuiTurnDecision::Complete { result, capture } => {
            assert_eq!(result, "PUPPET_MASTER_AGENT_MODE_OK");
            assert_eq!(capture, ResultCapture::Inferred);
        }
        other => panic!("expected complete, got {other:?}"),
    }
}

#[test]
fn stale_pre_dispatch_output_is_not_the_new_result() {
    let stale = "old answer still on screen\nAdd follow-up >";
    let mut watch = TuiTurnWatch::from_baseline(stale);
    let decision = tick_until_idle(&mut watch, "idle", stale, false, NOT_SUBMITTED_TICKS);
    assert_eq!(decision, TuiTurnDecision::NotSubmitted);
    assert!(tui_turn_evidence(stale, stale).is_none());
}

#[test]
fn permission_prompt_is_not_completion() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    let screen = "Allow once  Don't ask again\nThe connection fails because...\nAdd follow-up >";
    let decision = tick_until_idle(&mut watch, "idle", screen, true, 4);
    assert_eq!(decision, TuiTurnDecision::WaitingInput);
}

#[test]
fn pasted_prompt_fragments_are_not_an_assistant_result() {
    let task = "Remember the word READY ORCHID-731 and return it when asked.\n\nAcceptance criteria:\n- Return READY ORCHID-731";
    let screen = "\
[Pasted text #1 +36 lines]
Remember the word READY ORCHID-731 and return it when asked.
Acceptance criteria:
- Return READY ORCHID-731
Add follow-up >";
    assert!(clean_extracted_result(
        "[Pasted text #1 +36 lines]\nAcceptance criteria:\n- Return READY ORCHID-731",
        task
    )
    .is_none());
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >").with_task(task);
    let decision = tick_until_idle(&mut watch, "idle", screen, false, 4);
    assert!(
        !matches!(decision, TuiTurnDecision::Complete { .. }),
        "prompt paste must not complete the turn: {decision:?}"
    );
}

#[test]
fn conversation_replay_does_not_nest_prior_context() {
    let stored =
        "second question\n\nPrior context:\nUser: first\nAssistant: a\n\nAcceptance criteria:\n- x";
    assert_eq!(
        crate::agent_runs::persist::original_task_for_test(stored),
        "second question"
    );
}

#[test]
fn status_line_is_chrome_and_repeats_collapse() {
    let cleaned = crate::agent_runs::completion::clean_extracted_result(
        "done\ndone\nComposer 2.5 · 7.2%",
        "",
    );
    assert_eq!(cleaned.as_deref(), Some("done"));
}

#[test]
fn add_a_follow_up_hint_is_chrome() {
    let cleaned =
        crate::agent_runs::completion::clean_extracted_result("PONG\n→ Add a follow-up", "");
    assert_eq!(cleaned.as_deref(), Some("PONG"));
}

#[test]
fn slow_start_gets_one_enter_retry_before_failing() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    let mut seen_retry = 0;
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..NOT_SUBMITTED_TICKS {
        last = observe_tui_turn(&mut watch, "idle", "Add follow-up >", false);
        if last == TuiTurnDecision::RetrySubmit {
            seen_retry += 1;
        }
    }
    assert_eq!(seen_retry, 1);
    assert_eq!(last, TuiTurnDecision::NotSubmitted);
}

#[test]
fn busy_pane_never_triggers_submit_retry() {
    let mut watch = TuiTurnWatch::from_baseline("Add follow-up >");
    observe_tui_turn(&mut watch, "running", "working", false);
    for _ in 0..NOT_SUBMITTED_TICKS {
        let decision = observe_tui_turn(&mut watch, "idle", "Add follow-up >", false);
        assert_ne!(decision, TuiTurnDecision::RetrySubmit);
        assert_ne!(decision, TuiTurnDecision::NotSubmitted);
    }
}

#[test]
fn cursor_startup_banner_and_cwd_line_are_not_result_text() {
    let screen = "Cursor Agent\nv2026.10.01-e373342\nTip: Use /plan to plan execution.\nPONG\n~\\Desktop\\pm-bench";
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result(screen, "").as_deref(),
        Some("PONG")
    );
    // A real reply that merely mentions a version or folder is kept.
    let reply = "Upgrade to v2 now, then open ~/notes in your editor.";
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result(reply, "").as_deref(),
        Some(reply)
    );
}

#[test]
fn cursor_tool_activity_lines_are_not_part_of_the_result() {
    let raw = "Read todo.py\n$ cd proj && python -m pytest 11s\n… 2 output lines hidden · ctrl+o to expand\nAll 12 tests pass.\nComposer 2.5 · 7.2%";
    let cleaned = crate::agent_runs::completion::clean_extracted_result(raw, "");
    assert_eq!(cleaned.as_deref(), Some("All 12 tests pass."));
    // Ordinary prose that starts with "Read" survives.
    let prose = crate::agent_runs::completion::clean_extracted_result("Read the docs first.", "");
    assert_eq!(prose.as_deref(), Some("Read the docs first."));
}

#[test]
fn untouched_cursor_pane_is_not_a_result() {
    assert!(crate::agent_runs::completion::clean_extracted_result(
        "→ Plan, search, build anything\nComposer 2.5",
        ""
    )
    .is_none());
    // Text above the last tool marker is tool output, not the reply.
    let cleaned = crate::agent_runs::completion::clean_extracted_result(
        "partial output\n$ python todo.py list 3s\nThere are 3 items.",
        "",
    );
    assert_eq!(cleaned.as_deref(), Some("There are 3 items."));
}

#[test]
fn cursor_shell_turn_capture_keeps_only_the_reply() {
    // Real capture: tool output is indented deeper than the `$` line; the reply is back at its column.
    let screen = "  Using your shell tool, run `python todo.py list` and report the output.\n\n  $ python todo.py list 14s\n    … 1 output lines hidden · ctrl+o to expand\n    2 [ ] recheck-item\n    3 [ ] should-not-exist\n\n  python todo.py list exited with code 0 and printed:\n\n  1 [ ] bench-item-B\n  2 [ ] recheck-item\n  3 [ ] should-not-exist\n\n  All three items are unchecked ([ ]).\n";
    let evidence = crate::agent_runs::completion::tui_turn_evidence(screen, "").expect("evidence");
    let cleaned = crate::agent_runs::completion::clean_extracted_result(
        &evidence,
        "Using your shell tool, run `python todo.py list` and report the output.",
    )
    .expect("result");
    assert!(!cleaned.contains("hidden"), "{cleaned}");
    assert!(
        cleaned.starts_with("python todo.py list exited with code 0"),
        "{cleaned}"
    );
    assert!(cleaned.contains("1 [ ] bench-item-B"));
    assert!(cleaned.ends_with("All three items are unchecked ([ ])."));
    // The stray tool lines appear once (in the reply), not twice.
    assert_eq!(cleaned.matches("3 [ ] should-not-exist").count(), 1);
}

#[test]
fn cursor_default_model_is_parsed_from_about_output() {
    let about = "About Cursor CLI\n\nCLI Version         2026.10.01-e373342\nDefault Model:      \u{1b}[1mComposer 2.5\u{1b}[0m\nSubscription Tier   Pro+\n";
    assert_eq!(
        crate::tui::cursor::parse_default_model(about).as_deref(),
        Some("Composer 2.5")
    );
    assert!(crate::tui::cursor::parse_default_model("Version: 1").is_none());
}

#[test]
fn cursor_model_is_parsed_from_real_about_columns() {
    let about = "About Cursor CLI\n\nCLI Version         2026.10.01-e373342\nLatest              2026.10.01-e373342 (up to date)\nModel               Composer 2.5\nSubscription Tier   Pro+\n";
    assert_eq!(
        crate::tui::cursor::parse_default_model(about).as_deref(),
        Some("Composer 2.5")
    );
}

#[test]
fn an_empty_json_reply_is_a_result_but_rules_and_box_art_are_not() {
    // Real case: the agent printed the JSON list `[]` and the result came back as None.
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result("The JSON printed was:\n[]", "")
            .as_deref(),
        Some("The JSON printed was:\n[]")
    );
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result("[]", "").as_deref(),
        Some("[]")
    );
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result("{}", "").as_deref(),
        Some("{}")
    );
    assert!(
        crate::agent_runs::completion::clean_extracted_result("────────\n▄▄▄▄▄▄\n   \n", "")
            .is_none()
    );
}

#[test]
fn a_reply_that_repeats_the_prompt_text_is_still_captured() {
    // Real case: "Reply with exactly <payload>" - the reply is identical to part of the prompt.
    let task = "Reply with exactly the following text and nothing else:\nfirst line of the reply\nsecond line ünïcode ✓";
    let evidence = format!("{task}\nfirst line of the reply\nsecond line ünïcode ✓");
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result(&evidence, task).as_deref(),
        Some("first line of the reply\nsecond line ünïcode ✓")
    );
    // Only the echo on screen: still not a result.
    assert!(crate::agent_runs::completion::clean_extracted_result(task, task).is_none());
    // Punctuation-only replies survive even though the prompt mentions them.
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result(
            "Reply with exactly [] and nothing else.\n[]",
            "Reply with exactly [] and nothing else."
        )
        .as_deref(),
        Some("[]")
    );
    assert_eq!(
        crate::agent_runs::completion::clean_extracted_result(
            "Reply with exactly {} and nothing else.\n{}",
            "Reply with exactly {} and nothing else."
        )
        .as_deref(),
        Some("{}")
    );
    // The previous turn's prompt shown again is never rescued as this turn's reply.
    let previous = "Please remember the codeword ORCHID-7 for later";
    let stale = format!("{previous}\n{previous}");
    assert!(crate::agent_runs::completion::clean_extracted_result_after(
        &stale,
        "What is 2+2?",
        previous
    )
    .is_none());
}

// ---- OpenCode server gone mid-run (live 2026-10-04: port 4100 died, cancel/close failed) ------
