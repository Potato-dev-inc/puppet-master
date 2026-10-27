//! Reads one line of a transcript with the JSON pointers of a `[session_chat]` table. Generic: it knows the
//! shapes the table describes and no agent. Unknown or noise lines give no items; nothing here panics.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};

use super::table::{ChatMap, ChatValues, Pointers, SessionChatTable};
use crate::session_chat::{cut, ChatItem, SessionFormat, ToolDetail, TurnOutcome};

/// A longer line is skipped, not parsed: a runaway line must not cost memory or time on every poll.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Longest first line of a tool call's input kept as its summary, in characters.
const SUMMARY_CHARS: usize = 200;

/// Tool input fields that name what a call works on, in the order they are tried.
const SUMMARY_KEYS: [&str; 9] = ["command", "cmd", "file_path", "path", "pattern", "query", "url", "description", "prompt"];

/// Tool input fields that name a file, in the order they are read.
const PATH_KEYS: [&str; 8] = ["file_path", "filepath", "file", "filename", "target_file", "notebook_path", "path", "target"];

/// Input fields only a call that changes a file has.
const WRITE_INPUT_KEYS: [&str; 10] =
    ["content", "new_string", "new_str", "old_string", "old_str", "new_text", "new_content", "patch", "diff", "edits"];

/// Words in a tool's name that say it changes a file.
const WRITE_NAME_HINTS: [&str; 7] = ["write", "edit", "patch", "create", "replace", "insert", "save"];

/// A transcript format described by a `[session_chat]` table.
pub struct DeclaredSessionFile {
    map: ChatMap,
    values: ChatValues,
    write_tools: Vec<String>,
    helper_tools: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    User,
    Assistant,
    Tool,
}

impl DeclaredSessionFile {
    pub fn new(table: &SessionChatTable) -> Self {
        let lower = |list: &[String]| list.iter().map(|v| v.to_lowercase()).collect::<Vec<_>>();
        DeclaredSessionFile {
            map: table.map.clone(),
            values: ChatValues {
                user: lower(&table.values.user),
                assistant: lower(&table.values.assistant),
                tool: lower(&table.values.tool),
                turn_end: lower(&table.values.turn_end),
            },
            write_tools: lower(&table.write_tools),
            helper_tools: lower(&table.helper_tools),
        }
    }

    fn role(&self, line: &Value) -> Option<Role> {
        let word = scalar(first(line, &self.map.role)?)?.to_lowercase();
        let is = |list: &[String]| list.iter().any(|v| *v == word);
        if is(&self.values.user) {
            Some(Role::User)
        } else if is(&self.values.assistant) {
            Some(Role::Assistant)
        } else if is(&self.values.tool) {
            Some(Role::Tool)
        } else {
            None
        }
    }

    fn turn_ended(&self, line: &Value) -> bool {
        first(line, &self.map.turn_end)
            .and_then(scalar)
            .is_some_and(|word| self.values.turn_end.contains(&word.to_lowercase()))
    }

    fn tool_detail(&self, name: &str, input: Option<&Value>) -> ToolDetail {
        let mut detail = ToolDetail::from_input(input);
        if let Some(input) = input {
            for key in PATH_KEYS {
                if let Some(path) = input.get(key).and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty()) {
                    if !detail.paths.iter().any(|known| known == path) {
                        detail.paths.push(path.to_string());
                    }
                }
            }
        }
        let name = name.to_lowercase();
        detail.helper = self.helper_tools.contains(&name);
        detail.writes = !detail.paths.is_empty()
            && if self.write_tools.is_empty() {
                WRITE_NAME_HINTS.iter().any(|hint| name.contains(hint))
                    || input.is_some_and(|i| WRITE_INPUT_KEYS.iter().any(|key| i.get(key).is_some()))
            } else {
                self.write_tools.contains(&name)
            };
        detail
    }
}

impl SessionFormat for DeclaredSessionFile {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        if line.len() > MAX_LINE_BYTES {
            return Vec::new();
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        if !value.is_object() {
            return Vec::new();
        }
        let mut out = Out { offset, at: first(&value, &self.map.timestamp).and_then(timestamp), items: Vec::new() };
        let role = self.role(&value);
        let ended = self.turn_ended(&value);
        let text = first(&value, &self.map.text).map(text_of).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let result_for = first(&value, &self.map.tool_result_for).and_then(scalar);
        match role {
            Some(Role::User) => match result_for {
                Some(call_id) => self.tool_result(&value, call_id, text, &mut out),
                None => {
                    if let Some(text) = text {
                        out.push(|id, at| ChatItem::User { id, at, text });
                    }
                }
            },
            Some(Role::Assistant) => {
                if let Some(text) = text {
                    let model = first(&value, &self.map.model).and_then(scalar);
                    out.push(|id, at| ChatItem::Assistant { id, at, text, model, final_answer: ended });
                }
                self.tool_call(&value, &mut out);
            }
            Some(Role::Tool) => match result_for {
                Some(call_id) => self.tool_result(&value, call_id, text, &mut out),
                None => self.tool_call(&value, &mut out),
            },
            None => {}
        }
        if ended {
            out.push(|id, at| ChatItem::TurnEnd { id, at, outcome: TurnOutcome::Completed, duration_ms: None });
        }
        let number = |pointers: &Pointers| first(&value, pointers).and_then(count);
        let (input_tokens, output_tokens) = (number(&self.map.usage_input), number(&self.map.usage_output));
        if input_tokens.is_some() || output_tokens.is_some() {
            let (input_tokens, output_tokens) = (input_tokens.unwrap_or(0), output_tokens.unwrap_or(0));
            out.push(|id, at| ChatItem::Usage { id, at, input_tokens, output_tokens, context_window: None });
        }
        out.items
    }
}

impl DeclaredSessionFile {
    fn tool_call(&self, line: &Value, out: &mut Out) {
        let Some(name) = first(line, &self.map.tool_name).and_then(scalar).filter(|n| !n.trim().is_empty()) else {
            return;
        };
        let call_id = first(line, &self.map.tool_id).and_then(scalar).unwrap_or_else(|| format!("line-{}", out.offset));
        let input = first(line, &self.map.tool_input).and_then(tool_input);
        let summary = summary(input.as_ref(), &name);
        let detail = self.tool_detail(&name, input.as_ref());
        out.push(|id, at| ChatItem::ToolCall { id, at, call_id, name, summary, detail });
    }

    fn tool_result(&self, line: &Value, call_id: String, text: Option<String>, out: &mut Out) {
        let output = match first(line, &self.map.tool_output) {
            Some(value) => text_of(value),
            None => text.unwrap_or_default(),
        };
        let output = cut(&strip_ansi(&output));
        let failed = first(line, &self.map.tool_error).is_some_and(failed);
        out.push(|id, at| ChatItem::ToolResult { id, at, call_id, ok: !failed, output });
    }
}

/// The items of one line. Ids are `<line offset>:<index within the line>`.
struct Out {
    offset: u64,
    at: Option<String>,
    items: Vec<ChatItem>,
}

impl Out {
    fn push(&mut self, make: impl FnOnce(String, Option<String>) -> ChatItem) {
        let id = format!("{}:{}", self.offset, self.items.len());
        self.items.push(make(id, self.at.clone()));
    }
}

/// The first of `pointers` that finds a value which is not `null`.
fn first<'a>(line: &'a Value, pointers: &Pointers) -> Option<&'a Value> {
    pointers.0.iter().filter_map(|pointer| line.pointer(pointer)).find(|value| !value.is_null())
}

/// A string, number or boolean as text; anything else is not a scalar.
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// A token count: a number, or a string of digits.
fn count(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().or_else(|| number.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// A flag on a tool result: `true`, a number other than 0 or a message means the tool failed.
fn failed(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !matches!(text.trim().to_lowercase().as_str(), "" | "false" | "0" | "ok" | "success"),
        Value::Null => false,
        _ => true,
    }
}

/// The text of a message: a string, or the text of each part of an array (a plain string, or an object with a
/// `text` that has no `type` or a text type), joined by newlines. Anything else (an object) is its JSON.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(part_text).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn part_text(part: &Value) -> Option<String> {
    match part {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => {
            let kind = map.get("type").and_then(Value::as_str);
            let is_text = kind.is_none_or(|kind| matches!(kind, "text" | "output_text" | "input_text"));
            map.get("text").and_then(Value::as_str).filter(|_| is_text).map(str::to_string)
        }
        _ => None,
    }
}

/// A tool's input as JSON: an object as it is, a string that holds a JSON object parsed, any other string as
/// `{"command": text}`.
fn tool_input(value: &Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value.clone()),
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(parsed) if parsed.is_object() => Some(parsed),
            _ => Some(json!({"command": text})),
        },
        _ => None,
    }
}

/// The first input field that holds text, as its first line cut to `SUMMARY_CHARS`; the tool's name when none does.
fn summary(input: Option<&Value>, name: &str) -> String {
    let text = input
        .and_then(|input| SUMMARY_KEYS.iter().filter_map(|key| input.get(*key).and_then(Value::as_str)).find(|text| !text.trim().is_empty()))
        .unwrap_or(name);
    let line = text.trim().lines().next().unwrap_or("");
    crate::session_chat::cut_to(line, SUMMARY_CHARS)
}

/// A line's time as RFC 3339: a string as it is, a number as Unix seconds (or milliseconds when it is that large).
fn timestamp(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()).filter(|t| !t.is_empty()),
        Value::Number(number) => {
            let raw = number.as_f64().filter(|n| n.is_finite() && *n >= 0.0)?;
            let millis = if raw >= 1e11 { raw } else { raw * 1000.0 };
            DateTime::<Utc>::from_timestamp_millis(millis as i64).map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
        }
        _ => None,
    }
}

/// `text` without terminal escape codes: `ESC [ ... final`, `ESC ] ... BEL` or `ESC ] ... ESC \`, and any other `ESC x`.
fn strip_ansi(text: &str) -> String {
    if !text.contains('\u{1b}') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut prev = '\0';
                for next in chars.by_ref() {
                    if next == '\u{7}' || (prev == '\u{1b}' && next == '\\') {
                        break;
                    }
                    prev = next;
                }
            }
            _ => {}
        }
    }
    out
}
