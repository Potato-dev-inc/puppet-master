use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus, StateSource};
use parking_lot::Mutex;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub(super) fn infer_tui_prompt(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    _agent_type: &str,
) -> Option<serde_json::Value> {
    crate::pane_prompt::required_action(registry, pane_id)
}

pub(crate) fn stable_prompt_id(prompt: &serde_json::Value) -> Option<String> {
    let id_text = serde_json::to_string(prompt).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id_text.hash(&mut hasher);
    Some(format!("prompt-{:016x}", hasher.finish()))
}

/// Steers being delivered right now, per operation. A live steer is accepted while the turn is
/// still running, but its user message reaches the session a moment later; completing the turn in
/// that gap would report the superseded answer. The supervisor therefore decides completion under
/// this lock and skips it while a steer is in flight, and a steer registers itself before it
/// re-checks that the turn is still running, so exactly one of "turn completed first" or "steer
/// registered first" holds.
static STEERS_IN_FLIGHT: once_cell::sync::Lazy<Mutex<std::collections::HashMap<String, usize>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

pub(crate) struct SteerGuard(String);

impl SteerGuard {
    pub(crate) fn begin(operation_id: &str) -> Self {
        *STEERS_IN_FLIGHT
            .lock()
            .entry(operation_id.to_string())
            .or_insert(0) += 1;
        Self(operation_id.to_string())
    }
}

impl Drop for SteerGuard {
    fn drop(&mut self) {
        let mut map = STEERS_IN_FLIGHT.lock();
        if let Some(count) = map.get_mut(&self.0) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                map.remove(&self.0);
            }
        }
    }
}

/// Run `decide` only if no steer is being delivered to `operation_id`; the check and `decide`
/// are atomic with respect to `SteerGuard::begin`. Returns whether `decide` ran.
pub(super) fn unless_steer_in_flight(operation_id: &str, decide: impl FnOnce()) -> bool {
    let map = STEERS_IN_FLIGHT.lock();
    if map.contains_key(operation_id) {
        return false;
    }
    decide();
    drop(map);
    true
}

/// How long an OpenCode server may stay unreachable before the run waiting on it is failed. A
/// restarting server is back well within this; a dead one never is.
pub(super) const SERVER_LOSS_GRACE_MS: u64 = 60_000;

static SERVER_DOWN_SINCE: once_cell::sync::Lazy<Mutex<std::collections::HashMap<String, u64>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

/// Record one health observation for `operation_id`'s OpenCode server. Returns how long (ms) it
/// has been unreachable once that exceeds the grace period, else `None`. A reachable observation
/// resets the clock.
pub(super) fn note_server_health(operation_id: &str, reachable: bool, now_ms: u64) -> Option<u64> {
    let mut down = SERVER_DOWN_SINCE.lock();
    if reachable {
        down.remove(operation_id);
        return None;
    }
    let since = *down.entry(operation_id.to_string()).or_insert(now_ms);
    let downtime = now_ms.saturating_sub(since);
    (downtime >= SERVER_LOSS_GRACE_MS).then_some(downtime)
}

fn server_unreachable_error(downtime_ms: u64, detail: &str) -> OperationError {
    let mut error = OperationError::new(
        "OPENCODE_SERVER_UNREACHABLE",
        format!(
            "the OpenCode server has been unreachable for {}s, so this run can no longer make progress; start a new run (the session is not recoverable while the server is down): {detail}",
            downtime_ms / 1000
        ),
        true,
    );
    error.context = serde_json::json!({"downtime_ms": downtime_ms});
    error
}

/// A native read failed. If the server itself is unreachable for longer than the grace period,
/// fail the run instead of letting it sit `running` until its whole timeout.
fn handle_native_read_failure(
    operation: &OperationSnapshot,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
    error: &str,
) {
    if !crate::opencode::client::is_unreachable_error(error) {
        return;
    }
    let now = crate::event_log::now_ms().max(0) as u64;
    let Some(downtime) = note_server_health(&operation.operation_id, false, now) else {
        return;
    };
    SERVER_DOWN_SINCE.lock().remove(&operation.operation_id);
    if let Ok(failed) = operations::mark_operation_state(
        &operation.project_path,
        &operation.operation_id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("failed".into()),
        None,
        Some(server_unreachable_error(downtime, error)),
    ) {
        let _ = crate::bridge::publish_operation(&failed, registry, app);
    }
}

pub(super) fn supervise_native_observation(
    operation: &OperationSnapshot,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) {
    let Some(pane_id) = operation.pane_id.as_deref() else {
        return;
    };
    let messages = match crate::opencode::messages::read_pane_messages_without_question_lookup(
        registry, pane_id, 10_000,
    ) {
            Ok(messages) => messages,
            Err(error) => {
                handle_native_read_failure(operation, registry, app, &error.to_string());
                return;
            }
        };
    let status = match crate::opencode::status::pending_permission_status(registry, pane_id) {
        Ok(status) => status,
        Err(error) => {
            handle_native_read_failure(operation, registry, app, &error.to_string());
            return;
        }
    };
    let permission_ids = status.pending_permission_ids.clone();
    let (base_url, directory) = {
        let guard = registry.lock();
        let Some(link) = guard
            .panes
            .get(pane_id)
            .and_then(|pane| pane.opencode.as_ref())
        else {
            return;
        };
        (link.base_url.clone(), link.directory.clone())
    };
    let pending_questions =
        match crate::opencode::client::list_questions(&base_url, Some(&directory)) {
            Ok(requests) => requests
                .into_iter()
                .filter(|request| request.session_id == messages.session_id)
                .collect::<Vec<_>>(),
            Err(error) => {
                handle_native_read_failure(operation, registry, app, &error.to_string());
                return;
            }
        };
    note_server_health(&operation.operation_id, true, 0);
    let required_action = if !permission_ids.is_empty() {
        let first = status
            .pending_permissions
            .first()
            .cloned();
        Some(serde_json::json!({
            "kind": "permission_required",
            "prompt_id": permission_ids.first(),
            "permission_ids": permission_ids,
            "action": first.as_ref().and_then(|permission| permission.action.clone()),
            "resource": first.as_ref().and_then(|permission| permission.resource.clone()),
            "scope": first.as_ref().and_then(|permission| permission.scope.clone()),
        }))
    } else if let Some(question) = pending_questions
        .first()
        .and_then(|request| request.questions.first())
    {
        let request_id = pending_questions.first().map(|request| request.id.clone());
        Some(serde_json::json!({
            "kind": "question_required",
            "prompt_id": request_id,
            "request_id": request_id,
            "question": question.question,
            "options": question.options.iter().map(|option| serde_json::json!({"label": option.label, "description": option.description})).collect::<Vec<_>>(),
        }))
    } else {
        None
    };
    if let Some(action) = required_action {
        if operation.status != OperationStatus::WaitingInput
            || operation.required_action.as_ref() != Some(&action)
        {
            if let Ok(waiting) = operations::mark_operation_waiting_input(
                &operation.project_path,
                &operation.operation_id,
                Some("waiting_input".into()),
                action,
                StateSource::Native,
            ) {
                let _ = crate::bridge::publish_operation(&waiting, registry, app);
            }
        }
        return;
    }
    if operation.status == OperationStatus::WaitingInput
        && operation.stage.as_deref() != Some("waiting_for_pane_readiness")
    {
        if let Ok(running) = operations::mark_operation_observation(
            &operation.project_path,
            &operation.operation_id,
            Some("running".into()),
            None,
            StateSource::Native,
        ) {
            let _ = crate::bridge::publish_operation(&running, registry, app);
        }
    }
    if operation.status == OperationStatus::Starting
        || operation.status == OperationStatus::Queued
        || operation.stage.as_deref() == Some("waiting_for_pane_readiness")
    {
        return;
    }
    if native_prompt_never_landed(
        &messages,
        &operation.message_baseline_ids,
        operation.started_at_ms,
        crate::event_log::now_ms().max(0) as u64,
        &status.pane_status,
    ) {
        let error = OperationError::new(
            "PROMPT_NOT_SUBMITTED",
            format!(
                "no new message appeared in the OpenCode session {}s after dispatch and the pane is idle; the prompt never landed. Retry the turn.",
                NATIVE_PROMPT_LANDING_MS / 1000
            ),
            true,
        );
        if let Ok(failed) = operations::mark_operation_state(
            &operation.project_path,
            &operation.operation_id,
            OperationStatus::Failed,
            StateSource::Native,
            Some("failed".into()),
            None,
            Some(error),
        ) {
            let _ = crate::bridge::publish_operation(&failed, registry, app);
        }
        return;
    }
    // A cancel aborts the assistant message, which reads as an agent error. The snapshot this
    // tick started from can predate the cancel, so ask again before turning that into a failure.
    let latest_status = operations::get_operation(&operation.project_path, &operation.operation_id)
        .map(|fresh| fresh.status)
        .unwrap_or(operation.status);
    if turn_is_ending(latest_status) {
        return;
    }
    // cancel_agent / interrupt_agent hold the handle's lock from the abort until the turn is marked
    // interrupted, and the abort reads as an agent error in between: leave the turn to them.
    let handle = if operation.agent_run_id.is_empty() {
        operation.operation_id.as_str()
    } else {
        operation.agent_run_id.as_str()
    };
    let stop_in_progress = super::runtime::handle_lock(handle);
    if stop_in_progress.try_lock().is_none() {
        return;
    }
    if native_completion_evidence(&messages, &operation.message_baseline_ids, false).is_none() {
        return;
    }
    // The reads above can be stale by the time no steer is in flight: a whole steer (register,
    // deliver, release) may have happened since. A steer posts its user message before it
    // releases, so re-reading under the same lock sees it and withholds the superseded answer.
    unless_steer_in_flight(&operation.operation_id, || match crate::opencode::messages::read_pane_messages_without_question_lookup(registry, pane_id, 10_000)
        .ok()
        .and_then(|fresh| native_completion_evidence(&fresh, &operation.message_baseline_ids, false))
    {
        Some(Err(error)) => {
            super::transcript::append(
                &operation.project_path,
                &operation.operation_id,
                "execution_failure",
                &crate::redact::text(&error),
            );
            let failure = OperationError::new("NATIVE_AGENT_ERROR", error, true);
            if let Ok(failed) = operations::mark_operation_state(
                &operation.project_path,
                &operation.operation_id,
                OperationStatus::Failed,
                StateSource::Native,
                Some("failed".into()),
                None,
                Some(failure),
            ) {
                let _ = crate::bridge::publish_operation(&failed, registry, app);
            }
        }
        Some(Ok(evidence)) => {
            super::transcript::append(
                &operation.project_path,
                &operation.operation_id,
                "assistant_final",
                &crate::redact::text(&evidence),
            );
            let handle = if operation.agent_run_id.is_empty() {
                operation.operation_id.as_str()
            } else {
                operation.agent_run_id.as_str()
            };
            super::messaging::mark_open_steers_processed_for_turn(
                &operation.project_path,
                handle,
                &evidence,
                operation.turn_index,
            );
            // Persist the model BEFORE the terminal revision is visible: a waiter woken by the
            // completion must not read a snapshot whose resolved_model is still null.
            if let Some(model) = crate::agent_runs::persist::live_resolved_model(registry, operation) {
                let _ = operations::record_terminal_pane_state(
                    &operation.project_path,
                    &operation.operation_id,
                    Some(model),
                    false,
                );
            }
            if let Ok(completed) = operations::mark_operation_completed(
                &operation.project_path,
                &operation.operation_id,
                &evidence,
                true,
            ) {
                let _ = crate::bridge::publish_operation(&completed, registry, app);
                if let Some(pane_id) = operation.pane_id.as_deref() {
                    crate::opencode::refresh_native_tui(registry, app, pane_id);
                }
            }
        }
        None => {}
    });
}

/// A dispatched prompt shows up as a new user message within moments. If none has appeared this
/// long after dispatch, the session is idle, and nothing is pending, the prompt was lost; without
/// this the turn would wait out its whole timeout with no error.
pub(super) const NATIVE_PROMPT_LANDING_MS: u64 = 45_000;

pub(super) fn native_prompt_never_landed(
    messages: &crate::opencode::messages::SessionMessagesView,
    baseline_ids: &[String],
    started_at_ms: Option<u64>,
    now_ms: u64,
    pane_status: &str,
) -> bool {
    let Some(started) = started_at_ms else {
        return false;
    };
    let waited = now_ms.saturating_sub(started);
    // A session that never goes idle is still not running our prompt if no message of ours ever
    // appeared; give it a longer, hard cap instead of waiting out the whole turn timeout.
    let patience = if pane_status == "idle" {
        NATIVE_PROMPT_LANDING_MS
    } else {
        NATIVE_PROMPT_LANDING_MS * 3
    };
    if waited < patience {
        return false;
    }
    !messages.messages.iter().any(|message| {
        message
            .id
            .as_deref()
            .is_some_and(|id| !baseline_ids.iter().any(|known| known == id))
    })
}

/// The turn is being cancelled or already finished: the supervisor must not conclude anything.
fn turn_is_ending(status: OperationStatus) -> bool {
    status == OperationStatus::Cancelling || status.terminal()
}

#[cfg(test)]
mod ending_tests {
    use super::*;

    #[test]
    fn a_cancelling_or_finished_turn_is_never_concluded_by_the_tick() {
        assert!(turn_is_ending(OperationStatus::Cancelling));
        assert!(turn_is_ending(OperationStatus::Cancelled));
        assert!(turn_is_ending(OperationStatus::Completed));
        assert!(!turn_is_ending(OperationStatus::Running));
        assert!(!turn_is_ending(OperationStatus::WaitingInput));
    }
}

pub(super) fn native_completion_evidence(
    messages: &crate::opencode::messages::SessionMessagesView,
    baseline_ids: &[String],
    has_pending_prompt: bool,
) -> Option<Result<String, String>> {
    if has_pending_prompt {
        return None;
    }
    let (assistant_index, assistant) = messages
        .messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, message)| message.role.eq_ignore_ascii_case("assistant"))?;
    // A user message after the newest assistant message is a steer that has not been answered
    // yet. The earlier answer belongs to the superseded instruction, so it is not the result.
    // Deciding this from message order (against the dispatch baseline) is race-free; re-reading
    // the baseline after a steer is not, because the steer's answer may already exist by then.
    if messages.messages[assistant_index + 1..]
        .iter()
        .any(|message| message.role.eq_ignore_ascii_case("user"))
    {
        return None;
    }
    let id = assistant.id.as_ref()?;
    if baseline_ids.iter().any(|known| known == id) {
        return None;
    }
    if let Some(error) = assistant.error.as_deref() {
        return Some(Err(error.to_string()));
    }
    if assistant.finish.as_deref() != Some("stop") {
        return None;
    }
    let text = assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            crate::opencode::messages::MessagePartView::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    (!text.is_empty()).then_some(Ok(text))
}
