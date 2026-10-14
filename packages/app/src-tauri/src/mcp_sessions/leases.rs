//! Lease movement: release, takeover, transfer and token-authenticated coordinator reattach.

use super::handles::{agent_not_found, denied, pane_not_found};
use super::state::{coordinator_id_for, ensure_session, lock_state, new_control, PaneKind, PaneOrigin};
use crate::operations::OperationError;
use serde_json::{json, Value};

pub fn release_owned_agent(session_id: &str, handle: &str) -> Result<(), OperationError> {
    let mut state = lock_state();
    let agent = state
        .agents
        .get_mut(handle)
        .ok_or_else(|| agent_not_found(handle))?;
    if agent.controller.as_deref() != Some(session_id) {
        return Err(denied("this connection does not control the agent"));
    }
    agent.controller = Some(agent.owner.clone());
    agent.released = false;
    agent.explicitly_taken_over = false;
    Ok(())
}

pub fn take_over_agent(session_id: &str, handle: &str, grant: bool) -> Result<(), OperationError> {
    let mut state = lock_state();
    match state.agents.get_mut(handle) {
        Some(agent) => {
            let controlled_by_other = agent
                .controller
                .as_deref()
                .is_some_and(|controller| controller != session_id);
            if controlled_by_other && !agent.released && !grant {
                return Err(denied(
                    "taking over an agent controlled by another connection requires grant=true",
                ));
            }
            let needs_grant = agent.owner != session_id || agent.released || controlled_by_other;
            if needs_grant && !grant {
                return Err(denied("taking over an agent requires grant=true"));
            }
            agent.owner = session_id.to_owned();
            agent.controller = Some(session_id.to_owned());
            agent.released = false;
            agent.explicitly_taken_over = true;
        }
        None if grant => {
            let coordinator_id = coordinator_id_for(&mut state, session_id);
            let mut control = new_control(session_id, coordinator_id, PaneKind::Agent, true);
            // A handle nobody registered was not started by this connection: it is adopted work.
            control.origin = PaneOrigin::UserOpened;
            state.agents.insert(handle.to_owned(), control);
        }
        None => return Err(agent_not_found(handle)),
    }
    Ok(())
}

pub fn release_owned_pane(session_id: &str, pane_id: &str) -> Result<(), OperationError> {
    let mut state = lock_state();
    let pane = state
        .panes
        .get_mut(pane_id)
        .ok_or_else(|| pane_not_found(pane_id))?;
    if pane.controller.as_deref() != Some(session_id) {
        return Err(denied("this connection does not control the pane"));
    }
    pane.controller = Some(pane.owner.clone());
    pane.released = false;
    pane.explicitly_taken_over = false;
    Ok(())
}

pub fn take_over_pane(session_id: &str, pane_id: &str, grant: bool) -> Result<(), OperationError> {
    let mut state = lock_state();
    match state.panes.get_mut(pane_id) {
        Some(pane) => {
            let controlled_by_other = pane
                .controller
                .as_ref()
                .is_some_and(|owner| owner != session_id);
            if controlled_by_other && !grant {
                return Err(denied(
                    "pane is actively controlled by another MCP connection",
                ));
            }
            if controlled_by_other && grant {
                // ponytail: grant=true is explicit preemption for cross-host adoption (e.g. Claude after Cursor).
                pane.controller = Some(session_id.to_owned());
                pane.released = false;
                pane.explicitly_taken_over = true;
                return Ok(());
            }
            if (pane.released || pane.owner != session_id) && !grant {
                return Err(OperationError::new(
                    "AUTHORIZATION_DENIED",
                    "taking over a pane requires grant=true",
                    false,
                ));
            }
            pane.controller = Some(session_id.to_owned());
            pane.released = false;
            pane.explicitly_taken_over = true;
        }
        None if grant => {
            let coordinator_id = coordinator_id_for(&mut state, session_id);
            state.panes.insert(
                pane_id.to_owned(),
                new_control(session_id, coordinator_id, PaneKind::User, true),
            );
        }
        None => {
            return Err(OperationError::new(
                "AUTHORIZATION_DENIED",
                "taking over an unmanaged pane requires grant=true",
                false,
            ))
        }
    }
    Ok(())
}

pub fn session_controls_pane(session_id: &str, pane_id: &str) -> bool {
    lock_state()
        .panes
        .get(pane_id)
        .is_some_and(|pane| pane.controller.as_deref() == Some(session_id) && !pane.released)
}

/// Return temporary manual pane control to the lease owner. Does not drop the worker lease.
pub fn return_pane_control(session_id: &str, pane_id: &str) -> Result<(), OperationError> {
    release_owned_pane(session_id, pane_id)
}

/// Drop the worker control lease. Distinct from returning pane control and from transfer.
pub fn release_agent_lease(session_id: &str, handle: &str) -> Result<(), OperationError> {
    let mut state = lock_state();
    let coordinator_id = coordinator_id_for(&mut state, session_id);
    let agent = state
        .agents
        .get_mut(handle)
        .ok_or_else(|| agent_not_found(handle))?;
    let holds = agent.controller.as_deref() == Some(session_id)
        || agent.owner == session_id
        || agent.coordinator_id == coordinator_id;
    if !holds {
        return Err(denied("this connection does not hold the worker lease"));
    }
    agent.released = true;
    agent.controller = None;
    agent.explicitly_taken_over = false;
    Ok(())
}

/// Move the worker lease to another live connection. A coordinator name is not a credential.
pub fn transfer_agent(
    session_id: &str,
    handle: &str,
    to_session_id: Option<&str>,
    coordinator_name: Option<&str>,
) -> Result<Value, OperationError> {
    let Some(to_session_id) = to_session_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(denied(if coordinator_name.is_some() {
            "a coordinator name does not grant authority; to_session_id of a live connection is required"
        } else {
            "to_session_id of a live connection is required"
        }));
    };
    let mut state = lock_state();
    if !state.sessions.contains_key(to_session_id) {
        return Err(denied(
            "transfer target must be a live MCP connection, not a name",
        ));
    }
    let from_coordinator = coordinator_id_for(&mut state, session_id);
    let to_coordinator = coordinator_id_for(&mut state, to_session_id);
    let agent = state
        .agents
        .get_mut(handle)
        .ok_or_else(|| agent_not_found(handle))?;
    if agent.coordinator_id != from_coordinator && agent.owner != session_id {
        return Err(denied(
            "only the owning coordinator can transfer this worker",
        ));
    }
    agent.owner = to_session_id.to_string();
    agent.coordinator_id = to_coordinator.clone();
    agent.controller = Some(to_session_id.to_string());
    agent.released = false;
    agent.explicitly_taken_over = false;
    Ok(json!({
        "handle": handle,
        "from_connection": session_id,
        "to_connection": to_session_id,
        "coordinator_id": to_coordinator,
    }))
}

/// Authenticated reconnect: a new connection UUID presents the server-issued attach token.
/// A caller-supplied coordinator name never grants authority.
pub fn attach_coordinator(
    session_id: &str,
    attach_token: Option<&str>,
    coordinator_name: Option<&str>,
) -> Result<Value, OperationError> {
    let Some(token) = attach_token.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(denied(if coordinator_name.is_some() {
            "a coordinator name does not grant authority; attach_token is required"
        } else {
            "attach_token is required"
        }));
    };
    let mut state = lock_state();
    let found = state.coordinators.iter().find_map(|(id, record)| {
        (record.attach_token == token)
            .then(|| (id.clone(), record.connection_id.clone(), record.attach_token.clone()))
    });
    let Some((coordinator_id, old_connection, stored_token)) = found else {
        return Err(denied("invalid attach token"));
    };
    let new_session = ensure_session(&mut state, session_id);
    new_session.coordinator_id = coordinator_id.clone();
    new_session.attach_token = stored_token.clone();
    if let Some(record) = state.coordinators.get_mut(&coordinator_id) {
        record.connection_id = session_id.to_string();
    }
    let mut handles = Vec::new();
    for (handle, agent) in state.agents.iter_mut() {
        if agent.coordinator_id != coordinator_id {
            continue;
        }
        if agent.owner == old_connection {
            agent.owner = session_id.to_string();
        }
        if agent.controller.as_deref() == Some(old_connection.as_str()) {
            agent.controller = Some(session_id.to_string());
        }
        handles.push(handle.clone());
    }
    let mut panes = Vec::new();
    for (pane_id, pane) in state.panes.iter_mut() {
        if pane.coordinator_id != coordinator_id {
            continue;
        }
        if pane.owner == old_connection {
            pane.owner = session_id.to_string();
        }
        if pane.controller.as_deref() == Some(old_connection.as_str()) {
            pane.controller = Some(session_id.to_string());
        }
        panes.push(pane_id.clone());
    }
    Ok(json!({
        "connection_id": session_id,
        "coordinator_id": coordinator_id,
        "handles": handles,
        "panes": panes,
    }))
}
