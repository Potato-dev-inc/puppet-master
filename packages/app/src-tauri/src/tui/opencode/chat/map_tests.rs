//! The message mapping on a real capture (OpenCode 1.19: write a file, read it back, with a task list) and on
//! hand-made messages for the shapes a short run does not produce.

use serde_json::{json, Value};

use super::*;
use crate::session_chat::ChatItem::*;

const TURN: &str = include_str!("../fixtures/api_messages_turn.json");

fn turn() -> Vec<Value> {
    serde_json::from_str(TURN).expect("fixture parses")
}

fn no_window(_: &str, _: &str) -> Option<u64> {
    None
}

fn kinds(items: &[ChatItem]) -> Vec<&'static str> {
    items
        .iter()
        .map(|item| match item {
            User { .. } => "user",
            Assistant { .. } => "assistant",
            ToolCall { .. } => "call",
            ToolResult { .. } => "result",
            TurnEnd { .. } => "end",
            Usage { .. } => "usage",
            Session { .. } => "session",
            Plan { .. } => "plan",
            Notice { .. } => "notice",
        })
        .collect()
}

#[test]
fn a_whole_turn_becomes_rows_with_stable_ids() {
    let items = map_messages(&turn(), &no_window);
    assert_eq!(
        kinds(&items),
        ["user", "plan", "usage", "call", "result", "usage", "call", "result", "usage", "plan", "usage", "assistant", "usage", "end"]
    );
    let ids: Vec<&str> = items.iter().map(ChatItem::id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "ids repeat: {ids:?}");
    assert!(ids.iter().all(|id| id.starts_with("oc:msg_")), "{ids:?}");
    // The same messages give the same items.
    assert_eq!(items, map_messages(&turn(), &no_window));
}

#[test]
fn the_user_message_is_what_was_typed() {
    let items = map_messages(&turn(), &no_window);
    let User { text, at, .. } = &items[0] else { panic!("{:?}", items[0]) };
    assert_eq!(text, "Create oc-n28.txt containing hi, then read it back. Use a todo list with todowrite first.");
    assert!(at.as_deref().is_some_and(|at| at.ends_with('Z') && at.starts_with("2026-")), "{at:?}");
}

#[test]
fn a_write_call_names_the_file_and_carries_its_input_and_output() {
    let items = map_messages(&turn(), &no_window);
    let ToolCall { name, summary, detail, call_id, .. } = &items[3] else { panic!("{:?}", items[3]) };
    assert_eq!(name, "write");
    assert_eq!(summary, r"C:\Users\you\proj\oc-n28.txt");
    assert_eq!(detail.paths, [r"C:\Users\you\proj\oc-n28.txt"]);
    assert!(detail.writes && !detail.helper);
    assert_eq!(detail.input.as_ref().unwrap()["content"], "hi");
    let ToolResult { call_id: result_call, ok, output, .. } = &items[4] else { panic!("{:?}", items[4]) };
    assert_eq!(result_call, call_id);
    assert!(ok);
    assert_eq!(output, "Wrote file successfully.");
}

#[test]
fn a_read_call_is_not_a_write() {
    let items = map_messages(&turn(), &no_window);
    let ToolCall { name, detail, .. } = &items[6] else { panic!("{:?}", items[6]) };
    assert_eq!(name, "read");
    assert!(!detail.writes);
    assert_eq!(detail.paths.len(), 1);
    let ToolResult { output, .. } = &items[7] else { panic!() };
    assert!(output.contains("1: hi"), "{output}");
}

#[test]
fn the_task_list_is_a_plan_and_has_no_tool_row() {
    let items = map_messages(&turn(), &no_window);
    let Plan { items: first, replace, .. } = &items[1] else { panic!("{:?}", items[1]) };
    assert!(replace);
    assert_eq!(
        first.iter().map(|task| (task.text.as_str(), task.status)).collect::<Vec<_>>(),
        [("Create oc-n28.txt containing hi", Some(PlanStatus::InProgress)), ("Read oc-n28.txt back", Some(PlanStatus::Pending))]
    );
    let Plan { items: last, .. } = &items[9] else { panic!() };
    assert!(last.iter().all(|task| task.status == Some(PlanStatus::Completed)));
    assert!(!items.iter().any(|item| matches!(item, ToolCall { name, .. } if name == "todowrite")));
}

#[test]
fn the_last_message_is_the_final_answer_and_the_turn_ends_with_its_length() {
    let items = map_messages(&turn(), &no_window);
    let Assistant { text, final_answer, model, .. } = &items[11] else { panic!("{:?}", items[11]) };
    assert_eq!(text, "Done. `oc-n28.txt` contains `hi`.");
    assert!(final_answer);
    assert_eq!(model.as_deref(), Some("deepseek-v4.1-flash"));
    let TurnEnd { outcome, duration_ms, id, .. } = items.last().unwrap() else { panic!() };
    assert_eq!(*outcome, TurnOutcome::Completed);
    // 1793124859687 (last message completed) - 1793124799480 (the prompt was sent).
    assert_eq!(*duration_ms, Some(60_207));
    assert!(id.ends_with(":end"));
}

#[test]
fn every_message_reports_its_tokens_and_the_model_window() {
    let window = |provider: &str, model: &str| (provider == "opencode-go" && model == "deepseek-v4.1-flash").then_some(128_000);
    let items = map_messages(&turn(), &window);
    let usage: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            Usage { input_tokens, output_tokens, context_window, .. } => Some((*input_tokens, *output_tokens, *context_window)),
            _ => None,
        })
        .collect();
    // input + cache read; output + reasoning (the first call reasoned 40 tokens).
    assert_eq!(usage.first(), Some(&(12316 + 1792, 91 + 40, Some(128_000))));
    assert_eq!(usage.last(), Some(&(202 + 14464, 14, Some(128_000))));
    assert_eq!(usage.len(), 5);
}

#[test]
fn a_turn_that_is_still_going_has_no_end_and_no_final_answer() {
    let mut messages = turn();
    // The last model call has not finished: no `finish`, no `completed`.
    let last = messages.last_mut().unwrap();
    last["info"].as_object_mut().unwrap().remove("finish");
    last["info"]["time"].as_object_mut().unwrap().remove("completed");
    let items = map_messages(&messages, &no_window);
    assert!(!items.iter().any(|item| matches!(item, TurnEnd { .. })));
    let Assistant { final_answer, .. } = items.iter().rfind(|item| matches!(item, Assistant { .. })).unwrap() else { panic!() };
    assert!(!final_answer);
}

fn assistant_message(parts: Value, info: Value) -> Value {
    let mut base = json!({"id": "msg_a", "role": "assistant", "parentID": "msg_u", "modelID": "m", "providerID": "p", "time": {"created": 1000, "completed": 4000}});
    base.as_object_mut().unwrap().extend(info.as_object().unwrap().clone());
    json!({"info": base, "parts": parts})
}

fn user_message(text: &str) -> Value {
    json!({"info": {"id": "msg_u", "role": "user", "time": {"created": 1000}}, "parts": [{"type": "text", "text": text}]})
}

#[test]
fn an_edit_keeps_its_old_and_new_text_for_the_diff() {
    let part = json!({"type": "tool", "callID": "c1", "tool": "edit", "state": {"status": "completed",
        "input": {"filePath": "C:\\Users\\you\\proj\\a.rs", "oldString": "one", "newString": "two"}, "output": "Edit applied.",
        "time": {"start": 2000, "end": 2100}}});
    let items = map_messages(&[user_message("go"), assistant_message(json!([part]), json!({"finish": "stop"}))], &no_window);
    let ToolCall { detail, summary, .. } = &items[1] else { panic!("{:?}", items[1]) };
    let input = detail.input.as_ref().unwrap();
    assert_eq!((input["oldString"].as_str(), input["newString"].as_str()), (Some("one"), Some("two")));
    assert!(detail.writes);
    assert_eq!(summary, r"C:\Users\you\proj\a.rs");
}

#[test]
fn a_running_call_has_no_result_yet_and_a_failed_one_is_not_ok() {
    let running = json!({"type": "tool", "callID": "c1", "tool": "bash", "state": {"status": "running", "input": {"command": "npm test\nsecond line"}, "time": {"start": 2000}}});
    let failed = json!({"type": "tool", "callID": "c2", "tool": "read", "state": {"status": "error", "input": {"filePath": "C:\\Users\\you\\proj\\none.txt"}, "error": "File not found", "time": {"start": 2000, "end": 2001}}});
    let exit = json!({"type": "tool", "callID": "c3", "tool": "bash", "state": {"status": "completed", "input": {"command": "exit 3"}, "output": "", "metadata": {"exit": 3}, "time": {"start": 2000, "end": 2001}}});
    let items = map_messages(&[user_message("go"), assistant_message(json!([running, failed, exit]), json!({"finish": "tool-calls"}))], &no_window);
    assert_eq!(kinds(&items), ["user", "call", "call", "result", "call", "result"]);
    let ToolCall { summary, .. } = &items[1] else { panic!() };
    assert_eq!(summary, "npm test");
    let ToolResult { ok, output, .. } = &items[3] else { panic!() };
    assert!(!ok);
    assert_eq!(output, "File not found");
    let ToolResult { ok, .. } = &items[5] else { panic!() };
    assert!(!ok, "exit code 3 is a failed command");
}

#[test]
fn an_interrupted_turn_ends_interrupted_and_a_failed_one_says_why() {
    let aborted = assistant_message(json!([]), json!({"error": {"name": "MessageAbortedError", "data": {"message": "The operation was aborted."}}}));
    let items = map_messages(&[user_message("go"), aborted], &no_window);
    let TurnEnd { outcome, .. } = items.last().unwrap() else { panic!("{items:?}") };
    assert_eq!(*outcome, TurnOutcome::Interrupted);

    let broke = assistant_message(json!([]), json!({"error": {"name": "APIError", "data": {"message": "Rate limit reached."}}}));
    let items = map_messages(&[user_message("go"), broke], &no_window);
    assert_eq!(kinds(&items), ["user", "notice", "end"]);
    let Notice { notice_kind, text, .. } = &items[1] else { panic!() };
    assert_eq!((notice_kind.as_str(), text.as_str()), ("error", "Rate limit reached."));
    let TurnEnd { outcome, .. } = &items[2] else { panic!() };
    assert_eq!(*outcome, TurnOutcome::Failed);
}

#[test]
fn a_message_the_server_stamped_to_set_the_model_is_not_a_message() {
    let stamp = json!({"info": {"id": "msg_s", "role": "user", "time": {"created": 1}}, "parts": [{"type": "text", "text": "\u{200b}"}]});
    assert!(map_messages(&[stamp], &no_window).is_empty());
}

#[test]
fn odd_messages_give_nothing_and_never_panic() {
    let odd = [json!({}), json!({"info": {"role": "user"}}), json!({"info": {"id": "x", "role": "system"}, "parts": 3}), json!({"info": {"id": "y", "role": "assistant"}})];
    assert!(map_messages(&odd, &no_window).is_empty());
}

#[test]
fn a_long_output_is_cut() {
    let part = json!({"type": "tool", "callID": "c", "tool": "bash", "state": {"status": "completed", "input": {"command": "x"}, "output": "y".repeat(10_000), "time": {"start": 1, "end": 2}}});
    let items = map_messages(&[user_message("go"), assistant_message(json!([part]), json!({"finish": "stop"}))], &no_window);
    let ToolResult { output, .. } = &items[2] else { panic!() };
    assert_eq!(output.chars().count(), crate::session_chat::model::MAX_OUTPUT_CHARS + 1);
}
