//! Session state: who owns and controls which pane and agent lease, behind one process-wide
//! lock whose guard persists the state to disk when it was mutated.

#[cfg(not(test))]
use super::persist::restore_snapshot;
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
}

#[derive(Debug, Clone)]
pub(super) struct CoordinatorRecord {
    pub(super) attach_token: String,
    pub(super) connection_id: String,
}

#[derive(Default)]
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

impl Drop for StateGuard {
    fn drop(&mut self) {
        if self.dirty {
            if let Some(path) = persist_path() {
                let text = snapshot_state(&self.inner).to_string();
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, text).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
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
            },
        );
    }
    state.sessions.get_mut(session_id).expect("session just inserted")
}

pub(super) fn coordinator_id_for(state: &mut State, session_id: &str) -> String {
    ensure_session(state, session_id).coordinator_id.clone()
}

pub(super) fn new_control(session_id: &str, coordinator_id: String, kind: PaneKind, taken: bool) -> PaneControl {
    PaneControl {
        owner: session_id.to_owned(),
        coordinator_id,
        controller: Some(session_id.to_owned()),
        released: false,
        explicitly_taken_over: taken,
        kind,
    }
}
