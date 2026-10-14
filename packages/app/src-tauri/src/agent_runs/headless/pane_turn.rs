//! Helpers the pane supervisor uses on a visible TUI turn: reading the screen, auto-approving
//! allowed commands, re-delivering an undelivered prompt, and recording what the turn used.

use crate::agent_runs::{approval_policy, completion, delivery, runtime, transcript};
use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus, StateSource};
use parking_lot::Mutex;
use std::sync::Arc;

/// Answer a Cursor `Not in allowlist: <program>` prompt with allow-once when the run opted in to
/// that program. Returns true when the prompt was answered; false leaves it for the caller.
pub(super) fn auto_approve_allowed_command(
    current: &OperationSnapshot,
    action: &serde_json::Value,
    screen: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) -> bool {
    if !crate::tui::profile_for(&current.agent_type)
        .run_options()
        .allowed_commands
        || current.worker.allowed_commands.is_empty()
    {
        return false;
    }
    let Some(raw_prompt) = action.get("raw_prompt") else {
        return false;
    };
    if raw_prompt.get("kind").and_then(serde_json::Value::as_str) != Some("command_approval") {
        return false;
    }
    let Some(program) = approval_policy::decide(
        raw_prompt,
        screen,
        &current.worker.allowed_commands,
        Some(&current.project_path),
    ) else {
        return false;
    };
    let Some(pane_id) = current.pane_id.as_deref() else {
        return false;
    };
    // The screen can change between the read that produced `action` and the keypress: answer only
    // if the very same prompt is still showing.
    let still_showing = crate::pane_prompt::detect(registry, pane_id, false)
        .and_then(|prompt| prompt.get("prompt_id").cloned())
        == action.get("prompt_id").cloned();
    if !still_showing {
        return false;
    }
    let adapter = crate::agent_adapters::adapter_for(&current.agent_type);
    let Ok(reply) = adapter.prompt_reply(raw_prompt, "allow_once", false) else {
        return false;
    };
    let Some(text) = reply.key.or(reply.text) else {
        return false;
    };
    if crate::pty::registry::write_input(registry, app, pane_id, &text, true, false, None).is_err()
    {
        return false;
    }
    transcript::append(
        &current.project_path,
        &current.operation_id,
        "permission",
        &crate::redact::text(&format!("auto-approved once (run policy allowed_commands): {program}")),
    );
    if current.status == OperationStatus::WaitingInput {
        if let Ok(running) = operations::mark_operation_observation(
            &current.project_path,
            &current.operation_id,
            Some("running".into()),
            None,
            StateSource::Inferred,
        ) {
            let _ = crate::bridge::publish_operation(&running, registry, app);
        }
    }
    true
}

pub(super) fn read_tui_screen(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
) -> Option<String> {
    crate::pty::registry_read_snapshot(registry.as_ref(), pane_id).ok()
}

/// Run the delivery protocol (clear, retype, verify, submit, verify a turn started) for the
/// current turn's prompt and restart the turn watch from the screen as it is now. Returns false,
/// typing nothing, when a dialog is on screen (it takes hotkeys), so the caller can try later.
pub(super) fn redeliver_prompt(
    current: &OperationSnapshot,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
    watch: &mut completion::TuiTurnWatch,
    why: &str,
) -> bool {
    let Some(pane_id) = current.pane_id.as_deref() else {
        return false;
    };
    if let Some(screen) = read_tui_screen(registry, pane_id) {
        if crate::pane_prompt::dispatch_blocker(&current.agent_type, &screen).is_some() {
            return false;
        }
    }
    let profile = crate::tui::profile_for(&current.agent_type);
    let prompt = delivery::dispatch_prompt(
        &current.task,
        current.worker_has_mcp_tools,
        current.task_id.as_deref().unwrap_or_default(),
        pane_id,
        &current.project_path,
    );
    let mut io = delivery::PaneIo {
        registry,
        app,
        pane_id,
    };
    let mut trace = Vec::new();
    let delivery = delivery::deliver_prompt_traced(
        &mut io,
        profile.clear_input_sequence(),
        &prompt,
        &mut trace,
    );
    delivery::record_delivery(
        &current.project_path,
        &current.operation_id,
        Some(why),
        &delivery,
        &trace,
    );
    if !matches!(
        delivery,
        delivery::Delivery::NotVisible
            | delivery::Delivery::Blocked(_)
            | delivery::Delivery::WriteFailed(_)
    ) {
        runtime::record_user_task(&current.project_path, &current.operation_id, &prompt);
    }
    if let Some(screen) = read_tui_screen(registry, pane_id) {
        // Anything on screen before the retry was typed is stale.
        watch.restart(screen);
    }
    true
}

/// Why a run whose prompt was never typed can never be re-delivered, when the pane says so for
/// good: the user declined the startup dialog (`WORKSPACE_TRUST_DECLINED`), or the agent is gone
/// and its host shell shows a prompt (`AGENT_EXITED`). `None` when the pane might still take the
/// prompt (a dialog the user can answer, a screen that is still drawing).
///
/// Without this the supervisor asked for a re-delivery every tick, was refused every tick, and never
/// reached its deadline check: a declined trust dialog left the run `running` past its timeout.
pub(super) fn redelivery_dead_end(
    agent_type: &str,
    pane_id: &str,
    screen: &str,
) -> Option<OperationError> {
    if crate::pane_prompt::gate_was_declined(pane_id) {
        return Some(crate::bridge::startup_decline_error(pane_id, None));
    }
    let blocker = crate::pane_prompt::dispatch_blocker(agent_type, screen)?;
    (!blocker.recoverable).then_some(blocker)
}

/// The prompt of the turn before this one on the same handle (empty for a first turn). Its echo
/// stays on a reused pane's screen and must never be mistaken for this turn's output.
pub(super) fn previous_turn_task(snapshot: &OperationSnapshot) -> String {
    if snapshot.turn_index == 0 {
        return String::new();
    }
    let handle = if snapshot.agent_run_id.is_empty() {
        snapshot.operation_id.as_str()
    } else {
        snapshot.agent_run_id.as_str()
    };
    operations::turns_for_handle(&snapshot.project_path, handle)
        .unwrap_or_default()
        .into_iter()
        .rfind(|turn| turn.turn_index < snapshot.turn_index)
        .map(|turn| turn.task)
        .unwrap_or_default()
}

/// Record the model a completed pane run used: the TUI footer when visible (a confirmation),
/// else the CLI's configured default flagged unconfirmed, so `resolved_model` is not left null.
pub(super) fn record_completion_model(current: &OperationSnapshot, screen: &str) {
    let Some(found) = completion::completion_model(
        &current.agent_type,
        screen,
        current.worker.resolved_model.as_deref(),
        || crate::tui::profile_for(&current.agent_type).default_model(),
    ) else {
        return;
    };
    if found.confirmed {
        let _ = operations::record_terminal_pane_state(
            &current.project_path,
            &current.operation_id,
            Some(found.model),
            false,
        );
    } else {
        let _ = operations::record_unconfirmed_model(
            &current.project_path,
            &current.operation_id,
            &found.model,
        );
    }
}

/// Raw evidence for a RESULT_NOT_CAPTURED failure: the visible rows, the scrollback with history,
/// the raw output as the PTY wrote it (escape codes stripped), and what the watch saw. Bounded.
pub(super) fn not_captured_diagnostics(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    watch: &completion::TuiTurnWatch,
) -> serde_json::Value {
    let tail = |text: String, lines: usize| {
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    };
    serde_json::json!({
        "screen": read_tui_screen(registry, pane_id).unwrap_or_default(),
        "scrollback_tail": tail(
            crate::pty::registry_read_full_text(registry.as_ref(), pane_id).unwrap_or_default(),
            80,
        ),
        "raw_output_tail": tail(
            crate::pty::registry_read_buffer(registry.as_ref(), pane_id, 80).unwrap_or_default(),
            80,
        ),
        "watch": watch.diagnostics(),
    })
}
