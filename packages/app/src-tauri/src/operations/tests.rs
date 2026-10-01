use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

fn request(dir: &Path, key: &str) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: dir.to_string_lossy().into_owned(),
        task: "do work".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: key.into(),
        acceptance_criteria: Some(vec!["behavior works".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
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
#[test]
fn completion_default_wait_ignores_running_progress_revisions() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let mut running = create_operation(request(&dir, "progress-wait"))
        .unwrap()
        .0;
    running.status = OperationStatus::Running;
    persist(&running).unwrap();
    let cursor = OperationWaitCursor {
        project_path: project.clone(),
        operation_id: running.operation_id.clone(),
        after_revision: running.revision,
    };
    running.revision += 1;
    running.stage = Some("tool_call".into());
    persist(&running).unwrap();
    let result = wait_for_operations(&[cursor], None, 50, false, false).unwrap();
    assert_eq!(result.reason, "timeout");
    let _ = fs::remove_dir_all(dir);
}

fn temp() -> PathBuf {
    std::env::temp_dir().join(format!("pm-ops-{}", Uuid::new_v4()))
}

#[test]
fn persists_before_dispatch_and_reuses_idempotency_key() {
    let dir = temp();
    let count = AtomicUsize::new(0);
    let first = delegate_work(request(&dir, "same"), |op| {
        count.fetch_add(1, Ordering::SeqCst);
        assert!(get_operation(&dir.to_string_lossy(), &op.operation_id).is_ok());
        op.pane_id = Some("p1".into());
        op.pane_created = true;
        op.task_id = Some("task-assigned".into());
        Ok(())
    })
    .unwrap();
    let second = delegate_work(request(&dir, "same"), |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(first.operation_id, second.operation_id);
    assert_eq!(second.status, OperationStatus::Running);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn one_agent_handle_cannot_claim_the_same_turn_index_twice() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let mut first_request = request(&dir, "turn-zero-a");
    first_request.agent_run_id = Some("stable-agent".into());
    let first = create_operation(first_request).unwrap().0;

    let mut duplicate = request(&dir, "turn-zero-b");
    duplicate.agent_run_id = Some("stable-agent".into());
    let error = create_operation(duplicate).unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_EXISTS");

    let mut next_turn = request(&dir, "turn-one");
    next_turn.agent_run_id = Some("stable-agent".into());
    next_turn.turn_index = 1;
    let second = create_operation(next_turn).unwrap().0;
    assert_ne!(first.operation_id, second.operation_id);
    assert_eq!(
        latest_agent_run(&project, "stable-agent")
            .unwrap()
            .turn_index,
        1
    );
    let _ = fs::remove_dir_all(dir);
}

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
fn wait_observes_revision_and_terminal_state() {
    let dir = temp();
    let op = delegate_work(request(&dir, "wait"), |_| Ok(())).unwrap();
    let updated = mark_operation_state(
        &dir.to_string_lossy(),
        &op.operation_id,
        OperationStatus::WaitingInput,
        StateSource::Native,
        Some("approval".into()),
        None,
        None,
    )
    .unwrap();
    let got = wait_for_operation(
        &dir.to_string_lossy(),
        &op.operation_id,
        op.revision,
        Some(vec![OperationStatus::WaitingInput]),
        20,
    )
    .unwrap();
    assert_eq!(got.reason, "matched_state");
    assert_eq!(got.snapshot.revision, updated.revision);
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
fn explicit_task_evidence_completes_and_conflicting_key_is_rejected() {
    let dir = temp();
    let mut req = request(&dir, "task");
    req.task_id = Some("task-1".into());
    let op = delegate_work(req.clone(), |_| Ok(())).unwrap();
    let completed = mark_task_completed(&dir.to_string_lossy(), "task-1", "tests passed").unwrap();
    assert_eq!(completed[0].status, OperationStatus::Completed);
    assert_eq!(completed[0].result.as_deref(), Some("tests passed"));
    req.task = "different work".into();
    assert_eq!(
        delegate_work(req, |_| Ok(())).unwrap_err().code,
        "IDEMPOTENCY_KEY_CONFLICT"
    );
    assert_eq!(
        get_operation(&dir.to_string_lossy(), &op.operation_id)
            .unwrap()
            .status,
        OperationStatus::Completed
    );
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

#[test]
fn shell_presets_cannot_be_used_for_delegated_work() {
    let dir = temp();
    let mut req = request(&dir, "shell-agent");
    req.agent_type = "powershell".into();
    assert_eq!(
        create_operation(req).unwrap_err().code,
        "INVALID_AGENT_TYPE"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn recover_all_visits_nonselected_indexed_projects() {
    let selected = temp();
    let other_project = temp();
    let (queued, _) = create_operation(request(&selected, "recover-selected")).unwrap();
    mark_operation_state(
        &selected.to_string_lossy(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let (other, _) = create_operation(request(&other_project, "recover-other")).unwrap();
    for (project, operation_id) in [
        (&selected, queued.operation_id.clone()),
        (&other_project, other.operation_id.clone()),
    ] {
        let path = path_for(&project.to_string_lossy(), &operation_id).unwrap();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["runtime_id"] = serde_json::Value::String("previous-runtime".into());
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    }
    let recovered = recover_all_interrupted_operations().unwrap();
    let recovered_ids = recovered
        .iter()
        .map(|op| op.operation_id.as_str())
        .collect::<Vec<_>>();
    assert!(recovered_ids.contains(&queued.operation_id.as_str()));
    assert!(recovered_ids.contains(&other.operation_id.as_str()));
    assert!(recovered
        .iter()
        .all(|op| op.status == OperationStatus::Failed));
    let _ = fs::remove_dir_all(selected);
    let _ = fs::remove_dir_all(other_project);
}

#[test]
fn recover_all_skips_project_removed_after_indexing() {
    let removed_project = temp();
    create_operation(request(&removed_project, "removed-project-indexed")).unwrap();
    fs::remove_dir_all(&removed_project).unwrap();
    assert!(recover_all_interrupted_operations().is_ok());
}

#[test]
fn corrupt_indexes_fail_without_overwriting_or_persisting_operation() {
    struct ResetIndex;
    impl Drop for ResetIndex {
        fn drop(&mut self) {
            store::set_test_index_dir(None);
        }
    }
    let index_dir = temp();
    fs::create_dir_all(&index_dir).unwrap();
    store::set_test_index_dir(Some(index_dir.clone()));
    let _reset = ResetIndex;
    let project_index = index_dir.join("operation-project-index.json");
    let corrupt_projects = b"{broken";
    fs::write(&project_index, corrupt_projects).unwrap();
    let project = temp();
    let err = create_operation(request(&project, "corrupt-project-index")).unwrap_err();
    assert_eq!(err.code, "STORAGE_ERROR");
    assert_eq!(fs::read(&project_index).unwrap(), corrupt_projects);
    assert!(fs::read_dir(root(&project.to_string_lossy()).unwrap())
        .unwrap()
        .next()
        .is_none());

    fs::write(&project_index, b"[]").unwrap();
    let task_index = index_dir.join("operation-task-index.json");
    let corrupt_tasks = b"not json";
    fs::write(&task_index, corrupt_tasks).unwrap();
    let mut req = request(&project, "corrupt-task-index");
    req.task_id = Some("task-corrupt-index".into());
    let err = create_operation(req).unwrap_err();
    assert_eq!(err.code, "STORAGE_ERROR");
    assert_eq!(fs::read(&task_index).unwrap(), corrupt_tasks);
    assert_eq!(fs::read(&project_index).unwrap(), b"[]");
    assert!(fs::read_dir(root(&project.to_string_lossy()).unwrap())
        .unwrap()
        .next()
        .is_none());
    let _ = fs::remove_dir_all(index_dir);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn unknown_agent_run_error_names_the_id_and_expected_kind() {
    let dir = temp();
    let error = latest_agent_run(&dir.to_string_lossy(), "57c03490").unwrap_err();
    assert_eq!(error.code, "AGENT_NOT_FOUND");
    assert!(error.message.contains("57c03490"));
    assert!(error.message.contains("agent run handle"));
    assert_eq!(
        error.context.get("handle").and_then(Value::as_str),
        Some("57c03490")
    );
}

fn listed_run(
    dir: &Path,
    key: &str,
    owner: &str,
    status: OperationStatus,
    finished_at_ms: Option<u64>,
) -> OperationSnapshot {
    let mut req = request(dir, key);
    req.owner_session_id = Some(owner.into());
    let mut snapshot = delegate_work(req, |_| Ok(())).unwrap();
    snapshot.status = status;
    snapshot.finished_at_ms = finished_at_ms;
    snapshot
}

#[test]
fn list_agents_keeps_recent_ended_runs_marks_ownership_and_expires_old_ones() {
    use crate::agent_runs::visible_runs;
    let dir = temp();
    let now = 10 * 24 * 60 * 60 * 1000;
    let hour = 60 * 60 * 1000;
    let runs = vec![
        listed_run(&dir, "active", "me", OperationStatus::Running, None),
        listed_run(
            &dir,
            "failed-recent",
            "me",
            OperationStatus::Failed,
            Some(now - hour),
        ),
        listed_run(
            &dir,
            "cancelled-old",
            "me",
            OperationStatus::Cancelled,
            Some(now - 48 * hour),
        ),
        listed_run(
            &dir,
            "other-conn",
            "someone-else",
            OperationStatus::Completed,
            Some(now - hour),
        ),
    ];
    let ids: Vec<_> = runs.iter().map(|run| run.operation_id.clone()).collect();
    let visible = visible_runs(runs, Some("me"), now);
    let owned_of = |id: &str| {
        visible
            .iter()
            .find(|(run, _)| run.operation_id == id)
            .map(|(_, owned)| *owned)
    };
    assert_eq!(owned_of(&ids[0]), Some(true));
    assert_eq!(owned_of(&ids[1]), Some(true), "recent failure stays listed");
    assert_eq!(
        owned_of(&ids[2]),
        None,
        "ended run past retention is dropped"
    );
    assert_eq!(
        owned_of(&ids[3]),
        Some(false),
        "other connection's run is listed but not owned"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn compact_output_baseline_strips_tui_box_art() {
    let raw = "   \n┌──────────────────────────────┐\n│ Add follow-up                │\n└──────────────────────────────┘\n\nDeepSeek V4.1 Flash · Working\n";
    let compact = super::compact_output_baseline(raw).unwrap_or_default();
    assert!(!compact.contains('┌'));
    assert!(compact.contains("DeepSeek") || compact.contains("Working"));
}

#[test]
fn compact_output_baseline_omitted_when_only_chrome() {
    let raw = "   
╭────────╮
│  ▀▀▀▀  │
╰────────╯

   
";
    assert!(super::compact_output_baseline(raw).is_none());
    let mut snapshot = create_operation(request(&temp(), "baseline-api")).unwrap().0;
    snapshot.output_baseline = Some("x".into());
    let api = super::snapshot_for_api(snapshot);
    assert!(!serde_json::to_value(api).unwrap().as_object().unwrap().contains_key("output_baseline"));
}

#[test]
fn applied_delegate_context_policy_reflects_history() {
    use super::applied_delegate_context_policy as applied;
    assert_eq!(applied(None, 0, 0), ContextPolicy::Fresh);
    assert_eq!(applied(None, 1, 0), ContextPolicy::Resume);
    assert_eq!(applied(None, 0, 2), ContextPolicy::Resume);
    assert_eq!(applied(Some(ContextPolicy::Packet), 0, 0), ContextPolicy::Packet);
}

#[test]
fn compact_output_baseline_truncates_on_char_boundary() {
    let raw = format!("a{}", "ä".repeat(400));
    let compact = super::compact_output_baseline(&raw).unwrap_or_default();
    assert!(compact.starts_with('…'));
    assert!(compact.len() <= 480 + '…'.len_utf8());
}

#[test]
fn acceptance_criteria_are_evaluated_on_completion() {
    let dir = temp();
    let op = delegate_work(request(&dir, "accept"), |_| Ok(())).unwrap();
    mark_operation_finished(
        &dir.to_string_lossy(),
        &op.operation_id,
        Some("behavior works as expected".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let finished = get_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(
        finished.acceptance_status,
        AcceptanceStatus::Passed,
        "criteria substring should match result"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn acceptance_text_mismatch_is_not_checked_not_failed() {
    let dir = temp();
    let mut req = request(&dir, "accept-soft");
    req.acceptance_criteria = Some(vec!["behavior works".into()]);
    let op = delegate_work(req, |_| Ok(())).unwrap();
    mark_operation_finished(
        &dir.to_string_lossy(),
        &op.operation_id,
        Some("The backend is a Rust/Tauri crate with src and Cargo.toml.".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let finished = get_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(
        finished.acceptance_status,
        AcceptanceStatus::NotChecked,
        "paraphrased answers must not hard-fail text criteria"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn delegate_work_honors_context_policy() {
    let dir = temp();
    let mut req = request(&dir, "policy");
    req.context_policy = Some(ContextPolicy::Fresh);
    let op = delegate_work(req, |_| Ok(())).unwrap();
    assert_eq!(op.worker.context_policy, ContextPolicy::Fresh);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn acceptance_file_exists_criterion() {
    let dir = temp();
    fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("proof.txt");
    fs::write(&marker, "ok").unwrap();
    let mut req = request(&dir, "accept-file");
    req.acceptance_criteria = Some(vec!["file_exists:proof.txt".into()]);
    let op = delegate_work(req, |_| Ok(())).unwrap();
    mark_operation_finished(
        &dir.to_string_lossy(),
        &op.operation_id,
        Some("done".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let finished = get_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(finished.acceptance_status, AcceptanceStatus::Passed);
    let _ = fs::remove_dir_all(dir);
}
