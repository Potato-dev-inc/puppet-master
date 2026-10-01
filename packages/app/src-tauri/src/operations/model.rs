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
