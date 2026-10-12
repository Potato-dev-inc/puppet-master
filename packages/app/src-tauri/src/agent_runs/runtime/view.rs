//! `AgentRunView`: the flat JSON shape every agent tool returns for a run, built from a snapshot.

use super::receipt::{build_receipt, run_warnings, RunReceipt, RunWarning};
use crate::agent_runs::capabilities::{self, BackendCapabilities};
use crate::agent_runs::messaging::{self, Disposition};
use crate::agent_runs::persist::{self};
use crate::operations::{AcceptanceStatus, OperationSnapshot, OperationStatus, ResultCapture};
use serde::{Deserialize, Serialize};

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunView {
    pub handle: String,
    pub operation_id: String,
    pub turn_id: String,
    pub status: String,
    pub stage: Option<String>,
    pub result: Option<String>,
    pub result_capture: String,
    /// True when `result` is known to be only the tail of the reply: it outgrew the screen and the
    /// start of the turn could not be found in the scrollback (see the RESULT_TRUNCATED warning).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub result_truncated: bool,
    pub acceptance_status: String,
    /// Number of acceptance `checks` stored on this run, so a dropped gate is visible before
    /// completion (`checks` below holds results only once the run has finished).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub checks_configured: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<crate::agent_runs::checks::CheckResult>,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub result_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<serde_json::Value>,
    pub prompt: Option<serde_json::Value>,
    pub error: Option<crate::operations::OperationError>,
    pub pane_id: Option<String>,
    /// Whether the run's pane is still alive. Set only where the live registry is consulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_open: Option<bool>,
    /// True when the run is finished and its pane is being closed automatically right now
    /// (`pane_open` is already false in that case).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_closing: Option<bool>,
    /// Present when a finished run's pane stays open and needs `close_agent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_note: Option<String>,
    /// Prompt written to the worker until the turn finished (or now, while running).
    pub duration_ms: Option<u64>,
    /// Run created until the turn finished, so it includes queueing, pane spawn and readiness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<u64>,
    /// Run created until the prompt was written (`total_ms - duration_ms`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_ms: Option<u64>,
    pub revision: u64,
    pub next_cursor: u64,
    pub name: Option<String>,
    pub context_policy: String,
    pub context_continuity: String,
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    pub requested_reasoning: Option<String>,
    pub resolved_reasoning: Option<String>,
    pub model_mismatch: bool,
    #[serde(default)]
    pub session_reset: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing)]
    pub capabilities: BackendCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_steer: Option<SteerView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_unchanged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_command: Option<String>,
    /// Caveats on a finished run. They never change `status`; read them before trusting `result`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<RunWarning>,
    /// Honest summary of what this run did and did not verify, derived only from recorded facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<RunReceipt>,
    /// True when a warm standby worker (`crate::standby`) ran this turn (the stage log then shows
    /// `standby_claimed`). Absent for a freshly spawned worker.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub standby_used: bool,
    /// Present when the run wanted a visible pane and ran headless (same as `receipt.fallback`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<crate::operations::RunFallback>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerView {
    pub message_id: String,
    pub state: String,
    pub turn_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// End of a run's elapsed time: when it finished, else (still active) now. Measuring an active
/// run to its last update made a quiet run look like it had lasted only a few milliseconds.
fn run_end_ms(snapshot: &OperationSnapshot) -> u64 {
    if let Some(finished) = snapshot.finished_at_ms {
        return finished;
    }
    if snapshot.status.terminal() {
        return snapshot.updated_at_ms;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    now.max(snapshot.updated_at_ms)
}

/// `(duration_ms, total_ms, setup_ms)`. `started_at_ms` is stamped when the prompt is written to
/// the worker (after pane spawn and readiness), so `duration_ms` is the turn itself; `total_ms`
/// adds the setup before it. The dispatch start only stands in until a prompt is written.
pub(in crate::agent_runs) fn run_timings(
    snapshot: &OperationSnapshot,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    let end = run_end_ms(snapshot);
    let duration = snapshot
        .started_at_ms
        .map(|start| end.saturating_sub(start));
    let total = end.saturating_sub(snapshot.created_at_ms);
    let setup = snapshot
        .started_at_ms
        .map(|start| start.saturating_sub(snapshot.created_at_ms));
    (duration, Some(total), setup)
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl From<OperationSnapshot> for AgentRunView {
    fn from(snapshot: OperationSnapshot) -> Self {
        let (duration_ms, total_ms, setup_ms) = run_timings(&snapshot);
        let capabilities = capabilities::for_snapshot(&snapshot);
        let operation_id = snapshot.operation_id.clone();
        let project_path = snapshot.project_path.clone();
        let handle = if snapshot.agent_run_id.is_empty() {
            operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        };
        let turn_id = format!("turn-{}", snapshot.turn_index);
        let warnings = run_warnings(&snapshot);
        if snapshot.status.terminal() {
            messaging::expire_terminal_steers(&snapshot.project_path, &handle);
        }
        let last_steer =
            messaging::latest_steer(&snapshot.project_path, &handle).and_then(|receipt| {
                let same_turn = receipt.turn_id == turn_id;
                let open = matches!(
                    receipt.disposition,
                    Disposition::Accepted | Disposition::Queued | Disposition::Deferred
                );
                if !same_turn && !open {
                    return None;
                }
                Some(SteerView {
                    message_id: receipt.message_id,
                    state: receipt.disposition.as_str().into(),
                    turn_id: receipt.turn_id,
                    result: if same_turn { receipt.result } else { None },
                })
            });
        let result_capture: String = if snapshot.status.terminal() {
            match snapshot.result_capture {
                Some(ResultCapture::Authoritative) => "authoritative".into(),
                Some(ResultCapture::Inferred) => "inferred".into(),
                Some(ResultCapture::Missing) | None => "missing".into(),
            }
        } else {
            "pending".into()
        };
        let model_verified = match (
            snapshot.worker.requested_model.as_deref(),
            snapshot.worker.resolved_model.as_deref(),
        ) {
            (None, _) => None,
            (Some(requested), Some(resolved)) => Some(requested == resolved),
            (Some(_), None) => Some(false),
        };
        let receipt = build_receipt(&snapshot, &warnings, model_verified, &result_capture);
        Self {
            handle,
            turn_id,
            operation_id: operation_id.clone(),
            status: if snapshot
                .error
                .as_ref()
                .is_some_and(|error| error.code == "TIMEOUT")
            {
                "timeout".into()
            } else if snapshot.worker.closed || snapshot.stage.as_deref() == Some("closed") {
                "closed".into()
            } else if snapshot.stage.as_deref() == Some("interrupted") {
                "interrupted".into()
            } else {
                match snapshot.status {
                    OperationStatus::WaitingInput => "needs_input".into(),
                    OperationStatus::Queued
                    | OperationStatus::Starting
                    | OperationStatus::Running
                    | OperationStatus::Cancelling => "running".into(),
                    OperationStatus::Completed => "completed".into(),
                    OperationStatus::Failed => "failed".into(),
                    OperationStatus::Cancelled => "cancelled".into(),
                }
            },
            stage: snapshot.stage,
            result: snapshot.result,
            result_capture,
            result_truncated: snapshot
                .warnings
                .iter()
                .any(|warning| warning.code == super::RESULT_TRUNCATED),
            acceptance_status: match snapshot.acceptance_status {
                AcceptanceStatus::NotChecked => "not_checked".into(),
                AcceptanceStatus::Passed => "passed".into(),
                AcceptanceStatus::Failed => "failed".into(),
                AcceptanceStatus::Blocked => "blocked".into(),
            },
            checks_configured: snapshot.checks.len(),
            checks: snapshot.check_results.clone(),
            verified: snapshot.verified,
            result_verified: snapshot.verified,
            model_verified,
            recovery: snapshot.required_action.as_ref().and_then(|value| {
                value
                    .get("kind")
                    .and_then(|kind| kind.as_str())
                    .filter(|kind| matches!(*kind, "session_replaced" | "process_restarted"))
                    .map(|_| value.clone())
            }),
            prompt: snapshot
                .required_action
                .map(crate::agent_runs::headless::compact_prompt),
            error: snapshot.error,
            pane_id: snapshot.pane_id,
            pane_open: snapshot.worker.pane_disposed.then_some(false),
            pane_closing: None,
            pane_note: snapshot.worker.pane_disposed.then(|| {
                "pane was closed automatically when the run finished (keep_pane=false)".into()
            }),
            duration_ms,
            total_ms,
            setup_ms,
            revision: snapshot.revision,
            next_cursor: snapshot.worker.event_cursor.max(snapshot.revision),
            name: snapshot.worker.name.clone(),
            context_policy: snapshot.worker.context_policy.as_str().into(),
            context_continuity: snapshot.worker.context_continuity.as_str().into(),
            requested_model: snapshot.worker.requested_model.clone(),
            resolved_model: snapshot.worker.resolved_model.clone(),
            requested_reasoning: snapshot.worker.requested_reasoning.clone(),
            resolved_reasoning: snapshot.worker.resolved_reasoning.clone(),
            model_mismatch: persist::visible_mismatch(
                snapshot.worker.requested_model.as_deref(),
                snapshot.worker.resolved_model.as_deref(),
                snapshot.worker.requested_reasoning.as_deref(),
                snapshot.worker.resolved_reasoning.as_deref(),
            ),
            session_reset: snapshot.worker.session_reset,
            provider_session_id: snapshot.worker.provider_session_id.clone(),
            capabilities,
            last_steer,
            result_unchanged: None,
            wake_reason: None,
            // A settled turn has nothing left to wake on, so the caller needs no second call.
            watch_command: (!snapshot.status.terminal()).then(|| {
                crate::watch_command::format_watch_command(
                    operation_id.as_str(),
                    Some(project_path.as_str()),
                )
            }),
            warnings,
            receipt: Some(receipt),
            standby_used: snapshot.worker.standby_used,
            fallback: snapshot.worker.fallback.clone(),
        }
    }
}
