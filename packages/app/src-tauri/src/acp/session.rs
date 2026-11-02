//! `AcpSession`: one conversation with one agent, over one adapter process at a time.
//!
//! Callers (A2 and later) use five things:
//!
//! * [`AcpSession::prompt`] starts a turn (and starts or restores the adapter first if it is not running);
//! * [`AcpSession::cancel`] stops it, on the priority write path;
//! * [`AcpSession::answer_permission`] answers an agent's question with one of the options it offered;
//! * [`AcpSession::snapshot`] / [`AcpSession::wait_for`] read the state (`state.rs`);
//! * [`AcpSession::add_sink`] receives every [`SessionEvent`] in order, pushed.
//!
//! **Permissions are never answered by this module.** A `session/request_permission` is parked in the
//! session (and in the snapshot, so a viewer that connects later sees it) with no timeout, until
//! `answer_permission` picks one of the offered option ids, or `cancel` answers every open question
//! with "cancelled" as the protocol requires. The timers pause while a question is open.
//!
//! Locking: one mutex (`core`) guards the state. Nothing blocks while holding it: requests go out after it
//! is released, and events reach sinks through an ordered outbox drained outside the lock, so a sink may
//! call back into the session.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Condvar, Mutex, RwLock};
use serde_json::{json, Value};

use super::config::{self, AgentConfig, HistoryLostPolicy, HostEnv, ResumePreference};
use super::conn::{cancelled_outcome, selected_outcome, AcpConnection, ConnEvent, ConnHandler, ExitCause, ExitInfo};
use super::error::{AcpError, RPC_AUTH_REQUIRED, RPC_METHOD_NOT_FOUND};
use super::handshake::{self, AgentCaps, SessionInfo};
use super::pin;
use super::state::{
    ExitKind, ExitSummary, HistoryLost, PermissionWait, Phase, SessionState, Snapshot, TurnOutcome, TurnSummary,
    MAX_PERMISSION_WAITS,
};
use super::timers::{self, TurnClock, Verdict};
use super::types::{RequestPermissionRequest, SessionNotification, SessionUpdate, StopReason};

/// How often, and how many times, a cancel is sent again while the turn has not ended.
const CANCEL_REPEAT_EVERY: Duration = Duration::from_millis(250);
const CANCEL_REPEATS: usize = 480;

/// Open questions kept at once; one more is refused ("cancelled") rather than queued without bound.
const MAX_PARKED: usize = MAX_PERMISSION_WAITS * 2;

#[derive(Debug, Clone)]
pub struct SessionParams {
    /// A stable name for this session in the app (a pane id, a run handle).
    pub key: String,
    /// An absolute folder: the agent works there.
    pub cwd: PathBuf,
    /// The agent's own session id to bring back at the first start.
    pub resume_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Selected(String),
    Cancelled,
    /// The adapter went away (or the turn ended) with the question still open.
    AgentGone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartKind {
    New,
    Resumed,
    Loaded,
    /// The agent could not restore the session: a new one was made.
    HistoryLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    AlreadyRunning,
    Started(StartKind),
}

/// What the agent did with text sent into a running turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerOutcome {
    /// The running turn took it.
    Injected,
    /// No turn was running (it ended meanwhile): send it as a normal prompt.
    PromptRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickAction {
    Nothing,
    /// A window ran out: the agent was asked to stop.
    StopAsked,
    /// It did not stop in time: killed.
    Killed,
    /// Idle long enough: the adapter was stopped, the session can be brought back.
    Slept,
    /// The agent has been quiet for the inactivity window: the person is asked (Stop or Keep waiting). Nothing was stopped.
    QuietAsked,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// The snapshot changed (revision moved).
    Snapshot(Box<Snapshot>),
    Started { how: StartKind, caps: Box<AgentCaps> },
    Update { update: Box<SessionUpdate>, replay: bool },
    /// An update this build cannot type (a new kind, or a different shape). Kept raw so nothing is lost.
    UpdateUnparsed { kind: String, raw: Value, replay: bool },
    /// A notification that is not `session/update` (the adapters' own `_session/...` messages).
    Extension { method: String, params: Value },
    PermissionRequested(PermissionWait),
    PermissionResolved { ask_id: u64, how: Resolution },
    TurnStarted { turn: u64 },
    TurnEnded(TurnSummary),
    HistoryLost(HistoryLost),
    Exited(ExitSummary),
    Slept,
}

pub type Sink = Arc<dyn Fn(&SessionEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub foreign_updates: u64,
    pub stale_events: u64,
    pub unparsed_updates: u64,
    pub unsupported_requests: u64,
    pub refused_questions: u64,
    pub starts: u64,
}

struct Turn {
    id: u64,
    clock: TurnClock,
}

struct Parked {
    rpc_id: Value,
    options: Vec<String>,
}

struct Core {
    state: SessionState,
    conn: Option<AcpConnection>,
    caps: Option<AgentCaps>,
    starting: bool,
    closed: bool,
    /// `session/load` is replaying history: updates are not live work.
    loading: bool,
    /// The next start restores with `session/load` (replays the history) before `session/resume`.
    replay_next: bool,
    /// Why this conversation may not start an adapter (another writer has it). Set by `hold`.
    held: Option<String>,
    parked: BTreeMap<u64, Parked>,
    turn: Option<Turn>,
    desired_config: BTreeMap<String, Value>,
    idle_since: Option<Instant>,
    stats: SessionStats,
}

impl Core {
    fn is_running(&self) -> bool {
        self.conn.as_ref().is_some_and(AcpConnection::is_alive)
            && !matches!(self.state.phase(), Phase::Dormant | Phase::Exited | Phase::Starting | Phase::Resuming)
    }
}

#[derive(Default)]
struct Outbox {
    queue: VecDeque<SessionEvent>,
    draining: bool,
}

struct StartError {
    error: AcpError,
    /// The error text plus the adapter's last stderr lines: what the retry rules look at.
    hint: String,
}

pub struct AcpSession {
    me: Weak<AcpSession>,
    cfg: Arc<AgentConfig>,
    host: Arc<dyn HostEnv>,
    key: String,
    cwd: PathBuf,
    core: Mutex<Core>,
    changed: Condvar,
    outbox: Mutex<Outbox>,
    sinks: RwLock<Vec<Sink>>,
}

struct Handler {
    session: Weak<AcpSession>,
    epoch: u64,
}

impl ConnHandler for Handler {
    fn on_event(&self, event: ConnEvent) {
        if let Some(session) = self.session.upgrade() {
            session.on_conn_event(self.epoch, event);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl AcpSession {
    pub fn new(cfg: Arc<AgentConfig>, host: Arc<dyn HostEnv>, params: SessionParams) -> Arc<AcpSession> {
        let mut state = SessionState::new();
        state.set_cancel_reports_end_turn(cfg.quirks.cancel_reports_end_turn);
        if let Some(id) = &params.resume_session_id {
            state.adopt_session_id(id);
            state.commit();
        }
        Arc::new_cyclic(|me| AcpSession {
            me: me.clone(),
            cfg,
            host,
            key: params.key,
            cwd: params.cwd,
            core: Mutex::new(Core {
                state,
                conn: None,
                caps: None,
                starting: false,
                closed: false,
                loading: false,
                replay_next: false,
                held: None,
                parked: BTreeMap::new(),
                turn: None,
                desired_config: BTreeMap::new(),
                idle_since: None,
                stats: SessionStats::default(),
            }),
            changed: Condvar::new(),
            outbox: Mutex::new(Outbox::default()),
            sinks: RwLock::new(Vec::new()),
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn agent_id(&self) -> &str {
        &self.cfg.id
    }

    pub fn config(&self) -> &AgentConfig {
        &self.cfg
    }

    // ---- reading ---------------------------------------------------------------------------------------

    pub fn snapshot(&self) -> Snapshot {
        self.core.lock().state.snapshot().clone()
    }

    pub fn caps(&self) -> Option<AgentCaps> {
        self.core.lock().caps.clone()
    }

    pub fn stats(&self) -> SessionStats {
        self.core.lock().stats
    }

    /// The live adapter connection, if any (diagnostics and tests).
    pub fn connection(&self) -> Option<AcpConnection> {
        self.core.lock().conn.clone()
    }

    /// True while an adapter process is up.
    pub fn is_live(&self) -> bool {
        self.core.lock().conn.as_ref().is_some_and(AcpConnection::is_alive)
    }

    pub fn idle_since(&self) -> Option<Instant> {
        self.core.lock().idle_since
    }

    /// Open permission questions, for a viewer that connects late.
    pub fn pending_permissions(&self) -> Vec<PermissionWait> {
        self.core.lock().state.snapshot().permission_waits.clone()
    }

    /// Blocks until `predicate` holds for the snapshot, or `timeout`. Returns the snapshot that satisfied it.
    pub fn wait_for(&self, predicate: impl Fn(&Snapshot) -> bool, timeout: Duration) -> Option<Snapshot> {
        let deadline = Instant::now() + timeout;
        let mut core = self.core.lock();
        loop {
            let snap = core.state.snapshot();
            if predicate(snap) {
                return Some(snap.clone());
            }
            if self.changed.wait_until(&mut core, deadline).timed_out() {
                let snap = core.state.snapshot();
                return predicate(snap).then(|| snap.clone());
            }
        }
    }

    /// Receives every event from now on, in order, on whichever thread caused it.
    pub fn add_sink(&self, sink: Sink) {
        self.sinks.write().push(sink);
    }

    // ---- the one place state changes -------------------------------------------------------------------

    fn with_core<R>(&self, f: impl FnOnce(&mut Core, &mut Vec<SessionEvent>) -> R) -> R {
        let mut core = self.core.lock();
        let mut events = Vec::new();
        let result = f(&mut core, &mut events);
        if core.state.commit() {
            events.push(SessionEvent::Snapshot(Box::new(core.state.snapshot().clone())));
        }
        if !events.is_empty() {
            self.outbox.lock().queue.extend(events);
        }
        drop(core);
        self.changed.notify_all();
        self.drain();
        result
    }

    /// Delivers queued events to the sinks, in order, by one thread at a time.
    fn drain(&self) {
        let mut mine = false;
        loop {
            let batch: Vec<SessionEvent> = {
                let mut outbox = self.outbox.lock();
                if !mine {
                    if outbox.draining {
                        return;
                    }
                    outbox.draining = true;
                    mine = true;
                }
                if outbox.queue.is_empty() {
                    outbox.draining = false;
                    return;
                }
                outbox.queue.drain(..).collect()
            };
            let sinks: Vec<Sink> = self.sinks.read().clone();
            for event in &batch {
                for sink in &sinks {
                    let sink = sink.clone();
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event))).is_err() {
                        tracing::error!(session = %self.key, "acp: an event sink panicked");
                    }
                }
            }
        }
    }

    // ---- starting --------------------------------------------------------------------------------------

    /// Makes sure an adapter is running with the session open: starts it, or brings the session back.
    pub fn ensure_running(&self) -> Result<Readiness, AcpError> {
        {
            let mut core = self.core.lock();
            loop {
                if core.closed {
                    return Err(AcpError::SessionClosed);
                }
                if let Some(why) = &core.held {
                    return Err(AcpError::Held(why.clone()));
                }
                if core.is_running() {
                    return Ok(Readiness::AlreadyRunning);
                }
                if core.starting {
                    self.changed.wait(&mut core);
                    continue;
                }
                core.starting = true;
                break;
            }
        }
        let result = self.start_with_retries();
        self.core.lock().starting = false;
        self.changed.notify_all();
        result.map(Readiness::Started)
    }

    fn start_with_retries(&self) -> Result<StartKind, AcpError> {
        pin::enforce(&self.cfg, &*self.host)?;
        let mut attempt = 0usize;
        loop {
            match self.start_once() {
                Ok(kind) => return Ok(kind),
                Err(failure) => match self.retry_delay(&failure.hint, attempt) {
                    Some(delay) => {
                        attempt += 1;
                        tracing::info!(session = %self.key, attempt, "acp: start failed, trying again in {:?}", delay);
                        thread::sleep(delay);
                    }
                    None => return Err(failure.error.clone()),
                },
            }
        }
    }

    fn retry_delay(&self, hint: &str, attempt: usize) -> Option<Duration> {
        let retry = self.cfg.start_retry.as_ref()?;
        let hint = hint.to_lowercase();
        if !retry.patterns.iter().any(|p| hint.contains(&p.to_lowercase())) {
            return None;
        }
        retry.delays_ms.get(attempt).map(|ms| Duration::from_millis(*ms))
    }

    fn start_once(&self) -> Result<StartKind, Box<StartError>> {
        let (epoch, resume_id) = self.with_core(|core, _| {
            let resume_id = core.state.session_id().map(str::to_string);
            core.state.begin_epoch(resume_id.is_some());
            core.conn = None;
            core.caps = None;
            core.loading = false;
            core.stats.starts += 1;
            (core.state.epoch(), resume_id)
        });
        match self.connect_and_open(epoch, resume_id.as_deref()) {
            Ok(kind) => Ok(kind),
            Err(error) => {
                let (hint, summary_tail) = self.with_core(|core, _| {
                    // The adapter may already be gone (its exit was noticed first): its last words are then
                    // in the exit summary, otherwise still in the connection's ring.
                    let tail = match core.conn.as_ref() {
                        Some(conn) => conn.stderr_tail(20),
                        None => core.state.snapshot().exit.as_ref().map(|e| e.stderr_tail.clone()).unwrap_or_default(),
                    };
                    (format!("{error} {}", tail.join(" ")), tail)
                });
                self.fail_start(epoch, &error, summary_tail);
                Err(Box::new(StartError { error, hint }))
            }
        }
    }

    fn fail_start(&self, epoch: u64, error: &AcpError, stderr_tail: Vec<String>) {
        let conn = self.with_core(|core, ev| {
            if core.state.epoch() != epoch {
                return None;
            }
            let summary = ExitSummary { kind: ExitKind::StartFailed, code: None, stderr_tail, reason: Some(error.to_string()) };
            core.state.on_exit(summary.clone());
            ev.push(SessionEvent::Exited(summary));
            core.conn.take()
        });
        if let Some(conn) = conn {
            conn.kill();
        }
    }

    fn connect_and_open(&self, epoch: u64, resume_id: Option<&str>) -> Result<StartKind, AcpError> {
        let cfg = &self.cfg;
        let spec = config::resolve_spawn(cfg, &*self.host, &self.cwd)?;
        let handler = Arc::new(Handler { session: self.me.clone(), epoch });
        let conn = AcpConnection::spawn(spec, cfg.timeouts.conn_limits(), handler)?;
        self.with_core(|core, _| core.conn = Some(conn.clone()));

        let init = conn.request("initialize", handshake::initialize_params("puppet-master", env!("CARGO_PKG_VERSION")), cfg.timeouts.request())?;
        let caps = handshake::parse_initialize(&init)?;
        self.with_core(|core, _| core.caps = Some(caps.clone()));

        let (session_id, kind, info) = self.open_session(&conn, &caps, resume_id)?;

        let reapply = self.with_core(|core, ev| {
            if core.state.epoch() != epoch {
                return None;
            }
            core.state.on_ready(&session_id, &info);
            core.idle_since = Some(Instant::now());
            ev.push(SessionEvent::Started { how: kind, caps: Box::new(caps.clone()) });
            Some(core.desired_config.clone())
        });
        if let Some(desired) = reapply {
            self.reapply_config(&conn, &session_id, &desired);
        }
        Ok(kind)
    }

    fn session_params(&self, mut base: Value) -> Value {
        if let (Some(meta), Some(map)) = (config::session_meta(&self.cfg), base.as_object_mut()) {
            map.insert("_meta".into(), meta);
        }
        base
    }

    fn open_session(&self, conn: &AcpConnection, caps: &AgentCaps, resume_id: Option<&str>) -> Result<(String, StartKind, SessionInfo), AcpError> {
        let cwd = self.cwd.to_string_lossy().into_owned();
        let replay = self.with_core(|core, _| std::mem::take(&mut core.replay_next));
        let new_params = self.session_params(json!({ "cwd": cwd, "mcpServers": self.cfg.mcp_servers }));
        let Some(old_id) = resume_id else {
            let (id, info) = self.session_new(conn, caps, new_params)?;
            return Ok((id, StartKind::New, info));
        };

        let mut reason = "the agent cannot restore a session".to_string();
        let preference = if replay { ResumePreference::PreferLoad } else { self.cfg.resume };
        for method in restore_order(preference, caps) {
            let params = self.session_params(json!({ "sessionId": old_id, "cwd": cwd, "mcpServers": self.cfg.mcp_servers }));
            match self.restore(conn, method, params) {
                Ok(info) => return Ok((old_id.to_string(), if method == Restore::Resume { StartKind::Resumed } else { StartKind::Loaded }, info)),
                // Slow, dead or not yet started: the id is still good, try again later.
                Err(e) if e.is_transient() => return Err(e),
                Err(e) if is_auth_error(&e) => return Err(auth_required(caps, e)),
                Err(e) if !self.looks_like_unknown_session(&e) => return Err(e),
                Err(e) => reason = e.to_string(),
            }
        }
        // Nothing could bring the conversation back: start a fresh one and say so.
        let (new_id, info) = self.session_new(conn, caps, new_params)?;
        let lost = HistoryLost { old_session_id: old_id.to_string(), new_session_id: new_id.clone(), reason };
        self.with_core(|core, ev| {
            core.state.on_history_lost(lost.clone());
            ev.push(SessionEvent::HistoryLost(lost));
        });
        Ok((new_id, StartKind::HistoryLost, info))
    }

    fn looks_like_unknown_session(&self, error: &AcpError) -> bool {
        let patterns = &self.cfg.unknown_session_patterns;
        if patterns.is_empty() {
            return true;
        }
        let text = error.to_string().to_lowercase();
        patterns.iter().any(|p| text.contains(&p.to_lowercase()))
    }

    fn session_new(&self, conn: &AcpConnection, caps: &AgentCaps, params: Value) -> Result<(String, SessionInfo), AcpError> {
        let result = self
            .call_in_order(conn, "session/new", params, self.cfg.timeouts.new_session(), |session, outcome| {
                // In order with the updates that follow: from here on updates have a session to belong to.
                if let Ok(value) = outcome {
                    if let Some(id) = handshake::parse_session_info(value).session_id {
                        session.with_core(|core, _| core.state.adopt_session_id(&id));
                    }
                }
            })
            .map_err(|e| if is_auth_error(&e) { auth_required(caps, e) } else { e })?;
        let info = handshake::parse_session_info(&result);
        let id = info.session_id.clone().ok_or_else(|| AcpError::Protocol("session/new did not return a session id".into()))?;
        Ok((id, info))
    }

    fn restore(&self, conn: &AcpConnection, method: Restore, params: Value) -> Result<SessionInfo, AcpError> {
        let (name, timeout) = match method {
            Restore::Resume => ("session/resume", self.cfg.timeouts.request()),
            Restore::Load => ("session/load", self.cfg.timeouts.load()),
        };
        if method == Restore::Load {
            self.with_core(|core, _| core.loading = true);
        }
        let result = self.call_in_order(conn, name, params, timeout, move |session, _| {
            if method == Restore::Load {
                session.with_core(|core, _| core.loading = false);
            }
        });
        if method == Restore::Load && result.is_err() {
            self.with_core(|core, _| core.loading = false);
        }
        result.map(|value| handshake::parse_session_info(&value))
    }

    /// A request whose answer is applied on the dispatcher thread, in order with the updates around it,
    /// and then handed to this thread.
    fn call_in_order(
        &self,
        conn: &AcpConnection,
        method: &str,
        params: Value,
        timeout: Duration,
        apply: impl FnOnce(&AcpSession, &Result<Value, AcpError>) + Send + 'static,
    ) -> Result<Value, AcpError> {
        let (tx, rx) = mpsc::sync_channel(1);
        let weak = self.me.clone();
        conn.request_with(
            method,
            params,
            Box::new(move |outcome| {
                if let Some(session) = weak.upgrade() {
                    apply(&session, &outcome);
                }
                let _ = tx.send(outcome);
            }),
        )?;
        match rx.recv_timeout(timeout) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AcpError::Timeout { method: method.to_string(), secs: timeout.as_secs() }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AcpError::Closed),
        }
    }

    /// After a restart the agent forgets what the person picked (model, mode): put it back.
    fn reapply_config(&self, conn: &AcpConnection, session_id: &str, desired: &BTreeMap<String, Value>) {
        for (id, value) in desired {
            let current = self.with_core(|core, _| {
                core.state.config_options().iter().find(|o| o.get("id").and_then(Value::as_str) == Some(id)).and_then(|o| o.get("currentValue").cloned())
            });
            if current.as_ref() == Some(value) {
                continue;
            }
            let params = json!({ "sessionId": session_id, "configId": id, "value": value });
            match conn.request("session/set_config_option", params, self.cfg.timeouts.request()) {
                Ok(answer) => {
                    let options = handshake::parse_session_info(&answer).config_options;
                    if !options.is_empty() {
                        self.with_core(|core, _| core.state.set_config_options(options));
                    }
                }
                Err(e) => tracing::warn!(session = %self.key, config = %id, error = %e, "acp: could not restore a setting after restart"),
            }
        }
    }

    // ---- turns -----------------------------------------------------------------------------------------

    /// Starts a turn. The turn's events arrive through the sinks; `TurnEnded` closes it.
    pub fn prompt(&self, content: Vec<Value>) -> Result<u64, AcpError> {
        if content.is_empty() {
            return Err(AcpError::Unsupported("a prompt needs at least one content block".into()));
        }
        {
            let core = self.core.lock();
            if core.closed {
                return Err(AcpError::SessionClosed);
            }
            if core.turn.is_some() {
                return Err(AcpError::Busy);
            }
        }
        let readiness = self.ensure_running()?;
        if readiness == Readiness::Started(StartKind::HistoryLost) && self.cfg.history_lost == HistoryLostPolicy::Refuse {
            return Err(AcpError::HistoryLost);
        }
        let (conn, params, turn_id, epoch) = self.with_core(|core, ev| {
            if core.turn.is_some() {
                return Err(AcpError::Busy);
            }
            let conn = core.conn.clone().filter(AcpConnection::is_alive).ok_or(AcpError::Closed)?;
            let session_id = core.state.session_id().map(str::to_string).ok_or_else(|| AcpError::Protocol("no session id".into()))?;
            let turn_id = core.state.on_prompt_sent();
            core.turn = Some(Turn { id: turn_id, clock: TurnClock::new(Instant::now()) });
            core.idle_since = None;
            ev.push(SessionEvent::TurnStarted { turn: turn_id });
            Ok((conn, json!({ "sessionId": session_id, "prompt": content }), turn_id, core.state.epoch()))
        })?;
        let weak = self.me.clone();
        let sent = conn.request_with(
            "session/prompt",
            params,
            Box::new(move |outcome| {
                if let Some(session) = weak.upgrade() {
                    session.on_prompt_done(epoch, turn_id, outcome);
                }
            }),
        );
        if let Err(error) = sent {
            self.on_prompt_done(epoch, turn_id, Err(error.clone()));
            return Err(error);
        }
        Ok(turn_id)
    }

    pub fn prompt_text(&self, text: &str) -> Result<u64, AcpError> {
        self.prompt(vec![json!({ "type": "text", "text": text })])
    }

    fn on_prompt_done(&self, epoch: u64, turn_id: u64, outcome: Result<Value, AcpError>) {
        self.with_core(|core, ev| {
            if core.state.epoch() != epoch || core.turn.as_ref().map(|t| t.id) != Some(turn_id) {
                core.stats.stale_events += 1;
                return;
            }
            core.turn = None;
            let outcome = match outcome {
                Ok(value) => match value.get("stopReason").and_then(Value::as_str) {
                    Some(word) => match StopReason::parse(word) {
                        Some(reason) => TurnOutcome::Stop(reason),
                        None => TurnOutcome::Failed { message: format!("the agent ended the turn with a stop reason this app does not know: {word}"), code: None },
                    },
                    None => TurnOutcome::Failed { message: "the agent ended the turn without a stop reason".into(), code: None },
                },
                Err(AcpError::Rpc { code, message, .. }) => TurnOutcome::Failed { message, code: Some(code) },
                Err(AcpError::Closed) => TurnOutcome::Failed { message: "the adapter exited before the turn finished".into(), code: None },
                Err(other) => TurnOutcome::Failed { message: other.to_string(), code: None },
            };
            let summary = core.state.on_turn_end(outcome);
            // Questions still open belong to a turn that is over: nobody is waiting for the answer.
            let leftover: Vec<u64> = core.parked.keys().copied().collect();
            core.parked.clear();
            for ask_id in leftover {
                core.state.on_permission_resolved(ask_id);
                ev.push(SessionEvent::PermissionResolved { ask_id, how: Resolution::AgentGone });
            }
            core.idle_since = Some(Instant::now());
            ev.push(SessionEvent::TurnEnded(summary));
        });
    }

    /// Asks the agent to stop the turn. Goes out on the priority path, and answers every open permission
    /// question with "cancelled". Returns false when no turn is running.
    pub fn cancel(&self) -> Result<bool, AcpError> {
        let plan = self.with_core(|core, ev| {
            core.turn.as_ref()?;
            let conn = core.conn.clone()?;
            let session_id = core.state.session_id()?.to_string();
            let now = Instant::now();
            if let Some(turn) = core.turn.as_mut() {
                turn.clock.stop_asked_at.get_or_insert(now);
                turn.clock.paused = false;
            }
            core.state.on_cancel_requested();
            let open: Vec<(u64, Value)> = core.parked.iter().map(|(id, p)| (*id, p.rpc_id.clone())).collect();
            core.parked.clear();
            for (ask_id, _) in &open {
                core.state.on_permission_resolved(*ask_id);
                ev.push(SessionEvent::PermissionResolved { ask_id: *ask_id, how: Resolution::Cancelled });
            }
            Some((conn, session_id, open, core.turn.as_ref().map(|t| t.id)?, core.state.epoch()))
        });
        let Some((conn, session_id, open, turn_id, epoch)) = plan else { return Ok(false) };
        conn.notify_priority("session/cancel", json!({ "sessionId": session_id }))?;
        for (_, rpc_id) in open {
            conn.respond(&rpc_id, cancelled_outcome())?;
        }
        self.repeat_cancel(conn, session_id, turn_id, epoch);
        Ok(true)
    }

    /// A cancel is a notification: nothing says it arrived or that the agent had the turn registered yet.
    /// The first one can overtake its own prompt (it travels on the priority lane) or reach the adapter
    /// a moment before the adapter has set the turn up, and is then lost. So it is sent again every
    /// quarter of a second until the turn ends; a duplicate is harmless. The turn id is checked under the
    /// session lock together with the write to the queue, so a repeat can never land in the next turn.
    fn repeat_cancel(&self, conn: AcpConnection, session_id: String, turn_id: u64, epoch: u64) {
        let weak = self.me.clone();
        let spawned = thread::Builder::new().name("acp-cancel".into()).spawn(move || {
            for _ in 0..CANCEL_REPEATS {
                thread::sleep(CANCEL_REPEAT_EVERY);
                let Some(session) = weak.upgrade() else { return };
                let core = session.core.lock();
                let still = core.state.epoch() == epoch && core.turn.as_ref().map(|t| t.id) == Some(turn_id);
                if !still || !conn.is_alive() {
                    return;
                }
                if conn.notify_priority("session/cancel", json!({ "sessionId": session_id })).is_err() {
                    return;
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(session = %self.key, error = %e, "acp: could not start the cancel repeater");
        }
    }

    /// "Keep waiting": the person looked at the quiet agent and chose to wait. The quiet window starts again.
    /// False when no turn is running.
    pub fn keep_waiting(&self) -> bool {
        self.with_core(|core, _| {
            let Some(turn) = core.turn.as_mut() else { return false };
            turn.clock.keep_waiting(Instant::now());
            core.state.set_quiet(None);
            true
        })
    }

    /// Answers an open question with one of the options the agent offered.
    pub fn answer_permission(&self, ask_id: u64, option_id: &str) -> Result<(), AcpError> {
        let (conn, rpc_id) = self.with_core(|core, ev| {
            let parked = core.parked.get(&ask_id).ok_or(AcpError::NoSuchAsk)?;
            if !parked.options.iter().any(|o| o == option_id) {
                return Err(AcpError::BadOption(option_id.to_string()));
            }
            let conn = core.conn.clone().filter(AcpConnection::is_alive).ok_or(AcpError::Closed)?;
            let parked = core.parked.remove(&ask_id).ok_or(AcpError::NoSuchAsk)?;
            core.state.on_permission_resolved(ask_id);
            if core.parked.is_empty() {
                if let Some(turn) = core.turn.as_mut() {
                    turn.clock.resume(Instant::now());
                }
            }
            ev.push(SessionEvent::PermissionResolved { ask_id, how: Resolution::Selected(option_id.to_string()) });
            Ok((conn, parked.rpc_id))
        })?;
        conn.respond(&rpc_id, selected_outcome(option_id))
    }

    /// Sends text into the turn that runs, when the agent takes that (`AgentCaps::steering`, the adapters' own
    /// `_session/steering`). The answer tells whether the turn took it; with no turn running the answer is
    /// `PromptRequired` and nothing was sent. An agent that does not take it gets `Unsupported`: the caller
    /// queues the text instead.
    pub fn steer(&self, content: Vec<Value>) -> Result<SteerOutcome, AcpError> {
        if content.is_empty() {
            return Err(AcpError::Unsupported("a message needs at least one content block".into()));
        }
        let planned = self.with_core(|core, _| {
            if core.closed {
                return Err(AcpError::SessionClosed);
            }
            if core.turn.is_none() {
                return Ok(None);
            }
            if !core.caps.as_ref().is_some_and(|caps| caps.steering) {
                return Err(AcpError::Unsupported("the agent does not take text while it works".into()));
            }
            let conn = core.conn.clone().filter(AcpConnection::is_alive).ok_or(AcpError::Closed)?;
            let session_id = core.state.session_id().ok_or_else(|| AcpError::Protocol("no session id".into()))?.to_string();
            Ok(Some((conn, session_id)))
        })?;
        let Some((conn, session_id)) = planned else { return Ok(SteerOutcome::PromptRequired) };
        let answer = conn.request(
            "_session/steering",
            json!({ "sessionId": session_id, "prompt": content, "_meta": { "steering": { "idleBehavior": "promptRequired" } } }),
            Duration::from_secs(10),
        )?;
        match answer.get("outcome").and_then(Value::as_str) {
            Some("injected") => Ok(SteerOutcome::Injected),
            Some("promptRequired") => Ok(SteerOutcome::PromptRequired),
            other => Err(AcpError::Protocol(format!("unexpected steering answer: {other:?}"))),
        }
    }

    /// The next start brings the conversation back with `session/load`, which replays it (including turns
    /// somebody else added to the agent's own session file meanwhile), instead of the cheaper
    /// `session/resume`. Used after a hand-over and after an app restart, when the viewer has no history.
    pub fn replay_on_next_start(&self) {
        self.with_core(|core, _| core.replay_next = true);
    }

    /// The person has seen the "history lost" notice.
    pub fn acknowledge_history_lost(&self) {
        self.with_core(|core, _| core.state.clear_history_lost());
    }

    // ---- settings --------------------------------------------------------------------------------------

    /// Changes a setting the agent exposes (model, mode, effort) with `session/set_config_option`. Never
    /// typed into the chat as a command. Refused while a turn runs.
    pub fn set_config_option(&self, config_id: &str, value: Value) -> Result<(), AcpError> {
        self.ensure_running()?;
        let (conn, session_id) = self.with_core(|core, _| {
            if core.turn.is_some() {
                return Err(AcpError::Busy);
            }
            let conn = core.conn.clone().filter(AcpConnection::is_alive).ok_or(AcpError::Closed)?;
            let session_id = core.state.session_id().ok_or_else(|| AcpError::Protocol("no session id".into()))?.to_string();
            Ok((conn, session_id))
        })?;
        let answer = conn.request("session/set_config_option", json!({ "sessionId": session_id, "configId": config_id, "value": value }), self.cfg.timeouts.request())?;
        let options = handshake::parse_session_info(&answer).config_options;
        self.with_core(|core, _| {
            core.desired_config.insert(config_id.to_string(), value);
            if !options.is_empty() {
                core.state.set_config_options(options);
            }
        });
        Ok(())
    }

    // ---- stopping --------------------------------------------------------------------------------------

    /// Stops the adapter to free its memory. The conversation stays and comes back on the next prompt.
    /// Refused while a turn runs or a question is open.
    pub fn sleep(&self) -> Result<bool, AcpError> {
        let conn = self.with_core(|core, ev| {
            if core.turn.is_some() || !core.parked.is_empty() || core.starting {
                return Err(AcpError::Busy);
            }
            let Some(conn) = core.conn.take() else { return Ok(None) };
            core.state.on_dormant();
            core.idle_since = None;
            ev.push(SessionEvent::Slept);
            Ok(Some(conn))
        })?;
        match conn {
            Some(conn) => {
                conn.kill();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Stops the adapter (like [`AcpSession::sleep`]) and refuses to start one again until
    /// [`AcpSession::release_hold`]: another writer (the terminal) has the conversation. Refused with
    /// `Busy` while a turn runs, a question is open, or a start is under way. Returns once the adapter's
    /// processes are gone (a short wait), so the next writer does not meet its file locks.
    pub fn hold(&self, why: &str) -> Result<(), AcpError> {
        let conn = self.with_core(|core, ev| {
            if core.closed {
                return Err(AcpError::SessionClosed);
            }
            if core.turn.is_some() || !core.parked.is_empty() || core.starting {
                return Err(AcpError::Busy);
            }
            core.held = Some(why.to_string());
            let Some(conn) = core.conn.take() else { return Ok(None) };
            core.state.on_dormant();
            core.idle_since = None;
            ev.push(SessionEvent::Slept);
            Ok(Some(conn))
        })?;
        if let Some(conn) = conn {
            let pid = conn.pid();
            conn.kill();
            let deadline = Instant::now() + Duration::from_secs(3);
            while pid != 0 && crate::proc_util::pid_alive(pid) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(25));
            }
        }
        Ok(())
    }

    /// Lets the adapter start again (the other writer is gone).
    pub fn release_hold(&self) {
        self.with_core(|core, _| core.held = None);
    }

    /// Why the conversation may not start an adapter, if it may not.
    pub fn held_by(&self) -> Option<String> {
        self.core.lock().held.clone()
    }

    /// Ends the session for good: the adapter and everything under it are killed.
    pub fn close(&self) {
        let conn = self.with_core(|core, ev| {
            core.closed = true;
            if let Some(turn) = core.turn.take() {
                let _ = turn;
                let summary = core.state.on_turn_end(TurnOutcome::Failed { message: "the session was closed".into(), code: None });
                ev.push(SessionEvent::TurnEnded(summary));
            }
            for ask_id in std::mem::take(&mut core.parked).into_keys() {
                core.state.on_permission_resolved(ask_id);
                ev.push(SessionEvent::PermissionResolved { ask_id, how: Resolution::AgentGone });
            }
            let summary = ExitSummary { kind: ExitKind::Closed, code: None, stderr_tail: Vec::new(), reason: None };
            core.state.on_exit(summary.clone());
            ev.push(SessionEvent::Exited(summary));
            core.conn.take()
        });
        if let Some(conn) = conn {
            conn.kill();
        }
    }

    // ---- supervision -----------------------------------------------------------------------------------

    /// Looks at the clocks. Called by the supervisor about every few seconds; `now` is a parameter so a
    /// test can move time without waiting.
    pub fn tick(&self, now: Instant) -> TickAction {
        enum Plan {
            Nothing,
            AskStop(AcpConnection, String, u64, u64),
            QuietAsked,
            Kill(AcpConnection),
            Sleep,
        }
        let plan = self.with_core(|core, ev| {
            if core.closed || core.starting {
                return Plan::Nothing;
            }
            let timeouts = &self.cfg.timeouts;
            if let Some(turn) = core.turn.as_mut() {
                turn.clock.paused = !core.parked.is_empty();
                if turn.clock.paused {
                    turn.clock.quiet_asked = false;
                }
                if !turn.clock.quiet_asked {
                    // Something was heard (or a question opened) since the person was asked: the question is over.
                    core.state.set_quiet(None);
                }
                let Some(conn) = core.conn.clone().filter(AcpConnection::is_alive) else { return Plan::Nothing };
                return match timers::verdict(&turn.clock, timeouts, now) {
                    Verdict::Healthy | Verdict::Paused | Verdict::Stopping => Plan::Nothing,
                    Verdict::Quiet => {
                        // Silence is the person's call: Stop, or Keep waiting. The progress window below still ends a
                        // turn that never does real work.
                        if turn.clock.quiet_asked {
                            return Plan::Nothing;
                        }
                        turn.clock.quiet_asked = true;
                        core.state.set_quiet(Some(timeouts.inactivity_secs));
                        Plan::QuietAsked
                    }
                    Verdict::Stalled => {
                        turn.clock.stop_asked_at = Some(now);
                        let Some(session_id) = core.state.session_id().map(str::to_string) else { return Plan::Nothing };
                        core.state.on_cancel_requested();
                        Plan::AskStop(conn, session_id, turn.id, core.state.epoch())
                    }
                    Verdict::GraceOver => {
                        core.turn = None;
                        let summary = core.state.on_turn_end(TurnOutcome::Hung);
                        ev.push(SessionEvent::TurnEnded(summary));
                        for ask_id in std::mem::take(&mut core.parked).into_keys() {
                            core.state.on_permission_resolved(ask_id);
                            ev.push(SessionEvent::PermissionResolved { ask_id, how: Resolution::AgentGone });
                        }
                        let exit = ExitSummary {
                            kind: ExitKind::Hung,
                            code: None,
                            stderr_tail: conn.stderr_tail(20),
                            reason: Some("the agent did not stop when asked".into()),
                        };
                        core.state.on_exit(exit.clone());
                        ev.push(SessionEvent::Exited(exit));
                        core.conn = None;
                        Plan::Kill(conn)
                    }
                };
            }
            let idle = core.state.phase() == Phase::Idle && core.parked.is_empty();
            match (idle, core.idle_since) {
                (true, Some(since)) if core.conn.is_some() && timers::idle_expired(since, timeouts, now) => Plan::Sleep,
                _ => Plan::Nothing,
            }
        });
        match plan {
            Plan::Nothing => TickAction::Nothing,
            Plan::QuietAsked => TickAction::QuietAsked,
            Plan::AskStop(conn, session_id, turn_id, epoch) => {
                let _ = conn.notify_priority("session/cancel", json!({ "sessionId": session_id }));
                self.repeat_cancel(conn, session_id, turn_id, epoch);
                TickAction::StopAsked
            }
            Plan::Kill(conn) => {
                conn.kill();
                TickAction::Killed
            }
            Plan::Sleep => match self.sleep() {
                Ok(true) => TickAction::Slept,
                _ => TickAction::Nothing,
            },
        }
    }

    // ---- what the adapter says ---------------------------------------------------------------------------

    fn on_conn_event(&self, epoch: u64, event: ConnEvent) {
        match event {
            ConnEvent::Notification { method, params } if method == "session/update" => self.on_session_update(epoch, params),
            ConnEvent::Notification { method, params } => {
                self.with_core(|core, ev| {
                    if core.state.epoch() != epoch {
                        core.stats.stale_events += 1;
                        return;
                    }
                    if let Some(turn) = core.turn.as_mut() {
                        turn.clock.frame(Instant::now(), false);
                    }
                    ev.push(SessionEvent::Extension { method, params });
                });
            }
            ConnEvent::AgentRequest { id, method, params } if method == "session/request_permission" => self.on_permission_request(epoch, id, params),
            ConnEvent::AgentRequest { id, method, .. } => self.reject_request(epoch, &id, &method),
            ConnEvent::Exited(info) => self.on_exited(epoch, info),
        }
    }

    fn on_session_update(&self, epoch: u64, params: Value) {
        let parsed = serde_json::from_value::<SessionNotification>(params.clone());
        self.with_core(|core, ev| {
            if core.state.epoch() != epoch {
                core.stats.stale_events += 1;
                return;
            }
            let now = Instant::now();
            let replay = core.loading;
            match parsed {
                Ok(note) => {
                    // One session per adapter process. Until `session/new` has answered there is no id to compare.
                    if core.state.session_id().is_some_and(|mine| mine != note.session_id) {
                        core.stats.foreign_updates += 1;
                        return;
                    }
                    let effect = core.state.on_update(&note.update, replay);
                    if !replay {
                        if let Some(turn) = core.turn.as_mut() {
                            turn.clock.frame(now, effect.progress);
                        }
                    }
                    ev.push(SessionEvent::Update { update: Box::new(note.update), replay });
                }
                Err(_) => {
                    core.stats.unparsed_updates += 1;
                    let kind = params.pointer("/update/sessionUpdate").and_then(Value::as_str).unwrap_or("?").to_string();
                    if !replay {
                        if let Some(turn) = core.turn.as_mut() {
                            // A kind we do not know counts as work: better to wait than to stop a turn over it.
                            turn.clock.frame(now, true);
                        }
                    }
                    ev.push(SessionEvent::UpdateUnparsed { kind, raw: params, replay });
                }
            }
        });
    }

    fn on_permission_request(&self, epoch: u64, rpc_id: Value, params: Value) {
        enum Decision {
            Parked,
            Refuse(Option<AcpConnection>),
            Ignore,
        }
        let parsed = serde_json::from_value::<RequestPermissionRequest>(params);
        let decision = self.with_core(|core, ev| {
            if core.state.epoch() != epoch {
                core.stats.stale_events += 1;
                return Decision::Ignore;
            }
            let conn = core.conn.clone();
            let Ok(request) = parsed else {
                core.stats.refused_questions += 1;
                return Decision::Refuse(conn);
            };
            let foreign = core.state.session_id().is_some_and(|mine| mine != request.session_id);
            if foreign || core.parked.len() >= MAX_PARKED {
                core.stats.refused_questions += 1;
                return Decision::Refuse(conn);
            }
            let ask_id = core.state.on_permission_request(&request, now_ms());
            core.parked.insert(ask_id, Parked { rpc_id: rpc_id.clone(), options: request.options.iter().map(|o| o.option_id.clone()).collect() });
            if let Some(turn) = core.turn.as_mut() {
                turn.clock.paused = true;
            }
            if let Some(wait) = core.state.pending_wait(ask_id) {
                ev.push(SessionEvent::PermissionRequested(wait));
            }
            Decision::Parked
        });
        // A question that cannot be shown is answered "cancelled": the agent never proceeds on a guess.
        if let Decision::Refuse(Some(conn)) = decision {
            let _ = conn.respond(&rpc_id, cancelled_outcome());
        }
    }

    fn reject_request(&self, epoch: u64, id: &Value, method: &str) {
        let conn = self.with_core(|core, _| {
            if core.state.epoch() != epoch {
                return None;
            }
            core.stats.unsupported_requests += 1;
            if let Some(turn) = core.turn.as_mut() {
                turn.clock.frame(Instant::now(), false);
            }
            core.conn.clone()
        });
        if let Some(conn) = conn {
            // We offered no file access and no terminal: say so instead of leaving the agent waiting.
            let _ = conn.respond_error(id, RPC_METHOD_NOT_FOUND, &format!("this client does not support {method}"));
        }
    }

    fn on_exited(&self, epoch: u64, info: ExitInfo) {
        self.with_core(|core, ev| {
            if core.state.epoch() != epoch || matches!(core.state.phase(), Phase::Dormant | Phase::Exited) {
                return;
            }
            core.conn = None;
            if let Some(turn) = core.turn.take() {
                let _ = turn;
                let summary = core.state.on_turn_end(TurnOutcome::Failed { message: "the adapter exited before the turn finished".into(), code: None });
                ev.push(SessionEvent::TurnEnded(summary));
            }
            for ask_id in std::mem::take(&mut core.parked).into_keys() {
                core.state.on_permission_resolved(ask_id);
                ev.push(SessionEvent::PermissionResolved { ask_id, how: Resolution::AgentGone });
            }
            let kind = match info.cause {
                ExitCause::WriteStalled => ExitKind::WriteStalled,
                ExitCause::Killed => ExitKind::Closed,
                ExitCause::Exited | ExitCause::StdoutClosed => ExitKind::Crashed,
            };
            let summary = ExitSummary { kind, code: info.code, stderr_tail: info.stderr_tail, reason: None };
            core.state.on_exit(summary.clone());
            ev.push(SessionEvent::Exited(summary));
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restore {
    Resume,
    Load,
}

fn restore_order(preference: ResumePreference, caps: &AgentCaps) -> Vec<Restore> {
    let mut order = Vec::new();
    let resume = caps.resume.then_some(Restore::Resume);
    let load = caps.load_session.then_some(Restore::Load);
    match preference {
        ResumePreference::PreferResume => order.extend(resume.into_iter().chain(load)),
        ResumePreference::PreferLoad => order.extend(load.into_iter().chain(resume)),
    }
    order
}

fn is_auth_error(error: &AcpError) -> bool {
    match error {
        AcpError::Rpc { code, message, .. } => *code == RPC_AUTH_REQUIRED && message.to_lowercase().contains("auth"),
        _ => false,
    }
}

fn auth_required(caps: &AgentCaps, _cause: AcpError) -> AcpError {
    AcpError::AuthRequired { methods: caps.auth_methods.iter().map(|m| m.id.clone()).collect() }
}

#[cfg(test)]
pub(super) fn restore_order_for_tests(preference: ResumePreference, caps: &AgentCaps) -> Vec<&'static str> {
    restore_order(preference, caps).into_iter().map(|r| if r == Restore::Resume { "resume" } else { "load" }).collect()
}
