//! Recording ownership when a connection creates, adopts or binds a pane or agent handle.

use super::handles::{agent_not_found, denied};
use super::state::{coordinator_id_for, lock_state, new_control, PaneKind};
use crate::operations::OperationError;

pub fn register_owned_pane(session_id: &str, pane_id: &str) -> Result<(), OperationError> {
    let mut state = lock_state();
    if let Some(pane) = state.panes.get_mut(pane_id) {
        let controls = pane.controller.as_deref() == Some(session_id) && !pane.released;
        // take_over(grant=true) records a preexisting pane as User. Dispatch calls this
        // twice; keep explicitly_taken_over so the second call does not drop the grant.
        if controls && (pane.owner == session_id || pane.explicitly_taken_over) {
            pane.kind = PaneKind::Agent;
            return Ok(());
        }
        return Err(denied(
            "pane is already managed by another MCP connection or requires explicit takeover",
        ));
    }
    let coordinator_id = coordinator_id_for(&mut state, session_id);
    state.panes.insert(
        pane_id.to_owned(),
        new_control(session_id, coordinator_id, PaneKind::Agent, false),
    );
    Ok(())
}

pub fn bind_agent_pane(
    session_id: &str,
    handle: &str,
    pane_id: &str,
    grant: bool,
) -> Result<(), OperationError> {
    let mut state = lock_state();
    let agent = state
        .agents
        .get(handle)
        .ok_or_else(|| agent_not_found(handle))?;
    if agent.controller.as_deref() != Some(session_id) || agent.released {
        return Err(denied("this connection does not control the agent"));
    }
    let pane = state.panes.get_mut(pane_id).ok_or_else(|| {
        denied("pane must be registered or taken over before it is assigned to an agent")
    })?;
    if pane.kind == PaneKind::User && !grant {
        return Err(denied(
            "assigning a preexisting pane to an agent requires grant=true",
        ));
    }
    if pane.controller.as_deref() != Some(session_id) || pane.released {
        return Err(denied("pane is actively controlled by another connection"));
    }
    pane.kind = PaneKind::Agent;
    pane.explicitly_taken_over = false;
    Ok(())
}

pub fn register_shell_pane(session_id: &str, pane_id: &str) -> Result<(), OperationError> {
    let mut state = lock_state();
    if let Some(pane) = state.panes.get(pane_id) {
        if pane.owner == session_id && pane.kind == PaneKind::Shell {
            return Ok(());
        }
        return Err(denied("pane is already managed by another MCP connection"));
    }
    let coordinator_id = coordinator_id_for(&mut state, session_id);
    state.panes.insert(
        pane_id.to_owned(),
        new_control(session_id, coordinator_id, PaneKind::Shell, false),
    );
    Ok(())
}

pub fn register_owned_agent(session_id: &str, handle: &str) -> Result<(), OperationError> {
    let mut state = lock_state();
    if let Some(agent) = state.agents.get(handle) {
        if agent.owner == session_id {
            return Ok(());
        }
        return Err(denied(
            "agent handle is already owned by another MCP connection",
        ));
    }
    let coordinator_id = coordinator_id_for(&mut state, session_id);
    state.agents.insert(
        handle.to_owned(),
        new_control(session_id, coordinator_id, PaneKind::Agent, false),
    );
    Ok(())
}
