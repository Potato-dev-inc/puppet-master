//! `/potato/brain/*`: the two routes a Potato Bot brain uses (spec section 2).
//!
//! - `GET /potato/brain/inbox?cursor=N&wait_ms=25000`: a long poll. `{"v":1,"cursor":<next>,"items":[...]}`.
//! - `POST /potato/brain/events` with `{"v":1,"events":[...]}`: at most 32 events and 64 KB.
//!
//! Who may call them is decided before this file runs, in `bridge/http.rs::gate` through `potato::auth`
//! (loopback, no forwarder header, no `Origin`, the bot's token, the bot's own process). This file only
//! answers a caller that already passed, so it takes the bot's id from the gate and never from the
//! request. The router never serves these paths (`router.rs` refuses them), they send no CORS headers,
//! and any other path or method under `/potato/brain/` is 404.

use serde_json::{json, Value};

use crate::bridge::http::query_param;
use crate::potato::protocol::{self, PostError};
use crate::potato::service::Service;

pub(in crate::bridge) fn not_found() -> (u16, Value) {
    (404, json!({ "error": "not found" }))
}

/// Answers one brain request. `segments` have the `bridge` prefix already dropped.
pub(in crate::bridge) fn respond(service: &Service, bot: &str, method: &str, segments: &[&str], query: &str, body: &[u8]) -> (u16, Value) {
    match (method, segments) {
        ("GET", ["potato", "brain", "inbox"]) => {
            let cursor = query_param(query, crate::potato::wire_names::POLL_POSITION).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let wait_ms = query_param(query, "wait_ms").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            (200, service.inbox_poll(bot, cursor, wait_ms))
        }
        ("POST", ["potato", "brain", "events"]) => match protocol::parse_post(body) {
            Ok(events) => {
                service.ingest(bot, events);
                (200, json!({ "v": 1, "ok": true }))
            }
            Err(PostError::TooBig) => (413, json!({ "code": "POTATO_TOO_BIG", "error": "at most 64 KB" })),
            Err(PostError::Invalid) => (400, json!({ "code": "POTATO_BAD_POST", "error": "expected {\"v\":1,\"events\":[...]} with at most 32 events" })),
        },
        _ => not_found(),
    }
}
