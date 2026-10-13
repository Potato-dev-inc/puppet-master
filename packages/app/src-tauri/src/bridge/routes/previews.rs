//! `GET /previews`: dev-server URLs per pane (`pane_preview`), for the phone. Read-only. The
//! phone only displays them ("Open on your PC"); the bridge never proxies a dev-server port.

use super::{Ctx, RouteResult};
use serde_json::json;

pub(in crate::bridge) fn static_route(method: &str, segments: &[&str]) -> Option<(u16, serde_json::Value)> {
    match (method, segments) {
        ("GET", ["previews"]) => Some((200, json!({ "previews": crate::pane_preview::snapshot() }))),
        _ => None,
    }
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    static_route(cx.method, cx.segments).map(Ok)
}
