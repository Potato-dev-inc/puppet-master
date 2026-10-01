use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus, StateSource};
use parking_lot::Mutex;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub(super) fn infer_tui_prompt(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    agent_type: &str,
) -> Option<serde_json::Value> {
    let screen = {
        let guard = registry.lock();
        let pane = guard.panes.get(pane_id)?;
        let contents = pane.screen.lock().screen().contents();
        contents
    };
    let adapter = crate::agent_adapters::adapter_for(agent_type);
    let prompt = adapter.detect_prompt(&screen, false)?;
    let prompt_id = stable_prompt_id(&prompt)?;
    let options = prompt
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|choice| serde_json::json!({"id": choice, "label": choice}))
        .collect::<Vec<_>>();
    Some(serde_json::json!({
        "id": prompt_id,
        "prompt_id": prompt_id,
        "kind": prompt.get("kind"),
        "summary": prompt.get("text"),
        "options": options,
        "raw_prompt": prompt,
    }))
}

pub(crate) fn stable_prompt_id(prompt: &serde_json::Value) -> Option<String> {
    let id_text = serde_json::to_string(prompt).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id_text.hash(&mut hasher);
    Some(format!("prompt-{:016x}", hasher.finish()))
}

pub(super) fn supervise_native_observation(
    operation: &OperationSnapshot,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) {
    let Some(pane_id) = operation.pane_id.as_deref() else {
        return;
    };
    let messages =
        match crate::opencode::messages::read_pane_messages(registry, pane_id, 10_000, None) {
            Ok(messages) => messages,
            Err(_) => return,
        };
    let status = match crate::opencode::status::worker_status(registry, pane_id) {
        Ok(status) => status,
        Err(_) => return,
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
            Err(_) => return,
        };
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
    match native_completion_evidence(&messages, &operation.message_baseline_ids, false) {
        Some(Err(error)) => {
            super::transcript::append(
                &operation.project_path,
                &operation.operation_id,
                "execution_failure",
                &error,
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
                &evidence,
            );
            let handle = if operation.agent_run_id.is_empty() {
                operation.operation_id.as_str()
            } else {
                operation.agent_run_id.as_str()
            };
            super::messaging::mark_open_steers_processed(
                &operation.project_path,
                handle,
                &evidence,
            );
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
    let assistant = messages
        .messages
        .iter()
        .rev()
        .find(|message| message.role.eq_ignore_ascii_case("assistant"))?;
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
