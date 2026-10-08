//! `/agents/definitions` routes (SPEC 7.5): list, reload, validate, approve, revoke, consent.
//!
//! These belong to the app's own session only (APR-7). An MCP session (any request carrying the
//! session header) is refused here as well as by `mcp_sessions::authorize`, so the gate does not
//! depend on one layer. They run before the agent-run router, whose `GET /agents/{handle}` would
//! otherwise swallow `GET /agents/definitions`.

use serde_json::{json, Value};

use super::{Ctx, RouteResult};
use crate::tui::declarative::service::{self, ServiceError};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    respond(cx.method, cx.segments, &cx.request_body, cx.session)
}

fn fail(error: ServiceError) -> (u16, Value) {
    (error.status, error.to_json())
}

fn text<'a>(body: &'a Value, key: &str) -> Result<&'a str, (u16, Value)> {
    body.get(key).and_then(Value::as_str).filter(|v| !v.trim().is_empty()).ok_or_else(|| {
        let message = format!("{key} is required");
        (400, json!({"code": "INVALID_ARGUMENT", "error": message, "message": message}))
    })
}

/// The route table without the Tauri context, so tests can drive it.
pub(in crate::bridge) fn respond(
    method: &str,
    segments: &[&str],
    body: &Value,
    session: Option<&str>,
) -> Option<RouteResult> {
    let route = match (method, segments) {
        ("GET", ["agents", "definitions"]) => "list",
        (
            "POST",
            ["agents", "definitions", action @ ("reload" | "validate" | "approve" | "revoke" | "consent")],
        ) => *action,
        _ => return None,
    };
    if session.is_some_and(|s| !s.trim().is_empty()) {
        let message = "agent definitions are managed by the app session, not MCP sessions";
        return Some(Err((
            403,
            json!({"code": "AUTHORIZATION_DENIED", "error": message, "message": message, "recoverable": false}),
        )));
    }
    Some(match route {
        "list" => Ok((200, service::listing_global())),
        "reload" => Ok((200, service::reload_global_with_diff())),
        "validate" => text(body, "toml").and_then(|toml| {
            let source = body.get("source").and_then(Value::as_str).unwrap_or("user");
            service::validate_global(toml, source).map(|v| (200, v)).map_err(fail)
        }),
        "approve" => text(body, "file").and_then(|file| {
            let fingerprint = text(body, "fingerprint")?;
            service::approve_global(file, fingerprint).map(|v| (200, v)).map_err(fail)
        }),
        "revoke" => text(body, "id").and_then(|id| service::revoke_global(id).map(|v| (200, v)).map_err(fail)),
        _ => service::consent(body).map(|v| (200, v)).map_err(fail),
    })
}
