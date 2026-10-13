//! Bridge adapter for the push routes (`crate::push::routes` holds the logic).

use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.segments.first() != Some(&"push") {
        return None;
    }
    let Some(hub) = crate::push::hub() else {
        return Some(Err((503, json!({"error": "push_unavailable"}))));
    };
    let now = crate::push::now_ms();
    if cx.method == "POST" && cx.segments == ["push", "act"] {
        return Some(act(cx, &hub, now));
    }
    crate::push::routes::handle(&hub, cx.method, cx.segments, cx.headers, cx.body, now).map(
        |(status, body)| {
            if status < 400 {
                Ok((status, body))
            } else {
                Err((status, body))
            }
        },
    )
}

/// An Allow / Deny tap: the one-shot token names the pane and prompt; `pane_prompt::answer` still
/// verifies the prompt on screen is that very prompt before pressing a key.
fn act(cx: &Ctx, hub: &crate::push::hub::PushHub, now: u64) -> RouteResult {
    let act = crate::push::routes::redeem_act(hub, cx.body, now)?;
    let answered = crate::pane_prompt::answer(
        cx.registry,
        cx.app,
        &act.grant.pane_id,
        &act.grant.prompt_id,
        act.choice,
        false,
    )
    .map_err(operation_http_error)?;
    Ok((200, json!({"ok": true, "answered": answered})))
}
