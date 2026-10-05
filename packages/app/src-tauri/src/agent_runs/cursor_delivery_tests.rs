//! Visible Cursor pane: the failing delivery/capture sequences reported from a benchmark, driven
//! through `observe_tui_turn` (screens are hand-built from the reported symptoms; no live visible
//! pane capture was available).

use super::completion::{
    tui_turn_evidence,
    clean_extracted_result, clean_extracted_result_after, completion_model, observe_tui_turn,
    TuiTurnDecision, TuiTurnWatch, NOT_SUBMITTED_TICKS, RETRY_SUBMIT_TICKS,
};

const FIRST: &str = "Reply with exactly the word ALPHA and nothing else at all";
const SECOND: &str = "Reply with exactly the word BRAVO and nothing else at all please";
const FOOTER: &str = "→ Add a follow-up\nComposer 2.5 · 4.1%";

fn run(watch: &mut TuiTurnWatch, status: &str, screen: &str, ticks: u32) -> Vec<TuiTurnDecision> {
    (0..ticks)
        .map(|_| observe_tui_turn(watch, status, screen, false))
        .collect()
}

fn none_complete(decisions: &[TuiTurnDecision]) -> bool {
    !decisions
        .iter()
        .any(|decision| matches!(decision, TuiTurnDecision::Complete { .. }))
}

/// Screen after turn 1 finished: its prompt echo, its reply, the footer.
fn after_first_turn() -> String {
    format!("> {FIRST}\n\nALPHA\n\n{FOOTER}")
}

#[test]
fn stale_copy_of_the_previous_prompt_is_not_a_completed_follow_up() {
    // The follow-up never reached the TUI; the pane now shows the previous prompt a second time
    // (stale input-box copy). That must not complete as the follow-up's result.
    let baseline = after_first_turn();
    let mut watch = TuiTurnWatch::from_baseline(baseline.clone())
        .with_task(SECOND)
        .with_previous_task(FIRST);
    let stale = format!("> {FIRST}\n\nALPHA\n\n> {FIRST}\n{FOOTER}");
    let decisions = run(&mut watch, "idle", &stale, NOT_SUBMITTED_TICKS as u32);
    assert!(none_complete(&decisions), "{decisions:?}");
    assert_eq!(decisions.last(), Some(&TuiTurnDecision::NotSubmitted));
}

#[test]
fn stale_prompt_echo_is_dropped_even_without_the_previous_task_known() {
    // Without previous_task the repeated prompt line is new evidence by occurrence count; with it
    // the line is recognised as an echo and nothing is left.
    let stale = format!("> {FIRST}\n\nALPHA\n\n> {FIRST}\n{FOOTER}");
    let evidence = super::completion::tui_turn_evidence(&stale, &after_first_turn()).unwrap();
    assert!(clean_extracted_result_after(&evidence, SECOND, FIRST).is_none());
}

#[test]
fn follow_up_reply_still_completes_when_the_previous_prompt_is_on_screen() {
    let mut watch = TuiTurnWatch::from_baseline(after_first_turn())
        .with_task(SECOND)
        .with_previous_task(FIRST);
    let screen = format!("> {FIRST}\n\nALPHA\n\n> {SECOND}\n\nBRAVO\n\n{FOOTER}");
    let decisions = run(&mut watch, "idle", &screen, 3);
    match decisions.iter().find(|d| matches!(d, TuiTurnDecision::Complete { .. })) {
        Some(TuiTurnDecision::Complete { result, .. }) => assert_eq!(result, "BRAVO"),
        other => panic!("expected BRAVO, got {other:?} in {decisions:?}"),
    }
}

#[test]
fn blank_pane_after_dispatch_is_not_submitted_never_a_result() {
    // Blank baseline and a pane that stays blank: RETRY Enter once, then NotSubmitted.
    let mut watch = TuiTurnWatch::from_baseline("").with_task(SECOND);
    let decisions = run(&mut watch, "idle", "", NOT_SUBMITTED_TICKS as u32);
    assert!(none_complete(&decisions));
    assert!(decisions.contains(&TuiTurnDecision::RetrySubmit));
    assert_eq!(decisions.last(), Some(&TuiTurnDecision::NotSubmitted));
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| **decision == TuiTurnDecision::RetrySubmit)
            .count(),
        1
    );
    assert!(RETRY_SUBMIT_TICKS < NOT_SUBMITTED_TICKS);
}

#[test]
fn prompt_typed_but_not_submitted_is_reported_not_completed() {
    // The prompt sits in the input box (never submitted); the pane stays idle.
    let mut watch = TuiTurnWatch::from_baseline(after_first_turn())
        .with_task(SECOND)
        .with_previous_task(FIRST);
    let typed = format!("> {FIRST}\n\nALPHA\n\n│ {SECOND} │\nComposer 2.5 · 4.1%");
    let decisions = run(&mut watch, "idle", &typed, NOT_SUBMITTED_TICKS as u32);
    assert!(none_complete(&decisions), "typed-but-unsent text is not a result: {decisions:?}");
    assert_eq!(decisions.last(), Some(&TuiTurnDecision::NotSubmitted));
}

#[test]
fn restart_after_redelivery_watches_from_the_new_baseline() {
    let mut watch = TuiTurnWatch::from_baseline(after_first_turn()).with_task(SECOND);
    let _ = run(&mut watch, "idle", &after_first_turn(), NOT_SUBMITTED_TICKS as u32);
    watch.restart(format!("> {FIRST}\nALPHA\n> {SECOND}\n{FOOTER}"));
    let _ = observe_tui_turn(&mut watch, "running", "working", false);
    let screen = format!("> {FIRST}\nALPHA\n> {SECOND}\n\nBRAVO\n{FOOTER}");
    let decisions = run(&mut watch, "idle", &screen, 3);
    assert!(
        decisions.iter().any(|d| matches!(d, TuiTurnDecision::Complete { .. })),
        "{decisions:?}"
    );
}

// ---- D3: debris in captured results -------------------------------------------------------

#[test]
fn diff_gutter_lines_and_truncation_notices_never_reach_the_result() {
    let evidence = "Edited todo.py +3 -1\n\u{258e}+ def add(item):\n\u{258e}+     items.append(item)\n\u{258e}- pass\n\u{2026} truncated (14 more lines)\nI added the add() helper to todo.py and kept the old API.";
    let cleaned = clean_extracted_result(evidence, "add an add helper").unwrap();
    assert_eq!(cleaned, "I added the add() helper to todo.py and kept the old API.");
    assert!(!cleaned.contains('\u{258e}'));
    assert!(!cleaned.contains("truncated"));
}

#[test]
fn diff_only_evidence_is_no_result() {
    let evidence = "\u{258e}+ x = 1\n\u{258e}+ y = 2\n\u{2026} truncated (9 more lines)";
    assert!(clean_extracted_result(evidence, "make a change").is_none());
}

#[test]
fn wrapped_prompt_fragments_are_not_a_result() {
    let task = "Read the failing test in tests/login.rs and propose a minimal fix, then explain the root cause";
    // The TUI wrapped the echoed prompt at the box width, leaving short tail fragments.
    let evidence = "> Read the failing test in tests/login.rs and propose a minimal fix,\nthen explain the root cause\nThe root cause is a missing await.";
    let cleaned = clean_extracted_result(evidence, task).unwrap();
    assert_eq!(cleaned, "The root cause is a missing await.");
}

#[test]
fn short_reply_next_to_an_echo_is_kept() {
    // A one-word answer that also appears inside the task must survive.
    let cleaned = clean_extracted_result("> Reply with the word PONG\nPONG", "Reply with the word PONG");
    assert_eq!(cleaned.as_deref(), Some("PONG"));
}

#[test]
fn previous_prompt_fragments_are_dropped_from_a_follow_up_result() {
    let evidence = "> Reply with exactly the word ALPHA and nothing else at all\nBRAVO";
    let cleaned = clean_extracted_result_after(evidence, SECOND, FIRST);
    assert_eq!(cleaned.as_deref(), Some("BRAVO"));
}

// ---- D10: resolved_model ---------------------------------------------------------------

#[test]
fn model_comes_from_the_footer_when_visible_and_is_confirmed() {
    let found = completion_model("cursor_agent", "BRAVO\nComposer 2.5 · 4.1%", None, || {
        panic!("default must not be probed when the footer is visible")
    })
    .unwrap();
    assert_eq!((found.model.as_str(), found.confirmed), ("Composer 2.5", true));
}

#[test]
fn model_falls_back_to_the_cli_default_unconfirmed() {
    let found = completion_model("cursor_agent", "BRAVO\n→ Add a follow-up", None, || {
        Some("Composer 2.5".to_string())
    })
    .unwrap();
    assert_eq!((found.model.as_str(), found.confirmed), ("Composer 2.5", false));
}

#[test]
fn an_already_resolved_model_is_not_replaced_by_the_default() {
    assert!(completion_model("cursor_agent", "no footer", Some("gpt-5"), || Some("x".into()))
        .is_none());
    assert!(completion_model("codex", "no footer", None, || Some("x".into())).is_none());
}

// ---- D13: headless trust ----------------------------------------------------------------

#[test]
fn closing_a_run_parked_on_workspace_trust_does_not_claim_an_interrupted_turn() {
    use crate::operations::{self, OperationStatus, StateSource};
    let project = std::env::temp_dir().join(format!("pm-trust-close-{}", uuid::Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let request = operations::DelegateWorkRequest {
        project_path: project_path.clone(),
        task: "inspect".into(),
        agent_type: "cursor_agent".into(),
        pane_id: None,
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: true,
        keep_pane: false,
        owner_session_id: Some("trust-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("trusty".into()),
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    let op = operations::run_operation_dispatch(&project_path, &queued.operation_id, |_| Ok(()))
        .unwrap();
    operations::mark_operation_waiting_input(
        &project_path,
        &op.operation_id,
        Some("waiting_input".into()),
        serde_json::json!({"kind": "workspace_trust", "choices": ["trust_workspace"]}),
        StateSource::Native,
    )
    .unwrap();
    super::runtime::interrupt_active_turns(
        &project_path,
        "trusty",
        Some("trust-session"),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    let closed = operations::get_operation(&project_path, &op.operation_id).unwrap();
    assert_eq!(closed.status, OperationStatus::Cancelled);
    let result = closed.result.unwrap_or_default();
    assert!(!result.contains("interrupted"), "{result}");
    assert!(result.contains("workspace trust"), "{result}");
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn unconfirmed_pane_model_warns_through_the_profile() {
    let facts = crate::tui::RunFacts {
        result: Some("ok"),
        has_pane: true,
        completed: true,
        result_inferred: false,
        model_requested: false,
        model_unconfirmed: true,
    };
    let warnings = crate::tui::profile_for("cursor_agent").run_warnings(&facts);
    assert!(warnings.iter().any(|warning| warning.code == "MODEL_UNCONFIRMED"));
    let confirmed = crate::tui::RunFacts {
        model_unconfirmed: false,
        ..facts
    };
    assert!(crate::tui::profile_for("cursor_agent")
        .run_warnings(&confirmed)
        .iter()
        .all(|warning| warning.code != "MODEL_UNCONFIRMED"));
}

#[test]
fn an_answered_trust_dialog_in_the_scrollback_is_not_a_result() {
    // Real capture (2026-10-04): the dialog box stays on screen after "Trusting workspace..." and
    // the banner is redrawn below it. It used to be returned as the run's result.
    let screen = include_str!("../tui/cursor/fixtures/workspace_trust_visible_resolved.txt");
    assert_eq!(crate::agent_runs::completion::tui_turn_evidence(screen, ""), None);
    let with_reply = format!("{screen}\nPONG\n");
    assert_eq!(
        crate::agent_runs::completion::tui_turn_evidence(&with_reply, "").as_deref(),
        Some("PONG")
    );
}

#[test]
fn a_turn_whose_delivery_was_verified_never_gets_a_blind_second_enter() {
    // Live (2026-10-04): three visible-Cursor turns showed "Composing 36-41 tokens" and then went
    // idle with no reply painted. The supervisor re-pressed Enter after 3s of idle ticks with no
    // busy tick; for a pane whose delivery protocol already verified submit that Enter lands on
    // an empty composer while the turn is starting, the only input Puppet Master could have sent
    // mid-turn. Redelivery (clear, retype, verify) stays as the recovery.
    let mut watch = TuiTurnWatch::from_baseline(after_first_turn())
        .with_task(SECOND)
        .with_previous_task(FIRST)
        .with_verified_delivery(true);
    let echoed = format!("> {FIRST}\n\nALPHA\n\n> {SECOND}\n\n{FOOTER}");
    let decisions = run(&mut watch, "idle", &echoed, NOT_SUBMITTED_TICKS as u32);
    assert!(!decisions.contains(&TuiTurnDecision::RetrySubmit), "{decisions:?}");
    assert_eq!(decisions.last(), Some(&TuiTurnDecision::NotSubmitted));
}

// ---- B4: RESULT_NOT_CAPTURED recovery --------------------------------------------------------
// Live (2026-10-04, pm-regression visible_close #2): the follow-up went idle with the reply not
// on the rows the supervisor read and failed as RESULT_NOT_CAPTURED. Before failing, the
// supervisor re-reads the turn from the scrollback; it never invents a reply.

use super::completion::{recapture_reply, scrollback_reply};

#[test]
fn a_reply_missing_from_the_visible_rows_is_recovered_from_the_scrollback() {
    // The visible rows hold only the footer; the turn is in the history above them.
    let visible = FOOTER.to_string();
    let full = format!("> {FIRST}\n\nALPHA\n\n> {SECOND}\n\nBRAVO\n\n{FOOTER}");
    let watch = TuiTurnWatch::from_baseline(after_first_turn())
        .with_task(SECOND)
        .with_previous_task(FIRST);
    assert_eq!(recapture_reply(&watch, &visible, &full).as_deref(), Some("BRAVO"));
    assert_eq!(scrollback_reply(&full, SECOND, FIRST).as_deref(), Some("BRAVO"));
}

#[test]
fn recapture_never_invents_a_reply() {
    let watch = TuiTurnWatch::from_baseline(after_first_turn())
        .with_task(SECOND)
        .with_previous_task(FIRST);
    // Echo and "Composing" only, no assistant text anywhere.
    let full = format!("> {FIRST}\n\nALPHA\n\n> {SECOND}\n\nComposing 12 tokens\n\n{FOOTER}");
    assert_eq!(recapture_reply(&watch, FOOTER, &full), None);
    // No echo of this turn's prompt at all: nothing after it can be attributed to the turn.
    let stale = format!("> {FIRST}\n\nALPHA\n\n{FOOTER}");
    assert_eq!(scrollback_reply(&stale, SECOND, FIRST), None);
    // The previous turn's reply is not this turn's.
    assert_eq!(recapture_reply(&watch, FOOTER, &stale), None);
}

#[test]
fn watch_diagnostics_say_what_the_supervisor_saw() {
    let mut watch = TuiTurnWatch::from_baseline(after_first_turn())
        .with_task(SECOND)
        .with_previous_task(FIRST);
    let busy = format!("> {SECOND}\n\nComposing 12 tokens");
    let _ = observe_tui_turn(&mut watch, "running", &busy, false);
    let diag = watch.diagnostics();
    assert_eq!(diag["saw_activity"], true);
    assert_eq!(diag["saw_reply"], false);
    assert_eq!(diag["delivery_verified"], false);
    assert!(diag["baseline_tail"].as_str().unwrap().contains("ALPHA"));
}

// ---- Cursor reply capture: verify5 captures (tester build b8693a6) --------------------------

const PYTEST_TASK: &str = "Write test_todo.py with pytest tests for every command supported by todo.py. Isolate storage in temporary directories. Run python -m pytest -q and report the exact pass/fail count. Do not modify todo.py.";
const PYTEST_FINAL: &str = "Added test_todo.py with pytest coverage for todo.py’s add and list commands (plus no-args behavior). Each test runs\nthe CLI in a fresh tmp_path so todos.json stays isolated.\npython -m pytest -q: 8 passed, 0 failed (0.93s)";

#[test]
fn a_reply_that_is_a_slice_of_a_one_line_prompt_is_still_the_reply() {
    // Live capture: the prompt "Reply with exactly this sentence, no tools: S" is echoed as ONE
    // row, and the reply S is the tail of that row. S was dropped as an echo (issue: the standalone
    // "Composer 2.5 ..." sentence vanished). The footer is not involved.
    let screen = include_str!("../tui/cursor/fixtures/reply_composer_sentence_screen.txt");
    let task = "Reply with exactly this sentence, no tools: Composer 2.5 is mentioned in this answer.";
    let previous = "Reply with only {}, no tools, no explanation.";
    let reply = scrollback_reply(screen, task, previous);
    assert_eq!(reply.as_deref(), Some("Composer 2.5 is mentioned in this answer."));
    // The same screen through the live watch path (baseline = screen before the turn).
    let baseline = screen
        .split("  Reply with exactly this sentence")
        .next()
        .unwrap()
        .to_string();
    let evidence = tui_turn_evidence(screen, &baseline).expect("new text");
    assert_eq!(
        clean_extracted_result_after(&evidence, task, previous).as_deref(),
        Some("Composer 2.5 is mentioned in this answer.")
    );
}

#[test]
fn a_prompt_echo_alone_is_never_a_reply_even_when_it_is_one_line() {
    let task = "Reply with exactly this sentence, no tools: Composer 2.5 is mentioned in this answer.";
    let screen = format!("  {task}\n\n -----\n  → Add a follow-up\n  Composer 2.5 · 7.3%");
    assert_eq!(scrollback_reply(&screen, task, ""), None);
    let evidence = tui_turn_evidence(&screen, "").unwrap_or_default();
    assert_eq!(clean_extracted_result_after(&evidence, task, ""), None);
}

#[test]
fn a_turn_where_cursor_printed_no_reply_stays_without_a_result() {
    // Live capture (verify5-1): follow-up "Reply with only []..." after PONG. The pane shows the
    // prompt echo and the idle input box and nothing else; Cursor's own chat store holds an
    // assistant message with only (redacted) reasoning and no text part. Nothing may be invented.
    let screen = include_str!("../tui/cursor/fixtures/reply_empty_data_screen.txt");
    let task = "Reply with only [], no tools, no explanation.";
    assert_eq!(scrollback_reply(screen, task, "Reply with only PONG, no tools."), None);
    let baseline = screen.split("  Reply with only [], no tools").next().unwrap().to_string();
    let evidence = tui_turn_evidence(screen, &baseline).unwrap_or_default();
    assert_eq!(
        clean_extracted_result_after(&evidence, task, "Reply with only PONG, no tools."),
        None
    );
}

#[test]
fn tool_blocks_and_commentary_are_not_part_of_the_final_message() {
    let full = include_str!("../tui/cursor/fixtures/reply_pytest_turn_full.txt");
    let reply = scrollback_reply(
        full,
        PYTEST_TASK,
        "Reply with exactly this sentence, no tools: Composer 2.5 is mentioned in this answer.",
    )
    .expect("final message");
    assert_eq!(reply, PYTEST_FINAL);
}

#[test]
fn the_testers_pytest_turn_yields_only_the_final_message() {
    let full = include_str!("../tui/cursor/fixtures/reply_pytest_tester_turn_full.txt");
    let reply = scrollback_reply(full, PYTEST_TASK, "").expect("final message");
    assert!(reply.starts_with("Added test_todo.py with seven tests"), "{reply}");
    assert!(reply.ends_with("7 passed, 0 failed."), "{reply}");
    for leaked in ["Globbed", "globbed", "Read todo.py", "Using subprocess", "Edited", "$ python", "1m 27s"] {
        assert!(!reply.contains(leaked), "leaked {leaked:?}: {reply}");
    }
    // The answer's own quoted output survives.
    assert!(reply.contains("Exact python -m pytest -q output:"), "{reply}");
    assert!(reply.contains("7 passed in 1.26s"), "{reply}");
}

#[test]
fn the_visible_rows_of_a_finished_tool_turn_give_the_final_message_too() {
    let screen = include_str!("../tui/cursor/fixtures/reply_pytest_final_screen.txt");
    let baseline = "  Cursor Agent\n  → Add a follow-up\n  Composer 2.5 · 7.3%";
    let evidence = tui_turn_evidence(screen, baseline).expect("new text");
    let reply = clean_extracted_result_after(&evidence, PYTEST_TASK, "").expect("reply");
    assert_eq!(reply, PYTEST_FINAL);
}

#[test]
fn a_final_message_that_quotes_commands_keeps_them() {
    let full = "  Show me how to list todos.\n\n    Read todo.py\n\n  $ python todo.py list 0ms\n    1 [ ] a\n\n  Run it like this:\n  $ python todo.py list\n  1 [ ] a\n  Then add one with python todo.py add b.\n\n  → Add a follow-up\n  Composer 2.5 · 4%";
    let reply = scrollback_reply(full, "Show me how to list todos.", "").expect("reply");
    assert_eq!(
        reply,
        "Run it like this:\n$ python todo.py list\n1 [ ] a\nThen add one with python todo.py add b."
    );
}

#[test]
fn a_shell_header_wrapped_by_the_terminal_is_not_part_of_the_final_message() {
    // Live capture (verify5-1, build b694b47): the `$ cd "<long path>" && python ... 5.7s` header
    // is hard-wrapped, so its first row has no duration and the second starts at column 0.
    let full = include_str!("../tui/cursor/fixtures/reply_pytest_wrapped_header.txt");
    let reply = scrollback_reply(full, PYTEST_TASK, "").expect("final message");
    assert!(!reply.contains("$ cd"), "{reply}");
    assert!(!reply.contains("5.7s"), "{reply}");
    assert!(!reply.contains("8 passed in"), "{reply}");
    assert!(reply.starts_with("todo.py only implements add and list"), "{reply}");
    assert!(reply.ends_with("Pytest: 8 passed, 0 failed (in 0.86s)"), "{reply}");
}

#[test]
fn a_search_result_row_under_a_grep_header_is_tool_output_not_the_answer() {
    // Live capture (verify5-1, build 5114177): `Grepped "def " in .` prints `Found 14 matches`
    // under it, at the header's own indent.
    let full = include_str!("../tui/cursor/fixtures/reply_grep_turn.txt");
    let task = "Use your grep search tool (not the shell) to find the text \"def \" in the files of this folder, then in one sentence tell me how many matches you found.";
    let reply = scrollback_reply(full, task, "").expect("final message");
    assert_eq!(reply, "I found 14 matches for def  across 5 files in this folder.");
    // The same words as the whole answer, with no search header above them, are kept.
    let bare = "  Say how many.\n\n  Found 14 matches\n\n  → Add a follow-up\n  Composer 2.5 · 7.5%";
    assert_eq!(scrollback_reply(bare, "Say how many.", "").as_deref(), Some("Found 14 matches"));
}

#[test]
fn a_wrapped_shell_header_is_joined_whether_the_continuation_is_indented_or_not() {
    let wrapped = |continuation: &str| {
        format!(
            "  Run the tests please now.\n\n  Running them.\n\n  $ cd \"C:/very/long/path/to/the/project/folder/that/wraps/at/the/pane/width/and/goes/on/and/on/for/a/bit/more/to/be/sure\"
{continuation}ea20af2 && python -m pytest -q 5.7s\n    8 passed in 0.86s\n\n  All 8 tests pass.\n\n  → Add a follow-up\n  Composer 2.5 · 7%"
        )
    };
    for continuation in ["", "  ", "    "] {
        let reply = scrollback_reply(&wrapped(continuation), "Run the tests please now.", "").expect("reply");
        assert_eq!(reply, "All 8 tests pass.", "continuation {continuation:?}");
    }
}
