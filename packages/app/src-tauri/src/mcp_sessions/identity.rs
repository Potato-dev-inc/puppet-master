//! Connection identity and catalog mode: which tools a session sees and how it reconnects.

use super::state::{ensure_session, lock_state};
use crate::operations::OperationError;
use crate::tool_registry::{self, McpMode, ToolDefinition};

#[derive(Debug, Clone)]
pub struct SessionIdentity {
    pub connection_id: String,
    pub coordinator_id: String,
    pub attach_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseView {
    pub owner: String,
    pub coordinator_id: String,
    pub controller: Option<String>,
    pub released: bool,
}

pub fn mode_for(session_id: &str) -> McpMode {
    let mut state = lock_state();
    // Read first: a mutable borrow marks the state dirty and rewrites the persisted file, and
    // this runs on every authorized call.
    if let Some(session) = state.sessions.get(session_id) {
        return session.mode;
    }
    ensure_session(&mut state, session_id).mode
}

pub fn set_mode(session_id: &str, requested: &str) -> Result<Vec<ToolDefinition>, OperationError> {
    let mode = McpMode::parse(requested).ok_or_else(|| {
        OperationError::new(
            "INVALID_ARGUMENT",
            "mode must be agent, shell, both, or all",
            false,
        )
    })?;
    let mut state = lock_state();
    ensure_session(&mut state, session_id).mode = mode;
    Ok(tool_registry::tools_for_mode(mode))
}

pub fn session_identity(session_id: &str) -> SessionIdentity {
    let mut state = lock_state();
    let session = ensure_session(&mut state, session_id);
    SessionIdentity {
        connection_id: session_id.to_string(),
        coordinator_id: session.coordinator_id.clone(),
        attach_token: session.attach_token.clone(),
    }
}

pub fn inspect_lease(handle: &str) -> Option<LeaseView> {
    lock_state().agents.get(handle).map(|agent| LeaseView {
        owner: agent.owner.clone(),
        coordinator_id: agent.coordinator_id.clone(),
        controller: agent.controller.clone(),
        released: agent.released,
    })
}

pub fn agent_is_registered(handle: &str) -> bool {
    lock_state().agents.contains_key(handle)
}

pub fn tools_for_session(session_id: &str) -> Vec<ToolDefinition> {
    tool_registry::tools_for_mode(mode_for(session_id))
}
