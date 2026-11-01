//! Starting a chat session: the gate (Labs switch, the agent's choice or an explicit ask), the session, its pane.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use super::{AcpChat, ChatHost, CHATS, LANE};
use crate::acp::config::{AgentConfig, HostEnv};
use crate::acp::lane::RunLane;
use crate::acp::session::{AcpSession, SessionParams};
use crate::acp::supervisor::{self, SweeperHandle};
use crate::pty::registry::{insert_virtual_pane, VirtualPaneSpec};
use crate::pty::PaneRegistry;
use crate::session_chat::pushed;

/// What starting a chat session needs from the app.
#[derive(Clone)]
pub struct StartDeps {
    pub registry: Arc<Mutex<PaneRegistry>>,
    pub host: Arc<dyn ChatHost>,
    pub env: Arc<dyn HostEnv>,
}

pub struct StartRequest {
    /// The agent's id (`"codex"`): it must have an ACP definition (`acp::config`).
    pub agent_id: String,
    /// The folder the agent works in.
    pub folder: PathBuf,
    /// The caller asks for a chat session whatever the agent's saved choice is (the Labs switch still applies).
    pub explicit: bool,
}

/// Why a chat session did not start. `fallback`: the terminal is the way to run this agent instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    pub code: &'static str,
    pub message: String,
    pub fallback: bool,
}

impl StartError {
    fn new(code: &'static str, message: impl Into<String>, fallback: bool) -> Self {
        StartError { code, message: message.into(), fallback }
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Why the gate said no.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    /// The Labs switch is off: nothing starts.
    LabsOff,
    /// The agent has no ACP definition.
    NoDefinition,
    /// The agent runs in the terminal (its saved choice) and the caller did not ask for a chat session.
    TerminalChosen,
}

impl From<GateError> for StartError {
    fn from(error: GateError) -> Self {
        match error {
            GateError::LabsOff => StartError::new("ACP_LABS_OFF", "Chat sessions are off. Turn on Chat (ACP) in Labs first.", true),
            GateError::NoDefinition => StartError::new("ACP_NO_DEFINITION", "This agent has no chat (ACP) way to run yet.", true),
            GateError::TerminalChosen => StartError::new("ACP_LANE_NOT_CHOSEN", "This agent runs in the terminal. Choose Chat (ACP) for it, or ask for a chat session explicitly.", true),
        }
    }
}

/// The rule: Labs on, an ACP definition, and the agent's choice is Chat or the caller asks for it.
pub fn check_gate(labs_on: bool, has_definition: bool, chosen: RunLane, explicit: bool) -> Result<(), GateError> {
    if !labs_on {
        return Err(GateError::LabsOff);
    }
    if !has_definition {
        return Err(GateError::NoDefinition);
    }
    if chosen != RunLane::Acp && !explicit {
        return Err(GateError::TerminalChosen);
    }
    Ok(())
}

/// Starts a chat session for a built-in agent, behind the gate. Returns the pane id.
pub fn start_gated(deps: &StartDeps, request: StartRequest) -> Result<Arc<AcpChat>, StartError> {
    let cfg = crate::acp::config::find_builtin(&request.agent_id);
    let chosen = crate::acp::lane::LaneStore::in_app_data().lane_for(&request.agent_id);
    check_gate(crate::acp::chat_enabled(), cfg.is_some(), chosen, request.explicit)?;
    let Some(cfg) = cfg else { return Err(GateError::NoDefinition.into()) };
    start(deps, cfg, request)
}

static SWEEPER: OnceLock<SweeperHandle> = OnceLock::new();

/// Starts a chat session with `cfg` (no gate). The adapter is started and the session opened before the pane exists,
/// so a failure leaves nothing behind and the caller can run the agent in a terminal instead (`fallback`).
pub fn start(deps: &StartDeps, cfg: AgentConfig, request: StartRequest) -> Result<Arc<AcpChat>, StartError> {
    let folder = request.folder.clone();
    if !Path::new(&folder).is_absolute() || !folder.is_dir() {
        return Err(StartError::new("INVALID_ARGUMENT", "The folder must be an existing folder (a full path).", false));
    }
    // The add-on (the adapter) is not installed: say so, so the screen can offer to install it (A5).
    if crate::acp::install::is_missing(&cfg, &*deps.env) {
        let size = cfg.addon.size_mb.map(|mb| format!(" (about {mb} MB)")).unwrap_or_default();
        return Err(StartError::new("ACP_ADDON_MISSING", format!("Chat needs a small add-on for {}{size}. Install it first.", cfg.label), true));
    }
    let pane_id = uuid::Uuid::new_v4().to_string();
    let label = cfg.label.clone();
    let agent_id = cfg.id.clone();
    let session = AcpSession::new(Arc::new(cfg), deps.env.clone(), SessionParams { key: pane_id.clone(), cwd: folder.clone(), resume_session_id: None });
    let chat = AcpChat::build(&pane_id, &label, session.clone(), deps.host.clone());
    chat.attach();
    CHATS.lock().insert(pane_id.clone(), chat.clone());
    pushed::register(&pane_id, chat.clone());
    supervisor::global().insert(session.clone());
    SWEEPER.get_or_init(|| supervisor::global().start_thread());

    if let Err(error) = session.ensure_running() {
        chat.retire();
        let needs_sign_in = matches!(error, crate::acp::error::AcpError::AuthRequired { .. });
        let message = if needs_sign_in { "The agent needs you to sign in first.".to_string() } else { format!("The agent could not start: {error}") };
        return Err(StartError::new(if needs_sign_in { "ACP_SIGN_IN_NEEDED" } else { "ACP_START_FAILED" }, message, true));
    }

    let on_close = {
        let pane_id = pane_id.clone();
        Arc::new(move || super::on_pane_closed(&pane_id)) as Arc<dyn Fn() + Send + Sync>
    };
    let handles = insert_virtual_pane(
        &deps.registry,
        VirtualPaneSpec { pane_id: pane_id.clone(), agent_type: agent_id, cwd: folder.to_string_lossy().into_owned(), lane: LANE.to_string() },
        on_close,
    );
    chat.set_handles(handles);
    deps.host.panes_changed();
    deps.host.state_changed(&pane_id, &chat.state());
    Ok(chat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_needs_labs_a_definition_and_a_choice_or_an_explicit_ask() {
        assert_eq!(check_gate(false, true, RunLane::Acp, true), Err(GateError::LabsOff));
        assert_eq!(check_gate(true, false, RunLane::Acp, true), Err(GateError::NoDefinition));
        assert_eq!(check_gate(true, true, RunLane::Terminal, false), Err(GateError::TerminalChosen));
        assert_eq!(check_gate(true, true, RunLane::Acp, false), Ok(()));
        assert_eq!(check_gate(true, true, RunLane::Terminal, true), Ok(()));
    }

    #[test]
    fn every_refusal_offers_the_terminal_instead() {
        for error in [GateError::LabsOff, GateError::NoDefinition, GateError::TerminalChosen] {
            let start: StartError = error.into();
            assert!(start.fallback, "{start}");
        }
    }
}
