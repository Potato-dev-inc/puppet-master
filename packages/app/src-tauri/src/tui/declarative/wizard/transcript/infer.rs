//! Works out a `[session_chat]` mapping from the lines a CLI appended to its transcript during the wizard's test turn.
//!
//! The wizard knows two texts for certain: the task it typed and the reply it read off the screen. The line that
//! holds the task is the user's line, the other line that holds the reply is the assistant's; the JSON field that
//! differs between them (`role`, `type`, ...) is the role, and where each text sits is the text pointer. Tool
//! lines are found by shape (a name and an input together; an id and an output together). The result is only a
//! guess: `verify` runs the real reader over the lines before anything is written.

use serde_json::{Map, Value};

use super::leaves::{escape, leaves, norm, text_pointer, Leaf};

/// Field names a role is kept in, best first.
const ROLE_KEYS: [&str; 9] = ["role", "sender", "author", "from", "speaker", "actor", "type", "kind", "event"];
/// Other words for the roles, taken from the same field of the other lines.
const USER_WORDS: [&str; 2] = ["user", "human"];
const ASSISTANT_WORDS: [&str; 5] = ["assistant", "ai", "model", "agent", "bot"];
const TOOL_WORDS: [&str; 7] = ["tool", "function", "tool_result", "tool_use", "tool_call", "tool_output", "tool_response"];
/// Tool call fields.
const NAME_KEYS: [&str; 4] = ["tool_name", "tool", "name", "function"];
const INPUT_KEYS: [&str; 3] = ["input", "arguments", "args"];
const CALL_ID_KEYS: [&str; 5] = ["id", "tool_call_id", "call_id", "tool_use_id", "toolCallId"];
/// Tool result fields.
const RESULT_FOR_KEYS: [&str; 6] = ["tool_use_id", "tool_call_id", "call_id", "result_for", "toolCallId", "for"];
const OUTPUT_KEYS: [&str; 5] = ["output", "content", "result", "stdout", "text"];
const ERROR_KEYS: [&str; 4] = ["is_error", "isError", "error", "exit_code"];
const TIME_KEYS: [&str; 4] = ["timestamp", "ts", "created_at", "time"];

/// What was worked out. Every pointer is an RFC 6901 pointer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mapping {
    pub role: String,
    pub text: Vec<String>,
    pub user_values: Vec<String>,
    pub assistant_values: Vec<String>,
    pub tool_values: Vec<String>,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool: Option<ToolMap>,
}

/// The tool fields, as pointers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolMap {
    pub name: String,
    pub input: Option<String>,
    pub id: Option<String>,
    pub result_for: Option<String>,
    pub output: Option<String>,
    pub error: Option<String>,
    /// The name of the tool the call line holds, to check the mapping against.
    pub sample_name: String,
}

fn parse(lines: &[String]) -> Vec<Value> {
    lines.iter().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(Value::is_object).collect()
}

/// A leaf that could be a role word: short, no spaces, not inside an array.
fn role_leaves(line: &Value) -> Vec<Leaf> {
    let mut found: Vec<Leaf> = leaves(line)
        .into_iter()
        .filter(|l| {
            !l.in_array
                && ROLE_KEYS.contains(&l.key.as_str())
                && (1..=24).contains(&l.value.len())
                && l.value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
        .collect();
    found.sort_by_key(|l| (ROLE_KEYS.iter().position(|k| *k == l.key).unwrap_or(99), l.pointer.matches('/').count()));
    found
}

/// The string leaves of a line that hold `text` (compared in normal form), the shortest first.
fn holding(line: &Value, text: &str) -> Vec<Leaf> {
    let want = norm(text);
    let mut found: Vec<Leaf> = leaves(line).into_iter().filter(|l| !want.is_empty() && norm(&l.value).contains(&want)).collect();
    found.sort_by_key(|l| l.value.len());
    found
}

fn value_at<'a>(line: &'a Value, pointer: &str) -> Option<&'a str> {
    line.pointer(pointer).and_then(Value::as_str)
}

/// Why no mapping was found, from the least to the most that was found out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Missing {
    Json,
    Message,
    Reply,
    Role,
}

impl Missing {
    pub fn why(self) -> &'static str {
        match self {
            Missing::Json => "the file that changed has no JSON object lines",
            Missing::Message => "no line of the file that changed holds the message the wizard typed",
            Missing::Reply => "a line holds the message the wizard typed, but no other line holds the reply it read",
            Missing::Role => "the line with the message and the line with the reply show no field that tells who wrote them",
        }
    }
}

/// The texts that mark the assistant's line: the reply as read off the screen, the same without the glyphs and labels
/// the screen put in front of it (`● `, `reply: `), and the random word the test asked for. The word is the anchor: the
/// screen may decorate a reply, a transcript holds it as the program wrote it.
pub fn needles(reply: &str, word: &str) -> Vec<String> {
    let trimmed = reply.trim();
    let bare = trimmed.trim_start_matches(|c: char| !c.is_alphanumeric());
    let mut out: Vec<String> = Vec::new();
    for text in [trimmed, bare, word.trim()] {
        if !text.is_empty() && !out.iter().any(|known| known == text) {
            out.push(text.to_string());
        }
    }
    out
}

fn holding_any(line: &Value, texts: &[String]) -> Vec<Leaf> {
    texts.iter().map(|t| holding(line, t)).find(|found| !found.is_empty()).unwrap_or_default()
}

/// The mapping for `lines`, or why there is none. `reply` is what `needles` returns.
pub fn infer(lines: &[String], prompt: &str, reply: &[String]) -> Result<Mapping, Missing> {
    let all = parse(lines);
    if all.is_empty() {
        return Err(Missing::Json);
    }
    let user_lines: Vec<&Value> = all.iter().filter(|l| !holding(l, prompt).is_empty()).collect();
    if user_lines.is_empty() {
        return Err(Missing::Message);
    }
    // the assistant's line holds the reply and not the task (the task itself holds the reply word)
    let reply_lines: Vec<&Value> = all.iter().filter(|l| holding(l, prompt).is_empty() && !holding_any(l, reply).is_empty()).collect();
    if reply_lines.is_empty() {
        return Err(Missing::Reply);
    }
    for user in &user_lines {
        for assistant in &reply_lines {
            for role in role_leaves(user) {
                let Some(other) = value_at(assistant, &role.pointer) else { continue };
                if other.eq_ignore_ascii_case(&role.value) {
                    continue;
                }
                let mut mapping = Mapping { role: role.pointer.clone(), ..Mapping::default() };
                mapping.user_values.push(role.value.clone());
                mapping.assistant_values.push(other.to_string());
                let sources = [holding(user, prompt), holding_any(assistant, reply)];
                for pointer in sources.iter().filter_map(|l| l.first()).map(|l| text_pointer(&l.pointer)) {
                    if !pointer.is_empty() && !mapping.text.contains(&pointer) {
                        mapping.text.push(pointer);
                    }
                }
                if mapping.text.is_empty() {
                    continue;
                }
                words_from_others(&all, &mut mapping);
                mapping.timestamp = key_pointer(assistant, &TIME_KEYS, true).or_else(|| key_pointer(user, &TIME_KEYS, true));
                mapping.model = key_pointer(assistant, &["model"], false);
                mapping.tool = tool_map(&all);
                return Ok(mapping);
            }
        }
    }
    Err(Missing::Role)
}

/// More role words from the same field of the other lines (`human`, `tool_result`, ...).
fn words_from_others(all: &[Value], mapping: &mut Mapping) {
    for line in all {
        let Some(word) = value_at(line, &mapping.role).map(str::to_lowercase) else { continue };
        let known = |list: &[String]| list.iter().any(|v| v.eq_ignore_ascii_case(&word));
        if known(&mapping.user_values) || known(&mapping.assistant_values) || known(&mapping.tool_values) {
            continue;
        }
        if USER_WORDS.contains(&word.as_str()) {
            mapping.user_values.push(word);
        } else if ASSISTANT_WORDS.contains(&word.as_str()) {
            mapping.assistant_values.push(word);
        } else if TOOL_WORDS.contains(&word.as_str()) {
            mapping.tool_values.push(word);
        }
    }
}

/// The pointer of the first scalar (a string, or a number when `number`) in `line` under one of `keys`, not in an array.
fn key_pointer(line: &Value, keys: &[&str], number: bool) -> Option<String> {
    fn walk(value: &Value, pointer: &str, keys: &[&str], number: bool, depth: usize, out: &mut Vec<(usize, String)>) {
        if depth > 3 {
            return;
        }
        if let Value::Object(map) = value {
            for (name, item) in map {
                let at = format!("{pointer}/{}", escape(name));
                let scalar = item.is_string() || (number && item.is_number());
                if let (true, Some(rank)) = (scalar, keys.iter().position(|k| k == name)) {
                    out.push((rank * 10 + depth, at.clone()));
                }
                walk(item, &at, keys, number, depth + 1, out);
            }
        }
    }
    let mut found = Vec::new();
    walk(line, "", keys, number, 0, &mut found);
    found.into_iter().min().map(|(_, pointer)| pointer)
}

/// Objects in `value` (the line itself, or nested in objects and the first items of arrays) with their pointers.
fn objects<'a>(value: &'a Value, pointer: String, depth: usize, out: &mut Vec<(String, &'a Map<String, Value>)>) {
    if depth > 6 {
        return;
    }
    match value {
        Value::Object(map) => {
            out.push((pointer.clone(), map));
            for (name, item) in map {
                objects(item, format!("{pointer}/{}", escape(name)), depth + 1, out);
            }
        }
        Value::Array(items) => {
            for (at, item) in items.iter().take(4).enumerate() {
                objects(item, format!("{pointer}/{at}"), depth + 1, out);
            }
        }
        _ => {}
    }
}

/// The first of `keys` that `map` has with a value that is not null.
fn first_key<'a>(map: &Map<String, Value>, keys: &[&'a str]) -> Option<&'a str> {
    keys.iter().copied().find(|k| map.get(*k).is_some_and(|v| !v.is_null()))
}

fn is_id(value: &Value) -> bool {
    value.is_string() || value.is_number()
}

/// The first tool call and the first tool result found by shape.
fn tool_map(all: &[Value]) -> Option<ToolMap> {
    let mut tool: Option<ToolMap> = None;
    for line in all {
        let mut found = Vec::new();
        objects(line, String::new(), 0, &mut found);
        for (pointer, map) in found {
            let at = |key: &str| format!("{pointer}/{}", escape(key));
            match tool.as_mut() {
                None => {
                    let name = NAME_KEYS.iter().copied().find(|k| map.get(*k).and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty()));
                    if let (Some(name), Some(input)) = (name, first_key(map, &INPUT_KEYS)) {
                        tool = Some(ToolMap {
                            name: at(name),
                            input: Some(at(input)),
                            id: CALL_ID_KEYS.iter().copied().find(|k| map.get(*k).is_some_and(is_id)).map(at),
                            sample_name: map.get(name).and_then(Value::as_str).unwrap_or_default().to_string(),
                            ..ToolMap::default()
                        });
                    }
                }
                Some(t) if t.result_for.is_none() => {
                    let reference = RESULT_FOR_KEYS.iter().copied().find(|k| map.get(*k).is_some_and(is_id));
                    if let (Some(reference), Some(output)) = (reference, first_key(map, &OUTPUT_KEYS)) {
                        t.result_for = Some(at(reference));
                        t.output = Some(at(output));
                        t.error = first_key(map, &ERROR_KEYS).map(at);
                    }
                }
                Some(_) => {}
            }
        }
    }
    tool
}
