//! OpenCode routes: API key status/settings/rotation, worker status, and the per-pane native
//! session endpoints (status, messages, permissions, questions).

use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use super::{Ctx, RouteResult};
use crate::bridge::http::normalize_optional_string;
use crate::bridge::http::{parse_json, query_param};
use crate::bridge::pane_util::resolve_default_opencode_pane_id;

#[derive(Debug, Deserialize)]
struct OpenCodePermissionReplyBody {
    reply: String,
}

#[derive(Debug, Deserialize)]
struct OpenCodeQuestionReplyBody {
    answer: String,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeKeyRotateBody {
    #[serde(default)]
    profile: Option<String>,
    pane_id: Option<String>,
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["opencode", "keys", "status"]) => keys_status(),
        ("GET", ["opencode", "worker-status"]) => worker_status(cx),
        ("PATCH" | "POST", ["opencode", "keys", "settings"]) => keys_settings(cx),
        ("POST", ["opencode", "keys", "rotate"]) => keys_rotate(cx),
        _ => return None,
    })
}

/// Per-pane routes under `/panes/<id>/opencode`; `None` when the path is not one of them.
pub(super) fn handle_pane(cx: &Ctx, pane_id: &str) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", [_, _, "opencode", "status"]) => pane_status(cx, pane_id),
        ("GET", [_, _, "opencode", "messages"]) => pane_messages(cx, pane_id),
        ("GET", [_, _, "opencode"]) => pane_link(cx, pane_id),
        ("GET", [_, _, "opencode", "permissions"]) => pane_permissions(cx, pane_id),
        ("POST", [_, _, "opencode", "permissions", request_id, "reply"]) => {
            permission_reply(cx, pane_id, request_id)
        }
        ("POST", [_, _, "opencode", "question", "reply"]) => question_reply(cx, pane_id),
        _ => return None,
    })
}

fn keys_status() -> RouteResult {
    let status = crate::opencode::keys::status().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(status).unwrap()))
}

fn worker_status(cx: &Ctx) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    let pane_id = query_param(query, "pane_id")
        .or_else(|| query_param(query, "worker_id"))
        .or_else(|| resolve_default_opencode_pane_id(registry));
    let pane_id = pane_id.ok_or_else(|| {
        (
            400,
            json!({
                "error": "pane_id is required when more than one opencode_native pane is open",
                "code": "PANE_ID_REQUIRED",
            }),
        )
    })?;
    let status = crate::opencode::status::worker_status(registry, &pane_id)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(status).unwrap()))
}

fn keys_settings(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let patch: crate::opencode::keys::OpenCodeKeySettingsPatch = parse_json(body)?;
    let status = crate::opencode::keys::patch_settings(&patch)
        .map_err(|err| (400, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(status).unwrap()))
}

fn keys_rotate(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let req: OpenCodeKeyRotateBody = parse_json(body)?;
    let profile = req.profile.unwrap_or_else(|| "next".to_string());
    let target = crate::opencode::keys::RotateTarget::parse(&profile)
        .ok_or_else(|| (400, json!({ "error": "profile must be next, a, or b" })))?;
    let status =
        crate::opencode::keys::rotate(target).map_err(|err| (400, json!({ "error": err })))?;
    let restarted = if let Some(pane_id) = normalize_optional_string(req.pane_id) {
        vec![
            crate::opencode::native::restart_native_pane(Arc::clone(registry), app, &pane_id)
                .map_err(|err| (404, json!({ "error": err })))?,
        ]
    } else {
        crate::opencode::native::restart_all_native_panes(Arc::clone(registry), app)
            .map_err(|err| (500, json!({ "error": err })))?
    };
    Ok((
        200,
        json!({
            "ok": true,
            "active_profile": status.active_profile,
            "profiles": status.profiles,
            "restarted_panes": restarted,
        }),
    ))
}

fn pane_status(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let status = crate::opencode::status::worker_status(registry, pane_id)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(status).unwrap()))
}

fn pane_messages(cx: &Ctx, pane_id: &str) -> RouteResult {
    let query = cx.query;
    let registry = cx.registry;
    let limit = query_param(query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 200);
    let role = query_param(query, "role");
    let view =
        crate::opencode::messages::read_pane_messages(registry, pane_id, limit, role.as_deref())
            .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(view).unwrap()))
}

fn pane_link(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let link = crate::opencode::pane_link(registry, pane_id).ok_or_else(|| {
        (
            404,
            json!({ "error": format!("pane {pane_id} has no opencode native session") }),
        )
    })?;
    Ok((200, serde_json::to_value(link).unwrap()))
}

fn pane_permissions(cx: &Ctx, pane_id: &str) -> RouteResult {
    let registry = cx.registry;
    let permissions = crate::opencode::list_pane_permissions(registry, pane_id)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(permissions).unwrap()))
}

fn permission_reply(cx: &Ctx, pane_id: &str, request_id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: OpenCodePermissionReplyBody = parse_json(body)?;
    crate::opencode::reply_pane_permission(registry, pane_id, request_id, &req.reply)
        .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, json!({ "ok": true })))
}

fn question_reply(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: OpenCodeQuestionReplyBody = parse_json(body)?;
    let result = crate::opencode::reply_pane_question(
        registry,
        pane_id,
        &req.answer,
        req.request_id.as_deref(),
    )
    .map_err(|err| (404, json!({ "error": err })))?;
    Ok((200, result))
}
