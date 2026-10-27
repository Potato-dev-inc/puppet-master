//! `/panes` routes: list, spawn, kill, read buffers, write input, press keys, detach, resize,
//! model switching and agent context. Role, digest and OpenCode sub-routes delegate to their
//! own groups.

use serde::Deserialize;
use serde_json::json;
use tauri::Emitter;

use super::{opencode, session, Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;
use crate::bridge::http::{parse_json, query_param};
use crate::bridge::pane_util::{
    emit_panes_changed, pane_buffer_view, present_spawned_pane, register_spawned_pane,
};
use crate::pty::{
    registry_kill_pane, registry_read_buffer, registry_read_raw_buffer, registry_read_screen_replay_at, registry_read_snapshot,
    registry_spawn_pane, registry_write_input, SpawnPaneArgs,
};
use crate::settings_store;

#[derive(Debug, Deserialize)]
struct WriteInputBody {
    text: String,
    #[serde(default = "default_true")]
    append_newline: bool,
    #[serde(default)]
    via_opencode_api: bool,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SwitchModelBody {
    #[serde(default)]
    model_provider: Option<String>,
    model_id: String,
}

#[derive(Debug, Deserialize)]
struct PressKeyBody {
    key: String,
}

/// `POST /panes/{id}/viewport`: the phone's grid size while it shows the pane (`active`), or
/// `active: false` when it stops (see `pty::viewer_size`).
#[derive(Debug, Deserialize)]
struct ViewportBody {
    #[serde(default)]
    cols: u16,
    #[serde(default)]
    rows: u16,
    active: bool,
    /// Which phone (one id per open page); several phones share the smallest size.
    #[serde(default = "default_viewer")]
    viewer: String,
    /// Renew only: extend this phone's lease if it holds one; never claim, never resize.
    #[serde(default)]
    renew_only: bool,
}

fn default_viewer() -> String {
    "phone".to_string()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ResizeBody {
    cols: u16,
    rows: u16,
}
fn default_true() -> bool {
    true
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("GET", ["panes"]) => return Some(list(cx)),
        ("POST", ["panes"]) => return Some(spawn(cx)),
        ("POST", ["panes", "stop-all"]) => return Some(stop_all(cx)),
        _ => {}
    }
    let ["panes", pane_id, rest @ ..] = cx.segments else {
        return None;
    };
    let pane_id: &str = pane_id;
    if let Some(response) = registry_route(cx.method, cx.segments, cx.registry) {
        return Some(response);
    }
    if let Some(response) = opencode::handle_pane(cx, pane_id) {
        return Some(response);
    }
    Some(match (cx.method, rest) {
        ("DELETE", []) => kill(cx, pane_id),
        ("GET", ["buffer", ..]) => buffer(cx, pane_id),
        ("GET", ["raw", ..]) => raw(cx, pane_id),
        ("GET", ["snapshot", ..]) => snapshot(cx, pane_id),
        ("POST", ["input", ..]) => input(cx, pane_id),
        ("POST", ["messages"]) => message_send(cx, pane_id),
        ("GET", ["messages"]) => messages_list(cx, pane_id),
        ("DELETE", ["messages", message_id]) => message_cancel(cx, pane_id, message_id),
        ("POST", ["key", ..]) => key(cx, pane_id),
        ("POST", ["detach", ..]) => detach(cx, pane_id),
        ("POST", ["role", ..]) => session::set_pane_role(cx, pane_id),
        ("GET", ["digest", ..]) => session::pane_digest_get(cx, pane_id),
        ("POST", ["digest", ..]) => session::pane_digest_set(cx, pane_id),
        ("POST", ["resize", ..]) => resize(cx),
        ("POST", ["viewport"]) => viewport(cx, pane_id),
        ("POST", ["model", ..]) => switch_model(cx, pane_id),
        ("GET", ["model", ..]) => inspect_model(cx, pane_id),
        ("GET", ["agent-context", ..]) => agent_context(cx, pane_id),
        ("GET", ["chat"]) => super::chat::get(cx, pane_id),
        _ => return None,
    })
}

/// Panic button for the phone: interrupt every running agent pane. Authorised like every other
/// bridge route (loopback app, or a paired device's bearer token); MCP sessions are refused because
/// the route is not part of the tool catalog.
fn stop_all(cx: &Ctx) -> RouteResult {
    let report = crate::panic::stop_all(cx.registry, cx.app);
    Ok((200, serde_json::to_value(report).unwrap_or_default()))
}

fn list(cx: &Ctx) -> RouteResult {
    let registry = cx.registry;
    let panes = registry.lock().list();
    Ok((200, serde_json::to_value(panes).unwrap()))
}

/// Second path segments under `/panes` that are routes, not panes (`/panes/wait`, `/panes/stop-all`).
/// A pane with one of these ids would make its own routes ambiguous (review R12 2.2).
pub(crate) fn is_reserved_pane_id(id: &str) -> bool {
    matches!(id.trim(), "wait" | "stop-all")
}

#[cfg(test)]
#[test]
fn route_names_are_not_pane_ids() {
    assert!(is_reserved_pane_id("wait") && is_reserved_pane_id("stop-all"));
    assert!(!is_reserved_pane_id("waiter") && !is_reserved_pane_id("p-1"));
}

fn spawn(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let session = cx.session;
    let req: SpawnPaneArgs = parse_json(body)?;
    if let Some(id) = req.pane_id.as_deref().filter(|id| is_reserved_pane_id(id)) {
        return Err((400, json!({ "error": format!("pane id {id:?} is reserved: it names a bridge route under /panes"), "code": "INVALID_ARGUMENT" })));
    }
    crate::tui::require_registered(&req.agent_type).map_err(|unknown| {
        operation_http_error(crate::operations::OperationError::unknown_agent_type(
            unknown.id,
            unknown.valid,
        ))
    })?;
    let agent_type = req.agent_type.clone();
    let extra = serde_json::from_slice::<serde_json::Value>(body).ok();
    let placement = extra
        .as_ref()
        .and_then(|v| v.get("placement").and_then(|p| p.as_str().map(str::to_string)));
    // A phone spawning a pane (`viewer`) holds its size from the start: claimed before the pane
    // window opens, so the window's own fit is remembered instead of resizing the new agent once
    // (each resize makes Claude Code reprint).
    let viewer = extra
        .as_ref()
        .and_then(|v| v.get("viewer").and_then(|p| p.as_str().map(str::to_string)));
    let phone_size = req.cols.zip(req.rows);
    let pane_id =
        registry_spawn_pane(registry, app, req).map_err(|err| (500, json!({ "error": err })))?;
    if let (Some(viewer), Some((cols, rows))) = (viewer.as_deref(), phone_size) {
        let _ = crate::pty::viewer_size::claim(registry, &pane_id, viewer, cols, rows);
    }
    register_spawned_pane(session, &agent_type, &pane_id).map_err(operation_http_error)?;
    present_spawned_pane(registry, app, &pane_id, placement.as_deref());
    Ok((
        201,
        crate::mcp_hints::mutate_ok(
            json!({ "pane_id": pane_id }),
            crate::mcp_hints::suggested_wait_after_spawn(&pane_id),
        ),
    ))
}

fn kill(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let app = cx.app;
    registry_kill_pane(registry, pane_id).map_err(|err| (500, json!({ "error": err })))?;
    emit_panes_changed(registry, app);
    Ok((200, json!({ "ok": true })))
}

fn buffer(cx: &Ctx, pane_id: &str) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    let lines = query_param(query, "lines")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200);
    let requested_view = query_param(query, "view");
    let agent_type = registry
        .lock()
        .panes
        .get(pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| {
            (
                404,
                json!({ "error": crate::pty::registry::unknown_pane_message(pane_id) }),
            )
        })?;
    let requested_view = requested_view.as_deref();
    let view = pane_buffer_view(requested_view, &agent_type).map_err(|message| {
        (
            400,
            json!({"error":message,"allowed":["screen","scrollback"]}),
        )
    })?;
    let content = if view == "screen" {
        registry_read_snapshot(registry, pane_id)
    } else {
        registry_read_buffer(registry, pane_id, lines)
    }
    .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, json!({ "content": content, "view": view })))
}

fn raw(cx: &Ctx, pane_id: &str) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    let lines = query_param(query, "lines")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10_000);
    let raw = registry_read_raw_buffer(registry, pane_id, lines)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, json!({ "data": raw })))
}

/// The pane redrawn at its current size (scrollback + screen) for a viewer to replay, base64, and
/// the stream `offset` it was taken at (read under the emulator lock; `terminal` and `pane-resize`
/// events carry offsets of the same numbering). `offset` is absent for a pane that does not number
/// its stream: the viewer then falls back to dropping what arrived while the screen loaded.
pub(super) fn screen_response(registry: &parking_lot::Mutex<crate::pty::PaneRegistry>, pane_id: &str) -> RouteResult {
    use base64::Engine as _;
    let (bytes, cols, rows, offset) = registry_read_screen_replay_at(registry, pane_id)
        .map_err(|err| (404, json!({ "error": err })))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut body = json!({ "b64": b64, "cols": cols, "rows": rows });
    if let Some(offset) = offset {
        body["offset"] = json!(offset);
    }
    Ok((200, body))
}

/// The routes that need only the pane registry (no app handle), so they run in-process in tests.
pub(in crate::bridge) fn registry_route(
    method: &str,
    segments: &[&str],
    registry: &parking_lot::Mutex<crate::pty::PaneRegistry>,
) -> Option<RouteResult> {
    match (method, segments) {
        ("GET", ["panes", pane_id, "screen", ..]) => Some(screen_response(registry, pane_id)),
        // Closing a pane that is not there is a 404 like every other pane route, not a silent
        // `{"ok":true}` (a typo'd or stale id looked like a success). A known pane falls through to
        // `kill`, which needs the app handle.
        ("DELETE", ["panes", pane_id]) if !registry.lock().panes.contains_key(*pane_id) => {
            Some(Err(operation_http_error(crate::operations::OperationError::new(
                "PANE_NOT_FOUND",
                crate::pty::registry::unknown_pane_message(pane_id),
                false,
            ))))
        }
        _ => None,
    }
}


fn snapshot(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let content =
        registry_read_snapshot(registry, pane_id).map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, json!({ "content": content })))
}

fn input(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let req: WriteInputBody = parse_json(body)?;
    let settings = settings_store::read_public_settings(app);
    let model = crate::opencode::resolve_model(
        req.model_provider.as_deref(),
        req.model_id.as_deref(),
        &settings,
    );
    if !req.via_opencode_api {
        raw_keys_gate(cx, pane_id, &format!("{} characters{}", req.text.chars().count(), if req.append_newline { " and Enter" } else { "" }))?;
    }
    // The origin says "an API caller"; the flight recorder also wants which one (agent, phone, app).
    crate::flight::with_tag(bridge_tag(cx), || {
        registry_write_input(
            registry,
            app,
            pane_id,
            &req.text,
            req.append_newline,
            req.via_opencode_api,
            model,
            crate::pty::WriteOrigin::Api,
        )
    })
    .map_err(|err| (404, json!({ "error": err })))?;
    Ok((
        200,
        crate::mcp_hints::mutate_ok(
            json!({ "pane_id": pane_id, "written": true }),
            crate::mcp_hints::suggested_wait_after_write(pane_id),
        ),
    ))
}

/// `POST /panes/{id}/messages` `{ text }`: a message for the pane's agent, sent when the agent is ready (the pane
/// outbox, `pane_outbox`). The text in every reply is redacted: this route is the phone's.
fn message_send(cx: &Ctx, pane_id: &str) -> RouteResult {
    #[derive(Deserialize)]
    struct Body {
        text: String,
    }
    let req: Body = parse_json(cx.body)?;
    let message = crate::pane_outbox::global(cx.registry, cx.app)
        .send(pane_id, &req.text)
        .map_err(|error| (400, json!({ "error": error, "code": "MESSAGE_REFUSED" })))?;
    Ok((200, json!({ "message": crate::pane_outbox::redacted(&message) })))
}

/// `GET /panes/{id}/messages`: the pane's waiting messages and those that finished in the last two minutes.
fn messages_list(cx: &Ctx, pane_id: &str) -> RouteResult {
    let messages: Vec<_> = crate::pane_outbox::global(cx.registry, cx.app)
        .list(pane_id)
        .iter()
        .map(crate::pane_outbox::redacted)
        .collect();
    Ok((200, json!({ "messages": messages })))
}

/// `DELETE /panes/{id}/messages/{messageId}`: cancels a message that has not started to be typed.
fn message_cancel(cx: &Ctx, pane_id: &str, message_id: &str) -> RouteResult {
    let message = crate::pane_outbox::global(cx.registry, cx.app)
        .cancel(pane_id, message_id)
        .map_err(|error| (409, json!({ "error": error, "code": "MESSAGE_NOT_CANCELLABLE" })))?;
    Ok((200, json!({ "message": crate::pane_outbox::redacted(&message) })))
}

/// An agent's raw keys into a pane that shows a dialog answer it: the answer gate and an audit
/// line (`agent_authority::gate::check_raw_keys`). The person's keys and a pane with no dialog
/// pass untouched.
fn raw_keys_gate(cx: &Ctx, pane_id: &str, keys: &str) -> Result<(), (u16, serde_json::Value)> {
    let Some(session) = cx.session.filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let Some(prompt) = crate::pane_prompt::detect(cx.registry, pane_id, false) else {
        return Ok(());
    };
    crate::agent_authority::gate::check_raw_keys(Some(session), pane_id, &prompt, keys, Some(cx.registry))
        .map_err(crate::bridge::errors::operation_http_error)
}

/// Who is typing: an agent (MCP session), a paired phone, or the local app (flight recorder tag).
fn bridge_tag(cx: &Ctx) -> crate::flight::InputTag {
    let paired_phone = cx.session.is_none() && crate::mobile_pairing::requires_pairing_token(cx.headers, cx.peer_loopback);
    crate::flight::InputTag::for_bridge(cx.session.is_some(), paired_phone)
}

fn key(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let req: PressKeyBody = parse_json(body)?;
    let seq = crate::pty::keys::sequence(&req.key).map_err(|err| (400, json!({ "error": err })))?;
    let bytes = seq.len();
    raw_keys_gate(cx, pane_id, &format!("the {} key", req.key))?;
    crate::flight::with_tag(bridge_tag(cx), || registry_write_input(registry, app, pane_id, &seq, false, false, None, crate::pty::WriteOrigin::Api))
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, json!({ "ok": true, "key": req.key, "bytes": bytes })))
}

fn detach(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let app = cx.app;
    let pane = registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id)
        .ok_or_else(|| {
            (
                404,
                json!({ "error": crate::pty::registry::unknown_pane_message(pane_id) }),
            )
        })?;
    match crate::ui_mode::current() {
        crate::ui_mode::UiMode::Individual => {
            crate::app_windows::open_pane_window(app, &pane.id, &pane.agent_type, pane.cols, pane.rows, false)
        }
        // Orchestration has no pop-outs: show the pane in the cockpit without stealing focus.
        crate::ui_mode::UiMode::Orchestration => crate::app_windows::open_cockpit_window(app, Some(&pane.id), false),
    }
    Ok((200, json!({ "ok": true })))
}

fn resize(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    // Remote bridge clients must not resize the shared PTY — only desktop does.
    let _ = parse_json::<ResizeBody>(body)?;
    Ok((200, json!({ "ok": true, "ignored": true })))
}

fn viewport(cx: &Ctx, pane_id: &str) -> RouteResult {
    let req: ViewportBody = parse_json(cx.body)?;
    if !req.active {
        let applied = crate::pty::viewer_size::release(cx.registry, pane_id, &req.viewer);
        return Ok((200, json!({ "ok": true, "leased": false, "held": false, "restored": applied.map(|(cols, rows)| json!({ "cols": cols, "rows": rows })) })));
    }
    if req.renew_only {
        let held = crate::pty::viewer_size::renew(pane_id, &req.viewer);
        return Ok((200, json!({ "ok": true, "leased": held, "held": held })));
    }
    let (cols, rows) = crate::pty::viewer_size::claim(cx.registry, pane_id, &req.viewer, req.cols, req.rows)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, json!({ "ok": true, "leased": true, "held": true, "cols": cols, "rows": rows })))
}

fn switch_model(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let session = cx.session;
    let req: SwitchModelBody = parse_json(body)?;
    if let Some(error) = crate::agent_runs::pane_close::closed_after_run_error(
        &crate::pty::registry::get_project_path(registry),
        registry,
        pane_id,
    ) {
        return Err(operation_http_error(error));
    }
    if let Some(session) = session {
        if !crate::mcp_sessions::session_controls_pane(session, pane_id) {
            let project = crate::pty::registry::get_project_path(registry);
            if let Ok(handle) = crate::agent_runs::latest_handle_for_pane(&project, pane_id) {
                if crate::mcp_sessions::check_run_access(session, &handle).is_ok() {
                    let _ = crate::mcp_sessions::take_over_pane(session, pane_id, true);
                }
            }
        }
    }
    let settings = settings_store::read_public_settings(app);
    let model = crate::opencode::resolve_model(
        req.model_provider.as_deref(),
        Some(req.model_id.as_str()),
        &settings,
    )
    .ok_or_else(|| (400, json!({ "error": "model_id required" })))?;
    crate::opencode::switch_native_model(registry, app, pane_id, &model)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((
        200,
        crate::opencode::switch_model_response(registry, pane_id, &model),
    ))
}

fn inspect_model(cx: &Ctx, pane_id: &str) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    if let Some(error) = crate::agent_runs::pane_close::closed_after_run_error(
        &crate::pty::registry::get_project_path(registry),
        registry,
        pane_id,
    ) {
        return Err(operation_http_error(error));
    }
    let lines = query_param(query, "lines")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200);
    let pane = registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id)
        .ok_or_else(|| {
            (
                404,
                json!({ "error": crate::pty::registry::unknown_pane_message(pane_id) }),
            )
        })?;
    let agent_type = match crate::agent_contexts::registered_context_agent(&pane.agent_type) {
        Ok(Some(agent_type)) => agent_type,
        Ok(None) => {
            return Err((
                400,
                json!({ "error": format!("no model inspection for agent_type: {}", pane.agent_type) }),
            ));
        }
        Err(unknown) => {
            return Err(operation_http_error(
                crate::operations::OperationError::unknown_agent_type(unknown.id, unknown.valid),
            ));
        }
    };
    let buffer = registry_read_buffer(registry, pane_id, lines)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((
        200,
        serde_json::to_value(crate::agent_contexts::inspect_agent_model_with_registry(
            Some(registry),
            pane_id,
            agent_type,
            &buffer,
        ))
        .unwrap(),
    ))
}

fn agent_context(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let panes = registry.lock().list();
    let pane = panes.iter().find(|pane| pane.id == pane_id);
    if let Some(pane) = pane {
        let buffer = registry_read_buffer(registry, pane_id, 200)
            .map_err(|err| (404, json!({ "error": err })))?;
        let context =
            crate::agent_contexts::build_pane_agent_context(Some(registry), pane.clone(), &buffer)
                .ok_or_else(|| (400, json!({ "error": "unknown pane agent_type" })))?;
        return Ok((200, serde_json::to_value(context).unwrap()));
    }
    Err((
        404,
        json!({ "error": crate::pty::registry::unknown_pane_message(pane_id) }),
    ))
}
