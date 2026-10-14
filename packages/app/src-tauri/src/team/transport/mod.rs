//! How team messages travel. Each transport hides its own wire behind one trait, so the rest of
//! `team/` never knows which one is in use:
//! - [`folder::FolderTransport`]: a shared folder (demo, tests, a synced or network folder);
//! - [`relay::RelayTransport`]: a relay server (`relay/` at the top of the repository) over
//!   WebSocket, with a long-poll fallback;
//! - later, iroh peers (v2).
//!
//! A transport carries **sealed, signed envelopes** and two signed state slots, never plain
//! events: `secure::Session` seals before `send` and opens (checking signatures) after
//! `subscribe`, so a transport is only a pipe and cannot make a card by itself.
//!
//! Contract every transport keeps:
//! - **Outbound only from this app's point of view.** A transport may open connections or watch
//!   files; it never adds a route to the bridge and never listens for callers (plan §8.3). Team
//!   traffic never uses the bridge or device pairing.
//! - `send` returns once the envelope is handed off durably (written, or accepted by the relay).
//! - `subscribe` yields envelopes in no promised order, possibly more than once (the session's
//!   replay guard drops repeats). They are UNTRUSTED until `Session::open` accepts them.
//! - Our own envelopes may come back on `subscribe`; the session recognises them.

pub mod folder;
pub mod relay;

use pm_relay_protocol::{Envelope, JoinBox, SignedAcl, SignedBlob, TeamRegistration};
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a refusal by the relay's access list starts ([`TransportError::is_denied`]).
pub const DENIED: &str = "The relay doesn't let this computer in";

impl TransportError {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The relay refused this device: it is not (or no longer) on the team's access list.
    pub fn is_denied(&self) -> bool {
        self.0.starts_with(DENIED)
    }
}

/// One envelope as a transport received it (not checked yet).
#[derive(Debug, Clone)]
pub struct Inbound {
    pub envelope: Envelope,
    pub received_at_ms: u64,
    /// The relay's mailbox position after this envelope (0 for the folder).
    pub cursor: u64,
}

pub trait TeamTransport: Send + Sync {
    /// Short name for logs and Settings: `folder` or `relay`.
    fn kind(&self) -> &'static str;
    /// Hand one sealed envelope to the team.
    fn send(&self, envelope: &Envelope) -> Result<(), TransportError>;
    /// Start receiving envelopes after mailbox position `after`. The channel closes when the
    /// transport stops.
    fn subscribe(&self, after: u64) -> Result<Receiver<Inbound>, TransportError>;
    /// Write a signed state slot (`roster`, `keys`). The admin's app only.
    fn put_state(&self, blob: &SignedBlob) -> Result<(), TransportError>;
    /// Read a state slot. `Ok(None)` when there is none yet.
    fn get_state(&self, slot: &str) -> Result<Option<SignedBlob>, TransportError>;
    /// Post a sealed join request (anyone with an invite link).
    fn post_join(&self, join: &JoinBox) -> Result<(), TransportError>;
    /// Join requests waiting for the admin.
    fn joins(&self) -> Result<Vec<JoinBox>, TransportError>;
    /// Relay only: make the team known to the relay.
    fn register(&self, _registration: &TeamRegistration) -> Result<(), TransportError> {
        Ok(())
    }
    /// Relay only: which devices may use the mailbox.
    fn set_acl(&self, _acl: &SignedAcl) -> Result<(), TransportError> {
        Ok(())
    }
    /// How messages arrive right now, for Settings ("live", "polling", "offline").
    fn status(&self) -> &'static str {
        "live"
    }
    /// Stop every subscription and watcher. Idempotent.
    fn stop(&self);
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
