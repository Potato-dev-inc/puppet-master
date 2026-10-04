//! Worker and provider-session metadata recorded on an operation without changing its status.

use super::model::*;
use super::store::bump;
use super::{get_operation, resolve_agent_run};
use serde_json::Value;
use super::store::update_lock;

/// Record what must outlive the pane when a terminal run's pane is auto-closed: the live model
/// (unreadable once the pane is gone) and the fact that the pane was disposed.
pub fn record_terminal_pane_state(
    project_path: &str,
    operation_id: &str,
    live_model: Option<String>,
    pane_disposed: bool,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project_path, operation_id)?;
    let mut changed = false;
    if let Some(model) = live_model.filter(|model| !model.trim().is_empty()) {
        if op.worker.resolved_model.as_deref() != Some(model.as_str()) {
            op.worker.resolved_model = Some(model);
            changed = true;
        }
        // A model read from the live TUI is a confirmation.
        if op.worker.model_unconfirmed {
            op.worker.model_unconfirmed = false;
            changed = true;
        }
    }
    if pane_disposed && !op.worker.pane_disposed {
        op.worker.pane_disposed = true;
        changed = true;
    }
    if changed {
        bump(&mut op)?;
    }
    Ok(op)
}

/// Record the CLI's configured default as `resolved_model` when nothing better is known, flagged
/// unconfirmed. Never overwrites a model that is already set.
pub fn record_unconfirmed_model(
    project_path: &str,
    operation_id: &str,
    model: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project_path, operation_id)?;
    if op.worker.resolved_model.is_none() && !model.trim().is_empty() {
        op.worker.resolved_model = Some(model.to_string());
        op.worker.model_unconfirmed = true;
        bump(&mut op)?;
    }
    Ok(op)
}

pub fn mark_worker_closed(
    project_path: &str,
    agent_run_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let snapshot = resolve_agent_run(project_path, agent_run_id)?;
    let _serial = update_lock();
    let mut op = get_operation(&snapshot.project_path, &snapshot.operation_id)?;
    op.worker.closed = true;
    op.stage = Some("closed".into());
    bump(&mut op)?;
    Ok(op)
}

pub fn rebind_provider_session(
    project: &str,
    id: &str,
    session_id: &str,
    previous_session: Option<&str>,
    session_reset: bool,
    required_action: Option<Value>,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    op.worker.provider_session_id = Some(session_id.to_string());
    op.worker.session_reset = session_reset;
    op.worker.context_continuity = ContextContinuity::ReconstructedSummary;
    if !op.status.terminal() {
        op.stage = Some("session_replaced".into());
        op.message_baseline_ids = Vec::new();
        op.required_action = required_action.or_else(|| {
            Some(serde_json::json!({
                "kind": "session_replaced",
                "previous_session": previous_session,
                "session_id": session_id,
                "pane_id": op.pane_id,
                "detail": "worker process was replaced; rebound to the current native session"
            }))
        });
    }
    bump(&mut op)?;
    Ok(op)
}

pub fn set_provider_session(
    project: &str,
    id: &str,
    session_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    op.worker.provider_session_id = Some(session_id.to_string());
    bump(&mut op)?;
    Ok(op)
}

pub fn clear_session_reset(
    project: &str,
    id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    op.worker.session_reset = false;
    bump(&mut op)?;
    Ok(op)
}
