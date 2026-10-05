//! MCP connection sessions: catalog mode, identity, and ownership/control of panes and agent
//! handles, enforced by `authorize` before a bridge route runs.
//!
//! This file is the module hub; callers keep using `crate::mcp_sessions::<fn>`:
//! - `state` / `persist`: the process-wide ownership table and its on-disk snapshot
//! - `identity`: connection identity, catalog mode, lease inspection
//! - `registration`: recording ownership of new panes and agent handles
//! - `access`: run-access checks and dead-connection detection
//! - `leases`: release, takeover, transfer and token reattach
//! - `authorize`: the route gate (mode + ownership)
//! - `handles`: pane-id resolution and the denial/not-found errors

mod access;
mod authorize;
mod handles;
mod identity;
mod leases;
mod persist;
mod registration;
mod state;

pub use access::*;
pub use authorize::*;
pub use handles::*;
pub use identity::*;
pub use leases::*;
pub use registration::*;

#[cfg(test)]
use {persist::*, state::*};

#[cfg(test)]
mod tests;
