//! `ask_user`: block until a person answers, in short bridge long-polls (like `wait_agents`).
//!
//! The first poll creates the question on the app side; later polls carry its `question_id`, so a
//! bridge hiccup between polls loses nothing. If the agent aborts the tool call, the question is
//! cancelled so it leaves the Inbox.

use super::*;
use std::sync::mpsc::{Receiver, Sender};

/// One bridge slice; short so cancellation and progress notifications stay responsive.
const POLL_MS: u64 = 4_000;
const DEFAULT_TIMEOUT_S: u64 = 600;
const MAX_TIMEOUT_S: u64 = 3_600;
const REQUEST_CANCELLED: &str = r#"{"code":"REQUEST_CANCELLED","message":"ask_user was cancelled","recoverable":true,"context":{}}"#;

fn final_result(resolved: &Value) -> Value {
    let mut out = json!({ "answered": resolved.get("answered").and_then(Value::as_bool).unwrap_or(false) });
    for key in ["choice_id", "text", "answered_by", "reason", "question_id"] {
        if let Some(value) = resolved.get(key) {
            out[key] = value.clone();
        }
    }
    out
}

/// Returns the tool's JSON text. `notifications` receives `notifications/progress` messages.
pub(crate) fn run(
    args: &Value,
    cancel: Option<&Receiver<()>>,
    token: Option<&Value>,
    notifications: Option<&Sender<String>>,
) -> Result<String, String> {
    required_string(args, "question")?;
    let timeout_s = optional_number(args, "timeout_s").unwrap_or(DEFAULT_TIMEOUT_S).clamp(1, MAX_TIMEOUT_S);
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_s);
    let mut create = args.clone();
    create["timeout_s"] = json!(timeout_s);
    let mut question_id: Option<String> = None;
    let mut polls = 0_u64;
    loop {
        if cancel.is_some_and(|rx| rx.try_recv().is_ok()) {
            if let Some(id) = &question_id {
                let _ = bridge_request("POST", "/questions/ask", Some(json!({ "question_id": id, "cancel": true })));
            }
            return Err(REQUEST_CANCELLED.into());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as u64;
        if remaining == 0 && question_id.is_some() {
            // The app enforces the same deadline; this covers a bridge that went away for good.
            let id = question_id.unwrap_or_default();
            let _ = bridge_request("POST", "/questions/ask", Some(json!({ "question_id": id, "cancel": true })));
            return Ok(json!({ "answered": false, "reason": "timeout", "question_id": id }).to_string());
        }
        let body = match &question_id {
            Some(id) => json!({ "question_id": id, "wait_ms": remaining.clamp(1, POLL_MS) }),
            None => {
                let mut body = create.clone();
                body["wait_ms"] = json!(POLL_MS);
                body
            }
        };
        let response = match bridge_request("POST", "/questions/ask", Some(body)) {
            Ok(text) => text,
            Err(error) if question_id.is_some() && error.starts_with("bridge_down") => {
                // The app is reconnecting; keep the question and try again.
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            Err(error) if question_id.is_some() && error.contains("QUESTION_NOT_FOUND") => {
                // The app restarted: questions are not persisted.
                let id = question_id.unwrap_or_default();
                return Ok(json!({ "answered": false, "reason": "cancelled", "question_id": id,
                    "detail": "the question was lost when the app restarted" })
                .to_string());
            }
            Err(error) => return Err(error),
        };
        let value: Value = serde_json::from_str(&response).map_err(|e| format!("invalid ask_user response: {e}"))?;
        if question_id.is_none() {
            question_id = value.get("question_id").and_then(Value::as_str).map(str::to_string);
        }
        if value.get("resolved").and_then(Value::as_bool) == Some(true) {
            return Ok(final_result(&value).to_string());
        }
        polls += 1;
        if let (Some(sender), Some(token)) = (notifications, token) {
            let _ = sender.send(
                json!({"jsonrpc":"2.0","method":"notifications/progress",
                    "params":{"progressToken":token,"progress":polls,"message":"waiting for the user"}})
                .to_string(),
            );
        }
    }
}

/// `tools/call` result wrapping, matching `call_tool_cancellable`.
pub(crate) fn wrap(text: String) -> Value {
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if let Ok(structured) = serde_json::from_str::<Value>(&text) {
        result["structuredContent"] = structured;
    }
    result
}
