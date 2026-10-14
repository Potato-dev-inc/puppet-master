//! `GET /attention`: the one list of things that need a person (logic in `crate::attention`).

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
        return Some(Ok((200, crate::attention::routes::get(device.as_deref(), paired))));
    }
    Some(Err((404, json!({"error": "unknown_attention_route"}))))
}
