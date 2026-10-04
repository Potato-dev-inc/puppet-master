//! Mobile device pairing routes: `POST /pair` and `GET /pair/session/<code>`.

use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::http::parse_json;
use crate::mobile_pairing::{self, PairRequestBody};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("POST", ["pair"]) => Some(pair(cx)),
        ("GET", ["pair", "session", code]) => Some(session(code)),
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
