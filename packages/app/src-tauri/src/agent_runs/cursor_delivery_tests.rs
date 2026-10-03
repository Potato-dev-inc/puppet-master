//! Visible Cursor pane: the failing delivery/capture sequences reported from a benchmark, driven
//! through `observe_tui_turn` (screens are hand-built from the reported symptoms; no live visible
//! pane capture was available).

use super::completion::{
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
