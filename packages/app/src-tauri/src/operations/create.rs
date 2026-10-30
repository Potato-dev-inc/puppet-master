//! Operation creation: request validation, idempotency-key replay and the queued snapshot.

use super::model::*;
use super::store::{current_runtime_id, find_key, indexed_projects, now_ms, persist, delegate_lock};
use super::{list_operations, run_operation_dispatch};
use uuid::Uuid;

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

/// Create and persist a queued operation before dispatch. A repeated key returns its existing record and never re-runs dispatch.
pub fn delegate_work<F>(
    req: DelegateWorkRequest,
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
    let _serial = delegate_lock();
    if req.task.trim().is_empty() {
        return Err(OperationError::new(
            "INVALID_TASK",
            "task must not be empty",
            false,
        ));
    }
    if !crate::tui::is_agent_backend(&req.agent_type) {
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
        && !crate::agent_runs::read_only_supported_for(
            &req.agent_type,
            req.pane_id.as_deref(),
        )
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
    let project_path = crate::project_path::prepare_project_path_coded(std::path::Path::new(
        &req.project_path,
    ))
    .map_err(crate::project_path::PrepareError::into_operation_error)?
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
        keep_pane_requested: Some(req.keep_pane),
        placement: req.placement.clone(),
        owner_session_id: req.owner_session_id,
        parent_pane_id: req.parent_pane_id.or_else(crate::pty::pane_parent::request_caller),
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
        warnings: Vec::new(),
    };
    persist(&op)?;
    Ok((op, true))
}
