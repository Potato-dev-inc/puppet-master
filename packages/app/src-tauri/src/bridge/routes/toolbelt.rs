//! `/toolbelt` routes: the same actions as the Tauri commands, for the app's own session only
//! (like `/agents/definitions`): an MCP session or a remote device is refused, because the toolbelt
//! writes files and holds secrets.

use serde_json::{json, Value};

use super::definitions::is_local_app;
use super::{Ctx, RouteResult};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    let local_app = is_local_app(cx.headers, cx.peer_loopback);
    respond(cx.method, cx.segments, &cx.request_body, cx.session, local_app, &|a, b| {
        crate::toolbelt::service::call_live(a, b)
    })
}

fn denied(message: &str) -> RouteResult {
    Err((403, json!({"code": "AUTHORIZATION_DENIED", "error": message, "message": message, "recoverable": false})))
}

/// The route table without the Tauri context or the real filesystem, so tests can drive it.
pub(in crate::bridge) fn respond(
    method: &str,
    segments: &[&str],
    body: &Value,
    session: Option<&str>,
    local_app: bool,
    call: &dyn Fn(&str, &Value) -> Result<Value, String>,
) -> Option<RouteResult> {
    let action = match (method, segments) {
        ("GET", ["toolbelt"]) => "list",
        ("POST", ["toolbelt"]) => "add",
        ("POST", ["toolbelt", "update"]) => "update",
        ("POST", ["toolbelt", "remove"]) => "remove",
        ("POST", ["toolbelt", "sync", "preview"]) => "sync_preview",
        ("POST", ["toolbelt", "sync", "apply"]) => "sync_apply",
        ("POST", ["toolbelt", "import", "preview"]) => "import_preview",
        ("POST", ["toolbelt", "import", "apply"]) => "import_apply",
        _ => return None,
    };
    if session.is_some_and(|s| !s.trim().is_empty()) {
        return Some(denied("the toolbelt is managed by the app session, not MCP sessions"));
    }
    if !local_app {
        return Some(denied("the toolbelt can only be used from the local desktop app, not a remote device"));
    }
    Some(match call(action, body) {
        Ok(v) => Ok((200, v)),
        Err(message) => Err((400, json!({"code": "INVALID_ARGUMENT", "error": message, "message": message}))),
    })
}
