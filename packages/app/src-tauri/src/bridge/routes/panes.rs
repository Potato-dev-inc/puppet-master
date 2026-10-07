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
use crate::pty::agents::AgentType;
use crate::pty::{
    registry_kill_pane, registry_read_buffer, registry_read_raw_buffer, registry_read_snapshot,
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
        _ => {}
    }
    let ["panes", pane_id, rest @ ..] = cx.segments else {
        return None;
    };
    let pane_id: &str = pane_id;
    if let Some(response) = opencode::handle_pane(cx, pane_id) {
        return Some(response);
    }
    Some(match (cx.method, rest) {
        ("DELETE", []) => kill(cx, pane_id),
        ("GET", ["buffer", ..]) => buffer(cx, pane_id),
        ("GET", ["raw", ..]) => raw(cx, pane_id),
        ("GET", ["snapshot", ..]) => snapshot(cx, pane_id),
        ("POST", ["input", ..]) => input(cx, pane_id),
        ("POST", ["key", ..]) => key(cx, pane_id),
        ("POST", ["detach", ..]) => detach(cx, pane_id),
        ("POST", ["role", ..]) => session::set_pane_role(cx, pane_id),
        ("GET", ["digest", ..]) => session::pane_digest_get(cx, pane_id),
        ("POST", ["digest", ..]) => session::pane_digest_set(cx, pane_id),
        ("POST", ["resize", ..]) => resize(cx),
        ("POST", ["model", ..]) => switch_model(cx, pane_id),
        ("GET", ["model", ..]) => inspect_model(cx, pane_id),
        ("GET", ["agent-context", ..]) => agent_context(cx, pane_id),
        _ => return None,
    })
}

fn list(cx: &Ctx) -> RouteResult {
    let registry = cx.registry;
    let panes = registry.lock().list();
    Ok((200, serde_json::to_value(panes).unwrap()))
}

fn spawn(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let session = cx.session;
    let req: SpawnPaneArgs = parse_json(body)?;
    crate::tui::require_registered(&req.agent_type).map_err(|unknown| {
        operation_http_error(crate::operations::OperationError::unknown_agent_type(
            unknown.id,
            unknown.valid,
        ))
    })?;
    let agent_type = req.agent_type.clone();
    let pane_id =
        registry_spawn_pane(registry, app, req).map_err(|err| (500, json!({ "error": err })))?;
    register_spawned_pane(session, &agent_type, &pane_id).map_err(operation_http_error)?;
    present_spawned_pane(registry, app, &pane_id);
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
    registry_write_input(
        registry,
        app,
        pane_id,
        &req.text,
        req.append_newline,
        req.via_opencode_api,
        model,
    )
    .map_err(|err| (404, json!({ "error": err })))?;
    Ok((
        200,
        crate::mcp_hints::mutate_ok(
            json!({ "pane_id": pane_id, "written": true }),
            crate::mcp_hints::suggested_wait_after_write(pane_id),
        ),
    ))
}

fn key(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let req: PressKeyBody = parse_json(body)?;
    let seq = crate::pty::keys::sequence(&req.key).map_err(|err| (400, json!({ "error": err })))?;
    let bytes = seq.len();
    registry_write_input(registry, app, pane_id, &seq, false, false, None)
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
    let title = format!(
        "{} · {}",
        pane.agent_type,
        pane.id.chars().take(8).collect::<String>()
    );
    let _ = app.emit(
        "pane://detach",
        json!({
            "pane_id": pane.id,
            "title": title,
            "cols": pane.cols,
            "rows": pane.rows,
        }),
    );
    Ok((200, json!({ "ok": true })))
}

fn resize(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    // Remote bridge clients must not resize the shared PTY — only desktop does.
    let _ = parse_json::<ResizeBody>(body)?;
    Ok((200, json!({ "ok": true, "ignored": true })))
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
    let agent_type = match crate::tui::require_registered(&pane.agent_type) {
        Ok(profile) => AgentType::parse(profile.id()).expect("registered ids parse as AgentType"),
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
