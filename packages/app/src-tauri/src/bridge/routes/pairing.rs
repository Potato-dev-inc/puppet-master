//! Mobile device pairing routes: `POST /pair`, `GET /pair/session/<code>` and the local-only
//! `POST /pair/invite` that lets `puppet-master pair-link` mint a pairing link.

use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::http::parse_json;
use crate::mobile_pairing::{self, PairRequestBody};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("POST", ["pair"]) => Some(pair(cx)),
        ("GET", ["pair", "session", code]) => Some(session(code)),
        ("POST", ["pair", "invite"]) => Some(invite(cx)),
        _ => None,
    }
}

fn pair(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req: PairRequestBody = parse_json(body)?;
    let store = mobile_pairing::pairing_store()
        .ok_or_else(|| (503, serde_json::json!({ "error": "pairing_unavailable" })))?;
    let response = {
        let mut guard = store.lock();
        guard
            .pair_device(req)
            .map_err(|err| (400, json!({ "error": err })))?
    };
    Ok((200, serde_json::to_value(response).unwrap()))
}

fn session(code: &str) -> RouteResult {
    let store = mobile_pairing::pairing_store()
        .ok_or_else(|| (503, serde_json::json!({ "error": "pairing_unavailable" })))?;
    let info = {
        let guard = store.lock();
        guard
            .lookup_pairing_session(code)
            .map_err(|err| (404, json!({ "error": err })))?
    };
    Ok((200, serde_json::to_value(info).unwrap()))
}

/// Mints a pairing session for the public URL the caller names: the same session the desktop
/// UI creates through the Tauri command. Local callers only; the code is never logged.
fn invite(cx: &Ctx) -> RouteResult {
    if !mobile_pairing::is_local_only_caller(cx.headers, cx.peer_loopback) {
        return Err((403, json!({ "error": "local_only" })));
    }
    let bridge_url = cx
        .request_body
        .get("bridge_url")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (400, json!({ "error": "bridge_url_required" })))?
        .to_string();
    let store = mobile_pairing::pairing_store()
        .ok_or_else(|| (503, json!({ "error": "pairing_unavailable" })))?;
    let session = store.lock().create_pairing_session(bridge_url);
    Ok((200, serde_json::to_value(session).unwrap()))
}
