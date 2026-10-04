//! Operations tests: idempotency.

use super::*;

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
fn delegate_work_honors_context_policy() {
    let dir = temp();
    let mut req = request(&dir, "policy");
    req.context_policy = Some(ContextPolicy::Fresh);
    let op = delegate_work(req, |_| Ok(())).unwrap();
    assert_eq!(op.worker.context_policy, ContextPolicy::Fresh);
    let _ = fs::remove_dir_all(dir);
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

#[cfg(windows)]
#[test]
fn operations_persist_when_the_project_path_pushes_files_past_max_path() {
    // B3: project_path >= ~158 chars made the atomic replace fail with a raw os error 3.
    let mut dir = temp();
    while dir.as_os_str().len() < 170 {
        dir = dir.join("long-project-segment");
    }
    fs::create_dir_all(&dir).unwrap();
    let (op, _) = create_operation(request(&dir, "long-path")).unwrap();
    let path = path_for(&op.project_path, &op.operation_id).unwrap();
    assert!(path.as_os_str().len() + 45 > 260, "{}", path.as_os_str().len());
    let again = get_operation(&op.project_path, &op.operation_id).unwrap();
    assert_eq!(again.operation_id, op.operation_id);
    let _ = fs::remove_dir_all(
        dir.ancestors().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("pm-ops-"))).unwrap(),
    );
}
