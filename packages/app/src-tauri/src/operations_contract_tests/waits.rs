//! Operation contract tests: waits.

use super::*;

#[test]
fn wait_handles_already_observed_revision_and_timeout() {
    let project = Project::new();
    let queued = operations::create_operation(request(&project, "wait-race"))
        .unwrap()
        .0;
    let running =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| Ok(()))
            .unwrap();
    let updated = operations::mark_operation_state(
        &project.str(),
        &queued.operation_id,
        OperationStatus::WaitingInput,
        StateSource::Native,
        Some("approval".into()),
        None,
        None,
    )
    .unwrap();

    // The update happens before the wait begins; the revision cursor must still see it.
    let observed = operations::wait_for_operation(
        &project.str(),
        &queued.operation_id,
        running.revision,
        Some(vec![OperationStatus::WaitingInput]),
        50,
    )
    .unwrap();
    assert_eq!(observed.reason, "matched_state");
    assert_eq!(observed.snapshot.revision, updated.revision);

    let timed_out = operations::wait_for_operation(
        &project.str(),
        &queued.operation_id,
        updated.revision,
        Some(vec![OperationStatus::Completed]),
        10,
    )
    .unwrap();
    assert_eq!(timed_out.reason, "timeout");
    assert_eq!(timed_out.snapshot.revision, updated.revision);
}

#[test]
fn wait_any_wakes_when_queued_followup_is_cancelled_before_its_predecessor_finishes() {
    let project = Project::new();
    let predecessor =
        operations::delegate_work(request(&project, "active-predecessor"), |_| Ok(())).unwrap();
    let mut followup_request = request_with_worker_assigned_fields(&project, "queued-followup");
    followup_request.agent_run_id = Some(predecessor.agent_run_id.clone());
    followup_request.turn_index = predecessor.turn_index + 1;
    let followup = operations::create_operation(followup_request).unwrap().0;
    let cursors = vec![
        operations::OperationWaitCursor {
            project_path: project.str(),
            operation_id: predecessor.operation_id.clone(),
            after_revision: predecessor.revision,
        },
        operations::OperationWaitCursor {
            project_path: project.str(),
            operation_id: followup.operation_id.clone(),
            after_revision: followup.revision,
        },
    ];
    operations::cancel_operation(&project.str(), &followup.operation_id).unwrap();
    let result = operations::wait_for_operations(
        &cursors,
        Some(vec![
            OperationStatus::Completed,
            OperationStatus::Failed,
            OperationStatus::Cancelled,
        ]),
        1_000,
        false,
        false,
    )
    .unwrap();
    assert_eq!(result.reason, "matched_state");
    assert!(result.snapshots.iter().any(|snapshot| {
        snapshot.operation_id == followup.operation_id
            && snapshot.status == OperationStatus::Cancelled
    }));
}
