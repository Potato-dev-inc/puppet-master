//! The session state machine: pure, no I/O, fed by what the agent says.
//!
//! Nobody reads a screen here. The agent reports its own state through `session/update` and the stop
//! reason of `session/prompt`; this module folds that into one [`Snapshot`] that any viewer (desktop,
//! phone, `wait_agents`) can read without asking the agent anything. A viewer that connects late reads
//! the snapshot and sees "needs you" at once, because pending permission requests live here.
//!
//! * `epoch` counts adapter processes: it changes when the adapter is (re)started, so a viewer knows
//!   everything it cached about tools and commands is from an older process.
//! * `revision` counts changes the viewer can see. It moves only when the snapshot actually differs, so
//!   a thousand text chunks that change nothing here do not look like a thousand changes.
//!
//! Phases and what the rest of the app sees ([`Snapshot::pane_status`]):
//!
//! | phase | meaning | `PaneStatus` |
//! |---|---|---|
//! | `Starting` / `Resuming` | adapter starting / coming back | `Running` |
//! | `Idle` | ready, no turn | `Idle` |
//! | `Working` | a turn runs | `Running` |
//! | `NeedsInput` | a turn waits for a permission answer | `WaitingInput` |
//! | `Cancelling` | stop requested, waiting for the agent to end the turn | `Running` |
//! | `Dormant` | adapter stopped on purpose (idle), session resumable | `Idle` |
//! | `Exited` | adapter gone: closed by us = `Idle`, died / hung / deaf = `Error` | `Idle` / `Error` |

use serde::Serialize;
use serde_json::Value;

use super::handshake::SessionInfo;
use super::types::{AvailableCommand, RequestPermissionRequest, SessionUpdate, StopReason};
use crate::pty::status::PaneStatus;

pub const MAX_RUNNING_TOOLS: usize = 64;
pub const MAX_COMMANDS: usize = 100;
pub const MAX_CONFIG_OPTIONS: usize = 64;
pub const MAX_PERMISSION_WAITS: usize = 32;
const MAX_NAME_CHARS: usize = 64;
const MAX_TEXT_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Idle,
    Working,
    NeedsInput,
    Cancelling,
    Resuming,
    Dormant,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitKind {
    /// We stopped it on purpose and will not restart it.
    Closed,
    /// The process ended on its own (or before it answered).
    Crashed,
    /// It went quiet during a turn and was killed.
    Hung,
    /// It stopped reading its input and was killed.
    WriteStalled,
    /// It could not be started or could not open the session (see `reason`).
    StartFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExitSummary {
    pub kind: ExitKind,
    pub code: Option<i32>,
    pub stderr_tail: Vec<String>,
    /// Why, in words, when the adapter did not simply exit.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolBrief {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionBrief {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionWait {
    /// Ours: what a viewer sends back to answer.
    pub ask_id: u64,
    pub tool_call_id: Option<String>,
    pub title: Option<String>,
    pub tool_kind: Option<String>,
    pub options: Vec<OptionBrief>,
    /// Unix milliseconds.
    pub since_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryLost {
    pub old_session_id: String,
    pub new_session_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UsageBrief {
    pub used: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopKind {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    /// The turn could not finish: the adapter died, stopped reading, or the request failed.
    Failed,
    /// Stopped because the agent went quiet.
    Hung,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub epoch: u64,
    pub revision: u64,
    pub phase: Phase,
    pub session_id: Option<String>,
    /// Turns started in this session so far.
    pub turn: u64,
    pub running_tools: Vec<ToolBrief>,
    pub permission_waits: Vec<PermissionWait>,
    pub commands: Vec<CommandInfo>,
    pub config_options: Vec<Value>,
    pub modes: Option<Value>,
    pub title: Option<String>,
    pub usage: Option<UsageBrief>,
    pub last_stop: Option<StopKind>,
    /// The agent has been quiet this many seconds and the person has been asked what to do (Stop or Keep waiting).
    pub quiet_secs: Option<u64>,
    pub history_lost: Option<HistoryLost>,
    pub exit: Option<ExitSummary>,
}

impl Snapshot {
    fn initial() -> Self {
        Snapshot {
            epoch: 0,
            revision: 0,
            phase: Phase::Dormant,
            session_id: None,
            turn: 0,
            running_tools: Vec::new(),
            permission_waits: Vec::new(),
            commands: Vec::new(),
            config_options: Vec::new(),
            modes: None,
            title: None,
            usage: None,
            last_stop: None,
            quiet_secs: None,
            history_lost: None,
            exit: None,
        }
    }

    /// What `wait_agents`, badges and the phone already understand.
    pub fn pane_status(&self) -> PaneStatus {
        match self.phase {
            Phase::Starting | Phase::Resuming | Phase::Working | Phase::Cancelling => PaneStatus::Running,
            Phase::NeedsInput => PaneStatus::WaitingInput,
            Phase::Idle | Phase::Dormant => PaneStatus::Idle,
            Phase::Exited => match self.exit.as_ref().map(|e| e.kind) {
                Some(ExitKind::Closed) | None => PaneStatus::Idle,
                Some(_) => PaneStatus::Error,
            },
        }
    }

    /// A turn is in flight (running, waiting for an answer, or being stopped).
    pub fn turn_in_flight(&self) -> bool {
        matches!(self.phase, Phase::Working | Phase::NeedsInput | Phase::Cancelling)
    }
}

/// What a finished turn looked like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnSummary {
    pub turn: u64,
    pub stop: StopKind,
    /// The agent's own words for the stop reason (`end_turn`, `cancelled`...), when it gave one.
    pub stop_reason: Option<String>,
    /// We asked it to stop. The adapters may still answer `end_turn` (the spike saw exactly that), so this
    /// is remembered here and a cancelled turn is reported as `Cancelled` either way.
    pub cancel_requested: bool,
    /// The turn ended without a single text, thought or tool update: with `end_turn` that is the signature
    /// of a silent sign-in failure.
    pub empty: bool,
    pub error: Option<String>,
    /// The JSON-RPC code when the agent refused the request (-32000 = sign in first).
    pub error_code: Option<i64>,
}

/// How a turn ended, as the session learned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    Stop(StopReason),
    Failed { message: String, code: Option<i64> },
    Hung,
}

/// What `on_update` tells the session besides the state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpdateEffect {
    /// Real work (text, a tool, a plan), as opposed to bookkeeping. Resets the progress window.
    pub progress: bool,
}

pub struct SessionState {
    working: Snapshot,
    published: Snapshot,
    next_ask: u64,
    turn_active: bool,
    cancel_requested: bool,
    output_seen: bool,
    /// `config::Quirks::cancel_reports_end_turn`: a stop that was asked for is reported as cancelled even
    /// when the agent says `end_turn`.
    cancel_reports_end_turn: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    pub fn new() -> Self {
        let initial = Snapshot::initial();
        Self { working: initial.clone(), published: initial, next_ask: 1, turn_active: false, cancel_requested: false, output_seen: false, cancel_reports_end_turn: true }
    }

    pub fn set_cancel_reports_end_turn(&mut self, on: bool) {
        self.cancel_reports_end_turn = on;
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.published
    }

    pub fn phase(&self) -> Phase {
        self.working.phase
    }

    pub fn epoch(&self) -> u64 {
        self.working.epoch
    }

    pub fn session_id(&self) -> Option<&str> {
        self.working.session_id.as_deref()
    }

    /// The id of a session to bring back (before the first start) or the one `session/new` just made.
    pub fn adopt_session_id(&mut self, id: &str) {
        self.working.session_id = Some(id.to_string());
    }

    /// An open question as it will be published (the working copy, before `commit`).
    pub fn pending_wait(&self, ask_id: u64) -> Option<PermissionWait> {
        self.working.permission_waits.iter().find(|p| p.ask_id == ask_id).cloned()
    }

    pub fn config_options(&self) -> &[Value] {
        &self.working.config_options
    }

    /// Publishes the working copy if it differs. True when the revision moved.
    pub fn commit(&mut self) -> bool {
        let mut candidate = self.working.clone();
        candidate.revision = self.published.revision;
        if candidate == self.published {
            return false;
        }
        candidate.revision += 1;
        self.working.revision = candidate.revision;
        self.published = candidate;
        true
    }

    // ---- lifecycle -------------------------------------------------------------------------------------

    /// A new adapter process is starting. `resuming`: there is a session to bring back.
    pub fn begin_epoch(&mut self, resuming: bool) {
        let w = &mut self.working;
        w.epoch += 1;
        w.phase = if resuming { Phase::Resuming } else { Phase::Starting };
        w.running_tools.clear();
        w.permission_waits.clear();
        w.commands.clear();
        w.config_options.clear();
        w.modes = None;
        w.exit = None;
        self.cancel_requested = false;
    }

    pub fn on_ready(&mut self, session_id: &str, info: &SessionInfo) {
        let w = &mut self.working;
        w.session_id = Some(session_id.to_string());
        if !info.config_options.is_empty() {
            w.config_options = info.config_options.iter().take(MAX_CONFIG_OPTIONS).cloned().collect();
        }
        if info.modes.is_some() {
            w.modes = info.modes.clone();
        }
        w.phase = Phase::Idle;
        w.exit = None;
    }

    pub fn on_history_lost(&mut self, lost: HistoryLost) {
        self.working.history_lost = Some(lost);
    }

    pub fn clear_history_lost(&mut self) {
        self.working.history_lost = None;
    }

    /// The adapter stopped on purpose; the session can be brought back.
    pub fn on_dormant(&mut self) {
        let w = &mut self.working;
        w.phase = Phase::Dormant;
        w.running_tools.clear();
        w.permission_waits.clear();
        w.exit = None;
        self.turn_active = false;
    }

    /// The adapter is gone.
    pub fn on_exit(&mut self, exit: ExitSummary) {
        let w = &mut self.working;
        w.phase = Phase::Exited;
        w.running_tools.clear();
        w.permission_waits.clear();
        w.exit = Some(exit);
        self.turn_active = false;
    }

    // ---- turns -----------------------------------------------------------------------------------------

    /// A prompt goes out. Returns the new turn number.
    pub fn on_prompt_sent(&mut self) -> u64 {
        let w = &mut self.working;
        w.turn += 1;
        w.phase = Phase::Working;
        w.last_stop = None;
        self.turn_active = true;
        self.cancel_requested = false;
        self.output_seen = false;
        w.turn
    }

    /// The agent went quiet and the person is asked (`Some`), or the question is over (`None`).
    pub fn set_quiet(&mut self, secs: Option<u64>) {
        self.working.quiet_secs = secs;
    }

    pub fn on_cancel_requested(&mut self) {
        self.working.quiet_secs = None;
        if self.turn_active {
            self.working.phase = Phase::Cancelling;
            self.cancel_requested = true;
        }
    }

    pub fn on_turn_end(&mut self, outcome: TurnOutcome) -> TurnSummary {
        let (stop, stop_reason, error, error_code) = match &outcome {
            TurnOutcome::Stop(reason) => {
                let kind = match reason {
                    StopReason::EndTurn => StopKind::EndTurn,
                    StopReason::MaxTokens => StopKind::MaxTokens,
                    StopReason::MaxTurnRequests => StopKind::MaxTurnRequests,
                    StopReason::Refusal => StopKind::Refusal,
                    StopReason::Cancelled => StopKind::Cancelled,
                };
                (kind, Some(reason.as_str().to_string()), None, None)
            }
            TurnOutcome::Failed { message, code } => (StopKind::Failed, None, Some(message.clone()), *code),
            TurnOutcome::Hung => (StopKind::Hung, None, None, None),
        };
        // An agent that answers `end_turn` after being told to stop did stop: report what the person did.
        let stop = if self.cancel_requested && (stop == StopKind::Cancelled || (stop == StopKind::EndTurn && self.cancel_reports_end_turn)) { StopKind::Cancelled } else { stop };
        let summary = TurnSummary {
            turn: self.working.turn,
            stop,
            stop_reason,
            cancel_requested: self.cancel_requested,
            empty: !self.output_seen && matches!(stop, StopKind::EndTurn),
            error,
            error_code,
        };
        self.turn_active = false;
        let w = &mut self.working;
        if w.turn_in_flight() {
            w.phase = Phase::Idle;
        }
        w.running_tools.clear();
        w.permission_waits.clear();
        w.quiet_secs = None;
        w.last_stop = Some(stop);
        self.cancel_requested = false;
        summary
    }

    // ---- updates ---------------------------------------------------------------------------------------

    /// Folds one `session/update` in. `replay`: it is history being replayed by `session/load`, which
    /// must not look like live work.
    pub fn on_update(&mut self, update: &SessionUpdate, replay: bool) -> UpdateEffect {
        let progress = is_progress(update);
        match update {
            SessionUpdate::AgentMessageChunk(_) | SessionUpdate::AgentThoughtChunk(_) => {
                if !replay {
                    self.output_seen = true;
                }
            }
            SessionUpdate::ToolCall(call) if !replay => {
                self.output_seen = true;
                let brief = ToolBrief {
                    id: call.tool_call_id.clone(),
                    title: cut(&call.title, MAX_TEXT_CHARS),
                    kind: call.kind.clone(),
                    status: call.status.as_str().into(),
                };
                self.track_tool(brief, call.status.is_finished());
            }
            SessionUpdate::ToolCallUpdate(change) if !replay => {
                self.output_seen = true;
                let existing = self.working.running_tools.iter().find(|t| t.id == change.tool_call_id).cloned();
                let brief = ToolBrief {
                    id: change.tool_call_id.clone(),
                    title: change.title.as_deref().map(|t| cut(t, MAX_TEXT_CHARS)).or_else(|| existing.as_ref().map(|t| t.title.clone())).unwrap_or_default(),
                    kind: change.kind.clone().or_else(|| existing.as_ref().map(|t| t.kind.clone())).unwrap_or_else(|| "other".into()),
                    status: change.status.map(|s| s.as_str().to_string()).or_else(|| existing.map(|t| t.status)).unwrap_or_else(|| "in_progress".into()),
                };
                self.track_tool(brief, change.status.is_some_and(|s| s.is_finished()));
            }
            SessionUpdate::AvailableCommandsUpdate(list) => {
                self.working.commands = list.available_commands.iter().take(MAX_COMMANDS).filter_map(command_info).collect();
            }
            SessionUpdate::ConfigOptionUpdate(change) => {
                self.working.config_options = change.config_options.iter().take(MAX_CONFIG_OPTIONS).cloned().collect();
            }
            SessionUpdate::CurrentModeUpdate(change) => {
                if let Some(map) = self.working.modes.as_mut().and_then(Value::as_object_mut) {
                    map.insert("currentModeId".into(), Value::String(change.current_mode_id.clone()));
                }
            }
            SessionUpdate::SessionInfoUpdate(info) => {
                if let Some(title) = &info.title {
                    self.working.title = Some(cut(title, MAX_TEXT_CHARS));
                }
            }
            SessionUpdate::UsageUpdate(usage) => {
                self.working.usage = Some(UsageBrief { used: usage.used, size: usage.size });
            }
            _ => {}
        }
        UpdateEffect { progress }
    }

    fn track_tool(&mut self, brief: ToolBrief, finished: bool) {
        let tools = &mut self.working.running_tools;
        let at = tools.iter().position(|t| t.id == brief.id);
        if finished {
            if let Some(at) = at {
                tools.remove(at);
            }
            return;
        }
        match at {
            Some(at) => tools[at] = brief,
            None => {
                if tools.len() >= MAX_RUNNING_TOOLS {
                    tools.remove(0);
                }
                tools.push(brief);
            }
        }
    }

    // ---- permissions -----------------------------------------------------------------------------------

    /// The agent asks. Returns our id for the question. The state goes to `NeedsInput`.
    pub fn on_permission_request(&mut self, request: &RequestPermissionRequest, now_ms: u64) -> u64 {
        let ask_id = self.next_ask;
        self.next_ask += 1;
        let call = &request.tool_call;
        let wait = PermissionWait {
            ask_id,
            tool_call_id: Some(call.tool_call_id.clone()).filter(|id| !id.is_empty()),
            title: call.title.as_deref().map(|t| cut(t, MAX_TEXT_CHARS)),
            tool_kind: call.kind.clone(),
            options: request
                .options
                .iter()
                .map(|o| OptionBrief { id: o.option_id.clone(), name: cut(&o.name, MAX_TEXT_CHARS), kind: o.kind.clone() })
                .collect(),
            since_ms: now_ms,
        };
        let w = &mut self.working;
        if w.permission_waits.len() >= MAX_PERMISSION_WAITS {
            w.permission_waits.remove(0);
        }
        w.permission_waits.push(wait);
        if matches!(w.phase, Phase::Idle | Phase::Working) {
            w.phase = Phase::NeedsInput;
        }
        ask_id
    }

    /// An answer (or a cancel) went out, or the agent withdrew the question.
    pub fn on_permission_resolved(&mut self, ask_id: u64) -> bool {
        let w = &mut self.working;
        let before = w.permission_waits.len();
        w.permission_waits.retain(|p| p.ask_id != ask_id);
        let removed = w.permission_waits.len() != before;
        if removed && w.permission_waits.is_empty() && w.phase == Phase::NeedsInput {
            w.phase = if self.turn_active { Phase::Working } else { Phase::Idle };
        }
        removed
    }

    pub fn set_config_options(&mut self, options: Vec<Value>) {
        self.working.config_options = options.into_iter().take(MAX_CONFIG_OPTIONS).collect();
    }
}

/// Real work, as opposed to bookkeeping. An update kind this build does not know counts as work: it is
/// better to wait than to kill a turn because the adapter learned a new trick.
pub fn is_progress(update: &SessionUpdate) -> bool {
    !matches!(
        update,
        SessionUpdate::AvailableCommandsUpdate(_)
            | SessionUpdate::CurrentModeUpdate(_)
            | SessionUpdate::ConfigOptionUpdate(_)
            | SessionUpdate::SessionInfoUpdate(_)
            | SessionUpdate::UsageUpdate(_)
            | SessionUpdate::Notice(_)
    )
}

fn command_info(command: &AvailableCommand) -> Option<CommandInfo> {
    if command.name.is_empty() || command.name.chars().count() > MAX_NAME_CHARS {
        return None;
    }
    Some(CommandInfo {
        name: command.name.clone(),
        description: cut(&command.description, MAX_TEXT_CHARS),
        hint: command.input.as_ref().and_then(|i| i.hint.as_deref()).map(|h| cut(h, MAX_TEXT_CHARS)),
    })
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// For tests and logs: a compact line.
impl std::fmt::Display for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "epoch {} rev {} {:?} turn {} tools {} asks {}", self.epoch, self.revision, self.phase, self.turn, self.running_tools.len(), self.permission_waits.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn update(value: Value) -> SessionUpdate {
        serde_json::from_value(value).expect("a valid session update")
    }

    fn chunk(text: &str) -> SessionUpdate {
        update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}))
    }

    fn tool(id: &str, status: &str) -> SessionUpdate {
        update(json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": "Editing files", "kind": "edit", "status": status}))
    }

    fn tool_update(id: &str, status: &str) -> SessionUpdate {
        update(json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status}))
    }

    fn permission(id: &str) -> RequestPermissionRequest {
        serde_json::from_value(json!({
            "sessionId": "s1",
            "toolCall": {"toolCallId": id, "kind": "edit", "status": "pending"},
            "options": [
                {"optionId": "allow_once", "name": "Allow Once", "kind": "allow_once"},
                {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"}
            ]
        }))
        .unwrap()
    }

    fn ready(state: &mut SessionState) {
        state.begin_epoch(false);
        state.on_ready("s1", &SessionInfo::default());
        state.commit();
    }

    #[test]
    fn a_new_session_is_dormant_until_it_starts() {
        let state = SessionState::new();
        assert_eq!(state.snapshot().phase, Phase::Dormant);
        assert_eq!(state.snapshot().pane_status(), PaneStatus::Idle);
        assert_eq!((state.snapshot().epoch, state.snapshot().revision), (0, 0));
    }

    #[test]
    fn starting_then_ready_moves_epoch_once_and_revision_per_visible_change() {
        let mut state = SessionState::new();
        state.begin_epoch(false);
        assert!(state.commit());
        assert_eq!(state.snapshot().phase, Phase::Starting);
        assert_eq!(state.snapshot().pane_status(), PaneStatus::Running);
        state.on_ready("s1", &SessionInfo::default());
        assert!(state.commit());
        assert_eq!(state.snapshot().phase, Phase::Idle);
        assert_eq!(state.snapshot().epoch, 1);
        assert_eq!(state.snapshot().session_id.as_deref(), Some("s1"));
        // Nothing changed: no new revision.
        let revision = state.snapshot().revision;
        assert!(!state.commit());
        assert_eq!(state.snapshot().revision, revision);
    }

    #[test]
    fn a_thousand_text_chunks_do_not_move_the_revision() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        state.commit();
        let revision = state.snapshot().revision;
        for n in 0..1000 {
            let effect = state.on_update(&chunk(&format!("w{n} ")), false);
            assert!(effect.progress);
            assert!(!state.commit());
        }
        assert_eq!(state.snapshot().revision, revision);
    }

    #[test]
    fn a_turn_runs_and_ends_back_at_idle() {
        let mut state = SessionState::new();
        ready(&mut state);
        assert_eq!(state.on_prompt_sent(), 1);
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::Working);
        assert_eq!(state.snapshot().pane_status(), PaneStatus::Running);
        state.on_update(&chunk("hi"), false);
        let summary = state.on_turn_end(TurnOutcome::Stop(StopReason::EndTurn));
        state.commit();
        assert_eq!(summary.stop, StopKind::EndTurn);
        assert!(!summary.empty);
        assert_eq!(summary.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(state.snapshot().phase, Phase::Idle);
        assert_eq!(state.snapshot().last_stop, Some(StopKind::EndTurn));
        assert_eq!(state.on_prompt_sent(), 2);
    }

    #[test]
    fn tools_are_tracked_from_start_to_finish() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        state.on_update(&tool("t1", "in_progress"), false);
        state.on_update(&tool("t2", "pending"), false);
        state.commit();
        let ids: Vec<&str> = state.snapshot().running_tools.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t2"]);
        assert_eq!(state.snapshot().running_tools[0].kind, "edit");
        state.on_update(&tool_update("t1", "completed"), false);
        state.commit();
        assert_eq!(state.snapshot().running_tools.len(), 1);
        // A late update for a tool we never saw start still shows it running.
        state.on_update(&tool_update("t3", "in_progress"), false);
        state.commit();
        assert_eq!(state.snapshot().running_tools.len(), 2);
        // A turn end clears whatever is left.
        state.on_turn_end(TurnOutcome::Stop(StopReason::EndTurn));
        state.commit();
        assert!(state.snapshot().running_tools.is_empty());
    }

    #[test]
    fn the_running_tool_list_is_bounded() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        for n in 0..(MAX_RUNNING_TOOLS + 20) {
            state.on_update(&tool(&format!("t{n}"), "in_progress"), false);
        }
        state.commit();
        assert_eq!(state.snapshot().running_tools.len(), MAX_RUNNING_TOOLS);
        assert_eq!(state.snapshot().running_tools[0].id, "t20");
    }

    #[test]
    fn history_replay_changes_commands_but_never_looks_like_live_work() {
        let mut state = SessionState::new();
        ready(&mut state);
        let before = state.snapshot().clone();
        state.on_update(&tool("old", "in_progress"), true);
        state.on_update(&chunk("old text"), true);
        assert!(!state.commit());
        assert_eq!(state.snapshot(), &before);
        state.on_update(
            &update(json!({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "review", "description": "Review", "input": {"hint": "branch"}}]})),
            true,
        );
        assert!(state.commit());
        assert_eq!(state.snapshot().commands[0].hint.as_deref(), Some("branch"));
    }

    #[test]
    fn a_permission_request_parks_the_turn_until_it_is_answered() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        let ask = state.on_permission_request(&permission("call-1"), 1_700_000_000_000);
        state.commit();
        let snap = state.snapshot();
        assert_eq!(snap.phase, Phase::NeedsInput);
        assert_eq!(snap.pane_status(), PaneStatus::WaitingInput);
        assert_eq!(snap.permission_waits.len(), 1);
        assert_eq!(snap.permission_waits[0].ask_id, ask);
        assert_eq!(snap.permission_waits[0].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(snap.permission_waits[0].options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), vec!["allow_once", "reject_once"]);
        assert_eq!(snap.permission_waits[0].options[0].kind, "allow_once");
        assert!(state.on_permission_resolved(ask));
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::Working);
        assert!(!state.on_permission_resolved(ask), "answering twice is a no-op");
    }

    #[test]
    fn two_questions_need_two_answers() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        let first = state.on_permission_request(&permission("a"), 1);
        let second = state.on_permission_request(&permission("b"), 2);
        assert_ne!(first, second);
        state.on_permission_resolved(first);
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::NeedsInput);
        state.on_permission_resolved(second);
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::Working);
    }

    #[test]
    fn a_cancel_is_remembered_even_when_the_agent_says_end_turn() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        state.on_cancel_requested();
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::Cancelling);
        assert_eq!(state.snapshot().pane_status(), PaneStatus::Running);
        let summary = state.on_turn_end(TurnOutcome::Stop(StopReason::EndTurn));
        assert_eq!(summary.stop, StopKind::Cancelled);
        assert!(summary.cancel_requested);
        assert_eq!(summary.stop_reason.as_deref(), Some("end_turn"));
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::Idle);
    }

    #[test]
    fn an_agent_that_reports_its_own_stop_reason_is_believed_when_the_quirk_is_off() {
        let mut state = SessionState::new();
        state.set_cancel_reports_end_turn(false);
        ready(&mut state);
        state.on_prompt_sent();
        state.on_cancel_requested();
        let summary = state.on_turn_end(TurnOutcome::Stop(StopReason::EndTurn));
        assert_eq!(summary.stop, StopKind::EndTurn, "its word is kept");
        assert!(summary.cancel_requested, "but we still know we asked");
        state.on_prompt_sent();
        state.on_cancel_requested();
        assert_eq!(state.on_turn_end(TurnOutcome::Stop(StopReason::Cancelled)).stop, StopKind::Cancelled);
    }

    #[test]
    fn a_permission_that_arrives_while_cancelling_does_not_leave_cancelling() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        state.on_cancel_requested();
        state.on_permission_request(&permission("late"), 1);
        state.commit();
        assert_eq!(state.snapshot().phase, Phase::Cancelling);
    }

    #[test]
    fn a_cancel_with_no_turn_does_nothing() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_cancel_requested();
        assert!(!state.commit());
        assert_eq!(state.snapshot().phase, Phase::Idle);
    }

    #[test]
    fn an_end_turn_with_no_output_is_flagged_as_a_likely_silent_failure() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        let summary = state.on_turn_end(TurnOutcome::Stop(StopReason::EndTurn));
        assert!(summary.empty);
        state.on_prompt_sent();
        state.on_update(&tool("t", "in_progress"), false);
        assert!(!state.on_turn_end(TurnOutcome::Stop(StopReason::EndTurn)).empty);
        state.on_prompt_sent();
        assert!(!state.on_turn_end(TurnOutcome::Stop(StopReason::Refusal)).empty, "only end_turn can be a silent failure");
    }

    #[test]
    fn failures_and_hangs_are_turn_outcomes_too() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        let failed = state.on_turn_end(TurnOutcome::Failed { message: "adapter closed".into(), code: Some(-32000) });
        assert_eq!(failed.stop, StopKind::Failed);
        assert_eq!(failed.error.as_deref(), Some("adapter closed"));
        assert_eq!(failed.error_code, Some(-32000));
        state.on_prompt_sent();
        assert_eq!(state.on_turn_end(TurnOutcome::Hung).stop, StopKind::Hung);
    }

    #[test]
    fn exit_maps_to_idle_when_we_closed_it_and_to_error_when_it_died() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_exit(ExitSummary { kind: ExitKind::Closed, code: None, stderr_tail: vec![], reason: None });
        state.commit();
        assert_eq!(state.snapshot().pane_status(), PaneStatus::Idle);
        for kind in [ExitKind::Crashed, ExitKind::Hung, ExitKind::WriteStalled, ExitKind::StartFailed] {
            state.on_exit(ExitSummary { kind, code: Some(3), stderr_tail: vec!["boom".into()], reason: None });
            state.commit();
            assert_eq!(state.snapshot().phase, Phase::Exited);
            assert_eq!(state.snapshot().pane_status(), PaneStatus::Error);
        }
        // A restart is a new epoch and clears the exit.
        state.begin_epoch(true);
        state.commit();
        assert_eq!(state.snapshot().epoch, 2);
        assert_eq!(state.snapshot().phase, Phase::Resuming);
        assert!(state.snapshot().exit.is_none());
    }

    #[test]
    fn dormant_keeps_the_session_id_and_drops_process_state() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_prompt_sent();
        state.on_update(&tool("t", "in_progress"), false);
        state.on_dormant();
        state.commit();
        let snap = state.snapshot();
        assert_eq!(snap.phase, Phase::Dormant);
        assert_eq!(snap.session_id.as_deref(), Some("s1"));
        assert!(snap.running_tools.is_empty());
        assert_eq!(snap.pane_status(), PaneStatus::Idle);
    }

    #[test]
    fn history_lost_is_part_of_the_snapshot_until_cleared() {
        let mut state = SessionState::new();
        ready(&mut state);
        state.on_history_lost(HistoryLost { old_session_id: "a".into(), new_session_id: "b".into(), reason: "gone".into() });
        assert!(state.commit());
        assert_eq!(state.snapshot().history_lost.as_ref().unwrap().old_session_id, "a");
        state.clear_history_lost();
        assert!(state.commit());
        assert!(state.snapshot().history_lost.is_none());
    }

    #[test]
    fn commands_are_capped_and_misnamed_ones_dropped() {
        let mut state = SessionState::new();
        ready(&mut state);
        let many: Vec<Value> = (0..150).map(|n| json!({"name": format!("cmd{n}"), "description": "d".repeat(500)})).collect();
        let mut with_bad = many.clone();
        with_bad.insert(0, json!({"name": "x".repeat(100), "description": "too long a name"}));
        state.on_update(&update(json!({"sessionUpdate": "available_commands_update", "availableCommands": with_bad})), false);
        state.commit();
        let commands = &state.snapshot().commands;
        assert_eq!(commands.len(), MAX_COMMANDS - 1, "the first 100 were considered, one had a name that is too long");
        assert!(commands.iter().all(|c| c.description.chars().count() <= 300));
    }

    #[test]
    fn session_info_usage_and_mode_updates_land_in_the_snapshot() {
        let mut state = SessionState::new();
        state.begin_epoch(false);
        let info = SessionInfo { session_id: None, config_options: vec![json!({"id": "mode"})], modes: Some(json!({"currentModeId": "default"})) };
        state.on_ready("s1", &info);
        state.on_update(&update(json!({"sessionUpdate": "session_info_update", "title": "Fix the build"})), false);
        state.on_update(&update(json!({"sessionUpdate": "usage_update", "used": 100, "size": 200000})), false);
        state.on_update(&update(json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"})), false);
        state.commit();
        let snap = state.snapshot();
        assert_eq!(snap.title.as_deref(), Some("Fix the build"));
        assert_eq!(snap.usage, Some(UsageBrief { used: 100, size: 200000 }));
        assert_eq!(snap.modes.as_ref().unwrap()["currentModeId"], json!("plan"));
        assert_eq!(snap.config_options.len(), 1);
    }

    #[test]
    fn bookkeeping_updates_are_not_progress() {
        for value in [
            json!({"sessionUpdate": "session_info_update", "title": "t"}),
            json!({"sessionUpdate": "usage_update", "used": 1, "size": 2}),
            json!({"sessionUpdate": "available_commands_update", "availableCommands": []}),
        ] {
            assert!(!is_progress(&update(value)));
        }
        assert!(is_progress(&chunk("x")));
        assert!(is_progress(&tool("t", "pending")));
    }
}
