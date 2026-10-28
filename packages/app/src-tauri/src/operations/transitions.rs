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
    if pane_state.as_deref() == Some("waiting_input") && dispatching {
        // A dialog drew while the task is being delivered (live r13: Codex's folder-trust question
        // 1.7 s after its composer). The dispatch thread owns the run until the task is in: its
        // delivery refuses to type over the dialog and parks the run on it, then delivers once it is
        // answered. Flipping the run to waiting_input here made the dispatch abandon its delivery
        // and the supervisor deliver the task a second time after the answer.
    } else if pane_state.as_deref() == Some("waiting_input") {
        op.status = OperationStatus::WaitingInput;
        if !awaiting_readiness {
            // A usage limit keeps its own stage (`handoff`): it is not a prompt anyone can answer.
            let at_limit = required_action
                .as_ref()
                .and_then(|action| action.get("kind"))
                .and_then(Value::as_str)
                == Some(USAGE_LIMIT_KIND);
            op.stage = Some(if at_limit { USAGE_LIMIT_KIND } else { "waiting_input" }.into());
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
    // A run parked on a screen that is not ready keeps its explanation and screen
    // (`screen_not_ready`) through status changes: idle/running bring no action and an
    // unrecognised waiting_input only the id-less manual one, which would leave the card with
    // nothing to read. A dialog the profile recognises replaces it (that is news).
    let parked_reason = awaiting_readiness
        && op.required_action.as_ref().and_then(|a| a.get("kind")).and_then(Value::as_str) == Some("screen_not_ready");
    let recognised = required_action
        .as_ref()
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|kind| !matches!(kind, "manual_input_required" | "screen_not_ready"));
    if !parked_reason || recognised {
        op.required_action = required_action;
    }
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
    // A run parked on a startup dialog keeps its readiness stage: the dispatch thread owns the
    // run until the dialog is answered, and a supervisor that also sees the dialog must not take
    // the stage away (a later pane observation would then read the run as `running`).
    if op.stage.as_deref() != Some("waiting_for_pane_readiness") {
        op.stage = Some("waiting_input".into());
    }
    op.pane_state = pane_state;
    op.required_action = Some(required_action);
    op.observed_at_ms = Some(now_ms());
    op.source = source;
    bump(&mut op)?;
    Ok(op)
}

/// The `required_action.kind` and stage of a run whose agent ran out of usage (`crate::handoff`).
pub const USAGE_LIMIT_KIND: &str = "usage_limit";
/// The error code of a run a person ended at a usage limit ("Stop here").
pub const USAGE_LIMIT_STOPPED: &str = "USAGE_LIMIT_STOPPED";

/// Park a run because its agent hit a usage limit: `waiting_input`, stage `usage_limit`, with the
/// limit (window, reset time) as the required action. Nothing here answers anything: the action has
/// no options and the answer routes refuse it.
pub fn mark_operation_usage_limit(
    project: &str,
    id: &str,
    action: Value,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() || op.status == OperationStatus::Cancelling {
        return Ok(op);
    }
    if !matches!(
        op.status,
        OperationStatus::Starting | OperationStatus::Running | OperationStatus::WaitingInput
    ) {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "operation cannot stop at a usage limit in its current state",
            false,
        ));
    }
    op.status = OperationStatus::WaitingInput;
    if op.stage.as_deref() != Some("waiting_for_pane_readiness") {
        op.stage = Some(USAGE_LIMIT_KIND.into());
    }
    op.pane_state = Some("waiting_input".into());
    op.required_action = Some(action);
    op.observed_at_ms = Some(now_ms());
    op.source = StateSource::Inferred;
    bump(&mut op)?;
    Ok(op)
}

/// A person chose "Stop here" at a usage limit: the run ends as cancelled with the limit on its
/// error, and its pane is KEPT (the agent's own wait, its transcript and its screen stay as they
/// are). Nothing is pressed in the pane.
pub fn mark_operation_stopped_at_limit(
    project: &str,
    id: &str,
    context: Value,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() || op.status == OperationStatus::Cancelling {
        return Ok(op);
    }
    op.status = OperationStatus::Cancelled;
    op.stage = Some("stopped_at_usage_limit".into());
    let mut error = OperationError::new(
        USAGE_LIMIT_STOPPED,
        "stopped at a usage limit: the agent ran out of usage and the person chose to stop here",
        true,
    );
    error.context = context;
    op.error = Some(error);
    op.keep_pane = true;
    op.finished_at_ms = Some(now_ms());
    op.required_action = None;
    bump(&mut op)?;
    Ok(op)
}

/// The error code of a run whose work moved to another agent at a usage limit (Keep going,
/// `crate::handoff::keep_going`). The old run is cancelled, its pane is kept, and a follow-up on its
/// handle is refused with this code.
pub const HANDED_OFF: &str = "HANDED_OFF";

/// Hand a run over: any non-terminal run ends as `cancelled` with error `HANDED_OFF` (not
/// recoverable), stage `handed_off`, and its pane KEPT. Nothing is pressed in the pane. The
/// terminal state is written here, before the old agent is stopped, so the stop cannot race into
/// `AGENT_EXITED`. A run that is already terminal is left as it is.
pub fn mark_operation_handed_off(
    project: &str,
    id: &str,
    context: Value,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    op.status = OperationStatus::Cancelled;
    op.stage = Some("handed_off".into());
    let mut error = OperationError::new(
        HANDED_OFF,
        "this task moved to another agent at a usage limit",
        false,
    );
    error.context = context;
    op.error = Some(error);
    op.keep_pane = true;
    op.required_action = None;
    op.finished_at_ms = Some(now_ms());
    bump(&mut op)?;
    Ok(op)
}

/// Add where the work went (`{handle, agent_type}`) to a handed-off run's error context, as
/// `context.continued_by`. A no-op for any run that did not end as `HANDED_OFF`.
pub fn set_handed_off_to(project: &str, id: &str, continued_by: Value) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    let Some(error) = op.error.as_mut().filter(|error| error.code == HANDED_OFF) else {
        return Ok(op);
    };
    if !error.context.is_object() {
        error.context = Value::Object(serde_json::Map::new());
    }
    if let Some(context) = error.context.as_object_mut() {
        context.insert("continued_by".into(), continued_by);
    }
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

/// Turn an active visible operation into a queued headless one (fallback `from: visible`): the
/// pane binding and the failed attempt's state are dropped, the same operation and handle stay.
/// Refused (None) for a terminal or cancelling operation and when a fallback already happened.
pub fn convert_to_headless_fallback(
    project: &str,
    id: &str,
    reason: &str,
) -> Option<OperationSnapshot> {
    let _serial = update_lock();
    let mut op = get_operation(project, id).ok()?;
    let convertible = matches!(
        op.status,
        OperationStatus::Starting | OperationStatus::Running | OperationStatus::WaitingInput
    );
    if !convertible || op.worker.fallback.is_some() {
        return None;
    }
    op.worker.fallback = Some(RunFallback::to_headless(reason));
    op.status = OperationStatus::Queued;
    op.pane_id = None;
    op.pane_created = false;
    op.pane_state = None;
    op.required_action = None;
    op.output_baseline = None;
    op.message_baseline_ids = Vec::new();
    op.started_at_ms = None;
    op.error = None;
    op.progress_pct = None;
    op.stage = Some("fallback_headless".into());
    bump(&mut op).ok()?;
    Some(op)
}
