//! A refused typed write is never silent: the caller gets the refusal (the PWA shows a plain line)
//! and the app log gets ONE line per pane and channel.
//!
//! Live (VERIFY-R11): a loopback browser tab (no pairing token, no MCP session) typing into a pane a
//! `run_agent` run manages is refused 403 `AUTHORIZATION_DENIED` ("managed pane mutations require an
//! MCP session id", `mcp_sessions/authorize.rs`), and nothing on either side said so. A paired phone is
//! exempt (`router.rs`, `ws.rs`), so this only concerns the desktop's own browser tab, a dev server,
//! demos and filming. A held key repeats the refusal on every byte, hence once per pane and channel.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashSet;

/// How the refused write arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Channel {
    /// `POST /panes/{id}/input` or `/key`.
    Http,
    /// A `{"type":"input"}` frame on the events socket.
    Socket,
}

impl Channel {
    fn label(self) -> &'static str {
        match self {
            Channel::Http => "http",
            Channel::Socket => "socket",
        }
    }
}

/// Keeps the set from growing without bound on a long-running app.
const MAX_REMEMBERED: usize = 512;

static SEEN: Lazy<Mutex<HashSet<(String, Channel)>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// True the first time this pane is refused on this channel (the caller logs then).
pub(crate) fn first_refusal(pane_id: &str, channel: Channel) -> bool {
    let mut seen = SEEN.lock();
    if seen.len() >= MAX_REMEMBERED {
        seen.clear();
    }
    seen.insert((pane_id.to_string(), channel))
}

/// Log the refusal once per pane and channel.
pub(crate) fn note_refused(pane_id: &str, channel: Channel, reason: &str) {
    if first_refusal(pane_id, channel) {
        tracing::warn!(
            code = "TYPING_REFUSED",
            pane = %pane_id,
            channel = channel.label(),
            %reason,
            "typed input was refused (the caller is not a paired phone and the pane is run by an agent); the PWA shows the person why"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_logged_once_per_pane_and_channel() {
        let pane = format!("typing-refusal-test-{}", uuid::Uuid::new_v4());
        assert!(first_refusal(&pane, Channel::Http), "the first refusal is logged");
        assert!(!first_refusal(&pane, Channel::Http), "a held key repeating it is not");
        assert!(first_refusal(&pane, Channel::Socket), "the other channel has its own line");
        assert!(!first_refusal(&pane, Channel::Socket));
        assert!(first_refusal(&format!("{pane}-other"), Channel::Http), "another pane has its own line");
    }
}
