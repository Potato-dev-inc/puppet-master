//! Starting a chat session: the gate (Labs switch, the agent's choice or an explicit ask), the session, its pane.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use super::{AcpChat, ChatHost, CHATS, LANE};
use crate::acp::config::{AgentConfig, HostEnv};
use crate::acp::conversations::Conversations;
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
    /// The saved conversations. With them a chat session is a conversation (the pane's id is the conversation's):
    /// it survives an app restart and can be handed to a terminal and back. Without them it lives as long as the app.
    pub conversations: Option<Conversations>,
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
    let label = cfg.label.clone();
    let agent_id = cfg.id.clone();
    let (session, pane_id) = match &deps.conversations {
        Some(conversations) => {
            let view = conversations.create(&agent_id, &folder).map_err(|e| StartError::new("ACP_START_FAILED", format!("The agent could not start: {e}"), true))?;
            let session = conversations.session(&view.id).ok_or_else(|| StartError::new("ACP_START_FAILED", "The agent could not start.", true))?;
            (session, view.id)
        }
        None => {
            let pane_id = uuid::Uuid::new_v4().to_string();
            let session = AcpSession::new(Arc::new(cfg), deps.env.clone(), SessionParams { key: pane_id.clone(), cwd: folder.clone(), resume_session_id: None });
            supervisor::global().insert(session.clone());
            (session, pane_id)
        }
    };
    let chat = AcpChat::build(&pane_id, &label, session.clone(), deps.host.clone(), deps.conversations.clone());
    chat.attach();
    CHATS.lock().insert(pane_id.clone(), chat.clone());
    pushed::register(&pane_id, chat.clone());
    SWEEPER.get_or_init(|| supervisor::global().start_thread());

    if let Err(error) = session.ensure_running() {
        chat.retire();
        if let Some(conversations) = &deps.conversations {
            conversations.discard(&pane_id);
        }
        let needs_sign_in = matches!(error, crate::acp::error::AcpError::AuthRequired { .. });
        let message = if needs_sign_in { "The agent needs you to sign in first.".to_string() } else { start_failure_sentence(&label, &error) };
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

/// What the person reads when the chat program of `label` would not start: one cause, one thing to do. No path and no
/// raw system text (a path can carry a user name), and the terminal is always named as the way out.
pub fn start_failure_sentence(label: &str, error: &crate::acp::error::AcpError) -> String {
    use crate::acp::error::AcpError;
    match error {
        AcpError::Spawn(_) => format!("The chat program for {label} was not found or would not run. Check that it is installed, or run the agent in a terminal."),
        AcpError::Pin(_) => format!("The chat add-on for {label} is not the version this app was tested with. Install it again in Settings, under Agents, or run the agent in a terminal."),
        AcpError::Timeout { .. } => format!("{label} did not answer in time. It may still be starting or may need a sign-in. Try again, or run the agent in a terminal."),
        AcpError::Closed => format!("The chat program for {label} closed right away. Run the agent in a terminal once to see why (it may need a sign-in or an update)."),
        other => format!("The agent could not start: {other}"),
    }
}

/// After an app start: a chat pane for every conversation that came back (`Conversations::restore_lazily`). The pane
/// is listed at once; nothing starts until a viewer reads it (`AcpChat::wake_for_reader`) or a message is sent. A
/// conversation that is open in a terminal pane (the terminal survived) shows that. Returns the pane ids.
pub fn restore_panes(deps: &StartDeps) -> Vec<String> {
    let Some(conversations) = &deps.conversations else { return Vec::new() };
    let mut restored = Vec::new();
    for id in conversations.open_ids() {
        if CHATS.lock().contains_key(&id) {
            continue;
        }
        let (Some(session), Some(record)) = (conversations.session(&id), conversations.record(&id)) else { continue };
        let Ok(cfg) = conversations.config_for(&record.agent) else { continue };
        let chat = AcpChat::build(&id, &cfg.label, session, deps.host.clone(), Some(conversations.clone()));
        chat.attach();
        CHATS.lock().insert(id.clone(), chat.clone());
        pushed::register(&id, chat.clone());
        SWEEPER.get_or_init(|| supervisor::global().start_thread());
        let on_close = {
            let id = id.clone();
            Arc::new(move || super::on_pane_closed(&id)) as Arc<dyn Fn() + Send + Sync>
        };
        let handles = insert_virtual_pane(
            &deps.registry,
            VirtualPaneSpec { pane_id: id.clone(), agent_type: record.agent.clone(), cwd: record.cwd.clone(), lane: LANE.to_string() },
            on_close,
        );
        chat.set_handles(handles);
        restored.push(id);
    }
    if !restored.is_empty() {
        deps.host.panes_changed();
    }
    restored
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
    fn a_failed_start_says_one_cause_and_one_thing_to_do_without_a_path() {
        use crate::acp::error::AcpError;
        let spawn = start_failure_sentence("Kilo", &AcpError::Spawn("C:/Users/someone/kilo.exe: program not found (os error 2)".into()));
        assert_eq!(spawn, "The chat program for Kilo was not found or would not run. Check that it is installed, or run the agent in a terminal.");
        assert!(!spawn.contains("someone") && !spawn.contains("os error"));
        assert!(start_failure_sentence("Kilo", &AcpError::Pin("hash".into())).contains("Install it again in Settings"));
        assert!(start_failure_sentence("Kilo", &AcpError::Timeout { method: "session/new".into(), secs: 30 }).contains("did not answer in time"));
        assert!(start_failure_sentence("Kilo", &AcpError::Closed).contains("closed right away"));
        assert!(start_failure_sentence("Kilo", &AcpError::QueueFull).starts_with("The agent could not start"));
    }

    #[test]
    fn every_refusal_offers_the_terminal_instead() {
        for error in [GateError::LabsOff, GateError::NoDefinition, GateError::TerminalChosen] {
            let start: StartError = error.into();
            assert!(start.fallback, "{start}");
        }
    }
}
