//! The chat of an ACP session (A2): what the agent says becomes `ChatItem`s that the desktop chat, the phone and
//! `GET /panes/{id}/chat?since=` read exactly as they read any other chat; what the agent asks becomes a card the
//! person answers; Stop becomes `session/cancel`.
//!
//! ```text
//!   AcpSession --events--> AcpChat::on_event
//!                            |-- Mapper (mapper.rs)  events -> items --> Feed (session_chat::pushed) --> Watchers --> views
//!                            |-- cards (cards.rs)    permission wait --> question card --> answer --> answer_permission
//!                            `-- status              snapshot --> the pane's PaneStatus (VirtualPaneHandles) + host events
//! ```
//!
//! * **A pane with no PTY.** `start` puts a virtual pane (`pty::registry::virtual_pane`) in the registry so the pane
//!   list, `wait_agents` and the phone know the session by its id; its status is the session snapshot's. Input that
//!   reaches the pane as terminal input is routed here (`terminal_input`): Ctrl+C and Esc stop the turn, a line of
//!   text is a message. A message from the Chat tab goes through the pane outbox (`pane_outbox`), which waits for the
//!   session to be ready, so "queue it while the agent works" is the outbox's own queue; steering is tried first
//!   when the agent takes it (`AcpChat::steer`).
//! * **Nothing is answered for the person.** Every `session/request_permission` becomes a card with the agent's real
//!   options (`cards.rs`). The agent waits with no timeout; the card may be raised again if it runs out of time
//!   (`reconcile_cards`) and survives a reloaded view or a reconnecting phone because the question store and the
//!   session snapshot both keep it. Stop answers every open question "cancelled" (the session does that).
//! * **Generic.** Nothing here names an agent; the agent is an `AgentConfig`.
//!
//! The Labs switch and the per-agent choice gate `start_gated`; `start` itself does not look at them (tests start
//! fake agents).

pub mod api;
pub mod cards;
pub mod mapper;
mod start;

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::error::AcpError;
use super::session::{AcpSession, Resolution, SessionEvent, SteerOutcome};
use super::state::{Phase, Snapshot};
use crate::pty::registry::VirtualPaneHandles;
use crate::pty::status::PaneStatus;
use crate::questions::{self, Question, QuestionState, ACP_KIND_PERMISSION};
use crate::session_chat::{Feed, PushedChanges, PushedPos, PushedSource};
use cards::Card;
use mapper::Mapper;

pub use start::{start_gated, StartDeps, StartError, StartRequest};

/// `PaneInfo.lane` of a chat session.
pub const LANE: &str = "acp";
/// Said when raw terminal input reaches a pane that has no terminal.
pub const NOT_A_TERMINAL: &str = "This pane is a chat session, not a terminal. Send a message instead.";
const WAITING: &str = "Starting the chat. Nothing to show yet.";

/// What the app does for a chat session beyond keeping its items: the pane's status events, the state the windows
/// show, and telling the windows that questions changed. The live app implements it with Tauri events and the
/// event log (`commands/acp_chat.rs`); tests record or ignore.
pub trait ChatHost: Send + Sync + 'static {
    /// The pane's status changed (`pty://status` and the event log).
    fn status_changed(&self, _pane_id: &str, _status: PaneStatus) {}
    /// The session's state changed (phase, queue, what waits on the person): the windows re-read it.
    fn state_changed(&self, _pane_id: &str, _state: &Value) {}
    /// A question card was raised or taken down (the windows and phones re-read the list; a push may go out).
    fn questions_changed(&self, _raised: Option<&Question>) {}
    /// The set of panes changed (a chat pane was added).
    fn panes_changed(&self) {}
}

/// A host that does nothing.
pub struct QuietHost;
impl ChatHost for QuietHost {}

/// Why a message could not be sent now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// A turn runs (or the session is starting): wait for it.
    Busy,
    /// The chat was closed.
    Gone,
    /// Refused; the sentence says why.
    Refused(String),
}

/// How a message went in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    /// A new turn started.
    Started,
    /// The turn that runs took it (steering).
    Steered,
}

/// Whether the session can take a message now (the pane outbox asks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    /// A turn runs, a question waits, or the session is starting.
    Busy,
    /// The chat was closed.
    Gone,
}

pub struct AcpChat {
    pane_id: String,
    agent_id: String,
    label: String,
    session: Arc<AcpSession>,
    feed: Arc<Feed>,
    mapper: Mutex<Mapper>,
    host: Arc<dyn ChatHost>,
    handles: Mutex<Option<VirtualPaneHandles>>,
    cards: Mutex<BTreeMap<u64, Card>>,
    /// What the person sent, until the turn it starts is reported (the user item is made from it).
    pending_user: Mutex<Option<String>>,
    /// The message the agent refused because it needs a sign-in, kept to send again once the person has signed in.
    sign_in_held: Mutex<Option<String>>,
    closed: AtomicBool,
    /// How long a card lives before it is raised again (a day; a test makes it short).
    card_timeout_s: AtomicU64,
}

static CHATS: Lazy<Mutex<HashMap<String, Arc<AcpChat>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// The chat session of a pane, if the pane is one.
pub fn get(pane_id: &str) -> Option<Arc<AcpChat>> {
    CHATS.lock().get(pane_id).cloned()
}

/// Every open chat session.
pub fn all() -> Vec<Arc<AcpChat>> {
    CHATS.lock().values().cloned().collect()
}

/// Closes every chat session (the Labs switch went off: it is also the kill switch). Their panes stay listed, idle.
pub fn close_all() {
    close_these(all());
}

fn close_these(chats: Vec<Arc<AcpChat>>) {
    for chat in chats {
        chat.close();
    }
}

/// The pane was closed (the registry took it): its session stops.
pub fn on_pane_closed(pane_id: &str) {
    let chat = CHATS.lock().remove(pane_id);
    if let Some(chat) = chat {
        chat.retire();
    }
}

/// A question card was answered or dismissed: when it is a permission card of a chat session, the answer goes to the
/// agent. Called by the question store after the lock is released.
pub fn on_question_resolved(question: &Question) {
    if question.kind.as_deref() != Some(ACP_KIND_PERMISSION) {
        return;
    }
    let Some(chat) = question.pane_id.as_deref().and_then(get) else { return };
    chat.card_resolved(question);
}

/// Terminal input that reached a chat pane. `None`: the pane is not a chat session (write to its terminal).
/// Ctrl+C and Esc stop the turn; a line of text is a message (a new turn, or steering); other keys are refused.
pub fn terminal_input(pane_id: &str, text: &str, append_newline: bool) -> Option<Result<(), String>> {
    let chat = get(pane_id)?;
    Some(chat.handle_terminal_input(text, append_newline))
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn control_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_control())
}

impl AcpChat {
    fn build(pane_id: &str, label: &str, session: Arc<AcpSession>, host: Arc<dyn ChatHost>) -> Arc<AcpChat> {
        Arc::new(AcpChat {
            pane_id: pane_id.to_string(),
            agent_id: session.agent_id().to_string(),
            label: label.to_string(),
            session,
            feed: Arc::new(Feed::new()),
            mapper: Mutex::new(Mapper::new()),
            host,
            handles: Mutex::new(None),
            cards: Mutex::new(BTreeMap::new()),
            pending_user: Mutex::new(None),
            sign_in_held: Mutex::new(None),
            closed: AtomicBool::new(false),
            card_timeout_s: AtomicU64::new(cards::CARD_TIMEOUT_S),
        })
    }

    pub fn pane_id(&self) -> &str {
        &self.pane_id
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn session(&self) -> &Arc<AcpSession> {
        &self.session
    }

    /// The items so far (what the pane's chat shows).
    pub fn items(&self) -> Vec<crate::session_chat::ChatItem> {
        self.feed.all()
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn set_card_timeout(&self, secs: u64) {
        self.card_timeout_s.store(secs, Ordering::SeqCst);
    }

    /// Connects the session's events to this chat. Called once, before the session starts.
    fn attach(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        self.session.add_sink(Arc::new(move |event| {
            if let Some(chat) = weak.upgrade() {
                chat.on_event(event);
            }
        }));
    }

    /// The pane's status handles: the pane is in the registry now.
    pub(crate) fn set_handles(&self, handles: VirtualPaneHandles) {
        *self.handles.lock() = Some(handles);
        self.apply_status(&self.session.snapshot());
    }

    fn push(&self, items: Vec<crate::session_chat::ChatItem>) {
        for item in items {
            self.feed.upsert(item);
        }
        if let Some(handles) = self.handles.lock().as_ref() {
            *handles.last_output.lock() = Instant::now();
        }
    }

    // ---- events ------------------------------------------------------------------------------------------

    fn on_event(&self, event: &SessionEvent) {
        let now = Instant::now();
        let at = now_iso();
        match event {
            SessionEvent::Snapshot(snapshot) => {
                self.apply_status(snapshot);
                self.host.state_changed(&self.pane_id, &self.state());
            }
            SessionEvent::TurnStarted { turn } => {
                let text = self.pending_user.lock().take();
                let mut mapper = self.mapper.lock();
                let items = mapper.turn_started(*turn, text.as_deref(), now, &at);
                self.push(items);
            }
            SessionEvent::PermissionRequested(wait) => {
                {
                    let mut mapper = self.mapper.lock();
                    let title = wait.title.clone().or_else(|| wait.tool_call_id.as_deref().and_then(|id| mapper.tool_title(id))).unwrap_or_else(|| "a tool".to_string());
                    let items = mapper.permission_notice(wait.ask_id, format!("Waiting for your answer: {title}"), &at);
                    self.push(items);
                }
                self.reconcile_cards();
            }
            SessionEvent::PermissionResolved { ask_id, how } => self.on_resolved(*ask_id, how, &at),
            other => {
                let mut mapper = self.mapper.lock();
                let items = mapper.on_event(other, now, &at);
                self.push(items);
            }
        }
    }

    fn apply_status(&self, snapshot: &Snapshot) {
        let status = snapshot.pane_status();
        let changed = {
            let handles = self.handles.lock();
            let Some(handles) = handles.as_ref() else { return };
            let mut current = handles.status.lock();
            let changed = *current != status;
            *current = status;
            changed
        };
        if changed {
            self.host.status_changed(&self.pane_id, status);
        }
    }

    // ---- permission cards --------------------------------------------------------------------------------

    /// Makes sure every open question of the agent has a card the person can answer, and raises a new one for a
    /// card that ran out of time or is gone. Cheap when there is nothing open.
    pub fn reconcile_cards(&self) {
        let waits = self.session.pending_permissions();
        if waits.is_empty() {
            return;
        }
        let now = questions::now_ms();
        let mut raised: Vec<Question> = Vec::new();
        {
            let mut cards = self.cards.lock();
            for wait in &waits {
                let needed = match cards.get(&wait.ask_id) {
                    None => true,
                    Some(card) if card.resolved => false,
                    // Pending, answered and dismissed cards are on their way; only a card that timed out or was
                    // forgotten by the store is raised again.
                    Some(card) => !questions::store().get(&card.question_id, now).is_some_and(|q| q.state != QuestionState::TimedOut),
                };
                if !needed {
                    continue;
                }
                let (title, kind, paths) = {
                    let mapper = self.mapper.lock();
                    let call_id = wait.tool_call_id.as_deref();
                    let title = wait.title.clone().or_else(|| call_id.and_then(|id| mapper.tool_title(id)));
                    let brief = call_id.and_then(|id| mapper.tool_brief(id));
                    (title, wait.tool_kind.clone().or_else(|| brief.as_ref().map(|b| b.0.clone())), brief.map(|b| b.1).unwrap_or_default())
                };
                let (mut new, options) = cards::new_question(&self.pane_id, &self.label, wait, title.as_deref(), kind.as_deref(), &paths);
                new.timeout_s = Some(self.card_timeout_s.load(Ordering::SeqCst));
                match questions::store().create(new, now) {
                    Ok(question) => {
                        cards.insert(wait.ask_id, Card { question_id: question.id.clone(), options, resolved: false });
                        raised.push(question);
                    }
                    Err(error) => {
                        // The store is full: the agent keeps waiting and the next look tries again.
                        tracing::warn!(pane = %self.pane_id, code = error.code, "acp chat: could not raise a permission card");
                    }
                }
            }
        }
        for question in &raised {
            crate::push::notify_question(question);
            self.host.questions_changed(Some(question));
        }
    }

    /// The person answered or dismissed a card: the choice goes back to the agent.
    fn card_resolved(&self, question: &Question) {
        enum Send {
            Option(u64, String),
            Cancel,
            Nothing,
        }
        let plan = {
            let mut cards = self.cards.lock();
            let Some((ask_id, card)) = cards.iter_mut().find(|(_, card)| card.question_id == question.id) else { return };
            if card.resolved {
                return;
            }
            let ask_id = *ask_id;
            match question.state {
                QuestionState::Answered => {
                    let choice = question.answer.as_ref().and_then(|a| a.choice_id.as_deref());
                    let option_id = choice.and_then(|id| card.option_for_choice(id)).map(|o| o.option_id.clone());
                    match option_id {
                        Some(option_id) => {
                            card.resolved = true;
                            Send::Option(ask_id, option_id)
                        }
                        None => Send::Nothing,
                    }
                }
                // Dismissed: the person said no. Never "yes", never silence.
                QuestionState::Cancelled => {
                    card.resolved = true;
                    match card.rejection().map(|o| o.option_id.clone()) {
                        Some(option_id) => Send::Option(ask_id, option_id),
                        None => Send::Cancel,
                    }
                }
                QuestionState::Pending | QuestionState::TimedOut => Send::Nothing,
            }
        };
        let result = match plan {
            Send::Option(ask_id, option_id) => self.session.answer_permission(ask_id, &option_id),
            Send::Cancel => self.session.cancel().map(|_| ()),
            Send::Nothing => Ok(()),
        };
        match result {
            Ok(()) | Err(AcpError::NoSuchAsk) => {}
            Err(error) => tracing::warn!(pane = %self.pane_id, %error, "acp chat: the answer did not reach the agent"),
        }
    }

    /// The session closed one of its questions (answered, cancelled, or the agent went away): the chat says how,
    /// and a card still on screen is taken down.
    fn on_resolved(&self, ask_id: u64, how: &Resolution, at: &str) {
        let card = self.cards.lock().remove(&ask_id);
        let name_of = |option_id: &str| {
            card.as_ref()
                .and_then(|c| c.options.iter().find(|o| o.option_id == option_id).map(|o| o.name.clone()))
                .unwrap_or_else(|| option_id.to_string())
        };
        let text = match how {
            Resolution::Selected(option_id) => format!("Answered: {}", name_of(option_id)),
            Resolution::Cancelled => "Cancelled.".to_string(),
            Resolution::AgentGone => "The agent stopped asking.".to_string(),
        };
        {
            let mut mapper = self.mapper.lock();
            let items = mapper.permission_notice(ask_id, text, at);
            self.push(items);
        }
        if let Some(card) = card {
            // Answered through the card: it is resolved already. Any other way: take it down (the hook finds no card).
            if !card.resolved {
                let _ = questions::store().dismiss(&card.question_id, questions::now_ms());
            }
            self.host.questions_changed(None);
        }
    }

    // ---- sending, steering, stopping ---------------------------------------------------------------------

    /// Whether the session can take a message now.
    pub fn readiness(&self) -> Readiness {
        if self.is_closed() {
            return Readiness::Gone;
        }
        let snapshot = self.session.snapshot();
        match snapshot.phase {
            Phase::Idle | Phase::Dormant => Readiness::Ready,
            Phase::Exited => match snapshot.exit.as_ref().map(|e| e.kind) {
                Some(super::state::ExitKind::Closed) => Readiness::Gone,
                // The next message brings the adapter back.
                _ => Readiness::Ready,
            },
            Phase::Starting | Phase::Resuming | Phase::Working | Phase::NeedsInput | Phase::Cancelling => Readiness::Busy,
        }
    }

    /// Whether the agent takes text while it works.
    pub fn can_steer(&self) -> bool {
        self.session.caps().is_some_and(|caps| caps.steering)
    }

    /// Starts a turn with `text`. `Busy` while a turn runs.
    pub fn send(&self, text: &str) -> Result<Sent, SendError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(SendError::Refused("Write a message first.".into()));
        }
        if self.is_closed() {
            return Err(SendError::Gone);
        }
        *self.pending_user.lock() = Some(text.to_string());
        match self.session.prompt_text(text) {
            Ok(_) => {
                self.sign_in_held.lock().take();
                Ok(Sent::Started)
            }
            Err(error) => {
                self.pending_user.lock().take();
                if needs_sign_in(&error) {
                    // The sign-in card shows; the text waits for "I'm signed in — try again".
                    *self.sign_in_held.lock() = Some(text.to_string());
                    self.host.state_changed(&self.pane_id, &self.state());
                }
                Err(send_error(&error))
            }
        }
    }

    /// Whether the agent refused to work until the person signs in (the sign-in card is shown).
    pub fn sign_in_needed(&self) -> bool {
        self.sign_in_held.lock().is_some()
    }

    /// "I'm signed in — try again": sends the message that was refused for want of a sign-in. If the agent still wants
    /// a sign-in the card stays.
    pub fn retry_after_sign_in(&self) -> Result<Sent, SendError> {
        let held = self.sign_in_held.lock().take();
        let Some(text) = held else { return Err(SendError::Refused("Nothing is waiting to be sent.".into())) };
        let result = self.send(&text);
        if result.is_err() && !self.sign_in_needed() {
            // Another reason it did not go: keep the text so the person can try again.
            *self.sign_in_held.lock() = Some(text);
        }
        self.host.state_changed(&self.pane_id, &self.state());
        result
    }

    /// Sends `text` into the turn that runs, when the agent takes that. `None`: it does not (or no turn runs); the
    /// caller queues the text instead. The text shows in the chat as the person's message.
    pub fn steer(&self, text: &str) -> Option<Result<(), String>> {
        let text = text.trim();
        if text.is_empty() || self.is_closed() || !self.can_steer() || self.readiness() != Readiness::Busy {
            return None;
        }
        if !self.session.snapshot().turn_in_flight() {
            return None;
        }
        match self.session.steer(vec![json!({ "type": "text", "text": text })]) {
            Ok(SteerOutcome::Injected) => {
                let mut mapper = self.mapper.lock();
                let items = mapper.steered(text, &now_iso());
                self.push(items);
                Some(Ok(()))
            }
            // The turn ended meanwhile: the text waits for the next one, like any queued message.
            Ok(SteerOutcome::PromptRequired) => None,
            Err(AcpError::Unsupported(_)) => None,
            Err(error) => Some(Err(format!("The agent did not take the message: {error}"))),
        }
    }

    /// Stops the turn that runs. True when there was one. The state says "stopping" until the agent ends it.
    pub fn cancel(&self) -> Result<bool, AcpError> {
        let stopped = self.session.cancel()?;
        if stopped {
            self.host.state_changed(&self.pane_id, &self.state());
        }
        Ok(stopped)
    }

    fn handle_terminal_input(&self, text: &str, append_newline: bool) -> Result<(), String> {
        match text {
            // Ctrl+C (the stop buttons, the panic stop, `interrupt_agent`) and Esc.
            "\u{3}" | "\u{1b}" => {
                self.cancel().map(|_| ()).map_err(|e| e.to_string())
            }
            "" if append_newline => Ok(()),
            t if control_only(t) => Err(NOT_A_TERMINAL.to_string()),
            t if append_newline => match self.send(t) {
                Ok(_) => Ok(()),
                Err(SendError::Busy) => match self.steer(t) {
                    Some(result) => result,
                    None => Err("The agent is working. Try again when the turn ends.".to_string()),
                },
                Err(SendError::Gone) => Err("The chat was closed.".to_string()),
                Err(SendError::Refused(why)) => Err(why),
            },
            _ => Err(NOT_A_TERMINAL.to_string()),
        }
    }

    /// Stops the session for good: the adapter and everything under it, and the open cards. The conversation stays
    /// readable (the pane stays listed, idle) until the pane itself is closed (`retire`).
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // Cards still open are taken down without an answer: nobody is waiting any more.
        let open: Vec<Card> = std::mem::take(&mut *self.cards.lock()).into_values().collect();
        for card in &open {
            if !card.resolved {
                let _ = questions::store().dismiss(&card.question_id, questions::now_ms());
            }
        }
        self.session.close();
        super::supervisor::global().remove(self.session.key());
        if let Some(handles) = self.handles.lock().as_ref() {
            *handles.exited.lock() = true;
        }
        if !open.is_empty() {
            self.host.questions_changed(None);
        }
    }

    /// The pane is gone: the session stops, the chat's readers end, and nothing finds it by pane id any more.
    pub(crate) fn retire(&self) {
        self.close();
        self.feed.close();
        crate::session_chat::pushed::unregister(&self.pane_id);
        CHATS.lock().remove(&self.pane_id);
    }

    // ---- state -------------------------------------------------------------------------------------------

    /// The session as a viewer needs it: phase, "stopping", what waits on the person, the settings the agent
    /// exposes. Raises a card for a question that has none.
    pub fn state(&self) -> Value {
        let snapshot = self.session.snapshot();
        let cards = self.cards.lock();
        let permissions: Vec<Value> = snapshot
            .permission_waits
            .iter()
            .map(|wait| {
                let card = cards.get(&wait.ask_id);
                json!({
                    "askId": wait.ask_id,
                    "title": wait.title,
                    "toolKind": wait.tool_kind,
                    "questionId": card.map(|c| c.question_id.clone()),
                    "options": card.map(|c| c.options.iter().map(|o| json!({ "id": o.choice_id, "name": o.name, "kind": o.kind })).collect::<Vec<_>>()),
                    "sinceMs": wait.since_ms,
                })
            })
            .collect();
        drop(cards);
        json!({
            "lane": LANE,
            "agent": self.agent_id,
            "label": self.label,
            "phase": serde_json::to_value(snapshot.phase).unwrap_or(Value::Null),
            "status": snapshot.pane_status().as_str(),
            "turnInFlight": snapshot.turn_in_flight(),
            "stopping": snapshot.phase == Phase::Cancelling,
            "canSteer": self.can_steer(),
            "turn": snapshot.turn,
            "epoch": snapshot.epoch,
            "revision": snapshot.revision,
            "title": snapshot.title,
            "usage": snapshot.usage.map(|u| json!({ "used": u.used, "size": u.size })),
            "commands": snapshot.commands.iter().map(|c| json!({ "name": c.name, "description": c.description, "hint": c.hint })).collect::<Vec<_>>(),
            "configOptions": snapshot.config_options,
            "modes": snapshot.modes,
            "historyLost": snapshot.history_lost.as_ref().map(|h| json!({ "reason": h.reason })),
            "permissions": permissions,
            "exit": snapshot.exit.as_ref().map(|e| json!({ "kind": serde_json::to_value(e.kind).unwrap_or(Value::Null), "code": e.code, "reason": e.reason })),
            "closed": self.is_closed(),
            "signIn": { "needed": self.sign_in_needed(), "hint": self.session.config().addon.signin_hint },
        })
    }
}

impl PushedSource for AcpChat {
    fn changes(&self, known: Option<PushedPos>) -> Option<PushedChanges> {
        if self.feed.is_closed() {
            return None;
        }
        // A reader that asks is a viewer that may have missed a card (a reloaded view, a phone that came back).
        self.reconcile_cards();
        Some(self.feed.changes_since(known))
    }

    fn waiting(&self) -> String {
        WAITING.to_string()
    }

    fn state(&self) -> Option<Value> {
        Some(AcpChat::state(self))
    }
}

/// The agent wants the person to sign in first (its `authMethods` with nothing signed in, or its auth error).
fn needs_sign_in(error: &AcpError) -> bool {
    matches!(error, AcpError::AuthRequired { .. } | AcpError::Rpc { code: crate::acp::error::RPC_AUTH_REQUIRED, .. })
}

fn send_error(error: &AcpError) -> SendError {
    match error {
        AcpError::Busy => SendError::Busy,
        AcpError::SessionClosed => SendError::Gone,
        AcpError::AuthRequired { .. } => SendError::Refused("The agent needs you to sign in.".into()),
        AcpError::Rpc { code: -32000, .. } => SendError::Refused("The agent needs you to sign in.".into()),
        AcpError::HistoryLost => SendError::Refused("The earlier conversation could not be brought back. Send again to start a new one.".into()),
        other => SendError::Refused(format!("The message was not sent: {other}")),
    }
}
