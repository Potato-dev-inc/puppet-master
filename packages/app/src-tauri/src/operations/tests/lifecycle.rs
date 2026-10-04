//! Operations tests: lifecycle.

use super::*;

#[test]
fn idle_cannot_complete_and_completion_needs_evidence() {
    let dir = temp();
    let op = delegate_work(request(&dir, "complete"), |_| Ok(())).unwrap();
    assert!(mark_operation_state(
        &dir.to_string_lossy(),
        &op.operation_id,
        OperationStatus::Completed,
        StateSource::Inferred,
        None,
        None,
        None
    )
    .is_err());
    assert_eq!(
        get_operation(&dir.to_string_lossy(), &op.operation_id)
            .unwrap()
            .status,
        OperationStatus::Running
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dispatch_failure_is_durable_and_key_cannot_retry_it() {
    let dir = temp();
    let err = delegate_work(request(&dir, "failure"), |_| {
        Err(OperationError::new(
            "BRIDGE_DOWN",
            "bridge disconnected",
            true,
        ))
    })
    .unwrap_err();
    let id = err.context["operation_id"].as_str().unwrap();
    let saved = get_operation(&dir.to_string_lossy(), id).unwrap();
    assert_eq!(saved.status, OperationStatus::Failed);
    assert_eq!(saved.error.unwrap().code, "BRIDGE_DOWN");
    let retried = delegate_work(request(&dir, "failure"), |_| {
        panic!("must not dispatch twice")
    })
    .unwrap();
    assert_eq!(retried.operation_id, id);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dispatch_panic_is_durable_failure() {
    let dir = temp();
    let err = delegate_work(request(&dir, "panic"), |_| panic!("boom")).unwrap_err();
    assert_eq!(err.code, "DISPATCH_PANICKED");
    let saved = get_operation(
        &dir.to_string_lossy(),
        err.context["operation_id"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(saved.status, OperationStatus::Failed);
    assert_eq!(saved.error.unwrap().code, "DISPATCH_PANICKED");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cancel_persisted_queued_operation() {
    let dir = temp();
    let mut op = OperationSnapshot {
        operation_id: Uuid::new_v4().to_string(),
        runtime_id: current_runtime_id().to_string(),
        request_fingerprint: String::new(),
        project_path: dir.to_string_lossy().into_owned(),
        task: "queued".into(),
        agent_type: "codex".into(),
        pane_id: None,
        pane_created: false,
        idempotency_key: "queued-key".into(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        status: OperationStatus::Queued,
        revision: 1,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
        observed_at_ms: None,
        pane_state: None,
        required_action: None,
        source: StateSource::Native,
        stage: None,
        progress_pct: None,
        result: None,
        error: None,
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        keep_pane_requested: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: "test-run".into(),
        turn_index: 0,
        verified: false,
        started_at_ms: None,
        finished_at_ms: None,
        message_baseline_ids: vec![],
        output_baseline: None,
        result_capture: None,
        acceptance_status: crate::operations::AcceptanceStatus::NotChecked,
        worker: Default::default(),
        checks: Vec::new(),
        check_results: Vec::new(),
        warnings: Vec::new(),
    };
    persist(&op).unwrap();
    op = cancel_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);
    assert_eq!(op.revision, 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn observations_and_completion_cannot_steal_cancelling_state() {
    let dir = temp();
    let mut req = request(&dir, "cancel-observation");
    req.task_id = Some("task-cancel-race".into());
    let operation = delegate_work(req, |_| Ok(())).unwrap();
    let path = dir.to_string_lossy().into_owned();
    let id = operation.operation_id.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_path = path.clone();
    let worker_id = id.clone();
    let cancellation = std::thread::spawn(move || {
        cancel_operation_with(&worker_path, &worker_id, |snapshot| {
            assert_eq!(snapshot.status, OperationStatus::Cancelling);
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
        .unwrap()
    });
    entered_rx.recv().unwrap();

    let observed = mark_operation_observation(
        &path,
        &id,
        Some("waiting_input".into()),
        Some(serde_json::json!({"kind":"approval"})),
        StateSource::Native,
    )
    .unwrap();
    assert_eq!(observed.status, OperationStatus::Cancelling);
    assert_eq!(
        mark_task_completed(&path, "task-cancel-race", "completed concurrently")
            .unwrap_err()
            .code,
        "CANCELLATION_IN_PROGRESS"
    );

    release_tx.send(()).unwrap();
    assert_eq!(
        cancellation.join().unwrap().status,
        OperationStatus::Cancelled
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn startup_readiness_wait_uses_explicit_classifier_without_fabricating_idle() {
    let dir = temp();
    let (queued, _) = create_operation(request(&dir, "readiness-resume")).unwrap();
    let starting = mark_operation_state(
        &dir.to_string_lossy(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let reserved = reserve_pane(
        &dir.to_string_lossy(),
        &starting.operation_id,
        "pane-readiness",
        true,
    )
    .unwrap();
    let waiting = mark_operation_startup_wait(
        &dir.to_string_lossy(),
        &reserved.operation_id,
        Some("waiting_input".into()),
        Some(serde_json::json!({"kind":"manual_input_required"})),
        StateSource::Inferred,
    )
    .unwrap();
    assert_eq!(waiting.status, OperationStatus::WaitingInput);
    assert_eq!(waiting.stage.as_deref(), Some("waiting_for_pane_readiness"));
    assert!(resume_operation_dispatch(&dir.to_string_lossy(), &waiting.operation_id).is_err());
    let still_waiting = mark_operation_observation(
        &dir.to_string_lossy(),
        &waiting.operation_id,
        Some("waiting_input".into()),
        None,
        StateSource::Inferred,
    )
    .unwrap();
    assert_eq!(still_waiting.status, OperationStatus::WaitingInput);
    assert_eq!(
        still_waiting.stage.as_deref(),
        Some("waiting_for_pane_readiness")
    );
    let resumed = resume_operation_dispatch_when_ready(
        &dir.to_string_lossy(),
        &still_waiting.operation_id,
        "waiting_input",
        StateSource::Native,
    )
    .unwrap();
    assert_eq!(resumed.status, OperationStatus::Starting);
    assert_eq!(resumed.pane_state.as_deref(), Some("waiting_input"));
    assert_eq!(resumed.required_action, None);
    let dispatched =
        dispatch_input_if_active(&dir.to_string_lossy(), &resumed.operation_id, |_| Ok(()))
            .unwrap();
    assert_eq!(dispatched.status, OperationStatus::Starting);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dispatch_input_can_rebind_session_without_deadlocking() {
    let dir = temp();
    let (queued, _) = create_operation(request(&dir, "rebind-during-dispatch")).unwrap();
    let starting = mark_operation_state(
        &dir.to_string_lossy(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let project = dir.to_string_lossy().into_owned();
    let id = starting.operation_id.clone();
    dispatch_input_if_active(&project, &id, |_| {
        rebind_provider_session(
            &project,
            &id,
            "ses-new",
            Some("ses-old"),
            true,
            None,
        )
        .map(|_| ())
    })
    .unwrap();
    let rebound = get_operation(&project, &id).unwrap();
    assert_eq!(rebound.worker.provider_session_id.as_deref(), Some("ses-new"));
    assert!(rebound.worker.session_reset);
    assert_eq!(rebound.stage.as_deref(), Some("session_replaced"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn legacy_startup_resume_succeeds_for_observed_idle_pane() {
    let dir = temp();
    let (queued, _) = create_operation(request(&dir, "readiness-idle-compat")).unwrap();
    let starting = mark_operation_state(
        &dir.to_string_lossy(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    reserve_pane(
        &dir.to_string_lossy(),
        &starting.operation_id,
        "pane-idle-compat",
        true,
    )
    .unwrap();
    mark_operation_startup_wait(
        &dir.to_string_lossy(),
        &starting.operation_id,
        Some("idle".into()),
        Some(serde_json::json!({"kind":"readiness"})),
        StateSource::Native,
    )
    .unwrap();

    let resumed =
        resume_operation_dispatch(&dir.to_string_lossy(), &starting.operation_id).unwrap();
    assert_eq!(resumed.status, OperationStatus::Starting);
    assert_eq!(resumed.pane_state.as_deref(), Some("idle"));
    assert_eq!(resumed.required_action, None);
    let _ = fs::remove_dir_all(dir);
}
