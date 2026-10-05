//! Acceptance checks attached to runs and follow-ups.

use crate::operations::{self, OperationStatus, StateSource};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

fn run_with_checks(name: &str, checks: Vec<crate::agent_runs::checks::Check>) -> (String, String) {
    let project = std::env::temp_dir().join(format!("pm-checks-{name}-{}", Uuid::new_v4()));
    fs::create_dir_all(&project).unwrap();
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "worker", 0, "task");
    request.checks = checks;
    let (op, _) = operations::create_operation(request).unwrap();
    for status in [OperationStatus::Starting, OperationStatus::Running] {
        operations::mark_operation_state(
            &op.project_path,
            &op.operation_id,
            status,
            StateSource::Native,
            None,
            None,
            None,
        )
        .unwrap();
    }
    (op.project_path, op.operation_id)
}

#[test]
fn failing_check_turns_completion_into_acceptance_failed() {
    let (project, id) = run_with_checks(
        "fail",
        vec![crate::agent_runs::checks::Check::FileExists {
            path: "out.txt".into(),
        }],
    );
    let done = operations::mark_operation_completed(&project, &id, "all done", true).unwrap();
    assert_eq!(done.status, OperationStatus::Failed);
    assert!(!done.verified);
    assert_eq!(
        done.error.as_ref().map(|e| e.code.as_str()),
        Some("ACCEPTANCE_FAILED")
    );
    let view = crate::agent_runs::runtime::AgentRunView::from(done);
    assert_eq!(view.checks_configured, 1);
    assert_eq!(view.checks.len(), 1);
    assert!(!view.checks[0].passed);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn passing_check_completes_and_attaches_results() {
    let (project, id) = run_with_checks(
        "pass",
        vec![crate::agent_runs::checks::Check::FileExists {
            path: "out.txt".into(),
        }],
    );
    fs::write(std::path::Path::new(&project).join("out.txt"), "x").unwrap();
    let done = operations::mark_operation_completed(&project, &id, "all done", true).unwrap();
    assert_eq!(done.status, OperationStatus::Completed);
    assert!(done.verified);
    let view = crate::agent_runs::runtime::AgentRunView::from(done);
    assert_eq!(view.checks.len(), 1);
    assert!(view.checks[0].passed);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn run_without_checks_omits_checks_in_view_json() {
    let (project, id) = run_with_checks("none", vec![]);
    let done = operations::mark_operation_completed(&project, &id, "ok", true).unwrap();
    assert_eq!(done.status, OperationStatus::Completed);
    let value = serde_json::to_value(crate::agent_runs::runtime::AgentRunView::from(done)).unwrap();
    assert!(value.get("checks").is_none());
    let _ = fs::remove_dir_all(project);
}

#[test]
fn followup_keeps_checks_it_was_given_and_rejects_bad_ones() {
    let (project, id) = run_with_checks("adopt", vec![]);
    let done = operations::mark_operation_completed(&project, &id, "PONG", true).unwrap();
    let handle = done.agent_run_id.clone();
    let opts = |text: &str| crate::agent_runs::persist::FollowupOpts {
        checks: Some(vec![crate::agent_runs::checks::Check::ResultContains {
            text: text.into(),
        }]),
        locks: vec!["file:src/a.rs".into()],
        ..Default::default()
    };
    let (next, created) = crate::agent_runs::runtime::enqueue_followup(
        &project,
        &handle,
        &done,
        "again".into(),
        None,
        opts("ZZZ"),
    )
    .unwrap();
    assert!(created);
    assert_eq!(next.checks.len(), 1);
    assert_eq!(next.locks, vec!["file:src/a.rs".to_string()]);
    let bad = crate::agent_runs::persist::FollowupOpts {
        checks: Some(vec![crate::agent_runs::checks::Check::ResultContains {
            text: String::new(),
        }]),
        idempotency_key: Some("other".into()),
        ..Default::default()
    };
    let error = crate::agent_runs::runtime::enqueue_followup(
        &project,
        &handle,
        &next,
        "x".into(),
        None,
        bad,
    )
    .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    let _ = fs::remove_dir_all(project);
}
