//! Run view shapes: statuses, wait results, transcripts, failure text.

use crate::agent_runs::{headless, transcript};
use crate::operations::{
    self, DelegateWorkRequest, OperationError, OperationStatus, ResultCapture, StateSource,
};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

#[test]
fn agent_view_uses_compact_statuses_and_exposes_timeout() {
    let project = std::env::temp_dir().join(format!("pm-agent-view-{}", Uuid::new_v4()));
    let request = DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "inspect".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["report findings".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("stable-handle".into()),
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    let queued_view = crate::agent_runs::runtime::AgentRunView::from(queued.clone());
    assert_eq!(queued_view.status, "running");
    assert!(!queued_view.verified);

    let mut timed_out = queued;
    timed_out.status = OperationStatus::Failed;
    timed_out.error = Some(OperationError::new("TIMEOUT", "deadline exceeded", true));
    assert_eq!(
        crate::agent_runs::runtime::AgentRunView::from(timed_out).status,
        "timeout"
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_transcript_preserves_utf8_across_reader_chunks() {
    let project = std::env::temp_dir().join(format!("pm-agent-utf8-{}", Uuid::new_v4()));
    let project = project.to_string_lossy().into_owned();
    let operation = Uuid::new_v4().to_string();
    let bytes = "漢".as_bytes();
    let mut pending = Vec::new();
    pending.extend_from_slice(&bytes[..2]);
    headless::append_valid_utf8(&project, &operation, "stdout", &mut pending, false);
    pending.extend_from_slice(&bytes[2..]);
    headless::append_valid_utf8(&project, &operation, "stdout", &mut pending, false);
    headless::append_valid_utf8(&project, &operation, "stdout", &mut pending, true);
    let page = transcript::read(&project, &operation, 0).unwrap();
    let combined = page
        .chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect::<String>();
    assert_eq!(combined, "漢");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_failure_message_includes_last_stderr_line_and_trust_hint() {
    let message = headless::failure_message(1, "warming up\nWorkspace not trusted\n\n", None);
    assert!(message.contains("Workspace not trusted"));
    assert!(message.contains("trust the workspace folder"));
    let plain = headless::failure_message(1, "boom\n", None);
    assert!(plain.ends_with(": boom"));
    assert_eq!(
        headless::failure_message(1, "", None),
        "agent process exited with a failure status"
    );
    let buried = headless::failure_message(
        1,
        "error: unexpected argument '--ask-for-approval' found\n\nFor more information, try '--help'.\n",
        None,
    );
    assert!(buried.contains("unexpected argument '--ask-for-approval'"));
    assert!(!buried.contains("try '--help'"));
}

#[test]
fn wait_agents_view_separates_result_from_capture_and_acceptance() {
    let project = std::env::temp_dir().join(format!("pm-agent-wait-view-{}", Uuid::new_v4()));
    let request = DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "explain the failure".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["report findings".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("worker-123".into()),
        turn_index: 4,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(&queued.project_path, &queued.operation_id, |_| Ok(()))
        .unwrap();
    let finished = operations::mark_operation_finished(
        &queued.project_path,
        &queued.operation_id,
        Some("The connection fails because...".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let view = crate::agent_runs::runtime::AgentRunView::from(finished);
    assert_eq!(view.handle, "worker-123");
    assert_eq!(view.turn_id, "turn-4");
    assert_eq!(view.status, "completed");
    assert_eq!(
        view.result.as_deref(),
        Some("The connection fails because...")
    );
    assert_eq!(view.result_capture, "authoritative");
    assert_eq!(view.acceptance_status, "not_checked");
    assert_eq!(view.next_cursor, view.revision);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn transcript_truncation_does_not_drop_stored_result() {
    let project = std::env::temp_dir().join(format!("pm-agent-trunc-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let request = DelegateWorkRequest {
        project_path: project_path.clone(),
        task: "long log".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["keep the answer".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some("worker-trunc".into()),
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(&project_path, &queued.operation_id, |_| Ok(())).unwrap();
    for n in 0..250 {
        transcript::append(
            &project_path,
            &queued.operation_id,
            "stdout",
            &format!("{n}\n"),
        );
    }
    let finished = operations::mark_operation_finished(
        &project_path,
        &queued.operation_id,
        Some("kept final answer".into()),
        ResultCapture::Authoritative,
        false,
    )
    .unwrap();
    let page = transcript::read(&project_path, &queued.operation_id, 0).unwrap();
    assert!(page.truncated);
    assert_eq!(finished.result.as_deref(), Some("kept final answer"));
    let view = crate::agent_runs::runtime::AgentRunView::from(finished);
    assert_eq!(view.result.as_deref(), Some("kept final answer"));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn headless_mark_stop_failed_is_not_overwritten_by_late_exit_handler() {
    let project = std::env::temp_dir().join(format!("pm-stop-once-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let queued = operations::create_operation(persist_req(&project_path, "worker", 0, "task"))
        .unwrap()
        .0;
    let stop_error = OperationError::new("WORKER_STOP_FAILED", "taskkill failed", true);
    let failed = operations::mark_operation_state(
        &project_path,
        &queued.operation_id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("stop_failed".into()),
        None,
        Some(stop_error),
    )
    .unwrap();
    let again = operations::mark_operation_state(
        &project_path,
        &queued.operation_id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("failed".into()),
        None,
        Some(OperationError::new("WORKER_EXIT_FAILED", "late exit", true)),
    )
    .unwrap();
    assert_eq!(again.stage.as_deref(), Some("stop_failed"));
    assert_eq!(
        again.error.as_ref().map(|e| e.code.as_str()),
        Some("WORKER_STOP_FAILED")
    );
    assert_eq!(failed.operation_id, again.operation_id);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn wait_returns_the_result_unless_the_caller_already_saw_that_revision() {
    // Regression: a run finished before wait_agents started, no cursor was passed, and the result
    // came back null with result_unchanged=true.
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.result = Some("the answer".into());
    snapshot.revision = 7;
    let fresh = crate::agent_runs::runtime::agent_view_after_wait(snapshot.clone(), None, None);
    assert_eq!(fresh.result.as_deref(), Some("the answer"));
    assert_eq!(fresh.result_unchanged, None);
    let seen = crate::agent_runs::runtime::agent_view_after_wait(snapshot.clone(), Some(7), None);
    assert_eq!(seen.result, None);
    assert_eq!(seen.result_unchanged, Some(true));
    let behind = crate::agent_runs::runtime::agent_view_after_wait(snapshot, Some(3), None);
    assert_eq!(behind.result.as_deref(), Some("the answer"));
}
