//! Operation contract tests: recovery.

use super::*;

#[test]
fn interrupted_starting_operation_is_not_mistaken_for_task_completion() {
    let project = Project::new();
    let (queued, _) =
        operations::create_operation(request(&project, "interrupted-startup")).unwrap();
    // Simulate a process interruption after the durable startup claim but before dispatch returns.
    let starting = operations::mark_operation_state(
        &project.str(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let reloaded = operations::get_operation(&project.str(), &queued.operation_id).unwrap();
    assert_eq!(reloaded.status, OperationStatus::Starting);
    assert_eq!(reloaded.revision, starting.revision);

    // A different runtime id models a persisted record from the previous app process.
    let record_path = project
        .0
        .join(".puppet-master")
        .join("operations")
        .join(format!("{}.json", queued.operation_id));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["runtime_id"] = serde_json::Value::String("previous-runtime".into());
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();

    let recovered = operations::recover_interrupted_operations(&project.str()).unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].status, OperationStatus::Failed);
    assert_eq!(
        recovered[0].error.as_ref().unwrap().code,
        "OPERATION_INTERRUPTED"
    );
    assert!(recovered[0].error.as_ref().unwrap().recoverable);
    assert!(
        operations::mark_task_completed(&project.str(), "another-task", "done")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn interrupted_queued_operation_from_previous_runtime_is_failed_durably() {
    let project = Project::new();
    let queued = operations::create_operation(request(&project, "interrupted-queued"))
        .unwrap()
        .0;
    let record_path = project
        .0
        .join(".puppet-master")
        .join("operations")
        .join(format!("{}.json", queued.operation_id));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["runtime_id"] = serde_json::Value::String("previous-runtime".into());
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();

    let recovered = operations::recover_interrupted_operations(&project.str()).unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].operation_id, queued.operation_id);
    assert_eq!(recovered[0].status, OperationStatus::Failed);
    assert_eq!(
        recovered[0].error.as_ref().unwrap().code,
        "OPERATION_INTERRUPTED"
    );
    assert_eq!(
        operations::get_operation(&project.str(), &queued.operation_id)
            .unwrap()
            .status,
        OperationStatus::Failed
    );
}

/// Locks live in the durable event log, so an operation the restart marked interrupted must not
/// keep its resources locked forever.
#[test]
fn startup_recovery_releases_locks_held_by_interrupted_operations() {
    let project = Project::new();
    let held = locked_operation(&project, "lock-interrupted", "file:src/restart.rs");
    crate::bridge::acquire_operation_locks(&held).unwrap();
    operations::mark_operation_state(
        &project.str(),
        &held.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let record_path = project
        .0
        .join(".puppet-master")
        .join("operations")
        .join(format!("{}.json", held.operation_id));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["runtime_id"] = serde_json::Value::String("previous-runtime".into());
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();

    let blocked = locked_operation(&project, "lock-after-restart", "file:src/restart.rs");
    assert_eq!(
        crate::bridge::acquire_operation_locks(&blocked).unwrap_err().code,
        "RESOURCE_LOCKED"
    );

    let recovered = crate::bridge::recover_interrupted_operations_on_startup().unwrap();
    assert!(recovered
        .iter()
        .any(|op| op.operation_id == held.operation_id && op.status == OperationStatus::Failed));
    crate::bridge::acquire_operation_locks(&blocked).unwrap();
}
