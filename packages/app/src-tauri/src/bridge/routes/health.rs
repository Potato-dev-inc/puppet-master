//! Agent doctor routes: `GET /agents/health` (installed + version, free) and
//! `POST /agents/health/test {"agent": id}` (one bounded read-only question; costs tokens).

use super::{Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;
use serde_json::json;

/// The routes that need no app handle (the free quick check); also what the in-process test calls.
pub(in crate::bridge) fn static_route(
    method: &str,
    segments: &[&str],
) -> Option<(u16, serde_json::Value)> {
    match (method, segments) {
        ("GET", ["agents", "health"]) => {
            Some((200, json!({ "agents": crate::agent_health::check_all() })))
        }
        _ => None,
    }
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if let Some(reply) = static_route(cx.method, cx.segments) {
        return Some(Ok(reply));
    }
    match (cx.method, cx.segments) {
        ("POST", ["agents", "health", "test"]) => Some(test(cx)),
        _ => None,
    }
}

fn test(cx: &Ctx) -> RouteResult {
    let agent = cx.request_body.get("agent").and_then(|v| v.as_str()).unwrap_or("").trim();
    if agent.is_empty() {
        return Err(operation_http_error(crate::operations::OperationError::new(
            "INVALID_ARGUMENT",
            "agent is required",
            false,
        )));
    }
    let health = crate::agent_health::test_agent(agent, cx.registry.clone(), cx.app.clone());
    Ok((200, serde_json::to_value(health).unwrap_or_default()))
}
