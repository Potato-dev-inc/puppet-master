//! `GET /attention`: the one list of things that need a person, and `POST /attention/{id}/answer`:
//! a paired person answers one of them from the phone card (logic in `crate::attention`).
//! Any paired device may read the list (`watch` is not needed: it carries sentences, not terminal
//! bytes). The answer needs the `answer` ability (and `control` for a budget or usage-limit card);
//! `route_need` puts it in that class and the answer function checks it again.

use serde_json::json;

use super::{Ctx, RouteResult};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.segments.first() != Some(&"attention") {
        return None;
    }
    if cx.method == "GET" && cx.segments.len() == 1 {
        let device = crate::mobile_pairing::bearer_token(cx.headers)
            .and_then(|token| crate::mobile_pairing::pairing_store()?.lock().device_id_for_token(&token));
        let paired = crate::mobile_pairing::requires_pairing_token(cx.headers, cx.peer_loopback);
        let body = match (paired, device) {
            (true, Some(id)) => crate::attention::routes::get_for_device(&id),
            (true, None) => crate::attention::routes::get(Some(crate::attention::caps::DeviceCaps::none())),
            (false, _) => crate::attention::routes::get(None),
        };
        return Some(Ok((200, body)));
    }
    if cx.method == "POST" && cx.segments.len() == 3 && cx.segments[2] == "answer" {
        return Some(answer(cx, cx.segments[1]));
    }
    Some(Err((404, json!({"error": "unknown_attention_route"}))))
}

/// A person's answer from a paired phone. NOT for agents: an agent answers through `answer_prompt`,
/// where the agent-authority consent rules apply. A caller that is not a paired device (an MCP
/// session, a plain loopback program) is refused, whatever it sends.
fn answer(cx: &Ctx, item_id: &str) -> RouteResult {
    let device = crate::mobile_pairing::bearer_token(cx.headers)
        .and_then(|token| crate::mobile_pairing::pairing_store()?.lock().device_id_for_token(&token));
    let Some(device_id) = device.filter(|_| cx.session.is_none()) else {
        return Err((
            403,
            json!({
                "error": "paired_device_required",
                "code": "PERSON_ONLY",
                "hint": "This route answers for a paired person. An agent answers with answer_prompt."
            }),
        ));
    };
    let request: crate::attention::answer::Request = serde_json::from_value(cx.request_body.clone())
        .map_err(|error| (400, json!({"error": "invalid_request", "detail": error.to_string()})))?;
    let caller = crate::attention::answer::Caller {
        device_name: crate::attention::caps::device_name(&device_id).unwrap_or_else(|| "A phone".to_string()),
        caps: crate::attention::caps::for_device(&device_id),
        device_id,
        lock_screen: false,
    };
    let env = crate::attention::answer::RealEnv { registry: cx.registry.clone(), app: cx.app.clone() };
    let (status, body) = crate::attention::answer::answer(&env, &caller, item_id, &request);
    if status < 400 {
        Ok((status, body))
    } else {
        Err((status, body))
    }
}
