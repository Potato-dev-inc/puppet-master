//! Codex CLI rollout files, the session files `codex` appends to while a thread runs:
//! `<codex home>/sessions/YYYY/MM/DD/rollout-<local time>-<thread id>.jsonl`, one `{timestamp, type, payload}`
//! object per line, written as each event completes.
//!
//! Only the events a chat view needs are read. `response_item` lines are skipped: they repeat the
//! `event_msg` items and also hold injected context. Finished sessions are compressed to
//! `.jsonl.zst`; they are read the same way, decompressed by `session_chat::tail`.

use std::collections::HashSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use serde_json::Value;

use crate::session_chat::tail::open_lines;
use crate::session_chat::{cut, ChatItem, SessionFormat, TurnOutcome};

/// The Codex rollout format, for `codex` panes.
pub struct CodexRollout;

impl SessionFormat for CodexRollout {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        let Ok(root) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let mut out = Emit { offset, at: str_field(&root, "timestamp"), items: Vec::new() };
        let payload = &root["payload"];
        match root["type"].as_str() {
            Some("session_meta") => session_meta(&mut out, payload),
            Some("turn_context") => turn_context(&mut out, payload),
            Some("event_msg") => event(&mut out, payload),
            _ => {}
        }
        out.items
    }
}

/// The Codex home: `CODEX_HOME` when set, else `<home>/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => crate::project_path::home_dir().map(|home| home.join(".codex")),
    }
}

/// Rollouts modified at or after `since`, in the day folders for the local date of `since`, the day before it
/// and the day after it (a file is written into the folder of the day it started, and a session can start just
/// before midnight). Plain `rollout-*.jsonl` files and finished `rollout-*.jsonl.zst` files are both taken; when
/// a session has both, the plain file is taken. Sorted by path.
pub fn recent_rollouts(home: &Path, since: SystemTime) -> Vec<PathBuf> {
    let sessions = home.join("sessions");
    let today = DateTime::<Local>::from(since).date_naive();
    let days = [today.pred_opt(), Some(today), today.succ_opt()];
    let mut found = Vec::new();
    for day in days.into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(sessions.join(day.format("%Y/%m/%d").to_string())) else {
            continue;
        };
        let mut folder: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("rollout-") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))) {
                continue;
            }
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
            if modified.is_some_and(|modified| modified >= since) {
                folder.push((name, entry.path()));
            }
        }
        let plain: HashSet<String> =
            folder.iter().filter(|(name, _)| name.ends_with(".jsonl")).map(|(name, _)| name.clone()).collect();
        found.extend(
            folder
                .into_iter()
                .filter(|(name, _)| !name.strip_suffix(".zst").is_some_and(|stem| plain.contains(stem)))
                .map(|(_, path)| path),
        );
    }
    found.sort();
    found
}

/// The `cwd` and thread `id` from a rollout's first line (`session_meta`), or `None` when it has none.
pub fn rollout_meta(path: &Path) -> Option<(String, String)> {
    let mut first = String::new();
    open_lines(path).ok()?.read_line(&mut first).ok()?;
    let root: Value = serde_json::from_str(&first).ok()?;
    if root["type"].as_str()? != "session_meta" {
        return None;
    }
    let payload = &root["payload"];
    let cwd = str_field(payload, "cwd")?;
    let id = str_field(payload, "id").or_else(|| str_field(payload, "session_id"))?;
    Some((cwd, id))
}

/// When a rollout started: the `timestamp` of its first line (`session_meta`), or `None` when it has none.
pub fn rollout_started(path: &Path) -> Option<SystemTime> {
    let mut first = String::new();
    open_lines(path).ok()?.read_line(&mut first).ok()?;
    let root: Value = serde_json::from_str(&first).ok()?;
    let stamp = DateTime::parse_from_rfc3339(root["timestamp"].as_str()?).ok()?;
    Some(SystemTime::from(stamp))
}

/// The items of one line. Ids are `<line offset>:<index in the line>`, so they are stable across re-reads.
struct Emit {
    offset: u64,
    at: Option<String>,
    items: Vec<ChatItem>,
}

impl Emit {
    fn push(&mut self, build: impl FnOnce(String, Option<String>) -> ChatItem) {
        let id = format!("{}:{}", self.offset, self.items.len());
        let at = self.at.clone();
        self.items.push(build(id, at));
    }
}

/// The first line of a thread: the CLI version.
fn session_meta(out: &mut Emit, payload: &Value) {
    let cli_version = str_field(payload, "cli_version");
    out.push(|id, at| ChatItem::Session { id, at, model: None, cli_version });
}

fn turn_context(out: &mut Emit, payload: &Value) {
    if let Some(model) = str_field(payload, "model") {
        out.push(|id, at| ChatItem::Session { id, at, model: Some(model), cli_version: None });
    }
}

fn event(out: &mut Emit, payload: &Value) {
    match payload["type"].as_str() {
        Some("item_completed") => item_completed(out, &payload["item"]),
        Some("task_complete") => turn_end(out, TurnOutcome::Completed, payload),
        Some("turn_aborted") => turn_end(out, TurnOutcome::Interrupted, payload),
        Some("token_count") => usage(out, payload),
        _ => {} // task_started and the rest
    }
}

fn item_completed(out: &mut Emit, item: &Value) {
    match item["type"].as_str() {
        Some("UserMessage") => {
            let text = content_text(item);
            out.push(|id, at| ChatItem::User { id, at, text });
        }
        Some("AgentMessage") => {
            let text = content_text(item);
            let final_answer = item["phase"] == "final_answer";
            out.push(|id, at| ChatItem::Assistant { id, at, text, model: None, final_answer });
        }
        Some("CommandExecution") => command(out, item),
        Some("FileChange") => file_change(out, item),
        Some("McpToolCall") => mcp_tool(out, item),
        Some("WebSearch") => {
            let call_id = call_id(item);
            let summary = str_field(item, "query").unwrap_or_default();
            out.push(|id, at| ChatItem::ToolCall {
                id,
                at,
                call_id,
                name: "web_search".into(),
                summary,
            });
        }
        _ => {} // Reasoning and anything else
    }
}

fn command(out: &mut Emit, item: &Value) {
    let call_id = call_id(item);
    let summary = command_line(&item["command"]);
    out.push(|id, at| ChatItem::ToolCall {
        id,
        at,
        call_id: call_id.clone(),
        name: "exec".into(),
        summary,
    });
    let output = str_field(item, "aggregated_output")
        .or_else(|| str_field(item, "aggregatedOutput"))
        .unwrap_or_default();
    result(out, item, &call_id, output);
}

fn file_change(out: &mut Emit, item: &Value) {
    let call_id = call_id(item);
    let summary = changed_paths(item).join(", ");
    out.push(|id, at| ChatItem::ToolCall {
        id,
        at,
        call_id: call_id.clone(),
        name: "apply_patch".into(),
        summary,
    });
    result(out, item, &call_id, String::new());
}

fn mcp_tool(out: &mut Emit, item: &Value) {
    let call_id = call_id(item);
    let name = match (str_field(item, "server"), str_field(item, "tool")) {
        (Some(server), Some(tool)) => format!("{server}.{tool}"),
        _ => "mcp".to_string(),
    };
    out.push(|id, at| ChatItem::ToolCall {
        id,
        at,
        call_id: call_id.clone(),
        name,
        summary: String::new(),
    });
    result(out, item, &call_id, String::new());
}

/// A result row, only once the item has finished (`completed` or `failed`). A running item has none.
/// `ok` needs a `completed` status and an exit code of 0 (or no exit code: file changes have none).
fn result(out: &mut Emit, item: &Value, call_id: &str, output: String) {
    let status = str_field(item, "status");
    if !matches!(status.as_deref(), Some("completed" | "failed")) {
        return;
    }
    let ok = status.as_deref() == Some("completed") && exit_ok(item);
    let output = cut(&output);
    let call_id = call_id.to_string();
    out.push(|id, at| ChatItem::ToolResult { id, at, call_id, ok, output });
}

fn turn_end(out: &mut Emit, outcome: TurnOutcome, payload: &Value) {
    let duration_ms = payload["duration_ms"].as_u64();
    out.push(|id, at| ChatItem::TurnEnd { id, at, outcome, duration_ms });
}

/// Per-turn token use. A `null` info (a rate-limit-only event) gives nothing.
fn usage(out: &mut Emit, payload: &Value) {
    let last = &payload["info"]["last_token_usage"];
    if !last.is_object() {
        return;
    }
    let input_tokens = last["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = last["output_tokens"].as_u64().unwrap_or(0);
    let context_window = payload["info"]["model_context_window"].as_u64();
    out.push(|id, at| ChatItem::Usage { id, at, input_tokens, output_tokens, context_window });
}

/// The id that pairs a call with its result: the item's `id`, else its `call_id`.
fn call_id(item: &Value) -> String {
    str_field(item, "id").or_else(|| str_field(item, "call_id")).unwrap_or_default()
}

/// The command as one line. `command` is an array of words (joined with spaces) or a plain string.
fn command_line(command: &Value) -> String {
    let line = match command {
        Value::String(text) => text.clone(),
        Value::Array(words) => words.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    };
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The paths a file change touches, sorted. `changes` is keyed by path, or is a list with a `path` each.
fn changed_paths(item: &Value) -> Vec<String> {
    let mut paths: Vec<String> = match &item["changes"] {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(list) => list.iter().filter_map(|change| str_field(change, "path")).collect(),
        _ => str_field(item, "path").into_iter().collect(),
    };
    paths.sort();
    paths
}

/// The text of a message: the `text` of each content part, joined with newlines.
fn content_text(item: &Value) -> String {
    match &item["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            parts.iter().filter_map(|part| str_field(part, "text")).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    }
}

/// `exit_code` is 0, or absent.
fn exit_ok(item: &Value) -> bool {
    let code = item["exit_code"].as_i64().or_else(|| item["exitCode"].as_i64());
    code.map_or(true, |code| code == 0)
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_chat::items::read_lines;
    use serde_json::json;
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};

    /// The fixture as the reader sees it: LF endings (CRLF checkouts are normalised), each line with its byte offset.
    fn fixture_lines() -> Vec<(u64, String)> {
        let text = include_str!("fixtures/rollout_basic.jsonl").replace("\r\n", "\n");
        let mut offset = 0u64;
        let mut lines = Vec::new();
        for line in text.split_inclusive('\n') {
            lines.push((offset, line.trim_end_matches('\n').to_string()));
            offset += line.len() as u64;
        }
        lines
    }

    #[test]
    fn fixture_gives_the_expected_items_in_order() {
        let lines = fixture_lines();
        assert_eq!(lines.len(), 16);
        let id = |line: usize, i: usize| format!("{}:{}", lines[line].0, i);
        let at = |line: usize| Some(format!("2026-10-27T09:00:{line:02}.000Z"));
        let expected = vec![
            ChatItem::Session { id: id(0, 0), at: at(0), model: None, cli_version: Some("0.162.1".into()) },
            ChatItem::Session { id: id(1, 0), at: at(1), model: Some("gpt-test-1".into()), cli_version: None },
            ChatItem::User { id: id(3, 0), at: at(3), text: "Add a retry to the upload step.".into() },
            ChatItem::Assistant {
                id: id(4, 0),
                at: at(4),
                text: "I will read the upload code first.".into(),
                model: None,
                final_answer: false,
            },
            // in_progress: the call only
            ChatItem::ToolCall {
                id: id(5, 0),
                at: at(5),
                call_id: "cmd-1".into(),
                name: "exec".into(),
                summary: "rg upload src".into(),
            },
            // completed: the call again, then its result
            ChatItem::ToolCall {
                id: id(6, 0),
                at: at(6),
                call_id: "cmd-1".into(),
                name: "exec".into(),
                summary: "rg upload src".into(),
            },
            ChatItem::ToolResult {
                id: id(6, 1),
                at: at(6),
                call_id: "cmd-1".into(),
                ok: true,
                output: "src\\upload.rs:12:fn upload() {".into(),
            },
            // failed: exit code 101
            ChatItem::ToolCall {
                id: id(7, 0),
                at: at(7),
                call_id: "cmd-2".into(),
                name: "exec".into(),
                summary: "cargo test".into(),
            },
            ChatItem::ToolResult {
                id: id(7, 1),
                at: at(7),
                call_id: "cmd-2".into(),
                ok: false,
                output: "error[E0432]: unresolved import".into(),
            },
            ChatItem::ToolCall {
                id: id(8, 0),
                at: at(8),
                call_id: "fc-1".into(),
                name: "apply_patch".into(),
                summary: "src\\retry.rs, src\\upload.rs".into(),
            },
            ChatItem::ToolResult {
                id: id(8, 1),
                at: at(8),
                call_id: "fc-1".into(),
                ok: true,
                output: String::new(),
            },
            ChatItem::Assistant {
                id: id(9, 0),
                at: at(9),
                text: "Retry added. The upload now tries three times.".into(),
                model: None,
                final_answer: true,
            },
            // token_count with null info (line 10) gives nothing
            ChatItem::Usage {
                id: id(11, 0),
                at: at(11),
                input_tokens: 1200,
                output_tokens: 340,
                context_window: Some(258_000),
            },
            ChatItem::TurnEnd { id: id(12, 0), at: at(12), outcome: TurnOutcome::Completed, duration_ms: Some(4200) },
            // response_item (line 13) gives nothing
            ChatItem::TurnEnd {
                id: id(14, 0),
                at: at(14),
                outcome: TurnOutcome::Interrupted,
                duration_ms: Some(900),
            },
            // the half line at the end (line 15) gives nothing
        ];
        assert_eq!(read_lines(&CodexRollout, &lines), expected);
    }

    #[test]
    fn unknown_broken_and_noise_lines_give_nothing() {
        let noise = [
            "",
            "not json",
            "42",
            "null",
            r#"{"type":"response_item","payload":{"type":"reasoning"}}"#,
            r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
            r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","id":"r-1"}}}"#,
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#,
            r#"{"type":"turn_context","payload":{"cwd":"C:\\Users\\you\\proj"}}"#,
        ];
        for line in noise {
            assert!(CodexRollout.parse_line(line, 0).is_empty(), "{line}");
        }
    }

    #[test]
    fn camel_case_fields_mcp_calls_web_searches_and_long_output() {
        let command = json!({"timestamp": "t", "type": "event_msg", "payload": {"type": "item_completed",
            "item": {"type": "CommandExecution", "id": "c-9", "command": "npm   test", "status": "completed",
                     "exitCode": 3, "aggregatedOutput": "boom"}}});
        assert_eq!(
            CodexRollout.parse_line(&command.to_string(), 7),
            vec![
                ChatItem::ToolCall {
                    id: "7:0".into(),
                    at: Some("t".into()),
                    call_id: "c-9".into(),
                    name: "exec".into(),
                    summary: "npm test".into(),
                },
                ChatItem::ToolResult {
                    id: "7:1".into(),
                    at: Some("t".into()),
                    call_id: "c-9".into(),
                    ok: false,
                    output: "boom".into(),
                },
            ]
        );

        let mcp = json!({"timestamp": "t", "type": "event_msg", "payload": {"type": "item_completed",
            "item": {"type": "McpToolCall", "id": "m-1", "server": "pencil", "tool": "get_screenshot",
                     "status": "completed"}}});
        let items = CodexRollout.parse_line(&mcp.to_string(), 0);
        assert_eq!(
            items[0],
            ChatItem::ToolCall {
                id: "0:0".into(),
                at: Some("t".into()),
                call_id: "m-1".into(),
                name: "pencil.get_screenshot".into(),
                summary: String::new(),
            }
        );
        assert!(matches!(items[1], ChatItem::ToolResult { ok: true, .. }));

        let bare_mcp = json!({"type": "event_msg", "payload": {"type": "item_completed",
            "item": {"type": "McpToolCall", "id": "m-2", "tool": "x", "status": "in_progress"}}});
        let items = CodexRollout.parse_line(&bare_mcp.to_string(), 0);
        assert_eq!(items.len(), 1, "a running MCP call has no result");
        assert!(matches!(&items[0], ChatItem::ToolCall { name, .. } if name == "mcp"));

        let search = json!({"type": "event_msg", "payload": {"type": "item_completed",
            "item": {"type": "WebSearch", "id": "w-1", "query": "rust chrono local time"}}});
        assert_eq!(
            CodexRollout.parse_line(&search.to_string(), 3),
            vec![ChatItem::ToolCall {
                id: "3:0".into(),
                at: None,
                call_id: "w-1".into(),
                name: "web_search".into(),
                summary: "rust chrono local time".into(),
            }]
        );

        let long = json!({"type": "event_msg", "payload": {"type": "item_completed",
            "item": {"type": "CommandExecution", "id": "c-1", "command": ["cat", "big"], "status": "completed",
                     "exit_code": 0, "aggregated_output": "x".repeat(5000)}}});
        let items = CodexRollout.parse_line(&long.to_string(), 0);
        let ChatItem::ToolResult { output, ok, .. } = &items[1] else {
            panic!("expected a result, got {items:?}");
        };
        assert!(*ok);
        assert_eq!(output.chars().count(), 4001);
        assert!(output.ends_with('…'));
    }

    #[test]
    fn file_change_paths_come_from_a_keyed_object_or_a_list() {
        let list = json!({"type": "event_msg", "payload": {"type": "item_completed",
            "item": {"type": "FileChange", "id": "f-1", "status": "failed",
                     "changes": [{"path": "b.rs"}, {"path": "a.rs"}]}}});
        let items = CodexRollout.parse_line(&list.to_string(), 0);
        assert_eq!(
            items,
            vec![
                ChatItem::ToolCall {
                    id: "0:0".into(),
                    at: None,
                    call_id: "f-1".into(),
                    name: "apply_patch".into(),
                    summary: "a.rs, b.rs".into(),
                },
                ChatItem::ToolResult {
                    id: "0:1".into(),
                    at: None,
                    call_id: "f-1".into(),
                    ok: false,
                    output: String::new(),
                },
            ]
        );
    }

    #[test]
    fn rollout_meta_reads_cwd_and_id_from_the_first_line() {
        let home = temp_home("meta");
        let meta = r#"{"timestamp":"2026-10-27T09:00:00.000Z","type":"session_meta","payload":{"id":"0198a000-0000-7000-8000-00000000c0de","cwd":"C:\\Users\\you\\proj"}}"#;
        let expected = Some(("C:\\Users\\you\\proj".to_string(), "0198a000-0000-7000-8000-00000000c0de".to_string()));

        let with_crlf = home.join("rollout-crlf.jsonl");
        fs::write(&with_crlf, format!("{meta}\r\n{{\"type\":\"turn_context\"}}\r\n")).unwrap();
        assert_eq!(rollout_meta(&with_crlf), expected);

        let not_meta = home.join("rollout-other.jsonl");
        fs::write(&not_meta, "{\"type\":\"turn_context\",\"payload\":{\"model\":\"m\"}}\n").unwrap();
        assert_eq!(rollout_meta(&not_meta), None);

        let empty = home.join("rollout-empty.jsonl");
        fs::write(&empty, "").unwrap();
        assert_eq!(rollout_meta(&empty), None);

        assert_eq!(rollout_meta(&home.join("missing.jsonl")), None);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn recent_rollouts_takes_rollouts_from_nearby_day_folders_modified_since() {
        let home = temp_home("recent");
        let now = SystemTime::now();
        let since = now - Duration::from_secs(3600);
        let two_hours_ago = now - Duration::from_secs(2 * 3600);
        let today = DateTime::<Local>::from(since).date_naive();
        let day = |days: i64| {
            let date = today + chrono::Duration::days(days);
            home.join("sessions").join(date.format("%Y/%m/%d").to_string())
        };

        write(&day(-1).join("rollout-yesterday.jsonl"), now);
        write(&day(0).join("rollout-today.jsonl"), now);
        write(&day(0).join("rollout-old.jsonl"), two_hours_ago);
        write(&day(0).join("rollout-cut.jsonl.zst"), now);
        write(&day(0).join("notes.txt"), now);
        write(&day(1).join("rollout-tomorrow.jsonl"), now);
        write(&day(-5).join("rollout-stale.jsonl"), now);

        let names: Vec<String> = recent_rollouts(&home, since)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["rollout-yesterday.jsonl", "rollout-cut.jsonl.zst", "rollout-today.jsonl", "rollout-tomorrow.jsonl"]
        );
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_session_with_a_plain_and_a_zst_file_is_taken_once_as_the_plain_file() {
        let home = temp_home("recent-both");
        let now = SystemTime::now();
        let since = now - Duration::from_secs(3600);
        let today = DateTime::<Local>::from(since).date_naive();
        let folder = home.join("sessions").join(today.format("%Y/%m/%d").to_string());
        write(&folder.join("rollout-both.jsonl"), now);
        write(&folder.join("rollout-both.jsonl.zst"), now);
        write(&folder.join("rollout-done.jsonl.zst"), now);

        let names: Vec<String> = recent_rollouts(&home, since)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["rollout-both.jsonl", "rollout-done.jsonl.zst"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_zst_rollout_gives_its_cwd_and_id_from_its_first_line() {
        let home = temp_home("meta-zst");
        let folder = home.join("sessions").join("2026").join("10").join("27");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("rollout-2026-10-27T09-00-00-basic.jsonl.zst");
        fs::write(&path, include_bytes!("fixtures/rollout_basic.jsonl.zst")).unwrap();
        assert_eq!(
            rollout_meta(&path),
            Some(("C:\\Users\\you\\proj".to_string(), "0198a000-0000-7000-8000-00000000c0de".to_string()))
        );
        let started = SystemTime::from(DateTime::parse_from_rfc3339("2026-10-27T09:00:00.000Z").unwrap());
        assert_eq!(rollout_started(&path), Some(started));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn the_zst_fixture_gives_the_same_lines_and_items_as_the_plain_fixture() {
        let home = temp_home("zst-fixture");
        let path = home.join("rollout-basic.jsonl.zst");
        fs::write(&path, include_bytes!("fixtures/rollout_basic.jsonl.zst")).unwrap();
        let lines = crate::session_chat::tail::read_all_lines(&path).unwrap();
        assert_eq!(lines, fixture_lines());
        assert_eq!(read_lines(&CodexRollout, &lines), read_lines(&CodexRollout, &fixture_lines()));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn two_zst_frames_in_one_file_decode_to_the_plain_fixture_twice() {
        use std::io::Read;

        let home = temp_home("zst-two-frames");
        let path = home.join("rollout-two.jsonl.zst");
        let frame = include_bytes!("fixtures/rollout_basic.jsonl.zst");
        fs::write(&path, [frame.as_slice(), frame.as_slice()].concat()).unwrap();
        let plain = include_str!("fixtures/rollout_basic.jsonl").replace("\r\n", "\n");

        let mut text = String::new();
        open_lines(&path).unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, plain.repeat(2));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn recent_rollouts_of_a_missing_home_is_empty() {
        let home = temp_home("missing");
        assert!(recent_rollouts(&home.join("nope"), SystemTime::now()).is_empty());
        fs::remove_dir_all(&home).unwrap();
    }

    /// Reads the real rollout named by `PM_REAL_CODEX_ROLLOUT`; run with `cargo test -- --ignored`.
    /// Prints the item kinds only (never the text) and checks the chat a real `codex exec` run must give.
    #[test]
    #[ignore]
    fn a_real_session_file_reads_as_chat() {
        let Some(path) = std::env::var_os("PM_REAL_CODEX_ROLLOUT") else {
            println!("PM_REAL_CODEX_ROLLOUT is not set; skipped");
            return;
        };
        let text = fs::read_to_string(&path).expect("the real rollout can be read");
        let mut offset = 0u64;
        let mut lines = Vec::new();
        for line in text.split_inclusive('\n') {
            lines.push((offset, line.trim_end_matches(['\r', '\n']).to_string()));
            offset += line.len() as u64;
        }
        let items = read_lines(&CodexRollout, &lines);
        let kinds: Vec<&str> = items.iter().map(kind_name).collect();
        println!("{} lines gave {} items", lines.len(), items.len());
        println!("kinds: {}", kinds.join(" "));
        assert!(
            items.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text.contains("chatview-check"))),
            "the prompt is read as a User item"
        );
        assert!(items.iter().any(|item| matches!(item, ChatItem::ToolCall { .. })), "a ToolCall item");
        let call_ids: Vec<&String> = items
            .iter()
            .filter_map(|item| match item {
                ChatItem::ToolCall { call_id, .. } => Some(call_id),
                _ => None,
            })
            .collect();
        let results: Vec<(&String, bool)> = items
            .iter()
            .filter_map(|item| match item {
                ChatItem::ToolResult { call_id, ok, .. } => Some((call_id, *ok)),
                _ => None,
            })
            .collect();
        println!("ToolResult ok values: {:?}", results.iter().map(|(_, ok)| *ok).collect::<Vec<_>>());
        assert!(
            results.iter().any(|(call_id, _)| call_ids.contains(call_id)),
            "a ToolResult whose call_id is the call_id of a ToolCall in the same file"
        );
        assert!(
            items.iter().any(|item| matches!(item, ChatItem::Assistant { text, .. } if text.contains("done"))),
            "an Assistant item whose text contains \"done\""
        );
        assert!(items.iter().any(|item| matches!(item, ChatItem::TurnEnd { .. })), "a TurnEnd item");
    }

    /// The kind of an item, as printed by the real-file test.
    fn kind_name(item: &ChatItem) -> &'static str {
        match item {
            ChatItem::User { .. } => "User",
            ChatItem::Assistant { .. } => "Assistant",
            ChatItem::ToolCall { .. } => "ToolCall",
            ChatItem::ToolResult { .. } => "ToolResult",
            ChatItem::TurnEnd { .. } => "TurnEnd",
            ChatItem::Usage { .. } => "Usage",
            ChatItem::Session { .. } => "Session",
        }
    }

    fn temp_home(name: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-codex-rollout-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes `path` (with its folders) and sets its modified time.
    fn write(path: &Path, modified: SystemTime) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{}\n").unwrap();
        fs::OpenOptions::new().write(true).open(path).unwrap().set_modified(modified).unwrap();
    }
}
