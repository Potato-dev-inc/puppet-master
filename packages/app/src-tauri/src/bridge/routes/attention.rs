//! `GET /attention`: the one list of things that need a person (logic in `crate::attention`).
//! Any paired device may read it (`watch` is not needed: it carries sentences, not terminal bytes);
//! the answer routes of the design are not part of this build.

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
    Some(Err((404, json!({"error": "unknown_attention_route"}))))
}
