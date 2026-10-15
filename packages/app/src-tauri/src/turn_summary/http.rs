//! `GET /digest?since=<ms>&seen=1`: what happened since the phone last looked.
//!
//! The request is authorised by the bridge like every other route (paired bearer token for a
//! remote caller). `since` defaults to this device's stored "last seen"; `seen=1` stores "now" as
//! the new last seen, so the next resume only shows what is new.

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::digest::{self, Digest, LastSeen, NeedsYou};
use super::model::TurnSummary;

/// What the route needs from the running app (a fake in tests).
pub trait Source {
    fn now_ms(&self) -> u64;
    fn summaries_since(&self, since_ms: u64) -> Vec<TurnSummary>;
    fn needs_you(&self) -> Vec<NeedsYou>;
}

pub struct Live;

impl Source for Live {
    fn now_ms(&self) -> u64 {
        super::now_ms()
    }
    fn summaries_since(&self, since_ms: u64) -> Vec<TurnSummary> {
        super::tracker().since(since_ms)
    }
    fn needs_you(&self) -> Vec<NeedsYou> {
        super::needs_you()
    }
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

/// `None` when the request is not the digest route.
pub fn respond(
    src: &dyn Source,
    seen: &Mutex<LastSeen>,
    method: &str,
    segments: &[&str],
    query: &str,
    device: &str,
) -> Option<Result<(u16, Value), (u16, Value)>> {
    if !(method == "GET" && segments == ["digest"]) {
        return None;
    }
    let now = src.now_ms();
    let previous = seen.lock().get(device);
    let since = match query_value(query, "since") {
        Some(raw) => match raw.parse::<u64>() {
            Ok(ms) => ms,
            Err(_) => {
                return Some(Err((400, json!({"code": "INVALID_ARGUMENT", "error": "`since` must be milliseconds since the epoch"}))));
            }
        },
        None => previous.unwrap_or(now),
    };
    let digest: Digest = digest::build(&src.summaries_since(since), src.needs_you(), since, now);
    if matches!(query_value(query, "seen"), Some("1" | "true")) {
        seen.lock().set(device, now);
    }
    let mut body = serde_json::to_value(&digest).unwrap_or_default();
    body["previousSeenMs"] = json!(previous);
    body["empty"] = json!(digest.is_empty());
    Some(Ok((200, body)))
}
