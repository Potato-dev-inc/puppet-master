//! Startup readiness handshake: parking a dispatching operation until the pane is ready
//! for the prompt, then resuming dispatch.

use super::model::*;
use super::store::{bump, now_ms};
use super::get_operation;
use serde_json::Value;
use super::store::update_lock;

/// Record that the startup dispatcher is waiting for an explicit pane-readiness decision.
/// This is distinct from a normal task-level input request so pane observations cannot erase
/// the startup stage or accidentally resume dispatch.
pub fn mark_operation_startup_wait(
    project: &str,
    id: &str,
    pane_state: Option<String>,
    required_action: Option<Value>,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    // The pane observer can already have recorded a blocking dialog (status WaitingInput, stage
    // still waiting_for_pane_readiness) before the dispatch thread parks; parking is then a
    // no-op update, not an invalid transition that would kill the dispatch thread.
    let already_parked = op.status == OperationStatus::WaitingInput
        && op.stage.as_deref() == Some("waiting_for_pane_readiness");
    if op.status != OperationStatus::Starting && !already_parked {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "startup readiness wait requires a dispatching operation",
            false,
        ));
    }
    op.status = OperationStatus::WaitingInput;
    op.stage = Some("waiting_for_pane_readiness".into());
    op.pane_state = pane_state;
    op.required_action = required_action;
    op.observed_at_ms = Some(now_ms());
    op.source = source;
    bump(&mut op)?;
    Ok(op)
}

/// A startup prompt was resolved and the pane is redrawing. Drop the parked prompt and report the
/// run as starting again instead of "needs input" with nothing to answer; the stage stays
/// waiting_for_pane_readiness so status events cannot move it before dispatch resumes.
pub fn mark_operation_startup_settling(
    project: &str,
    id: &str,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status != OperationStatus::WaitingInput
        || op.stage.as_deref() != Some("waiting_for_pane_readiness")
    {
        return Ok(op);
    }
    op.status = OperationStatus::Starting;
    op.pane_state = Some("running".into());
    op.required_action = None;
    op.observed_at_ms = Some(now_ms());
    op.source = source;
    bump(&mut op)?;
    Ok(op)
}

/// Resume startup only after the bridge's readiness classifier supplies a current pane state.
/// `waiting_input` or `running` is accepted when the classifier has established that any prior
/// prompt was cleared; the operation retains that observed state instead of fabricating `idle`.
pub fn resume_operation_dispatch_when_ready(
    project: &str,
    id: &str,
    actual_pane_state: &str,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    if !matches!(actual_pane_state, "idle" | "waiting_input" | "running") {
        return Err(OperationError::new(
            "OPERATION_NOT_READY_TO_RESUME",
            "readiness classifier did not report a dispatchable pane state",
            true,
        ));
    }
    let _serial = update_lock();
    resume_operation_dispatch_locked(project, id, actual_pane_state, source)
}

fn resume_operation_dispatch_locked(
    project: &str,
    id: &str,
    actual_pane_state: &str,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let mut op = get_operation(project, id)?;
    // WaitingInput while a prompt is parked; Starting once the prompt is resolved and the pane
    // is only settling (see `mark_operation_startup_settling`).
    if !matches!(op.status, OperationStatus::WaitingInput | OperationStatus::Starting)
        || op.stage.as_deref() != Some("waiting_for_pane_readiness")
    {
        return Err(OperationError::new(
            "OPERATION_NOT_READY_TO_RESUME",
            "operation is not waiting for startup pane readiness",
            true,
        ));
    }
    op.status = OperationStatus::Starting;
    if op.started_at_ms.is_none() {
        op.started_at_ms = Some(now_ms());
    }
    op.stage = Some("dispatching".into());
    op.pane_state = Some(actual_pane_state.to_string());
    op.required_action = None;
    op.observed_at_ms = Some(now_ms());
    op.source = source;
    bump(&mut op)?;
    Ok(op)
}

/// Backwards-compatible resume helper for callers that require an observed idle pane.
pub fn resume_operation_dispatch(
    project: &str,
    id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let op = get_operation(project, id)?;
    if op.pane_state.as_deref() != Some("idle") {
        return Err(OperationError::new(
            "OPERATION_NOT_READY_TO_RESUME",
            "startup can resume only after the assigned pane is observed idle",
            true,
        ));
    }
    resume_operation_dispatch_locked(project, id, "idle", op.source)
}
