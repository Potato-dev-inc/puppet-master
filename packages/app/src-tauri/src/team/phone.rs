//! Team cards on the phone (the paired PWA): bridge routes behind the paired device's bearer token
//! and its capabilities (device caps, `mobile_pairing::caps`):
//!
//! | Route | Needs |
//! |---|---|
//! | `GET /team/cards` | `watch` |
//! | `POST /team/cards/{id}/dismiss` | `answer` or `control` |
//! | `POST /team/cards/{id}/pull` | `control` |
//! | `POST /team/cards/{id}/queue` `{paneId, when}` | `control` |
//! | `POST /team/jobs/{id}/apply` | `control` |
//! | `POST /team/jobs/{id}/discard` | `control` |
//!
//! These are the person's own routes (their desktop app or their paired phone), checked by the
//! bridge gate like every other route. They are **not** how teammates reach this app: team traffic
//! never touches the bridge (it is outbound-only, `transport`), and no team route is public. Agent
//! sessions (MCP) are refused: a teammate's card is acted on by a person.

use super::service;
use crate::audit::Who;
use crate::mobile_pairing::Caller;
use serde_json::{json, Value};

pub type RouteResult = Result<(u16, Value), (u16, Value)>;

/// What this caller may do with a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Can {
    pub dismiss: bool,
    pub act: bool,
}

pub fn can(caller: &Caller) -> Can {
    use crate::mobile_pairing::Cap;
    Can {
        dismiss: caller.may(Cap::Answer) || caller.may(Cap::Control),
        act: caller.may(Cap::Control),
    }
}

fn who(caller: &Caller) -> Who {
    match caller {
        Caller::Device { device_id, .. } => Who::Device(Some(device_id.clone())),
        _ => Who::Desktop,
    }
}

fn bad(status: u16, error: impl Into<String>) -> RouteResult {
    Err((status, json!({ "error": error.into() })))
}

fn ok(value: Value) -> RouteResult {
    Ok((200, value))
}

fn done<T: serde::Serialize>(result: Result<T, String>) -> RouteResult {
    match result {
        Ok(value) => ok(json!({ "ok": true, "result": value })),
        Err(error) => bad(409, error),
    }
}

/// The phone's view: the cockpit's cards and queued updates (no unsent drafts; my own pushes are
/// never offered on the phone), and what this caller may do.
pub fn cards_view(view: service::TeamView, can: Can) -> Value {
    json!({
        "enabled": view.enabled,
        "active": view.active,
        "missing": view.missing,
        "error": view.error,
        "cards": view.cards,
        "jobs": view.jobs,
        "can": { "dismiss": can.dismiss, "act": can.act },
    })
}

/// `None` when the route is not a team route.
pub fn handle(method: &str, segments: &[&str], body: &Value, caller: &Caller, agent_session: bool) -> Option<RouteResult> {
    if segments.first() != Some(&"team") {
        return None;
    }
    if agent_session {
        return Some(bad(403, "Team cards are acted on by a person, not an agent."));
    }
    let can = can(caller);
    let who = who(caller);
    let reply = match (method, segments) {
        ("GET", ["team", "cards"]) => match service::view() {
            Ok(view) => ok(cards_view(view, can)),
            Err(error) => bad(503, error),
        },
        ("POST", ["team", "cards", id, "dismiss"]) => {
            if !can.dismiss {
                return Some(bad(403, "not_allowed"));
            }
            done(service::dismiss_card(id))
        }
        ("POST", ["team", "cards", id, "pull"]) => {
            if !can.act {
                return Some(bad(403, "not_allowed"));
            }
            done(service::pull_as(id, &who))
        }
        ("POST", ["team", "cards", id, "queue"]) => {
            if !can.act {
                return Some(bad(403, "not_allowed"));
            }
            let pane = body.get("paneId").and_then(Value::as_str).unwrap_or_default();
            let when = body.get("when").and_then(Value::as_str).unwrap_or("idle");
            if pane.is_empty() {
                return Some(bad(400, "Pick which agent should do it."));
            }
            done(service::queue_as(id, pane, when, &who))
        }
        ("POST", ["team", "jobs", id, "apply"]) => {
            if !can.act {
                return Some(bad(403, "not_allowed"));
            }
            done(service::apply_as(id, &who))
        }
        ("POST", ["team", "jobs", id, "discard"]) => {
            if !can.act {
                return Some(bad(403, "not_allowed"));
            }
            done(service::discard_as(id, &who))
        }
        _ => bad(404, "not found"),
    };
    Some(reply)
}
