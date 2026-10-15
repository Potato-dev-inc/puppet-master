//! `/mcp/*` discovery and session routes plus the `/agents/*` lease and handoff routes.

use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::errors::{enrich_handle_error, operation_http_error};
use crate::bridge::handoff::handoff_route;

/// MCP session routes; these run before the agent-run router.
pub(super) fn handle_session(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["mcp", "tools"]) => tools(cx),
        ("GET", ["mcp", "session"]) => session_identity(cx),
        ("POST", ["mcp", "mode"]) => set_mode(cx),
        _ => return None,
    })
}

/// Agent lease/handoff routes and the static MCP registry listings; these run after the
/// agent-run router and the shell route.
pub(super) fn handle_agents(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["agents", "types"]) => {
            Ok((200, types_route(cx.session, cx.headers, cx.peer_loopback)))
        }
        ("POST", ["agents", "take-over"]) => handoff(cx, false),
        ("POST", ["agents", "release"]) => handoff(cx, true),
        ("POST", ["agents", "attach"]) => attach(cx),
        ("POST", ["agents", "release-lease"]) => release_lease(cx),
        ("POST", ["agents", "transfer"]) => transfer(cx),
        _ => return registry_route(cx.method, cx.segments).map(Ok),
    })
}

pub(in crate::bridge) fn registry_route(
    method: &str,
    segments: &[&str],
) -> Option<(u16, serde_json::Value)> {
    match (method, segments) {
        ("GET", ["mcp", "tools"]) => Some((
            200,
            serde_json::to_value(crate::tool_registry::tools()).unwrap(),
        )),
        ("GET", ["mcp", "resources"]) => Some((
            200,
            serde_json::to_value(crate::tool_registry::resources()).unwrap(),
        )),
        ("GET", ["mcp", "prompts"]) => Some((
            200,
            serde_json::to_value(crate::tool_registry::prompts()).unwrap(),
        )),
        ("GET", ["mcp", "instructions"]) => Some((
            200,
            json!({"instructions": crate::tool_registry::mcp_instructions()}),
        )),
        // Static listing of the registered agent types (SPEC 7.5). Sits under `/agents/`, so the
        // router's session gate for agent APIs applies to it.
        ("GET", ["agents", "types"]) => Some((200, crate::tui::agent_types_listing())),
        _ => None,
    }
}

/// `GET /agents/types` body. The listing is readable without a session (the phone's "new agent"
/// sheet), but a definition's absolute `path` can reveal the user name, so only the desktop app
/// itself (no MCP session, local peer) gets it; everyone else gets the fingerprint and state only.
/// The body of `GET /agents/types` for this caller: the full listing only for the desktop app
/// itself (no MCP session, local peer, no device token).
pub(in crate::bridge) fn types_route(session: Option<&str>, headers: &str, peer_loopback: bool) -> serde_json::Value {
    let app_session = session.is_none_or(|s| s.trim().is_empty())
        && super::definitions::is_local_app(headers, peer_loopback);
    agent_types_for(app_session)
}

pub(in crate::bridge) fn agent_types_for(app_session: bool) -> serde_json::Value {
    redact_definition_paths(crate::tui::agent_types_listing(), app_session)
}

pub(in crate::bridge) fn redact_definition_paths(mut listing: serde_json::Value, keep: bool) -> serde_json::Value {
    if keep {
        return listing;
    }
    if let Some(agents) = listing.get_mut("agents").and_then(serde_json::Value::as_array_mut) {
        for agent in agents {
            if let Some(definition) = agent.get_mut("definition").and_then(serde_json::Value::as_object_mut) {
                definition.remove("path");
            }
        }
    }
    listing
}

fn tools(cx: &Ctx) -> RouteResult {
    let session = cx.session;
    let tools = session
        .map(crate::mcp_sessions::tools_for_session)
        .unwrap_or_else(crate::tool_registry::tools);
    Ok((
        200,
        json!({"tools": tools, "catalog_version": crate::tool_registry::catalog_version()}),
    ))
}

fn session_identity(cx: &Ctx) -> RouteResult {
    let session = cx.session;
    let session = session.ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "SESSION_REQUIRED",
            "an MCP session is required",
            false,
        ))
    })?;
    let identity = crate::mcp_sessions::session_identity(session);
    Ok((
        200,
        json!({
            "connection_id": identity.connection_id,
            "coordinator_id": identity.coordinator_id,
            "attach_token": identity.attach_token,
        }),
    ))
}

fn set_mode(cx: &Ctx) -> RouteResult {
    let session = cx.session;
    let request_body = &cx.request_body;
    let session = session.ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "SESSION_REQUIRED",
            "an MCP session is required",
            false,
        ))
    })?;
    let mode = request_body
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let tools = crate::mcp_sessions::set_mode(session, mode).map_err(operation_http_error)?;
    Ok((200, json!({"mode":mode,"tool_count":tools.len()})))
}

fn handoff(cx: &Ctx, release: bool) -> RouteResult {
    handoff_route(cx.session, &cx.request_body, cx.registry, release)
        .map_err(|error| enrich_handle_error(error, cx.registry))
        .map(|value| (200, value))
        .map_err(operation_http_error)
}

fn attach(cx: &Ctx) -> RouteResult {
    let session = cx.session;
    let request_body = &cx.request_body;
    let session = session.ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "SESSION_REQUIRED",
            "attach requires an MCP connection",
            false,
        ))
    })?;
    crate::mcp_sessions::attach_coordinator(
        session,
        request_body
            .get("attach_token")
            .and_then(serde_json::Value::as_str),
        request_body
            .get("coordinator_name")
            .and_then(serde_json::Value::as_str),
    )
    .map(|value| (200, value))
    .map_err(operation_http_error)
}

fn release_lease(cx: &Ctx) -> RouteResult {
    let session = cx.session;
    let request_body = &cx.request_body;
    let session = session.ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "SESSION_REQUIRED",
            "release_lease requires an MCP connection",
            false,
        ))
    })?;
    let handle = request_body
        .get("handle")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "INVALID_ARGUMENT",
                "handle is required",
                false,
            ))
        })?;
    crate::mcp_sessions::release_agent_lease(session, handle)
        .map(|()| (200, json!({"handle": handle, "lease": "released"})))
        .map_err(operation_http_error)
}

fn transfer(cx: &Ctx) -> RouteResult {
    let session = cx.session;
    let request_body = &cx.request_body;
    let session = session.ok_or_else(|| {
        operation_http_error(crate::operations::OperationError::new(
            "SESSION_REQUIRED",
            "transfer_agent requires an MCP connection",
            false,
        ))
    })?;
    let handle = request_body
        .get("handle")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            operation_http_error(crate::operations::OperationError::new(
                "INVALID_ARGUMENT",
                "handle is required",
                false,
            ))
        })?;
    crate::mcp_sessions::transfer_agent(
        session,
        handle,
        request_body
            .get("to_session_id")
            .and_then(serde_json::Value::as_str),
        request_body
            .get("coordinator_name")
            .and_then(serde_json::Value::as_str),
    )
    .map(|value| (200, value))
    .map_err(operation_http_error)
}
