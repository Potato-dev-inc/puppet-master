//! Per-pane evaluation for `wait_for_panes`: settled/state/model/key-swap conditions and hints for the caller.

use super::*;

/// Hints for panes that have no OpenCode API session, keyed on the pane's agent type so a
/// Cursor approval or a shell prompt is never pointed at OpenCode-only tools.
pub(super) fn tui_hint_for(agent_type: &str, reason: &str, status: Option<&str>) -> Option<Value> {
    let blocked =
        matches!(reason, "waiting_input" | "tui_prompt") || status == Some("waiting_input");
    let kind = crate::tui::profile_for(agent_type).features().kind;
    match kind {
        crate::tui::Kind::Shell => (reason == "idle" || status == Some("idle")).then(|| json!({
            "action": "shell_exec",
            "tools": ["shell_exec", "read_terminal_buffer"],
            "detail": "Shell is at its prompt. Run a command with shell_exec (shell mode) or write_terminal_input, then read the output with read_terminal_buffer (scrollback view)."
        })),
        crate::tui::Kind::Cli if blocked => Some(json!({
            "action": "answer_prompt",
            "tools": ["answer_prompt", "read_terminal_buffer", "press_key"],
            "detail": "A prompt is blocking this pane. If `prompt` is present, check prompt.command (and prompt.cwd) is what you mean to approve, then call answer_prompt(pane_id, prompt.prompt_id, choice: allow_once|deny); this connection must control the pane (take_over with grant=true for panes it did not start). Otherwise read the screen once and use press_key (Cursor: y = run once, n or esc = skip). Workspace trust must be accepted by the user. Nothing is answered automatically."
        })),
        crate::tui::Kind::Cli if reason == "idle" => Some(json!({
            "action": "read_terminal_buffer",
            "tools": ["read_terminal_buffer"],
            "detail": "Agent settled idle; read the terminal buffer once for its output."
        })),
        _ => None,
    }
}

pub(super) fn check_pane(
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

    if until.contains(&WaitUntil::ModelReady)
        && status::is_model_ready(registry, pane_id, model_provider, model_id).unwrap_or(false)
    {
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

    if until.contains(&WaitUntil::TuiReady)
        && status::is_tui_ready(registry, pane_id).unwrap_or(false)
    {
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

pub(super) fn worker_settled_reason(
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
            if crate::tui::looks_like_menu(&buffer) && !buffer.contains("You selected") {
                return Some("tui_prompt");
            }
        }
    }
    None
}

pub(super) fn agent_hint_for_reason(
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

pub(super) fn clear_pending_model(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) {
    let mut reg = registry.lock();
    if let Some(pane) = reg.panes.get_mut(pane_id) {
        if let Some(link) = pane.opencode.as_ref() {
            *link.pending_model().lock() = None;
        }
    }
}

pub(super) fn try_key_swap_wait(
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
