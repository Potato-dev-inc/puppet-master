//! Operation contract tests: cancellation.

use super::*;

#[test]
fn cancelling_queued_operation_prevents_dispatch_and_late_overwrite() {
    let project = Project::new();
    let (queued, created) =
        operations::create_operation(request(&project, "cancel-before-start")).unwrap();
    assert!(created);
    let cancelled = operations::cancel_operation(&project.str(), &queued.operation_id).unwrap();
    assert_eq!(cancelled.status, OperationStatus::Cancelled);

    let dispatches = AtomicUsize::new(0);
    let after_cancel =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| {
            dispatches.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    assert_eq!(after_cancel.status, OperationStatus::Cancelled);
    assert_eq!(after_cancel.revision, cancelled.revision);
}

#[test]
fn worker_cancellation_during_startup_prevents_dispatch_completion_overwrite() {
    let project = Project::new();
    let (queued, _) = operations::create_operation(request(&project, "cancel-starting")).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let path = project.str();
    let operation_id = queued.operation_id.clone();
    let worker = thread::spawn(move || {
        operations::run_operation_dispatch(&path, &operation_id, |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            Ok(())
        })
        .unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();

    let approval = operations::mark_operation_observation(
        &project.str(),
        &queued.operation_id,
        Some("waiting_input".into()),
        Some(serde_json::json!({"type": "approval", "prompt": "Continue?"})),
        StateSource::Native,
    )
    .unwrap();
    assert_eq!(
        approval.required_action.as_ref().unwrap()["type"],
        "approval"
    );

    let cancellation =
        operations::cancel_operation_with(&project.str(), &queued.operation_id, |op| {
            assert_eq!(op.status, OperationStatus::Cancelling);
            Ok(())
        })
        .unwrap();
    assert_eq!(cancellation.status, OperationStatus::Cancelled);
    assert_eq!(cancellation.stage.as_deref(), Some("cancelled"));
    assert_eq!(cancellation.required_action, None);
    release_tx.send(()).unwrap();
    let after_dispatch_returns = worker.join().unwrap();
    assert_eq!(after_dispatch_returns.status, OperationStatus::Cancelled);
    assert_eq!(after_dispatch_returns.revision, cancellation.revision);
}

#[test]
fn failed_worker_cancellation_restores_the_prior_status() {
    let project = Project::new();
    let queued = operations::create_operation(request(&project, "cancel-fails"))
        .unwrap()
        .0;
    let running =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| Ok(()))
            .unwrap();

    let error = operations::cancel_operation_with(&project.str(), &running.operation_id, |_| {
        Err(operations::OperationError::new(
            "WORKER_UNAVAILABLE",
            "worker did not stop",
            true,
        ))
    })
    .unwrap_err();
    assert_eq!(error.code, "WORKER_UNAVAILABLE");

    let persisted = operations::get_operation(&project.str(), &running.operation_id).unwrap();
    assert_eq!(persisted.status, OperationStatus::Running);
    assert_eq!(persisted.stage.as_deref(), Some("cancellation_failed"));
    assert_eq!(persisted.error.as_ref().unwrap().code, "WORKER_UNAVAILABLE");
}
