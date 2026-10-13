//! Run receipts, outcomes, and timing fields of the run view.

use crate::operations::{self, OperationError, OperationStatus};

use super::fixtures::*;

#[test]
fn duration_of_an_active_run_tracks_elapsed_time_not_the_last_update() {
    let mut snapshot = parked_snapshot(OperationStatus::Running, "none");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    snapshot.started_at_ms = Some(now - 45_000);
    snapshot.updated_at_ms = now - 44_971; // last event 29 ms after start
    snapshot.finished_at_ms = None;
    let view = crate::agent_runs::runtime::AgentRunView::from(snapshot.clone());
    let duration = view.duration_ms.expect("duration");
    assert!(duration >= 45_000, "active run showed {duration} ms");
    // A finished run is measured to its finish time, not to now.
    snapshot.status = OperationStatus::Completed;
    snapshot.finished_at_ms = Some(now - 40_000);
    let done = crate::agent_runs::runtime::AgentRunView::from(snapshot);
    assert_eq!(done.duration_ms, Some(5_000));
}

fn receipt_of(snapshot: operations::OperationSnapshot) -> crate::agent_runs::runtime::RunReceipt {
    crate::agent_runs::runtime::AgentRunView::from(snapshot)
        .receipt
        .expect("every view carries a receipt")
}

#[test]
fn receipt_for_unchecked_inferred_run_lists_what_was_not_verified() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.result_capture = Some(operations::ResultCapture::Inferred);
    snapshot.worker.requested_model = Some("gpt-x".into());
    let receipt = receipt_of(snapshot);
    assert_eq!(receipt.backend, "cursor_agent");
    assert_eq!(receipt.read_only, "enforced");
    assert_eq!(receipt.checks, "none configured");
    assert_eq!(receipt.result_capture, "inferred");
    assert_eq!(receipt.model.as_deref(), Some("gpt-x"));
    assert_eq!(
        receipt.not_verified,
        vec![
            "no checks configured",
            "result read from terminal screen",
            "model not confirmed"
        ]
    );
}

#[test]
fn receipt_counts_passed_checks_and_confirmed_model_is_not_flagged() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.result_capture = Some(operations::ResultCapture::Authoritative);
    snapshot.read_only = false;
    snapshot.worker.requested_model = Some("m".into());
    snapshot.worker.resolved_model = Some("m".into());
    let check = crate::agent_runs::checks::Check::FileExists { path: "a".into() };
    snapshot.checks = vec![check.clone(), check.clone()];
    snapshot.check_results = vec![
        crate::agent_runs::checks::CheckResult {
            check: check.clone(),
            passed: true,
            detail: String::new(),
        },
        crate::agent_runs::checks::CheckResult {
            check,
            passed: false,
            detail: String::new(),
        },
    ];
    let receipt = receipt_of(snapshot);
    assert_eq!(receipt.checks, "1/2 passed");
    assert_eq!(receipt.read_only, "off");
    assert!(
        receipt.not_verified.is_empty(),
        "{:?}",
        receipt.not_verified
    );
}

#[test]
fn receipt_flags_read_only_the_backend_cannot_enforce() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.agent_type = "opencode".into();
    let receipt = receipt_of(snapshot);
    assert_eq!(receipt.read_only, "unenforced");
    assert!(receipt
        .not_verified
        .contains(&"read_only not enforced".to_string()));
}

#[test]
fn receipt_serializes_additively_and_running_runs_do_not_claim_unverified_model() {
    let mut snapshot = parked_snapshot(OperationStatus::Running, "none");
    snapshot.worker.requested_model = Some("m".into());
    let view = crate::agent_runs::runtime::AgentRunView::from(snapshot);
    let value = serde_json::to_value(&view).unwrap();
    assert_eq!(value["receipt"]["result_capture"], "pending");
    let not_verified = value["receipt"]["not_verified"].as_array().unwrap();
    assert!(!not_verified
        .iter()
        .any(|entry| entry == "model not confirmed"));
}

#[test]
fn duration_measures_prompt_to_finish_and_total_includes_setup() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.created_at_ms = 1_000;
    snapshot.started_at_ms = Some(51_000);
    snapshot.finished_at_ms = Some(62_000);
    let value =
        serde_json::to_value(crate::agent_runs::runtime::AgentRunView::from(snapshot)).unwrap();
    assert_eq!(value["duration_ms"], 11_000);
    assert_eq!(value["setup_ms"], 50_000);
    assert_eq!(value["total_ms"], 61_000);
    assert_eq!(value["receipt"]["build_id"], env!("PM_BUILD_ID"));
}

#[test]
fn pane_open_is_false_while_an_auto_closing_pane_is_still_in_the_registry() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.pane_created = true;
    snapshot.keep_pane = false;
    let (open, closing, note) =
        crate::agent_runs::runtime::pane_presentation(&snapshot, true, false);
    assert!(!open && closing);
    assert!(note.unwrap().contains("being closed"));
    // The dispose is skipped when another operation holds the pane: it really stays open.
    assert_eq!(
        crate::agent_runs::runtime::pane_presentation(&snapshot, true, true),
        (true, false, None)
    );
    // keep_pane runs and adopted panes are never auto-closed.
    snapshot.keep_pane = true;
    let (open, closing, note) =
        crate::agent_runs::runtime::pane_presentation(&snapshot, true, false);
    assert!(open && !closing && note.unwrap().contains("keep_pane=true"));
    snapshot.keep_pane = false;
    snapshot.pane_created = false;
    assert_eq!(
        crate::agent_runs::runtime::pane_presentation(&snapshot, true, false),
        (true, false, None)
    );
}

#[test]
fn receipt_outcome_is_the_single_answer_to_did_it_work() {
    let check = crate::agent_runs::checks::Check::FileExists { path: "a".into() };
    let passed = |ok: bool| crate::agent_runs::checks::CheckResult {
        check: check.clone(),
        passed: ok,
        detail: String::new(),
    };

    // Completed with every check passed: the requested behaviour was proven.
    let mut proven = parked_snapshot(OperationStatus::Completed, "none");
    proven.checks = vec![check.clone()];
    proven.check_results = vec![passed(true)];
    assert_eq!(receipt_of(proven).outcome, "passed_checks");

    // Completed with nothing checking it: finished is not the same as proven.
    let unchecked = parked_snapshot(OperationStatus::Completed, "none");
    assert_eq!(receipt_of(unchecked).outcome, "completed_unchecked");

    // A failing check fails the run.
    let mut failed_checks = parked_snapshot(OperationStatus::Failed, "none");
    failed_checks.checks = vec![check.clone()];
    failed_checks.check_results = vec![passed(false)];
    failed_checks.error = Some(OperationError::new(
        "ACCEPTANCE_FAILED",
        "check failed",
        false,
    ));
    assert_eq!(receipt_of(failed_checks).outcome, "failed_checks");

    let mut failed = parked_snapshot(OperationStatus::Failed, "none");
    failed.error = Some(OperationError::new("RESULT_NOT_CAPTURED", "x", true));
    assert_eq!(receipt_of(failed).outcome, "failed");

    assert_eq!(
        receipt_of(parked_snapshot(OperationStatus::WaitingInput, "none")).outcome,
        "needs_input"
    );
    assert_eq!(
        receipt_of(parked_snapshot(OperationStatus::Running, "none")).outcome,
        "in_progress"
    );
    assert_eq!(
        receipt_of(parked_snapshot(OperationStatus::Cancelled, "none")).outcome,
        "cancelled"
    );
}

#[test]
fn receipt_and_view_carry_the_visible_to_headless_fallback() {
    let plain = receipt_of(parked_snapshot(OperationStatus::Completed, "none"));
    assert!(plain.fallback.is_none());
    assert!(!plain.not_verified.iter().any(|item| item.contains("headless")));
    assert_eq!(serde_json::to_value(&plain).unwrap()["fallback"], serde_json::Value::Null);

    for reason in ["no_visible_mode", "visible_failed"] {
        let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
        snapshot.worker.fallback = Some(operations::RunFallback::to_headless(reason));
        let view = crate::agent_runs::runtime::AgentRunView::from(snapshot);
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["fallback"], serde_json::json!({"from":"visible","to":"headless","reason":reason}));
        assert_eq!(json["receipt"]["fallback"], json["fallback"]);
        let receipt = view.receipt.expect("receipt");
        assert!(
            receipt.not_verified.iter().any(|item| item.contains("ran headless") && item.contains(reason)),
            "{:?}",
            receipt.not_verified
        );
    }
}

fn sample_worktree(project: &str) -> crate::worktrees::WorktreeRef {
    crate::worktrees::WorktreeRef {
        path: format!("{project}/.puppet-master/worktrees/h-1a2b"),
        cwd: format!("{project}/.puppet-master/worktrees/h-1a2b"),
        branch: "pm/h".into(),
        base: "main".into(),
        base_commit: "abc".into(),
        repo: project.into(),
        handle: "h".into(),
        created_at_ms: 1,
        closed: None,
        changes: Some(crate::worktrees::ChangeSummary { files_changed: 3, additions: 10, deletions: 2 }),
    }
}

#[test]
fn an_isolated_run_receipt_names_its_worktree_and_stats_and_the_run_keeps_the_origin_project() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    assert!(receipt_of(snapshot.clone()).worktree.is_none(), "plain runs carry no worktree");
    assert_eq!(snapshot.work_dir(), snapshot.project_path);

    let project = snapshot.project_path.clone();
    snapshot.worker.worktree = Some(sample_worktree(&project));
    // the worker runs in the worktree, but the run is still keyed on the origin project
    assert!(snapshot.work_dir().ends_with("h-1a2b"));
    assert_eq!(snapshot.project_path, project);
    let worktree = receipt_of(snapshot.clone()).worktree.expect("worktree on the receipt");
    assert_eq!(worktree.branch, "pm/h");
    assert_eq!(worktree.base, "main");
    assert_eq!(worktree.state, "unreviewed");
    assert_eq!((worktree.files_changed, worktree.additions, worktree.deletions), (Some(3), Some(10), Some(2)));
    let json = serde_json::to_value(receipt_of(snapshot.clone())).unwrap();
    assert_eq!(json["worktree"]["branch"], "pm/h");

    // merged: the receipt says so and later turns fall back to the project folder
    snapshot.worker.worktree.as_mut().unwrap().closed = Some("merged".into());
    assert_eq!(receipt_of(snapshot.clone()).worktree.unwrap().state, "merged");
    assert_eq!(snapshot.work_dir(), snapshot.project_path);
}

#[test]
fn the_worktree_survives_a_snapshot_round_trip_so_a_restart_does_not_lose_it() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    let project = snapshot.project_path.clone();
    snapshot.worker.worktree = Some(sample_worktree(&project));
    let text = serde_json::to_string(&snapshot).unwrap();
    let back: operations::OperationSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back.worker.worktree, snapshot.worker.worktree);
    // a snapshot written before this feature has no worktree
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    old["worker"].as_object_mut().unwrap().remove("worktree");
    let old: operations::OperationSnapshot = serde_json::from_value(old).unwrap();
    assert!(old.worker.worktree.is_none());
}
