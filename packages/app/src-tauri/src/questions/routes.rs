//! Bridge routes for questions. Free of `AppHandle` so they can be driven in-process by tests
//! and by the MCP shim test; the router passes a `notify` closure that publishes the change.
//!
//! - `POST /questions/ask`           the `ask_user` MCP tool (create or keep waiting; see below)
//! - `GET  /questions`               pending questions (desktop / paired phone)
//! - `POST /questions/{id}/answer`   `{choice_id}` or `{text}`
//! - `POST /questions/{id}/dismiss`
//!
//! Only `/questions/ask` is in the tool catalog, so MCP sessions cannot reach the other three
//! (`mcp_sessions::authorize` refuses routes that are not a catalog tool): an agent cannot
//! answer its own question. A paired phone passes the normal bearer-token gate in `router.rs`.

use super::{now_ms, store, Choice, NewQuestion, Question, QuestionError};
use serde_json::{json, Value};
use std::time::Duration;

pub type RouteReply = Result<(u16, Value), (u16, Value)>;

/// Longest single block inside `/questions/ask`; the shim loops, like `wait_agents`.
const MAX_ASK_WAIT_MS: u64 = 25_000;

fn error(e: QuestionError) -> (u16, Value) {
    (e.status(), json!({ "code": e.code, "error": e.message, "recoverable": false }))
}

fn string_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Handles a questions route; `None` when the request is not one. `via_phone` marks a request
/// that came through the paired-device gate (recorded as `answered_by: "phone"`).
pub fn handle(
    method: &str,
    segments: &[&str],
    body: &Value,
    session: Option<&str>,
    via_phone: bool,
    notify: &dyn Fn(),
) -> Option<RouteReply> {
    let before = store().revision();
    let reply = match (method, segments) {
        ("GET", ["questions"]) => Some(Ok((
            200,
            json!({ "questions": store().list_pending(now_ms()), "revision": store().revision() }),
        ))),
        ("POST", ["questions", "ask"]) => Some(ask(body, session)),
        ("POST", ["questions", id, "answer"]) => Some(answer(id, body, via_phone)),
        ("POST", ["questions", id, "dismiss"]) => Some(
            store()
                .dismiss(id, now_ms())
                .map(|r| (200, serde_json::to_value(r).unwrap()))
                .map_err(error),
        ),
        _ => None,
    };
    if reply.is_some() && store().revision() != before {
        notify();
    }
    reply
}

fn answer(id: &str, body: &Value, via_phone: bool) -> RouteReply {
    let by = match string_field(body, "answered_by").as_deref() {
        Some("phone") => "phone",
        Some("desktop") => "desktop",
        _ if via_phone => "phone",
        _ => "desktop",
    };
    store()
        .answer(
            id,
            body.get("choice_id").and_then(Value::as_str),
            body.get("text").and_then(Value::as_str),
            by,
            now_ms(),
        )
        .map(|r| (200, serde_json::to_value(r).unwrap()))
        .map_err(error)
}

/// `ask_user`. The first call carries the question and creates it; every call then blocks up to
/// `wait_ms` for a resolution and answers `{resolved, question_id, ...}`. The shim repeats the
/// call with `{question_id, wait_ms}` until `resolved` (so a bridge reconnect between polls
/// loses nothing) and sends `{question_id, cancel: true}` if the agent aborts the tool call.
fn ask(body: &Value, session: Option<&str>) -> RouteReply {
    let wait = Duration::from_millis(
        body.get("wait_ms").and_then(Value::as_u64).unwrap_or(MAX_ASK_WAIT_MS).min(MAX_ASK_WAIT_MS),
    );
    let id = if let Some(existing) = string_field(body, "question_id") {
        if body.get("cancel").and_then(Value::as_bool) == Some(true) {
            let resolution = store().dismiss(&existing, now_ms()).map_err(error)?;
            return Ok((200, super::tool_result(&resolution.question)));
        }
        existing
    } else {
        let choices = match body.get("choices") {
            None | Some(Value::Null) => Vec::new(),
            Some(value) => serde_json::from_value::<Vec<Choice>>(value.clone()).map_err(|e| {
                error(QuestionError {
                    code: "INVALID_ARGUMENT",
                    message: format!("choices must be [{{id,label}}]: {e}"),
                })
            })?,
        };
        let created = store()
            .create(
                NewQuestion {
                    asker: session.map(str::to_string),
                    agent: string_field(body, "agent"),
                    pane_id: string_field(body, "pane_id"),
                    question: string_field(body, "question").unwrap_or_default(),
                    choices,
                    allow_free_text: body.get("allow_free_text").and_then(Value::as_bool),
                    context: string_field(body, "context"),
                    timeout_s: body.get("timeout_s").and_then(Value::as_u64),
                    urgency: string_field(body, "urgency"),
                },
                now_ms(),
            )
            .map_err(error)?;
        crate::push::notify_question(&created);
        created.id
    };
    match store().wait_resolved(&id, wait) {
        Some(q) => Ok((200, with_question(&q))),
        None => Err(error(QuestionError {
            code: "QUESTION_NOT_FOUND",
            message: format!("no question {id}"),
        })),
    }
}

fn with_question(q: &Question) -> Value {
    super::tool_result(q)
}

/// Tell the desktop windows (`questions://changed`) and phones (SSE `questions`) to re-read.
pub fn publish_changed(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let payload = json!({ "revision": store().revision() });
    let _ = app.emit("questions://changed", payload.clone());
    crate::bridge::push_sse(format!("event: questions\ndata: {payload}\n\n"));
}
