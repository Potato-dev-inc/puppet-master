//! Session-context routes: goal, pane roles and digests, delegation prompts, and the
//! orchestrator standby policy plus the mobile orchestrator message/viewport relays.

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;

use super::{Ctx, RouteResult};
use crate::bridge::http::{normalize_optional_string, parse_json};
use crate::bridge::sse::push_sse;
use crate::bridge::store::rebuild_read_models;
use crate::events::{SystemEvent, TaskId};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OrchestratorMessageBody {
    pub text: String,
    pub message_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OrchestratorViewportBody {
    pub width: f64,
    pub height: f64,
    pub active: bool,
}

#[derive(Debug, Deserialize)]
struct SessionContextPatchBody {
    current_goal: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetPaneRoleBody {
    role: crate::session_context::PaneRole,
}

#[derive(Debug, Deserialize)]
struct PaneDigestBody {
    summary: String,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrchestratorStatePatchBody {
    standby_poll_ms: Option<u64>,
    standby_max_ms: Option<u64>,
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["session", "context"]) => context_get(),
        ("PATCH", ["session", "context"]) => context_patch(cx),
        ("POST", ["delegate-task"]) => delegate_task(cx),
        ("GET", ["orchestrator", "state"]) => orchestrator_state_get(),
        ("PATCH", ["orchestrator", "state"]) => orchestrator_state_patch(cx),
        ("POST", ["orchestrator", "message"]) => orchestrator_message(cx),
        ("POST", ["orchestrator", "viewport"]) => orchestrator_viewport(cx),
        _ => return None,
    })
}

fn context_get() -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.session).unwrap()))
}

fn context_patch(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req: SessionContextPatchBody = parse_json(body)?;
    crate::event_log::append_bridge_event(SystemEvent::SessionGoalUpdated {
        current_goal: normalize_optional_string(req.current_goal),
    });
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.session).unwrap()))
}

fn delegate_task(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req = parse_json::<crate::session_context::DelegateTaskRequest>(body)?
        .validated()
        .map_err(|err| (400, json!({ "error": err })))?;
    let prompt = crate::session_context::render_codex_delegation_prompt(&req);
    crate::event_log::append_bridge_event(SystemEvent::DelegationPrepared {
        task_id: req.task_id.clone().map(TaskId),
        target_pane_id: req.target_pane_id.clone().map(crate::events::PaneId),
        intent: req.intent.clone(),
    });
    let mut payload = json!({
        "task_id": req.task_id,
        "target_pane_id": req.target_pane_id,
        "prompt": prompt,
    });
    if let Some(pane_id) = req.target_pane_id.as_deref() {
        return Ok((
            200,
            crate::mcp_hints::mutate_ok(
                payload,
                crate::mcp_hints::suggested_wait_after_delegate(pane_id),
            ),
        ));
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("ok".into(), json!(true));
    }
    Ok((200, payload))
}

fn orchestrator_state_get() -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((
        200,
        serde_json::to_value(read_models.session.orchestrator).unwrap(),
    ))
}

fn orchestrator_state_patch(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req: OrchestratorStatePatchBody = parse_json(body)?;
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    let current = read_models.session.orchestrator;
    let standby_poll_ms = req
        .standby_poll_ms
        .unwrap_or(current.standby_poll_ms)
        .max(1);
    let standby_max_ms = req.standby_max_ms.unwrap_or(current.standby_max_ms).max(1);
    crate::event_log::append_bridge_event(SystemEvent::OrchestratorStandbyPolicyUpdated {
        standby_poll_ms,
        standby_max_ms,
    });
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((
        200,
        serde_json::to_value(read_models.session.orchestrator).unwrap(),
    ))
}

fn orchestrator_message(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let app = cx.app;
    let req: OrchestratorMessageBody = parse_json(body)?;
    let user_event = json!({
        "type": "user",
        "message_id": req.message_id,
        "text": req.text,
    });
    push_sse(format!("event: chat\ndata: {user_event}\n\n"));
    let _ = app.emit("orchestrator://message", req);
    Ok((200, json!({ "ok": true })))
}

fn orchestrator_viewport(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req: OrchestratorViewportBody = parse_json(body)?;
    if let Ok(json) = serde_json::to_string(&req) {
        push_sse(format!("event: orchestrator-viewport\ndata: {json}\n\n"));
    }
    Ok((200, json!({ "ok": true })))
}

pub(super) fn set_pane_role(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let req: SetPaneRoleBody = parse_json(body)?;
    crate::event_log::append_bridge_event(SystemEvent::PaneRoleSet {
        pane_id: crate::events::PaneId(pane_id.to_string()),
        role: req.role,
    });
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.session).unwrap()))
}

pub(super) fn pane_digest_get(_cx: &Ctx, pane_id: &str) -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    let digest = read_models
        .session
        .pane_digests
        .get(pane_id)
        .ok_or_else(|| (404, json!({ "error": "pane digest not found" })))?;
    Ok((200, serde_json::to_value(digest).unwrap()))
}

pub(super) fn pane_digest_set(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let req: PaneDigestBody = parse_json(body)?;
    let summary = req.summary.trim();
    if summary.is_empty() {
        return Err((400, json!({ "error": "summary is required" })));
    }
    crate::event_log::append_bridge_event(SystemEvent::PaneDigestUpdated {
        pane_id: crate::events::PaneId(pane_id.to_string()),
        summary: summary.to_string(),
        source: normalize_optional_string(req.source).unwrap_or_else(|| "manual".to_string()),
    });
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    let digest = read_models
        .session
        .pane_digests
        .get(pane_id)
        .ok_or_else(|| (500, json!({ "error": "pane digest projection missing" })))?;
    Ok((200, serde_json::to_value(digest).unwrap()))
}
