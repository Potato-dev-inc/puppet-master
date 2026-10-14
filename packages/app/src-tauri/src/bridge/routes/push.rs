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
    let reply = crate::push::routes::handle(&hub, cx.method, cx.segments, cx.headers, cx.body, now);
    // A phone that just went hidden may release a notification held back while it was visible.
    crate::push::flush_outbox(&hub);
    reply.map(
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
    // The tap acts for the device the notification went to: it needs that device's `answer` cap.
    if !device_may_answer(&act.grant.device_id) {
        return Err((403, json!({"error": "not_allowed", "code": "DEVICE_NOT_ALLOWED", "needs": "answer"})));
    }
    let answered = crate::pane_prompt::answer(
        cx.registry,
        cx.app,
        &act.grant.pane_id,
        &act.grant.prompt_id,
        act.choice,
        false,
    )
    .map_err(operation_http_error)?;
    // A lock-screen tap: the audit log says which device pressed which choice (never the command).
    crate::audit::record(
        crate::audit::Kind::Approval,
        crate::audit::Who::Device(Some(act.grant.device_id.clone())),
        &format!("A phone answered from its lock screen: {}", if act.choice == "deny" { "Said no" } else { "Allowed once" }),
        json!({ "pane_id": act.grant.pane_id, "prompt_id": act.grant.prompt_id, "choice": act.choice, "via": "push" }),
    );
    Ok((200, json!({"ok": true, "answered": answered})))
}

pub(in crate::bridge) fn device_may_answer(device_id: &str) -> bool {
    crate::mobile_pairing::pairing_store()
        .and_then(|store| store.lock().device_caps(device_id))
        .is_some_and(|caps| caps.has(crate::mobile_pairing::Cap::Answer))
}

#[cfg(test)]
mod tests {
    use crate::mobile_pairing::{init_pairing_store, pairing_store, Cap, Caps, PairRequestBody};

    #[test]
    fn a_notification_tap_needs_the_answer_cap_of_its_device() {
        if pairing_store().is_none() {
            let dir = std::env::temp_dir().join(format!("pm-push-act-caps-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let _ = init_pairing_store(dir.join("pairing.json"));
        }
        let pair = |caps: Caps| {
            let store = pairing_store().unwrap();
            let mut guard = store.lock();
            let s = guard.create_pairing_session_with_caps("http://127.0.0.1:1".into(), caps);
            guard
                .pair_device(PairRequestBody {
                    pairing_code: s.pairing_code,
                    device_name: "p".into(),
                    device_public_key: s.server_public_key,
                })
                .unwrap()
                .device_id
        };
        assert!(super::device_may_answer(&pair(Caps::all())));
        assert!(super::device_may_answer(&pair(Caps::watch_and_answer())));
        assert!(!super::device_may_answer(&pair(Caps::none().with(Cap::Watch, true))));
        assert!(!super::device_may_answer("no-such-device"));
    }
}
