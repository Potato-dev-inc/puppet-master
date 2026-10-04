//! Operation contract tests: idempotency.

use super::*;

#[test]
fn simultaneous_same_key_dispatches_once_and_returns_same_operation() {
    let project = Arc::new(Project::new());
    let start = Arc::new(Barrier::new(8));
    let dispatches = Arc::new(AtomicUsize::new(0));
    let threads = (0..8)
        .map(|_| {
            let project = Arc::clone(&project);
            let start = Arc::clone(&start);
            let dispatches = Arc::clone(&dispatches);
            thread::spawn(move || {
                start.wait();
                operations::delegate_work(request(&project, "same-key"), |_| {
                    dispatches.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(30));
                    Ok(())
                })
                .unwrap()
            })
        })
        .collect::<Vec<_>>();

    let snapshots = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    assert!(snapshots
        .iter()
        .all(|op| op.operation_id == snapshots[0].operation_id));
}

#[test]
fn replay_matches_original_request_after_dispatch_assigns_pane_task_and_locks() {
    let project = Project::new();
    let original = request_with_worker_assigned_fields(&project, "worker-assigned-fields");
    let first = operations::delegate_work(original.clone(), |op| {
        op.pane_id = Some("pane-created-by-worker".into());
        op.pane_created = true;
        op.task_id = Some("task-created-by-worker".into());
        op.exclusive = true;
        op.locks = vec!["file:src/generated.rs".into()];
        Ok(())
    })
    .unwrap();
    let dispatches = AtomicUsize::new(0);
    let replay = operations::delegate_work(original, |_| {
        dispatches.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();

    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    assert_eq!(first.operation_id, replay.operation_id);
    assert_eq!(replay.pane_id.as_deref(), Some("pane-created-by-worker"));
    assert!(replay.pane_created);
}

#[test]
fn replay_while_running_returns_the_same_run_and_never_repeats_side_effects() {
    let project = Project::new();
    let mut original = request_with_worker_assigned_fields(&project, "running-replay");
    original.agent_run_id = Some("stable-handle".into());
    let spawns = AtomicUsize::new(0);
    let first = operations::delegate_work(original.clone(), |op| {
        spawns.fetch_add(1, Ordering::SeqCst); // pane spawn + prompt delivery stand-in
        op.pane_id = Some("spawned-pane".into());
        op.pane_created = true;
        Ok(())
    })
    .unwrap();
    assert_eq!(first.status, OperationStatus::Running, "the worker is mid-turn");

    for _ in 0..3 {
        let replay = operations::delegate_work(original.clone(), |_| {
            spawns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(replay.operation_id, first.operation_id);
        assert_eq!(replay.agent_run_id, "stable-handle");
        assert_eq!(replay.status, OperationStatus::Running);
        assert_eq!(replay.pane_id.as_deref(), Some("spawned-pane"));
    }
    assert_eq!(spawns.load(Ordering::SeqCst), 1);

    // A changed request under the same key is a conflict, also while the run is live, and it
    // neither dispatches nor disturbs the running operation.
    let mut changed = original;
    changed.task = "something else".into();
    let dispatched = AtomicUsize::new(0);
    let error = operations::delegate_work(changed, |_| {
        dispatched.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "IDEMPOTENCY_KEY_CONFLICT");
    assert_eq!(dispatched.load(Ordering::SeqCst), 0);
    let still = operations::get_operation(&project.str(), &first.operation_id).unwrap();
    assert_eq!(still.status, OperationStatus::Running);
    assert_eq!(operations::list_operations(&project.str()).unwrap().len(), 1);
}

#[test]
fn reusing_key_with_changed_request_fields_conflicts() {
    let project = Project::new();
    let original = request(&project, "conflict-key");
    operations::create_operation(original.clone()).unwrap();

    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.task = "different task".into();
    changes.push(changed);
    let mut changed = original.clone();
    changed.pane_id = Some("pane-b".into());
    changes.push(changed);
    let mut changed = original.clone();
    changed.locks = vec!["file:src/other.rs".into()];
    changes.push(changed);
    let mut changed = original.clone();
    changed.exclusive = true;
    changes.push(changed);
    let mut changed = original.clone();
    changed.acceptance_criteria = Some(vec!["different evidence".into()]);
    changes.push(changed);
    let mut changed = original;
    changed.task_id = Some("task-b".into());
    changes.push(changed);

    for changed in changes {
        assert_eq!(
            operations::create_operation(changed).unwrap_err().code,
            "IDEMPOTENCY_KEY_CONFLICT"
        );
    }
}

#[test]
fn read_only_on_unsupported_backend_fails_before_any_operation_exists() {
    let project = Project::new();
    let mut native = request(&project, "ro-native");
    native.agent_type = "opencode_native".into();
    // An adopted (shared) pane cannot be made read-only.
    native.pane_id = Some("adopted-native-pane".into());
    native.read_only = true;
    let err = operations::create_operation(native).unwrap_err();
    assert_eq!(err.code, "READ_ONLY_UNSUPPORTED");
    assert!(err.message.contains("opencode_native"));
    assert!(err.message.contains("claude, codex, cursor_agent"));
    assert!(operations::list_operations(&project.str()).unwrap().is_empty());

    // A native run that spawns its own pane is started with enforced read-only config.
    let mut spawned = request(&project, "ro-native-spawned");
    spawned.agent_type = "opencode_native".into();
    spawned.pane_id = None;
    spawned.read_only = true;
    assert!(operations::create_operation(spawned).is_ok());

    let mut supported = request(&project, "ro-codex");
    supported.pane_id = None;
    supported.read_only = true;
    assert!(operations::create_operation(supported).is_ok());
}

#[test]
fn invalid_task_criteria_and_project_values_are_rejected() {
    let project = Project::new();
    let mut empty_task = request(&project, "empty-task");
    empty_task.task = "  ".into();
    assert_eq!(
        operations::create_operation(empty_task).unwrap_err().code,
        "INVALID_TASK"
    );

    for criteria in [Some(vec!["  ".into()])] {
        let mut invalid = request(&project, "invalid-criteria");
        invalid.acceptance_criteria = criteria;
        assert_eq!(
            operations::create_operation(invalid).unwrap_err().code,
            "INVALID_ACCEPTANCE_CRITERIA"
        );
    }

    let mut optional = request(&project, "optional-criteria");
    optional.acceptance_criteria = None;
    assert!(operations::create_operation(optional).is_ok());
    let mut empty = request(&project, "empty-criteria");
    empty.acceptance_criteria = Some(vec![]);
    assert!(operations::create_operation(empty).is_ok());

    let mut relative_project = request(&project, "relative-project");
    relative_project.project_path = "relative/project".into();
    let (normalized, _) = operations::create_operation(relative_project).unwrap();
    assert!(std::path::Path::new(&normalized.project_path).is_absolute());

    let mut missing_task_id = request(&project, "no-task-id");
    missing_task_id.task_id = None;
    let (op, _) = operations::create_operation(missing_task_id).unwrap();
    assert_eq!(op.task_id, None);
}

#[test]
fn operation_survives_reload_and_lookup_reports_missing_ids() {
    let project = Project::new();
    let queued = operations::create_operation(request(&project, "reload"))
        .unwrap()
        .0;
    let running =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| Ok(()))
            .unwrap();

    let loaded = operations::get_operation(&project.str(), &running.operation_id).unwrap();
    assert_eq!(loaded.revision, running.revision);
    assert_eq!(loaded.status, OperationStatus::Running);
    assert_eq!(loaded.task, "implement feature");
    assert_eq!(
        operations::operation_for_task(&project.str(), "task-a")
            .unwrap()
            .operation_id,
        loaded.operation_id
    );
    assert_eq!(
        operations::operation_for_task(&project.str(), "absent")
            .unwrap_err()
            .code,
        "OPERATION_NOT_FOUND"
    );
    assert_eq!(
        operations::get_operation(&project.str(), "bad/id")
            .unwrap_err()
            .code,
        "INVALID_OPERATION_ID"
    );
}
