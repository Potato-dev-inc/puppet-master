//! Run-access checks (control, read, wait) and the liveness test that makes a dead
//! connection's lease reclaimable.

use super::handles::agent_not_found;
use super::state::{lock_state, PaneControl, State};
use crate::operations::OperationError;
use serde_json::json;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

pub fn check_run_access(session_id: &str, handle: &str) -> Result<(), OperationError> {
    let state = lock_state();
    match state.agents.get(handle) {
        Some(agent) if agent.controller.as_deref() == Some(session_id) && !agent.released => Ok(()),
        Some(agent) => Err(control_denied(session_id, handle, agent, &state)),
        None => Err(agent_not_found(handle)),
    }
}

/// Read-only agent APIs: control, reclaimable lease, or unregistered handle (operations-backed).
pub fn check_run_read_access(session_id: &str, handle: &str) -> Result<(), OperationError> {
    if check_run_access(session_id, handle).is_ok() {
        return Ok(());
    }
    let state = lock_state();
    match state.agents.get(handle) {
        None => Ok(()),
        Some(agent) if agent.released || controller_is_stale(&state, agent) => Ok(()),
        Some(agent) => Err(control_denied(session_id, handle, agent, &state)),
    }
}

/// Waits must not change leases; same visibility as read plus registered runs you can reclaim.
pub fn check_run_wait_access(session_id: &str, handle: &str) -> Result<(), OperationError> {
    if check_run_access(session_id, handle).is_ok() {
        return Ok(());
    }
    let state = lock_state();
    match state.agents.get(handle) {
        Some(agent) if agent.released || controller_is_stale(&state, agent) => Ok(()),
        Some(agent) => Err(control_denied(session_id, handle, agent, &state)),
        None => Ok(()),
    }
}

/// MCP connection id -> OS pid of the process serving it (sent by the MCP binary on each request).
/// In memory only: a pid means nothing after the bridge restarts.
pub(super) static SESSION_PIDS: Lazy<Mutex<HashMap<String, u32>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn note_session_pid(session_id: &str, pid: u32) {
    if let Ok(mut pids) = SESSION_PIDS.lock() {
        if pids.get(session_id) != Some(&pid) {
            pids.insert(session_id.to_string(), pid);
        }
    }
}

pub(super) fn process_is_alive(pid: u32) -> bool {
    // Asked of the OS directly (no `tasklist`/`kill` child): this runs on every MCP access check,
    // under the session lock, and a child per check meant a console flash and ~50 ms each.
    crate::proc_util::pid_alive(pid)
}

/// True only when we know this connection's process and it has exited. A host that abandons a
/// request after a timeout restarts the MCP process under a new id; the old id is then dead
/// weight, not a live competitor.
pub(super) fn session_process_dead(session_id: &str) -> bool {
    let pid = SESSION_PIDS
        .lock()
        .ok()
        .and_then(|pids| pids.get(session_id).copied());
    pid.is_some_and(|pid| !process_is_alive(pid))
}

pub(super) fn coordinator_live_connection(state: &State, coordinator_id: &str) -> Option<String> {
    state
        .coordinators
        .get(coordinator_id)
        .map(|record| record.connection_id.clone())
}

pub(super) fn controller_is_stale(state: &State, agent: &PaneControl) -> bool {
    match agent.controller.as_deref() {
        None => true,
        Some(_) if agent.released => true,
        Some(controller) => {
            coordinator_live_connection(state, &agent.coordinator_id)
                .is_some_and(|live| live != controller)
                || session_process_dead(controller)
        }
    }
}

pub(super) fn control_denied(
    session_id: &str,
    handle: &str,
    agent: &PaneControl,
    state: &State,
) -> OperationError {
    let message = if controller_is_stale(state, agent) || agent.released {
        format!(
            "this connection does not control agent {handle}; call take_over(handle, grant=true) to reclaim, or attach_agents(attach_token) after reconnect (see session_identity)"
        )
    } else {
        format!(
            "agent {handle} is controlled by another live MCP connection; call take_over(handle, grant=true) to preempt, transfer_agent, or attach_agents(attach_token) if this is your coordinator after reconnect"
        )
    };
    let mut error = OperationError::new("AUTHORIZATION_DENIED", message, false);
    error.context = json!({
        "handle": handle,
        "session_id": session_id,
        "reclaim_with": "take_over",
        "grant": true,
        "reconnect_with": "attach_agents",
    });
    error
}
