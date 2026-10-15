//! `POST /hooks/event` (§4.2, §4.4) and the hook-token scope guard.
//!
//! Rules on top of the bridge's own authorization (none removed):
//! 1. local only: a caller that would need a pairing token (a phone, a tunnel) gets 403
//!    `HOOK_LOCAL_ONLY`, even with a valid device bearer;
//! 2. credentials stay separate: `Authorization` or an MCP session header gets 400
//!    `HOOK_MIXED_CREDENTIALS`;
//! 3. the per-pane token must match (constant time), be live and belong to the named agent;
//! 4. append-only: the reply is `202 {}` and never carries data, there is no GET route;
//! 5. scope: any request carrying `X-PM-Hook-Token` to any other route gets 403 `HOOK_TOKEN_SCOPE`.
//!
//! Nothing reachable from here types, answers, approves or unlocks anything.

use serde_json::{json, Value};

use crate::mobile_pairing::{header_value, normalize_bridge_segments, requires_pairing_token};

pub const TOKEN_HEADER: &str = "X-PM-Hook-Token";
pub const PANE_HEADER: &str = "X-PM-Hook-Pane";
pub const AGENT_HEADER: &str = "X-PM-Hook-Agent";
/// Largest body the route takes (the emitter shrinks to 512 KiB).
pub const MAX_BODY: usize = 1024 * 1024;

/// `POST /hooks/event` (also under `/bridge/`).
pub fn is_hook_ingest(method: &str, segments: &[&str]) -> bool {
    method == "POST" && normalize_bridge_segments(segments) == ["hooks", "event"]
}

/// Same, from a raw request path.
pub fn is_hook_ingest_path(method: &str, path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    is_hook_ingest(method, &segments)
}

/// A hook token is good for `POST /hooks/event` and nothing else.
pub fn scope_guard(headers: &str, segments: &[&str], method: &str) -> Result<(), (u16, Value)> {
    if header_value(headers, TOKEN_HEADER).is_some() && !is_hook_ingest(method, segments) {
        return Err((403, json!({ "code": "HOOK_TOKEN_SCOPE", "error": "a hook token may only post hook events" })));
    }
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn reject(status: u16, code: &str) -> (u16, Value) {
    (status, json!({ "code": code }))
}

/// Handle one `POST /hooks/event`. The answer never says more than its status and code.
pub fn handle(headers: &str, body: &[u8], peer_loopback: bool) -> (u16, Value) {
    handle_at(headers, body, peer_loopback, now_ms())
}

pub fn handle_at(headers: &str, body: &[u8], peer_loopback: bool, now: u64) -> (u16, Value) {
    if requires_pairing_token(headers, peer_loopback) {
        return reject(403, "HOOK_LOCAL_ONLY");
    }
    if header_value(headers, "Authorization").is_some() || header_value(headers, "X-Puppet-Master-Session").is_some() {
        return reject(400, "HOOK_MIXED_CREDENTIALS");
    }
    if body.len() > MAX_BODY {
        return reject(413, "HOOK_TOO_LARGE");
    }
    let (Some(token), Some(pane), Some(agent)) =
        (header_value(headers, TOKEN_HEADER), header_value(headers, PANE_HEADER), header_value(headers, AGENT_HEADER))
    else {
        return reject(401, "HOOK_TOKEN_INVALID");
    };
    let cwd = match super::token::check(pane, token, agent, now) {
        super::token::Check::Valid { cwd } => cwd,
        super::token::Check::Invalid => return reject(401, "HOOK_TOKEN_INVALID"),
        super::token::Check::AgentMismatch => {
            super::count_rejection("agent_mismatch");
            return reject(403, "HOOK_AGENT_MISMATCH");
        }
        super::token::Check::RateLimited => {
            super::count_rejection("rate_limited");
            return reject(429, "HOOK_RATE_LIMITED");
        }
    };
    let Some(scheme) = crate::tui::profile_for(agent).hook_scheme() else {
        return (202, json!({}));
    };
    let payload: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({ "unparsed": true, "bytes": body.len() }));
    if let Ok((event, applied)) = super::ledger::append(&scheme, &payload, pane, agent, &cwd, now) {
        super::shadow::on_hook_event(&event, applied);
    }
    (202, json!({}))
}
