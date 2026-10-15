//! HTTP routes for `/agents/*`: dispatches each request to the runtime functions that serve it.
//!
//! - `wire`: request bodies and query-string helpers.
//! - `list`, `handles`, `wait`, `answer`: the handlers that need more than a call into `runtime`.

mod answer;
mod handles;
mod list;
#[cfg(test)]
mod tests;
mod wait;
mod wire;

use super::runtime::{self, AgentRunRequest};
use crate::operations::OperationError;
use handles::resolve_run_handle;
use list::{list_agents, AgentListFilter};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use tauri::AppHandle;
use wait::{transcript, wait_agents};
use wire::{
    parse, project_path, query_value, AgentHandleRequest, AnswerPromptRequest, SendAgentRequest,
    WaitAgentsRequest,
};

pub(crate) use answer::answer_for_pane;
pub(crate) use handles::latest_handle_for_pane;
#[cfg(test)]
pub(super) use handles::run_for_pane;
#[cfg(test)]
pub(crate) use list::visible_runs;

pub fn handle_request(
    method: &str,
    segments: &[&str],
    query: &str,
    body: &[u8],
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    session_id: Option<&str>,
) -> Option<Result<Value, OperationError>> {
    // Mission handles (`mission-<id>`) and `run_agent(until_checks_pass)`: `crate::missions`.
    if let Some(result) = crate::missions::routes::intercept(method, segments, body, registry, app, session_id) {
        return Some(result);
    }
    let result = match (method, segments) {
        ("POST", ["agents", "run"]) => parse::<AgentRunRequest>(body).and_then(|mut request| {
            if request
                .project_path
                .as_deref()
                .is_none_or(|path| path.trim().is_empty())
            {
                request.project_path = query_value(query, "project_path");
            }
            runtime::run_agent(
                request,
                registry.clone(),
                app.clone(),
                session_id.map(str::to_string),
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("POST", ["agents", "quick"]) => {
            parse::<super::quick::QuickAskRequest>(body).and_then(|mut request| {
                if request
                    .project_path
                    .as_deref()
                    .is_none_or(|path| path.trim().is_empty())
                {
                    request.project_path = query_value(query, "project_path");
                }
                super::quick::quick_ask(
                    request,
                    registry.clone(),
                    app.clone(),
                    session_id.map(str::to_string),
                )
            })
        }
        ("POST", ["agents", "wait"]) => parse::<WaitAgentsRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            wait_agents(&project, request, session_id, registry)
        }),
        ("POST", ["agents", "send"]) => parse::<SendAgentRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            let message = request.message.or(request.task).unwrap_or_default();
            if message.trim().is_empty() {
                return Err(OperationError::new(
                    "INVALID_TASK",
                    "message is required",
                    false,
                ));
            }
            runtime::send_message(
                &project,
                &request.handle,
                message,
                session_id,
                request.delivery.as_deref(),
                request.idempotency_key.as_deref(),
                registry.clone(),
                app.clone(),
            )
        }),
        ("POST", ["agents", "followup"]) => parse::<SendAgentRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            let task = request.task.or(request.message).unwrap_or_default();
            if task.trim().is_empty() {
                return Err(OperationError::new(
                    "INVALID_TASK",
                    "task is required",
                    false,
                ));
            }
            runtime::send_agent(
                &project,
                &request.handle,
                task,
                session_id.map(str::to_string),
                registry.clone(),
                app.clone(),
                super::persist::FollowupOpts {
                    context_policy: request.context_policy,
                    selected_history: request.selected_history,
                    requested_model: request.model,
                    requested_reasoning: request.reasoning,
                    role: None,
                    scope: request.scope,
                    background: request.background,
                    wait_ms: request.wait_ms,
                    idempotency_key: request.idempotency_key,
                    acceptance_criteria: request.acceptance_criteria,
                    checks: request.checks,
                    locks: request.locks,
                    ..Default::default()
                },
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("POST", ["agents", "answer"]) => parse::<AnswerPromptRequest>(body)
            .and_then(|request| answer::route_answer(request, query, session_id, registry, app)),
        ("POST", ["agents", "cancel"]) => parse::<AgentHandleRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            runtime::cancel_agent(
                &project,
                &request.handle,
                session_id,
                registry.clone(),
                app.clone(),
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("GET", ["agents", "steer-receipt"]) => {
            let handle = query_value(query, "handle").filter(|value| !value.is_empty());
            let idempotency_key =
                query_value(query, "idempotency_key").filter(|value| !value.is_empty());
            let (Some(handle), Some(idempotency_key)) = (handle, idempotency_key) else {
                return Some(Err(OperationError::new(
                    "INVALID_ARGUMENT",
                    "handle and idempotency_key are required",
                    false,
                )));
            };
            let project = project_path(
                query_value(query, "project_path").as_deref(),
                query,
                registry,
            );
            super::messaging::find_receipt_by_key(&project, &handle, &idempotency_key)
                .map(|receipt| super::messaging::receipt_value(&receipt))
                .ok_or_else(|| {
                    OperationError::new("STEER_RECEIPT_NOT_FOUND", "steer receipt not found", false)
                })
        }
        ("GET", ["agents"]) => {
            let explicit = query_value(query, "project_path");
            let project = project_path(explicit.as_deref(), query, registry);
            let filter = AgentListFilter::from_query(query);
            list_agents(&project, session_id, registry, explicit.is_some(), filter)
        }
        // `GET /agents/types` is the agent-type listing served by `bridge/routes/mcp.rs`.
        ("GET", ["agents", "types"]) => return None,
        ("GET", ["agents", handle]) => {
            let project = project_path(None, query, registry);
            resolve_run_handle(&project, handle, registry).and_then(|handle| {
                let mut value = runtime::inspect_agent(&project, &handle, session_id)?;
                if let Ok(snapshot) = runtime::current_agent_for_read(&project, &handle, session_id)
                {
                    super::persist::overlay_live_model(&mut value, &snapshot, Some(registry));
                }
                Ok(value)
            })
        }
        ("POST", ["agents", "close"]) => parse::<AgentHandleRequest>(body).and_then(|request| {
            let project = project_path(request.project_path.as_deref(), query, registry);
            runtime::close_agent(
                &project,
                &request.handle,
                session_id,
                registry.clone(),
                app.clone(),
            )
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        }),
        ("GET", ["agents", handle, "transcript"]) => {
            let project = project_path(None, query, registry);
            resolve_run_handle(&project, handle, registry)
                .and_then(|handle| transcript(&project, &handle, query, session_id))
        }
        _ => return None,
    };
    Some(result)
}
