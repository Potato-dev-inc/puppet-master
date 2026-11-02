//! Mapping tests: real, redacted adapter traffic (`fixtures/`, recorded from the Codex and Claude adapters in the
//! Windows spike) goes through the mapper and the items are compared with golden files (`golden/`). Regenerate a
//! golden with `PM_UPDATE_GOLDEN=1 cargo test --lib acp::chat::golden_tests` and read the diff: a golden that
//! changes is a change to what every chat view shows.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::mapper::Mapper;
use crate::acp::handshake::AgentCaps;
use crate::acp::session::{SessionEvent, StartKind};
use crate::acp::state::{ExitKind, ExitSummary, StopKind, TurnSummary};
use crate::acp::types::SessionUpdate;
use crate::session_chat::{ChatItem, Feed, TurnOutcome};

const AT: &str = "2026-11-01T10:00:00.000Z";

fn lines(text: &str) -> Vec<Value> {
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str::<Value>(l).expect("a fixture line")["update"].clone()).collect()
}

fn update_event(raw: &Value, replay: bool) -> SessionEvent {
    match serde_json::from_value::<SessionUpdate>(raw.clone()) {
        Ok(update) => SessionEvent::Update { update: Box::new(update), replay },
        Err(_) => SessionEvent::UpdateUnparsed { kind: raw["sessionUpdate"].as_str().unwrap_or("?").to_string(), raw: raw.clone(), replay },
    }
}

fn summary(turn: u64, stop: StopKind) -> TurnSummary {
    TurnSummary { turn, stop, stop_reason: None, cancel_requested: stop == StopKind::Cancelled, empty: false, error: None, error_code: None }
}

fn caps() -> Box<AgentCaps> {
    Box::new(AgentCaps { agent_name: Some("adapter".into()), agent_title: Some("Agent".into()), agent_version: Some("1.2.3".into()), ..AgentCaps::default() })
}

/// A mapper and the list it fills, the way `AcpChat` uses them.
struct Run {
    mapper: Mapper,
    feed: Feed,
    t0: Instant,
    steps: u64,
}

impl Run {
    fn new() -> Self {
        Run { mapper: Mapper::new(), feed: Feed::new(), t0: Instant::now(), steps: 0 }
    }

    fn now(&mut self) -> Instant {
        self.steps += 1;
        self.t0 + Duration::from_millis(250 * self.steps)
    }

    fn put(&mut self, items: Vec<ChatItem>) -> Vec<ChatItem> {
        for item in &items {
            self.feed.upsert(item.clone());
        }
        items
    }

    fn event(&mut self, event: &SessionEvent) -> Vec<ChatItem> {
        let now = self.now();
        let items = self.mapper.on_event(event, now, AT);
        self.put(items)
    }

    fn turn(&mut self, turn: u64, text: &str) {
        let now = self.now();
        let items = self.mapper.turn_started(turn, Some(text), now, AT);
        self.put(items);
    }

    fn started(&mut self, how: StartKind) {
        self.event(&SessionEvent::Started { how, caps: caps() });
    }

    fn updates(&mut self, raws: &[Value], replay: bool) {
        for raw in raws {
            self.event(&update_event(raw, replay));
        }
    }

    fn json(&self) -> Value {
        serde_json::to_value(self.feed.all()).unwrap()
    }
}

/// Compares `actual` with `golden/<name>.json` (or writes it when `PM_UPDATE_GOLDEN=1`).
fn golden(name: &str, actual: &Value) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/acp/chat/golden").join(format!("{name}.json"));
    let text = format!("{}\n", serde_json::to_string_pretty(actual).unwrap());
    if std::env::var("PM_UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("no golden file {}; run with PM_UPDATE_GOLDEN=1", path.display()));
    assert_eq!(expected.replace("\r\n", "\n"), text, "golden {name} differs; if the change is wanted, regenerate it with PM_UPDATE_GOLDEN=1");
}

fn kinds(items: &[Value]) -> Vec<String> {
    items.iter().map(|i| i["kind"].as_str().unwrap_or("?").to_string()).collect()
}

// ---- recorded traffic ------------------------------------------------------------------------------------------

#[test]
fn a_codex_turn_with_a_failed_edit_becomes_one_message_a_tool_row_and_a_turn_end() {
    let mut run = Run::new();
    run.started(StartKind::New);
    run.turn(1, "Create spike.txt with the word ok");
    run.updates(&lines(include_str!("fixtures/codex_live_turn.ndjson")), false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let items = run.json();
    golden("codex_live_turn", &items);

    let list = items.as_array().unwrap();
    // 52+ chunks of the same stream are a few messages, not a few dozen items; thoughts are not shown.
    assert!(list.iter().filter(|i| i["kind"] == "assistant").count() <= 4, "{}", kinds(list).join(","));
    assert!(list.iter().all(|i| i["kind"] != "thinking"));
    let tool = list.iter().find(|i| i["kind"] == "toolCall").expect("the edit");
    assert_eq!(tool["name"], "edit");
    assert_eq!(tool["writes"], true);
    assert_eq!(tool["summary"], "Editing files");
    assert!(tool["paths"][0].as_str().unwrap().ends_with("spike.txt"));
    // The diff the agent sent is what the view draws.
    assert_eq!(tool["input"]["new_string"], "ok\n");
    assert_eq!(tool["input"]["old_string"], "");
    let result = list.iter().find(|i| i["kind"] == "toolResult").expect("the failed edit's result");
    assert_eq!(result["ok"], false);
    assert_eq!(result["callId"], tool["callId"]);
    let last = list.last().unwrap();
    assert_eq!(last["kind"], "turnEnd");
    assert_eq!(last["outcome"], "completed");
}

#[test]
fn a_claude_turn_whose_write_was_refused_shows_the_title_that_arrived_late_and_the_refusal() {
    let mut run = Run::new();
    run.started(StartKind::New);
    run.turn(1, "Create spike.txt");
    run.updates(&lines(include_str!("fixtures/claude_live_turn.ndjson")), false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let items = run.json();
    golden("claude_live_turn", &items);

    let list = items.as_array().unwrap();
    // The call was announced as "Preparing file…" and named later; the row shows the later name, in place.
    let calls: Vec<&Value> = list.iter().filter(|i| i["kind"] == "toolCall").collect();
    assert_eq!(calls.len(), 1, "one tool row for one tool call");
    assert_eq!(calls[0]["summary"], "Write spike.txt");
    let result = list.iter().find(|i| i["kind"] == "toolResult").unwrap();
    assert_eq!(result["ok"], false);
    // The adapter's code fence around the refusal is formatting, not output.
    assert_eq!(result["output"], "User refused permission to run tool");
    // The agent's words come after the tool, as one message.
    let order = kinds(list);
    let tool_at = order.iter().position(|k| k == "toolCall").unwrap();
    let text_at = order.iter().position(|k| k == "assistant").unwrap();
    assert!(tool_at < text_at, "{order:?}");
}

#[test]
fn replayed_history_into_an_empty_chat_is_the_chat_and_its_ids_never_meet_the_live_turns() {
    for (name, text) in [("codex_replay", include_str!("fixtures/codex_replay.ndjson")), ("claude_replay", include_str!("fixtures/claude_replay.ndjson"))] {
        let mut run = Run::new();
        run.started(StartKind::Loaded);
        run.updates(&lines(text), true);
        let items = run.json();
        golden(name, &items);
        let list = items.as_array().unwrap();
        assert!(list.iter().any(|i| i["kind"] == "user"), "{name}: the earlier question");
        assert!(list.iter().any(|i| i["kind"] == "assistant"), "{name}: the earlier answer");
        for item in list.iter().filter(|i| matches!(i["kind"].as_str(), Some("user") | Some("assistant"))) {
            assert!(item["id"].as_str().unwrap().starts_with('h'), "{name}: {}", item["id"]);
        }
        // A live turn after the replay starts at u1 and a1.1: no collision.
        run.turn(1, "next");
        assert!(run.feed.get("u1").is_some());
    }
}

#[test]
fn a_replay_into_a_chat_that_already_has_items_is_dropped() {
    let mut run = Run::new();
    run.started(StartKind::New);
    run.turn(1, "hello");
    run.updates(&[json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi there"}})], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let before = run.json();
    // The adapter came back (a crash, an idle sleep) and replays the history it already showed.
    run.started(StartKind::Loaded);
    run.updates(&lines(include_str!("fixtures/codex_replay.ndjson")), true);
    let after = run.json();
    let strip = |v: &Value| -> Vec<Value> { v.as_array().unwrap().iter().filter(|i| i["kind"] != "session").cloned().collect() };
    assert_eq!(strip(&before), strip(&after));
}

// ---- the turn ----------------------------------------------------------------------------------------------------

#[test]
fn chunks_of_one_message_grow_one_item_and_a_tool_splits_the_text_in_two() {
    let mut run = Run::new();
    run.turn(1, "go");
    let text = |t: &str| json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}});
    run.updates(&[text("Let me "), text("look. ")], false);
    assert_eq!(run.feed.get("a1.1").map(|i| serde_json::to_value(i).unwrap()["text"].clone()), Some(json!("Let me look. ")));
    run.updates(&[json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Read a.rs", "kind": "read", "status": "completed", "locations": [{"path": "a.rs"}]})], false);
    run.updates(&[text("Now I see.")], false);
    let all = serde_json::to_value(run.feed.all()).unwrap();
    assert_eq!(kinds(all.as_array().unwrap()), ["user", "assistant", "toolCall", "toolResult", "assistant"]);
    assert_eq!(all[4]["id"], "a1.2");
    assert_eq!(all[2]["name"], "read");
    assert_eq!(all[2]["paths"], json!(["a.rs"]));
    assert!(all[2].get("writes").is_none());
}

#[test]
fn a_second_message_id_starts_a_second_message() {
    let mut run = Run::new();
    run.turn(1, "go");
    let text = |t: &str, id: &str| json!({"sessionUpdate": "agent_message_chunk", "messageId": id, "content": {"type": "text", "text": t}});
    run.updates(&[text("one", "m1"), text(" more", "m1"), text("two", "m2")], false);
    assert_eq!(run.feed.get("a1.1").map(|i| serde_json::to_value(i).unwrap()["text"].clone()), Some(json!("one more")));
    assert_eq!(run.feed.get("a1.2").map(|i| serde_json::to_value(i).unwrap()["text"].clone()), Some(json!("two")));
}

#[test]
fn the_last_message_of_a_finished_turn_is_the_final_answer_and_the_turn_end_has_its_duration() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Done."}})], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let all = serde_json::to_value(run.feed.all()).unwrap();
    assert_eq!(all[1]["finalAnswer"], true);
    assert_eq!(all[2]["noticeKind"], "ending:done");
    assert!(all[2]["text"].as_str().unwrap().starts_with("Done"));
    assert_eq!(all[3]["kind"], "turnEnd");
    assert_eq!(all[3]["id"], "end:1");
    assert_eq!(all[3]["outcome"], "completed");
    assert!(all[3]["durationMs"].as_u64().unwrap() > 0);
}

#[test]
fn a_plan_is_one_whole_list_that_is_replaced_in_place_and_usage_is_one_item() {
    let mut run = Run::new();
    run.turn(1, "go");
    let plan = |a: &str| json!({"sessionUpdate": "plan", "entries": [{"content": "Write it", "priority": "high", "status": a}, {"content": "Test it", "priority": "low", "status": "pending"}]});
    run.updates(&[plan("in_progress"), plan("completed")], false);
    run.updates(&[json!({"sessionUpdate": "usage_update", "used": 10, "size": 1000}), json!({"sessionUpdate": "usage_update", "used": 20, "size": 1000})], false);
    let all = serde_json::to_value(run.feed.all()).unwrap();
    let plans: Vec<&Value> = all.as_array().unwrap().iter().filter(|i| i["kind"] == "plan").collect();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0]["replace"], true);
    assert_eq!(plans[0]["items"][0]["status"], "completed");
    assert_eq!(plans[0]["items"][1]["status"], "pending");
    let usage: Vec<&Value> = all.as_array().unwrap().iter().filter(|i| i["kind"] == "usage").collect();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0]["inputTokens"], 20);
    assert_eq!(usage[0]["contextWindow"], 1000);
}

#[test]
fn title_model_and_compaction_become_session_items_and_notices() {
    let mut run = Run::new();
    run.started(StartKind::New);
    run.updates(
        &[
            json!({"sessionUpdate": "session_info_update", "title": "Fix the build"}),
            json!({"sessionUpdate": "config_option_update", "configOptions": [{"id": "model", "category": "model", "currentValue": "b", "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "Model B"}]}]}),
            json!({"sessionUpdate": "compaction_update", "status": "done"}),
        ],
        false,
    );
    let all = serde_json::to_value(run.feed.all()).unwrap();
    let session = all.as_array().unwrap().iter().find(|i| i["kind"] == "session").unwrap();
    assert_eq!(session["model"], "Model B");
    assert_eq!(session["cliVersion"], "1.2.3");
    let notices: Vec<&str> = all.as_array().unwrap().iter().filter(|i| i["kind"] == "notice").map(|i| i["noticeKind"].as_str().unwrap()).collect();
    assert_eq!(notices, ["title", "compacted"]);
}

#[test]
fn a_cancelled_turn_is_interrupted_and_closes_the_tools_it_left_open() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Run build", "kind": "execute", "status": "in_progress"})], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::Cancelled)));
    let all = serde_json::to_value(run.feed.all()).unwrap();
    let list = all.as_array().unwrap();
    assert_eq!(list.iter().find(|i| i["kind"] == "toolCall").unwrap()["name"], "run");
    let result = list.iter().find(|i| i["kind"] == "toolResult").unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(result["output"], "Stopped before it finished.");
    assert_eq!(list.last().unwrap()["outcome"], "interrupted");
    let notices: Vec<&Value> = list.iter().filter(|i| i["kind"] == "notice").collect();
    assert_eq!(notices.len(), 1, "stopping on purpose is one grey line, not an error");
    assert_eq!((notices[0]["noticeKind"].as_str(), notices[0]["text"].as_str()), (Some("ending:stopped"), Some("You stopped it")));
}

#[test]
fn a_crash_mid_turn_keeps_the_stream_consistent_and_a_late_turn_end_changes_nothing() {
    let mut run = Run::new();
    run.started(StartKind::New);
    run.turn(1, "build it");
    run.updates(
        &[
            json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "starting "}}),
            json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Run build", "kind": "execute", "status": "in_progress"}),
        ],
        false,
    );
    // The adapter dies; the exit is noticed before the turn is reported as ended.
    run.event(&SessionEvent::Exited(ExitSummary { kind: ExitKind::Crashed, code: Some(3), stderr_tail: vec![], reason: None }));
    let after_exit = run.json();
    let list = after_exit.as_array().unwrap();
    let ends: Vec<&Value> = list.iter().filter(|i| i["kind"] == "turnEnd").collect();
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0]["outcome"], "failed");
    assert_eq!(list.iter().find(|i| i["kind"] == "toolResult").unwrap()["ok"], false, "the open tool does not keep spinning");
    let notice = list.iter().find(|i| i["kind"] == "notice").unwrap();
    assert_eq!(notice["noticeKind"], "ending:failed");
    assert!(notice["text"].as_str().unwrap().starts_with("Something went wrong: "));
    assert!(notice["text"].as_str().unwrap().contains("exit code 3"));
    // The session's own report of the same end arrives: no second end, nothing new.
    let mut failed = summary(1, StopKind::Failed);
    failed.error = Some("the adapter exited before the turn finished".into());
    run.event(&SessionEvent::TurnEnded(failed));
    assert_eq!(run.json(), after_exit);
    // Every tool call has a result and every turn has an end.
    assert_eq!(list.iter().filter(|i| i["kind"] == "toolCall").count(), list.iter().filter(|i| i["kind"] == "toolResult").count());
}

#[test]
fn an_end_turn_with_no_answer_says_so() {
    let mut run = Run::new();
    run.turn(1, "hi");
    let mut s = summary(1, StopKind::EndTurn);
    s.empty = true;
    run.event(&SessionEvent::TurnEnded(s));
    let all = serde_json::to_value(run.feed.all()).unwrap();
    let notice = all.as_array().unwrap().iter().find(|i| i["kind"] == "notice").unwrap();
    assert_eq!(notice["noticeKind"], "ending:empty");
    assert_eq!(notice["text"], "The turn ended with no answer");
}

#[test]
fn a_turn_that_failed_with_the_sign_in_code_says_so() {
    let mut run = Run::new();
    run.turn(1, "hi");
    let mut s = summary(1, StopKind::Failed);
    s.error_code = Some(-32000);
    s.error = Some("Authentication required".into());
    run.event(&SessionEvent::TurnEnded(s));
    let all = serde_json::to_value(run.feed.all()).unwrap();
    let notice = all.as_array().unwrap().iter().find(|i| i["kind"] == "notice").unwrap();
    assert_eq!(notice["noticeKind"], "ending:signed_out");
    assert_eq!(notice["text"], "Probably signed out");
    assert_eq!(all.as_array().unwrap().last().unwrap()["outcome"], "failed");
}

/// The grey line a finished turn ends with: (notice kind, text).
fn ending_line(run: &Run) -> (String, String) {
    let all = run.json();
    let notice = all.as_array().unwrap().iter().rev().find(|i| i["kind"] == "notice").expect("a turn end line");
    (notice["noticeKind"].as_str().unwrap().to_string(), notice["text"].as_str().unwrap().to_string())
}

fn say(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
}

#[test]
fn a_turn_that_changed_files_says_how_many_and_how_long() {
    let mut run = Run::new();
    run.turn(1, "go");
    for (id, path, kind) in [("e1", "a.txt", "edit"), ("e2", "b.txt", "edit"), ("e3", "a.txt", "edit"), ("r1", "c.txt", "read")] {
        run.updates(
            &[
                json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": "t", "kind": kind, "status": "pending", "locations": [{"path": path}]}),
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed"}),
            ],
            false,
        );
    }
    // A refused edit changed nothing.
    run.updates(
        &[
            json!({"sessionUpdate": "tool_call", "toolCallId": "e4", "title": "t", "kind": "edit", "status": "pending", "locations": [{"path": "d.txt"}]}),
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "e4", "status": "failed"}),
            say("All set."),
        ],
        false,
    );
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let (kind, text) = ending_line(&run);
    assert_eq!(kind, "ending:done");
    assert!(text.starts_with("Done · 2 files · "), "{text}");
}

#[test]
fn a_usage_limit_names_the_agent_by_its_label() {
    let mut run = Run::new();
    run.started(StartKind::New);
    run.turn(1, "go");
    let mut s = summary(1, StopKind::Failed);
    s.error = Some("Usage limit reached. Resets at 5pm.".into());
    run.event(&SessionEvent::TurnEnded(s));
    assert_eq!(ending_line(&run), ("ending:usage_limit".to_string(), "Agent hit its usage limit".to_string()));
    assert_eq!(run.json().as_array().unwrap().last().unwrap()["outcome"], "failed");
}

#[test]
fn a_turn_the_agent_left_quiet_ends_as_no_answer() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::Hung)));
    assert_eq!(ending_line(&run), ("ending:no_answer".to_string(), "No answer for 5 min".to_string()));
}

#[test]
fn an_end_turn_with_only_thoughts_or_only_leaked_markup_is_a_silent_failure() {
    // The session saw a thought chunk, so its own `empty` flag is false; the person has nothing to read.
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm"}})], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    assert_eq!(ending_line(&run).0, "ending:empty");
    assert_eq!(run.json().as_array().unwrap().last().unwrap()["outcome"], "failed");

    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[say("<|im_end|>"), say("<|endoftext|>")], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let all = run.json();
    assert!(all.as_array().unwrap().iter().all(|i| i["kind"] != "assistant"), "a message that is only markup is not shown");
    assert_eq!(ending_line(&run).0, "ending:empty");
}

#[test]
fn a_turn_that_only_ran_tools_is_done_not_silent() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Run build", "kind": "execute", "status": "completed"})], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    assert_eq!(ending_line(&run).0, "ending:done");
}

#[test]
fn leaked_markup_is_stripped_from_the_answer_even_when_a_token_arrives_in_pieces() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[say("The answer is 4.<|im"), say("_end|><|endoftext|>"), say("
<function_calls>
<invoke name=\"x\">
</invoke>
</function_calls>")], false);
    run.event(&SessionEvent::TurnEnded(summary(1, StopKind::EndTurn)));
    let all = run.json();
    let answer = all.as_array().unwrap().iter().find(|i| i["kind"] == "assistant").unwrap();
    assert_eq!(answer["text"], "The answer is 4.");
    assert_eq!(answer["finalAnswer"], true);
}

#[test]
fn a_chat_closed_in_a_turn_ends_it_as_stopped() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[say("working")], false);
    run.event(&SessionEvent::Exited(ExitSummary { kind: ExitKind::Closed, code: None, stderr_tail: vec![], reason: None }));
    assert_eq!(ending_line(&run).0, "ending:stopped");
}

#[test]
fn steering_text_is_the_persons_message_inside_the_turn_and_the_next_words_are_a_new_message() {
    let mut run = Run::new();
    run.turn(1, "go");
    run.updates(&[json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "working"}})], false);
    let now = run.now();
    let _ = now;
    let items = run.mapper.steered("also do X", AT);
    run.put(items);
    run.updates(&[json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ok X"}})], false);
    let all = serde_json::to_value(run.feed.all()).unwrap();
    assert_eq!(kinds(all.as_array().unwrap()), ["user", "assistant", "user", "assistant"]);
    assert_eq!(all[2]["text"], "also do X");
}

#[test]
fn updates_this_build_cannot_type_and_thoughts_add_nothing() {
    let mut run = Run::new();
    run.turn(1, "go");
    let before = run.json();
    run.updates(
        &[
            json!({"sessionUpdate": "future_thing", "x": 1}),
            json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm"}}),
            json!({"sessionUpdate": "available_commands_update", "availableCommands": []}),
            json!({"sessionUpdate": "current_mode_update", "currentModeId": "read-only"}),
        ],
        false,
    );
    assert_eq!(run.json(), before);
}

#[test]
fn a_runaway_message_is_cut_not_allowed_to_grow_without_end() {
    let mut run = Run::new();
    run.turn(1, "go");
    let big = "x".repeat(150_000);
    let chunk = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": big}});
    run.updates(&[chunk.clone(), chunk.clone(), chunk], false);
    let item = serde_json::to_value(run.feed.get("a1.1").unwrap()).unwrap();
    let text = item["text"].as_str().unwrap();
    assert!(text.chars().count() < 200_100);
    assert!(text.ends_with("(cut: the message was too long)"));
}

#[test]
fn turn_outcomes_serialise_as_the_views_know_them() {
    assert_eq!(serde_json::to_value(TurnOutcome::Interrupted).unwrap(), "interrupted");
}
