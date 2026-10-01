//! Durable, revisioned operations exposed to orchestration clients.
//!
//! Pane lifecycle and task lifecycle are separate: an idle or missing pane never
//! implies that the operation's task completed.

mod model;
mod store;
pub use model::*;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};
use store::{
    bump, change, current_runtime_id, find_key, indexed_projects, now_ms, path_for, persist, read,
    root, task_index_path, DELEGATE_LOCK, UPDATE_LOCK,
};
use uuid::Uuid;

/// Statuses that mean the coordinator can act on a turn (answer, follow up, or handle failure).
pub fn completion_wait_until() -> Vec<OperationStatus> {
    vec![
        OperationStatus::Completed,
        OperationStatus::Failed,
        OperationStatus::Cancelled,
        OperationStatus::WaitingInput,
    ]
}

/// Resolve an operation previously created with the same idempotency key.
pub fn find_operation_by_idempotency_key(
    project: Option<&str>,
    key: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    if key.trim().is_empty() {
        return Ok(None);
    }
    if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
        return find_key(project, key);
    }
    for project in indexed_projects()? {
        if let Some(op) = find_key(&project, key)? {
            return Ok(Some(op));
        }
    }
    Ok(None)
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

/// Create and persist a queued operation before dispatch. A repeated key returns its existing record and never re-runs dispatch.
pub fn delegate_work<F>(
    mut req: DelegateWorkRequest,
    dispatch: F,
) -> Result<OperationSnapshot, OperationError>
where
    F: FnOnce(&mut OperationSnapshot) -> Result<(), OperationError>,
{
    let (op, created) = create_operation(req)?;
    if !created {
        return Ok(op);
    }
    run_operation_dispatch(&op.project_path, &op.operation_id, dispatch)
}

/// Persist an idempotent queued operation and return promptly to the caller.
pub fn create_operation(
    mut req: DelegateWorkRequest,
) -> Result<(OperationSnapshot, bool), OperationError> {
    let _serial = DELEGATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    if req.task.trim().is_empty() {
        return Err(OperationError::new(
            "INVALID_TASK",
            "task must not be empty",
            false,
        ));
    }
    if !matches!(
        req.agent_type.as_str(),
        "claude" | "codex" | "cursor" | "cursor_agent" | "opencode" | "opencode_native"
    ) {
        return Err(OperationError::new(
            "INVALID_AGENT_TYPE",
            "agent_type is not supported",
            false,
        ));
    }
    if req.idempotency_key.trim().is_empty() {
        return Err(OperationError::new(
            "INVALID_IDEMPOTENCY_KEY",
            "idempotency_key must not be empty",
            false,
        ));
    }
    if req.read_only
        && !crate::agent_runs::read_only_supported(&req.agent_type, req.pane_id.is_some())
    {
        return Err(OperationError::new(
            "READ_ONLY_UNSUPPORTED",
            crate::agent_runs::read_only_unsupported_message(
                &req.agent_type,
                req.pane_id.is_some(),
            ),
            false,
        ));
    }
    if req.timeout_ms == 0 || req.timeout_ms > 86_400_000 {
        return Err(OperationError::new(
            "INVALID_TIMEOUT",
            "timeout_ms must be between 1 and 86400000",
            false,
        ));
    }
    if req
        .acceptance_criteria
        .as_ref()
        .is_some_and(|items| items.iter().any(|item| item.trim().is_empty()))
    {
        return Err(OperationError::new(
            "INVALID_ACCEPTANCE_CRITERIA",
            "acceptance_criteria entries must be non-empty when provided",
            false,
        ));
    }
    if req.locks.iter().any(|value| {
        let Some((kind, name)) = value.split_once(':') else {
            return true;
        };
        kind.trim().is_empty() || name.trim().is_empty()
    }) {
        return Err(OperationError::new(
            "INVALID_LOCK",
            "locks must use non-empty type:name values",
            false,
        ));
    }
    let project_path = crate::project_path::prepare_project_path(std::path::Path::new(
        &req.project_path,
    ))
    .map_err(|err| OperationError::new("INVALID_PROJECT_PATH", err, false))?
    .to_string_lossy()
    .into_owned();
    req.project_path = project_path;
    let request_fingerprint = serde_json::to_string(&req).map_err(OperationError::io)?;
    if let Some(existing) = find_key(&req.project_path, &req.idempotency_key)? {
        if existing.request_fingerprint != request_fingerprint {
            return Err(OperationError::new(
                "IDEMPOTENCY_KEY_CONFLICT",
                "idempotency_key was already used for a different request",
                false,
            ));
        }
        return Ok((existing, false));
    }
    if let Some(handle) = req.agent_run_id.as_deref() {
        if list_operations(&req.project_path)?.iter().any(|snapshot| {
            snapshot.agent_run_id == handle && snapshot.turn_index == req.turn_index
        }) {
            return Err(OperationError::new(
                "AGENT_TURN_EXISTS",
                "another operation already owns this turn index for the agent handle",
                true,
            ));
        }
    }
    let now = now_ms();
    let operation_id = Uuid::new_v4().to_string();
    let agent_run_id = req
        .agent_run_id
        .clone()
        .unwrap_or_else(|| operation_id.clone());
    let mut worker = req.worker;
    if let Some(policy) = req.context_policy {
        worker.context_policy = policy;
    }
    if worker.name.is_none() {
        worker.name = Some(agent_run_id.clone());
    }
    if worker.workspace.is_none() {
        worker.workspace = Some(req.project_path.clone());
    }
    let op = OperationSnapshot {
        operation_id: operation_id.clone(),
        runtime_id: current_runtime_id().to_string(),
        request_fingerprint,
        project_path: req.project_path,
        task: req.task,
        agent_type: req.agent_type,
        pane_id: req.pane_id,
        pane_created: false,
        idempotency_key: req.idempotency_key,
        acceptance_criteria: req.acceptance_criteria,
        task_id: req.task_id,
        exclusive: req.exclusive,
        locks: req.locks,
        status: OperationStatus::Queued,
        revision: worker.event_cursor.max(1),
        created_at_ms: now,
        updated_at_ms: now,
        observed_at_ms: None,
        pane_state: None,
        required_action: None,
        source: StateSource::Native,
        stage: None,
        progress_pct: None,
        result: None,
        error: None,
        timeout_ms: req.timeout_ms,
        read_only: req.read_only,
        keep_pane: req.keep_pane,
        owner_session_id: req.owner_session_id,
        worker_has_mcp_tools: req.worker_has_mcp_tools,
        agent_run_id,
        turn_index: req.turn_index,
        verified: false,
        started_at_ms: None,
        finished_at_ms: None,
        message_baseline_ids: Vec::new(),
        output_baseline: None,
        result_capture: None,
        acceptance_status: AcceptanceStatus::NotChecked,
        worker,
        checks: req.checks,
        check_results: Vec::new(),
    };
    persist(&op)?;
    Ok((op, true))
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
        let _update = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
        op.started_at_ms = Some(store::now_ms());
        op.stage = Some("dispatching".into());
        bump(&mut op)?;
    }
    let mut op = get_operation(project, id)?;
    let dispatch_result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(&mut op)))
            .unwrap_or_else(|payload| {
                Err(OperationError::new(
                    "DISPATCH_PANICKED",
                    panic_payload_message(payload),
                    true,
                ))
            });
    match dispatch_result {
        Ok(()) => {
            let dispatched = op;
            let _update = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
            let _update = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

pub fn get_operation(
    project_path: &str,
    operation_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    read(&path_for(project_path, operation_id)?)
}

/// Read all durable operation records for a project, ordered by creation time.
pub fn list_operations(project_path: &str) -> Result<Vec<OperationSnapshot>, OperationError> {
    let dir = root(project_path)?;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(OperationError::io(error)),
    };
    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            match read(&entry.path()) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(error) if error.code == "OPERATION_NOT_FOUND" => continue,
                Err(error) => return Err(error),
            }
        }
    }
    snapshots.sort_by_key(|snapshot| (snapshot.created_at_ms, snapshot.turn_index));
    Ok(snapshots)
}

/// Operations for `project_path` plus every other indexed project. Used when a handle
/// or listing should not be confined to the currently selected directory.
pub fn list_indexed_operations(project_path: &str) -> Result<Vec<OperationSnapshot>, OperationError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut snapshots = Vec::new();
    let mut projects = vec![project_path.to_string()];
    projects.extend(indexed_projects().unwrap_or_default());
    for project in projects {
        if !seen.insert(project.clone()) {
            continue;
        }
        snapshots.extend(list_operations(&project)?);
    }
    snapshots.sort_by_key(|snapshot| (snapshot.created_at_ms, snapshot.turn_index));
    Ok(snapshots)
}

/// Resolve the newest turn of a stable agent-run handle.
pub fn latest_agent_run(
    project_path: &str,
    agent_run_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    latest_in_project(project_path, agent_run_id)?
        .ok_or_else(|| crate::mcp_sessions::agent_not_found(agent_run_id))
}

/// Prefer `project_path`, then any indexed project that owns this handle.
pub fn resolve_agent_run(
    project_path: &str,
    agent_run_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    if let Some(snapshot) = latest_in_project(project_path, agent_run_id)? {
        return Ok(snapshot);
    }
    for project in indexed_projects()? {
        if project == project_path {
            continue;
        }
        if let Some(snapshot) = latest_in_project(&project, agent_run_id)? {
            return Ok(snapshot);
        }
    }
    Err(crate::mcp_sessions::agent_not_found(agent_run_id))
}

pub fn turns_for_handle(
    project_path: &str,
    agent_run_id: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    let mut turns: Vec<_> = list_operations(project_path)?
        .into_iter()
        .filter(|snapshot| {
            snapshot.agent_run_id == agent_run_id
                || (snapshot.agent_run_id.is_empty() && snapshot.operation_id == agent_run_id)
        })
        .collect();
    turns.sort_by_key(|snapshot| (snapshot.turn_index, snapshot.created_at_ms));
    Ok(turns)
}

fn latest_in_project(
    project_path: &str,
    agent_run_id: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    Ok(turns_for_handle(project_path, agent_run_id)?
        .into_iter()
        .max_by_key(|snapshot| (snapshot.turn_index, snapshot.created_at_ms)))
}

pub fn mark_worker_closed(
    project_path: &str,
    agent_run_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let snapshot = resolve_agent_run(project_path, agent_run_id)?;
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut op = get_operation(&snapshot.project_path, &snapshot.operation_id)?;
    op.worker.closed = true;
    op.stage = Some("closed".into());
    bump(&mut op)?;
    Ok(op)
}

pub fn save_operation_snapshot(
    snapshot: &OperationSnapshot,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

/// Find the active operation bound to a task id in the supplied project.
pub fn operation_for_task(
    project: &str,
    task_id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let dir = root(project)?;
    let entries = fs::read_dir(dir).map_err(OperationError::io)?;
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = match read(&entry.path()) {
                Ok(operation) => operation,
                Err(error) if error.code == "OPERATION_NOT_FOUND" => continue,
                Err(error) => return Err(error),
            };
            if op.task_id.as_deref() == Some(task_id) {
                return Ok(op);
            }
        }
    }
    Err(OperationError::new(
        "OPERATION_NOT_FOUND",
        "no operation is linked to this task",
        false,
    ))
}
pub fn operation_for_task_any_project(task_id: &str) -> Result<OperationSnapshot, OperationError> {
    let index = task_index_path()?;
    let bytes = fs::read(index).map_err(OperationError::io)?;
    let entries: HashMap<String, (String, String)> =
        serde_json::from_slice(&bytes).map_err(OperationError::io)?;
    let (project, operation_id) = entries.get(task_id).ok_or_else(|| {
        OperationError::new(
            "OPERATION_NOT_FOUND",
            "no operation is linked to this task",
            false,
        )
    })?;
    get_operation(project, operation_id)
}

pub fn mark_task_completed_any_project(
    task_id: &str,
    evidence: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    let operation = operation_for_task_any_project(task_id)?;
    mark_task_completed(&operation.project_path, task_id, evidence)
}

/// Mark work left active by a previous app process as interrupted. It cannot be inferred as successful.
pub fn recover_interrupted_operations(
    project: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(OperationError::io(e)),
    };
    let mut recovered = Vec::new();
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = match read(&entry.path()) {
                Ok(operation) => operation,
                Err(error) if error.code == "OPERATION_NOT_FOUND" => continue,
                Err(error) => return Err(error),
            };
            if op.runtime_id != current_runtime_id()
                && matches!(
                    op.status,
                    OperationStatus::Queued
                        | OperationStatus::Starting
                        | OperationStatus::Running
                        | OperationStatus::WaitingInput
                )
            {
                let mut error = OperationError::new(
                    "OPERATION_INTERRUPTED",
                    "app restarted while operation was active",
                    true,
                );
                error.context = serde_json::json!({ "operation_id": op.operation_id });
                let result = mark_operation_state(
                    project,
                    &op.operation_id,
                    OperationStatus::Failed,
                    StateSource::Native,
                    Some("interrupted".into()),
                    None,
                    Some(error),
                );
                match result {
                    Ok(snapshot) => recovered.push(snapshot),
                    Err(error) if error.code == "OPERATION_NOT_FOUND" => continue,
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(recovered)
}

pub fn recover_all_interrupted_operations() -> Result<Vec<OperationSnapshot>, OperationError> {
    let mut recovered = Vec::new();
    for project in indexed_projects()? {
        recovered.extend(recover_interrupted_operations(&project)?);
    }
    Ok(recovered)
}

/// Return the active operation currently assigned to a pane, if any.
pub fn active_operation_for_pane(
    project: &str,
    pane_id: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OperationError::io(e)),
    };
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = read(&entry.path())?;
            if op.pane_id.as_deref() == Some(pane_id) && !op.status.terminal() {
                return Ok(Some(op));
            }
        }
    }
    Ok(None)
}

/// Active occupant of `pane_id` other than `operation_id`, if any.
///
/// `create_operation` stores a requested `pane_id` before dispatch, so the
/// current operation would otherwise look busy to itself. Scan every record:
/// first-match-then-skip-self misses a real occupant listed after `self`.
pub fn conflicting_pane_operation(
    project: &str,
    pane_id: &str,
    operation_id: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OperationError::io(e)),
    };
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = read(&entry.path())?;
            if op.pane_id.as_deref() == Some(pane_id)
                && !op.status.terminal()
                && op.operation_id != operation_id
            {
                return Ok(Some(op));
            }
        }
    }
    Ok(None)
}

/// Atomically reserve a pane for an operation before readiness checks or input writes.
pub fn reserve_pane(
    project: &str,
    operation_id: &str,
    pane_id: &str,
    pane_created: bool,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
        let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

pub fn mark_operation_observation(
    project: &str,
    id: &str,
    pane_state: Option<String>,
    required_action: Option<Value>,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
    if pane_state.as_deref() == Some("waiting_input") {
        op.status = OperationStatus::WaitingInput;
        if !awaiting_readiness {
            op.stage = Some("waiting_input".into());
        }
    } else if !awaiting_readiness
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

pub fn set_message_baseline(
    project: &str,
    id: &str,
    message_ids: Vec<String>,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    op.message_baseline_ids = message_ids;
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut op = get_operation(project, id)?;
    op.worker.provider_session_id = Some(session_id.to_string());
    bump(&mut op)?;
    Ok(op)
}

pub fn clear_session_reset(
    project: &str,
    id: &str,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut op = get_operation(project, id)?;
    op.worker.session_reset = false;
    bump(&mut op)?;
    Ok(op)
}

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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut op = get_operation(project, id)?;
    if op.status != OperationStatus::Starting {
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    resume_operation_dispatch_locked(project, id, actual_pane_state, source)
}

fn resume_operation_dispatch_locked(
    project: &str,
    id: &str,
    actual_pane_state: &str,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let mut op = get_operation(project, id)?;
    if op.status != OperationStatus::WaitingInput
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

pub fn mark_operation_waiting_input(
    project: &str,
    id: &str,
    pane_state: Option<String>,
    required_action: Value,
    source: StateSource,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

pub fn cancel_operation_with<F>(
    project: &str,
    id: &str,
    worker_cancel: F,
) -> Result<OperationSnapshot, OperationError>
where
    F: FnOnce(&OperationSnapshot) -> Result<(), OperationError>,
{
    let (snapshot, previous) = {
        let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

/// Complete operations tied to an explicitly completed task with evidence.
pub fn mark_task_completed(
    project: &str,
    task_id: &str,
    evidence: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    if evidence.trim().is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "evidence must not be empty",
            false,
        ));
    }
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(OperationError::io(e)),
    };
    let mut completed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = read(&entry.path())?;
            if op.task_id.as_deref() == Some(task_id) && !op.status.terminal() {
                completed.push(mark_operation_completed(
                    project,
                    &op.operation_id,
                    evidence,
                    true,
                )?);
            }
        }
    }
    Ok(completed)
}

/// Finish a run with explicit completion evidence. `verified` is set only by a caller that
/// obtained native structured evidence (for example, a successful CLI result event).
pub fn mark_operation_completed(
    project: &str,
    id: &str,
    evidence: &str,
    verified: bool,
) -> Result<OperationSnapshot, OperationError> {
    let capture = if verified {
        ResultCapture::Authoritative
    } else {
        ResultCapture::Inferred
    };
    mark_operation_finished(
        project,
        id,
        Some(evidence.to_string()),
        capture,
        verified,
    )
}

/// Finish a turn. Missing capture is allowed so an idle worker is not left "running" until timeout.
pub fn mark_operation_finished(
    project: &str,
    id: &str,
    result: Option<String>,
    capture: ResultCapture,
    verified: bool,
) -> Result<OperationSnapshot, OperationError> {
    let evidence = result.as_deref().unwrap_or("").trim();
    if capture != ResultCapture::Missing && evidence.is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "completion requires explicit result evidence",
            false,
        ));
    }
    if verified && evidence.is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "verified completion requires explicit result evidence",
            false,
        ));
    }
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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
    if !matches!(
        op.status,
        OperationStatus::Running | OperationStatus::WaitingInput | OperationStatus::Starting
    ) {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "operation cannot complete in its current state",
            false,
        ));
    }
    op.status = OperationStatus::Completed;
    op.result = result.filter(|value| !value.trim().is_empty());
    op.result_capture = Some(capture);
    op.verified = verified;
    op.finished_at_ms = Some(now_ms());
    op.stage = Some("completed".into());
    op.source = match capture {
        ResultCapture::Authoritative => StateSource::Native,
        ResultCapture::Inferred | ResultCapture::Missing => StateSource::Inferred,
    };
    op.progress_pct = None;
    op.required_action = None;
    evaluate_acceptance_criteria(&mut op);
    if !op.checks.is_empty() {
        let results = crate::agent_runs::checks::evaluate(
            std::path::Path::new(&op.project_path),
            &op.checks,
        );
        let failure = crate::agent_runs::checks::failure_summary(&results);
        op.check_results = results;
        if let Some(message) = failure {
            op.status = OperationStatus::Failed;
            op.verified = false;
            op.stage = Some("failed".into());
            op.acceptance_status = AcceptanceStatus::Failed;
            op.error = Some(OperationError::new("ACCEPTANCE_FAILED", message, false));
        } else {
            // Verified `checks` are the authoritative acceptance signal.
            op.acceptance_status = AcceptanceStatus::Passed;
        }
    }
    bump(&mut op)?;
    Ok(op)
}

/// Finish a turn as interrupted without treating it as a successful completion.
pub fn mark_turn_interrupted(
    project: &str,
    id: &str,
    partial: Option<String>,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    op.status = OperationStatus::Cancelled;
    op.stage = Some("interrupted".into());
    op.result = partial.filter(|value| !value.trim().is_empty()).or(op.result);
    if op.result.is_none() {
        op.result_capture = Some(ResultCapture::Missing);
    } else if op.result_capture.is_none() {
        op.result_capture = Some(ResultCapture::Inferred);
    }
    op.keep_pane = true;
    op.finished_at_ms = Some(now_ms());
    op.required_action = None;
    bump(&mut op)?;
    Ok(op)
}

pub fn cancel_operation(project: &str, id: &str) -> Result<OperationSnapshot, OperationError> {
    let _serial = UPDATE_LOCK.get_or_init(|| Mutex::new(())).lock();
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

fn normalize_acceptance_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn criterion_matches_result_text(criterion: &str, result: &str) -> bool {
    let needle = normalize_acceptance_text(criterion);
    if needle.is_empty() {
        return false;
    }
    let haystack = normalize_acceptance_text(result);
    if haystack.contains(&needle) {
        return true;
    }
    // ponytail: loose token gate for paraphrased replies; upgrade with structured checks (file_exists:).
    let tokens: Vec<&str> = needle
        .split_whitespace()
        .filter(|word| word.len() >= 4)
        .collect();
    if tokens.len() >= 2 {
        tokens.iter().all(|word| haystack.contains(word))
    } else {
        false
    }
}

fn evaluate_acceptance_criteria(op: &mut OperationSnapshot) {
    let Some(criteria) = op.acceptance_criteria.as_ref() else {
        op.acceptance_status = AcceptanceStatus::NotChecked;
        return;
    };
    let meaningful: Vec<&str> = criteria
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect();
    if meaningful.is_empty() {
        op.acceptance_status = AcceptanceStatus::NotChecked;
        return;
    }
    let result = op.result.as_deref().unwrap_or("");
    let workspace = std::path::Path::new(&op.project_path);
    let mut hard_fail = false;
    let mut all_verified = true;

    for criterion in &meaningful {
        if let Some(relative) = criterion.strip_prefix("file_exists:") {
            let relative = relative.trim();
            if relative.is_empty() {
                all_verified = false;
                continue;
            }
            let target = workspace.join(relative);
            if !target.exists() {
                hard_fail = true;
            }
            continue;
        }
        if criterion_matches_result_text(criterion, result) {
            continue;
        }
        all_verified = false;
    }

    op.acceptance_status = if hard_fail {
        AcceptanceStatus::Failed
    } else if all_verified {
        AcceptanceStatus::Passed
    } else {
        AcceptanceStatus::NotChecked
    };
}

/// Context policy actually applied by `delegate_work`. It never builds a context packet, so
/// an unspecified policy is `fresh` for a first turn and `resume` when continuing a worker
/// that already has earlier turns; `packet` is reported only when explicitly requested.
pub fn applied_delegate_context_policy(
    requested: Option<ContextPolicy>,
    turn_index: u32,
    prior_turns_for_handle: usize,
) -> ContextPolicy {
    requested.unwrap_or(if turn_index > 0 || prior_turns_for_handle > 0 {
        ContextPolicy::Resume
    } else {
        ContextPolicy::Fresh
    })
}

/// Trim TUI chrome from dispatch baselines before exposing operations over MCP/HTTP.
pub fn compact_output_baseline(raw: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let trimmed: String = line
            .chars()
            .filter(|ch| {
                !matches!(
                    ch,
                    '█' | '▀' | '▐' | '▔' | '░' | '▁' | '▂' | '▃' | '▄' | '▅' | '▆' | '▇' | '▉' | '▊' | '▋' | '▌'
                        | '▍' | '▎' | '▏' | '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤'
                        | '┬' | '┴' | '┼'
                )
            })
            .collect::<String>()
            .trim()
            .to_string();
        if trimmed.len() >= 4 && trimmed.chars().any(|ch| ch.is_alphanumeric()) {
            lines.push(trimmed);
        }
    }
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(8);
    let mut tail = lines[start..].join("\n");
    if tail.len() > 480 {
        let mut cut = tail.len() - 479;
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail = format!("…{}", &tail[cut..]);
    }
    Some(tail)
}

pub fn snapshot_for_api(mut snapshot: OperationSnapshot) -> OperationSnapshot {
    snapshot.output_baseline = None;
    snapshot
}

#[cfg(test)]
#[path = "operations/tests.rs"]
mod tests;
