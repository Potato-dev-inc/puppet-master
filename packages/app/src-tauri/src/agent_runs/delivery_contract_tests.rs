//! Regression tests for the run/steer/follow-up contracts callers depend on: what a steering
//! receipt claims, when it expires, what a failing check does to a run, pane disposal after a
//! terminal state, and conversation continuity across turns.

use super::messaging::{self, DeliveryMode, Disposition};
use crate::operations::{
    self, DelegateWorkRequest, OperationSnapshot, OperationStatus, ResultCapture, StateSource,
    WorkerPersist,
};
use std::fs;
use uuid::Uuid;

struct TempProject(String);

impl TempProject {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-delivery-{tag}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir.to_string_lossy().into_owned())
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(project: &str, handle: &str, turn: u32, agent: &str) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: project.to_string(),
        task: format!("turn {turn}"),
        agent_type: agent.into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("delivery-session".into()),
        parent_pane_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.into()),
        turn_index: turn,
        worker: WorkerPersist::default(),
        context_policy: None,
        checks: Vec::new(),
    }
}

/// A turn dispatched onto a pane (the way a TUI or native pane run looks).
fn running_pane_turn(
    project: &str,
    handle: &str,
    agent: &str,
    session: Option<&str>,
) -> OperationSnapshot {
    let mut req = request(project, handle, 0, agent);
    req.pane_id = Some("pane-1".into());
    req.worker.provider_session_id = session.map(str::to_string);
    let queued = operations::create_operation(req).unwrap().0;
    operations::run_operation_dispatch(project, &queued.operation_id, |snapshot| {
        snapshot.pane_id = Some("pane-1".into());
        snapshot.pane_created = true;
        Ok(())
    })
    .unwrap()
}

#[test]
fn opencode_live_steering_receipt_says_applied() {
    let project = TempProject::new("oc-live");
    let running = running_pane_turn(&project.0, "oc", "opencode_native", Some("ses-live"));
    let receipt = messaging::send_message(
        &project.0,
        "oc",
        "use the other file",
        &running,
        DeliveryMode::LiveOrQueue,
        Some("oc-1"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Accepted);
    let value = messaging::receipt_value(&receipt);
    assert_eq!(value["applied"], true);
    assert_eq!(value["disposition"], "accepted");
    // A strict live request is applied just the same.
    let strict = messaging::send_message(
        &project.0,
        "oc",
        "and rename it",
        &running,
        DeliveryMode::Live,
        Some("oc-2"),
    )
    .unwrap();
    assert_eq!(messaging::receipt_value(&strict)["applied"], true);
}

#[test]
fn cursor_pane_send_message_is_not_applied_and_cannot_change_the_running_turn() {
    let project = TempProject::new("cursor-pane");
    let running = running_pane_turn(&project.0, "cur", "cursor_agent", Some("chat-1"));

    let queued = messaging::send_message(
        &project.0,
        "cur",
        "also cover edge cases",
        &running,
        DeliveryMode::LiveOrQueue,
        Some("c-1"),
    )
    .unwrap();
    assert_eq!(queued.disposition, Disposition::Queued);
    let value = messaging::receipt_value(&queued);
    assert_eq!(value["applied"], false);
    assert!(value["next_action"].as_str().unwrap().contains("followup_task"));

    let strict = messaging::send_message(
        &project.0,
        "cur",
        "now",
        &running,
        DeliveryMode::Live,
        Some("c-2"),
    )
    .unwrap();
    assert_eq!(strict.disposition, Disposition::Unsupported);
    assert_eq!(messaging::receipt_value(&strict)["applied"], false);

    // Nothing was injected: no new operation, the running turn is untouched.
    assert_eq!(operations::list_operations(&project.0).unwrap().len(), 1);
    let after = operations::get_operation(&project.0, &running.operation_id).unwrap();
    assert_eq!(after.status, OperationStatus::Running);
    assert_eq!(after.turn_index, 0);
}

#[test]
fn queued_message_expires_when_its_turn_ends_and_never_reports_applied() {
    let project = TempProject::new("expire");
    let running = running_pane_turn(&project.0, "cur", "cursor_agent", None);
    let receipt = messaging::send_message(
        &project.0,
        "cur",
        "too late soon",
        &running,
        DeliveryMode::LiveOrQueue,
        Some("e-1"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Queued);

    let done = operations::mark_operation_finished(
        &project.0,
        &running.operation_id,
        Some("finished without the message".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    // The turn it was queued in finishing must not make the message look processed.
    messaging::mark_open_steers_processed_for_turn(&project.0, "cur", "finished", 0);
    assert_eq!(
        messaging::find_receipt_by_key(&project.0, "cur", "e-1").unwrap().disposition,
        Disposition::Queued
    );

    // Reading the terminal run settles the receipt as expired.
    let view = super::runtime::AgentRunView::from(done);
    let settled = messaging::find_receipt_by_key(&project.0, "cur", "e-1").unwrap();
    assert_eq!(settled.disposition, Disposition::Expired);
    let value = messaging::receipt_value(&settled);
    assert_eq!(value["applied"], false);
    assert_eq!(value["state"], "expired");
    assert!(value["next_action"].as_str().unwrap().contains("followup_task"));
    assert!(view.last_steer.as_ref().is_none_or(|steer| steer.state != "processed"));

    // And a replay of the same key returns the expired receipt, not a fresh delivery.
    let replay = messaging::send_message(
        &project.0,
        "cur",
        "too late soon",
        &operations::get_operation(&project.0, &running.operation_id).unwrap(),
        DeliveryMode::LiveOrQueue,
        Some("e-1"),
    )
    .unwrap();
    assert_eq!(replay.message_id, receipt.message_id);
    assert_eq!(replay.disposition, Disposition::Expired);
}

#[test]
fn sending_to_a_finished_turn_is_rejected_and_not_applied() {
    let project = TempProject::new("rejected");
    let running = running_pane_turn(&project.0, "cur", "cursor_agent", None);
    let done = operations::mark_operation_finished(
        &project.0,
        &running.operation_id,
        Some("done".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let receipt = messaging::send_message(
        &project.0,
        "cur",
        "late",
        &done,
        DeliveryMode::LiveOrQueue,
        Some("r-1"),
    )
    .unwrap();
    assert_eq!(receipt.disposition, Disposition::Rejected);
    assert_eq!(messaging::receipt_value(&receipt)["applied"], false);
}

#[test]
fn failing_checks_fail_the_run_with_acceptance_failed_and_a_failed_checks_outcome() {
    let project = TempProject::new("checks");
    let mut req = request(&project.0, "worker", 0, "codex");
    req.checks = vec![super::checks::Check::FileExists { path: "never-written.txt".into() }];
    let op = operations::create_operation(req).unwrap().0;
    for status in [OperationStatus::Starting, OperationStatus::Running] {
        operations::mark_operation_state(
            &project.0,
            &op.operation_id,
            status,
            StateSource::Native,
            None,
            None,
            None,
        )
        .unwrap();
    }
    let done =
        operations::mark_operation_completed(&project.0, &op.operation_id, "I did it", true).unwrap();
    assert_eq!(done.status, OperationStatus::Failed);
    assert_eq!(done.error.as_ref().unwrap().code, "ACCEPTANCE_FAILED");
    assert!(!done.verified);
    let view = super::runtime::AgentRunView::from(done);
    assert_eq!(view.receipt.expect("receipt").outcome, "failed_checks");
    assert!(!view.checks[0].passed);
}

#[test]
fn pane_disposal_after_a_terminal_state_follows_keep_pane_and_who_created_it() {
    let project = TempProject::new("dispose");
    let base = running_pane_turn(&project.0, "w", "codex", None);
    let terminals = [
        OperationStatus::Completed,
        OperationStatus::Failed,
        OperationStatus::Cancelled,
    ];
    for status in terminals {
        for (created, keep, expect) in [
            (true, false, true),
            (true, true, false),
            (false, false, false),
            (false, true, false),
        ] {
            let mut snapshot = base.clone();
            snapshot.status = status;
            snapshot.pane_created = created;
            snapshot.keep_pane = keep;
            assert_eq!(
                super::pane_close::should_dispose_after_terminal(&snapshot),
                expect,
                "{status:?} created={created} keep_pane={keep}"
            );
        }
    }
}

#[test]
fn follow_ups_keep_the_agent_run_id_and_increment_the_turn_index() {
    let project = TempProject::new("continuity");
    let first = running_pane_turn(&project.0, "convo", "codex", None);
    assert_eq!(first.turn_index, 0);
    let finished = operations::mark_operation_finished(
        &project.0,
        &first.operation_id,
        Some("first answer".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();

    let (second, created) = super::runtime::enqueue_followup(
        &project.0,
        "convo",
        &finished,
        "second question".into(),
        Some("delivery-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert!(created);
    assert_eq!(second.agent_run_id, "convo");
    assert_eq!(second.turn_index, 1);

    operations::run_operation_dispatch(&project.0, &second.operation_id, |_| Ok(())).unwrap();
    let finished_second = operations::mark_operation_finished(
        &project.0,
        &second.operation_id,
        Some("second answer".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let (third, _) = super::runtime::enqueue_followup(
        &project.0,
        "convo",
        &finished_second,
        "third question".into(),
        Some("delivery-session".into()),
        super::persist::FollowupOpts::default(),
    )
    .unwrap();
    assert_eq!(third.agent_run_id, "convo");
    assert_eq!(third.turn_index, 2);
    assert_ne!(third.operation_id, second.operation_id);
    let latest = operations::latest_agent_run(&project.0, "convo").unwrap();
    assert_eq!(latest.operation_id, third.operation_id);
}
