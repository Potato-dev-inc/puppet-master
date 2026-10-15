//! Cancellation of queued and dispatched operations.

use super::model::*;
use super::store::{bump, now_ms};
use super::get_operation;
use super::store::update_lock;

pub fn cancel_operation_with<F>(
    project: &str,
    id: &str,
    worker_cancel: F,
) -> Result<OperationSnapshot, OperationError>
where
    F: FnOnce(&OperationSnapshot) -> Result<(), OperationError>,
{
    let (snapshot, previous) = {
        let _serial = update_lock();
        let mut snapshot = get_operation(project, id)?;
        if snapshot.status == OperationStatus::Queued {
            snapshot.status = OperationStatus::Cancelled;
            snapshot.stage = Some("cancelled".into());
            snapshot.finished_at_ms = Some(now_ms());
            bump(&mut snapshot)?;
            return Ok(snapshot);
        }
        if snapshot.status.terminal() {
            return Ok(snapshot);
        }
        if snapshot.status == OperationStatus::Cancelling {
            return Err(OperationError::new(
                "CANCELLATION_IN_PROGRESS",
                "cancellation is already in progress",
                true,
            ));
        }
        let previous = snapshot.status;
        snapshot.status = OperationStatus::Cancelling;
        snapshot.stage = Some("cancelling".into());
        bump(&mut snapshot)?;
        (snapshot, previous)
    };
    let cancel_result = worker_cancel(&snapshot);
    let _serial = update_lock();
    let mut current = get_operation(project, id)?;
    if current.status != OperationStatus::Cancelling {
        return Ok(current);
    }
    match cancel_result {
        Ok(()) => {
            current.status = OperationStatus::Cancelled;
            current.stage = Some("cancelled".into());
            current.required_action = None;
            current.finished_at_ms = Some(now_ms());
            bump(&mut current)?;
            Ok(current)
        }
        Err(mut err) => {
            current.status = previous;
            current.stage = Some("cancellation_failed".into());
            current.error = Some(err.clone());
            err.context = serde_json::json!({"operation_id": id, "cancellation_failed": true});
            bump(&mut current)?;
            Err(err)
        }
    }
}

pub fn cancel_operation(project: &str, id: &str) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status == OperationStatus::Starting
        || op.status == OperationStatus::Running
        || op.status == OperationStatus::WaitingInput
    {
        return Err(OperationError::new(
            "CANCELLATION_REQUIRES_WORKER_CONTROL",
            "operation is already dispatched; use worker cancellation",
            false,
        ));
    }
    if !op.status.terminal() {
        op.status = OperationStatus::Cancelled;
        op.stage = Some("cancelled".into());
        op.finished_at_ms = Some(now_ms());
        bump(&mut op)?;
    }
    Ok(op)
}
