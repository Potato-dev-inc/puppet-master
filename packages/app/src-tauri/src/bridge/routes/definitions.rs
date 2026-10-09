//! `/agents/definitions` routes (SPEC 7.5): list, reload, validate, approve, revoke, consent, plus
//! `source` (switch a source on/off) and `extra` (add/remove an extra directory) for Settings.
//!
//! These belong to the app's own session only (APR-7). An MCP session (any request carrying the
//! session header) is refused here as well as by `mcp_sessions::authorize`, so the gate does not
//! depend on one layer. They run before the agent-run router, whose `GET /agents/{handle}` would
//! otherwise swallow `GET /agents/definitions`.

use serde_json::{json, Value};

use super::{Ctx, RouteResult};
use crate::tui::declarative::service::{self, ServiceError};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    let local_app = is_local_app(cx.headers, cx.peer_loopback);
    respond(cx.method, cx.segments, &cx.request_body, cx.session, local_app)
}

/// True only for the desktop app itself: a loopback peer that is neither proxied (tunnel / vite
/// proxy add `X-PM-Proxied`) nor carries a paired-device bearer token or forwarding headers.
/// A paired remote device (PWA over the tunnel) is therefore never the "app session" (APR-7).
pub(in crate::bridge) fn is_local_app(headers: &str, peer_loopback: bool) -> bool {
    use crate::mobile_pairing::{bearer_token, header_value, requires_pairing_token};
    peer_loopback
        && !requires_pairing_token(headers, peer_loopback)
        && bearer_token(headers).is_none()
        && header_value(headers, "Authorization").is_none()
        && ["X-Forwarded-For", "Forwarded", "CF-Connecting-IP", "X-Real-IP", "CF-Ray"]
            .iter()
            .all(|h| header_value(headers, h).is_none())
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
    local_app: bool,
) -> Option<RouteResult> {
    let route = match (method, segments) {
        ("GET", ["agents", "definitions"]) => "list",
        (
            "POST",
            ["agents", "definitions", action @ ("reload" | "validate" | "approve" | "revoke" | "consent" | "source" | "extra")],
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
    if route != "list" && !local_app {
        let message = "agent definitions can only be changed from the local desktop app, not a remote device";
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
        "source" => service::set_source(body).map(|v| (200, v)).map_err(fail),
        "extra" => service::extra_dir(body).map(|v| (200, v)).map_err(fail),
        _ => service::consent(body).map(|v| (200, v)).map_err(fail),
    })
}
