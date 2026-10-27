//! The parts of a Codex rollout beyond messages and tool rows: what a tool call touches (the patch of a file change),
//! the task list (`update_plan`), and session notes (compaction, errors, thread name).
//!
//! Captured from Codex 0.162.1 (`codex exec`): `FileChange` items (`changes: {<path>: {type: "add", content}}`),
//! `CommandExecution`, `token_count` and `task_started`. That run had no `update_plan` tool, so the lines marked
//! "shape from docs/guess" are read defensively from Codex's protocol (`plan_update {explanation, plan: [{step,
//! status}]}`, `context_compacted`, `error {message}`, `thread_name_updated {thread_name}`).

use serde_json::{json, Value};

use crate::session_chat::{bound_input, cut_to, ChatItem, PlanItem, PlanStatus, ToolDetail};

/// Header words of an `apply_patch` text, each followed by a path.
const PATCH_HEADERS: [&str; 4] = ["*** Add File: ", "*** Update File: ", "*** Delete File: ", "*** Move to: "];

/// Longest notice text, in characters.
const NOTICE_CHARS: usize = 300;

/// The files in an `apply_patch` text: the paths named by its `*** Add File:`, `*** Update File:`, `*** Delete File:`
/// and `*** Move to:` header lines, in order, without repeats.
pub(super) fn patch_paths(patch: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for line in patch.lines() {
        let line = line.trim_start();
        let found = PATCH_HEADERS.iter().find_map(|header| line.strip_prefix(header));
        if let Some(path) = found.map(str::trim).filter(|path| !path.is_empty()) {
            if !paths.iter().any(|known| known == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

/// What a file change touches. `changes` is keyed by path (or a list with a `path` each); each entry may name a
/// `move_path`. When no path is named that way, every text in the change is searched for patch headers.
/// `sorted_keys` are the paths the caller already found, kept first.
pub(super) fn file_change_detail(item: &Value, sorted_keys: Vec<String>) -> ToolDetail {
    let changes = ["changes", "patch", "input"].iter().filter_map(|key| item.get(*key)).find(|value| !is_empty(value));
    let mut paths = sorted_keys;
    if let Some(changes) = changes {
        let entries: Vec<&Value> = match changes {
            Value::Object(map) => map.values().collect(),
            Value::Array(list) => list.iter().collect(),
            _ => Vec::new(),
        };
        for entry in entries {
            if let Some(moved) = entry.get("move_path").or_else(|| entry.get("movePath")).and_then(Value::as_str) {
                push_unique(&mut paths, moved.to_string());
            }
        }
    }
    // No keyed path: the change is a patch text, so look for its header lines.
    if paths.is_empty() {
        let mut texts = Vec::new();
        if let Some(changes) = changes {
            collect_strings(changes, &mut texts);
        }
        for text in texts {
            for path in patch_paths(text) {
                push_unique(&mut paths, path);
            }
        }
    }
    // A patch given as plain text becomes `{patch: <text>}`, so `input` is always an object.
    let input = match changes {
        Some(Value::String(text)) => bound_input(&json!({ "patch": text })),
        Some(other) => bound_input(other),
        None => None,
    };
    ToolDetail { input, paths, writes: true, helper: false }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(list) => list.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn push_unique(paths: &mut Vec<String>, path: String) {
    if !path.is_empty() && !paths.contains(&path) {
        paths.push(path);
    }
}

fn collect_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(list) => list.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

/// The task list from an `update_plan` payload: `{explanation?, plan: [{step, status}]}`. Always the whole list.
/// Shape from docs/guess (no short run has the tool): `step` may also be called `text` or `content`.
pub(super) fn plan_items(args: &Value) -> Option<Vec<PlanItem>> {
    let plan = args.get("plan").and_then(Value::as_array)?;
    Some(
        plan.iter()
            .filter_map(|entry| {
                let text = ["step", "text", "content"].iter().find_map(|key| entry.get(*key).and_then(Value::as_str))?;
                let status = entry.get("status").and_then(Value::as_str).and_then(PlanStatus::parse).unwrap_or(PlanStatus::Pending);
                Some(PlanItem { text: text.trim().to_string(), status: Some(status), task_id: None })
            })
            .collect(),
    )
}

/// The `arguments` of a function call: a JSON text (usual) or already an object.
pub(super) fn call_arguments(payload: &Value) -> Option<Value> {
    match payload.get("arguments").or_else(|| payload.get("input"))? {
        Value::String(text) => serde_json::from_str(text).ok(),
        other => Some(other.clone()),
    }
}

/// A `response_item` that calls the plan tool, as a whole-list `Plan` item. Other response items give nothing (they
/// repeat the `event_msg` items). Shape from docs/guess.
pub(super) fn plan_call(payload: &Value) -> Option<Vec<PlanItem>> {
    let is_call = matches!(payload.get("type").and_then(Value::as_str), Some("function_call" | "custom_tool_call"));
    if !is_call || payload.get("name").and_then(Value::as_str) != Some("update_plan") {
        return None;
    }
    plan_items(&call_arguments(payload)?)
}

/// The to-do list item of `codex exec --json` (`{items: [{text, completed}]}`) as a whole list. Shape from docs/guess.
pub(super) fn todo_items(item: &Value) -> Option<Vec<PlanItem>> {
    let list = item.get("items").and_then(Value::as_array)?;
    Some(
        list.iter()
            .filter_map(|entry| {
                let text = entry.get("text").and_then(Value::as_str)?.trim().to_string();
                let done = entry.get("completed").and_then(Value::as_bool).unwrap_or(false);
                let status = if done { PlanStatus::Completed } else { PlanStatus::Pending };
                Some(PlanItem { text, status: Some(status), task_id: None })
            })
            .collect(),
    )
}

/// The notice an `event_msg` of this type gives, as `(kind, text)`. Shape from docs/guess for all three.
pub(super) fn event_notice(kind: &str, payload: &Value) -> Option<(&'static str, String)> {
    match kind {
        "context_compacted" => Some(("compacted", "Context compacted".to_string())),
        "error" => {
            let message = payload.get("message").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty());
            Some(("error", cut_to(&format!("Error: {}", message.unwrap_or("something went wrong")), NOTICE_CHARS)))
        }
        "thread_name_updated" => {
            let name = payload.get("thread_name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty())?;
            Some(("title", cut_to(name, NOTICE_CHARS)))
        }
        _ => None,
    }
}

/// A notice item for the line being read.
pub(super) fn notice(id: String, at: Option<String>, kind: &str, text: String) -> ChatItem {
    ChatItem::Notice { id, at, notice_kind: kind.to_string(), text, url: None }
}

#[cfg(test)]
#[path = "session_extra_tests.rs"]
mod tests;
