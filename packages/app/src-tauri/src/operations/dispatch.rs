//! Dispatch of a queued operation: claiming it, the in-flight guard, pane reservation and
//! checkpoints taken while the prompt is being delivered.

use super::model::*;
use super::store::{bump, now_ms};
use super::{conflicting_pane_operation, get_operation};
use parking_lot::Mutex;
use serde_json::Value;
use super::store::update_lock;

static DISPATCHING: std::sync::OnceLock<Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

/// Marks an operation as having its dispatch closure running in this process.
struct DispatchGuard(String);

impl DispatchGuard {
    fn enter(id: &str) -> Self {
        DISPATCHING
            .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
            .lock()
            .insert(id.to_string());
        Self(id.to_string())
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        if let Some(set) = DISPATCHING.get() {
            set.lock().remove(&self.0);
        }
    }
}

/// Whether the prompt of this operation is being typed into its pane right now.
pub fn dispatch_in_flight(id: &str) -> bool {
    DISPATCHING.get().is_some_and(|set| set.lock().contains(id))
}

/// Whether this turn's dispatch ran to the point of submitting the prompt. A turn whose stage log
/// shows it entered `dispatching` but never reached `dispatched_waiting_for_task_completion` had
/// its dispatch abandoned (the operation left `starting` first): nothing was typed. A snapshot
/// with no `dispatching` mark at all is not judged.
pub fn prompt_dispatched(op: &OperationSnapshot) -> bool {
    let log = &op.worker.stage_log;
    match log.iter().rposition(|mark| mark.stage == "dispatching") {
        None => true,
        Some(at) => log[at..]
            .iter()
            .any(|mark| mark.stage == "dispatched_waiting_for_task_completion"),
    }
}

/// Claim a queued operation before invoking dispatch. Cancellation can win while queued;
/// once claimed, cancellation is rejected until the worker control path supports it.
pub fn run_operation_dispatch<F>(
    project: &str,
    id: &str,
    dispatch: F,
) -> Result<OperationSnapshot, OperationError>
where
    F: FnOnce(&mut OperationSnapshot) -> Result<(), OperationError>,
{
    {
        let _update = update_lock();
        let mut op = get_operation(project, id)?;
        if op.status == OperationStatus::Cancelled {
            return Ok(op);
        }
        if op.status != OperationStatus::Queued {
            return Err(OperationError::new(
                "OPERATION_ALREADY_DISPATCHED",
                "operation is no longer queued",
                false,
            ));
        }
        op.status = OperationStatus::Starting;
        op.started_at_ms = Some(now_ms());
        op.stage = Some("dispatching".into());
        bump(&mut op)?;
    }
    let mut op = get_operation(project, id)?;
    let in_flight = DispatchGuard::enter(id);
    let dispatch_result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(&mut op)))
            .unwrap_or_else(|payload| {
                Err(OperationError::new(
                    "DISPATCH_PANICKED",
                    panic_payload_message(payload),
                    true,
                ))
            });
    drop(in_flight);
    match dispatch_result {
        Ok(()) => {
            let dispatched = op;
            let _update = update_lock();
            op = get_operation(project, id)?;
            if op.status != OperationStatus::Starting {
                return Ok(op);
            }
            op.pane_id = dispatched.pane_id;
            op.pane_created = dispatched.pane_created;
            op.task_id = dispatched.task_id;
            op.exclusive = dispatched.exclusive;
            op.locks = dispatched.locks;
            op.worker_has_mcp_tools = dispatched.worker_has_mcp_tools;
            op.agent_run_id = dispatched.agent_run_id;
            op.turn_index = dispatched.turn_index;
            op.verified = dispatched.verified;
            op.pane_state = dispatched.pane_state;
            op.required_action = dispatched.required_action;
            op.observed_at_ms = dispatched.observed_at_ms;
            op.source = dispatched.source;
            op.status = if dispatched.status == OperationStatus::WaitingInput {
                OperationStatus::WaitingInput
            } else {
                OperationStatus::Running
            };
            op.stage = dispatched.stage.or_else(|| Some("running".into()));
            op.progress_pct = None;
            bump(&mut op)?;
            Ok(op)
        }
        Err(mut err) => {
            let dispatched = op;
            let _update = update_lock();
            op = get_operation(project, id)?;
            if op.status != OperationStatus::Starting {
                return Ok(op);
            }
            op.pane_id = dispatched.pane_id;
            op.pane_created = dispatched.pane_created;
            op.task_id = dispatched.task_id;
            op.exclusive = dispatched.exclusive;
            op.locks = dispatched.locks;
            op.worker_has_mcp_tools = dispatched.worker_has_mcp_tools;
            op.agent_run_id = dispatched.agent_run_id;
            op.turn_index = dispatched.turn_index;
            op.verified = dispatched.verified;
            op.pane_state = dispatched.pane_state;
            op.required_action = dispatched.required_action;
            op.status = OperationStatus::Failed;
            op.finished_at_ms = Some(now_ms());
            let context = err.context.as_object_mut();
            if let Some(context) = context {
                context.insert(
                    "operation_id".into(),
                    Value::String(op.operation_id.clone()),
                );
            } else {
                err.context = serde_json::json!({ "operation_id": op.operation_id });
            }
            op.error = Some(err.clone());
            op.stage = Some("failed".into());
            bump(&mut op)?;
            Err(err)
        }
    }
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .copied()
        .map(str::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "dispatch panicked".into())
}

/// Atomically reserve a pane for an operation before readiness checks or input writes.
pub fn reserve_pane(
    project: &str,
    operation_id: &str,
    pane_id: &str,
    pane_created: bool,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    if let Some(active) = conflicting_pane_operation(project, pane_id, operation_id)? {
        let mut error = OperationError::new(
            "PANE_BUSY",
            "pane is already assigned to another active operation",
            true,
        );
        error.context =
            serde_json::json!({"pane_id": pane_id, "operation_id": active.operation_id});
        return Err(error);
    }
    let mut op = get_operation(project, operation_id)?;
    if op.status != OperationStatus::Starting {
        return Err(OperationError::new(
            "OPERATION_NOT_DISPATCHING",
            "operation is no longer dispatching",
            false,
        ));
    }
    op.pane_id = Some(pane_id.to_string());
    op.pane_created = pane_created;
    bump(&mut op)?;
    Ok(op)
}

/// Checkpoint task, pane, and lock assignments during dispatch without changing lifecycle state.
pub fn checkpoint_operation(
    snapshot: &OperationSnapshot,
) -> Result<OperationSnapshot, OperationError> {
    save_operation_snapshot(snapshot)
}

/// Serialize the final dispatch side effect against cancellation. If cancellation has reserved
/// the operation, input is never written to the pane.
///
/// ponytail: do not hold UPDATE_LOCK across `dispatch_input`. Rate-limit recovery restarts the
/// native pane and rebinds the provider session on this same mutex; keeping the lock across
/// `prompt_async` deadlocks the process.
pub fn dispatch_input_if_active<F>(
    project: &str,
    id: &str,
    dispatch_input: F,
) -> Result<OperationSnapshot, OperationError>
where
    F: FnOnce(&OperationSnapshot) -> Result<(), OperationError>,
{
    let op = {
        let _serial = update_lock();
        let op = get_operation(project, id)?;
        if op.status != OperationStatus::Starting {
            return Err(OperationError::new(
                "DISPATCH_CANCELLED",
                "operation is no longer in dispatching state",
                false,
            ));
        }
        op
    };
    dispatch_input(&op)?;
    Ok(op)
}

pub fn save_operation_snapshot(
    snapshot: &OperationSnapshot,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut current = get_operation(&snapshot.project_path, &snapshot.operation_id)?;
    if current.status != OperationStatus::Starting {
        return Err(OperationError::new(
            "OPERATION_NOT_DISPATCHING",
            "operation is no longer dispatching",
            false,
        ));
    }
    current.pane_id = snapshot.pane_id.clone();
    current.pane_created = snapshot.pane_created;
    current.task_id = snapshot.task_id.clone();
    current.exclusive = snapshot.exclusive;
    current.locks = snapshot.locks.clone();
    current.pane_state = snapshot.pane_state.clone();
    current.required_action = snapshot.required_action.clone();
    current.stage = snapshot.stage.clone();
    current.message_baseline_ids = snapshot.message_baseline_ids.clone();
    current.output_baseline = snapshot.output_baseline.clone();
    current.started_at_ms = snapshot.started_at_ms;
    current.finished_at_ms = snapshot.finished_at_ms;
    current.worker_has_mcp_tools = snapshot.worker_has_mcp_tools;
    current.agent_run_id = snapshot.agent_run_id.clone();
    current.turn_index = snapshot.turn_index;
    current.verified = snapshot.verified;
    current.worker = snapshot.worker.clone();
    bump(&mut current)?;
    Ok(current)
}
