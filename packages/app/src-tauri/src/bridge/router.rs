//! Top-level request routing: authorisation, the root and health endpoints, body parsing,
//! then hand-off to the route groups in `routes`.

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tauri::AppHandle;

use super::errors::{enrich_handle_error, operation_http_error, RouteResult};
use super::http::request_session;
use super::routes::{self, Ctx};
use crate::mobile_pairing;
use crate::pty::PaneRegistry;

#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
    version: &'static str,
    catalog_version: String,
    /// Git sha, dirty flag and build time of the running binary.
    build_id: &'static str,
    tool_count: usize,
    /// Warm standby pool counts (see `GET /standby` for the detail); absent before it starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    standby: Option<crate::standby::HealthSummary>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn route(
    method: &str,
    segments: &[&str],
    query: &str,
    body: &[u8],
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    headers: &str,
    peer_loopback: bool,
) -> RouteResult {
    mobile_pairing::authorize_bridge_request(headers, peer_loopback, segments, method)?;
    if segments.is_empty() && method == "GET" {
        return Ok((
            200,
            json!({
                "service": "puppet-master-bridge",
                "hint": "This is the API bridge, not the mobile UI. Tunnel the Vite app (port 1420 or 4173) and use /bridge on that origin.",
                "health": "/health",
            }),
        ));
    }

    if segments == ["health"] && method == "GET" {
        crate::mcp_sessions::authorize(request_session(headers), method, "/health", &json!({}))
            .map_err(operation_http_error)?;
        return Ok((
            200,
            serde_json::to_value(Health {
                ok: true,
                version: env!("CARGO_PKG_VERSION"),
                catalog_version: crate::tool_registry::catalog_version(),
                build_id: env!("PM_BUILD_ID"),
                tool_count: crate::tool_registry::external_mcp_tool_count(),
                standby: crate::standby::health_summary(),
            })
            .unwrap(),
        ));
    }

    let session = request_session(headers);
    let request_body: serde_json::Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(body).map_err(|error| {
            (
                400,
                json!({"code":"INVALID_ARGUMENT","error":error.to_string()}),
            )
        })?
    };
    let route_path = format!("/{}", segments.join("/"));
    // A paired phone (it got past the pairing token check above) is the user at another screen:
    // like the desktop pane window, it may type into any pane, including ones an MCP run manages.
    let paired_phone_typing = session.is_none()
        && mobile_pairing::requires_pairing_token(headers, peer_loopback)
        && is_pane_typing_route(method, segments);
    if !paired_phone_typing {
        crate::mcp_sessions::authorize(session, method, &route_path, &request_body)
            .map_err(|error| enrich_handle_error(error, registry))
            .map_err(operation_http_error)?;
    }

    if let Some(reply) = crate::questions::routes::handle(
        method,
        segments,
        &request_body,
        session,
        mobile_pairing::requires_pairing_token(headers, peer_loopback),
        &|| crate::questions::routes::publish_changed(app),
    ) {
        return reply;
    }

    let cx = Ctx {
        method,
        segments,
        query,
        body,
        request_body,
        headers,
        peer_loopback,
        session,
        registry,
        app,
    };
    routes::dispatch(&cx).unwrap_or_else(|| {
        Err((
            404,
            json!({
                "error": "not found",
                "hint": "Unknown bridge route. Mobile UI is served by Vite on port 1420/4173; API lives under /health, /events, /panes, /orchestrator/message, /orchestrator/viewport.",
            }),
        ))
    })
}

/// `POST /panes/{id}/input` and `POST /panes/{id}/key`: typing into a pane.
fn is_pane_typing_route(method: &str, segments: &[&str]) -> bool {
    method == "POST" && matches!(segments, ["panes", _, "input" | "key"])
}

#[cfg(test)]
mod tests {
    use super::is_pane_typing_route;

    #[test]
    fn only_pane_input_and_keys_count_as_typing() {
        assert!(is_pane_typing_route("POST", &["panes", "p1", "input"]));
        assert!(is_pane_typing_route("POST", &["panes", "p1", "key"]));
        assert!(!is_pane_typing_route("GET", &["panes", "p1", "input"]));
        assert!(!is_pane_typing_route("DELETE", &["panes", "p1"]));
        assert!(!is_pane_typing_route("POST", &["panes", "p1", "viewport"]));
        assert!(!is_pane_typing_route("POST", &["agents", "run"]));
    }
}
