//! `/operations/*` routes: delegate, look up by idempotency key or handle, get, wait, cancel.

use serde::Deserialize;

use super::{Ctx, RouteResult};
use crate::bridge::delegation::{
    cancel_operation_http, delegate_operation_http, operation_response_value,
};
use crate::bridge::errors::operation_http_error;
use crate::bridge::http::{parse_json, query_param, request_session, requested_project_path};
use crate::bridge::projects::resolve_operation_project;

#[derive(Debug, Deserialize)]
struct WaitOperationBody {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    after_revision: Option<u64>,
    #[serde(default)]
    until: Option<Vec<crate::operations::OperationStatus>>,
    #[serde(default = "default_operation_wait_ms")]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
struct CancelOperationBody {
    #[serde(default)]
    project_path: Option<String>,
}
fn default_operation_wait_ms() -> u64 {
    30_000
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("POST", ["operations", "delegate"]) => delegate(cx),
        ("GET", ["operations", "by-key"]) => by_key(cx),
        ("GET", ["operations", "by-handle"]) => by_handle(cx),
        ("GET", ["operations", id]) => get(cx, id),
        ("POST", ["operations", id, "wait"]) => wait(cx, id),
        ("POST", ["operations", id, "cancel"]) => cancel(cx, id),
        _ => return None,
    })
}

fn delegate(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let headers = cx.headers;
    let registry = cx.registry.clone();
    let app = cx.app.clone();
    let mut req: crate::operations::DelegateWorkRequest = parse_json(body)?;
    req.owner_session_id = request_session(headers).map(str::to_string);
    delegate_operation_http(req, registry, app)
}

fn by_key(cx: &Ctx) -> RouteResult {
    let query = cx.query;
    let key = query_param(query, "idempotency_key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "INVALID_ARGUMENT",
                "idempotency_key is required",
                false,
            ))
        })?;
    let project = query_param(query, "project_path");
    let snapshot = crate::operations::find_operation_by_idempotency_key(project.as_deref(), &key)
        .map_err(operation_http_error)?
        .ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "OPERATION_NOT_FOUND",
                "no operation found for idempotency_key",
                false,
            ))
        })?;
    Ok((200, operation_response_value(snapshot)))
}

fn by_handle(cx: &Ctx) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    let handle = query_param(query, "agent_run_id")
        .or_else(|| query_param(query, "handle"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "INVALID_ARGUMENT",
                "agent_run_id or handle is required",
                false,
            ))
        })?;
    let project =
        resolve_operation_project(query_param(query, "project_path").as_deref(), registry)?;
    let snapshot = crate::operations::resolve_agent_run(&project.to_string_lossy(), &handle)
        .map_err(operation_http_error)?;
    Ok((200, operation_response_value(snapshot)))
}

fn get(cx: &Ctx, id: &str) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    let segments = ["", id];
    let requested = query_param(query, "project_path");
    let project = resolve_operation_project(requested.as_deref(), registry)?;
    let snapshot = crate::operations::get_operation(&project.to_string_lossy(), segments[1])
        .map_err(operation_http_error)?;
    Ok((200, operation_response_value(snapshot)))
}

fn wait(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let query = cx.query;
    let registry = cx.registry;
    let segments = ["", id];
    let req: WaitOperationBody = parse_json(body)?;
    let project = resolve_operation_project(
        requested_project_path(req.project_path.as_deref(), query).as_deref(),
        registry,
    )?;
    let after_revision = match req.after_revision {
        Some(revision) => revision,
        None => {
            crate::operations::get_operation(&project.to_string_lossy(), segments[1])
                .map_err(operation_http_error)?
                .revision
        }
    };
    let result = crate::operations::wait_for_operation(
        &project.to_string_lossy(),
        segments[1],
        after_revision,
        req.until,
        req.timeout_ms,
    )
    .map_err(operation_http_error)?;
    let sanitized = crate::operations::OperationWaitResult {
        snapshot: crate::operations::snapshot_for_api(result.snapshot),
        reason: result.reason,
    };
    Ok((200, serde_json::to_value(sanitized).unwrap()))
}

fn cancel(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let query = cx.query;
    let registry = cx.registry.clone();
    let app = cx.app.clone();
    let segments = ["", id];
    let req: CancelOperationBody = if body.is_empty() {
        CancelOperationBody { project_path: None }
    } else {
        parse_json(body)?
    };
    let project = resolve_operation_project(
        requested_project_path(req.project_path.as_deref(), query).as_deref(),
        &registry,
    )?;
    let project_text = project.to_string_lossy().into_owned();
    cancel_operation_http(project_text, segments[1], registry, app)
}
