//! Mapping what a caller typed (handle, name, pane id) to a run handle, and pane -> run lookups.

use crate::operations::{self, OperationError};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

/// The run (project, handle) bound to `pane_id`, searching `projects` in order. Answers use
/// `pane_api::run_of_pane` (which also searches a worktree's origin project).
#[cfg(test)]
pub(in crate::agent_runs) fn run_for_pane(
    projects: &[String],
    pane_id: &str,
) -> Option<(String, String)> {
    projects.iter().find_map(|project| {
        latest_handle_for_pane(project, pane_id)
            .ok()
            .map(|handle| (project.clone(), handle))
    })
}

pub(crate) fn latest_handle_for_pane(
    project: &str,
    pane_id: &str,
) -> Result<String, OperationError> {
    let mut best: Option<crate::operations::OperationSnapshot> = None;
    for snapshot in operations::list_operations(project)? {
        if snapshot.pane_id.as_deref() != Some(pane_id) {
            continue;
        }
        let replace = best
            .as_ref()
            .map_or(true, |existing| snapshot.turn_index >= existing.turn_index);
        if replace {
            best = Some(snapshot);
        }
    }
    best.map(|snapshot| {
        if snapshot.agent_run_id.is_empty() {
            snapshot.operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        }
    })
    .ok_or_else(|| {
        OperationError::new(
            "AGENT_NOT_FOUND",
            format!("no agent run is bound to pane {pane_id}; use list_agents for the run handle"),
            false,
        )
    })
}

pub(super) fn resolve_run_handle(
    project: &str,
    id: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> Result<String, OperationError> {
    if operations::resolve_agent_run(project, id).is_ok() {
        return Ok(id.to_string());
    }
    let pane_ids: Vec<String> = registry.lock().panes.keys().cloned().collect();
    if let Ok(pane_id) = crate::mcp_sessions::resolve_pane_id(id, &pane_ids) {
        let kind = if pane_id == id {
            "a pane id"
        } else {
            "a pane id prefix"
        };
        let run_handle = latest_handle_for_pane(project, &pane_id).ok();
        let mut error = OperationError::new(
            "INVALID_AGENT_HANDLE",
            crate::mcp_sessions::pane_id_as_handle_message(
                id,
                kind,
                &pane_id,
                run_handle.as_deref(),
            ),
            false,
        );
        error.context = json!({"handle": id, "expected": "agent_run_handle", "actual_kind": "pane_id", "pane_id": pane_id, "run_handle": run_handle});
        return Err(error);
    }
    match operations::resolve_agent_run(project, id) {
        Ok(_) => Ok(id.to_string()),
        Err(error) if error.code == "AGENT_NOT_FOUND" => {
            // Stable worker names survive restarts; accept one that matches a single run.
            match crate::agent_runs::persist::resolve_worker_name(project, id)? {
                Some(handle) => Ok(handle),
                None => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}
