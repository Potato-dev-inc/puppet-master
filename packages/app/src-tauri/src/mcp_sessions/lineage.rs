//! Lineage: which connection started a pane or a run, and whether the caller may treat it as its
//! own. `agent_authority` asks these questions before an agent takes over something.
//!
//! A pane is in a connection's lineage when that connection started it, when another connection of
//! the same coordinator did (a reconnect keeps the coordinator), or when a helper of the connection
//! did. A helper is a connection linked to its parent with `link_child_session`. The link is made by
//! the bridge, never from a header the helper sends about itself, so an agent cannot claim another
//! agent's lineage. Panes the person opened are in nobody's lineage.

use super::handles::denied;
use super::state::{lock_state, PaneControl, PaneOrigin, State};
use crate::operations::OperationError;

/// How far up a helper chain to look; also stops a malformed (cyclic) table from looping.
const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relation {
    /// The caller already holds the lease. Nothing to ask.
    Controls,
    /// Started by the caller, by its coordinator, or by one of its helpers.
    Own,
    /// Opened by the person, perhaps adopted by some agent since.
    UserOpened,
    /// Started by an agent that is not the caller and not the caller's helper.
    OtherAgent,
}

fn coordinator_of(state: &State, session: &str) -> Option<String> {
    state.sessions.get(session).map(|s| s.coordinator_id.clone())
}

/// True when `spawner` is `ancestor`, shares its coordinator, or has `ancestor` somewhere up its
/// chain of parents.
fn in_lineage_of(state: &State, ancestor: &str, spawner: &str) -> bool {
    let ancestor_coordinator = coordinator_of(state, ancestor);
    let mut current = spawner.to_owned();
    for _ in 0..MAX_DEPTH {
        if current == ancestor || (ancestor_coordinator.is_some() && coordinator_of(state, &current) == ancestor_coordinator) {
            return true;
        }
        match state.sessions.get(&current).and_then(|s| s.parent.clone()) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
    false
}

fn relation(state: &State, session: &str, control: Option<&PaneControl>, judge_control: bool) -> Relation {
    let Some(control) = control else {
        return Relation::UserOpened;
    };
    if judge_control && control.controller.as_deref() == Some(session) && !control.released {
        return Relation::Controls;
    }
    match &control.origin {
        PaneOrigin::UserOpened => Relation::UserOpened,
        PaneOrigin::Spawned { by_session } if in_lineage_of(state, session, by_session) => Relation::Own,
        PaneOrigin::Spawned { .. } => Relation::OtherAgent,
    }
}

/// The caller's relation to a pane. A pane this table never saw was opened by the person.
pub fn relation_to_pane(session_id: &str, pane_id: &str) -> Relation {
    let state = lock_state();
    relation(&state, session_id, state.panes.get(pane_id), true)
}

/// Where a pane came from, whoever holds its lease now: `Own`, `UserOpened` or `OtherAgent`. An
/// agent that was allowed to take over a person's pane still does not "own" it.
pub fn lineage_of_pane(session_id: &str, pane_id: &str) -> Relation {
    let state = lock_state();
    relation(&state, session_id, state.panes.get(pane_id), false)
}

/// The caller's relation to an agent run handle.
pub fn relation_to_agent(session_id: &str, handle: &str) -> Relation {
    let state = lock_state();
    relation(&state, session_id, state.agents.get(handle), true)
}

/// Who started a pane, for the Inbox card and the audit line. `None` for a pane nobody adopted.
pub fn pane_origin(pane_id: &str) -> Option<PaneOrigin> {
    lock_state().panes.get(pane_id).map(|p| p.origin.clone())
}

/// Record that `child` is a helper of `parent`: the child's agent runs inside a pane the parent
/// started. Refuses a link to itself, to an unknown connection, or one that would make a loop.
pub fn link_child_session(child: &str, parent: &str) -> Result<(), OperationError> {
    if child == parent {
        return Err(denied("a connection cannot be its own helper"));
    }
    let mut state = lock_state();
    if !state.sessions.contains_key(parent) {
        return Err(denied("the parent connection is not known"));
    }
    if in_lineage_of(&state, child, parent) {
        return Err(denied("that link would make a loop"));
    }
    super::state::ensure_session(&mut state, child).parent = Some(parent.to_owned());
    Ok(())
}

/// What each connection said its MCP host is called (`initialize` → `clientInfo.name`), for the
/// words on a consent card. It is the caller's own claim, so it is cut down to plain characters and
/// only ever used as a label, never to decide anything.
static SESSION_CLIENTS: once_cell::sync::Lazy<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn note_session_client(session_id: &str, name: &str) {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .take(40)
        .collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() {
        return;
    }
    if let Ok(mut clients) = SESSION_CLIENTS.lock() {
        if clients.get(session_id) != Some(&clean) {
            clients.insert(session_id.to_string(), clean);
        }
    }
}

pub fn session_client_name(session_id: &str) -> Option<String> {
    SESSION_CLIENTS.lock().ok()?.get(session_id).cloned()
}
