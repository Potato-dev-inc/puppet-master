//! How team events travel. Each transport hides its own wire (a shared folder today; a relay over
//! WebSocket with end-to-end encryption in v1, iroh peers in v2) behind one trait, so the rest of
//! `team/` never knows which one is in use.
//!
//! Contract every transport keeps:
//! - **Outbound only from this app's point of view.** A transport may open connections or watch
//!   files; it never adds a route to the bridge and never listens for callers (plan §8.3). Team
//!   traffic never uses the bridge or device pairing.
//! - `send` returns once the event is handed off durably (written, or accepted by the relay).
//! - `subscribe` yields parsed events, each at most once per subscription, in no promised order.
//!   They are UNTRUSTED: the service validates every one (`event::validate_inbound`), filters out
//!   its own, and dedupes by id. A relay transport decrypts and checks signatures *before*
//!   yielding, and drops what fails.
//! - Our own sent events may come back on `subscribe`; the caller filters them.
//!
//! A `RelayTransport` (v1) slots in by implementing this trait: `send` seals the event into an
//! envelope `{team, epoch, sender_device, nonce, ciphertext, sig}` and posts it; `subscribe`
//! reads envelopes from the WebSocket (SSE fallback), opens them and yields the events.

pub mod folder;

use super::event::TeamEvent;
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One event as a transport received it (not validated yet).
#[derive(Debug, Clone)]
pub struct Inbound {
    pub event: TeamEvent,
    pub received_at_ms: u64,
}

pub trait TeamTransport: Send + Sync {
    /// Short name for logs and Settings: `folder`, later `relay`.
    fn kind(&self) -> &'static str;
    /// Hand one event to the team.
    fn send(&self, event: &TeamEvent) -> Result<(), TransportError>;
    /// Start receiving. The channel closes when the transport stops.
    fn subscribe(&self) -> Result<Receiver<Inbound>, TransportError>;
    /// Stop every subscription and watcher. Idempotent.
    fn stop(&self);
}
