//! Operation contract tests: completion.

use super::*;

#[test]
fn checks_drive_acceptance_status_and_free_text_does_not() {
    let project = Project::new();
    let mut req = request(&project, "checks-pass");
    req.pane_id = None;
    req.checks = vec![crate::agent_runs::checks::Check::FileAbsent {
        path: "nope.txt".into(),
    }];
    let (op, _) = operations::create_operation(req).unwrap();
    operations::run_operation_dispatch(&op.project_path, &op.operation_id, |_| Ok(())).unwrap();
    let done = operations::mark_operation_finished(
        &op.project_path,
        &op.operation_id,
        Some("unrelated text".into()),
        operations::ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    assert_eq!(done.acceptance_status, operations::AcceptanceStatus::Passed);
}

#[test]
fn only_explicit_task_completion_evidence_completes_an_operation() {
    let project = Project::new();
    let queued = operations::create_operation(request(&project, "explicit-completion"))
        .unwrap()
        .0;
    let running =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| Ok(()))
            .unwrap();

    let idle_observation = operations::mark_operation_observation(
        &project.str(),
        &running.operation_id,
        Some("idle".into()),
        None,
        StateSource::Inferred,
    )
    .unwrap();
    assert_eq!(idle_observation.status, OperationStatus::Running);

    let missing_evidence = operations::mark_operation_state(
        &project.str(),
        &running.operation_id,
        OperationStatus::Completed,
        StateSource::Native,
        None,
        Some(" ".into()),
        None,
    )
    .unwrap_err();
    assert_eq!(missing_evidence.code, "COMPLETION_EVIDENCE_REQUIRED");

    let completed =
        operations::mark_task_completed(&project.str(), "task-a", "tests passed").unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].status, OperationStatus::Completed);
    assert_eq!(completed[0].result.as_deref(), Some("tests passed"));
}
