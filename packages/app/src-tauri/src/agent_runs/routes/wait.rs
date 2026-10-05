//! `POST /agents/wait` and `GET /agents/<handle>/transcript`.

use super::{
    handles::resolve_run_handle,
    wire::{query_value, WaitAgentsRequest},
};
use crate::agent_runs::runtime::{self};
use crate::operations::{self, OperationError, OperationWaitCursor};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub(super) fn wait_agents(
    project: &str,
    request: WaitAgentsRequest,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Result<Value, OperationError> {
    if request.handles.is_empty() {
        return Err(OperationError::new(
            "INVALID_AGENT_HANDLES",
            "handles must not be empty",
            false,
        ));
    }
    let mut cursors = Vec::new();
    let mut caller_cursors = Vec::new();
    let mut latest_by_id = HashMap::new();
    for raw in &request.handles {
        let handle = resolve_run_handle(project, raw, registry)?;
        let snapshot = runtime::current_agent_for_wait(project, &handle, session_id)?;
        let explicit = request
            .after_revisions
            .get(raw.as_str())
            .or_else(|| request.after_revisions.get(&handle))
            .copied()
            .or(request.after_cursor);
        caller_cursors.push(explicit);
        cursors.push(OperationWaitCursor {
            project_path: snapshot.project_path.clone(),
            operation_id: snapshot.operation_id.clone(),
            after_revision: explicit.unwrap_or(snapshot.worker.event_cursor.max(snapshot.revision)),
        });
        latest_by_id.insert(snapshot.operation_id.clone(), handle);
    }
    let wait_all = request.mode.as_deref() == Some("all");
    if request
        .mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "all" | "any"))
    {
        return Err(OperationError::new(
            "INVALID_WAIT_MODE",
            "mode must be 'any' or 'all'",
            false,
        ));
    }
    if request.wait_ms > 300_000 {
        return Err(OperationError::new(
            "INVALID_TIMEOUT",
            "wait_ms must be between 0 and 300000",
            false,
        ));
    }
    let wait_started = std::time::Instant::now();
    let result = operations::wait_for_operations(
        &cursors,
        request.until,
        request.wait_ms,
        wait_all,
        request.wake_on_progress,
    )?;
    let waited_ms = wait_started.elapsed().as_millis() as u64;
    let agents = result
        .snapshots
        .into_iter()
        .zip(caller_cursors.iter())
        .zip(result.wake_reasons.iter())
        .map(|((snapshot, caller_cursor), wake_reason)| {
            let overlay_snapshot = snapshot.clone();
            let view =
                runtime::agent_view_after_wait(snapshot, *caller_cursor, Some(wake_reason.clone()));
            let mut value = serde_json::to_value(view).unwrap_or(Value::Null);
            crate::agent_runs::persist::overlay_live_model(
                &mut value,
                &overlay_snapshot,
                Some(registry),
            );
            value
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"agents": agents, "reason": result.reason, "mode": if wait_all {"all"} else {"any"}, "waited_ms": waited_ms, "requested_wait_ms": request.wait_ms}),
    )
}

pub(super) fn transcript(
    project: &str,
    handle: &str,
    query: &str,
    session_id: Option<&str>,
) -> Result<Value, OperationError> {
    let snapshot = runtime::current_agent_for_read(project, handle, session_id)?;
    let project = snapshot.project_path.as_str();
    let after = query_value(query, "after")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let page = if let Some(operation_id) = query_value(query, "operation_id") {
        let selected = operations::get_operation(project, &operation_id)?;
        if selected.agent_run_id != handle && selected.operation_id != handle {
            return Err(OperationError::new(
                "AGENT_NOT_FOUND",
                "operation does not belong to this agent handle",
                false,
            ));
        }
        runtime::transcript_page(project, &operation_id, after)?
    } else {
        runtime::transcript_for_handle(project, handle, after)?
    };
    serde_json::to_value(page)
        .map_err(|error| OperationError::new("SERIALIZATION_ERROR", error.to_string(), false))
}
