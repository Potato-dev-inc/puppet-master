use super::*;

use serde_json::json;

use crate::session_chat::SessionFormat;
use crate::tui::codex::session_file::CodexRollout;

/// Scrubbed lines in the shapes captured from Codex 0.162.1 (FileChange, task_started) plus the plan and note shapes
/// read from docs (see the module comment).
const FIXTURE: &str = include_str!("fixtures/rollout_plan.jsonl");

const HELLO: &str = r"C:\Users\you\proj\hello.txt";

fn items_of(index: usize) -> Vec<ChatItem> {
    let text = FIXTURE.replace("\r\n", "\n");
    let line = text.lines().nth(index).expect("fixture line");
    CodexRollout.parse_line(line, 0)
}

fn plan_of(items: &[ChatItem]) -> (bool, Vec<(String, Option<PlanStatus>)>) {
    match items {
        [ChatItem::Plan { replace, items, .. }] => {
            (*replace, items.iter().map(|item| (item.text.clone(), item.status)).collect())
        }
        other => panic!("expected one plan item, got {other:?}"),
    }
}

#[test]
fn the_plan_tool_call_gives_the_whole_list() {
    let (replace, items) = plan_of(&items_of(3));
    assert!(replace);
    assert_eq!(
        items,
        vec![("Read the code".to_string(), Some(PlanStatus::InProgress)), ("Add the file".to_string(), Some(PlanStatus::Pending))]
    );
}

#[test]
fn a_plan_update_event_gives_the_whole_list_too() {
    let (replace, items) = plan_of(&items_of(4));
    assert!(replace);
    assert_eq!(items[0], ("Read the code".to_string(), Some(PlanStatus::Completed)));
    assert_eq!(items[1], ("Add the file".to_string(), Some(PlanStatus::InProgress)));
}

#[test]
fn other_response_items_and_a_plan_call_without_a_list_give_nothing() {
    let other = r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}"}}"#;
    assert!(CodexRollout.parse_line(other, 0).is_empty());
    let bad = r#"{"type":"response_item","payload":{"type":"function_call","name":"update_plan","arguments":"not json"}}"#;
    assert!(CodexRollout.parse_line(bad, 0).is_empty());
    let none = r#"{"type":"event_msg","payload":{"type":"plan_update"}}"#;
    assert!(CodexRollout.parse_line(none, 0).is_empty());
}

#[test]
fn plan_steps_without_a_status_are_pending_and_the_arguments_may_be_an_object() {
    let payload = json!({"type": "custom_tool_call", "name": "update_plan", "input": {"plan": [{"step": "Only step"}]}});
    let items = plan_call(&payload).unwrap();
    assert_eq!(items, vec![PlanItem { text: "Only step".into(), status: Some(PlanStatus::Pending), task_id: None }]);
}

#[test]
fn a_file_change_keeps_its_changes_as_input_with_the_paths_and_is_a_write() {
    let items = items_of(5);
    let ChatItem::ToolCall { name, summary, detail, .. } = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!((name.as_str(), summary.as_str()), ("apply_patch", HELLO));
    assert_eq!(detail.paths, vec![HELLO.to_string()]);
    assert!(detail.writes && !detail.helper);
    let input = detail.input.as_ref().unwrap();
    assert_eq!(input[HELLO]["type"], "add");
    assert_eq!(input[HELLO]["content"], "hello\n");
    assert!(matches!(items[1], ChatItem::ToolResult { ok: true, .. }));
}

#[test]
fn a_moved_file_names_both_paths() {
    let items = items_of(6);
    let ChatItem::ToolCall { detail, .. } = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!(detail.paths, vec![r"C:\Users\you\proj\a.rs".to_string(), r"C:\Users\you\proj\b.rs".to_string()]);
    assert!(detail.input.as_ref().unwrap()[r"C:\Users\you\proj\a.rs"]["unified_diff"].as_str().unwrap().contains("+b"));
}

#[test]
fn patch_headers_name_the_files() {
    let patch = "*** Begin Patch\n*** Add File: a.txt\n+x\n*** Update File: src/b.rs\n*** Move to: src/c.rs\n@@\n-a\n+b\n*** Delete File: d.txt\n*** Update File: src/b.rs\n*** End Patch";
    assert_eq!(patch_paths(patch), vec!["a.txt", "src/b.rs", "src/c.rs", "d.txt"]);
    assert!(patch_paths("no headers here").is_empty());
    assert!(patch_paths("*** Add File:   ").is_empty());
}

#[test]
fn a_patch_text_in_the_change_gives_its_files_when_no_path_is_keyed() {
    let item = json!({"type": "FileChange", "id": "f", "changes": [], "patch": "*** Begin Patch\n*** Add File: x.txt\n+1\n*** End Patch"});
    let detail = file_change_detail(&item, Vec::new());
    assert_eq!(detail.paths, vec!["x.txt".to_string()]);
    assert_eq!(detail.input.unwrap()["patch"].as_str().unwrap().lines().next(), Some("*** Begin Patch"));
    // a file whose content looks like a patch header is not a path
    let tricky = json!({"changes": {"real.txt": {"type": "add", "content": "*** Add File: fake.txt\n"}}});
    assert_eq!(file_change_detail(&tricky, vec!["real.txt".to_string()]).paths, vec!["real.txt".to_string()]);
}

#[test]
fn a_command_keeps_its_line_and_folder_as_input() {
    let line = json!({"type": "event_msg", "payload": {"type": "item_completed", "item": {
        "type": "CommandExecution", "id": "c1", "command": ["cargo", "test"], "cwd": r"C:\Users\you\proj", "status": "in_progress"}}});
    let items = CodexRollout.parse_line(&line.to_string(), 0);
    let ChatItem::ToolCall { detail, .. } = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!(detail.input, Some(json!({"command": "cargo test", "cwd": r"C:\Users\you\proj"})));
    assert!(detail.paths.is_empty() && !detail.writes);
}

#[test]
fn an_mcp_call_keeps_its_arguments() {
    let line = json!({"type": "event_msg", "payload": {"type": "item_completed", "item": {
        "type": "McpToolCall", "id": "m1", "server": "s", "tool": "t", "arguments": {"q": 1}, "status": "in_progress"}}});
    let items = CodexRollout.parse_line(&line.to_string(), 0);
    let ChatItem::ToolCall { detail, .. } = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!(detail.input, Some(json!({"q": 1})));
}

#[test]
fn notes_are_notices() {
    let notice_of = |index: usize| match items_of(index).as_slice() {
        [ChatItem::Notice { notice_kind: kind, text, url, .. }] => (kind.clone(), text.clone(), url.clone()),
        other => panic!("{other:?}"),
    };
    assert_eq!(notice_of(7), ("compacted".into(), "Context compacted".into(), None));
    assert_eq!(notice_of(8), ("error".into(), "Error: stream disconnected".into(), None));
    assert_eq!(notice_of(9), ("title".into(), "Add a hello file".into(), None));
    let bare = CodexRollout.parse_line(r#"{"type":"event_msg","payload":{"type":"error"}}"#, 0);
    assert!(matches!(bare.as_slice(), [ChatItem::Notice { text, .. }] if text == "Error: something went wrong"));
    let compaction = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"ContextCompaction","id":"x"}}}"#;
    assert!(matches!(CodexRollout.parse_line(compaction, 0).as_slice(), [ChatItem::Notice { notice_kind: kind, .. }] if kind == "compacted"));
    assert!(CodexRollout.parse_line(r#"{"type":"event_msg","payload":{"type":"thread_name_updated"}}"#, 0).is_empty());
}

#[test]
fn a_to_do_list_item_gives_the_whole_list() {
    let line = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"TodoList","id":"t","items":[{"text":"One","completed":true},{"text":"Two","completed":false}]}}}"#;
    let (replace, items) = plan_of(&CodexRollout.parse_line(line, 0));
    assert!(replace);
    assert_eq!(
        items,
        vec![("One".to_string(), Some(PlanStatus::Completed)), ("Two".to_string(), Some(PlanStatus::Pending))]
    );
}

#[test]
fn usage_keeps_the_window_the_rollout_reports() {
    // `token_count` carries `model_context_window` (258400 in the captured run).
    let line = json!({"type": "event_msg", "payload": {"type": "token_count", "info": {
        "last_token_usage": {"input_tokens": 21784, "output_tokens": 77}, "model_context_window": 258400}}});
    assert!(matches!(
        CodexRollout.parse_line(&line.to_string(), 0).as_slice(),
        [ChatItem::Usage { input_tokens: 21784, output_tokens: 77, context_window: Some(258_400), .. }]
    ));
}
