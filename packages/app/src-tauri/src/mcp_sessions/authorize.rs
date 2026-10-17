//! Bridge gate: catalog mode plus pane/agent ownership are enforced before a route runs.

use super::access::{check_run_access, check_run_read_access, check_run_wait_access};
use super::handles::denied;
use super::identity::mode_for;
use super::leases::session_controls_pane;
use super::lineage::pane_started_by;
use super::state::{lock_state, PaneKind};
use crate::operations::OperationError;
use crate::tool_registry::{self, McpMode, ToolSafety};
use serde_json::{json, Value};

pub(super) fn readonly_operation_route(method: &str, clean_path: &str) -> bool {
    if clean_path == "/operations/by-key" || clean_path == "/operations/by-handle" {
        return method == "GET";
    }
    let Some(rest) = clean_path.strip_prefix("/operations/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
    match parts.as_slice() {
        [id] if method == "GET" => !id.is_empty(),
        [id, "wait"] if method == "POST" => !id.is_empty(),
        _ => false,
    }
}

/// Enforce both the connection's catalog mode and pane ownership before a bridge route runs.
/// Calls without a session header retain the legacy local MCP behavior.
pub fn authorize(
    session_id: Option<&str>,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<(), OperationError> {
    let Some(session_id) = session_id.filter(|id| !id.trim().is_empty()) else {
        if method == "POST" && path == "/mcp/mode" {
            return Err(OperationError::new(
                "SESSION_REQUIRED",
                "set_mode requires an MCP connection session id",
                false,
            ));
        }
        let clean_path = path.split('?').next().unwrap_or(path);
        // The app's own session (webview, local tools) carries no MCP header; it is the only
        // caller `/agents/definitions` serves (APR-7). MCP sessions are refused further down.
        // `GET /agents/types` is the read-only agent list the mobile PWA's "new agent" sheet needs.
        let app_route = clean_path == "/agents/definitions"
            || clean_path.starts_with("/agents/definitions/")
            || clean_path.starts_with("/agents/wizard/")
            || (method == "GET" && clean_path == "/agents/types");
        if !app_route && (clean_path == "/agents" || clean_path.starts_with("/agents/")) {
            return Err(OperationError::new(
                "SESSION_REQUIRED",
                "agent APIs require an MCP session id",
                false,
            ));
        }
        let mutating = tool_registry::tools().iter().any(|tool| {
            method == tool.method
                && route_matches(tool.path, clean_path)
                && matches!(tool.safety, ToolSafety::Mutating | ToolSafety::Destructive)
        });
        let target_pane =
            pane_id_from_route(clean_path).or_else(|| body.get("pane_id").and_then(Value::as_str));
        if clean_path == "/agents/take-over"
            || clean_path == "/agents/release"
            || clean_path == "/agents/attach"
            || clean_path == "/agents/release-lease"
            || clean_path == "/agents/transfer"
            || clean_path == "/mcp/session"
        {
            return Err(OperationError::new(
                "SESSION_REQUIRED",
                "agent ownership operations require an MCP session id",
                false,
            ));
        }
        let handle = body
            .get("handle")
            .and_then(Value::as_str)
            .or_else(|| body.get("operation_id").and_then(Value::as_str))
            .or_else(|| {
                clean_path
                    .strip_prefix("/agents/")
                    .and_then(|rest| rest.strip_suffix("/transcript"))
            })
            .or_else(|| {
                clean_path
                    .strip_prefix("/operations/")
                    .and_then(|rest| rest.split('/').next())
            });
        if !readonly_operation_route(method, clean_path)
            && handle.is_some_and(|handle| lock_state().agents.contains_key(handle))
        {
            return Err(denied("managed agent operations require an MCP session id"));
        }
        let handles = body.get("handles").and_then(Value::as_array);
        if handles.is_some_and(|handles| {
            let state = lock_state();
            handles
                .iter()
                .filter_map(Value::as_str)
                .any(|handle| state.agents.contains_key(handle))
        }) {
            return Err(denied("managed agent operations require an MCP session id"));
        }
        if mutating && target_pane.is_some_and(|pane| lock_state().panes.contains_key(pane)) {
            if method == "DELETE" {
                return Err(managed_pane_close_denied(target_pane.unwrap_or_default()));
            }
            return Err(denied("managed pane mutations require an MCP session id"));
        }
        return Ok(());
    };
    let clean_path = path.split('?').next().unwrap_or(path);
    if (method == "GET" && clean_path == "/mcp/tools")
        || (method == "GET" && clean_path == "/mcp/mode")
        || (method == "POST" && clean_path == "/mcp/mode")
        || (method == "GET" && clean_path == "/mcp/session")
    {
        return Ok(());
    }

    // Definition management is the app session only (APR-7); `GET /agents/{handle}` must not
    // be able to claim `/agents/definitions` for an MCP session either.
    if clean_path == "/agents/definitions" || clean_path.starts_with("/agents/definitions/") {
        return Err(denied("agent definitions are managed by the app session, not MCP sessions"));
    }
    // The agent wizard runs programs and writes definitions: app session only, like definitions.
    if clean_path == "/agents/wizard" || clean_path.starts_with("/agents/wizard/") {
        return Err(denied("the agent wizard belongs to the app session, not MCP sessions"));
    }

    let definitions = tool_registry::tools();
    let tool = definitions
        .iter()
        .find(|tool| method == tool.method && route_matches(tool.path, clean_path))
        .ok_or_else(|| denied("route is not exposed to MCP sessions"))?;
    let mode = mode_for(session_id);
    if !tool_registry::tool_visible_in_mode(tool.name, mode) {
        let target_mode = if tool_registry::tool_visible_in_mode(tool.name, McpMode::Agent) {
            "agent"
        } else if tool_registry::tool_visible_in_mode(tool.name, McpMode::Shell) {
            "shell"
        } else {
            "both"
        };
        let mut error = OperationError::new(
            "MODE_MISMATCH",
            format!("{} is unavailable in {} mode", tool.name, mode.as_str()),
            false,
        );
        error.context = json!({"tool":tool.name,"current_mode":mode.as_str(),"available_in":target_mode,"switch_with":{"mode":target_mode}});
        if tool.name == "kill_pane_process" {
            // The common case: closing the pane a run created. That is the run's `close_agent`.
            error.message = format!(
                "kill_pane_process is unavailable in {} mode. To close a pane that run_agent created, call close_agent with the run's handle (it ends the worker and closes its pane); kill_pane_process is for terminal panes in shell mode",
                mode.as_str()
            );
            error.context["use_instead"] = json!({"tool":"close_agent","route":"POST /agents/close","needs":"the run handle"});
        }
        return Err(error);
    }

    let handle = body
        .get("handle")
        .and_then(Value::as_str)
        .or_else(|| body.get("operation_id").and_then(Value::as_str))
        .or_else(|| {
            clean_path
                .strip_prefix("/operations/")
                .and_then(|rest| rest.split('/').next())
        })
        .or_else(|| {
            let rest = clean_path.strip_prefix("/agents/")?;
            match rest.split_once('/') {
                Some((candidate, suffix)) if suffix == "transcript" => Some(candidate),
                None if !rest.is_empty()
                    && rest != "run"
                    && rest != "wait"
                    && rest != "send"
                    && rest != "followup"
                    && rest != "answer"
                    && rest != "cancel"
                    && rest != "close"
                    && rest != "take-over"
                    && rest != "release"
                    && rest != "attach"
                    && rest != "release-lease"
                    && rest != "transfer" =>
                {
                    Some(rest)
                }
                _ => None,
            }
        });
    if matches!(
        tool.name,
        "wait_agents"
            | "send_agent"
            | "send_message"
            | "followup_task"
            | "answer_prompt"
            | "cancel_agent"
            | "interrupt_agent"
            | "inspect_agent"
            | "agent_transcript"
            | "close_agent"
            | "review_worktree"
            | "cancel_operation"
    ) {
        if tool.name == "wait_agents" {
            let handles = body
                .get("handles")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    OperationError::new(
                        "INVALID_ARGUMENT",
                        "handles must be a non-empty array",
                        false,
                    )
                })?;
            for handle in handles.iter().filter_map(Value::as_str) {
                check_run_wait_access(session_id, handle)?;
            }
        } else if tool.name == "inspect_agent" || tool.name == "agent_transcript" {
            if let Some(handle) = handle {
                check_run_read_access(session_id, handle)?;
            }
        } else if let Some(handle) = handle {
            check_run_access(session_id, handle)?;
        }
    }

    let pane_id =
        pane_id_from_route(clean_path).or_else(|| body.get("pane_id").and_then(Value::as_str));
    let is_pane_mutation = tool.name != "take_over"
        && matches!(tool.safety, ToolSafety::Mutating | ToolSafety::Destructive)
        && (pane_id.is_some() || clean_path.starts_with("/panes/"));
    if is_pane_mutation {
        if let Some(pane_id) = pane_id {
            if tool.name == "switch_agent_model" {
                if session_controls_pane(session_id, pane_id) {
                    return Ok(());
                }
                if let Some(handle) = body.get("handle").and_then(Value::as_str) {
                    if check_run_access(session_id, handle).is_ok() {
                        return Ok(());
                    }
                }
            }
            let (pane, started_here) = {
                let state = lock_state();
                let pane = state.panes.get(pane_id).cloned();
                let started_here = pane
                    .as_ref()
                    .is_some_and(|pane| pane_started_by(&state, session_id, pane));
                (pane, started_here)
            };
            let pane = match pane {
                Some(pane) => pane,
                None if tool.name == "run_agent" => {
                    // UI panes are adopted inside bind_existing_worker (take_over_pane grant=true).
                    return Ok(());
                }
                None => {
                    return Err(denied(
                        "pane is not adopted by this connection; call take_over with grant=true or run_agent(pane_id) to adopt a UI worker",
                    ));
                }
            };
            // A pane this connection's own run_agent / followup_task opened is its own: it may
            // press keys, type and answer a prompt in it without a take_over. Panes the person
            // opened, or another agent started, still need take_over (and the consent gate), and
            // every other terminal control (shell_exec) keeps its explicit take_over.
            let own_run_pane = started_here
                && matches!(tool.name, "press_key" | "write_terminal_input" | "answer_prompt");
            let agent_requires_takeover =
                pane.kind == PaneKind::Agent && !pane.explicitly_taken_over && !own_run_pane;
            if agent_requires_takeover && tool.name != "run_agent" {
                return Err(denied(
                    "agent panes are read-only to terminal controls until explicitly taken over",
                ));
            }
            if pane.released && tool.name != "run_agent" {
                return Err(denied(
                    "this pane was released; call take_over again (grant=true) to regain control",
                ));
            }
            if pane.controller.as_deref() != Some(session_id) {
                return Err(denied(
                    "this pane is controlled by another MCP coordinator; wait for release or use transfer_agent. run_agent(pane_id) cannot adopt a pane owned by another connection",
                ));
            }
        }
    }
    Ok(())
}

/// `DELETE /panes/<id>` with no MCP session on a pane a run created: say how to close it.
fn managed_pane_close_denied(pane_id: &str) -> OperationError {
    let mut error = denied(
        "this pane belongs to an agent run started by an MCP session, so it is closed through that run, not over bare HTTP. From the owning session call close_agent with the run's handle (POST /agents/close), or cancel the run (POST /operations/<operation_id>/cancel); in the app use Close on the pane. DELETE /panes/<id> is for panes you opened yourself",
    );
    error.context = json!({
        "pane_id": pane_id,
        "close_with": [
            {"tool":"close_agent","route":"POST /agents/close","needs":"the run handle, from the owning MCP session"},
            {"route":"POST /operations/<operation_id>/cancel","effect":"cancels the run and closes the pane it created"},
            {"where":"app","action":"Close on the pane"}
        ]
    });
    error
}

pub(super) fn route_matches(template: &str, path: &str) -> bool {
    let expected: Vec<_> = template.trim_matches('/').split('/').collect();
    let actual: Vec<_> = path.trim_matches('/').split('/').collect();
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual.iter())
            .all(|(a, b)| (a.starts_with('{') && a.ends_with('}')) || a == b)
}

pub(super) fn pane_id_from_route(path: &str) -> Option<&str> {
    let mut parts = path.trim_matches('/').split('/');
    (parts.next() == Some("panes"))
        .then(|| parts.next())
        .flatten()
}
