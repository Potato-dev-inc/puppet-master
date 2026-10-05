//! Waiting on operations: the status predicate for "the coordinator can act" and the
//! single- and multi-handle blocking waits driven by the store's change condvar.

use super::model::*;
use super::store::change;
use super::get_operation;
use std::time::{Duration, Instant};

/// Statuses that mean the coordinator can act on a turn (answer, follow up, or handle failure).
pub fn completion_wait_until() -> Vec<OperationStatus> {
    vec![
        OperationStatus::Completed,
        OperationStatus::Failed,
        OperationStatus::Cancelled,
        OperationStatus::WaitingInput,
    ]
}

fn operation_wait_ready(
    snapshot: &OperationSnapshot,
    cursor: &OperationWaitCursor,
    until: &Option<Vec<OperationStatus>>,
    wake_on_progress: bool,
) -> (bool, &'static str) {
    if wake_on_progress {
        if snapshot.revision > cursor.after_revision {
            return (true, "revision_changed");
        }
        if snapshot.status.terminal() {
            return (true, "terminal");
        }
        return (false, "timeout");
    }
    let states = until
        .as_ref()
        .cloned()
        .unwrap_or_else(completion_wait_until);
    let status_matched = states.contains(&snapshot.status);
    if status_matched
        && (snapshot.revision > cursor.after_revision || snapshot.status.terminal())
    {
        return (true, "matched_state");
    }
    (false, "timeout")
}

pub fn wait_for_operation(
    project: &str,
    id: &str,
    after_revision: u64,
    until: Option<Vec<OperationStatus>>,
    timeout_ms: u64,
) -> Result<OperationWaitResult, OperationError> {
    wait_for_operation_with_stage(project, id, after_revision, until, None, timeout_ms)
}

pub fn wait_for_operation_with_stage(
    project: &str,
    id: &str,
    after_revision: u64,
    until: Option<Vec<OperationStatus>>,
    stage: Option<&str>,
    timeout_ms: u64,
) -> Result<OperationWaitResult, OperationError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(300_000));
    loop {
        let (lock, cv) = change();
        let mut guard = lock.lock();
        let generation = *guard;
        drop(guard);
        let snapshot = get_operation(project, id)?;
        let matched = until
            .as_ref()
            .is_some_and(|states| states.contains(&snapshot.status));
        let matched_stage = stage.is_some_and(|value| snapshot.stage.as_deref() == Some(value));
        if snapshot.revision > after_revision
            && (until.is_none() || matched || matched_stage || snapshot.status.terminal())
        {
            return Ok(OperationWaitResult {
                snapshot,
                reason: if matched {
                    "matched_state"
                } else {
                    "revision_changed"
                }
                .into(),
            });
        }
        if snapshot.status.terminal() {
            return Ok(OperationWaitResult {
                snapshot,
                reason: "terminal".into(),
            });
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(OperationWaitResult {
                snapshot,
                reason: "timeout".into(),
            });
        }
        guard = lock.lock();
        if *guard == generation {
            cv.wait_for(&mut guard, deadline.saturating_duration_since(now));
        }
    }
}

/// Wait on multiple durable handles without polling the filesystem. Any mode returns on the
/// first changed/terminal operation; all mode returns once every operation meets the predicate.
pub fn wait_for_operations(
    cursors: &[OperationWaitCursor],
    until: Option<Vec<OperationStatus>>,
    timeout_ms: u64,
    wait_for_all: bool,
    wake_on_progress: bool,
) -> Result<MultiOperationWaitResult, OperationError> {
    if cursors.is_empty() {
        return Err(OperationError::new(
            "INVALID_AGENT_HANDLES",
            "at least one agent handle is required",
            false,
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(300_000));
    loop {
        let (lock, cv) = change();
        let mut guard = lock.lock();
        let generation = *guard;
        drop(guard);
        let snapshots = cursors
            .iter()
            .map(|cursor| get_operation(&cursor.project_path, &cursor.operation_id))
            .collect::<Result<Vec<_>, _>>()?;
        let mut wake_reasons = Vec::with_capacity(snapshots.len());
        let ready = snapshots
            .iter()
            .zip(cursors)
            .map(|(snapshot, cursor)| {
                let (is_ready, reason) =
                    operation_wait_ready(snapshot, cursor, &until, wake_on_progress);
                wake_reasons.push(reason.into());
                is_ready
            })
            .collect::<Vec<_>>();
        let all_ready = ready.iter().all(|value| *value);
        let any_ready = ready.iter().any(|value| *value);
        if (wait_for_all && all_ready) || (!wait_for_all && any_ready) {
            let reason = if any_ready {
                wake_reasons
                    .iter()
                    .find(|value| *value != "timeout")
                    .cloned()
                    .unwrap_or_else(|| "matched_state".into())
            } else {
                "timeout".into()
            };
            return Ok(MultiOperationWaitResult {
                snapshots,
                reason,
                wake_reasons,
            });
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(MultiOperationWaitResult {
                snapshots,
                reason: "timeout".into(),
                wake_reasons,
            });
        }
        guard = lock.lock();
        if *guard == generation {
            cv.wait_for(&mut guard, deadline.saturating_duration_since(now));
        }
    }
}
