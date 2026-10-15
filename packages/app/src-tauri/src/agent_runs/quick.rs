//! `quick_ask`: a tiny one-shot question, as easy as a native subagent call. It composes the
//! normal `run_agent` path with conservative settings (headless, read-only, fresh context, no
//! checks, no shell) and projects the result down to an answer plus a receipt.

use super::runtime::{AgentRunRequest, AgentRunView};
use crate::operations::OperationError;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) const DEFAULT_WAIT_MS: u64 = 90_000;
pub(crate) const MAX_WAIT_MS: u64 = 120_000;
const TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickAskRequest {
    pub task: String,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Defaults to true; only an explicit `false` lets the agent write.
    #[serde(default)]
    pub read_only: Option<bool>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// Backend for the run: the hint when it is a headless read-capable CLI that is installed, else
/// the first installed backend that enforces `read_only` (registry order). Never a backend that
/// cannot enforce read-only, because the default is read-only.
pub(crate) fn select_backend(
    hint: Option<&str>,
    installed: &dyn Fn(&str) -> bool,
) -> Result<String, OperationError> {
    let usable = crate::tui::read_only_backends();
    if let Some(hint) = hint.map(str::trim).filter(|hint| !hint.is_empty()) {
        let profile = crate::tui::profile_for(hint);
        let id = profile.id();
        if !usable.iter().any(|candidate| candidate.eq_ignore_ascii_case(id))
            || !crate::tui::is_agent_backend(id)
        {
            return Err(OperationError::new(
                "INVALID_ARGUMENT",
                format!("backend `{hint}` is not supported by quick_ask; use one of: {}", usable.join(", ")),
                false,
            ));
        }
        if !installed(id) {
            return Err(OperationError::new(
                "BACKEND_UNAVAILABLE",
                format!("backend `{id}` was not found on PATH"),
                false,
            ));
        }
        return Ok(id.to_string());
    }
    usable
        .iter()
        .find(|id| installed(id))
        .map(|id| id.to_string())
        .ok_or_else(|| {
            OperationError::new(
                "BACKEND_UNAVAILABLE",
                format!("none of the supported CLIs were found on PATH: {}", usable.join(", ")),
                false,
            )
        })
}

pub(crate) fn clamp_wait(wait_ms: Option<u64>) -> u64 {
    wait_ms.unwrap_or(DEFAULT_WAIT_MS).min(MAX_WAIT_MS)
}

/// The `run_agent` request a quick question becomes.
pub(crate) fn build_run_request(
    request: &QuickAskRequest,
    backend: &str,
) -> Result<AgentRunRequest, OperationError> {
    if request.task.trim().is_empty() {
        return Err(OperationError::new("INVALID_TASK", "task is required", false));
    }
    let body = json!({
        "task": request.task,
        "project_path": request.project_path,
        "agent_type": backend,
        "model": request.model,
        "headless": true,
        "background": false,
        "read_only": request.read_only.unwrap_or(true),
        "keep_pane": false,
        "context_policy": "fresh",
        "allow_shell": false,
        "trust_workspace": false,
        "timeout_ms": TIMEOUT_MS,
        "wait_ms": clamp_wait(request.wait_ms),
        "idempotency_key": request.idempotency_key,
    });
    serde_json::from_value(body)
        .map_err(|error| OperationError::new("INVALID_ARGUMENT", error.to_string(), false))
}

/// `{answer, receipt}` for a finished run, `{status:"running", ...}` for one still going.
pub(crate) fn project(view: &AgentRunView) -> Result<Value, OperationError> {
    match view.status.as_str() {
        "completed" => Ok(json!({"answer": view.result, "receipt": view.receipt})),
        "running" => Ok(json!({"status": "running", "handle": view.handle, "hint": "wait_agents"})),
        "needs_input" => Ok(json!({
            "status": "needs_input",
            "handle": view.handle,
            "prompt": view.prompt,
            "hint": "answer_prompt",
        })),
        other => {
            let mut error = view.error.clone().unwrap_or_else(|| {
                OperationError::new(
                    "QUICK_ASK_FAILED",
                    format!("run ended with status {other}"),
                    false,
                )
            });
            error.context = json!({"handle": view.handle, "status": other, "receipt": view.receipt});
            Err(error)
        }
    }
}

pub fn quick_ask(
    request: QuickAskRequest,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
    session_id: Option<String>,
) -> Result<Value, OperationError> {
    let backend = select_backend(request.backend.as_deref(), &|id| {
        crate::tui::profile_for(id).is_installed()
    })?;
    let run = build_run_request(&request, &backend)?;
    let view = super::runtime::run_agent(run, registry, app, session_id)?;
    project(&view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(task: &str) -> QuickAskRequest {
        serde_json::from_value(json!({"task": task})).unwrap()
    }

    #[test]
    fn builds_a_conservative_headless_read_only_run() {
        let run = build_run_request(&request("what is 2+2?"), "claude").unwrap();
        assert_eq!(run.agent_type, "claude");
        assert!(run.headless && !run.background && run.read_only && !run.keep_pane);
        assert!(!run.allow_shell && !run.trust_workspace);
        assert_eq!(run.timeout_ms, 300_000);
        assert_eq!(run.wait_ms, DEFAULT_WAIT_MS);
        assert_eq!(run.context_policy, crate::operations::ContextPolicy::Fresh);
        assert!(run.checks.is_none() && run.model.is_none());
    }

    #[test]
    fn caller_may_opt_out_of_read_only_and_wait_is_clamped() {
        let mut ask = request("x");
        ask.read_only = Some(false);
        ask.wait_ms = Some(999_999);
        ask.model = Some("m".into());
        ask.idempotency_key = Some("k".into());
        let run = build_run_request(&ask, "codex").unwrap();
        assert!(!run.read_only);
        assert_eq!(run.wait_ms, MAX_WAIT_MS);
        assert_eq!(run.model.as_deref(), Some("m"));
        assert_eq!(run.idempotency_key.as_deref(), Some("k"));
        assert!(build_run_request(&request("  "), "codex").is_err());
    }

    #[test]
    fn picks_first_installed_read_only_backend_and_never_opencode() {
        let order = crate::tui::read_only_backends();
        assert!(!order.contains(&"opencode") && !order.contains(&"opencode_native"));
        assert_eq!(select_backend(None, &|_| true).unwrap(), order[0]);
        // Only the last one is installed.
        let last = *order.last().unwrap();
        assert_eq!(select_backend(None, &|id| id == last).unwrap(), last);
        let none = select_backend(None, &|_| false).unwrap_err();
        assert_eq!(none.code, "BACKEND_UNAVAILABLE");
    }

    #[test]
    fn hint_must_be_a_read_only_backend_that_is_installed() {
        let order = crate::tui::read_only_backends();
        assert_eq!(select_backend(Some(order[1]), &|_| true).unwrap(), order[1]);
        assert_eq!(
            select_backend(Some(order[1]), &|_| false).unwrap_err().code,
            "BACKEND_UNAVAILABLE"
        );
        for bad in ["opencode", "powershell", "nonsense"] {
            assert_eq!(select_backend(Some(bad), &|_| true).unwrap_err().code, "INVALID_ARGUMENT", "{bad}");
        }
    }

    fn view(status: &str) -> AgentRunView {
        use crate::operations::{OperationSnapshot, OperationStatus, StateSource};
        let snapshot = OperationSnapshot {
            operation_id: "op-1".into(),
            runtime_id: "rt".into(),
            request_fingerprint: String::new(),
            project_path: "/tmp".into(),
            task: "task".into(),
            agent_type: "claude".into(),
            pane_id: None,
            pane_created: false,
            idempotency_key: "key".into(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            status: OperationStatus::Completed,
            revision: 2,
            created_at_ms: 0,
            updated_at_ms: 0,
            observed_at_ms: None,
            pane_state: None,
            required_action: None,
            source: StateSource::Native,
            stage: None,
            progress_pct: None,
            result: None,
            error: None,
            timeout_ms: 900_000,
            read_only: true,
            keep_pane: false,
            placement: None,
            keep_pane_requested: None,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: "h1".into(),
            turn_index: 0,
            verified: false,
            started_at_ms: None,
            finished_at_ms: Some(1),
            message_baseline_ids: vec![],
            output_baseline: None,
            result_capture: None,
            acceptance_status: Default::default(),
            worker: Default::default(),
            checks: Vec::new(),
            check_results: Vec::new(),
        warnings: Vec::new(),
        };
        let mut view = AgentRunView::from(snapshot);
        view.status = status.into();
        view.handle = "h1".into();
        view
    }

    #[test]
    fn projects_answer_with_receipt_or_a_running_handle() {
        let mut done = view("completed");
        done.result = Some("4".into());
        let value = project(&done).unwrap();
        assert_eq!(value["answer"], "4");
        assert_eq!(value["receipt"]["backend"], "claude");
        assert!(value["receipt"]["not_verified"].is_array());
        assert!(value.get("handle").is_none());

        let running = project(&view("running")).unwrap();
        assert_eq!(running, json!({"status":"running","handle":"h1","hint":"wait_agents"}));

        let failed = project(&view("failed")).unwrap_err();
        assert_eq!(failed.code, "QUICK_ASK_FAILED");
    }
}
