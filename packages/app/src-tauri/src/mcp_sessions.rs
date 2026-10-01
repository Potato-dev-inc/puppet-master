use crate::operations::OperationError;
use crate::tool_registry::{self, McpMode, ToolDefinition, ToolSafety};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
struct PaneControl {
    owner: String,
    coordinator_id: String,
    controller: Option<String>,
    released: bool,
    explicitly_taken_over: bool,
    kind: PaneKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneKind {
    Agent,
    Shell,
    User,
}

#[derive(Debug, Clone)]
struct Session {
    mode: McpMode,
    coordinator_id: String,
    attach_token: String,
}

#[derive(Debug, Clone)]
struct CoordinatorRecord {
    attach_token: String,
    connection_id: String,
}

#[derive(Default)]
struct State {
    sessions: HashMap<String, Session>,
    coordinators: HashMap<String, CoordinatorRecord>,
    panes: HashMap<String, PaneControl>,
    agents: HashMap<String, PaneControl>,
}

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

static STATE: Lazy<Mutex<State>> = Lazy::new(|| {
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

/// Coordinator identity, attach tokens and run/pane leases survive a bridge restart so the
/// recorded owner can reclaim with attach_agents(attach_token). Tokens are never logged.
fn persist_path() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        Some(crate::app_paths::app_data_dir().join("mcp_sessions.json"))
    }
}

struct StateGuard {
    inner: std::sync::MutexGuard<'static, State>,
    dirty: bool,
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

fn lock_state() -> StateGuard {
    StateGuard {
        inner: STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        dirty: false,
    }
}

fn kind_str(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Agent => "agent",
        PaneKind::Shell => "shell",
        PaneKind::User => "user",
    }
}

fn control_to_json(control: &PaneControl) -> Value {
    json!({
        "owner": control.owner,
        "coordinator_id": control.coordinator_id,
        "controller": control.controller,
        "released": control.released,
        "taken_over": control.explicitly_taken_over,
        "kind": kind_str(control.kind),
    })
}

fn control_from_json(value: &Value) -> Option<PaneControl> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(PaneControl {
        owner: text("owner")?,
        coordinator_id: text("coordinator_id")?,
        controller: text("controller"),
        released: value.get("released").and_then(Value::as_bool).unwrap_or(false),
        explicitly_taken_over: value.get("taken_over").and_then(Value::as_bool).unwrap_or(false),
        kind: match value.get("kind").and_then(Value::as_str) {
            Some("shell") => PaneKind::Shell,
            Some("user") => PaneKind::User,
            _ => PaneKind::Agent,
        },
    })
}

fn snapshot_state(state: &State) -> Value {
    json!({
        "version": 1,
        "sessions": state.sessions.iter().map(|(id, s)| (id.clone(), json!({
            "mode": s.mode.as_str(), "coordinator_id": s.coordinator_id, "attach_token": s.attach_token,
        }))).collect::<serde_json::Map<_, _>>(),
        "coordinators": state.coordinators.iter().map(|(id, c)| (id.clone(), json!({
            "attach_token": c.attach_token, "connection_id": c.connection_id,
        }))).collect::<serde_json::Map<_, _>>(),
        "panes": state.panes.iter().map(|(id, c)| (id.clone(), control_to_json(c))).collect::<serde_json::Map<_, _>>(),
        "agents": state.agents.iter().map(|(id, c)| (id.clone(), control_to_json(c))).collect::<serde_json::Map<_, _>>(),
    })
}

/// Inserts persisted entries; existing in-memory entries win.
fn restore_snapshot(state: &mut State, snapshot: &Value) {
    let obj = |key: &str| snapshot.get(key).and_then(Value::as_object);
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    for (id, v) in obj("coordinators").into_iter().flatten() {
        if let (Some(attach_token), Some(connection_id)) =
            (text(v, "attach_token"), text(v, "connection_id"))
        {
            state
                .coordinators
                .entry(id.clone())
                .or_insert(CoordinatorRecord { attach_token, connection_id });
        }
    }
    for (id, v) in obj("sessions").into_iter().flatten() {
        if let (Some(coordinator_id), Some(attach_token)) =
            (text(v, "coordinator_id"), text(v, "attach_token"))
        {
            let mode = text(v, "mode")
                .and_then(|m| McpMode::parse(&m))
                .unwrap_or(McpMode::Agent);
            state
                .sessions
                .entry(id.clone())
                .or_insert(Session { mode, coordinator_id, attach_token });
        }
    }
    for (id, v) in obj("panes").into_iter().flatten() {
        if let Some(control) = control_from_json(v) {
            state.panes.entry(id.clone()).or_insert(control);
        }
    }
    for (id, v) in obj("agents").into_iter().flatten() {
        if let Some(control) = control_from_json(v) {
            state.agents.entry(id.clone()).or_insert(control);
        }
    }
}

fn ensure_session<'a>(state: &'a mut State, session_id: &str) -> &'a mut Session {
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

fn coordinator_id_for(state: &mut State, session_id: &str) -> String {
    ensure_session(state, session_id).coordinator_id.clone()
}

fn new_control(session_id: &str, coordinator_id: String, kind: PaneKind, taken: bool) -> PaneControl {
    PaneControl {
        owner: session_id.to_owned(),
        coordinator_id,
        controller: Some(session_id.to_owned()),
        released: false,
        explicitly_taken_over: taken,
        kind,
    }
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
            "mode must be agent, shell, or both",
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

fn coordinator_live_connection(state: &State, coordinator_id: &str) -> Option<String> {
    state
        .coordinators
        .get(coordinator_id)
        .map(|record| record.connection_id.clone())
}

fn controller_is_stale(state: &State, agent: &PaneControl) -> bool {
    match agent.controller.as_deref() {
        None => true,
        Some(controller) if agent.released => true,
        Some(controller) => coordinator_live_connection(state, &agent.coordinator_id)
            .is_some_and(|live| live != controller),
    }
}

fn control_denied(
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
            state.agents.insert(
                handle.to_owned(),
                new_control(session_id, coordinator_id, PaneKind::Agent, true),
            );
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

fn readonly_operation_route(method: &str, clean_path: &str) -> bool {
    if clean_path == "/operations/by-key" || clean_path == "/operations/by-handle" {
        return method == "GET";
    }
    let Some(rest) = clean_path.strip_prefix("/operations/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
    match parts.as_slice() {
        [id] if method == "GET" => !id.is_empty(),
        [id, "wait"] if method == "POST" => !id.is_empty(),
        _ => false,
    }
}

/// Enforce both the connection's catalog mode and pane ownership before a bridge route runs.
/// Calls without a session header retain the legacy local MCP behavior.
pub fn authorize(
    session_id: Option<&str>,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<(), OperationError> {
    let Some(session_id) = session_id.filter(|id| !id.trim().is_empty()) else {
        if method == "POST" && path == "/mcp/mode" {
            return Err(OperationError::new(
                "SESSION_REQUIRED",
                "set_mode requires an MCP connection session id",
                false,
            ));
        }
        let clean_path = path.split('?').next().unwrap_or(path);
        if clean_path == "/agents" || clean_path.starts_with("/agents/") {
            return Err(OperationError::new(
                "SESSION_REQUIRED",
                "agent APIs require an MCP session id",
                false,
            ));
        }
        let mutating = tool_registry::tools().iter().any(|tool| {
            method == tool.method
                && route_matches(tool.path, clean_path)
                && matches!(tool.safety, ToolSafety::Mutating | ToolSafety::Destructive)
        });
        let target_pane =
            pane_id_from_route(clean_path).or_else(|| body.get("pane_id").and_then(Value::as_str));
        if clean_path == "/agents/take-over"
            || clean_path == "/agents/release"
            || clean_path == "/agents/attach"
            || clean_path == "/agents/release-lease"
            || clean_path == "/agents/transfer"
            || clean_path == "/mcp/session"
        {
            return Err(OperationError::new(
                "SESSION_REQUIRED",
                "agent ownership operations require an MCP session id",
                false,
            ));
        }
        let handle = body
            .get("handle")
            .and_then(Value::as_str)
            .or_else(|| body.get("operation_id").and_then(Value::as_str))
            .or_else(|| {
                clean_path
                    .strip_prefix("/agents/")
                    .and_then(|rest| rest.strip_suffix("/transcript"))
            })
            .or_else(|| {
                clean_path
                    .strip_prefix("/operations/")
                    .and_then(|rest| rest.split('/').next())
            });
        if !readonly_operation_route(method, clean_path)
            && handle.is_some_and(|handle| lock_state().agents.contains_key(handle))
        {
            return Err(denied("managed agent operations require an MCP session id"));
        }
        let handles = body.get("handles").and_then(Value::as_array);
        if handles.is_some_and(|handles| {
            let state = lock_state();
            handles
                .iter()
                .filter_map(Value::as_str)
                .any(|handle| state.agents.contains_key(handle))
        }) {
            return Err(denied("managed agent operations require an MCP session id"));
        }
        if mutating && target_pane.is_some_and(|pane| lock_state().panes.contains_key(pane)) {
            return Err(denied("managed pane mutations require an MCP session id"));
        }
        return Ok(());
    };
    let clean_path = path.split('?').next().unwrap_or(path);
    if (method == "GET" && clean_path == "/mcp/tools")
        || (method == "GET" && clean_path == "/mcp/mode")
        || (method == "POST" && clean_path == "/mcp/mode")
        || (method == "GET" && clean_path == "/mcp/session")
    {
        return Ok(());
    }

    let definitions = tool_registry::tools();
    let tool = definitions
        .iter()
        .find(|tool| method == tool.method && route_matches(tool.path, clean_path))
        .ok_or_else(|| denied("route is not exposed to MCP sessions"))?;
    let mode = mode_for(session_id);
    if !tool_registry::tool_visible_in_mode(tool.name, mode) {
        let target_mode = if tool_registry::tool_visible_in_mode(tool.name, McpMode::Agent) {
            "agent"
        } else if tool_registry::tool_visible_in_mode(tool.name, McpMode::Shell) {
            "shell"
        } else {
            "both"
        };
        let mut error = OperationError::new(
            "MODE_MISMATCH",
            format!("{} is unavailable in {} mode", tool.name, mode.as_str()),
            false,
        );
        error.context = json!({"tool":tool.name,"current_mode":mode.as_str(),"available_in":target_mode,"switch_with":{"mode":target_mode}});
        return Err(error);
    }

    let handle = body
        .get("handle")
        .and_then(Value::as_str)
        .or_else(|| body.get("operation_id").and_then(Value::as_str))
        .or_else(|| {
            clean_path
                .strip_prefix("/operations/")
                .and_then(|rest| rest.split('/').next())
        })
        .or_else(|| {
            let rest = clean_path.strip_prefix("/agents/")?;
            match rest.split_once('/') {
                Some((candidate, suffix)) if suffix == "transcript" => Some(candidate),
                None if !rest.is_empty()
                    && rest != "run"
                    && rest != "wait"
                    && rest != "send"
                    && rest != "followup"
                    && rest != "answer"
                    && rest != "cancel"
                    && rest != "close"
                    && rest != "take-over"
                    && rest != "release"
                    && rest != "attach"
                    && rest != "release-lease"
                    && rest != "transfer" =>
                {
                    Some(rest)
                }
                _ => None,
            }
        });
    if matches!(
        tool.name,
        "wait_agents"
            | "send_agent"
            | "send_message"
            | "followup_task"
            | "answer_prompt"
            | "cancel_agent"
            | "interrupt_agent"
            | "inspect_agent"
            | "agent_transcript"
            | "close_agent"
            | "cancel_operation"
    ) {
        if tool.name == "wait_agents" {
            let handles = body
                .get("handles")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    OperationError::new(
                        "INVALID_ARGUMENT",
                        "handles must be a non-empty array",
                        false,
                    )
                })?;
            for handle in handles.iter().filter_map(Value::as_str) {
                check_run_wait_access(session_id, handle)?;
            }
        } else if tool.name == "inspect_agent" || tool.name == "agent_transcript" {
            if let Some(handle) = handle {
                check_run_read_access(session_id, handle)?;
            }
        } else if let Some(handle) = handle {
            check_run_access(session_id, handle)?;
        }
    }

    let pane_id =
        pane_id_from_route(clean_path).or_else(|| body.get("pane_id").and_then(Value::as_str));
    let is_pane_mutation = tool.name != "take_over"
        && matches!(tool.safety, ToolSafety::Mutating | ToolSafety::Destructive)
        && (pane_id.is_some() || clean_path.starts_with("/panes/"));
    if is_pane_mutation {
        if let Some(pane_id) = pane_id {
            if tool.name == "switch_agent_model" {
                if session_controls_pane(session_id, pane_id) {
                    return Ok(());
                }
                if let Some(handle) = body.get("handle").and_then(Value::as_str) {
                    if check_run_access(session_id, handle).is_ok() {
                        return Ok(());
                    }
                }
            }
            let pane = match lock_state().panes.get(pane_id).cloned() {
                Some(pane) => pane,
                None if tool.name == "run_agent" => {
                    // UI panes are adopted inside bind_existing_worker (take_over_pane grant=true).
                    return Ok(());
                }
                None => {
                    return Err(denied(
                        "pane is not adopted by this connection; call take_over with grant=true or run_agent(pane_id) to adopt a UI worker",
                    ));
                }
            };
            let agent_requires_takeover =
                pane.kind == PaneKind::Agent && !pane.explicitly_taken_over;
            if agent_requires_takeover && tool.name != "run_agent" {
                return Err(denied(
                    "agent panes are read-only to terminal controls until explicitly taken over",
                ));
            }
            if pane.released && tool.name != "run_agent" {
                return Err(denied(
                    "this pane was released; call take_over again (grant=true) to regain control",
                ));
            }
            if pane.controller.as_deref() != Some(session_id) {
                return Err(denied(
                    "this pane is controlled by another MCP coordinator; wait for release or use transfer_agent. run_agent(pane_id) cannot adopt a pane owned by another connection",
                ));
            }
        }
    }
    Ok(())
}

fn route_matches(template: &str, path: &str) -> bool {
    let expected: Vec<_> = template.trim_matches('/').split('/').collect();
    let actual: Vec<_> = path.trim_matches('/').split('/').collect();
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual.iter())
            .all(|(a, b)| (a.starts_with('{') && a.ends_with('}')) || a == b)
}

fn pane_id_from_route(path: &str) -> Option<&str> {
    let mut parts = path.trim_matches('/').split('/');
    (parts.next() == Some("panes"))
        .then(|| parts.next())
        .flatten()
}

fn pane_not_found(pane: &str) -> OperationError {
    OperationError::new(
        "PANE_NOT_FOUND",
        format!("pane {pane} is not registered to this MCP session"),
        false,
    )
}

const MIN_PANE_PREFIX_LEN: usize = 4;

/// Error for an agent handle this connection cannot resolve. The message is generic here;
/// `enrich_handle_error` upgrades it when the id turns out to be a pane id.
pub fn agent_not_found(handle: &str) -> OperationError {
    let mut error = OperationError::new(
        "AGENT_NOT_FOUND",
        format!(
            "agent handle {handle} is not registered to this MCP connection; expected an agent run handle from run_agent or list_workers for this project (runs started by another connection show as owned=false; UI panes appear in list_workers and can be adopted with run_agent(worker_id))"
        ),
        false,
    );
    error.context = json!({"handle": handle, "expected": "agent_run_handle"});
    error
}

/// If an AGENT_NOT_FOUND error names an id that is actually a pane id (or pane id prefix),
/// say so and point at the right parameter instead of a bare "not found".
pub fn enrich_handle_error(error: OperationError, pane_ids: &[String]) -> OperationError {
    enrich_handle_error_with(error, pane_ids, |_| None)
}

/// Exact wording for a pane id given where an agent run handle is required.
pub fn pane_id_as_handle_message(
    given: &str,
    kind: &str,
    pane_id: &str,
    run_handle: Option<&str>,
) -> String {
    let mut message = format!(
        "{given} is {kind}, not an agent run handle. This tool needs an agent run handle; get it from the run_agent result field `handle` or from list_agents `handle`."
    );
    match run_handle {
        Some(handle) => message.push_str(&format!(
            " Pane {pane_id} belongs to run handle `{handle}`; pass that handle instead."
        )),
        None => message.push_str(&format!(
            " Pane {pane_id} has no known run; to control a pane directly use take_over with pane_id (grant=true for panes this connection did not start)."
        )),
    }
    message
}

/// Like `enrich_handle_error`, with a lookup that maps a pane id to its latest run handle.
pub fn enrich_handle_error_with(
    mut error: OperationError,
    pane_ids: &[String],
    run_handle_for_pane: impl Fn(&str) -> Option<String>,
) -> OperationError {
    if error.code != "AGENT_NOT_FOUND" {
        return error;
    }
    let Some(handle) = error
        .context
        .get("handle")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return error;
    };
    if let Ok(pane) = resolve_pane_id(&handle, pane_ids) {
        let kind = if pane == handle {
            "a pane id"
        } else {
            "a pane id prefix"
        };
        let run_handle = run_handle_for_pane(&pane);
        error.message = pane_id_as_handle_message(&handle, kind, &pane, run_handle.as_deref());
        error.context = json!({"handle": handle, "expected": "agent_run_handle", "actual_kind": "pane_id", "pane_id": pane, "run_handle": run_handle});
    }
    error
}

/// Resolve a caller-supplied pane reference: a full pane id or a unique prefix (min 4 chars).
pub fn resolve_pane_id(input: &str, pane_ids: &[String]) -> Result<String, OperationError> {
    let input = input.trim();
    if pane_ids.iter().any(|id| id == input) {
        return Ok(input.to_owned());
    }
    let lower = input.to_ascii_lowercase();
    if lower.len() >= MIN_PANE_PREFIX_LEN {
        let matches: Vec<&String> = pane_ids
            .iter()
            .filter(|id| id.to_ascii_lowercase().starts_with(&lower))
            .collect();
        match matches.as_slice() {
            [one] => return Ok((*one).clone()),
            [] => {}
            many => {
                let mut error = OperationError::new(
                    "AMBIGUOUS_PANE_ID",
                    format!(
                        "pane id prefix {input} matches {} panes; pass a longer prefix or the full pane id",
                        many.len()
                    ),
                    false,
                );
                error.context = json!({"candidates": many});
                return Err(error);
            }
        }
    }
    let hint = if input.chars().all(|c| c.is_ascii_digit()) {
        " (looks like a UI display number, which is not a pane id)"
    } else if lower.len() < MIN_PANE_PREFIX_LEN {
        " (prefixes need at least 4 characters)"
    } else {
        ""
    };
    let mut error = OperationError::new(
        "PANE_NOT_FOUND",
        format!(
            "unknown pane: {input}{hint}; pane_id must be a full pane id or a unique prefix, see list_panes (agent run handles go in `handle`)"
        ),
        false,
    );
    error.context = json!({"pane_id": input, "expected": "pane_id"});
    Err(error)
}

fn denied(message: &str) -> OperationError {
    OperationError::new("AUTHORIZATION_DENIED", message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use uuid::Uuid;

    fn id() -> String {
        Uuid::new_v4().to_string()
    }

    #[test]
    fn sessions_start_in_agent_mode_and_catalog_is_filtered() {
        let first = id();
        let second = id();
        let agent = tools_for_session(&first);
        let names: HashSet<_> = agent.iter().map(|tool| tool.name).collect();
        for name in [
            "set_mode",
            "run_agent",
            "wait_agents",
            "send_message",
            "followup_task",
            "interrupt_agent",
            "inspect_agent",
            "agent_transcript",
            "close_agent",
        ] {
            assert!(names.contains(name), "agent catalog missing {name}");
        }
        assert!(names.contains("send_agent") && names.contains("cancel_agent"));
        assert!(names.contains("list_panes"));
        assert!(names.contains("list_workers"));
        assert!(names.contains("reply_opencode_question"));
        assert!(!names.contains("write_terminal_input"));
        assert!(!names.contains("spawn_agent"));
        let shell = set_mode(&second, "shell").unwrap();
        assert!(shell.iter().any(|tool| tool.name == "shell_exec"));
        assert!(!shell.iter().any(|tool| tool.name == "run_agent"));
        assert_eq!(mode_for(&first), McpMode::Agent);
    }

    #[test]
    fn invalid_mode_is_rejected_and_does_not_change_session() {
        let session = id();
        assert!(set_mode(&session, "root").is_err());
        assert_eq!(mode_for(&session), McpMode::Agent);
    }

    #[test]
    fn wrong_mode_calls_are_rejected_even_if_client_guesses_route() {
        let session = id();
        let error = authorize(
            Some(&session),
            "POST",
            "/panes/p1/input",
            &serde_json::json!({"text":"x"}),
        )
        .unwrap_err();
        assert_eq!(error.code, "MODE_MISMATCH");
        assert_eq!(
            error
                .context
                .pointer("/switch_with/mode")
                .and_then(Value::as_str),
            Some("shell")
        );
        set_mode(&session, "shell").unwrap();
        assert!(authorize(Some(&session), "POST", "/agents/run", &Value::Null).is_err());
        let shell_id = format!("shell-{}", Uuid::new_v4());
        register_shell_pane(&session, &shell_id).unwrap();
        assert!(authorize(
            Some(&session),
            "POST",
            "/shell/exec",
            &serde_json::json!({"command":"ls","pane_id":shell_id})
        )
        .is_ok());
        assert!(authorize(
            Some(&session),
            "POST",
            "/shell/exec",
            &serde_json::json!({"command":"ls","cwd":"C:/project"})
        )
        .is_ok());
    }

    #[test]
    fn managed_agent_pane_needs_explicit_shell_takeover_and_other_sessions_need_grant() {
        let owner = id();
        let other = id();
        register_owned_pane(&owner, "agent-pane-1").unwrap();
        set_mode(&owner, "shell").unwrap();
        let write = || {
            authorize(
                Some(&owner),
                "POST",
                "/panes/agent-pane-1/input",
                &serde_json::json!({"text":"x"}),
            )
        };
        assert!(write().is_err());
        take_over_pane(&owner, "agent-pane-1", true).unwrap();
        assert!(write().is_ok());
        assert!(take_over_pane(&other, "agent-pane-1", false).is_err());
        take_over_pane(&other, "agent-pane-1", true).unwrap();
        assert!(session_controls_pane(&other, "agent-pane-1"));
        assert!(!session_controls_pane(&owner, "agent-pane-1"));
    }

    #[test]
    fn release_returns_control_to_original_owner_without_removing_managed_pane_record() {
        let owner = id();
        register_owned_pane(&owner, "agent-pane-2").unwrap();
        release_owned_pane(&owner, "agent-pane-2").unwrap();
        assert!(session_controls_pane(&owner, "agent-pane-2"));
        take_over_pane(&owner, "agent-pane-2", false).unwrap();
    }

    #[test]
    fn registration_cannot_steal_a_managed_pane_or_agent() {
        let owner = id();
        let other = id();
        register_owned_pane(&owner, "protected-pane").unwrap();
        register_owned_agent(&owner, "protected-agent").unwrap();
        assert!(register_owned_pane(&other, "protected-pane").is_err());
        assert!(register_owned_agent(&other, "protected-agent").is_err());
        take_over_agent(&other, "protected-agent", true).unwrap();
        assert!(check_run_access(&other, "protected-agent").is_ok());
    }

    #[test]
    fn shell_panes_are_not_mistaken_for_read_only_agent_panes() {
        let shell = id();
        register_shell_pane(&shell, "shell-pane").unwrap();
        set_mode(&shell, "shell").unwrap();
        assert!(authorize(
            Some(&shell),
            "POST",
            "/panes/shell-pane/input",
            &serde_json::json!({"text":"ls"})
        )
        .is_ok());
    }

    #[test]
    fn shell_exec_targeting_agent_pane_requires_takeover() {
        let owner = id();
        register_owned_pane(&owner, "agent-pane-shell-exec").unwrap();
        set_mode(&owner, "shell").unwrap();
        let args = serde_json::json!({"command":"pytest","pane_id":"agent-pane-shell-exec"});
        assert!(authorize(Some(&owner), "POST", "/shell/exec", &args).is_err());
        take_over_pane(&owner, "agent-pane-shell-exec", true).unwrap();
        assert!(authorize(Some(&owner), "POST", "/shell/exec", &args).is_ok());
    }

    #[test]
    fn missing_session_cannot_mutate_a_managed_pane_through_key_or_shell_routes() {
        let owner = id();
        register_owned_pane(&owner, "agent-pane-no-header").unwrap();
        assert!(authorize(
            None,
            "POST",
            "/panes/agent-pane-no-header/key",
            &serde_json::json!({"key":"enter"})
        )
        .is_err());
        assert!(authorize(
            None,
            "POST",
            "/shell/exec",
            &serde_json::json!({"command":"ls","pane_id":"agent-pane-no-header"})
        )
        .is_err());
    }

    #[test]
    fn missing_session_cannot_use_agent_apis_even_for_unowned_handles() {
        for (method, path, body) in [
            ("GET", "/agents", Value::Null),
            ("POST", "/agents/run", serde_json::json!({"task":"work"})),
            (
                "POST",
                "/agents/cancel",
                serde_json::json!({"handle":"unknown-run"}),
            ),
        ] {
            let error = authorize(None, method, path, &body).unwrap_err();
            assert_eq!(error.code, "SESSION_REQUIRED");
        }
    }

    #[test]
    fn readonly_operation_wait_allowed_without_session_for_managed_runs() {
        let owner = id();
        register_owned_agent(&owner, "managed-op").unwrap();
        assert!(authorize(
            None,
            "POST",
            "/operations/managed-op/wait",
            &serde_json::json!({"timeout_ms": 1000}),
        )
        .is_ok());
        assert!(authorize(
            None,
            "GET",
            "/operations/managed-op",
            &Value::Null,
        )
        .is_ok());
    }

    #[test]
    fn agent_handles_are_private_until_the_owner_releases_them() {
        let owner = id();
        let other = id();
        set_mode(&owner, "both").unwrap();
        set_mode(&other, "both").unwrap();
        register_owned_agent(&owner, "owned-run").unwrap();
        assert!(authorize(
            Some(&owner),
            "GET",
            "/agents/owned-run/transcript",
            &Value::Null
        )
        .is_ok());
        assert!(authorize(
            Some(&other),
            "GET",
            "/agents/owned-run/transcript",
            &Value::Null
        )
        .is_err());
        take_over_agent(&other, "owned-run", true).unwrap();
        assert!(check_run_access(&other, "owned-run").is_ok());
        release_owned_agent(&other, "owned-run").unwrap();
        take_over_agent(&owner, "owned-run", true).unwrap();
        assert!(check_run_access(&owner, "owned-run").is_ok());
    }

    #[test]
    fn wait_agents_checks_every_handle_in_the_batch() {
        let owner = id();
        let other = id();
        register_owned_agent(&owner, "owner-run").unwrap();
        register_owned_agent(&other, "other-run").unwrap();
        let own = serde_json::json!({"handles":["owner-run"],"timeout_ms":1});
        let mixed = serde_json::json!({"handles":["owner-run","other-run"],"timeout_ms":1});
        assert!(authorize(Some(&owner), "POST", "/agents/wait", &own).is_ok());
        assert!(authorize(Some(&owner), "POST", "/agents/wait", &mixed).is_err());
    }

    #[test]
    fn run_agent_with_unadopted_pane_id_passes_authorize() {
        let session = id();
        set_mode(&session, "agent").unwrap();
        let body = serde_json::json!({"task": "work", "pane_id": "fresh-ui-pane"});
        assert!(authorize(Some(&session), "POST", "/agents/run", &body).is_ok());
    }

    #[test]
    fn run_agent_with_pane_owned_by_another_session_is_denied() {
        let owner = id();
        let other = id();
        register_owned_pane(&owner, "adopted-pane").unwrap();
        take_over_pane(&owner, "adopted-pane", true).unwrap();
        set_mode(&other, "agent").unwrap();
        let body = serde_json::json!({"task": "work", "pane_id": "adopted-pane"});
        let err = authorize(Some(&other), "POST", "/agents/run", &body).unwrap_err();
        assert_eq!(err.code, "AUTHORIZATION_DENIED");
        assert!(err.message.contains("another MCP coordinator"));
    }

    fn panes() -> Vec<String> {
        vec![
            "57c03490-a9bd-4e4e-bde8-b65f54d25be3".into(),
            "57c0ffff-0000-4e4e-bde8-b65f54d25be3".into(),
            "9a1b2c3d-a9bd-4e4e-bde8-b65f54d25be3".into(),
        ]
    }

    #[test]
    fn pane_ids_resolve_by_full_id_or_unique_prefix() {
        let ids = panes();
        assert_eq!(resolve_pane_id(&ids[0], &ids).unwrap(), ids[0]);
        assert_eq!(resolve_pane_id("57c03", &ids).unwrap(), ids[0]);
        assert_eq!(resolve_pane_id("9A1B", &ids).unwrap(), ids[2]);
    }

    #[test]
    fn ambiguous_short_and_numeric_pane_ids_fail_with_guidance() {
        let ids = panes();
        assert_eq!(
            resolve_pane_id("57c0", &ids).unwrap_err().code,
            "AMBIGUOUS_PANE_ID"
        );
        let numeric = resolve_pane_id("362", &ids).unwrap_err();
        assert_eq!(numeric.code, "PANE_NOT_FOUND");
        assert!(numeric.message.contains("UI display number"));
        assert!(numeric.message.contains("list_panes"));
        let short = resolve_pane_id("57c", &ids).unwrap_err();
        assert!(short.message.contains("at least 4"));
    }

    #[test]
    fn pane_id_prefix_passed_as_agent_handle_is_named_as_a_pane_id() {
        let ids = panes();
        let error = enrich_handle_error(agent_not_found("57c03490"), &ids);
        assert_eq!(error.code, "AGENT_NOT_FOUND");
        assert!(error
            .message
            .contains("pane id prefix, not an agent run handle"));
        assert!(error.message.contains(&ids[0]));
        let unrelated = enrich_handle_error(agent_not_found("c6399aed"), &ids);
        assert!(unrelated
            .message
            .contains("not registered to this MCP connection"));
        assert!(unrelated.message.contains("list_workers"));
    }

    #[test]
    fn pane_id_as_handle_message_names_kinds_sources_and_run_handle() {
        let ids = panes();
        let error = enrich_handle_error_with(agent_not_found(&ids[0]), &ids, |_| {
            Some("run-xyz".to_string())
        });
        assert!(error.message.contains("a pane id, not an agent run handle"));
        assert!(error.message.contains("run_agent result field `handle`"));
        assert!(error.message.contains("list_agents `handle`"));
        assert!(error.message.contains("`run-xyz`"));
        assert_eq!(error.context["run_handle"], "run-xyz");
    }

    #[test]
    fn check_run_access_failure_carries_the_handle_for_enrichment() {
        let error = check_run_access(&id(), "no-such-run").unwrap_err();
        assert_eq!(
            error.context.get("handle").and_then(Value::as_str),
            Some("no-such-run")
        );
    }

    #[test]
    fn resolving_a_prefix_never_bypasses_grant_for_unmanaged_panes() {
        let ids = panes();
        let session = id();
        let full = resolve_pane_id("57c03490", &ids).unwrap();
        assert!(take_over_pane(&session, &full, false).is_err());
        take_over_pane(&session, &full, true).unwrap();
        assert!(session_controls_pane(&session, &full));
    }

    #[test]
    fn taken_over_user_pane_can_be_registered_for_agent_dispatch() {
        let session = id();
        let other = id();
        take_over_pane(&session, "cursor-tui", true).unwrap();
        assert!(session_controls_pane(&session, "cursor-tui"));
        register_owned_pane(&session, "cursor-tui").unwrap();
        register_owned_pane(&session, "cursor-tui").unwrap();
        assert!(session_controls_pane(&session, "cursor-tui"));
        set_mode(&session, "both").unwrap();
        assert!(authorize(
            Some(&session),
            "POST",
            "/panes/cursor-tui/input",
            &serde_json::json!({"text": "x"}),
        )
        .is_ok());
        let denied = register_owned_pane(&other, "cursor-tui").unwrap_err();
        assert_eq!(denied.code, "AUTHORIZATION_DENIED");
    }

    #[test]
    fn answer_prompt_by_pane_needs_control_like_any_pane_write() {
        let owner = id();
        let other = id();
        register_owned_pane(&owner, "prompt-pane").unwrap();
        take_over_pane(&owner, "prompt-pane", true).unwrap();
        set_mode(&owner, "both").unwrap();
        set_mode(&other, "both").unwrap();
        let body =
            serde_json::json!({"pane_id":"prompt-pane","prompt_id":"p","choice":"allow_once"});
        assert!(authorize(Some(&owner), "POST", "/agents/answer", &body).is_ok());
        let denied = authorize(Some(&other), "POST", "/agents/answer", &body).unwrap_err();
        assert_eq!(denied.code, "AUTHORIZATION_DENIED");
        assert!(denied.message.contains("another MCP coordinator"));
        release_owned_pane(&owner, "prompt-pane").unwrap();
    }

    #[test]
    fn reconnect_attaches_with_token_without_the_old_connection() {
        let original = id();
        register_owned_agent(&original, "worker-a").unwrap();
        register_owned_pane(&original, "pane-a").unwrap();
        let identity = session_identity(&original);
        let replacement = id();
        let attached = attach_coordinator(&replacement, Some(&identity.attach_token), None).unwrap();
        assert_eq!(attached["coordinator_id"], identity.coordinator_id);
        assert!(check_run_access(&replacement, "worker-a").is_ok());
        assert!(check_run_access(&original, "worker-a").is_err());
        assert!(session_controls_pane(&replacement, "pane-a"));
        assert!(!session_controls_pane(&original, "pane-a"));
        let lease = inspect_lease("worker-a").unwrap();
        assert_eq!(lease.owner, replacement);
        assert_eq!(lease.coordinator_id, identity.coordinator_id);
        assert!(!lease.released);
    }

    /// Simulate a bridge restart for one session: persist, drop its in-memory entries, reload.
    fn restart_for(session: &str, handles: &[&str]) {
        let snapshot = {
            let state = lock_state();
            let mut snap = snapshot_state(&state);
            // Round-trip through text exactly as the file would.
            snap = serde_json::from_str(&snap.to_string()).unwrap();
            snap
        };
        let mut state = lock_state();
        let coordinator = state.sessions.remove(session).map(|s| s.coordinator_id);
        if let Some(coordinator) = coordinator {
            state.coordinators.remove(&coordinator);
        }
        for handle in handles {
            state.agents.remove(*handle);
            state.panes.remove(*handle);
        }
        restore_snapshot(&mut state, &snapshot);
    }

    #[test]
    fn restart_keeps_owner_access_and_token_reclaim() {
        let owner = id();
        let handle = format!("run-{}", id());
        register_owned_agent(&owner, &handle).unwrap();
        let identity = session_identity(&owner);
        restart_for(&owner, &[&handle]);
        // Same transport id still controls its run after restart.
        assert!(check_run_access(&owner, &handle).is_ok());
        assert_eq!(session_identity(&owner).attach_token, identity.attach_token);
        // A new connection with the valid token reclaims it.
        let replacement = id();
        let attached = attach_coordinator(&replacement, Some(&identity.attach_token), None).unwrap();
        assert!(attached["handles"].as_array().unwrap().iter().any(|h| h == handle.as_str()));
        assert!(check_run_access(&replacement, &handle).is_ok());
    }

    #[test]
    fn restart_reclaim_rejects_wrong_token_and_other_sessions() {
        let owner = id();
        let other = id();
        let handle = format!("run-{}", id());
        register_owned_agent(&owner, &handle).unwrap();
        let identity = session_identity(&owner);
        restart_for(&owner, &[&handle]);
        let attacker = id();
        assert_eq!(
            attach_coordinator(&attacker, Some("not-the-token"), None).unwrap_err().code,
            "AUTHORIZATION_DENIED"
        );
        assert_eq!(
            attach_coordinator(&attacker, None, Some(&identity.coordinator_id)).unwrap_err().code,
            "AUTHORIZATION_DENIED"
        );
        assert!(check_run_access(&attacker, &handle).is_err());
        assert!(check_run_access(&other, &handle).is_err());
        assert!(take_over_agent(&other, &handle, false).is_err());
    }

    #[test]
    fn unauthorized_takeover_stays_blocked_even_with_a_coordinator_name() {
        let owner = id();
        let other = id();
        register_owned_agent(&owner, "locked-worker").unwrap();
        assert!(take_over_agent(&other, "locked-worker", false).is_err());
        assert_eq!(
            attach_coordinator(&other, None, Some("alice")).unwrap_err().code,
            "AUTHORIZATION_DENIED"
        );
        take_over_agent(&other, "locked-worker", true).unwrap();
        assert!(check_run_access(&other, "locked-worker").is_ok());
    }

    #[test]
    fn return_pane_control_does_not_drop_the_worker_lease() {
        let owner = id();
        register_owned_agent(&owner, "leased-worker").unwrap();
        register_owned_pane(&owner, "manual-pane").unwrap();
        take_over_pane(&owner, "manual-pane", true).unwrap();
        return_pane_control(&owner, "manual-pane").unwrap();
        assert!(check_run_access(&owner, "leased-worker").is_ok());
        let lease = inspect_lease("leased-worker").unwrap();
        assert!(!lease.released);
        assert_eq!(lease.controller.as_deref(), Some(owner.as_str()));
        assert!(session_controls_pane(&owner, "manual-pane"));
    }

    #[test]
    fn take_over_agent_grant_reclaims_without_prior_control() {
        let owner = id();
        let other = id();
        register_owned_agent(&owner, "todo-builder").unwrap();
        take_over_agent(&other, "todo-builder", true).unwrap();
        assert!(check_run_access(&other, "todo-builder").is_ok());
        assert!(check_run_read_access(&owner, "todo-builder").is_ok());
    }

    fn lease_release_is_not_ownership_transfer() {
        let owner = id();
        let other = id();
        set_mode(&other, "agent").unwrap();
        register_owned_agent(&owner, "movable").unwrap();
        let before = inspect_lease("movable").unwrap();
        release_agent_lease(&owner, "movable").unwrap();
        let released = inspect_lease("movable").unwrap();
        assert!(released.released);
        assert!(released.controller.is_none());
        assert_eq!(released.owner, owner);
        assert_eq!(released.coordinator_id, before.coordinator_id);
        assert!(check_run_access(&owner, "movable").is_err());
        take_over_agent(&other, "movable", true).unwrap();
        assert_eq!(inspect_lease("movable").unwrap().owner, owner);

        register_owned_agent(&owner, "giftable").unwrap();
        transfer_agent(&owner, "giftable", Some(&other), None).unwrap();
        let transferred = inspect_lease("giftable").unwrap();
        assert!(!transferred.released);
        assert_eq!(transferred.owner, other);
        assert_eq!(transferred.controller.as_deref(), Some(other.as_str()));
        assert_ne!(transferred.coordinator_id, before.coordinator_id);
        assert!(check_run_access(&other, "giftable").is_ok());
        assert!(check_run_access(&owner, "giftable").is_err());
    }

    #[test]
    fn caller_supplied_coordinator_name_does_not_grant_authority() {
        let owner = id();
        let other = id();
        register_owned_agent(&owner, "named-worker").unwrap();
        let identity = session_identity(&owner);
        assert!(attach_coordinator(&other, None, Some(&identity.coordinator_id)).is_err());
        assert!(attach_coordinator(&other, Some("not-the-token"), Some("alice")).is_err());
        assert!(transfer_agent(&other, "named-worker", None, Some("alice")).is_err());
        assert!(transfer_agent(&owner, "named-worker", Some("alice"), Some("alice")).is_err());
        assert!(check_run_access(&owner, "named-worker").is_ok());
        assert!(check_run_access(&other, "named-worker").is_err());
    }
}
