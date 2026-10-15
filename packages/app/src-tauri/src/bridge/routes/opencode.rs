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
    /// An agent replying `always` must opt in, as with `answer_prompt` (`BROAD_APPROVAL_REQUIRES_OPT_IN`).
    #[serde(default)]
    allow_broad: bool,
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
    let req: OpenCodePermissionReplyBody = parse_json(cx.body)?;
    let session = cx.session.filter(|s| !s.trim().is_empty());
    gate_reply(session, cx.registry, pane_id, request_id, Some(&req.reply), req.allow_broad)
        .map_err(crate::bridge::errors::operation_http_error)?;
    crate::opencode::reply_pane_permission(cx.registry, pane_id, request_id, &req.reply)
        .map_err(|err| (404, json!({ "error": err })))?;
    crate::agent_authority::gate::note_answer(&answered_by(cx), pane_id, request_id, &req.reply, Some(cx.registry));
    Ok((200, json!({ "ok": true })))
}

fn question_reply(cx: &Ctx, pane_id: &str) -> RouteResult {
    let req: OpenCodeQuestionReplyBody = parse_json(cx.body)?;
    let session = cx.session.filter(|s| !s.trim().is_empty());
    let prompt_key = req.request_id.clone().unwrap_or_else(|| "opencode-question".to_string());
    gate_reply(session, cx.registry, pane_id, &prompt_key, None, false)
        .map_err(crate::bridge::errors::operation_http_error)?;
    let result = crate::opencode::reply_pane_question(
        cx.registry,
        pane_id,
        &req.answer,
        req.request_id.as_deref(),
    )
    .map_err(|err| (404, json!({ "error": err })))?;
    crate::agent_authority::gate::note_answer(&answered_by(cx), pane_id, &prompt_key, &req.answer, Some(cx.registry));
    Ok((200, result))
}

/// Who sent a reply through the bridge (`AnsweredBy::for_bridge`): a phone, a local program or an
/// agent, never "desktop" (the bridge cannot tell the Puppet Master window from any other local caller).
fn answered_by(cx: &Ctx) -> crate::agent_authority::gate::AnsweredBy {
    crate::agent_authority::gate::AnsweredBy::for_bridge(cx.session, crate::mobile_pairing::requires_pairing_token(cx.headers, cx.peer_loopback))
}

/// The checks `answer_prompt` makes before an answer goes out, for an OpenCode permission or
/// question reply (review R12 3.2): an agent's `always` needs `allow_broad=true`, and with setting
/// A on an agent answering in a pane outside its lineage needs a person's OK (`check_answer`; the
/// OpenCode API does not grade risk, so it counts as not low). `session` is `None` for the person
/// (desktop or phone), who is never gated.
pub(crate) fn gate_reply(
    session: Option<&str>,
    registry: &Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    prompt_key: &str,
    permission_reply: Option<&str>,
    allow_broad: bool,
) -> Result<(), crate::operations::OperationError> {
    if session.is_some() && permission_reply.is_some_and(|reply| reply.trim() == "always") && !allow_broad {
        return Err(crate::operations::OperationError::new(
            "BROAD_APPROVAL_REQUIRES_OPT_IN",
            "reply=always lets OpenCode do this kind of thing from now on without asking; an agent must pass allow_broad=true to opt in, or reply once",
            false,
        ));
    }
    crate::agent_authority::gate::check_answer(session, pane_id, prompt_key, None, None, Some(registry))
}

#[cfg(test)]
mod tests {
    use super::gate_reply;
    use crate::agent_authority::config::{with_config, Config};

    const ASKING: Config = Config { ask_before_agent_control: true, agents_may_merge: true };
    const TODAY: Config = Config { ask_before_agent_control: false, agents_may_merge: true };

    fn registry() -> std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>> {
        std::sync::Arc::new(parking_lot::Mutex::new(crate::pty::PaneRegistry::new()))
    }

    fn id(tag: &str) -> String {
        format!("{tag}-{}", uuid::Uuid::new_v4())
    }

    /// Review R12 3.2: an agent's OpenCode reply skipped `check_answer` and the `allow_broad` rule.
    #[test]
    fn an_agents_opencode_always_needs_allow_broad_like_answer_prompt() {
        let (agent, pane) = (id("agent"), id("oc-pane"));
        with_config(TODAY, || {
            let err = gate_reply(Some(&agent), &registry(), &pane, "perm-1", Some("always"), false).unwrap_err();
            assert_eq!(err.code, "BROAD_APPROVAL_REQUIRES_OPT_IN");
            gate_reply(Some(&agent), &registry(), &pane, "perm-1", Some("always"), true).expect("opted in");
            gate_reply(Some(&agent), &registry(), &pane, "perm-1", Some("once"), false).expect("once needs no opt-in");
            gate_reply(None, &registry(), &pane, "perm-1", Some("always"), false).expect("the person is never gated");
        });
    }

    #[test]
    fn with_setting_a_an_agents_opencode_reply_in_a_pane_it_did_not_start_raises_a_card() {
        let (agent, pane) = (id("agent"), id("oc-user-pane"));
        with_config(ASKING, || {
            let err = gate_reply(Some(&agent), &registry(), &pane, "perm-2", Some("once"), false).unwrap_err();
            assert_eq!(err.code, "CONSENT_PENDING");
            let err = gate_reply(Some(&agent), &registry(), &pane, "q-1", None, false).unwrap_err();
            assert_eq!(err.code, "CONSENT_PENDING", "a question reply is an answer too");
            gate_reply(None, &registry(), &pane, "perm-2", Some("once"), false).expect("the person is never gated");
        });
        with_config(TODAY, || {
            gate_reply(Some(&agent), &registry(), &pane, "perm-3", Some("once"), false).expect("setting A off: today's behaviour");
        });
    }
}
