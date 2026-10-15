//! `take_over_agent` / `release_agent` handoff: resolves a run handle or pane id and moves
//! pane control between the agent and the requesting MCP session.

use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

use super::errors::operation_error_from_http;
use super::pane_util::registry_pane_ids;
use super::projects::resolve_operation_project;
use crate::pty::{registry_read_snapshot, PaneRegistry};

pub(super) fn handoff_route(
    session: Option<&str>,
    request: &serde_json::Value,
    registry: &Arc<Mutex<PaneRegistry>>,
    release: bool,
) -> Result<serde_json::Value, crate::operations::OperationError> {
    use crate::operations::OperationError;
    let session = session.ok_or_else(|| {
        OperationError::new(
            "SESSION_REQUIRED",
            "handoff requires an MCP connection",
            false,
        )
    })?;
    let requested_handle = request
        .get("handle")
        .and_then(serde_json::Value::as_str)
        .filter(|handle| !handle.is_empty());
    let mut prompt = serde_json::Value::Null;
    let mut run_handle: Option<String> = None;
    let pane_id = if let Some(handle) = requested_handle {
        let project = resolve_operation_project(
            request
                .get("project_path")
                .and_then(serde_json::Value::as_str),
            registry,
        )
        .map_err(operation_error_from_http)?;
        let project_str = project.to_string_lossy().into_owned();
        let mut resolved_name = None;
        let lookup = match crate::operations::resolve_agent_run(&project_str, handle) {
            Err(error) if error.code == "AGENT_NOT_FOUND" => {
                match crate::agent_runs::resolve_worker_name(&project_str, handle)? {
                    Some(named) => {
                        let found = crate::operations::resolve_agent_run(&project_str, &named);
                        resolved_name = Some(named);
                        found
                    }
                    None => Err(error),
                }
            }
            other => other,
        };
        let handle = resolved_name.as_deref().unwrap_or(handle);
        match lookup {
            Ok(snapshot) => {
                prompt = snapshot.required_action.unwrap_or(serde_json::Value::Null);
                run_handle = Some(handle.to_string());
                snapshot.pane_id.clone().unwrap_or_default()
            }
            Err(error) => {
                let ids = registry_pane_ids(registry);
                match crate::mcp_sessions::resolve_pane_id(handle, &ids) {
                    Ok(id) => id,
                    Err(_) => return Err(crate::mcp_sessions::enrich_handle_error(error, &ids)),
                }
            }
        }
    } else {
        let requested = request
            .get("pane_id")
            .and_then(serde_json::Value::as_str)
            .filter(|pane| !pane.is_empty())
            .ok_or_else(|| {
                OperationError::new("PANE_REQUIRED", "specify a handle or pane_id", false)
            })?;
        let ids = registry_pane_ids(registry);
        match crate::mcp_sessions::resolve_pane_id(requested, &ids) {
            Ok(id) => id,
            Err(error) => {
                if error.code == "PANE_NOT_FOUND" {
                    if let Some(closed) = crate::agent_runs::pane_close::closed_after_run_error(
                        &crate::pty::registry::get_project_path(registry),
                        registry,
                        requested,
                    ) {
                        return Err(closed);
                    }
                    if let Ok(project) = resolve_operation_project(
                        request
                            .get("project_path")
                            .and_then(serde_json::Value::as_str),
                        registry,
                    ) {
                        if crate::operations::resolve_agent_run(
                            &project.to_string_lossy(),
                            requested,
                        )
                        .is_ok()
                        {
                            return Err(OperationError::new("PANE_NOT_FOUND",format!("{requested} is an agent run handle, not a pane id; pass it as `handle`"),false));
                        }
                    }
                }
                return Err(error);
            }
        }
    };
    if !pane_id.is_empty() && pane_id.starts_with("puppet-master-orchestrator-") {
        return Err(OperationError::new(
            "AUTHORIZATION_DENIED",
            "orchestrator panes cannot be taken over",
            false,
        ));
    }
    let pane_alive = !pane_id.is_empty() && registry.lock().panes.contains_key(&pane_id);
    if !pane_alive && !pane_id.is_empty() {
        if let Some(error) = crate::agent_runs::pane_close::closed_after_run_error(
            &crate::pty::registry::get_project_path(registry),
            registry,
            &pane_id,
        ) {
            return Err(error);
        }
    }
    let screen = if pane_alive {
        registry_read_snapshot(registry, &pane_id)
            .map_err(|error| OperationError::new("PANE_NOT_FOUND", error, false))?
    } else {
        String::new()
    };
    if release {
        if let Some(handle) = run_handle.as_deref() {
            crate::mcp_sessions::check_run_access(session, handle)?;
        }
    }
    if release {
        crate::mcp_sessions::release_owned_pane(session, &pane_id)?;
        if let Some(handle) = run_handle.as_deref() {
            crate::mcp_sessions::release_owned_agent(session, handle)?;
        }
    } else {
        let grant = request
            .get("grant")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        // Setting A: an agent taking a pane or run it did not start needs a person's OK first.
        if grant {
            crate::agent_authority::gate::check_take_over(
                session,
                pane_alive.then_some(pane_id.as_str()),
                run_handle.as_deref(),
                Some(registry),
            )?;
        }
        if pane_alive {
            crate::mcp_sessions::take_over_pane(session, &pane_id, grant)?;
        } else if run_handle.is_none() {
            return Err(OperationError::new(
                "PANE_NOT_FOUND",
                "pane is not in the registry; pass `handle` with grant=true to reclaim the run lease",
                false,
            ));
        }
        if let Some(handle) = run_handle.as_deref() {
            crate::mcp_sessions::take_over_agent(session, handle, grant)?;
        }
    }
    Ok(json!({
        "handle": run_handle.as_deref().or(requested_handle).unwrap_or(pane_id.as_str()),
        "pane_id": if pane_id.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(pane_id.clone())
        },
        "screen": screen,
        "prompt": prompt,
        "control": if release { "agent" } else { "shell" },
    }))
}
