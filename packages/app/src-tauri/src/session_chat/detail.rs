//! The extra detail a chat item can carry for a richer view: what a tool was called with, which files it touches
//! and the agent's task list. Generic: each agent's session reader decides which tools are writes or helpers.

use serde::Serialize;
use serde_json::Value;

use super::model::MAX_OUTPUT_CHARS;

/// Most characters (of the JSON text) a tool call's `input` may take in all.
pub const MAX_INPUT_CHARS: usize = 8000;

/// Input fields that name a file or folder, in the order they are read.
const PATH_KEYS: [&str; 3] = ["file_path", "notebook_path", "path"];

/// What a `ToolCall` carries besides its one-line summary. Every field is left out of the JSON when empty, so
/// older viewers see the same items as before.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDetail {
    /// The tool's input JSON with each string cut and the whole kept near `MAX_INPUT_CHARS`.
    /// Edit-like tools keep their old/new text here so a view can draw a diff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// Files the call touches, in order, without repeats.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The call changes the files in `paths` (an edit or a write, not a read).
    #[serde(skip_serializing_if = "is_false")]
    pub writes: bool,
    /// The call starts a helper agent (a sub-agent thread) rather than doing the work itself.
    #[serde(skip_serializing_if = "is_false")]
    pub helper: bool,
}

fn is_false(flag: &bool) -> bool {
    !*flag
}

impl ToolDetail {
    /// Detail from a tool's raw input: bounded input, and the files named by the generic keys.
    pub fn from_input(input: Option<&Value>) -> Self {
        ToolDetail {
            input: input.and_then(bound_input),
            paths: input.map(paths_from_input).unwrap_or_default(),
            writes: false,
            helper: false,
        }
    }
}

/// One entry of the agent's task list.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    /// The task's text. Empty in an update that only changes the status.
    pub text: String,
    /// Always set in a full list. `None` in an update that does not change the status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PlanStatus>,
    /// The agent's own id for the task. Set when the list is sent task by task, so a later update finds it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
    /// Only in an update: the task is gone from the list.
    Deleted,
}

impl PlanStatus {
    /// Reads an agent's status word (`pending`, `in_progress` / `inProgress` / `in-progress`, `completed` / `done`,
    /// `deleted`). `None` for anything else.
    pub fn parse(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "pending" | "todo" | "not_started" => Some(PlanStatus::Pending),
            "in_progress" | "inprogress" | "active" | "running" => Some(PlanStatus::InProgress),
            "completed" | "complete" | "done" => Some(PlanStatus::Completed),
            "deleted" | "removed" | "cancelled" | "canceled" => Some(PlanStatus::Deleted),
            _ => None,
        }
    }
}

/// `text` cut to `limit` characters; a cut text ends with "…". Never splits a character.
pub fn cut_to(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        None => text.to_string(),
        Some((byte_end, _)) => format!("{}…", &text[..byte_end]),
    }
}

/// A tool input bounded for the view: every string cut like tool output (`cut`), and when the JSON is still
/// longer than `MAX_INPUT_CHARS` the strings are cut shorter step by step. `None` when nothing fits (or the input is
/// empty).
pub fn bound_input(input: &Value) -> Option<Value> {
    if input.is_null() || input.as_object().is_some_and(|map| map.is_empty()) {
        return None;
    }
    let mut limit = MAX_OUTPUT_CHARS;
    while limit >= 64 {
        let cut = cut_strings(input, limit);
        if cut.to_string().chars().count() <= MAX_INPUT_CHARS {
            return Some(cut);
        }
        limit /= 2;
    }
    None
}

fn cut_strings(value: &Value, limit: usize) -> Value {
    match value {
        Value::String(text) => Value::String(cut_to(text, limit)),
        Value::Array(list) => Value::Array(list.iter().map(|item| cut_strings(item, limit)).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(key, item)| (key.clone(), cut_strings(item, limit))).collect()),
        other => other.clone(),
    }
}

/// The files a tool input names with the generic keys (`file_path`, `notebook_path`, `path`), without repeats.
pub fn paths_from_input(input: &Value) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for key in PATH_KEYS {
        if let Some(path) = input.get(key).and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty()) {
            if !paths.iter().any(|known| known == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_detail_adds_nothing_to_the_json() {
        let value = serde_json::to_value(ToolDetail::default()).unwrap();
        assert_eq!(value, json!({}));
    }

    #[test]
    fn detail_serialises_camel_case_and_only_what_is_set() {
        let detail = ToolDetail {
            input: Some(json!({"file_path": "a.rs"})),
            paths: vec!["a.rs".into()],
            writes: true,
            helper: false,
        };
        assert_eq!(
            serde_json::to_value(detail).unwrap(),
            json!({"input": {"file_path": "a.rs"}, "paths": ["a.rs"], "writes": true})
        );
    }

    #[test]
    fn plan_item_leaves_out_unset_fields() {
        let item = PlanItem { text: "Run tests".into(), status: Some(PlanStatus::InProgress), task_id: None };
        assert_eq!(serde_json::to_value(item).unwrap(), json!({"text": "Run tests", "status": "in_progress"}));
        let update = PlanItem { text: String::new(), status: None, task_id: Some("3".into()) };
        assert_eq!(serde_json::to_value(update).unwrap(), json!({"text": "", "taskId": "3"}));
    }

    #[test]
    fn status_words_are_read_loosely() {
        assert_eq!(PlanStatus::parse("pending"), Some(PlanStatus::Pending));
        assert_eq!(PlanStatus::parse("in_progress"), Some(PlanStatus::InProgress));
        assert_eq!(PlanStatus::parse("In-Progress"), Some(PlanStatus::InProgress));
        assert_eq!(PlanStatus::parse("completed"), Some(PlanStatus::Completed));
        assert_eq!(PlanStatus::parse("deleted"), Some(PlanStatus::Deleted));
        assert_eq!(PlanStatus::parse("weird"), None);
    }

    #[test]
    fn small_input_is_kept_whole_and_empty_input_is_none() {
        let input = json!({"command": "ls", "n": 3, "flag": true});
        assert_eq!(bound_input(&input), Some(input));
        assert_eq!(bound_input(&json!({})), None);
        assert_eq!(bound_input(&Value::Null), None);
    }

    #[test]
    fn each_string_is_cut_like_tool_output() {
        let input = json!({"content": "x".repeat(MAX_OUTPUT_CHARS + 50)});
        let bounded = bound_input(&input).unwrap();
        // The cut text is 4000 characters + "…" = 4001, which fits the 8000 total.
        assert_eq!(bounded["content"].as_str().unwrap().chars().count(), MAX_OUTPUT_CHARS + 1);
    }

    #[test]
    fn a_big_input_is_cut_shorter_until_it_fits_the_total() {
        let input = json!({
            "old_string": "a".repeat(MAX_OUTPUT_CHARS * 2),
            "new_string": "b".repeat(MAX_OUTPUT_CHARS * 2),
            "file_path": "x.rs"
        });
        let bounded = bound_input(&input).unwrap();
        assert!(bounded.to_string().chars().count() <= MAX_INPUT_CHARS);
        assert!(bounded["old_string"].as_str().unwrap().ends_with('…'));
        assert_eq!(bounded["file_path"], "x.rs");
    }

    #[test]
    fn an_input_that_cannot_fit_gives_none() {
        let many: Vec<Value> = (0..2000).map(|n| json!({"k": n, "v": "some text here"})).collect();
        assert_eq!(bound_input(&Value::Array(many)), None);
    }

    #[test]
    fn paths_come_from_the_generic_keys_in_order_without_repeats() {
        assert_eq!(paths_from_input(&json!({"path": "a", "file_path": "b"})), vec!["b", "a"]);
        assert_eq!(paths_from_input(&json!({"file_path": "a", "path": "a"})), vec!["a"]);
        assert_eq!(paths_from_input(&json!({"notebook_path": " n.ipynb ", "file_path": ""})), vec!["n.ipynb"]);
        assert!(paths_from_input(&json!({"command": "ls"})).is_empty());
        assert!(paths_from_input(&json!("text")).is_empty());
    }

    #[test]
    fn cut_to_counts_characters() {
        assert_eq!(cut_to("héllo", 3), "hél…");
        assert_eq!(cut_to("hi", 3), "hi");
    }
}
