//! Operation record types: status and source enums, the delegate request, the persisted
//! snapshot, errors and wait results.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Queued,
    Starting,
    Running,
    #[serde(alias = "needs_input")]
    WaitingInput,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}
impl OperationStatus {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateSource {
    Native,
    Inferred,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateWorkRequest {
    pub project_path: String,
    pub task: String,
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    #[serde(default)]
    pub pane_id: Option<String>,
    pub idempotency_key: String,
    #[serde(default)]
    pub acceptance_criteria: Option<Vec<String>>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub locks: Vec<String>,
    /// Worker wall-clock deadline. Defaults to fifteen minutes.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// Request a restricted worker mode. Unsupported agents must reject this.
    #[serde(default)]
    pub read_only: bool,
    /// Keep a worker pane after its operation ends.
    #[serde(default)]
    pub keep_pane: bool,
    /// Where a visible pane opens: `auto` | `grid` | `window` (`None` = auto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    /// MCP client session that owns this run, when supplied by the host.
    #[serde(default)]
    pub owner_session_id: Option<String>,
    /// Whether the worker prompt explicitly enables Puppet Master tool access.
    #[serde(default)]
    pub worker_has_mcp_tools: bool,
    /// Optional stable conversation handle for subsequent turns.
    #[serde(default)]
    pub agent_run_id: Option<String>,
    #[serde(default)]
    pub turn_index: u32,
    #[serde(default)]
    pub worker: WorkerPersist,
    #[serde(default)]
    pub context_policy: Option<ContextPolicy>,
    /// State-based acceptance checks evaluated against the filesystem at completion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<crate::agent_runs::checks::Check>,
}

impl Default for DelegateWorkRequest {
    fn default() -> Self {
        Self {
            project_path: String::new(),
            task: String::new(),
            agent_type: default_agent_type(),
            pane_id: None,
            idempotency_key: String::new(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: Vec::new(),
            timeout_ms: default_timeout_ms(),
            read_only: false,
            keep_pane: false,
            placement: None,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: None,
            turn_index: 0,
            worker: WorkerPersist::default(),
            context_policy: None,
            checks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(default)]
    pub context: Value,
}
impl OperationError {
    pub fn new(code: &str, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
            retry_after_ms: None,
            context: Value::Null,
        }
    }

    /// A request body that did not parse. Always a client mistake, so every code here maps to a
    /// 4xx in `bridge::errors::operation_http_error`:
    /// - `INVALID_JSON`: not JSON at all (syntax error, truncated body);
    /// - `MISSING_FIELD`: valid JSON without a required field (`context.field` names it);
    /// - `INVALID_FIELD`: valid JSON whose shape or value is wrong (wrong type, unknown variant).
    pub fn invalid_body(error: &serde_json::Error) -> Self {
        use serde_json::error::Category;
        let text = error.to_string();
        let (code, field) = match error.classify() {
            Category::Syntax | Category::Eof | Category::Io => ("INVALID_JSON", None),
            Category::Data => match missing_field_name(&text) {
                Some(field) => ("MISSING_FIELD", Some(field)),
                None => ("INVALID_FIELD", None),
            },
        };
        let mut out = Self::new(code, text, false);
        if let Some(field) = field {
            out.context = serde_json::json!({ "field": field });
        }
        out
    }

    pub fn unknown_agent_type(id: impl Into<String>, valid: Vec<String>) -> Self {
        let id = id.into();
        let mut error = Self::new(
            "UNKNOWN_AGENT_TYPE",
            format!("unknown agent_type: {id}"),
            false,
        );
        error.context = serde_json::json!({
            "valid": valid,
            "hint": "GET /agents/types",
        });
        error
    }
    pub(super) fn io(err: impl std::fmt::Display) -> Self {
        Self::new("STORAGE_ERROR", err.to_string(), true)
    }
    /// Storage error that names the failing step and path (a bare OS error like
    /// "The system cannot find the path specified. (os error 3)" is undiagnosable).
    pub(super) fn io_at(
        step: &str,
        path: &std::path::Path,
        err: impl std::fmt::Display,
    ) -> Self {
        let mut error = Self::new(
            "STORAGE_ERROR",
            format!("{step} failed for {}: {err}", path.display()),
            true,
        );
        error.context = serde_json::json!({"step": step, "path": path.display().to_string()});
        error
    }
}

/// `missing field `prompt_id` at line 1 column 40` -> `prompt_id`.
fn missing_field_name(text: &str) -> Option<String> {
    let rest = text.strip_prefix("missing field `")?;
    rest.split('`').next().map(str::to_string)
}

#[cfg(test)]
mod io_at_tests {
    use super::*;

    #[test]
    fn io_at_names_step_and_path() {
        let error = OperationError::io_at(
            "create operations dir",
            std::path::Path::new("C:/x/Roaming/ws/.puppet-master/operations"),
            "The system cannot find the path specified. (os error 3)",
        );
        assert_eq!(error.code, "STORAGE_ERROR");
        assert!(error.message.contains("create operations dir"));
        assert!(error.message.contains("Roaming/ws"));
        assert_eq!(error.context["step"], "create operations dir");
    }
}

/// A caveat recorded on an operation while it ran or was stopped (never changes its status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationWarning {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationSnapshot {
    pub operation_id: String,
    #[serde(default)]
    pub runtime_id: String,
    #[serde(default)]
    pub request_fingerprint: String,
    pub project_path: String,
    pub task: String,
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    pub pane_id: Option<String>,
    #[serde(default)]
    pub pane_created: bool,
    pub idempotency_key: String,
    pub acceptance_criteria: Option<Vec<String>>,
    pub task_id: Option<String>,
    pub exclusive: bool,
    pub locks: Vec<String>,
    pub status: OperationStatus,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub observed_at_ms: Option<u64>,
    #[serde(default)]
    pub pane_state: Option<String>,
    #[serde(default)]
    pub required_action: Option<Value>,
    pub source: StateSource,
    pub stage: Option<String>,
    pub progress_pct: Option<u8>,
    pub result: Option<String>,
    pub error: Option<OperationError>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub keep_pane: bool,
    /// What the caller asked for. `keep_pane` is forced on when a turn is interrupted (so the
    /// worker can be resumed), which loses the request; `cancel_agent` needs it to know whether
    /// to release the pane. `None` on operations stored before this field existed.
    #[serde(default)]
    pub keep_pane_requested: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    #[serde(default)]
    pub owner_session_id: Option<String>,
    #[serde(default)]
    pub worker_has_mcp_tools: bool,
    #[serde(default)]
    pub agent_run_id: String,
    #[serde(default)]
    pub turn_index: u32,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub finished_at_ms: Option<u64>,
    /// IDs of native messages present before the current prompt was dispatched.
    #[serde(default)]
    pub message_baseline_ids: Vec<String>,
    /// Pre-dispatch TUI screen used to attribute new output to this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_baseline: Option<String>,
    #[serde(default)]
    pub result_capture: Option<ResultCapture>,
    #[serde(default)]
    pub acceptance_status: AcceptanceStatus,
    #[serde(default)]
    pub worker: WorkerPersist,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<crate::agent_runs::checks::Check>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub check_results: Vec<crate::agent_runs::checks::CheckResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<OperationWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicy {
    Fresh,
    #[default]
    Packet,
    ParentSummary,
    Resume,
    SelectedHistory,
}

impl ContextPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Packet => "packet",
            Self::ParentSummary => "parent_summary",
            Self::Resume => "resume",
            Self::SelectedHistory => "selected_history",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextContinuity {
    Resume,
    ReconstructedSummary,
    #[default]
    None,
}

impl ContextContinuity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::ReconstructedSummary => "reconstructed_summary",
            Self::None => "none",
        }
    }
}

/// When a run entered a `stage` (spawning_pane, waiting_for_pane_readiness, ...).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageMark {
    pub stage: String,
    pub at_ms: u64,
}

/// Durable collaborator record that outlives a single turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorkerPersist {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub requested_model: Option<String>,
    #[serde(default)]
    pub resolved_model: Option<String>,
    #[serde(default)]
    pub requested_reasoning: Option<String>,
    #[serde(default)]
    pub resolved_reasoning: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    /// True after the provider process/session was replaced; follow-ups must
    /// reconstruct context until a turn lands on the new session.
    #[serde(default)]
    pub session_reset: bool,
    #[serde(default)]
    pub context_policy: ContextPolicy,
    #[serde(default)]
    pub context_continuity: ContextContinuity,
    #[serde(default)]
    pub pending_messages: Vec<String>,
    #[serde(default)]
    pub queued_task_ids: Vec<String>,
    #[serde(default)]
    pub event_cursor: u64,
    #[serde(default)]
    pub closed: bool,
    /// True once the app closed this run's pane on its own (`keep_pane=false` terminal cleanup).
    #[serde(default)]
    pub pane_disposed: bool,
    /// `resolved_model` came from the CLI's configured default, not from the TUI footer: a best
    /// answer, not a confirmation of what ran.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub model_unconfirmed: bool,
    /// Caller opted in to `cursor-agent --trust` for headless runs in this workspace.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trust_workspace: bool,
    /// Caller opted in to `cursor-agent --force` (shell commands run unreviewed) for this run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_shell: bool,
    /// The user account (`crate::accounts`) this worker runs under; absent = the CLI's own login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The env guard this run's pane or process starts under (`pty::env_guard`, e.g. `no_push`).
    /// Set only by Rust code inside the app, never from a request body. Persisted so a restart,
    /// a headless fallback and a follow-up keep it. Absent = no guard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_env: Option<String>,
    /// A warm standby worker (`crate::standby`) ran this turn instead of a freshly spawned one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub standby_used: bool,
    /// The agent definition this turn started from (OBS-2); absent for code profiles. Set at every
    /// turn's spawn, so a later edit to the file never rewrites an earlier run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_definition: Option<crate::tui::DefinitionRef>,
    /// What the profile this turn started under could enforce, recorded at spawn so the receipt
    /// never re-reads a registry that an edit may have changed mid-run. Absent on old snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_features: Option<RunFeatures>,
    /// This turn asked for a visible pane but ran headless (see `agent_runs::fallback`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<RunFallback>,
    /// Set when the agent's "How agent runs work" setting decided this turn's mode
    /// (`agent_run_mode`); absent when the caller chose or the default applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_mode: Option<RunModeNote>,
    /// Programs pre-authorised for allow-once on Cursor shell prompts (see approval_policy).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_commands: Vec<String>,
    /// Stage entries for the current turn, oldest first (capped), for startup diagnosis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_log: Vec<StageMark>,
    /// The git worktree this handle works in (`isolation: "worktree"`). The operation's
    /// `project_path` stays the origin repo; only the spawn cwd moves (see `work_dir`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<crate::worktrees::WorktreeRef>,
}

impl OperationSnapshot {
    /// Directory the worker runs in: its worktree while that exists, else the project.
    pub fn work_dir(&self) -> &str {
        self.worker
            .worktree
            .as_ref()
            .and_then(|worktree| worktree.active_cwd())
            .unwrap_or(&self.project_path)
    }
}

/// A run that wanted a visible pane and ran headless instead. `reason` is `no_visible_mode` (the
/// agent has no visible mode) or `visible_failed` (the pane failed to launch or the prompt could
/// not be delivered before the turn started).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFallback {
    pub from: String,
    pub to: String,
    pub reason: String,
}

impl RunFallback {
    pub fn to_headless(reason: &str) -> Self {
        Self { from: "visible".into(), to: "headless".into(), reason: reason.into() }
    }

    /// One-task mode was chosen for the agent but is not (or no longer) verified: the run used a
    /// visible pane instead (`reason` `not_verified`).
    pub fn one_task_to_visible(reason: &str) -> Self {
        Self { from: "one_task".into(), to: "visible".into(), reason: reason.into() }
    }

    /// True for the fallback to the visible pane (not the usual one to headless).
    pub fn is_one_task_to_visible(&self) -> bool {
        self.from == "one_task"
    }
}

/// Which mode a run used and why, when the agent's setting decided it: `mode` is `one_task` or
/// `visible`, `source` is `agent_setting`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunModeNote {
    pub mode: String,
    pub source: String,
}

impl RunModeNote {
    pub fn new(mode: &str, source: &str) -> Self {
        Self { mode: mode.into(), source: source.into() }
    }
}

/// Facts about the profile a turn ran under that the receipt reports (see `WorkerPersist::run_features`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFeatures {
    pub read_only_headless: bool,
    pub read_only_spawned_pane: bool,
    pub experimental: bool,
}

impl RunFeatures {
    pub fn of(profile: &dyn crate::tui::TuiProfile) -> Self {
        let features = profile.features();
        Self {
            read_only_headless: features.read_only_headless,
            read_only_spawned_pane: features.read_only_spawned_pane,
            experimental: profile.experimental(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCapture {
    Authoritative,
    Inferred,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceStatus {
    #[default]
    NotChecked,
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationWaitResult {
    pub snapshot: OperationSnapshot,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationWaitCursor {
    pub project_path: String,
    pub operation_id: String,
    pub after_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiOperationWaitResult {
    pub snapshots: Vec<OperationSnapshot>,
    pub reason: String,
    #[serde(default)]
    pub wake_reasons: Vec<String>,
}

fn default_agent_type() -> String {
    "codex".into()
}

fn default_timeout_ms() -> u64 {
    900_000
}
