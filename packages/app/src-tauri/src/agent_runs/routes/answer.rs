//! `POST /agents/answer`: routes an answer to a run's blocking prompt (or a bare pane), after
//! every refusable check and before the key is sent.

use crate::agent_runs::runtime::{self};
use crate::operations::{self, OperationError, OperationStatus, StateSource};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

use super::handles::run_for_pane;
use super::wire::{query_value, AnswerPromptRequest};

/// The desktop UI answering the prompt on a pane: the same path as `POST /agents/answer` with a
/// bare `pane_id` (stale and replay guards included). Broad approvals are never opted into.
pub(crate) fn answer_for_pane(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    prompt_id: &str,
    choice: &str,
) -> Result<Value, OperationError> {
    route_answer(
        AnswerPromptRequest {
            project_path: None,
            handle: None,
            pane_id: Some(pane_id.to_string()),
            prompt_id: prompt_id.to_string(),
            choice: choice.to_string(),
            allow_broad: false,
        },
        "",
        None,
        registry,
        app,
    )
}

/// Answers by run handle, or by bare pane id (routed through the owning run when one exists). A
/// delivered answer is also noted on the pane's open turn (turn summaries: "you approved 1 command").
pub(super) fn route_answer(
    request: AnswerPromptRequest,
    query: &str,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
) -> Result<Value, OperationError> {
    let pane_id = request.pane_id.clone().filter(|id| !id.is_empty());
    let shown = pane_id.as_deref().and_then(crate::turn_summary::prompt_before_answer);
    let (prompt_id, choice) = (request.prompt_id.clone(), request.choice.clone());
    let result = route_answer_inner(request, query, session_id, registry, app);
    if let Ok(value) = &result {
        let pane = pane_id.or_else(|| value.get("pane_id").and_then(Value::as_str).map(str::to_string));
        if let Some(pane) = pane {
            crate::agent_authority::gate::note_answer(session_id, &pane, &prompt_id, &choice, Some(registry));
            crate::turn_summary::record_answer(&pane, &prompt_id, &choice, shown);
        }
    }
    result
}

fn route_answer_inner(
    request: AnswerPromptRequest,
    query: &str,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
) -> Result<Value, OperationError> {
    if let Some(pane_id) = request.pane_id.as_deref().filter(|id| !id.is_empty()) {
        crate::checkpoints::before_answer(pane_id);
    }
    if request.handle.is_none() {
        let pane_id = request.pane_id.as_deref().filter(|id| !id.is_empty()).ok_or_else(|| {
                    OperationError::new(
                        "INVALID_ARGUMENT",
                        "answer_prompt needs `handle` (agent run) or `pane_id` (full pane id from wait_for_panes)",
                        false,
                    )
                })?;
        // A pane that belongs to a run is answered through that run, so a repeated answer
        // finds the same replay ledger entry instead of pressing the key twice.
        let (default_project, cwd) = {
            let guard = registry.lock();
            (
                guard.project_path.clone(),
                guard.panes.get(pane_id).map(|pane| pane.info.cwd.clone()),
            )
        };
        let candidates: Vec<String> = [Some(default_project), cwd].into_iter().flatten().collect();
        if let Some((project, handle)) = run_for_pane(&candidates, pane_id) {
            let routed = AnswerPromptRequest {
                project_path: Some(project.clone()),
                handle: Some(handle),
                pane_id: request.pane_id.clone(),
                prompt_id: request.prompt_id.clone(),
                choice: request.choice.clone(),
                allow_broad: request.allow_broad,
            };
            match answer_prompt(&project, Some(&project), &routed, session_id, registry, app) {
                // Pane control was already authorised for this caller; run ownership
                // is not required to answer a prompt on a pane it has taken over.
                Err(error) if error.code == "AUTHORIZATION_DENIED" => {}
                other => return other,
            }
        }
        // Setting A: an agent answering above low risk in a pane it did not start needs a person's OK.
        if session_id.is_some() {
            if let Some(prompt) = crate::pane_prompt::detect(registry, pane_id, false) {
                crate::agent_authority::gate::check_answer(
                    session_id,
                    pane_id,
                    &request.prompt_id,
                    prompt.pointer("/risk/level").and_then(Value::as_str),
                    prompt.pointer("/risk/headline").and_then(Value::as_str),
                    Some(registry),
                )?;
            }
        }
        return crate::pane_prompt::answer(
            registry,
            app,
            pane_id,
            &request.prompt_id,
            &request.choice,
            request.allow_broad,
        );
    }
    let explicit = request
        .project_path
        .clone()
        .filter(|path| !path.trim().is_empty())
        .or_else(|| query_value(query, "project_path"));
    let default_project = registry.lock().project_path.clone();
    answer_prompt(
        &default_project,
        explicit.as_deref(),
        &request,
        session_id,
        registry,
        app,
    )
}

/// Answer the prompt blocking an agent run. Order matters: everything that can refuse (project,
/// ownership, broad opt-in, stale or already-answered prompt) happens before the key is sent;
/// after it is sent nothing can turn the answer into an error (see `answer_guard::finish`).
fn answer_prompt(
    default_project: &str,
    explicit_project: Option<&str>,
    request: &AnswerPromptRequest,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
) -> Result<Value, OperationError> {
    let handle = request.handle.as_deref().unwrap_or_default();
    let snapshot = crate::agent_runs::answer_guard::resolve_target(
        default_project,
        explicit_project,
        handle,
        session_id,
    )?;
    let _serial = crate::agent_runs::answer_guard::serialize_answers();
    // The run's own project, not whichever project the request or the bridge defaulted to.
    let project = snapshot.project_path.as_str();
    let choice_key = match request.choice.as_str() {
        "once" => "allow_once",
        "reject" => "deny",
        "always" | "allow_broad" => "allow_all",
        other => other,
    }
    .to_string();
    // The stored run state can lag the pane: a caller sees the prompt in a live view before the
    // dispatch thread has recorded it. Fall back to the dialog actually on screen, which is
    // checked against the same prompt id below, so only the prompt the caller was shown can be
    // answered.
    let stored = snapshot
        .required_action
        .as_ref()
        .filter(|_| snapshot.status == OperationStatus::WaitingInput);
    let live_action = match stored {
        Some(_) => None,
        None => snapshot
            .pane_id
            .as_deref()
            .and_then(|pane| crate::pane_prompt::required_action(registry, pane)),
    };
    let action = match (stored, live_action.as_ref()) {
        (Some(action), _) | (None, Some(action)) => action,
        (None, None) => {
            // A retry of an answer that already went out finds the prompt gone: hand back the
            // receipt instead of failing (and never press again).
            if let Some(receipt) = crate::agent_runs::answer_guard::replay_receipt(
                &snapshot,
                &request.prompt_id,
                &choice_key,
                false,
            ) {
                return Ok(receipt);
            }
            // Say what actually happened: a prompt that is already gone usually means the run
            // moved on (someone answered it, or the TUI accepted it), not that nothing changed.
            let status = format!("{:?}", snapshot.status).to_ascii_lowercase();
            let message = if snapshot.status == OperationStatus::WaitingInput {
                "the agent is waiting, but not on a prompt Puppet Master recognises; read the pane"
                    .to_string()
            } else {
                format!(
                    "no prompt is pending: it was already resolved and the run continued (status {status}); nothing was sent"
                )
            };
            let mut error = OperationError::new("PROMPT_NOT_PENDING", message, false);
            error.context = json!({
                "run_status": status,
                "stage": snapshot.stage,
                "already_resolved": snapshot.status != OperationStatus::WaitingInput,
                "sent_input": false,
            });
            return Err(error);
        }
    };
    // A usage limit is not a prompt (`handoff`): nothing is typed for it, whatever the caller sends.
    if let Some(error) = crate::handoff::refuse_answer(action) {
        return Err(error);
    }
    let matches_prompt = ["prompt_id", "request_id", "message_id", "id"]
        .iter()
        .filter_map(|key| action.get(*key).and_then(Value::as_str))
        .any(|id| id == request.prompt_id)
        || action
            .get("permission_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&request.prompt_id)));
    if let Some(receipt) = crate::agent_runs::answer_guard::replay_receipt(
        &snapshot,
        &request.prompt_id,
        &choice_key,
        matches_prompt,
    ) {
        return Ok(receipt);
    }
    if !matches_prompt {
        return Err(OperationError::new(
            "STALE_PROMPT",
            "prompt id is no longer current",
            false,
        ));
    }
    if matches!(
        request.choice.as_str(),
        "allow_all" | "allow_broad" | "always"
    ) && !request.allow_broad
    {
        return Err(OperationError::new(
            "BROAD_APPROVAL_REQUIRES_OPT_IN",
            "allow_all requires allow_broad=true",
            false,
        ));
    }
    if snapshot.pane_id.is_none()
        && request.choice == "trust_workspace"
        && action.get("kind").and_then(Value::as_str) == Some("workspace_trust")
        && crate::agent_runs::headless::blocked_on_trust(&snapshot)
    {
        // A headless run parked on the trust screen has no worker to answer: re-run the same
        // task on this handle with --trust, which is what the caller just asked for.
        let view = runtime::send_agent(
            project,
            handle,
            snapshot.task.clone(),
            session_id.map(str::to_string),
            registry.clone(),
            app.clone(),
            crate::agent_runs::persist::FollowupOpts {
                trust_workspace: true,
                ..Default::default()
            },
        )?;
        return serde_json::to_value(view)
            .map_err(|error| OperationError::new("SERIALIZATION_ERROR", error.to_string(), false));
    }
    if let Some(pane) = snapshot.pane_id.as_deref() {
        // Setting A: an agent answering above low risk in a pane it did not start needs a person's OK.
        crate::agent_authority::gate::check_answer(
            session_id,
            pane,
            &request.prompt_id,
            action.pointer("/risk/level").and_then(Value::as_str),
            action.pointer("/risk/headline").and_then(Value::as_str),
            Some(registry),
        )?;
    }
    let pane_id = snapshot.pane_id.as_deref().ok_or_else(|| {
        OperationError::new(
            "PROMPT_UNAVAILABLE",
            "headless worker does not accept interactive prompt replies; for workspace_trust, answer with choice=trust_workspace, or trust the folder in an interactive agent yourself and retry with send_agent",
            false,
        )
    })?;
    let kind = action
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if snapshot.agent_type == "opencode_native" && kind == "permission_required" {
        let choice = match request.choice.as_str() {
            "allow_once" | "once" => "once",
            "deny" | "reject" => "reject",
            "allow_broad" | "always" if request.allow_broad => "always",
            _ => return Err(OperationError::new("INVALID_PROMPT_CHOICE", "permission choice must be allow_once or deny; allow_broad requires allow_broad=true", false)),
        };
        if choice == "always" && !request.allow_broad {
            return Err(OperationError::new(
                "INVALID_PROMPT_CHOICE",
                "broad permission requires allow_broad=true",
                false,
            ));
        }
        crate::opencode::reply_pane_permission(registry, pane_id, &request.prompt_id, choice)
            .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
    } else if snapshot.agent_type == "opencode_native" && kind == "question_required" {
        let question = crate::opencode::messages::pending_question_for_pane(registry, pane_id)
            .ok_or_else(|| {
                OperationError::new(
                    "STALE_PROMPT",
                    "native question is no longer pending",
                    false,
                )
            })?;
        let allowed = question
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>();
        if !allowed.is_empty() && !allowed.contains(&request.choice.as_str()) {
            return Err(OperationError::new(
                "INVALID_PROMPT_CHOICE",
                "answer must match one of the current explicit options",
                false,
            ));
        }
        crate::opencode::reply_pane_question(
            registry,
            pane_id,
            &request.choice,
            Some(&request.prompt_id),
        )
        .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
    } else {
        let screen = {
            let locked = registry.lock();
            let pane = locked.panes.get(pane_id).ok_or_else(|| {
                OperationError::new("PANE_NOT_FOUND", "agent pane is no longer available", false)
            })?;
            let contents = pane.screen.lock().screen().contents();
            contents
        };
        let adapter = crate::agent_adapters::adapter_for(&snapshot.agent_type);
        // The id handed to callers comes from `pane_prompt` (stable per command), so the live
        // prompt must be re-identified the same way. Hashing the whole screen here made every
        // answer to a command approval STALE_PROMPT.
        let prompt = crate::pane_prompt::detect_in_screen(
            &snapshot.agent_type,
            &screen,
            request.allow_broad,
        )
        .ok_or_else(|| {
            OperationError::new(
                "STALE_PROMPT",
                "current screen no longer contains a supported prompt",
                false,
            )
        })?;
        if prompt.get("prompt_id").and_then(Value::as_str) != Some(request.prompt_id.as_str()) {
            let mut error = OperationError::new(
                "STALE_PROMPT",
                "the prompt on screen is a different one; review it and answer with its id",
                false,
            );
            error.context = json!({
                "current_prompt": {
                    "prompt_id": prompt.get("prompt_id"),
                    "kind": prompt.get("kind"),
                    "command": prompt.get("command"),
                    "cwd": prompt.get("cwd"),
                }
            });
            return Err(error);
        }
        let choice = match request.choice.as_str() {
            "once" => "allow_once",
            "reject" => "deny",
            "always" | "allow_broad" if request.allow_broad => "allow_all",
            choice => choice,
        };
        let reply = adapter
            .prompt_reply(&prompt, choice, request.allow_broad)
            .map_err(|error| OperationError::new("INVALID_PROMPT_CHOICE", error, false))?;
        let text = reply.key.or(reply.text).ok_or_else(|| {
            OperationError::new(
                "INVALID_PROMPT_CHOICE",
                "adapter did not produce a reply",
                false,
            )
        })?;
        crate::pty::registry::write_input(registry, app, pane_id, &text, true, false, None, crate::pty::WriteOrigin::Answer)
            .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))?;
        // A "deny" on a startup gate (folder trust) is a final decline of the run waiting on it.
        crate::pane_prompt::note_gate_answer(pane_id, &prompt, choice);
    }
    // The key is out. From here a failure is a warning on an applied receipt, not an error.
    Ok(crate::agent_runs::answer_guard::finish(
        &snapshot,
        &request.prompt_id,
        &choice_key,
        |current| {
            let updated = operations::mark_operation_observation(
                &current.project_path,
                &current.operation_id,
                Some("running".into()),
                None,
                StateSource::Native,
            )?;
            crate::bridge::publish_operation(&updated, registry, app)?;
            Ok(updated)
        },
    ))
}
