//! Safety rails around `answer_prompt` for an agent-run handle. Everything that can refuse an
//! answer (which project the handle lives in, who owns it, whether this prompt was already
//! answered) is decided here, before a key is sent to the pane. The receipt it hands back is
//! truthful about what happened after the key went out.

use super::runtime::{self, AgentRunView};
use crate::operations::{self, OperationError, OperationSnapshot};
use serde_json::{json, Value};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// How long after a delivered answer the same prompt id still counts as that answer even if the
/// pane has not redrawn yet (a retry in that window must not press the key a second time).
const REPLAY_WINDOW_MS: u64 = 5_000;
/// Receipts kept per process; the oldest are dropped past this.
const LEDGER_CAP: usize = 512;

#[derive(Clone)]
struct Applied {
    choice: String,
    at_ms: u64,
}

type Ledger = HashMap<(String, String), Applied>;

fn ledger() -> &'static Mutex<Ledger> {
    static LEDGER: OnceLock<Mutex<Ledger>> = OnceLock::new();
    LEDGER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Answers are checked against the ledger and then sent as a keystroke; two concurrent answers
/// for one prompt must not both pass the check before either has recorded itself, or the key is
/// pressed twice (the TUI then receives a stray character). Hold this across check, send, record.
pub(crate) fn serialize_answers() -> parking_lot::MutexGuard<'static, ()> {
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| Mutex::new(())).lock()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Find the run `handle` names and check this connection may answer for it. Nothing here touches
/// the pane, so every refusal happens before a key is sent.
///
/// * explicit `project_path`: the handle must live there, otherwise `PROJECT_MISMATCH`;
/// * omitted: the bridge's selected project wins when it owns the handle, else the single
///   indexed project that does; several owners is `AMBIGUOUS_HANDLE`.
pub(super) fn resolve_target(
    default_project: &str,
    explicit_project: Option<&str>,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    let explicit = explicit_project.filter(|path| !path.trim().is_empty());
    let search_from = explicit.unwrap_or(default_project);
    let chosen = match operations::latest_agent_run(search_from, handle) {
        Ok(snapshot) => snapshot,
        Err(not_here) => {
            let mut elsewhere = operations::agent_runs_by_project(search_from, handle)?;
            match (explicit, elsewhere.len()) {
                (_, 0) => return Err(not_here),
                (None, 1) => elsewhere.remove(0),
                (None, _) => {
                    let projects: Vec<&str> =
                        elsewhere.iter().map(|s| s.project_path.as_str()).collect();
                    let mut error = OperationError::new(
                        "AMBIGUOUS_HANDLE",
                        format!(
                            "handle `{handle}` exists in {} projects; nothing was sent. Pass project_path to say which one",
                            projects.len()
                        ),
                        false,
                    );
                    error.context =
                        json!({"handle": handle, "projects": projects, "sent_input": false});
                    return Err(error);
                }
                (Some(asked), _) => {
                    // Ownership first, so a caller without access learns nothing about the run.
                    runtime::check_snapshot_mutate_access(&elsewhere[0], handle, session_id)?;
                    let mut error = OperationError::new(
                        "PROJECT_MISMATCH",
                        format!(
                            "handle `{handle}` belongs to project {}, not {asked}; nothing was sent",
                            elsewhere[0].project_path
                        ),
                        false,
                    );
                    error.context = json!({
                        "handle": handle,
                        "requested_project": asked,
                        "handle_project": elsewhere[0].project_path,
                        "sent_input": false,
                    });
                    return Err(error);
                }
            }
        }
    };
    runtime::check_snapshot_mutate_access(&chosen, handle, session_id)?;
    Ok(chosen)
}

fn answer_block(
    snapshot: &OperationSnapshot,
    prompt_id: &str,
    choice: &str,
    already_applied: bool,
    at_ms: u64,
) -> Value {
    json!({
        "applied": true,
        "already_applied": already_applied,
        "prompt_id": prompt_id,
        "choice": choice,
        "handle": snapshot.agent_run_id,
        "operation_id": snapshot.operation_id,
        "answered_at_ms": at_ms,
    })
}

fn view_with_answer(snapshot: &OperationSnapshot, answer: Value) -> Value {
    let mut value = serde_json::to_value(AgentRunView::from(snapshot.clone()))
        .unwrap_or_else(|_| json!({"handle": snapshot.agent_run_id}));
    if let Some(object) = value.as_object_mut() {
        object.insert("answer".into(), answer);
    }
    value
}

/// The receipt of an answer already delivered for this very prompt and choice, or `None` when
/// the key should be pressed. A retry is recognised when the prompt is gone from the screen, or
/// when it is still drawn within `REPLAY_WINDOW_MS` of the press (not yet redrawn). The same
/// command prompting again later carries the same id, so an old receipt alone never blocks it.
pub(super) fn replay_receipt(
    snapshot: &OperationSnapshot,
    prompt_id: &str,
    choice: &str,
    prompt_still_pending: bool,
) -> Option<Value> {
    let applied = ledger()
        .lock()
        .get(&(snapshot.operation_id.clone(), prompt_id.to_string()))
        .cloned()?;
    if applied.choice != choice {
        return None;
    }
    if prompt_still_pending && now_ms().saturating_sub(applied.at_ms) > REPLAY_WINDOW_MS {
        return None;
    }
    let answer = answer_block(snapshot, prompt_id, choice, true, applied.at_ms);
    Some(view_with_answer(snapshot, answer))
}

/// The receipt for an answer that repeats one already delivered for this prompt occurrence by any
/// path (`pane_prompt::answers::answer_once`): nothing was sent this time.
pub(super) fn repeat_receipt(snapshot: &OperationSnapshot, prompt_id: &str, choice: &str, at_ms: u64) -> Value {
    view_with_answer(snapshot, answer_block(snapshot, prompt_id, choice, true, at_ms))
}

/// Remember that `choice` was delivered for this prompt. Call right after the key is sent and
/// before any bookkeeping that could fail.
pub(super) fn record_applied(snapshot: &OperationSnapshot, prompt_id: &str, choice: &str) {
    let mut ledger = ledger().lock();
    if ledger.len() >= LEDGER_CAP {
        if let Some(oldest) = ledger
            .iter()
            .min_by_key(|(_, applied)| applied.at_ms)
            .map(|(key, _)| key.clone())
        {
            ledger.remove(&oldest);
        }
    }
    ledger.insert(
        (snapshot.operation_id.clone(), prompt_id.to_string()),
        Applied {
            choice: choice.to_string(),
            at_ms: now_ms(),
        },
    );
}

/// After the key went out: record it, then run the bookkeeping (`observe`). A bookkeeping
/// failure cannot undo the keystroke, so it becomes a warning on an applied receipt, never an
/// error.
pub(super) fn finish(
    snapshot: &OperationSnapshot,
    prompt_id: &str,
    choice: &str,
    observe: impl FnOnce(&OperationSnapshot) -> Result<OperationSnapshot, OperationError>,
) -> Value {
    record_applied(snapshot, prompt_id, choice);
    let at_ms = now_ms();
    match observe(snapshot) {
        Ok(updated) => {
            let answer = answer_block(&updated, prompt_id, choice, false, at_ms);
            view_with_answer(&updated, answer)
        }
        Err(error) => {
            let mut answer = answer_block(snapshot, prompt_id, choice, false, at_ms);
            answer["warning"] = Value::String(format!(
                "the answer was delivered to the pane, but updating the run state failed ({}: {}); do not answer again, re-check with inspect_agent",
                error.code, error.message
            ));
            view_with_answer(snapshot, answer)
        }
    }
}

#[cfg(test)]
pub(super) fn age_receipts_for_test(snapshot: &OperationSnapshot, prompt_id: &str) {
    if let Some(applied) = ledger()
        .lock()
        .get_mut(&(snapshot.operation_id.clone(), prompt_id.to_string()))
    {
        applied.at_ms = 0;
    }
}

#[cfg(test)]
mod tests;
