//! Lifecycle state transitions driven by pane observations and the supervisor.
//! Terminal completion lives in `finish`; cancellation in `cancel`.

use super::model::*;
use super::store::{bump, now_ms};
use super::get_operation;
use serde_json::Value;
use super::store::update_lock;

/// Update authoritative or inferred status. Callers must use Completed only with explicit task evidence.
pub fn mark_operation_state(
    project: &str,
    id: &str,
    status: OperationStatus,
    source: StateSource,
    stage: Option<String>,
    result: Option<String>,
    error: Option<OperationError>,
) -> Result<OperationSnapshot, OperationError> {
    if status == OperationStatus::Completed && result.as_deref().unwrap_or("").trim().is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "completion requires explicit result evidence",
            false,
        ));
    }
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    if op.status == OperationStatus::Cancelling {
        return Err(OperationError::new(
            "CANCELLATION_IN_PROGRESS",
            "operation cancellation has reserved the lifecycle state",
            true,
        ));
    }
    let allowed = op.status == status
        || matches!(
            (op.status, status),
            (
                OperationStatus::Queued,
                OperationStatus::Starting | OperationStatus::Cancelled | OperationStatus::Failed
            ) | (
                OperationStatus::Starting,
                OperationStatus::Running
                    | OperationStatus::WaitingInput
                    | OperationStatus::Failed
                    | OperationStatus::Cancelled
                    | OperationStatus::Cancelling
            ) | (
                OperationStatus::Running,
                OperationStatus::WaitingInput
                    | OperationStatus::Completed
                    | OperationStatus::Failed
                    | OperationStatus::Cancelled
                    | OperationStatus::Cancelling
            ) | (
                OperationStatus::WaitingInput,
                OperationStatus::Running
                    | OperationStatus::Completed
                    | OperationStatus::Failed
                    | OperationStatus::Cancelled
                    | OperationStatus::Cancelling
            ) | (
                OperationStatus::Cancelling,
                OperationStatus::Cancelled
                    | OperationStatus::Failed
                    | OperationStatus::Running
                    | OperationStatus::WaitingInput
            )
        );
    if !allowed {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "operation status transition is not allowed",
            false,
        ));
    }
    op.status = status;
    if status == OperationStatus::Starting && op.started_at_ms.is_none() {
        op.started_at_ms = Some(now_ms());
    }
    if status.terminal() {
        op.finished_at_ms = Some(now_ms());
    }
    op.source = source;
    op.stage = stage;
    op.result = result;
    op.error = error;
    op.progress_pct = None;
    bump(&mut op)?;
    Ok(op)
}

/// Record a named stage on a still-active operation (shows up in `stage_timings`). A no-op once
/// the operation is terminal or already at that stage.
pub fn mark_operation_stage(project: &str, id: &str, stage: &str) {
    let _serial = update_lock();
    let Ok(mut op) = get_operation(project, id) else {
        return;
    };
    if op.status.terminal() || op.stage.as_deref() == Some(stage) {
        return;
    }
    op.stage = Some(stage.to_string());
    let _ = bump(&mut op);
}

/// Record that a warm standby worker (`crate::standby`) was claimed for this turn: sets
/// `worker.standby_used` and adds the `standby_claimed` stage. A no-op once terminal.
pub fn mark_standby_claimed(project: &str, id: &str) {
    let _serial = update_lock();
    let Ok(mut op) = get_operation(project, id) else {
        return;
    };
    if op.status.terminal() {
        return;
    }
    op.worker.standby_used = true;
    op.stage = Some("standby_claimed".to_string());
    let _ = bump(&mut op);
}

pub fn mark_operation_observation(
    project: &str,
    id: &str,
    pane_state: Option<String>,
    required_action: Option<Value>,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    if op.status == OperationStatus::Cancelling {
        op.pane_state = pane_state;
        op.required_action = required_action;
        op.observed_at_ms = Some(now_ms());
        op.source = source;
        bump(&mut op)?;
        return Ok(op);
    }
    let awaiting_readiness = op.stage.as_deref() == Some("waiting_for_pane_readiness");
    // Pane output while the prompt is still being dispatched is the previous turn's TUI repainting
    // (or the typing itself), not this turn running. Leaving `starting` here made the dispatch
    // refuse to type the prompt.
    let dispatching =
        op.status == OperationStatus::Starting && op.stage.as_deref() == Some("dispatching");
    if pane_state.as_deref() == Some("waiting_input") {
        op.status = OperationStatus::WaitingInput;
        if !awaiting_readiness {
            op.stage = Some("waiting_input".into());
        }
    } else if !awaiting_readiness
        && !dispatching
        && matches!(
            op.status,
            OperationStatus::Starting | OperationStatus::WaitingInput
        )
    {
        op.status = OperationStatus::Running;
        op.stage = Some("running".into());
    }
    op.pane_state = pane_state;
    op.required_action = required_action;
    op.observed_at_ms = Some(now_ms());
    op.source = source;
    bump(&mut op)?;
    Ok(op)
}

pub fn mark_operation_waiting_input(
    project: &str,
    id: &str,
    pane_state: Option<String>,
    required_action: Value,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    if op.status == OperationStatus::Cancelling {
        return Ok(op);
    }
    if !matches!(
        op.status,
        OperationStatus::Starting | OperationStatus::Running | OperationStatus::WaitingInput
    ) {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "operation cannot wait for input in its current state",
            false,
        ));
    }
    op.status = OperationStatus::WaitingInput;
    op.stage = Some("waiting_input".into());
    op.pane_state = pane_state;
    op.required_action = Some(required_action);
    op.observed_at_ms = Some(now_ms());
    op.source = source;
    bump(&mut op)?;
    Ok(op)
}

/// Record an idle TUI as settled while leaving task completion explicitly unverified.
pub fn mark_operation_settled_unverified(
    project: &str,
    id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() || op.status == OperationStatus::Cancelling {
        return Ok(op);
    }
    if op.status != OperationStatus::Running {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "only a running operation can settle without completion evidence",
            false,
        ));
    }
    if op.stage.as_deref() == Some("settled_unverified") && op.pane_state.as_deref() == Some("idle")
    {
        return Ok(op);
    }
    op.stage = Some("settled_unverified".into());
    op.pane_state = Some("idle".into());
    op.source = StateSource::Inferred;
    op.observed_at_ms = Some(now_ms());
    bump(&mut op)?;
    Ok(op)
}

/// Record a caveat on an operation (once per code) without touching its status.
pub fn add_warning(
    project: &str,
    id: &str,
    code: &str,
    detail: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.warnings.iter().any(|warning| warning.code == code) {
        return Ok(op);
    }
    op.warnings.push(OperationWarning {
        code: code.into(),
        detail: detail.into(),
    });
    bump(&mut op)?;
    Ok(op)
}
