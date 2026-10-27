//! The parts of Claude Code's session file beyond messages: the task list, `system` lines (turn length, compaction,
//! API errors), session notes (pull request link, title), the context window and what a tool call touches.
//!
//! Shapes were captured from Claude Code 2.1.311 runs (`claude -p`): `TodoWrite`, `TaskCreate`, `TaskUpdate`, `Write`,
//! `Edit` and `Agent` calls and their results. Lines marked "shape from docs/guess" could not be produced in a short
//! run (they only appear in long interactive sessions) and are read defensively: a field that is not there gives
//! nothing, never a panic.

use serde_json::Value;

use super::session_file::Line;
use crate::session_chat::{cut_to, ChatItem, PlanItem, PlanStatus, ToolDetail, TurnOutcome};

/// Tools that change the files they name.
const WRITE_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Tools that start a helper agent (shown as a helper thread). `Task` is the older name of `Agent`.
const HELPER_TOOLS: [&str; 2] = ["Agent", "Task"];

/// Tools whose `path` is a folder to look in, not a file they touch.
const FOLDER_TOOLS: [&str; 3] = ["Glob", "Grep", "LS"];

/// The window of a model that is not marked `[1m]`.
const DEFAULT_WINDOW: u64 = 200_000;
const LONG_WINDOW: u64 = 1_000_000;

/// Model ids (prefixes) whose default window is 1M. Empty: no model is known to start with one. A session started
/// with the 1M option names its model with a `[1m]` suffix (`requestedModel` on the line), which is read below.
const LONG_WINDOW_MODELS: [&str; 0] = [];

/// Longest notice text, in characters.
const NOTICE_CHARS: usize = 300;

/// The context window of the model that wrote a line, in tokens. The session file does not store it, so it is
/// worked out: `[1m]` in the model name or a model of `LONG_WINDOW_MODELS` gives 1M; a call that already read more
/// than 200k tokens must have had 1M; anything else gets 200k.
pub(super) fn context_window(names: &[Option<&str>], input_tokens: u64) -> u64 {
    let long = names.iter().flatten().any(|name| {
        let name = name.to_ascii_lowercase();
        name.contains("[1m]") || LONG_WINDOW_MODELS.iter().any(|known| name.starts_with(known))
    });
    if long || input_tokens > DEFAULT_WINDOW {
        LONG_WINDOW
    } else {
        DEFAULT_WINDOW
    }
}

/// What a tool call touches: its bounded input, its files, and whether it writes them or starts a helper.
pub(super) fn tool_detail(name: &str, input: Option<&Value>) -> ToolDetail {
    let mut detail = ToolDetail::from_input(input);
    if FOLDER_TOOLS.contains(&name) {
        detail.paths.clear();
    }
    detail.writes = WRITE_TOOLS.contains(&name) && !detail.paths.is_empty();
    detail.helper = HELPER_TOOLS.contains(&name);
    detail
}

/// A call that updates the task list gives a `Plan` item instead of a tool row; true when `name` was one.
/// - `TodoWrite` (older, and the non-interactive default): `{todos: [{content, status, activeForm}]}` is the whole list.
/// - `TaskUpdate`: `{taskId, status?, subject?}` changes one task. `TaskCreate` has no id in its input (the id is in
///   the result, see `plan_result`), so its call gives nothing.
pub(super) fn plan_call(name: &str, input: Option<&Value>, out: &mut Line) -> bool {
    match name {
        "TodoWrite" => {
            let items = input
                .and_then(|input| input.get("todos"))
                .and_then(Value::as_array)
                .map(|todos| todos.iter().filter_map(todo_item).collect())
                .unwrap_or_default();
            out.push(|id, at| ChatItem::Plan { id, at, items, replace: true });
            true
        }
        "TaskCreate" => true,
        "TaskUpdate" => {
            let task_id = input.and_then(|input| input.get("taskId")).and_then(id_text);
            let status = input.and_then(|input| input.get("status")).and_then(Value::as_str).and_then(PlanStatus::parse);
            let text = input.and_then(|input| input.get("subject")).and_then(Value::as_str).unwrap_or("").trim().to_string();
            if task_id.is_some() && (status.is_some() || !text.is_empty()) {
                let items = vec![PlanItem { text, status, task_id }];
                out.push(|id, at| ChatItem::Plan { id, at, items, replace: false });
            }
            true
        }
        _ => false,
    }
}

fn todo_item(todo: &Value) -> Option<PlanItem> {
    let text = todo.get("content").and_then(Value::as_str)?.trim().to_string();
    let status = todo.get("status").and_then(Value::as_str).and_then(PlanStatus::parse).unwrap_or(PlanStatus::Pending);
    Some(PlanItem { text, status: Some(status), task_id: None })
}

/// A task id as text: the file stores it as a string (`"1"`); a number is accepted too.
fn id_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// The user line that carries a tool result of a task-list tool. Returns true when the result belongs to one (the
/// result row is then left out, since the call gave no tool row). A created task shows up here, with its id:
/// `toolUseResult: {task: {id, subject}}` (text fallback: `Task #1 created successfully: <subject>`).
pub(super) fn plan_result(line: &Value, blocks: &[Value], out: &mut Line) -> bool {
    let result = line.get("toolUseResult");
    // A created task has only `id` and `subject`; a task with a `status` is a lookup (`TaskGet`), not a creation.
    let created = result.and_then(|result| result.get("task")).filter(|task| task.is_object() && task.get("status").is_none());
    if let Some(task) = created {
        let task_id = task.get("id").and_then(id_text);
        let text = task.get("subject").and_then(Value::as_str).unwrap_or("").trim().to_string();
        push_created(task_id, text, out);
        return true;
    }
    // Updates (`{success, taskId, updatedFields}`) were already sent by the call; whole lists (`newTodos`) too.
    if result.is_some_and(|result| result.get("newTodos").is_some() || result.get("updatedFields").is_some()) {
        return true;
    }
    // Fallback when `toolUseResult` is missing: the result text of a created task.
    for block in blocks {
        let text = match block.get("content") {
            Some(Value::String(text)) => text.as_str(),
            _ => continue,
        };
        if let Some((task_id, subject)) = created_text(text) {
            push_created(Some(task_id), subject, out);
            return true;
        }
    }
    false
}

fn push_created(task_id: Option<String>, text: String, out: &mut Line) {
    if task_id.is_none() {
        return;
    }
    let items = vec![PlanItem { text, status: Some(PlanStatus::Pending), task_id }];
    out.push(|id, at| ChatItem::Plan { id, at, items, replace: false });
}

/// `Task #3 created successfully: Write tests` gives `("3", "Write tests")`.
fn created_text(text: &str) -> Option<(String, String)> {
    let rest = text.trim().strip_prefix("Task #")?;
    let (id, rest) = rest.split_once(' ')?;
    let subject = rest.strip_prefix("created successfully:")?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| (id.to_string(), subject.trim().to_string()))
}

/// A `system` line. `turn_duration` (`durationMs`, `messageCount`) is the real length of a turn, written when it
/// ends (interactive sessions only). The other subtypes are session notes.
pub(super) fn system_line(value: &Value, out: &mut Line) {
    match value.get("subtype").and_then(Value::as_str) {
        Some("turn_duration") => {
            let duration_ms = value.get("durationMs").and_then(Value::as_u64);
            out.push(|id, at| ChatItem::TurnEnd { id, at, outcome: TurnOutcome::Completed, duration_ms });
        }
        // shape from docs/guess: {content, compactMetadata: {trigger: "auto"|"manual", preTokens}}
        Some("compact_boundary") => {
            let metadata = value.get("compactMetadata").or_else(|| value.get("compact_metadata"));
            let before = metadata.and_then(|m| m.get("preTokens").or_else(|| m.get("pre_tokens"))).and_then(Value::as_u64);
            let text = match before {
                Some(tokens) => format!("Context compacted (was {} tokens)", with_commas(tokens)),
                None => "Context compacted".to_string(),
            };
            notice(out, "compacted", text, None);
        }
        // shape from docs/guess: {error: {status, error: {error: {message}}}, retryAttempt, maxRetries, content}
        Some("api_error") => notice(out, "error", api_error_text(value), None),
        _ => {}
    }
}

/// The lines that are session notes, not messages: a pull request link and the session title.
/// All three are shape from docs/guess: `pr-link {prNumber, prUrl, prRepository}`, `custom-title {customTitle}`,
/// `ai-title {aiTitle}`. Claude Code writes the title lines again from time to time; a view keeps the newest.
pub(super) fn note_line(kind: &str, value: &Value, out: &mut Line) {
    let text_of = |key: &str| value.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
    match kind {
        "pr-link" => {
            let url = text_of("prUrl").map(str::to_string);
            let number = value.get("prNumber").and_then(Value::as_u64);
            let text = match (number, text_of("prRepository")) {
                (Some(number), Some(repo)) => format!("Pull request #{number} in {repo}"),
                (Some(number), None) => format!("Pull request #{number}"),
                _ if url.is_some() => "Pull request".to_string(),
                _ => return,
            };
            notice(out, "pr", text, url);
        }
        "custom-title" | "ai-title" => {
            if let Some(title) = text_of("customTitle").or_else(|| text_of("aiTitle")).or_else(|| text_of("title")) {
                notice(out, "title", cut_to(title, NOTICE_CHARS), None);
            }
        }
        _ => {}
    }
}

fn notice(out: &mut Line, kind: &str, text: String, url: Option<String>) {
    let kind = kind.to_string();
    out.push(|id, at| ChatItem::Notice { id, at, notice_kind: kind, text, url });
}

/// One line for an API error: the status and message found in `error` (searched a few levels down), and the retry
/// count when the line has one. Falls back to `content`, then to a plain sentence.
fn api_error_text(value: &Value) -> String {
    let error = value.get("error");
    let status = error.and_then(|e| e.get("status")).and_then(Value::as_u64);
    let message = error
        .and_then(|e| find_message(e, 4))
        .or_else(|| value.get("content").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    let mut text = match (status, message.trim()) {
        (Some(status), "") => format!("API error {status}"),
        (Some(status), message) => format!("API error {status}: {message}"),
        (None, "") => "API error".to_string(),
        (None, message) => format!("API error: {message}"),
    };
    let attempt = value.get("retryAttempt").and_then(Value::as_u64);
    let max = value.get("maxRetries").and_then(Value::as_u64);
    if let (Some(attempt), Some(max)) = (attempt, max) {
        text.push_str(&format!(" (retry {attempt} of {max})"));
    }
    cut_to(&text, NOTICE_CHARS)
}

/// The first `message` string in `value`, looking at most `depth` levels down.
fn find_message(value: &Value, depth: usize) -> Option<String> {
    if let Some(message) = value.get("message").and_then(Value::as_str).filter(|m| !m.trim().is_empty()) {
        return Some(message.to_string());
    }
    if depth == 0 {
        return None;
    }
    value.as_object()?.values().find_map(|child| find_message(child, depth - 1))
}

/// 167000 gives `167,000`.
fn with_commas(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
#[path = "session_extra_tests.rs"]
mod tests;
