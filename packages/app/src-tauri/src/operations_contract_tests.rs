//! Contract tests for the public durable operation API.
//!
//! These tests deliberately use the public entry points and fake dispatch callbacks so
//! they exercise persistence, idempotency, waiting, and lifecycle behavior without a PTY.

use crate::operations::{self, DelegateWorkRequest, OperationStatus, StateSource};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("pm-operation-contract-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn str(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request(project: &Project, key: &str) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: project.str(),
        task: "implement feature".into(),
        agent_type: "codex".into(),
        pane_id: Some("pane-a".into()),
        idempotency_key: key.into(),
        acceptance_criteria: Some(vec!["behavior works".into()]),
        task_id: Some("task-a".into()),
        exclusive: false,
        locks: vec!["file:src/main.rs".into()],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    }
}

fn request_with_worker_assigned_fields(project: &Project, key: &str) -> DelegateWorkRequest {
    let mut req = request(project, key);
    req.pane_id = None;
    req.task_id = None;
    req.exclusive = false;
    req.locks.clear();
    req
}

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
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(())
        })
        .unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();

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

#[test]
fn read_only_on_unsupported_backend_fails_before_any_operation_exists() {
    let project = Project::new();
    let mut native = request(&project, "ro-native");
    native.agent_type = "opencode_native".into();
    native.pane_id = None;
    native.read_only = true;
    let err = operations::create_operation(native).unwrap_err();
    assert_eq!(err.code, "READ_ONLY_UNSUPPORTED");
    assert!(err.message.contains("opencode_native"));
    assert!(err.message.contains("claude, codex, cursor_agent"));
    assert!(operations::list_operations(&project.str()).unwrap().is_empty());

    let mut supported = request(&project, "ro-codex");
    supported.pane_id = None;
    supported.read_only = true;
    assert!(operations::create_operation(supported).is_ok());
}

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

#[test]
fn separate_operations_can_dispatch_concurrently() {
    let project = Arc::new(Project::new());
    let (first, _) = operations::create_operation(request(&project, "parallel-a")).unwrap();
    let (second, _) = operations::create_operation(request(&project, "parallel-b")).unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));

    let workers = [first, second]
        .into_iter()
        .map(|op| {
            let project_path = project.str();
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            thread::spawn(move || {
                operations::run_operation_dispatch(&project_path, &op.operation_id, |_| {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(100));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap()
            })
        })
        .collect::<Vec<_>>();

    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results
        .iter()
        .all(|op| op.status == OperationStatus::Running));
    assert_eq!(max_active.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_observations_receive_monotonically_increasing_revisions() {
    let project = Arc::new(Project::new());
    let queued = operations::create_operation(request(&project, "monotonic-revisions"))
        .unwrap()
        .0;
    let running =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| Ok(()))
            .unwrap();
    let workers_count = 12;
    let start = Arc::new(Barrier::new(workers_count));
    let workers = (0..workers_count)
        .map(|index| {
            let project = Arc::clone(&project);
            let start = Arc::clone(&start);
            let operation_id = queued.operation_id.clone();
            thread::spawn(move || {
                start.wait();
                operations::mark_operation_observation(
                    &project.str(),
                    &operation_id,
                    Some(format!("observation-{index}")),
                    None,
                    StateSource::Native,
                )
                .unwrap()
                .revision
            })
        })
        .collect::<Vec<_>>();

    let mut revisions = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    revisions.sort_unstable();
    let expected =
        (running.revision + 1..=running.revision + workers_count as u64).collect::<Vec<_>>();
    assert_eq!(revisions, expected);
    assert_eq!(
        operations::get_operation(&project.str(), &queued.operation_id)
            .unwrap()
            .revision,
        running.revision + workers_count as u64
    );
}

#[test]
fn concurrent_reservation_allows_only_one_operation_to_claim_a_pane() {
    let project = Arc::new(Project::new());
    let mut first_request = request(&project, "reserve-pane-a");
    first_request.pane_id = None;
    let mut second_request = request(&project, "reserve-pane-b");
    second_request.pane_id = None;
    let first = operations::create_operation(first_request).unwrap().0;
    let second = operations::create_operation(second_request).unwrap().0;
    for operation in [&first, &second] {
        operations::mark_operation_state(
            &project.str(),
            &operation.operation_id,
            OperationStatus::Starting,
            StateSource::Native,
            Some("dispatching".into()),
            None,
            None,
        )
        .unwrap();
    }

    let start = Arc::new(Barrier::new(2));
    let workers = [first.clone(), second.clone()]
        .into_iter()
        .map(|operation| {
            let project = Arc::clone(&project);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                operations::reserve_pane(
                    &project.str(),
                    &operation.operation_id,
                    "shared-pane",
                    false,
                )
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();

    let winners = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(winners, 1);
    let loser = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .unwrap();
    assert_eq!(loser.code, "PANE_BUSY");
    let assigned = operations::active_operation_for_pane(&project.str(), "shared-pane")
        .unwrap()
        .unwrap();
    assert!(
        assigned.operation_id == first.operation_id || assigned.operation_id == second.operation_id
    );
}

#[test]
fn requested_pane_id_is_not_busy_for_the_same_operation() {
    let project = Project::new();
    let mut req = request(&project, "self-pane");
    req.pane_id = Some("cursor-tui".into());
    let created = operations::create_operation(req).unwrap().0;
    operations::mark_operation_state(
        &project.str(),
        &created.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    assert!(
        operations::active_operation_for_pane(&project.str(), "cursor-tui")
            .unwrap()
            .is_some()
    );
    assert!(operations::conflicting_pane_operation(
        &project.str(),
        "cursor-tui",
        &created.operation_id,
    )
    .unwrap()
    .is_none());
    operations::reserve_pane(&project.str(), &created.operation_id, "cursor-tui", false).unwrap();
}

#[test]
fn conflicting_pane_operation_finds_the_other_occupant_even_when_self_is_listed_first() {
    let project = Project::new();
    let mut first_req = request(&project, "pane-first");
    first_req.pane_id = Some("shared-tui".into());
    let first = operations::create_operation(first_req).unwrap().0;
    let mut second_req = request(&project, "pane-second");
    second_req.pane_id = Some("shared-tui".into());
    let second = operations::create_operation(second_req).unwrap().0;
    operations::mark_operation_state(
        &project.str(),
        &first.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    operations::mark_operation_state(
        &project.str(),
        &second.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let vs_first =
        operations::conflicting_pane_operation(&project.str(), "shared-tui", &first.operation_id)
            .unwrap()
            .expect("other occupant");
    let vs_second =
        operations::conflicting_pane_operation(&project.str(), "shared-tui", &second.operation_id)
            .unwrap()
            .expect("other occupant");
    assert_eq!(vs_first.operation_id, second.operation_id);
    assert_eq!(vs_second.operation_id, first.operation_id);
}

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

fn locked_operation(project: &Project, key: &str, lock: &str) -> operations::OperationSnapshot {
    let mut req = request(project, key);
    req.pane_id = None;
    req.task_id = None;
    req.locks = vec![lock.into()];
    operations::create_operation(req).unwrap().0
}

#[test]
fn second_run_for_a_held_lock_fails_fast_with_resource_locked() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:src/a.rs");
    let second = locked_operation(&project, "lock-b", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();

    let error = crate::bridge::fail_operation_on_lock_conflict(&second).unwrap_err();
    assert_eq!(error.code, "RESOURCE_LOCKED");
    assert_eq!(error.context["owner_id"], first.operation_id);
    assert_eq!(error.context["resource_id"], "file:src/a.rs");
    assert!(error.retry_after_ms.is_some());
    let stored = operations::get_operation(&project.str(), &second.operation_id).unwrap();
    assert_eq!(stored.status, OperationStatus::Failed);
    // the failed run must not have stolen or freed the holder's lock
    let third = locked_operation(&project, "lock-c", "file:src/a.rs");
    assert_eq!(
        crate::bridge::acquire_operation_locks(&third).unwrap_err().code,
        "RESOURCE_LOCKED"
    );
}

#[test]
fn lock_is_free_again_after_the_holder_reaches_a_terminal_state() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();
    // re-acquiring by the same owner is idempotent
    crate::bridge::acquire_operation_locks(&first).unwrap();
    crate::bridge::release_operation_locks(&first).unwrap();

    let second = locked_operation(&project, "lock-b", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&second).unwrap();
}

#[test]
fn different_locks_do_not_conflict_and_release_only_frees_own_locks() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:src/a.rs");
    let second = locked_operation(&project, "lock-b", "file:src/b.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();
    crate::bridge::acquire_operation_locks(&second).unwrap();
    crate::bridge::release_operation_locks(&second).unwrap();
    let other = locked_operation(&project, "lock-c", "file:src/a.rs");
    assert_eq!(
        crate::bridge::acquire_operation_locks(&other).unwrap_err().code,
        "RESOURCE_LOCKED"
    );
    // releasing by a non-owner must not free the holder's lock
    crate::bridge::release_operation_locks(&other).unwrap();
    assert_eq!(
        crate::bridge::acquire_operation_locks(&other).unwrap_err().code,
        "RESOURCE_LOCKED"
    );
}

#[test]
fn concurrent_runs_locking_the_same_file_let_exactly_one_proceed() {
    let project = Arc::new(Project::new());
    let ops = (0..6)
        .map(|index| locked_operation(&project, &format!("race-{index}"), "file:src/race.rs"))
        .collect::<Vec<_>>();
    let start = Arc::new(Barrier::new(ops.len()));
    let threads = ops
        .into_iter()
        .map(|op| {
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                crate::bridge::fail_operation_on_lock_conflict(&op).is_ok()
            })
        })
        .collect::<Vec<_>>();
    let winners = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
}
