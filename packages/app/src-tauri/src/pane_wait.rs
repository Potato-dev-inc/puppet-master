//! Long-poll worker pane wait — avoids orchestrator MCP polling loops.

use crate::opencode::quota::{self, KeySwapEvent, KEY_ROTATED, KEY_SWAP_REQUIRED};
use crate::opencode::status;
use crate::pane_wait_notify;
use crate::pty::status::looks_like_opencode_tui_menu;
use crate::pty::{registry_read_buffer, PaneRegistry};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

pub fn wait_for_panes(
    registry: &Arc<Mutex<PaneRegistry>>,
    req: WaitForPanesRequest,
) -> Result<WaitForPanesResult, String> {
    let mut result = wait_for_panes_inner(registry, req)?;
    let blocked = result.status.as_deref() == Some("waiting_input")
        || matches!(result.reason.as_str(), "waiting_input" | "tui_prompt");
    if blocked {
        result.prompt = crate::pane_prompt::detect(registry, &result.pane_id, true);
    }
    if result.agent_hint.is_none() {
        if let Some(info) = status::pane_info(registry, &result.pane_id) {
            result.agent_hint =
                tui_hint_for(&info.agent_type, &result.reason, result.status.as_deref());
        }
    }
    Ok(result)
}

/// Hints for panes that have no OpenCode API session, keyed on the pane's agent type so a
/// Cursor approval or a shell prompt is never pointed at OpenCode-only tools.
fn tui_hint_for(agent_type: &str, reason: &str, status: Option<&str>) -> Option<Value> {
    let blocked =
        matches!(reason, "waiting_input" | "tui_prompt") || status == Some("waiting_input");
    match agent_type {
        "powershell" | "cmd" | "bash" => (reason == "idle" || status == Some("idle")).then(|| json!({
            "action": "shell_exec",
            "tools": ["shell_exec", "read_terminal_buffer"],
            "detail": "Shell is at its prompt. Run a command with shell_exec (shell mode) or write_terminal_input, then read the output with read_terminal_buffer (scrollback view)."
        })),
        "cursor_agent" | "claude" | "codex" | "cursor" | "opencode" if blocked => Some(json!({
            "action": "answer_prompt",
            "tools": ["answer_prompt", "read_terminal_buffer", "press_key"],
            "detail": "A prompt is blocking this pane. If `prompt` is present, call answer_prompt(pane_id, prompt.prompt_id, choice: allow_once|deny); this connection must control the pane (take_over with grant=true for panes it did not start). Otherwise read the screen once and use press_key (Cursor: y = run once, n or esc = skip). Workspace trust must be accepted by the user. Nothing is answered automatically."
        })),
        "claude" | "codex" | "cursor_agent" | "cursor" | "opencode" if reason == "idle" => Some(json!({
            "action": "read_terminal_buffer",
            "tools": ["read_terminal_buffer"],
            "detail": "Agent settled idle; read the terminal buffer once for its output."
        })),
        _ => None,
    }
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

fn check_pane(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    until: &[WaitUntil],
    model_provider: Option<&str>,
    model_id: Option<&str>,
    task_id: Option<&str>,
    output_regex: Option<&Regex>,
) -> Option<WaitForPanesResult> {
    let info = match status::pane_info(registry, pane_id) {
        Some(info) => info,
        None if until.contains(&WaitUntil::Gone) => {
            return Some(wait_result(
                "gone",
                pane_id,
                "error".into(),
                None,
                None,
                None,
                task_id.map(str::to_string),
                None,
                None,
            ));
        }
        None => return None,
    };
    let opencode = status::wait_snapshot(registry, pane_id).ok().flatten();

    if until.contains(&WaitUntil::Settled) || until.contains(&WaitUntil::TuiPrompt) {
        if let Some(reason) =
            worker_settled_reason(registry, pane_id, &info.status, opencode.as_ref(), until)
        {
            let buffer_tail = if reason == "tui_prompt" {
                registry_read_buffer(registry, pane_id, 40).ok()
            } else {
                None
            };
            return Some(wait_result(
                reason,
                pane_id,
                info.status.clone(),
                opencode,
                None,
                None,
                task_id.map(str::to_string),
                task_status(task_id),
                buffer_tail,
            ));
        }
    }

    if let Some(result) = try_key_swap_wait(
        registry,
        pane_id,
        until,
        &info.status,
        opencode.clone(),
        task_id,
    ) {
        return Some(result);
    }

    if until.contains(&WaitUntil::TaskCompleted) || until.contains(&WaitUntil::TaskBlocked) {
        if let Some(task_id) = task_id {
            if let Ok(read_models) = crate::event_log::rebuild_read_models() {
                if let Some(task) = read_models.tasks.iter().find(|task| task.id.0 == task_id) {
                    if until.contains(&WaitUntil::TaskCompleted) && task.status == "completed" {
                        return Some(wait_result(
                            "task_completed",
                            pane_id,
                            info.status.clone(),
                            opencode.clone(),
                            None,
                            None,
                            Some(task_id.to_string()),
                            Some(task.status.clone()),
                            None,
                        ));
                    }
                    if until.contains(&WaitUntil::TaskBlocked) && task.status == "blocked" {
                        return Some(wait_result(
                            "task_blocked",
                            pane_id,
                            info.status.clone(),
                            opencode.clone(),
                            None,
                            None,
                            Some(task_id.to_string()),
                            Some(task.status.clone()),
                            None,
                        ));
                    }
                }
            }
        }
    }

    if until.contains(&WaitUntil::OutputMatch) {
        if let Some(regex) = output_regex {
            if let Ok(buffer) = crate::pty::registry_read_buffer(registry, pane_id, 400) {
                if regex.is_match(&buffer) {
                    return Some(wait_result(
                        "output_match",
                        pane_id,
                        info.status.clone(),
                        opencode.clone(),
                        None,
                        None,
                        task_id.map(str::to_string),
                        task_status(task_id),
                        None,
                    ));
                }
            }
        }
    }

    if until.contains(&WaitUntil::ModelReady) {
        if status::is_model_ready(registry, pane_id, model_provider, model_id).unwrap_or(false) {
            clear_pending_model(registry, pane_id);
            return Some(wait_result(
                "model_ready",
                pane_id,
                info.status.clone(),
                opencode.clone(),
                None,
                None,
                task_id.map(str::to_string),
                task_status(task_id),
                None,
            ));
        }
    }

    if until.contains(&WaitUntil::TuiReady) {
        if status::is_tui_ready(registry, pane_id).unwrap_or(false) {
            return Some(wait_result(
                "tui_ready",
                pane_id,
                info.status.clone(),
                opencode.clone(),
                None,
                None,
                task_id.map(str::to_string),
                task_status(task_id),
                None,
            ));
        }
    }

    if let Some(snapshot) = opencode.as_ref() {
        if until.contains(&WaitUntil::Unhealthy) && !snapshot.serve_healthy {
            return Some(wait_result(
                "unhealthy",
                pane_id,
                info.status.clone(),
                opencode,
                None,
                None,
                task_id.map(str::to_string),
                task_status(task_id),
                None,
            ));
        }
        if until.contains(&WaitUntil::Permission) && snapshot.pending_permission_count > 0 {
            return Some(wait_result(
                "permission",
                pane_id,
                info.status.clone(),
                opencode,
                None,
                None,
                task_id.map(str::to_string),
                task_status(task_id),
                None,
            ));
        }
    }

    if until.contains(&WaitUntil::Idle) && info.status == "idle" {
        return Some(wait_result(
            "status_changed",
            pane_id,
            info.status,
            opencode,
            None,
            None,
            task_id.map(str::to_string),
            task_status(task_id),
            None,
        ));
    }
    // A blocked TUI can never become idle without an answer; do not sleep through it.
    let blocked_tui = info.status == "waiting_input"
        && !crate::pty::status::is_shell_agent(&info.agent_type)
        && until.contains(&WaitUntil::Idle);
    if (until.contains(&WaitUntil::WaitingInput) || blocked_tui) && info.status == "waiting_input" {
        return Some(wait_result(
            "status_changed",
            pane_id,
            info.status,
            opencode,
            None,
            None,
            task_id.map(str::to_string),
            task_status(task_id),
            None,
        ));
    }
    if until.contains(&WaitUntil::Running)
        && info.status == "running"
        && pane_has_visible_output(registry, pane_id)
    {
        return Some(wait_result(
            "running",
            pane_id,
            info.status,
            opencode,
            None,
            None,
            task_id.map(str::to_string),
            task_status(task_id),
            None,
        ));
    }
    if until.contains(&WaitUntil::Error) && info.status == "error" {
        return Some(wait_result(
            "status_changed",
            pane_id,
            info.status,
            opencode,
            None,
            None,
            task_id.map(str::to_string),
            task_status(task_id),
            None,
        ));
    }
    if until.contains(&WaitUntil::Gone) && info.status == "error" {
        return Some(wait_result(
            "gone",
            pane_id,
            info.status,
            opencode,
            None,
            None,
            task_id.map(str::to_string),
            task_status(task_id),
            None,
        ));
    }

    None
}

fn worker_settled_reason(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    pane_status: &str,
    opencode: Option<&status::OpenCodeWaitSnapshot>,
    until: &[WaitUntil],
) -> Option<&'static str> {
    let check_settled = until.contains(&WaitUntil::Settled);
    let check_tui = until.contains(&WaitUntil::TuiPrompt);
    if !check_settled && !check_tui {
        return None;
    }
    if check_settled {
        match pane_status {
            "idle" => return Some("idle"),
            "waiting_input" => return Some("waiting_input"),
            "error" => return Some("error"),
            _ => {}
        }
        if opencode.is_some_and(|snap| snap.pending_permission_count > 0) {
            return Some("permission");
        }
    }
    if check_settled || check_tui {
        if crate::opencode::messages::pending_question_for_pane(registry, pane_id).is_some() {
            return Some("tui_prompt");
        }
        if let Ok(buffer) = registry_read_buffer(registry, pane_id, 80) {
            if looks_like_opencode_tui_menu(&buffer) && !buffer.contains("You selected") {
                return Some("tui_prompt");
            }
        }
    }
    None
}

fn agent_hint_for_reason(
    reason: &str,
    opencode: Option<&status::OpenCodeWaitSnapshot>,
) -> Option<Value> {
    // These suggested actions call OpenCode API tools. Only native OpenCode
    // panes have the API-backed session those tools can address.
    opencode?;
    match reason {
        "tui_prompt" | "waiting_input" => Some(json!({
            "action": "reply_opencode_question",
            "tools": ["reply_opencode_question", "read_opencode_messages"],
            "detail": "Answer via reply_opencode_question (option label Yes/No). Do not use press_key — API prompts are not in the TUI input box."
        })),
        "permission" => {
            let request_id = opencode?.pending_permission_ids.first()?;
            Some(json!({
                "action": "reply_opencode_permission",
                "request_id": request_id,
                "detail": "OpenCode API permission pending — reply once, always, or deny."
            }))
        }
        "idle" => Some(json!({
            "action": "read_opencode_messages",
            "tools": ["read_opencode_messages", "read_terminal_buffer"],
            "detail": "Worker settled idle — read_opencode_messages for model output (opencode_native), or read_terminal_buffer once for raw TUI evidence."
        })),
        "timeout" => Some(json!({
            "action": "read_opencode_worker_status",
            "detail": "Wait timed out — check read_opencode_worker_status and read_terminal_buffer once; worker may still be thinking."
        })),
        _ => None,
    }
}

fn clear_pending_model(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) {
    let mut reg = registry.lock();
    if let Some(pane) = reg.panes.get_mut(pane_id) {
        if let Some(link) = pane.opencode.as_ref() {
            *link.pending_model().lock() = None;
        }
    }
}

fn try_key_swap_wait(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    until: &[WaitUntil],
    pane_status: &str,
    opencode: Option<status::OpenCodeWaitSnapshot>,
    task_id: Option<&str>,
) -> Option<WaitForPanesResult> {
    let want_swap = until.contains(&WaitUntil::KeySwapRequired);
    let want_rotated = until.contains(&WaitUntil::KeyRotated);
    if !want_swap && !want_rotated {
        return None;
    }

    let event = quota::pending_key_event(registry, pane_id)?;
    let reason = match_key_event_to_reason(&event, want_swap, want_rotated)?;
    let taken = quota::take_pending_key_event(registry, pane_id)?;
    Some(wait_result(
        reason,
        pane_id,
        pane_status.to_string(),
        opencode,
        Some(taken.from_profile),
        taken.to_profile,
        task_id.map(str::to_string),
        task_status(task_id),
        None,
    ))
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

pub fn wait_for_worker(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: vec!["settled".into(), "error".into()],
            timeout_ms,
            task_id: None,
            output_regex: None,
            model_match: None,
        },
    )
}

/// Spawn readiness for `delegate_work`. Claude/Ink TUIs stay `running` and never
/// match `settled` (idle / waiting_input). A live pane with output is enough to
/// accept the task prompt. `wait_for_worker` after a prompt is unchanged.
pub fn wait_for_dispatch_ready(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    let result = wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: vec!["settled".into(), "running".into(), "error".into()],
            timeout_ms,
            task_id: None,
            output_regex: None,
            model_match: None,
        },
    )?;
    if result.reason != "timeout" {
        return Ok(result);
    }
    // ponytail: Ink TUIs may never leave `running`; only proceed if the pane
    // is still live and has painted something. Dead/empty panes stay PANE_NOT_READY.
    if let Some(info) = status::pane_info(registry, pane_id) {
        if live_enough_for_dispatch(Some(&info.status))
            && pane_has_visible_output(registry, pane_id)
        {
            return Ok(wait_result(
                &info.status,
                pane_id,
                info.status.clone(),
                status::wait_snapshot(registry, pane_id).ok().flatten(),
                None,
                None,
                None,
                None,
                None,
            ));
        }
    }
    Ok(result)
}

fn pane_has_visible_output(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> bool {
    registry_read_buffer(registry, pane_id, 40)
        .map(|buf| !buf.trim().is_empty())
        .unwrap_or(false)
}

fn live_enough_for_dispatch(status: Option<&str>) -> bool {
    matches!(status, Some("running" | "idle" | "waiting_input"))
}

pub fn wait_for_model(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    model_provider: Option<&str>,
    model_id: Option<&str>,
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: vec!["model_ready".into(), "tui_ready".into(), "error".into()],
            timeout_ms,
            task_id: None,
            output_regex: None,
            model_match: Some(WaitModelMatch {
                provider_id: model_provider.map(str::to_string),
                model_id: model_id.map(str::to_string),
            }),
        },
    )
}

pub fn wait_for_task(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    task_id: &str,
    until: &[&str],
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: until.iter().map(|value| value.to_string()).collect(),
            timeout_ms,
            task_id: Some(task_id.to_string()),
            output_regex: None,
            model_match: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencode::quota::KeySwapEvent;

    #[test]
    fn parses_wait_triggers() {
        assert_eq!(
            WaitUntil::parse("permission").unwrap(),
            WaitUntil::Permission
        );
        assert_eq!(
            WaitUntil::parse("waiting").unwrap(),
            WaitUntil::WaitingInput
        );
        assert_eq!(
            WaitUntil::parse("key_swap").unwrap(),
            WaitUntil::KeySwapRequired
        );
        assert_eq!(
            WaitUntil::parse("rate_limited").unwrap(),
            WaitUntil::KeyRotated
        );
        assert_eq!(
            WaitUntil::parse("model_ready").unwrap(),
            WaitUntil::ModelReady
        );
        assert_eq!(WaitUntil::parse("tui_ready").unwrap(), WaitUntil::TuiReady);
        assert_eq!(
            WaitUntil::parse("task_completed").unwrap(),
            WaitUntil::TaskCompleted
        );
        assert_eq!(WaitUntil::parse("settled").unwrap(), WaitUntil::Settled);
        assert_eq!(
            WaitUntil::parse("tui_prompt").unwrap(),
            WaitUntil::TuiPrompt
        );
        assert_eq!(WaitUntil::parse("running").unwrap(), WaitUntil::Running);
    }

    #[test]
    fn live_running_pane_is_dispatchable_after_readiness_timeout() {
        assert!(live_enough_for_dispatch(Some("running")));
        assert!(live_enough_for_dispatch(Some("idle")));
        assert!(live_enough_for_dispatch(Some("waiting_input")));
        assert!(!live_enough_for_dispatch(Some("error")));
        assert!(!live_enough_for_dispatch(None));
    }

    #[test]
    fn dispatch_ready_accepts_running_claude_pane_with_output() {
        let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::new()));
        let pane = crate::pty::PaneRegistry::test_pane_stub("claude-1");
        pane.scrollback
            .lock()
            .push_chunk(b"Claude Code loaded\nLogin expired\n");
        registry.lock().panes.insert("claude-1".into(), pane);
        let result = wait_for_dispatch_ready(&registry, "claude-1", 1_000).expect("wait");
        assert_eq!(result.reason, "running");
        assert_eq!(result.status.as_deref(), Some("running"));
    }

    #[test]
    fn pane_info_uses_live_status_lock_not_spawn_snapshot() {
        let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::new()));
        let pane = crate::pty::PaneRegistry::test_pane_stub("live-status");
        *pane.status.lock() = crate::pty::status::PaneStatus::Idle;
        registry.lock().panes.insert("live-status".into(), pane);
        let info = status::pane_info(&registry, "live-status").expect("pane");
        assert_eq!(info.status, "idle");
        assert_ne!(
            info.status,
            registry.lock().panes["live-status"].info.status
        );
    }

    #[test]
    fn opencode_reply_hint_requires_native_state() {
        assert!(agent_hint_for_reason("tui_prompt", None).is_none());
    }

    #[test]
    fn default_until_includes_permission_and_key_swap() {
        let triggers = default_until();
        assert!(triggers.contains(&WaitUntil::Permission));
        assert!(triggers.contains(&WaitUntil::Idle));
        assert!(triggers.contains(&WaitUntil::KeySwapRequired));
        assert!(triggers.contains(&WaitUntil::KeyRotated));
    }

    #[test]
    fn match_key_event_maps_kind_to_reason() {
        let rotated = KeySwapEvent {
            kind: KEY_ROTATED.into(),
            pane_id: "p1".into(),
            from_profile: "a".into(),
            to_profile: Some("b".into()),
            reason: "rate_limited".into(),
            auto_rotated: true,
            at_ms: 0,
        };
        assert_eq!(
            match_key_event_to_reason(&rotated, true, true),
            Some("key_rotated")
        );
        assert_eq!(match_key_event_to_reason(&rotated, true, false), None);

        let swap = KeySwapEvent {
            kind: KEY_SWAP_REQUIRED.into(),
            pane_id: "p1".into(),
            from_profile: "a".into(),
            to_profile: None,
            reason: "rate_limited".into(),
            auto_rotated: false,
            at_ms: 0,
        };
        assert_eq!(
            match_key_event_to_reason(&swap, true, false),
            Some("key_swap_required")
        );
    }

    #[test]
    fn hints_follow_the_agent_type_not_opencode_presence() {
        let cursor = tui_hint_for("cursor_agent", "waiting_input", Some("waiting_input")).unwrap();
        assert!(cursor["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "press_key"));
        assert!(!cursor.to_string().contains("opencode"));
        let shell = tui_hint_for("powershell", "idle", Some("idle")).unwrap();
        assert_eq!(shell["action"], "shell_exec");
        assert!(tui_hint_for("powershell", "waiting_input", Some("waiting_input")).is_none());
        assert!(tui_hint_for("opencode_native", "idle", Some("idle")).is_none());
    }
}
