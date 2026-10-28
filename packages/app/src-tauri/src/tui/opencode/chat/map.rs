//! OpenCode's session messages (`GET /session/{id}/message`) as chat items.
//!
//! Shapes captured from OpenCode 1.19 (`../fixtures/api_messages_turn.json`): one user message per prompt, then one
//! assistant message per model call (`finish: "tool-calls"` while the turn goes on, `"stop"` on the last). An
//! assistant message holds `step-start`, `reasoning`, `text`, `tool` (with `state.status` pending / running /
//! completed / error), `step-finish` (`tokens`) and `patch` parts. Ids are `oc:<message id>:<part index>`, so a
//! message that grows is sent again under the same ids. Read defensively: a field that is not there gives nothing,
//! never a panic. Chat text is never logged.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use crate::session_chat::{bound_input, cut, cut_to, ChatItem, PlanItem, PlanStatus, ToolDetail, TurnOutcome};

/// Tools that change the files they name.
const WRITE_TOOLS: [&str; 5] = ["edit", "write", "multiedit", "patch", "apply_patch"];
/// Tools whose `path` is a folder to look in, not a file they touch.
const FOLDER_TOOLS: [&str; 3] = ["glob", "grep", "list"];
/// Input fields that name a file, in the order they are read.
const PATH_KEYS: [&str; 3] = ["filePath", "file_path", "path"];
/// Longest one-line summary of a tool call, in characters.
const SUMMARY_CHARS: usize = 200;
/// Longest error text, in characters.
const ERROR_CHARS: usize = 300;

/// The items of a conversation. `window` gives a model's context window (provider id, model id).
pub(crate) fn map_messages(messages: &[Value], window: &dyn Fn(&str, &str) -> Option<u64>) -> Vec<ChatItem> {
    let mut out = Vec::new();
    // When each user message was sent: the length of a turn is counted from there.
    let mut sent: Vec<(&str, u64)> = Vec::new();
    for message in messages {
        let info = &message["info"];
        let Some(id) = info["id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        match info["role"].as_str() {
            Some("user") => {
                if let Some(created) = millis(&info["time"]["created"]) {
                    sent.push((id, created));
                }
                if let Some(text) = user_text(message) {
                    out.push(ChatItem::User { id: format!("oc:{id}:user"), at: iso(&info["time"]["created"]), text });
                }
            }
            Some("assistant") => assistant(message, id, &sent, window, &mut out),
            _ => {}
        }
    }
    out
}

/// What the person typed: the text parts that are theirs. `None` for a message with no visible text (OpenCode
/// itself stamps empty messages with a zero-width space to set the model).
fn user_text(message: &Value) -> Option<String> {
    let parts = message["parts"].as_array()?;
    let text: Vec<&str> = parts
        .iter()
        .filter(|part| part["type"] == "text" && part["synthetic"] != true && part["ignored"] != true)
        .filter_map(|part| part["text"].as_str())
        .collect();
    let text = text.join("\n");
    (!text.trim_matches(|c: char| c.is_whitespace() || c == '\u{200b}' || c == '\u{feff}').is_empty()).then_some(text)
}

fn assistant(message: &Value, id: &str, sent: &[(&str, u64)], window: &dyn Fn(&str, &str) -> Option<u64>, out: &mut Vec<ChatItem>) {
    let info = &message["info"];
    let model = info["modelID"].as_str().filter(|m| !m.is_empty()).map(str::to_string);
    let completed = millis(&info["time"]["completed"]);
    let failed = info.get("error").is_some_and(|error| !error.is_null());
    // The model call that ends the turn: it finished with something other than "go on with the tools".
    let finish = info["finish"].as_str();
    let ended = failed || (completed.is_some() && !matches!(finish, Some("tool-calls") | Some("unknown")));
    let mut tokens: Option<&Value> = None;
    for (index, part) in message["parts"].as_array().into_iter().flatten().enumerate() {
        match part["type"].as_str() {
            Some("text") => {
                let Some(text) = part["text"].as_str().filter(|t| !t.trim().is_empty()) else {
                    continue;
                };
                if part["ignored"] == true {
                    continue;
                }
                out.push(ChatItem::Assistant {
                    id: format!("oc:{id}:{index}"),
                    at: iso(&part["time"]["start"]).or_else(|| iso(&info["time"]["created"])),
                    text: text.to_string(),
                    model: model.clone(),
                    final_answer: ended,
                });
            }
            Some("tool") => tool(part, &format!("oc:{id}:{index}"), out),
            Some("compaction") => out.push(ChatItem::Notice {
                id: format!("oc:{id}:{index}"),
                at: iso(&info["time"]["created"]),
                notice_kind: "compacted".into(),
                text: "The conversation was compacted to free up room.".into(),
                url: None,
            }),
            Some("step-finish") => tokens = part.get("tokens").filter(|t| t.is_object()).or(tokens),
            _ => {}
        }
    }
    // A stopped turn (Esc) is the person's own doing: it ends the turn but is not an error to tell about.
    if failed && info["error"]["name"] != "MessageAbortedError" {
        let text = error_text(&info["error"]);
        out.push(ChatItem::Notice { id: format!("oc:{id}:error"), at: iso(&info["time"]["completed"]), notice_kind: "error".into(), text, url: None });
    }
    if let Some(tokens) = tokens {
        let number = |value: &Value| value.as_u64().unwrap_or(0);
        let input_tokens = number(&tokens["input"]).saturating_add(number(&tokens["cache"]["read"])).saturating_add(number(&tokens["cache"]["write"]));
        let output_tokens = number(&tokens["output"]).saturating_add(number(&tokens["reasoning"]));
        let context_window = info["providerID"].as_str().zip(info["modelID"].as_str()).and_then(|(provider, model)| window(provider, model));
        out.push(ChatItem::Usage { id: format!("oc:{id}:usage"), at: iso(&info["time"]["completed"]), input_tokens, output_tokens, context_window });
    }
    if ended {
        let parent = info["parentID"].as_str().unwrap_or("");
        let started = sent.iter().rev().find(|(user, _)| *user == parent).map(|(_, at)| *at);
        let duration_ms = completed.zip(started).map(|(done, start)| done.saturating_sub(start));
        let outcome = match info["error"]["name"].as_str() {
            Some("MessageAbortedError") => TurnOutcome::Interrupted,
            Some(_) => TurnOutcome::Failed,
            None if failed => TurnOutcome::Failed,
            None => TurnOutcome::Completed,
        };
        out.push(ChatItem::TurnEnd { id: format!("oc:{parent}:end"), at: iso(&info["time"]["completed"]), outcome, duration_ms });
    }
}

/// One `tool` part: the call, and its result once it has one. The task list tool gives a plan instead of a row.
fn tool(part: &Value, id: &str, out: &mut Vec<ChatItem>) {
    let Some(name) = part["tool"].as_str().filter(|name| !name.is_empty()) else {
        return;
    };
    let state = &part["state"];
    let input = state.get("input").filter(|input| input.is_object());
    let at = iso(&state["time"]["start"]);
    if name == "todowrite" {
        let items = input.and_then(|input| input["todos"].as_array()).map(|todos| todos.iter().filter_map(todo).collect()).unwrap_or_default();
        out.push(ChatItem::Plan { id: id.to_string(), at, items, replace: true });
        return;
    }
    let call_id = part["callID"].as_str().or_else(|| part["id"].as_str()).unwrap_or(id).to_string();
    let mut detail = ToolDetail { input: input.and_then(bound_input), paths: Vec::new(), writes: false, helper: name == "task" };
    if !FOLDER_TOOLS.contains(&name) {
        detail.paths = paths(input, state);
    }
    detail.writes = WRITE_TOOLS.contains(&name) && !detail.paths.is_empty();
    out.push(ChatItem::ToolCall { id: id.to_string(), at, call_id: call_id.clone(), name: name.to_string(), summary: summary(name, input), detail });
    let end = iso(&state["time"]["end"]);
    match state["status"].as_str() {
        Some("completed") => {
            // A command that ran and failed is completed for OpenCode; its exit code says otherwise.
            let ok = state["metadata"]["exit"].as_i64().is_none_or(|code| code == 0);
            out.push(ChatItem::ToolResult { id: format!("{id}:r"), at: end, call_id, ok, output: cut(state["output"].as_str().unwrap_or("")) });
        }
        Some("error") => {
            let error = state["error"].as_str().unwrap_or("The tool failed.");
            out.push(ChatItem::ToolResult { id: format!("{id}:r"), at: end, call_id, ok: false, output: cut(error) });
        }
        _ => {}
    }
}

fn todo(entry: &Value) -> Option<PlanItem> {
    let text = entry["content"].as_str().map(str::trim).filter(|text| !text.is_empty())?;
    let status = entry["status"].as_str().and_then(PlanStatus::parse).unwrap_or(PlanStatus::Pending);
    // A cancelled task is no longer on the list.
    (status != PlanStatus::Deleted).then(|| PlanItem { text: text.to_string(), status: Some(status), task_id: None })
}

/// The files a call names: the input's file fields, then what the tool reports it touched.
fn paths(input: Option<&Value>, state: &Value) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut add = |path: Option<&str>| {
        if let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) {
            if !paths.iter().any(|known| known == path) {
                paths.push(path.to_string());
            }
        }
    };
    for key in PATH_KEYS {
        add(input.and_then(|input| input[key].as_str()));
    }
    add(state["metadata"]["filepath"].as_str());
    for file in state["metadata"]["files"].as_array().into_iter().flatten() {
        add(file.as_str().or_else(|| file["filePath"].as_str()).or_else(|| file["file"].as_str()));
    }
    paths
}

/// One short line for a call: the command, the file, the pattern, the address, or the tool's own description.
fn summary(name: &str, input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    let keys: &[&str] = match name {
        "bash" => &["command", "description"],
        "grep" | "glob" => &["pattern", "path"],
        "webfetch" => &["url"],
        "websearch" => &["query"],
        "task" => &["description", "prompt"],
        _ => &["filePath", "file_path", "path", "command", "pattern", "url", "description"],
    };
    let line = keys.iter().find_map(|key| input[*key].as_str().map(str::trim).filter(|text| !text.is_empty()));
    let line = line.or_else(|| input.as_object()?.values().find_map(|value| value.as_str().map(str::trim).filter(|text| !text.is_empty())));
    cut_to(line.and_then(|text| text.lines().next()).unwrap_or(""), SUMMARY_CHARS)
}

fn error_text(error: &Value) -> String {
    let text = error["data"]["message"].as_str().or_else(|| error["message"].as_str()).or_else(|| error["name"].as_str()).unwrap_or("The model call failed.");
    cut_to(text, ERROR_CHARS)
}

fn millis(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_f64().filter(|ms| *ms >= 0.0).map(|ms| ms as u64))
}

/// A time in milliseconds since 1970 as text (`2026-10-28T10:00:00.000Z`).
fn iso(value: &Value) -> Option<String> {
    let at: DateTime<Utc> = DateTime::from_timestamp_millis(i64::try_from(millis(value)?).ok()?)?;
    Some(at.to_rfc3339_opts(SecondsFormat::Millis, true))
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
