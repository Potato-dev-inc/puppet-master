//! Chat conversations: the record of each one, its session, and who may write to it.
//!
//! [`Conversations`] ties three things together for every conversation: the persisted record
//! (`record.rs`), the live `AcpSession` kept by the supervisor, and the writer ledger (`writers.rs`).
//! What it adds to the core:
//!
//! * **The record stays true.** The agent's own session id is learned at the first start (and replaced if
//!   the agent had to start a new session); both are written to the record as they happen.
//! * **Restart.** [`Conversations::restore_lazily`] rebuilds a session for every conversation that was
//!   live. Nothing is started: the adapter starts on the first prompt (or [`Conversations::open_chat`]),
//!   and it then brings the conversation back with `session/load`, which replays it, because the chat view
//!   of a new app run has no history of its own. If the agent cannot find the session the core starts a
//!   new one and says so; that arrives here as a notice on the record.
//! * **Hand-over** to a terminal and back (`handover.rs`), with one live writer at a time.
//!
//! Agent specifics come in as data (`AgentSource` gives the `[acp]` table); nothing here names an agent.

use std::path::Path;
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;

use super::config::{AgentConfig, HostEnv};
use super::error::AcpError;
use super::lane::RunLane;
use super::record::{ConversationRecord, LastState, Notice, NoticeKind, RecordStore};
use super::session::{AcpSession, Readiness, SessionEvent, SessionParams};
use super::supervisor::Supervisor;
use super::writers::{Writer, WriterLedger};

/// Where the `[acp]` table of an agent comes from. The real source is the profile registry; tests give a table.
pub trait AgentSource: Send + Sync {
    fn config(&self, agent: &str) -> Option<Arc<AgentConfig>>;
}

/// The agents registered with the app (code profiles and approved definitions).
pub struct RegisteredAgents;

impl AgentSource for RegisteredAgents {
    fn config(&self, agent: &str) -> Option<Arc<AgentConfig>> {
        super::config::find_builtin(agent).map(Arc::new)
    }
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(super) struct Inner {
    pub(super) records: Mutex<Vec<ConversationRecord>>,
    pub(super) store: RecordStore,
    pub(super) writers: WriterLedger,
    pub(super) supervisor: Arc<Supervisor>,
    pub(super) host: Arc<dyn HostEnv>,
    pub(super) agents: Arc<dyn AgentSource>,
}

#[derive(Clone)]
pub struct Conversations(pub(super) Arc<Inner>);

/// A conversation as a viewer sees it.
#[derive(Debug, Clone, Serialize)]
pub struct ConversationView {
    pub id: String,
    pub agent: String,
    pub session_id: Option<String>,
    pub cwd: String,
    pub last_known_state: LastState,
    pub terminal_pane: Option<String>,
    pub writer: Option<Writer>,
    pub notices: Vec<Notice>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConversationError {
    #[error("no such conversation")]
    Unknown,
    #[error("{0:?} has no chat (ACP) definition")]
    NoDefinition(String),
    #[error("chat for {agent:?} is off: {note}")]
    Disabled { agent: String, note: String },
    #[error("{0}")]
    Chat(AcpError),
}

/// What a restart did for one conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    /// Brought back as a chat (not started yet).
    pub restored: Vec<String>,
    /// Still in a terminal pane that survived.
    pub in_terminal: Vec<String>,
    /// Could not be brought back (the agent is gone, or it is off in this build).
    pub failed: Vec<String>,
}

impl Conversations {
    pub fn new(store: RecordStore, supervisor: Arc<Supervisor>, host: Arc<dyn HostEnv>, agents: Arc<dyn AgentSource>) -> Self {
        let records = store.load();
        store.sweep_orphan_journals(&records);
        Conversations(Arc::new(Inner { records: Mutex::new(records), store, writers: WriterLedger::new(), supervisor, host, agents }))
    }

    // ---- reading ---------------------------------------------------------------------------------------

    pub fn list(&self) -> Vec<ConversationView> {
        let records = self.0.records.lock().clone();
        records.iter().map(|r| self.view_of(r)).collect()
    }

    pub fn get(&self, id: &str) -> Option<ConversationView> {
        let record = self.record(id)?;
        Some(self.view_of(&record))
    }

    pub fn record(&self, id: &str) -> Option<ConversationRecord> {
        self.0.records.lock().iter().find(|r| r.id == id).cloned()
    }

    fn view_of(&self, r: &ConversationRecord) -> ConversationView {
        ConversationView {
            id: r.id.clone(),
            agent: r.agent.clone(),
            session_id: r.session_id.clone(),
            cwd: r.cwd.clone(),
            last_known_state: r.last_known_state,
            terminal_pane: r.terminal_pane.clone(),
            writer: self.0.writers.holder(&r.id),
            notices: r.notices.clone(),
        }
    }

    /// The journal file of a conversation (`journal.rs`).
    pub fn journal_path(&self, id: &str) -> std::path::PathBuf {
        self.0.store.journal_path(id)
    }

    pub fn session(&self, id: &str) -> Option<Arc<AcpSession>> {
        self.0.supervisor.get(id)
    }

    /// The session of a conversation that is not closed, built again if the supervisor lost it (the Labs
    /// switch was turned off and on). A conversation a terminal has stays held shut.
    pub fn live_session(&self, id: &str) -> Result<Arc<AcpSession>, ConversationError> {
        if let Some(session) = self.session(id) {
            return Ok(session);
        }
        let record = self.record(id).ok_or(ConversationError::Unknown)?;
        if record.last_known_state == LastState::Closed {
            return Err(ConversationError::Unknown);
        }
        self.restore_one(&record, true)?;
        let session = self.session(id).ok_or(ConversationError::Unknown)?;
        if self.writer(id).is_some_and(|w| w.is_terminal()) {
            let _ = session.hold("open in a terminal");
        }
        Ok(session)
    }

    /// Who may write to the conversation right now.
    pub fn writer(&self, id: &str) -> Option<Writer> {
        self.0.writers.holder(id)
    }

    /// Claims the conversation for a writer. Refused while somebody else has it (the ledger's rule).
    pub fn claim_writer(&self, id: &str, who: Writer) -> Result<(), super::writers::WriterConflict> {
        self.0.writers.claim(id, who)
    }

    /// Who has the agent's own session `session_id` of `agent`, if one of our conversations does. For entry
    /// points that start a terminal on a session by themselves (resuming an old chat): they must not start a
    /// second writer on a conversation the chat has open.
    pub fn writer_of_session(&self, agent: &str, session_id: &str) -> Option<(String, Writer)> {
        let records = self.0.records.lock();
        let record = records.iter().find(|r| r.agent.eq_ignore_ascii_case(agent) && r.session_id.as_deref() == Some(session_id) && r.last_known_state != LastState::Closed)?;
        let writer = self.0.writers.holder(&record.id)?;
        Some((record.id.clone(), writer))
    }

    // ---- changing --------------------------------------------------------------------------------------

    /// Starts a new chat conversation. Nothing runs yet: the adapter starts with the first prompt.
    pub fn create(&self, agent: &str, cwd: &Path) -> Result<ConversationView, ConversationError> {
        let cfg = self.config_for(agent)?;
        let id = format!("acp-{}", uuid::Uuid::new_v4().simple());
        let now = now_ms();
        let record = ConversationRecord {
            id: id.clone(),
            agent: cfg.id.clone(),
            session_id: None,
            cwd: cwd.to_string_lossy().into_owned(),
            lane: RunLane::Acp,
            last_known_state: LastState::Live,
            terminal_pane: None,
            notices: Vec::new(),
            created_ms: now,
            updated_ms: now,
        };
        self.0.records.lock().push(record.clone());
        self.save();
        self.build_session(&id, cfg, cwd, None, false);
        let _ = self.0.writers.claim(&id, Writer::Chat);
        Ok(self.view_of(&record))
    }

    /// Brings an agent's own old session (one the person had in a terminal or another app) into the chat as a new
    /// conversation. Nothing runs yet: the first read starts the adapter, which loads the session and replays it.
    pub fn import(&self, agent: &str, cwd: &Path, session_id: &str) -> Result<ConversationView, ConversationError> {
        let cfg = self.config_for(agent)?;
        let id = format!("acp-{}", uuid::Uuid::new_v4().simple());
        let now = now_ms();
        let record = ConversationRecord {
            id: id.clone(),
            agent: cfg.id.clone(),
            session_id: Some(session_id.to_string()),
            cwd: cwd.to_string_lossy().into_owned(),
            lane: RunLane::Acp,
            last_known_state: LastState::Live,
            terminal_pane: None,
            notices: Vec::new(),
            created_ms: now,
            updated_ms: now,
        };
        self.0.records.lock().push(record.clone());
        self.save();
        self.build_session(&id, cfg, cwd, Some(session_id.to_string()), true);
        let _ = self.0.writers.claim(&id, Writer::Chat);
        Ok(self.view_of(&record))
    }

    /// Starts the adapter and opens the session if it is not running (a viewer opening the chat).
    pub fn open_chat(&self, id: &str) -> Result<Readiness, ConversationError> {
        let session = self.live_session(id)?;
        session.ensure_running().map_err(ConversationError::Chat)
    }

    /// The session of a conversation stops and leaves the supervisor; the record stays as it is.
    pub fn forget_session(&self, id: &str) {
        self.0.supervisor.remove(id);
    }

    /// Drops a conversation that never got going (its first start failed): the record, the session, the claim.
    pub fn discard(&self, id: &str) {
        self.0.supervisor.remove(id);
        self.0.writers.forget(id);
        let changed = {
            let mut records = self.0.records.lock();
            let before = records.len();
            records.retain(|r| r.id != id);
            records.len() != before
        };
        if changed {
            super::journal::Journal::at(self.journal_path(id)).remove();
            self.save();
        }
    }

    /// The conversations a chat pane can be built for after an app start: live ones, with a session in the
    /// supervisor (nothing is started). A conversation that is in a terminal pane is included: its pane shows
    /// that, and offers "Back to chat".
    pub fn open_ids(&self) -> Vec<String> {
        let records = self.0.records.lock().clone();
        records.into_iter().filter(|r| r.last_known_state != LastState::Closed && self.session(&r.id).is_some()).map(|r| r.id).collect()
    }

    /// The conversation whose terminal pane is `pane_id`, if that pane holds one.
    pub fn of_terminal(&self, pane_id: &str) -> Option<ConversationView> {
        let record = self.0.records.lock().iter().find(|r| r.terminal_pane.as_deref() == Some(pane_id) && r.last_known_state == LastState::Terminal).cloned()?;
        Some(self.view_of(&record))
    }

    /// The person closed the conversation: the adapter goes, the record stays (as closed).
    pub fn close(&self, id: &str) -> Result<(), ConversationError> {
        self.record(id).ok_or(ConversationError::Unknown)?;
        self.0.supervisor.remove(id);
        self.0.writers.forget(id);
        self.update(id, |r| {
            r.last_known_state = LastState::Closed;
            r.terminal_pane = None;
        });
        Ok(())
    }

    /// The person removed Chat for `agent`: each of its live conversations stops (adapter gone, no restore) and keeps a
    /// notice. The record stays, so the conversation is still listed and can be opened in a terminal. A conversation
    /// that a terminal pane holds is left alone. Returns the ids. Idempotent: a closed conversation is skipped.
    pub fn chat_removed(&self, agent: &str) -> Vec<String> {
        let live: Vec<String> = self
            .0
            .records
            .lock()
            .iter()
            .filter(|r| r.agent.eq_ignore_ascii_case(agent) && r.last_known_state == LastState::Live)
            .map(|r| r.id.clone())
            .collect();
        for id in &live {
            self.0.supervisor.remove(id);
            self.0.writers.forget(id);
            self.update(id, |r| {
                r.last_known_state = LastState::Closed;
                r.terminal_pane = None;
                r.push_notice(NoticeKind::ChatRemoved, "Chat removed. Open this conversation in a terminal, or add Chat again.", now_ms());
            });
        }
        live
    }

    /// The person has seen the notices of a conversation.
    pub fn mark_notices_seen(&self, id: &str) {
        self.update(id, |r| r.notices.iter_mut().for_each(|n| n.seen = true));
        if let Some(session) = self.session(id) {
            session.acknowledge_history_lost();
        }
    }

    /// After an app start: every conversation that was live gets its session back, lazily. A conversation
    /// that was in a terminal is looked for there: if the pane is still around it stays handed over,
    /// otherwise the chat takes it back (and will replay what the terminal added).
    pub fn restore_lazily(&self, panes: &dyn super::handover::PaneHost) -> RestoreReport {
        let mut report = RestoreReport::default();
        let records = self.0.records.lock().clone();
        for record in records {
            match record.last_known_state {
                LastState::Closed => continue,
                LastState::Terminal => {
                    let pane = record.terminal_pane.clone().filter(|pane| panes.activity(pane) != super::handover::PaneActivity::Gone);
                    if let Some(pane) = pane {
                        if self.restore_one(&record, true).is_ok() {
                            self.0.writers.swap(&record.id, &Writer::Chat, Writer::Terminal { pane_id: Some(pane) }).ok();
                            if let Some(session) = self.session(&record.id) {
                                let _ = session.hold("open in a terminal");
                            }
                            report.in_terminal.push(record.id.clone());
                            continue;
                        }
                    } else if self.restore_one(&record, true).is_ok() {
                        self.update(&record.id, |r| {
                            r.last_known_state = LastState::Live;
                            r.terminal_pane = None;
                            r.push_notice(NoticeKind::TerminalEnded, "The terminal was closed. The chat has the conversation again.", now_ms());
                        });
                        report.restored.push(record.id.clone());
                        continue;
                    }
                    report.failed.push(record.id.clone());
                }
                LastState::Live => match self.restore_one(&record, true) {
                    Ok(()) => report.restored.push(record.id.clone()),
                    Err(_) => report.failed.push(record.id.clone()),
                },
            }
        }
        report
    }

    fn restore_one(&self, record: &ConversationRecord, replay: bool) -> Result<(), ConversationError> {
        let cfg = match self.config_for(&record.agent) {
            Ok(cfg) => cfg,
            Err(e) => {
                let text = match &e {
                    ConversationError::Disabled { note, .. } => format!("Chat for this agent is off: {note}"),
                    _ => "The agent for this conversation is not available any more.".to_string(),
                };
                self.update(&record.id, |r| {
                    r.last_known_state = LastState::Closed;
                    r.terminal_pane = None;
                    r.push_notice(NoticeKind::RestoreFailed, text, now_ms());
                });
                return Err(e);
            }
        };
        self.build_session(&record.id, cfg, Path::new(&record.cwd), record.session_id.clone(), replay);
        let _ = self.0.writers.claim(&record.id, Writer::Chat);
        Ok(())
    }

    pub(super) fn config_for(&self, agent: &str) -> Result<Arc<AgentConfig>, ConversationError> {
        let cfg = self.0.agents.config(agent).ok_or_else(|| ConversationError::NoDefinition(agent.to_string()))?;
        if cfg.disabled {
            return Err(ConversationError::Disabled { agent: cfg.id.clone(), note: cfg.disabled_note.clone() });
        }
        Ok(cfg)
    }

    pub(super) fn build_session(&self, id: &str, cfg: Arc<AgentConfig>, cwd: &Path, resume: Option<String>, replay: bool) -> Arc<AcpSession> {
        let session = AcpSession::new(cfg, self.0.host.clone(), SessionParams { key: id.to_string(), cwd: cwd.to_path_buf(), resume_session_id: resume });
        if replay {
            session.replay_on_next_start();
        }
        self.attach_sink(&session, id);
        self.0.supervisor.insert(session.clone());
        session
    }

    /// Keeps the record true as the session learns its id, loses its history, or starts.
    fn attach_sink(&self, session: &Arc<AcpSession>, id: &str) {
        let weak: Weak<Inner> = Arc::downgrade(&self.0);
        let id = id.to_string();
        session.add_sink(Arc::new(move |event| {
            let Some(inner) = weak.upgrade() else { return };
            Conversations(inner).on_event(&id, event);
        }));
    }

    fn on_event(&self, id: &str, event: &SessionEvent) {
        match event {
            SessionEvent::Snapshot(snapshot) => {
                if let Some(session_id) = &snapshot.session_id {
                    if self.record(id).is_some_and(|r| r.session_id.as_ref() != Some(session_id)) {
                        let session_id = session_id.clone();
                        self.update(id, |r| r.session_id = Some(session_id));
                    }
                }
            }
            SessionEvent::HistoryLost(lost) => {
                let new_id = lost.new_session_id.clone();
                self.update(id, |r| {
                    r.session_id = Some(new_id);
                    r.push_notice(NoticeKind::HistoryLost, "The earlier conversation could not be brought back, so a new one was started. What was said before is not in it.", now_ms());
                });
            }
            _ => {}
        }
    }

    // ---- the record ------------------------------------------------------------------------------------

    pub(super) fn update(&self, id: &str, change: impl FnOnce(&mut ConversationRecord)) {
        {
            let mut records = self.0.records.lock();
            let Some(record) = records.iter_mut().find(|r| r.id == id) else { return };
            change(record);
            record.updated_ms = now_ms();
        }
        self.save();
    }

    fn save(&self) {
        let records = self.0.records.lock().clone();
        if let Err(e) = self.0.store.save(&records) {
            tracing::warn!("acp: could not save the conversation records: {e}");
        }
    }
}

static GLOBAL: std::sync::OnceLock<Conversations> = std::sync::OnceLock::new();

/// The app's conversations: records in the app data folder, the app's supervisor, the real machine, and the
/// registered agents.
pub fn global() -> &'static Conversations {
    GLOBAL.get_or_init(|| {
        Conversations::new(RecordStore::in_app_data(), super::supervisor::global().clone(), Arc::new(super::config::RealHost), Arc::new(RegisteredAgents))
    })
}
