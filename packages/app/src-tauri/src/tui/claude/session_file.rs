//! Claude Code's session file: `<config dir>/projects/<encoded cwd>/<session id>.jsonl`, one JSON
//! object per line. Only the lines a chat view needs give items; the rest (attachments, hooks,
//! snapshots, thinking) is noise. Shapes: `D:\pm-acp-spike\PROBES.md` section B3.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use super::session_extra::{context_window, note_line, plan_call, plan_result, system_line, tool_detail};
use crate::session_chat::{cut, ChatItem, SessionFormat, ToolDetail, TurnOutcome};

/// Longest first line of a tool call's input kept as its summary, in characters.
const SUMMARY_CHARS: usize = 200;

/// Tool input fields that name what a call works on, in the order they are tried.
const SUMMARY_KEYS: [&str; 7] = ["command", "file_path", "path", "pattern", "url", "description", "prompt"];

/// User text that the CLI injects rather than the person typing it.
const INJECTED: [&str; 4] = ["<command-name>", "<local-command-", "<system-reminder>", "Caveat:"];

const INTERRUPTED: &str = "[Request interrupted by user";

/// The tool name a `!` shell command gets in the chat view.
const SHELL: &str = "Shell";

pub struct ClaudeSessionFile;

impl SessionFormat for ClaudeSessionFile {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let mut out = Line {
            offset,
            at: value.get("timestamp").and_then(Value::as_str).map(str::to_string),
            items: Vec::new(),
        };
        match kind(&value) {
            Some("user") => user_line(&value, &mut out),
            Some("assistant") => assistant_line(&value, &mut out),
            Some("system") => system_line(&value, &mut out),
            Some(note @ ("pr-link" | "custom-title" | "ai-title")) => note_line(note, &value, &mut out),
            _ => {}
        }
        out.items
    }
}

/// The items of one line. Ids are `<line offset>:<index within the line>`.
pub(super) struct Line {
    offset: u64,
    at: Option<String>,
    items: Vec<ChatItem>,
}

impl Line {
    pub(super) fn push(&mut self, make: impl FnOnce(String, Option<String>) -> ChatItem) {
        let id = format!("{}:{}", self.offset, self.items.len());
        self.items.push(make(id, self.at.clone()));
    }
}

fn kind(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

fn user_line(value: &Value, out: &mut Line) {
    // Every user line that names its CLI version tells the view which version wrote it.
    if let Some(version) = value.get("version").and_then(Value::as_str) {
        let cli_version = version.to_string();
        out.push(|id, at| ChatItem::Session { id, at, model: None, cli_version: Some(cli_version) });
    }
    // `isCompactSummary` is the summary the CLI writes as a user message after compacting: not what the person typed.
    if flag(value, "isMeta") || flag(value, "isSidechain") || flag(value, "isCompactSummary") {
        return;
    }
    match value.pointer("/message/content") {
        Some(Value::String(text)) => user_text(text, value, out),
        Some(Value::Array(blocks)) => {
            user_text(&texts(blocks).join("\n"), value, out);
            // The result of a task-list tool is a plan item, not a tool row.
            if !plan_result(value, blocks, out) {
                for block in blocks.iter().filter(|b| kind(b) == Some("tool_result")) {
                    tool_result(block, out);
                }
            }
        }
        _ => {}
    }
}

/// What the person typed, or the end of a turn the person interrupted. Injected text gives nothing, except the
/// `!` shell commands the person ran (`shell_call`, `shell_result`).
fn user_text(text: &str, value: &Value, out: &mut Line) {
    let text = text.trim();
    if text.starts_with(INTERRUPTED) {
        out.push(|id, at| ChatItem::TurnEnd { id, at, outcome: TurnOutcome::Interrupted, duration_ms: None });
    } else if text.starts_with("<bash-input>") {
        shell_call(tagged(text, "bash-input").unwrap_or(""), value, out);
    } else if text.starts_with("<bash-stdout>") || text.starts_with("<bash-stderr>") {
        shell_result(text, value, out);
    } else if !text.is_empty() && !INJECTED.iter().any(|prefix| text.starts_with(*prefix)) {
        let text = text.to_string();
        out.push(|id, at| ChatItem::User { id, at, text });
    }
}

/// A `!` command the person ran: a `Shell` tool call with the command as its summary and input. The output line that
/// follows names this line as its `parentUuid`, which gives both lines the same call id (`shell:<uuid>`).
fn shell_call(command: &str, value: &Value, out: &mut Line) {
    let call_id = format!("shell:{}", line_uuid(value, out.offset));
    let input = json!({"command": command.trim()});
    let summary = summary(Some(&input));
    let detail = tool_detail(SHELL, Some(&input));
    out.push(|id, at| ChatItem::ToolCall { id, at, call_id, name: SHELL.to_string(), summary, detail });
}

/// The output of a `!` command: `<bash-stdout>` then `<bash-stderr>`, with terminal codes removed. It is the result of
/// the call its `parentUuid` names. An output line without that link gets a call of its own, so it still shows.
fn shell_result(text: &str, value: &Value, out: &mut Line) {
    let stdout = tagged(text, "bash-stdout").unwrap_or("");
    let stderr = tagged(text, "bash-stderr").unwrap_or("");
    let mut output = String::new();
    for part in [stdout, stderr].iter().map(|part| part.trim_end()).filter(|part| !part.is_empty()) {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(part);
    }
    let output = cut(&strip_ansi(&output));
    let ok = stderr.trim().is_empty();
    let call_id = match value.get("parentUuid").and_then(Value::as_str) {
        Some(parent) => format!("shell:{parent}"),
        None => {
            let own = format!("shell:{}", line_uuid(value, out.offset));
            let call = own.clone();
            out.push(|id, at| ChatItem::ToolCall {
                id,
                at,
                call_id: call,
                name: SHELL.to_string(),
                summary: String::new(),
                detail: ToolDetail::default(),
            });
            own
        }
    };
    out.push(|id, at| ChatItem::ToolResult { id, at, call_id, ok, output });
}

/// The line's `uuid`, or its byte offset when the line has none.
fn line_uuid(value: &Value, offset: u64) -> String {
    value.get("uuid").and_then(Value::as_str).map_or_else(|| offset.to_string(), str::to_string)
}

/// The text between `<tag>` and the `</tag>` after it, when `text` has both.
fn tagged<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{tag}>"))? + start;
    Some(&text[start..end])
}

/// `text` without terminal escape codes: `ESC [ ... final` (colours, cursor moves), `ESC ] ... BEL` or
/// `ESC ] ... ESC \` (window titles, links), and any other `ESC x` pair.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // Parameters and intermediates, then one final byte from 0x40 to 0x7E.
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

fn tool_result(block: &Value, out: &mut Line) {
    let Some(call_id) = block.get("tool_use_id").and_then(Value::as_str) else {
        return;
    };
    let call_id = call_id.to_string();
    let ok = !flag(block, "is_error");
    let output = cut(&result_text(block.get("content")));
    out.push(|id, at| ChatItem::ToolResult { id, at, call_id, ok, output });
}

fn assistant_line(value: &Value, out: &mut Line) {
    if flag(value, "isSidechain") {
        return;
    }
    let Some(message) = value.get("message") else {
        return;
    };
    let model = message.get("model").and_then(Value::as_str).map(str::to_string);
    // `requestedModel` is the name the session asked for, with a `[1m]` suffix when it uses the 1M window.
    let requested = value.get("requestedModel").and_then(Value::as_str);
    let ended = message.get("stop_reason").and_then(Value::as_str) == Some("end_turn");
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for block in blocks {
            match kind(block) {
                Some("text") => {
                    let Some(text) = block.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) else {
                        continue;
                    };
                    let text = text.to_string();
                    let model = model.clone();
                    out.push(|id, at| ChatItem::Assistant { id, at, text, model, final_answer: ended });
                }
                Some("tool_use") => tool_call(block, out),
                _ => {} // thinking blocks are stored empty; anything else is not chat
            }
        }
    }
    if ended {
        out.push(|id, at| ChatItem::TurnEnd { id, at, outcome: TurnOutcome::Completed, duration_ms: None });
    }
    if let Some(usage) = message.get("usage").filter(|u| u.is_object()) {
        let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        let input_tokens = number("input_tokens")
            .saturating_add(number("cache_read_input_tokens"))
            .saturating_add(number("cache_creation_input_tokens"));
        let output_tokens = number("output_tokens");
        let window = Some(context_window(&[model.as_deref(), requested], input_tokens));
        out.push(|id, at| ChatItem::Usage { id, at, input_tokens, output_tokens, context_window: window });
    }
}

fn tool_call(block: &Value, out: &mut Line) {
    let Some(call_id) = block.get("id").and_then(Value::as_str) else {
        return;
    };
    let call_id = call_id.to_string();
    let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let input = block.get("input");
    if plan_call(&name, input, out) {
        return;
    }
    let summary = summary(input);
    let detail = tool_detail(&name, input);
    out.push(|id, at| ChatItem::ToolCall { id, at, call_id, name, summary, detail });
}

/// The first input field that holds text, as its first line cut to `SUMMARY_CHARS`.
fn summary(input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    let text = SUMMARY_KEYS
        .iter()
        .filter_map(|key| input.get(*key).and_then(Value::as_str))
        .find(|text| !text.trim().is_empty())
        .unwrap_or("");
    let line = text.trim().lines().next().unwrap_or("");
    match line.char_indices().nth(SUMMARY_CHARS) {
        None => line.to_string(),
        Some((end, _)) => format!("{}…", &line[..end]),
    }
}

/// The `text` of each `{type: "text"}` block, in order. Images and other blocks are left out.
fn texts(blocks: &[Value]) -> Vec<&str> {
    blocks
        .iter()
        .filter(|b| kind(b) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect()
}

/// A tool result's text: a plain string, or the text blocks of an array joined by newlines.
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => texts(blocks).join("\n"),
        _ => String::new(),
    }
}

/// Claude Code's folder name for a working folder: every character that is not an ASCII letter
/// or digit becomes `-`. `C:\Users\x\proj` gives `C--Users-x-proj`.
pub fn encode_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// `<config>/projects/<encoded cwd>`.
pub fn project_dir(config_dir: &Path, cwd: &Path) -> PathBuf {
    config_dir.join("projects").join(encode_cwd(&cwd.to_string_lossy()))
}

/// `CLAUDE_CONFIG_DIR` when set, else `<home>/.claude`.
pub fn config_dir() -> Option<PathBuf> {
    // The Claude CLI is Node, which takes the home folder from USERPROFILE on Windows.
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    config_dir_from(std::env::var_os("CLAUDE_CONFIG_DIR"), home)
}

fn config_dir_from(env: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match env.filter(|value| !value.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => home.map(|home| home.join(".claude")),
    }
}

/// The newest `*.jsonl` file directly in `dir` that was modified at or after `since`.
/// Subfolders (including folders named `*.jsonl`) are ignored.
pub fn newest_session(dir: &Path, since: SystemTime) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let meta = entry.metadata().ok()?;
            let modified = meta.modified().ok()?;
            let is_session = meta.is_file() && path.extension().is_some_and(|ext| ext == "jsonl");
            (is_session && modified >= since).then_some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// `<dir>/<session id>.jsonl`.
pub fn session_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.jsonl"))
}

#[cfg(test)]
#[path = "session_file_tests.rs"]
mod tests;
