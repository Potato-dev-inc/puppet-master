//! The chat of a pane as the phone reads it (`GET /panes/{id}/chat?since=<rev>`): the newest items, or only what
//! changed since the revision the phone saw last. Every text that leaves goes through `redact` first, and the
//! "what is it waiting for" line is a plain sentence (the desktop's names the folders it looks in).
//! The phone holds the reader with a lease that each request renews; it ends 60 seconds after the last one.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::model::ChatItem;
use super::opening::Opening;
use super::watch::{Hold, Watchers};

/// How long the reader stays open after the phone's last request.
pub const LEASE: Duration = Duration::from_secs(60);
/// Most items one answer carries.
pub const MAX_ITEMS: usize = 300;
/// The one lease every phone shares on a pane: any request renews it.
const HOLDER: &str = "bridge";

pub const NOT_AGENT: &str = "Chat is for agent panes.";
const GONE: &str = "This chat cannot be read right now.";
const WAITING: &str = "Nothing to show yet. Waiting for the agent.";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    available: bool,
    reason: Option<String>,
    source: Option<&'static str>,
    waiting_for: Option<String>,
    /// Ask for what is newer than this next time.
    rev: u64,
    /// Drop the list held and show `items` instead.
    reset: bool,
    items: Vec<ChatItem>,
}

fn unavailable(reason: &str) -> Reply {
    Reply { available: false, reason: Some(reason.to_string()), source: None, waiting_for: None, rev: 0, reset: false, items: Vec::new() }
}

/// The answer for one request. `opening`: how the pane is read (`None`: it has no agent). `since`: the revision the
/// phone saw last. The result is redacted JSON, ready to send.
pub fn read(watchers: &Watchers, pane_id: &str, opening: Option<&Opening>, since: Option<u64>) -> Value {
    read_with(watchers, pane_id, opening, since, LEASE)
}

fn read_with(watchers: &Watchers, pane_id: &str, opening: Option<&Opening>, since: Option<u64>, lease: Duration) -> Value {
    let reply = match opening {
        None => unavailable(NOT_AGENT),
        Some(opening) => {
            opening.open(watchers, pane_id, Hold::lease(HOLDER, lease));
            match watchers.since(pane_id, since, MAX_ITEMS) {
                None => unavailable(GONE),
                Some(delta) => Reply {
                    available: true,
                    reason: None,
                    source: Some(opening.source_name()),
                    waiting_for: watchers.waiting_for(pane_id).map(|_| WAITING.to_string()),
                    rev: delta.rev,
                    reset: delta.reset,
                    items: delta.items,
                },
            }
        }
    };
    let value = serde_json::to_value(&reply).unwrap_or(Value::Null);
    crate::redact::json(value).into_inner()
}

#[cfg(test)]
#[path = "phone_tests.rs"]
mod tests;
