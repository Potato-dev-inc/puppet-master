//! "Open in terminal" and "Back to chat": one conversation, two views, one live writer.
//!
//! The agent keeps the conversation in its own session files; the chat reads and writes it through the
//! ACP adapter, a terminal pane through the agent's normal terminal program. They are taken in turns:
//!
//! ```text
//!   Open in terminal                           Back to chat
//!   1. no turn running (or `force`: stop it)   1. the terminal is not mid-turn (or `force`)
//!   2. adapter stopped, session held shut      2. the pane is stopped and confirmed gone
//!   3. writer: chat -> terminal (atomic)       3. writer: terminal -> chat (atomic)
//!   4. pane starts with the agent's resume     4. hold released; the next start uses session/load,
//!      command for the same session id            which replays what the terminal added
//! ```
//!
//! A failure at any step leaves exactly one writer: a pane that would not start gives the conversation
//! back to the chat; a pane that would not stop keeps it with the terminal. The resume command is data in
//! the agent's `[acp]` table (`resume_command`); the pane itself is started and stopped through
//! [`PaneHost`], so the logic here has no window or process code of its own.

use serde::Serialize;

use super::conversations::{ConversationError, Conversations};
use super::error::AcpError;
use super::record::{LastState, NoticeKind};
use super::session::{Readiness, StartKind};
use super::writers::Writer;
use crate::session_chat::history::resume_args_from;

use std::time::Duration;

/// How long a turn that was asked to stop gets before the hand-over gives up.
const STOP_WAIT: Duration = Duration::from_secs(15);

/// What the pane's agent is doing, as far as the app can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneActivity {
    Gone,
    Idle,
    Working,
    NeedsInput,
}

/// What a terminal pane is started with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalStart {
    pub agent_type: String,
    pub cwd: String,
    /// The agent's resume command, filled in for this session.
    pub extra_args: Vec<String>,
}

/// The app's panes, as the hand-over needs them. The real one is in `commands/acp_conversations.rs`.
pub trait PaneHost: Send + Sync {
    /// Starts the pane and returns its id.
    fn start(&self, start: &TerminalStart) -> Result<String, String>;
    fn activity(&self, pane_id: &str) -> PaneActivity;
    /// Stops the pane and returns once its processes are stopped.
    fn stop(&self, pane_id: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandOverError {
    #[error("{0}")]
    Conversation(ConversationError),
    #[error("the conversation has no session yet: send a first message in the chat, or open a normal terminal")]
    NoSessionYet,
    #[error("this agent has no way to continue a conversation in its terminal")]
    NoResumeCommand,
    /// A turn is running in the chat. `force` stops it first.
    #[error("{}", if *.waiting_for_answer { "the agent is waiting for an answer in the chat" } else { "the agent is working in the chat" })]
    TurnRunning { waiting_for_answer: bool },
    /// The terminal's agent is working. `force` stops the pane anyway.
    #[error("the agent in the terminal is still working")]
    TerminalBusy { activity: PaneActivity },
    #[error("the conversation is already open in a terminal")]
    AlreadyInTerminal,
    #[error("the conversation is not open in a terminal")]
    NotInTerminal,
    #[error("another writer has the conversation: {0}")]
    OtherWriter(String),
    #[error("the terminal could not be started: {0}")]
    StartFailed(String),
    #[error("the terminal could not be stopped, so the chat did not take the conversation: {0}")]
    StopFailed(String),
    #[error("{0}")]
    Chat(AcpError),
}

impl From<ConversationError> for HandOverError {
    fn from(e: ConversationError) -> Self {
        HandOverError::Conversation(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HandedOver {
    pub conversation: String,
    pub pane_id: String,
    pub agent: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackInChat {
    pub conversation: String,
    pub session_id: Option<String>,
    /// `loaded` (history replayed), `resumed`, `new`, or `history_lost`.
    pub how: &'static str,
}

fn how_word(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::AlreadyRunning => "resumed",
        Readiness::Started(StartKind::New) => "new",
        Readiness::Started(StartKind::Resumed) => "resumed",
        Readiness::Started(StartKind::Loaded) => "loaded",
        Readiness::Started(StartKind::HistoryLost) => "history_lost",
    }
}

impl Conversations {
    /// "Open in terminal". `force`: a running turn is stopped first instead of being a reason to ask.
    pub fn open_in_terminal(&self, id: &str, force: bool, panes: &dyn PaneHost) -> Result<HandedOver, HandOverError> {
        let record = self.record(id).ok_or(ConversationError::Unknown)?;
        if self.writer(id).is_some_and(|w| w.is_terminal()) {
            return Err(HandOverError::AlreadyInTerminal);
        }
        let cfg = self.config_for(&record.agent)?;
        let session_id = record.session_id.clone().ok_or(HandOverError::NoSessionYet)?;
        let extra_args = resume_args_from(&cfg.resume_command, &session_id).ok_or(HandOverError::NoResumeCommand)?;
        let session = self.live_session(id)?;

        // 1. No turn running, or stop it on purpose.
        let snapshot = session.snapshot();
        if snapshot.turn_in_flight() {
            if !force {
                return Err(HandOverError::TurnRunning { waiting_for_answer: !snapshot.permission_waits.is_empty() });
            }
            session.cancel().map_err(HandOverError::Chat)?;
            let stopped = session.wait_for(|s| !s.turn_in_flight(), STOP_WAIT);
            if stopped.is_none() {
                return Err(HandOverError::TurnRunning { waiting_for_answer: false });
            }
        }

        // 2. The adapter goes, and the session refuses to start one while the terminal has the conversation.
        session.hold("open in a terminal").map_err(|e| match e {
            AcpError::Busy => HandOverError::TurnRunning { waiting_for_answer: false },
            other => HandOverError::Chat(other),
        })?;

        // 3. The writer moves from the chat to a terminal that is starting.
        let starting = Writer::Terminal { pane_id: None };
        if let Err(conflict) = self.0.writers.swap(id, &Writer::Chat, starting.clone()) {
            session.release_hold();
            return Err(HandOverError::OtherWriter(conflict.to_string()));
        }

        // 4. The terminal starts on the same session.
        let start = TerminalStart { agent_type: cfg.id.clone(), cwd: record.cwd.clone(), extra_args };
        match panes.start(&start) {
            Ok(pane_id) => {
                let _ = self.0.writers.swap(id, &starting, Writer::Terminal { pane_id: Some(pane_id.clone()) });
                let pane = pane_id.clone();
                self.update(id, |r| {
                    r.last_known_state = LastState::Terminal;
                    r.terminal_pane = Some(pane);
                    r.push_notice(NoticeKind::HandedToTerminal, "Opened in a terminal. The chat is paused until you come back to it.", super::conversations::now_ms());
                });
                Ok(HandedOver { conversation: id.to_string(), pane_id, agent: cfg.id.clone(), session_id })
            }
            Err(why) => {
                // The conversation goes back to the chat: one writer, as before.
                let _ = self.0.writers.swap(id, &starting, Writer::Chat);
                session.release_hold();
                Err(HandOverError::StartFailed(why))
            }
        }
    }

    /// "Back to chat". `force`: a terminal that is mid-turn is stopped anyway.
    pub fn back_to_chat(&self, id: &str, force: bool, panes: &dyn PaneHost) -> Result<BackInChat, HandOverError> {
        let record = self.record(id).ok_or(ConversationError::Unknown)?;
        let holder = self.writer(id);
        let Some(Writer::Terminal { pane_id }) = holder else { return Err(HandOverError::NotInTerminal) };
        let pane = pane_id.or(record.terminal_pane.clone());

        // 1 and 2. The pane is stopped, and only a pane that is confirmed gone lets the chat take over.
        if let Some(pane) = &pane {
            match panes.activity(pane) {
                activity @ (PaneActivity::Working | PaneActivity::NeedsInput) if !force => {
                    return Err(HandOverError::TerminalBusy { activity });
                }
                PaneActivity::Gone => {}
                _ => panes.stop(pane).map_err(HandOverError::StopFailed)?,
            }
            if panes.activity(pane) != PaneActivity::Gone {
                return Err(HandOverError::StopFailed("it is still running".into()));
            }
        }

        // 3. The writer moves back, and the next start replays the conversation.
        // Stopping the pane may already have told us it ended (`terminal_closed`), which moved the claim.
        if let Err(conflict) = self.0.writers.swap(id, &Writer::Terminal { pane_id: pane.clone() }, Writer::Chat) {
            if conflict.held_by != Writer::Chat {
                return Err(HandOverError::OtherWriter(conflict.to_string()));
            }
        }
        self.take_back(id)
    }

    /// A terminal pane ended on its own (the person closed it): the chat has the conversation again, lazily.
    /// Returns the conversation, if the pane was one that had been handed a conversation.
    pub fn terminal_closed(&self, pane_id: &str) -> Option<String> {
        let id = self.0.records.lock().iter().find(|r| r.terminal_pane.as_deref() == Some(pane_id) && r.last_known_state == LastState::Terminal).map(|r| r.id.clone())?;
        let holder = Writer::Terminal { pane_id: Some(pane_id.to_string()) };
        self.0.writers.swap(&id, &holder, Writer::Chat).ok()?;
        let session = self.live_session(&id).ok()?;
        session.release_hold();
        session.replay_on_next_start();
        // The chat of this conversation starts its history over from the replay (it holds what was said before).
        super::chat::history_will_be_replayed(&id);
        self.update(&id, |r| {
            r.last_known_state = LastState::Live;
            r.terminal_pane = None;
            r.push_notice(NoticeKind::TerminalEnded, "The terminal was closed. The chat has the conversation again.", super::conversations::now_ms());
        });
        Some(id)
    }

    /// The chat holds the ledger claim already: let the session start again and bring it back.
    fn take_back(&self, id: &str) -> Result<BackInChat, HandOverError> {
        let session = self.live_session(id)?;
        session.release_hold();
        session.replay_on_next_start();
        super::chat::history_will_be_replayed(id);
        self.update(id, |r| {
            r.last_known_state = LastState::Live;
            r.terminal_pane = None;
        });
        // The conversation is the chat's even if the adapter will not start now; the caller can retry.
        let readiness = session.ensure_running().map_err(HandOverError::Chat)?;
        self.update(id, |r| r.push_notice(NoticeKind::BackInChat, "Back in the chat. What was done in the terminal is in the conversation.", super::conversations::now_ms()));
        let session_id = self.record(id).and_then(|r| r.session_id);
        Ok(BackInChat { conversation: id.to_string(), session_id, how: how_word(readiness) })
    }
}
