//! The pane each MCP connection says its agent runs in, and the connection's process id.
//!
//! The MCP shim reads `PUPPET_MASTER_PANE_ID` (set on every pane Puppet Master launches) and sends it
//! as `X-Puppet-Master-Pane`. It is a caller-supplied claim, kept in memory only and used for one
//! thing: nesting a helper pane under its parent in the sidebar (`pty::pane_parent` checks the claim
//! against the live registry and the process tree first). It never feeds `authorize`, leases or
//! `agent_authority`: no right depends on it.

use super::access::SESSION_PIDS;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

/// The variable Puppet Master sets on every pane it launches, and the cleaning the shim applies
/// before sending it (re-exported here because the shim reaches the app library through this module).
pub use crate::pty::pane_parent::clean_hint as clean_pane_hint;
pub use crate::pty::PANE_ID_ENV;

static SESSION_PANES: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Remember the pane a connection named (cleaned; an empty or malformed value is dropped).
pub fn note_session_pane(session_id: &str, raw_pane: &str) {
    let Some(pane) = crate::pty::pane_parent::clean_hint(raw_pane) else {
        return;
    };
    if let Ok(mut panes) = SESSION_PANES.lock() {
        if panes.get(session_id) != Some(&pane) {
            panes.insert(session_id.to_string(), pane);
        }
    }
}

/// The pane this connection claims to run in, unchecked.
pub fn claimed_session_pane(session_id: &str) -> Option<String> {
    SESSION_PANES.lock().ok()?.get(session_id).cloned()
}

/// The OS process id of the MCP process serving this connection, as it reported it.
pub fn session_pid(session_id: &str) -> Option<u32> {
    SESSION_PIDS.lock().ok()?.get(session_id).copied()
}
