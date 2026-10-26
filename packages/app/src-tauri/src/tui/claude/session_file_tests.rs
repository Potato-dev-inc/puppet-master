use super::*;

use std::fs::File;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::json;

use crate::session_chat::model::MAX_OUTPUT_CHARS;

/// Synthetic session: invented chat, `C:\Users\you\proj`, one broken line. See the fixtures folder.
const FIXTURE: &str = include_str!("fixtures/session_basic.jsonl");

fn fixture() -> String {
    FIXTURE.replace("\r\n", "\n")
}

/// Byte offset where each line of `text` starts (LF endings), and the items every line gives.
fn parse_text(text: &str) -> (Vec<u64>, Vec<ChatItem>) {
    let mut offsets = Vec::new();
    let mut items = Vec::new();
    let mut at = 0u64;
    for line in text.lines() {
        offsets.push(at);
        items.extend(ClaudeSessionFile.parse_line(line, at));
        at += line.len() as u64 + 1;
    }
    (offsets, items)
}

/// The timestamp the fixture gives line `line`.
fn ts(line: usize) -> Option<String> {
    Some(format!("2026-10-27T09:00:{line:02}Z"))
}

/// The id of item `index` in fixture line `line`.
fn id(offsets: &[u64], line: usize, index: usize) -> String {
    format!("{}:{}", offsets[line], index)
}

fn session(offsets: &[u64], line: usize) -> ChatItem {
    ChatItem::Session {
        id: id(offsets, line, 0),
        at: ts(line),
        model: None,
        cli_version: Some("2.1.300".into()),
    }
}

fn model() -> Option<String> {
    Some("claude-sonnet-5-5".into())
}

#[test]
fn fixture_gives_the_exact_chat_sequence() {
    let (o, items) = parse_text(&fixture());
    let want = vec![
        session(&o, 0),
        ChatItem::User { id: id(&o, 0, 1), at: ts(0), text: "list the files".into() },
        session(&o, 1),
        session(&o, 2),
        ChatItem::Assistant {
            id: id(&o, 3, 0),
            at: ts(3),
            text: "I will list the folder.".into(),
            model: model(),
            final_answer: false,
        },
        ChatItem::ToolCall {
            id: id(&o, 3, 1),
            at: ts(3),
            call_id: "toolu_01".into(),
            name: "Bash".into(),
            summary: "dir".into(),
        },
        ChatItem::Usage { id: id(&o, 3, 2), at: ts(3), input_tokens: 1120, output_tokens: 42, context_window: None },
        session(&o, 4),
        ChatItem::ToolResult {
            id: id(&o, 4, 1),
            at: ts(4),
            call_id: "toolu_01".into(),
            ok: true,
            output: "proj.txt\nnotes.md".into(),
        },
        ChatItem::ToolCall {
            id: id(&o, 5, 0),
            at: ts(5),
            call_id: "toolu_02".into(),
            name: "Read".into(),
            summary: r"C:\Users\you\proj\missing.md".into(),
        },
        ChatItem::Usage { id: id(&o, 5, 1), at: ts(5), input_tokens: 1450, output_tokens: 20, context_window: None },
        session(&o, 6),
        ChatItem::ToolResult {
            id: id(&o, 6, 1),
            at: ts(6),
            call_id: "toolu_02".into(),
            ok: false,
            output: "File does not exist.".into(),
        },
        ChatItem::Assistant {
            id: id(&o, 7, 0),
            at: ts(7),
            text: "The folder has one file that is missing.".into(),
            model: model(),
            final_answer: true,
        },
        ChatItem::TurnEnd { id: id(&o, 7, 1), at: ts(7), outcome: TurnOutcome::Completed, duration_ms: None },
        ChatItem::Usage { id: id(&o, 7, 2), at: ts(7), input_tokens: 1810, output_tokens: 64, context_window: None },
        session(&o, 8),
        ChatItem::TurnEnd { id: id(&o, 8, 1), at: ts(8), outcome: TurnOutcome::Interrupted, duration_ms: None },
        session(&o, 12),
        ChatItem::User { id: id(&o, 12, 1), at: ts(12), text: "thanks, that is all".into() },
        session(&o, 13),
        session(&o, 14),
    ];
    assert_eq!(items, want);
}

#[test]
fn fixture_keeps_the_noise_and_broken_lines_the_sequence_depends_on() {
    let text = fixture();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 15);
    assert!(lines[9].contains("\"attachment\""));
    assert!(lines[10].contains("turn_duration"));
    assert!(serde_json::from_str::<Value>(lines[11]).is_err());
}

#[test]
fn noise_and_broken_lines_give_nothing() {
    let parse = |line: &str| ClaudeSessionFile.parse_line(line, 0);
    assert!(parse("").is_empty());
    assert!(parse("null").is_empty());
    assert!(parse("[1,2]").is_empty());
    assert!(parse("\"user\"").is_empty());
    assert!(parse(r#"{"type":"user""#).is_empty());
    assert!(parse(r#"{"type":"user"}"#).is_empty());
    assert!(parse(r#"{"type":"assistant","message":{"content":"plain"}}"#).is_empty());
    assert!(parse(r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#).is_empty());
    assert!(parse(r#"{"type":"attachment","version":"2.1.300"}"#).is_empty());
}

#[test]
fn a_user_line_without_a_version_gives_no_session_item() {
    let line = r#"{"type":"user","message":{"content":"hi"}}"#;
    assert_eq!(
        ClaudeSessionFile.parse_line(line, 7),
        vec![ChatItem::User { id: "7:0".into(), at: None, text: "hi".into() }]
    );
}

#[test]
fn interrupt_written_as_plain_text_also_ends_the_turn() {
    let line = json!({"type": "user", "message": {"content": "[Request interrupted by user for tool use]"}});
    assert_eq!(
        ClaudeSessionFile.parse_line(&line.to_string(), 0),
        vec![ChatItem::TurnEnd { id: "0:0".into(), at: None, outcome: TurnOutcome::Interrupted, duration_ms: None }]
    );
}

#[test]
fn injected_user_text_and_sidechain_assistant_lines_are_left_out() {
    for text in ["<local-command-stdout>ok</local-command-stdout>", "  <system-reminder>x</system-reminder>", "Caveat: x", "   "] {
        let line = json!({"type": "user", "message": {"content": text}}).to_string();
        assert!(ClaudeSessionFile.parse_line(&line, 0).is_empty(), "{text:?}");
    }
    let sidechain = json!({
        "type": "assistant",
        "isSidechain": true,
        "message": {"content": [{"type": "text", "text": "sub"}], "stop_reason": "end_turn"}
    });
    assert!(ClaudeSessionFile.parse_line(&sidechain.to_string(), 0).is_empty());
}

#[test]
fn tool_output_is_cut_to_the_chat_limit() {
    let long = "é".repeat(MAX_OUTPUT_CHARS + 10);
    let line = json!({
        "type": "user",
        "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": long}]}
    });
    let items = ClaudeSessionFile.parse_line(&line.to_string(), 0);
    match items.as_slice() {
        [ChatItem::ToolResult { ok, output, .. }] => {
            assert!(*ok);
            assert_eq!(output.chars().count(), MAX_OUTPUT_CHARS + 1);
            assert!(output.ends_with('…'));
        }
        other => panic!("{other:?}"),
    }
}

fn summary_of(input: Value) -> String {
    let line = json!({
        "type": "assistant",
        "message": {"content": [{"type": "tool_use", "id": "t1", "name": "X", "input": input}]}
    });
    match ClaudeSessionFile.parse_line(&line.to_string(), 0).as_slice() {
        [ChatItem::ToolCall { summary, .. }] => summary.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn tool_summary_is_the_first_known_input_field_on_its_first_line() {
    assert_eq!(summary_of(json!({"command": "git status\necho two", "description": "x"})), "git status");
    assert_eq!(summary_of(json!({"command": "  ", "file_path": "a.rs"})), "a.rs");
    assert_eq!(summary_of(json!({"pattern": "TODO", "path": "src"})), "src");
    assert_eq!(summary_of(json!({"prompt": "first\nsecond"})), "first");
    assert_eq!(summary_of(json!({"unknown": "x"})), "");
    assert_eq!(summary_of(json!({})), "");
}

#[test]
fn tool_summary_is_cut_to_200_characters() {
    let command = format!("{}\nsecond line", "c".repeat(300));
    assert_eq!(summary_of(json!({ "command": command })), format!("{}…", "c".repeat(200)));
}

#[test]
fn encode_cwd_replaces_every_non_alphanumeric_character_with_a_dash() {
    assert_eq!(encode_cwd(r"C:\Users\x\Desktop\work2\puppet-master"), "C--Users-x-Desktop-work2-puppet-master");
    assert_eq!(encode_cwd("/home/a.b_c/é"), "-home-a-b-c--");
}

#[test]
fn project_dir_sits_under_projects_with_the_encoded_cwd() {
    assert_eq!(
        project_dir(Path::new("cfg"), Path::new(r"C:\Users\you\proj")),
        Path::new("cfg").join("projects").join("C--Users-you-proj")
    );
}

#[test]
fn config_dir_prefers_claude_config_dir_then_home() {
    let home = PathBuf::from("home-x");
    assert_eq!(config_dir_from(Some("cfg-y".into()), Some(home.clone())), Some(PathBuf::from("cfg-y")));
    assert_eq!(config_dir_from(None, Some(home.clone())), Some(home.join(".claude")));
    assert_eq!(config_dir_from(Some("".into()), Some(home.clone())), Some(home.join(".claude")));
    assert_eq!(config_dir_from(None, None), None);
}

#[test]
fn session_path_is_the_id_as_a_jsonl_file() {
    assert_eq!(session_path(Path::new("dir"), "abc-123"), Path::new("dir").join("abc-123.jsonl"));
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A folder in the system temp dir, removed when the guard drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pm-claude-session-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Writes a file under the folder and sets its modification time.
    fn file(&self, name: &str, modified: SystemTime) {
        let path = self.0.join(name);
        std::fs::write(&path, b"{}\n").unwrap();
        File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn newest_session_is_the_latest_jsonl_file_modified_since_the_given_time() {
    let scratch = Scratch::new();
    let base = SystemTime::now() - Duration::from_secs(3600);
    scratch.file("older.jsonl", base);
    scratch.file("newer.jsonl", base + Duration::from_secs(10));
    scratch.file("notes.txt", base + Duration::from_secs(20));
    std::fs::create_dir(scratch.path().join("nested")).unwrap();
    std::fs::write(scratch.path().join("nested").join("inner.jsonl"), b"{}\n").unwrap();

    assert_eq!(newest_session(scratch.path(), base), Some(scratch.path().join("newer.jsonl")));
    assert_eq!(newest_session(scratch.path(), base + Duration::from_secs(11)), None);
}

#[test]
fn newest_session_ignores_folders_named_like_session_files() {
    let scratch = Scratch::new();
    std::fs::create_dir(scratch.path().join("folder.jsonl")).unwrap();
    assert_eq!(newest_session(scratch.path(), SystemTime::UNIX_EPOCH), None);
}

#[test]
fn newest_session_of_a_missing_folder_is_none() {
    let scratch = Scratch::new();
    assert_eq!(newest_session(&scratch.path().join("missing"), SystemTime::UNIX_EPOCH), None);
}

/// Reads the real session file named by `PM_REAL_CLAUDE_SESSION`; run with `cargo test -- --ignored`.
/// Prints the item kinds only (never the text) and checks the chat a real `claude -p` run must give.
#[test]
#[ignore]
fn a_real_session_file_reads_as_chat() {
    let Some(path) = std::env::var_os("PM_REAL_CLAUDE_SESSION") else {
        println!("PM_REAL_CLAUDE_SESSION is not set; skipped");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("the real session file can be read");
    let (_, items) = parse_text(&text);
    let kinds: Vec<&str> = items.iter().map(kind_name).collect();
    println!("{} lines gave {} items", text.lines().count(), items.len());
    println!("kinds: {}", kinds.join(" "));
    assert!(
        items.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text.contains("chatview-check"))),
        "the prompt is read as a User item"
    );
    assert!(items.iter().any(|item| matches!(item, ChatItem::ToolCall { .. })), "a ToolCall item");
    assert!(
        items.iter().any(|item| matches!(item, ChatItem::ToolResult { ok: true, .. })),
        "a ToolResult item with ok=true"
    );
    assert!(
        items.iter().any(|item| matches!(item, ChatItem::Assistant { text, .. } if text.contains("done"))),
        "an Assistant item whose text contains \"done\""
    );
    assert!(items.iter().any(|item| matches!(item, ChatItem::TurnEnd { .. })), "a TurnEnd item");
}

/// The kind of an item, as printed by the real-file tests.
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
