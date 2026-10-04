//! Operations tests: waits.

use super::*;

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
fn wait_blocks_for_the_requested_time_when_nothing_changes() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let running = delegate_work(request(&dir, "wait-honours-timeout"), |op| {
        op.pane_id = Some("p1".into());
        Ok(())
    })
    .unwrap();
    let cursor = OperationWaitCursor {
        project_path: project,
        operation_id: running.operation_id.clone(),
        after_revision: running.revision,
    };
    let started = std::time::Instant::now();
    let result = wait_for_operations(&[cursor], None, 900, false, false).unwrap();
    let elapsed = started.elapsed();
    assert_eq!(result.reason, "timeout");
    assert!(
        elapsed >= std::time::Duration::from_millis(850),
        "returned after {elapsed:?}, before the 900 ms timeout"
    );
    assert!(elapsed < std::time::Duration::from_secs(5), "blocked far too long: {elapsed:?}");
    let _ = fs::remove_dir_all(dir);
}
