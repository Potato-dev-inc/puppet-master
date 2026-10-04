//! Long-poll worker pane wait — avoids orchestrator MCP polling loops.

use crate::opencode::quota::{self, KeySwapEvent, KEY_ROTATED, KEY_SWAP_REQUIRED};
use crate::opencode::status;
use crate::pane_wait_notify;
use crate::pty::{registry_read_buffer, PaneRegistry};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod check;
mod model_task;
mod worker;

use check::*;
pub use model_task::*;
pub use worker::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitUntil {
    Idle,
    WaitingInput,
    Error,
    Gone,
    Permission,
    Unhealthy,
    KeySwapRequired,
    KeyRotated,
    ModelReady,
    TuiReady,
    TaskCompleted,
    TaskBlocked,
    OutputMatch,
    /// Worker stopped: idle, waiting_input, permission, tui menu, or error.
    Settled,
    /// OpenCode TUI yes/no or numbered menu visible in scrollback.
    TuiPrompt,
    /// Pane is `running` and has produced output. Ink/Claude TUIs often never idle.
    Running,
}

impl WaitUntil {
    pub fn parse_all(raw: &[String]) -> Result<Vec<Self>, String> {
        if raw.is_empty() {
            return Ok(default_until());
        }
        raw.iter().map(|value| Self::parse(value)).collect()
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_lowercase().as_str() {
            "idle" => Ok(Self::Idle),
            "waiting_input" | "waiting" | "input" => Ok(Self::WaitingInput),
            "error" | "errored" => Ok(Self::Error),
            "gone" | "exit" | "exited" => Ok(Self::Gone),
            "permission" | "permissions" => Ok(Self::Permission),
            "unhealthy" | "health" => Ok(Self::Unhealthy),
            "key_swap_required" | "key_swap" => Ok(Self::KeySwapRequired),
            "key_rotated" | "rate_limited" => Ok(Self::KeyRotated),
            "model_ready" | "model" => Ok(Self::ModelReady),
            "tui_ready" | "tui_attached" | "attached" => Ok(Self::TuiReady),
            "task_completed" | "task_done" => Ok(Self::TaskCompleted),
            "task_blocked" | "blocked" => Ok(Self::TaskBlocked),
            "output_match" | "buffer_match" => Ok(Self::OutputMatch),
            "settled" | "worker_settled" | "stopped" | "done" => Ok(Self::Settled),
            "tui_prompt" | "tui_menu" | "confirmation" => Ok(Self::TuiPrompt),
            "running" => Ok(Self::Running),
            other => Err(format!("unsupported wait trigger '{other}'")),
        }
    }
}

fn default_until() -> Vec<WaitUntil> {
    vec![
        WaitUntil::Idle,
        WaitUntil::WaitingInput,
        WaitUntil::Error,
        WaitUntil::Gone,
        WaitUntil::Permission,
        WaitUntil::Unhealthy,
        WaitUntil::KeySwapRequired,
        WaitUntil::KeyRotated,
    ]
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WaitModelMatch {
    #[serde(rename = "provider_id", alias = "providerID")]
    pub provider_id: Option<String>,
    #[serde(rename = "model_id", alias = "modelID", alias = "id")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaitForPanesRequest {
    pub pane_ids: Vec<String>,
    #[serde(default)]
    pub until: Vec<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub output_regex: Option<String>,
    #[serde(default, rename = "match")]
    pub model_match: Option<WaitModelMatch>,
}

fn default_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WaitForPanesResult {
    pub reason: String,
    pub pane_id: String,
    pub status: Option<String>,
    pub opencode: Option<status::OpenCodeWaitSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_hint: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_tail: Option<String>,
    /// Structured approval prompt blocking the pane: `{prompt_id, kind, choices, text}`.
    /// Answer it with answer_prompt(pane_id, prompt_id, choice).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Value>,
}

const PROMPT_SETTLE_POLLS: usize = 6;
const PROMPT_SETTLE_STEP: Duration = Duration::from_millis(250);

pub fn wait_for_panes(
    registry: &Arc<Mutex<PaneRegistry>>,
    req: WaitForPanesRequest,
) -> Result<WaitForPanesResult, String> {
    let mut result = wait_for_panes_inner(registry, req)?;
    let blocked = result.status.as_deref() == Some("waiting_input")
        || matches!(result.reason.as_str(), "waiting_input" | "tui_prompt");
    if blocked {
        result.prompt = crate::pane_prompt::detect(registry, &result.pane_id, true);
        // A dialog can still be drawing when the status flips: give a CLI pane a moment to
        // settle so the first wake carries the classified prompt (id, kind, command).
        let settleable = result.prompt.is_none()
            && result.status.as_deref() == Some("waiting_input")
            && status::pane_info(registry, &result.pane_id).is_some_and(|info| {
                matches!(
                    crate::tui::profile_for(&info.agent_type).features().kind,
                    crate::tui::Kind::Cli
                )
            });
        if settleable {
            for _ in 0..PROMPT_SETTLE_POLLS {
                std::thread::sleep(PROMPT_SETTLE_STEP);
                result.prompt = crate::pane_prompt::detect(registry, &result.pane_id, true);
                if result.prompt.is_some() {
                    break;
                }
            }
        }
    }
    if result.agent_hint.is_none() {
        if let Some(info) = status::pane_info(registry, &result.pane_id) {
            result.agent_hint =
                tui_hint_for(&info.agent_type, &result.reason, result.status.as_deref());
        }
    }
    Ok(result)
}

fn wait_for_panes_inner(
    registry: &Arc<Mutex<PaneRegistry>>,
    req: WaitForPanesRequest,
) -> Result<WaitForPanesResult, String> {
    if req.pane_ids.is_empty() {
        return Err("pane_ids must not be empty".into());
    }
    let until = WaitUntil::parse_all(&req.until)?;
    let timeout = Duration::from_millis(req.timeout_ms.clamp(1_000, 300_000));
    let deadline = Instant::now() + timeout;
    let output_regex = req
        .output_regex
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Regex::new)
        .transpose()
        .map_err(|err| format!("invalid output_regex: {err}"))?;
    let model_provider = req.model_match.as_ref().and_then(|m| m.provider_id.clone());
    let model_id = req.model_match.as_ref().and_then(|m| m.model_id.clone());
    let mut wake_generation = 0_u64;

    loop {
        for pane_id in &req.pane_ids {
            if let Some(result) = check_pane(
                registry,
                pane_id,
                &until,
                model_provider.as_deref(),
                model_id.as_deref(),
                req.task_id.as_deref(),
                output_regex.as_ref(),
            ) {
                return Ok(result);
            }
        }

        if Instant::now() >= deadline {
            let pane_id = req.pane_ids[0].clone();
            let status = status::pane_info(registry, &pane_id).map(|info| info.status);
            let opencode = status::wait_snapshot(registry, &pane_id).ok().flatten();
            let buffer_tail = registry_read_buffer(registry, &pane_id, 40).ok();
            return Ok(WaitForPanesResult {
                reason: "timeout".into(),
                pane_id,
                status,
                opencode: opencode.clone(),
                from_profile: None,
                to_profile: None,
                task_id: req.task_id.clone(),
                task_status: task_status(req.task_id.as_deref()),
                agent_hint: agent_hint_for_reason("timeout", opencode.as_ref()),
                buffer_tail,
                prompt: None,
            });
        }
        pane_wait_notify::wait_for_change(deadline, &mut wake_generation);
    }
}

fn task_status(task_id: Option<&str>) -> Option<String> {
    let task_id = task_id?;
    let read_models = crate::event_log::rebuild_read_models().ok()?;
    read_models
        .tasks
        .iter()
        .find(|task| task.id.0 == task_id)
        .map(|task| task.status.clone())
}

pub(crate) fn match_key_event_to_reason(
    event: &KeySwapEvent,
    want_swap: bool,
    want_rotated: bool,
) -> Option<&'static str> {
    match event.kind.as_str() {
        KEY_ROTATED if want_rotated => Some("key_rotated"),
        KEY_SWAP_REQUIRED if want_swap => Some("key_swap_required"),
        KEY_ROTATED | KEY_SWAP_REQUIRED => None,
        _ => None,
    }
}

fn wait_result(
    reason: &str,
    pane_id: &str,
    status: String,
    opencode: Option<status::OpenCodeWaitSnapshot>,
    from_profile: Option<String>,
    to_profile: Option<String>,
    task_id: Option<String>,
    task_status: Option<String>,
    buffer_tail: Option<String>,
) -> WaitForPanesResult {
    WaitForPanesResult {
        reason: reason.to_string(),
        pane_id: pane_id.to_string(),
        status: Some(status),
        agent_hint: agent_hint_for_reason(reason, opencode.as_ref()),
        buffer_tail,
        prompt: None,
        opencode,
        from_profile,
        to_profile,
        task_id,
        task_status,
    }
}

/// Native OpenCode prompts go through the HTTP API, so the attached TUI does not have
/// to have painted (that takes seconds) before dispatch: serve healthy + attach live
/// is enough.
pub(crate) fn native_status_ready_for_dispatch(
    status: &crate::opencode::status::OpenCodeWorkerStatus,
) -> bool {
    matches!(status.pane_status.as_str(), "running" | "idle")
        && status.serve_healthy
        && status.tui_attached
        && !status.reattaching
        && !status.key_swap_pending
}

#[cfg(test)]
mod tests;
