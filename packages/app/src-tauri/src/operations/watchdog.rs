//! The operation watchdog: a periodic sweep that fails any run still `running` or `waiting_input`
//! well past its deadline.
//!
//! A run parked before its task was typed (`stage: waiting_for_pane_readiness`) is skipped: its
//! dispatch thread supervises the park and ends it (review R12 1.7).
//!
//! A run's own supervisor (`agent_runs::headless::supervise`) enforces `timeout_ms`, but it is a
//! thread: when it panics, or gives up after repeated snapshot read failures, nothing is left
//! to end the run and it stays active for ever. The watchdog does not depend on any supervisor:
//! it only looks at the durable record. It acts `WATCHDOG_GRACE_MS` after the deadline, so a
//! healthy supervisor (which acts at the deadline) always gets there first.

use super::model::*;
use super::store::{deadline_ms, now_ms, untrack_for_watchdog, watched_past_deadline};
use super::{get_operation, mark_operation_state};

/// How long after its deadline a run must still be active before the watchdog steps in.
pub const WATCHDOG_GRACE_MS: u64 = 60_000;
/// How often the app sweeps.
pub const WATCHDOG_INTERVAL_MS: u64 = 15_000;

/// Fails every watched run that is `grace` or more past its deadline at `now`, with a typed
/// `TIMEOUT` error whose context says the watchdog ended it. `stop_worker` is asked first (best
/// effort: stopping the agent can fail, the run is failed anyway and the reason is recorded in the
/// error context). `only_project` limits the sweep to one project (tests); the app passes `None`.
/// Returns the snapshots it failed, so the caller can publish them.
pub fn sweep_expired_runs(
    now: u64,
    grace_ms: u64,
    only_project: Option<&str>,
    stop_worker: &mut dyn FnMut(&OperationSnapshot) -> Result<(), OperationError>,
) -> Vec<OperationSnapshot> {
    let mut failed = Vec::new();
    for (project, id, _) in watched_past_deadline(now, grace_ms, only_project) {
        let current = match get_operation(&project, &id) {
            Ok(current) => current,
            Err(error) => {
                // The record is gone or unreadable for good: nothing left to end.
                if error.code == "OPERATION_NOT_FOUND" || error.code == "INVALID_PROJECT_PATH" {
                    untrack_for_watchdog(&project, &id);
                }
                continue;
            }
        };
        if !matches!(
            current.status,
            OperationStatus::Running | OperationStatus::WaitingInput
        ) {
            untrack_for_watchdog(&project, &id);
            continue;
        }
        // Parked by its dispatch thread (a startup dialog, a screen that is not ready): that thread
        // is supervising it and ends the park itself (answered, declined, gone, cancelled, or five
        // minutes). Stays tracked, so it is looked at again once the park is over.
        if current.stage.as_deref() == Some("waiting_for_pane_readiness") {
            continue;
        }
        let deadline = deadline_ms(&current);
        if now < deadline.saturating_add(grace_ms) {
            continue; // the record moved on (a steer or a follow-up reset its clock)
        }
        let stop_error = stop_worker(&current).err().map(|error| error.message);
        let mut error = OperationError::new(
            "TIMEOUT",
            "the run passed its deadline and nothing was supervising it, so Puppet Master ended it",
            true,
        );
        error.context = serde_json::json!({
            "timeout_ms": current.timeout_ms,
            "enforced_by": "watchdog",
            "overdue_ms": now.saturating_sub(deadline),
            "worker_stop_error": stop_error,
        });
        match mark_operation_state(
            &project,
            &id,
            OperationStatus::Failed,
            StateSource::Native,
            Some("timeout".into()),
            None,
            Some(error),
        ) {
            Ok(snapshot) if snapshot.status == OperationStatus::Failed => failed.push(snapshot),
            _ => {}
        }
    }
    failed
}

/// `sweep_expired_runs` at the current time with the standard grace.
pub fn sweep_now(
    stop_worker: &mut dyn FnMut(&OperationSnapshot) -> Result<(), OperationError>,
) -> Vec<OperationSnapshot> {
    sweep_expired_runs(now_ms(), WATCHDOG_GRACE_MS, None, stop_worker)
}
