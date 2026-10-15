//! Operations tests: the watchdog fails a run past its deadline when no supervisor is left.

use super::*;
use std::cell::Cell;

fn running_run(dir: &Path, key: &str, timeout_ms: u64) -> OperationSnapshot {
    let mut req = request(dir, key);
    req.timeout_ms = timeout_ms;
    delegate_work(req, |_| Ok(())).unwrap()
}

fn deadline_of(op: &OperationSnapshot) -> u64 {
    op.started_at_ms.unwrap_or(op.created_at_ms) + op.timeout_ms
}

const GRACE: u64 = WATCHDOG_GRACE_MS;

#[test]
fn a_running_run_nobody_supervises_is_failed_with_a_typed_timeout_after_its_deadline() {
    // R8 "left undone": a supervisor that panics (or loses 40 snapshot reads in a row) leaves its
    // run `running` for ever. Nothing in this test starts a supervisor: only the record exists.
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running_run(&dir, "wd-running", 1_000);
    assert_eq!(op.status, OperationStatus::Running);
    let stops = Cell::new(0);
    let failed = sweep_expired_runs(deadline_of(&op) + GRACE + 5, GRACE, Some(&project), &mut |_| {
        stops.set(stops.get() + 1);
        Ok(())
    });
    assert_eq!(failed.len(), 1, "the overdue run is failed");
    assert_eq!(stops.get(), 1, "its worker is stopped first");
    let saved = get_operation(&project, &op.operation_id).unwrap();
    assert_eq!(saved.status, OperationStatus::Failed);
    assert_eq!(saved.stage.as_deref(), Some("timeout"));
    let error = saved.error.expect("a typed error");
    assert_eq!(error.code, "TIMEOUT");
    assert!(error.recoverable);
    assert_eq!(error.context["enforced_by"], "watchdog");
    assert_eq!(error.context["timeout_ms"], 1_000);
    assert!(saved.finished_at_ms.is_some());
    // Failing it again is a no-op: a terminal run is never touched twice.
    let again = sweep_expired_runs(deadline_of(&op) + GRACE + 10, GRACE, Some(&project), &mut |_| {
        panic!("a failed run has no worker to stop")
    });
    assert!(again.is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_run_waiting_for_input_past_its_deadline_is_failed_too() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running_run(&dir, "wd-waiting", 1_000);
    mark_operation_waiting_input(
        &project,
        &op.operation_id,
        Some("waiting_input".into()),
        serde_json::json!({"id": "p1", "summary": "Run command: npm test"}),
        StateSource::Inferred,
    )
    .unwrap();
    let failed = sweep_expired_runs(deadline_of(&op) + GRACE, GRACE, Some(&project), &mut |_| Ok(()));
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].error.as_ref().unwrap().code, "TIMEOUT");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_run_inside_its_deadline_or_its_grace_is_left_to_its_own_supervisor() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running_run(&dir, "wd-healthy", 1_000);
    let deadline = deadline_of(&op);
    for now in [deadline - 1, deadline, deadline + GRACE - 1] {
        let failed = sweep_expired_runs(now, GRACE, Some(&project), &mut |_| {
            panic!("must not stop a worker whose supervisor still has time")
        });
        assert!(failed.is_empty(), "now = deadline{:+}", now as i64 - deadline as i64);
    }
    assert_eq!(
        get_operation(&project, &op.operation_id).unwrap().status,
        OperationStatus::Running
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_run_that_finished_is_never_touched() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running_run(&dir, "wd-done", 1_000);
    mark_operation_finished(&project, &op.operation_id, Some("PONG".into()), ResultCapture::Authoritative, false).unwrap();
    let failed = sweep_expired_runs(deadline_of(&op) + 10 * GRACE, GRACE, Some(&project), &mut |_| {
        panic!("a finished run has nothing to stop")
    });
    assert!(failed.is_empty());
    assert_eq!(
        get_operation(&project, &op.operation_id).unwrap().status,
        OperationStatus::Completed
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_worker_that_cannot_be_stopped_does_not_keep_the_run_alive() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running_run(&dir, "wd-stuck-worker", 1_000);
    let failed = sweep_expired_runs(deadline_of(&op) + GRACE, GRACE, Some(&project), &mut |_| {
        Err(OperationError::new("WORKER_STOP_FAILED", "the pane would not close", true))
    });
    assert_eq!(failed.len(), 1);
    let error = failed[0].error.as_ref().unwrap();
    assert_eq!(error.code, "TIMEOUT");
    assert_eq!(error.context["worker_stop_error"], "the pane would not close");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_sweep_scoped_to_one_project_leaves_the_runs_of_others_alone() {
    let (mine, other) = (temp(), temp());
    let (mine_path, other_path) = (mine.to_string_lossy().into_owned(), other.to_string_lossy().into_owned());
    let a = running_run(&mine, "wd-scope-a", 1_000);
    let b = running_run(&other, "wd-scope-b", 1_000);
    let now = deadline_of(&a).max(deadline_of(&b)) + GRACE;
    let failed = sweep_expired_runs(now, GRACE, Some(&mine_path), &mut |_| Ok(()));
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].operation_id, a.operation_id);
    assert_eq!(
        get_operation(&other_path, &b.operation_id).unwrap().status,
        OperationStatus::Running
    );
    let _ = fs::remove_dir_all(mine);
    let _ = fs::remove_dir_all(other);
}

/// Review R12 1.7: a run parked by its dispatch thread (`waiting_for_pane_readiness`: a startup
/// dialog, or a screen that is not ready, for up to 5 minutes) is supervised by that thread. With a
/// `timeout_ms` under the park, the watchdog failed it as "nothing was supervising it" and stopped
/// the worker in the user's pane.
#[test]
fn a_run_parked_by_its_dispatch_thread_is_not_failed_by_the_watchdog() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running_run(&dir, "wd-parked", 1_000);
    let mut parked = get_operation(&project, &op.operation_id).unwrap();
    parked.status = OperationStatus::WaitingInput;
    parked.stage = Some("waiting_for_pane_readiness".into());
    parked.required_action = Some(serde_json::json!({"kind": "screen_not_ready"}));
    persist(&parked).unwrap();
    let failed = sweep_expired_runs(deadline_of(&op) + GRACE + 5, GRACE, Some(&project), &mut |_| {
        panic!("the parked run's worker must not be stopped")
    });
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(get_operation(&project, &op.operation_id).unwrap().status, OperationStatus::WaitingInput);
    // Once the park is over and the run is past its deadline with no one on it, the watchdog acts.
    let mut resumed = get_operation(&project, &op.operation_id).unwrap();
    resumed.status = OperationStatus::Running;
    resumed.stage = Some("running".into());
    persist(&resumed).unwrap();
    let failed = sweep_expired_runs(deadline_of(&op) + GRACE + 10, GRACE, Some(&project), &mut |_| Ok(()));
    assert_eq!(failed.len(), 1);
    let _ = fs::remove_dir_all(dir);
}
