use super::*;

use serde_json::json;

use crate::session_chat::SessionFormat;
use crate::tui::claude::session_file::ClaudeSessionFile;

/// Scrubbed lines in the shapes captured from Claude Code 2.1.311 (task list, Write, Edit, Agent) plus the shapes
/// read from docs (see the module comment). One line per fixture row.
const FIXTURE: &str = include_str!("fixtures/session_tasks.jsonl");

const NOTES: &str = r"C:\Users\you\proj\notes.txt";

fn lines() -> Vec<String> {
    FIXTURE.replace("\r\n", "\n").lines().map(str::to_string).collect()
}

/// The items fixture line `index` gives.
fn items_of(index: usize) -> Vec<ChatItem> {
    ClaudeSessionFile.parse_line(&lines()[index], 0)
}

/// The kinds of `items`, for checking what a line gives at a glance.
fn kinds(items: &[ChatItem]) -> Vec<&'static str> {
    items
        .iter()
        .map(|item| match item {
            ChatItem::User { .. } => "user",
            ChatItem::Assistant { .. } => "assistant",
            ChatItem::ToolCall { .. } => "toolCall",
            ChatItem::ToolResult { .. } => "toolResult",
            ChatItem::TurnEnd { .. } => "turnEnd",
            ChatItem::Usage { .. } => "usage",
            ChatItem::Session { .. } => "session",
            ChatItem::Plan { .. } => "plan",
            ChatItem::Notice { .. } => "notice",
        })
        .collect()
}

fn plan_of(items: &[ChatItem]) -> (bool, Vec<PlanItem>) {
    match items.iter().find(|item| matches!(item, ChatItem::Plan { .. })) {
        Some(ChatItem::Plan { replace, items, .. }) => (*replace, items.clone()),
        _ => panic!("no plan item in {items:?}"),
    }
}

fn notice_of(items: &[ChatItem]) -> (String, String, Option<String>) {
    match items {
        [ChatItem::Notice { notice_kind: kind, text, url, .. }] => (kind.clone(), text.clone(), url.clone()),
        other => panic!("expected one notice, got {other:?}"),
    }
}

fn detail_of(items: &[ChatItem]) -> (String, String, ToolDetail) {
    match items.iter().find(|item| matches!(item, ChatItem::ToolCall { .. })) {
        Some(ChatItem::ToolCall { name, summary, detail, .. }) => (name.clone(), summary.clone(), detail.clone()),
        _ => panic!("no tool call in {items:?}"),
    }
}

#[test]
fn fixture_has_the_lines_these_tests_index() {
    assert_eq!(lines().len(), 23);
    assert!(lines()[16].contains("turn_duration"));
    assert!(lines()[19].contains("pr-link"));
}

#[test]
fn task_create_call_gives_no_tool_row_and_its_result_gives_the_added_task() {
    // line 1: text, the TaskCreate call (swallowed), usage
    assert_eq!(kinds(&items_of(1)), vec!["assistant", "usage"]);
    // line 2: the result has the id; the version line also gives a session item
    let result = items_of(2);
    assert_eq!(kinds(&result), vec!["session", "plan"]);
    let (replace, items) = plan_of(&result);
    assert!(!replace);
    assert_eq!(
        items,
        vec![PlanItem { text: "Define inputs".into(), status: Some(PlanStatus::Pending), task_id: Some("1".into()) }]
    );
}

#[test]
fn task_update_gives_a_status_change_by_task_id_and_its_result_nothing() {
    let call = items_of(5);
    assert_eq!(kinds(&call), vec!["plan", "usage"]);
    let (replace, items) = plan_of(&call);
    assert!(!replace);
    assert_eq!(items, vec![PlanItem { text: String::new(), status: Some(PlanStatus::Completed), task_id: Some("1".into()) }]);
    assert_eq!(kinds(&items_of(6)), vec!["session"]);
}

#[test]
fn task_update_with_nothing_to_change_or_no_id_gives_nothing() {
    let parse = |input: Value| {
        let line = json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t", "name": "TaskUpdate", "input": input}]}});
        ClaudeSessionFile.parse_line(&line.to_string(), 0)
    };
    assert!(parse(json!({"taskId": "1"})).is_empty());
    assert!(parse(json!({"status": "completed"})).is_empty());
    // a rename, and a deletion
    let (_, renamed) = plan_of(&parse(json!({"taskId": "4", "subject": "New name"})));
    assert_eq!(renamed, vec![PlanItem { text: "New name".into(), status: None, task_id: Some("4".into()) }]);
    let (_, deleted) = plan_of(&parse(json!({"taskId": 5, "status": "deleted"})));
    assert_eq!(deleted[0].status, Some(PlanStatus::Deleted));
    assert_eq!(deleted[0].task_id.as_deref(), Some("5"));
}

#[test]
fn todo_write_gives_the_whole_list() {
    let call = items_of(7);
    assert_eq!(kinds(&call), vec!["plan", "usage"]);
    let (replace, items) = plan_of(&call);
    assert!(replace);
    let shape: Vec<(&str, Option<PlanStatus>)> = items.iter().map(|i| (i.text.as_str(), i.status)).collect();
    assert_eq!(
        shape,
        vec![
            ("Define the numbers", Some(PlanStatus::Completed)),
            ("Add them", Some(PlanStatus::InProgress)),
            ("Print the result", Some(PlanStatus::Pending)),
        ]
    );
    assert!(items.iter().all(|item| item.task_id.is_none()));
    // its result row is left out
    assert_eq!(kinds(&items_of(8)), vec!["session"]);
}

#[test]
fn an_empty_todo_write_clears_the_list() {
    let line = json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t", "name": "TodoWrite", "input": {"todos": []}}]}});
    let items = ClaudeSessionFile.parse_line(&line.to_string(), 0);
    assert_eq!(plan_of(&items), (true, Vec::new()));
}

#[test]
fn a_task_lookup_result_is_an_ordinary_tool_result() {
    // `TaskGet` returns the task with a status: not a creation.
    let line = json!({
        "type": "user",
        "message": {"content": [{"type": "tool_result", "tool_use_id": "g1", "content": "Task #1: Define inputs"}]},
        "toolUseResult": {"task": {"id": "1", "subject": "Define inputs", "status": "pending"}}
    });
    let items = ClaudeSessionFile.parse_line(&line.to_string(), 0);
    assert_eq!(kinds(&items), vec!["toolResult"]);
}

#[test]
fn a_created_task_is_read_from_the_result_text_when_the_structured_result_is_missing() {
    let line = json!({
        "type": "user",
        "message": {"content": [{"type": "tool_result", "tool_use_id": "c1", "content": "Task #12 created successfully: Ship it"}]}
    });
    let (replace, items) = plan_of(&ClaudeSessionFile.parse_line(&line.to_string(), 0));
    assert!(!replace);
    assert_eq!(items[0].task_id.as_deref(), Some("12"));
    assert_eq!(items[0].text, "Ship it");
    assert_eq!(created_text("Task #x created successfully: a"), None);
    assert_eq!(created_text("nothing"), None);
}

#[test]
fn write_and_edit_keep_their_input_and_paths_and_are_marked_as_writes() {
    let (name, summary, write) = detail_of(&items_of(9));
    assert_eq!((name.as_str(), summary.as_str()), ("Write", NOTES));
    assert_eq!(write.paths, vec![NOTES.to_string()]);
    assert!(write.writes && !write.helper);
    assert_eq!(write.input.as_ref().unwrap()["content"], "alpha");

    let (_, _, edit) = detail_of(&items_of(11));
    assert_eq!(edit.paths, vec![NOTES.to_string()]);
    assert!(edit.writes);
    let input = edit.input.unwrap();
    assert_eq!((input["old_string"].as_str(), input["new_string"].as_str()), (Some("alpha"), Some("beta")));
}

#[test]
fn a_read_names_its_file_but_is_not_a_write_and_a_search_names_no_file() {
    let detail = |name: &str, input: Value| tool_detail(name, Some(&input));
    let read = detail("Read", json!({"file_path": NOTES}));
    assert_eq!((read.paths.len(), read.writes), (1, false));
    let grep = detail("Grep", json!({"pattern": "x", "path": r"C:\Users\you\proj"}));
    assert!(grep.paths.is_empty() && !grep.writes);
    let multi = detail("MultiEdit", json!({"file_path": NOTES, "edits": [{"old_string": "a", "new_string": "b"}]}));
    assert!(multi.writes);
    assert!(multi.input.unwrap()["edits"].is_array());
    let bash = detail("Bash", json!({"command": "ls"}));
    assert!(bash.paths.is_empty() && !bash.writes && !bash.helper);
}

#[test]
fn agent_and_task_calls_are_helpers() {
    let (name, summary, detail) = detail_of(&items_of(13));
    assert_eq!((name.as_str(), summary.as_str()), ("Agent", "Reply with hi"));
    assert!(detail.helper && !detail.writes);
    assert_eq!(detail.input.unwrap()["subagent_type"], "general-purpose");
    assert!(tool_detail("Task", None).helper);
    assert!(!tool_detail("TaskGet", None).helper);
}

#[test]
fn turn_duration_ends_the_turn_with_its_length() {
    let items = items_of(16);
    assert!(matches!(
        items.as_slice(),
        [ChatItem::TurnEnd { outcome: TurnOutcome::Completed, duration_ms: Some(9500), .. }]
    ));
    // a line without the field still ends the turn, with no length
    let bare = ClaudeSessionFile.parse_line(r#"{"type":"system","subtype":"turn_duration"}"#, 0);
    assert!(matches!(bare.as_slice(), [ChatItem::TurnEnd { duration_ms: None, .. }]));
}

#[test]
fn compaction_is_a_notice_with_the_size_before() {
    assert_eq!(notice_of(&items_of(17)), ("compacted".into(), "Context compacted (was 167,000 tokens)".into(), None));
    let bare = ClaudeSessionFile.parse_line(r#"{"type":"system","subtype":"compact_boundary"}"#, 0);
    assert_eq!(notice_of(&bare).1, "Context compacted");
}

#[test]
fn an_api_error_is_a_notice_with_status_message_and_retry() {
    assert_eq!(notice_of(&items_of(18)), ("error".into(), "API error 529: Overloaded (retry 2 of 10)".into(), None));
    let bare = ClaudeSessionFile.parse_line(r#"{"type":"system","subtype":"api_error"}"#, 0);
    assert_eq!(notice_of(&bare).1, "API error");
    let content = ClaudeSessionFile.parse_line(r#"{"type":"system","subtype":"api_error","content":"Request failed"}"#, 0);
    assert_eq!(notice_of(&content).1, "API error: Request failed");
}

#[test]
fn a_pull_request_link_is_a_notice_with_the_url() {
    assert_eq!(
        notice_of(&items_of(19)),
        ("pr".into(), "Pull request #12 in you/proj".into(), Some("https://github.com/you/proj/pull/12".into()))
    );
    let only_url = ClaudeSessionFile.parse_line(r#"{"type":"pr-link","prUrl":"https://example.test/p/1"}"#, 0);
    assert_eq!(notice_of(&only_url), ("pr".into(), "Pull request".into(), Some("https://example.test/p/1".into())));
    assert!(ClaudeSessionFile.parse_line(r#"{"type":"pr-link"}"#, 0).is_empty());
}

#[test]
fn both_title_lines_are_title_notices() {
    assert_eq!(notice_of(&items_of(20)), ("title".into(), "Add two numbers".into(), None));
    assert_eq!(notice_of(&items_of(21)), ("title".into(), "Adding numbers".into(), None));
    assert!(ClaudeSessionFile.parse_line(r#"{"type":"custom-title","customTitle":"  "}"#, 0).is_empty());
}

#[test]
fn the_summary_written_after_compacting_is_not_a_user_message() {
    assert_eq!(kinds(&items_of(22)), vec!["session"]);
}

#[test]
fn unknown_system_subtypes_give_nothing() {
    for subtype in ["informational", "away_summary", "stop_hook_summary", "local_command", ""] {
        let line = json!({"type": "system", "subtype": subtype}).to_string();
        assert!(ClaudeSessionFile.parse_line(&line, 0).is_empty(), "{subtype}");
    }
    assert!(ClaudeSessionFile.parse_line(r#"{"type":"system"}"#, 0).is_empty());
}

#[test]
fn usage_carries_the_context_window_of_the_model() {
    let usage_window = |items: Vec<ChatItem>| {
        items.into_iter().find_map(|item| match item {
            ChatItem::Usage { context_window, .. } => Some(context_window),
            _ => None,
        })
    };
    assert_eq!(usage_window(items_of(15)), Some(Some(200_000)));
    let long = json!({
        "type": "assistant",
        "requestedModel": "claude-opus-5-5[1m]",
        "message": {"model": "claude-opus-5-5", "content": [], "usage": {"input_tokens": 10, "output_tokens": 1}}
    });
    assert_eq!(usage_window(ClaudeSessionFile.parse_line(&long.to_string(), 0)), Some(Some(1_000_000)));
}

#[test]
fn context_window_rules() {
    assert_eq!(context_window(&[Some("claude-sonnet-5-5"), None], 5_000), 200_000);
    assert_eq!(context_window(&[None, None], 0), 200_000);
    assert_eq!(context_window(&[Some("claude-sonnet-5-5[1M]")], 5_000), 1_000_000);
    // a call that read more than 200k tokens must have had the 1M window
    assert_eq!(context_window(&[Some("claude-sonnet-5-5")], 250_000), 1_000_000);
}

#[test]
fn big_edit_input_is_bounded() {
    let input = json!({"file_path": NOTES, "old_string": "a".repeat(9000), "new_string": "b".repeat(9000)});
    let detail = tool_detail("Edit", Some(&input));
    assert!(detail.input.unwrap().to_string().chars().count() <= crate::session_chat::detail::MAX_INPUT_CHARS);
    assert_eq!(detail.paths, vec![NOTES.to_string()]);
}

#[test]
fn numbers_get_thousands_commas() {
    assert_eq!(with_commas(0), "0");
    assert_eq!(with_commas(999), "999");
    assert_eq!(with_commas(1000), "1,000");
    assert_eq!(with_commas(1234567), "1,234,567");
}
