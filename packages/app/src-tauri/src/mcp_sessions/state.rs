//! Session state: who owns and controls which pane and agent lease, behind one process-wide
//! lock whose guard persists the state to disk when it was mutated.

#[cfg(not(test))]
use super::persist::restore_snapshot;
use super::coalesce::Coalescer;
use super::persist::{persist_path, snapshot_state};
use crate::tool_registry::McpMode;
use once_cell::sync::Lazy;
#[cfg(not(test))]
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub(super) struct PaneControl {
    pub(super) owner: String,
    pub(super) coordinator_id: String,
    pub(super) controller: Option<String>,
    pub(super) released: bool,
    pub(super) explicitly_taken_over: bool,
    pub(super) kind: PaneKind,
    /// Who made this pane or run: the lineage that agent authority (`agent_authority`) judges.
    pub(super) origin: PaneOrigin,
}

/// Where a pane or run came from. A person's own panes carry no session, so no agent is ever
/// "in the lineage" of one; an agent's helpers are in the lineage of the session that started them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneOrigin {
    /// Opened by the person (or by something this table never saw), then perhaps adopted by an agent.
    UserOpened,
    /// Started by an MCP connection (`run_agent`, `spawn_agent`, a shell pane it created).
    Spawned { by_session: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneKind {
    Agent,
    Shell,
    User,
}

#[derive(Debug, Clone)]
pub(super) struct Session {
    pub(super) mode: McpMode,
    pub(super) coordinator_id: String,
    pub(super) attach_token: String,
    /// The connection whose helper this one is (it runs inside a pane that connection started).
    /// Set only through `link_child_session`, never from a caller-supplied header.
    pub(super) parent: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CoordinatorRecord {
    pub(super) attach_token: String,
    pub(super) connection_id: String,
}

#[derive(Default, Clone)]
pub(super) struct State {
    pub(super) sessions: HashMap<String, Session>,
    pub(super) coordinators: HashMap<String, CoordinatorRecord>,
    pub(super) panes: HashMap<String, PaneControl>,
    pub(super) agents: HashMap<String, PaneControl>,
}

pub(super) static STATE: Lazy<Mutex<State>> = Lazy::new(|| {
    #[cfg_attr(test, allow(unused_mut))]
    let mut state = State::default();
    #[cfg(not(test))]
    if let Some(path) = persist_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(snapshot) = serde_json::from_str::<Value>(&text) {
                restore_snapshot(&mut state, &snapshot);
            }
        }
    }
    Mutex::new(state)
});

pub(super) struct StateGuard {
    pub(super) inner: std::sync::MutexGuard<'static, State>,
    pub(super) dirty: bool,
}

impl std::ops::Deref for StateGuard {
    type Target = State;
    fn deref(&self) -> &State {
        &self.inner
    }
}

impl std::ops::DerefMut for StateGuard {
    fn deref_mut(&mut self) -> &mut State {
        self.dirty = true;
        &mut self.inner
    }
}

/// Background writer for the persisted table: the guard publishes a clone and moves on, so the
/// process-wide lock is never held across serialization or disk I/O. Identical consecutive
/// snapshots (a take_over that changed nothing) are not rewritten.
static WRITER: Lazy<Option<std::sync::Arc<Coalescer<State>>>> = Lazy::new(|| {
    let path = persist_path()?;
    let mut last_written: Option<String> = None;
    Some(Coalescer::spawn("mcp-sessions-writer", move |state: State| {
        let text = snapshot_state(&state).to_string();
        if last_written.as_deref() == Some(text.as_str()) {
            return;
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &text).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
            last_written = Some(text);
        }
    }))
});

impl Drop for StateGuard {
    fn drop(&mut self) {
        if self.dirty {
            if let Some(writer) = WRITER.as_ref() {
                writer.submit((*self.inner).clone());
            }
        }
    }
}

pub(super) fn lock_state() -> StateGuard {
    StateGuard {
        inner: STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        dirty: false,
    }
}

pub(super) fn ensure_session<'a>(state: &'a mut State, session_id: &str) -> &'a mut Session {
    if !state.sessions.contains_key(session_id) {
        let coordinator_id = uuid::Uuid::new_v4().to_string();
        let attach_token = uuid::Uuid::new_v4().to_string();
        state.coordinators.insert(
            coordinator_id.clone(),
            CoordinatorRecord {
                attach_token: attach_token.clone(),
                connection_id: session_id.to_owned(),
            },
        );
        state.sessions.insert(
            session_id.to_owned(),
            Session {
                mode: McpMode::Agent,
                coordinator_id,
                attach_token,
                parent: None,
            },
        );
    }
    state.sessions.get_mut(session_id).expect("session just inserted")
}

pub(super) fn coordinator_id_for(state: &mut State, session_id: &str) -> String {
    ensure_session(state, session_id).coordinator_id.clone()
}

pub(super) fn new_control(session_id: &str, coordinator_id: String, kind: PaneKind, taken: bool) -> PaneControl {
    // A pane first recorded as `User` is one the connection adopted, not one it started.
    let origin = if kind == PaneKind::User {
        PaneOrigin::UserOpened
    } else {
        PaneOrigin::Spawned { by_session: session_id.to_owned() }
    };
    PaneControl {
        owner: session_id.to_owned(),
        coordinator_id,
        controller: Some(session_id.to_owned()),
        released: false,
        explicitly_taken_over: taken,
        kind,
        origin,
    }
}
