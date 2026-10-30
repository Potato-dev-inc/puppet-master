//! Live check: a 1-turn budget on a REAL Claude run, then the followup that hits the pause card.
//! Spends up to two real agent turns, so it is ignored by default:
//!
//!   cargo test --lib budgets::live_tests -- --ignored --nocapture
//!
//! The turns are the same headless child `run_agent` starts (`tui::headless_launch` for `claude`,
//! cwd `D:\pm-demo`), recorded as operations; the budget gate is the one `send_agent` calls.

use super::card::run_of_card;
use super::*;
use crate::operations::{self, DelegateWorkRequest, ResultCapture, WorkerPersist};
use crate::questions;
use std::process::Command;
use uuid::Uuid;

const PROJECT: &str = "D:\\pm-demo";

fn say(line: &str) {
    println!("[live] {line}");
}

/// One real headless turn of Claude, recorded as the operation `turn` of `handle`.
fn real_turn(handle: &str, turn: u32, task: &str) -> operations::OperationSnapshot {
    let op = operations::create_operation(DelegateWorkRequest {
        project_path: PROJECT.into(),
        task: task.into(),
        agent_type: "claude".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 300_000,
        read_only: true,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("budget-live".into()),
        parent_pane_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.into()),
        turn_index: turn,
        worker: WorkerPersist::default(),
        context_policy: None,
        checks: Vec::new(),
    })
    .unwrap()
    .0;
    let profile = crate::tui::profile_for("claude");
    let spec = crate::tui::headless_launch(
        "claude",
        &crate::tui::HeadlessLaunch { read_only: true, ..crate::tui::HeadlessLaunch::new(PROJECT, task) },
    )
    .expect("claude headless launch");
    say(&format!("turn {turn}: starting real Claude: {} {}", spec.program, spec.args.join(" ").chars().take(120).collect::<String>()));
    operations::run_operation_dispatch(PROJECT, &op.operation_id, |_| Ok(())).unwrap();
    let mut command = Command::new(&spec.program);
    command.args(&spec.args).current_dir(&spec.cwd).stdin(std::process::Stdio::null());
    spec.apply_to_command(&mut command);
    let output = command.output().expect("claude ran");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);
    let result = profile.extract_result(&stdout, code).unwrap_or_default();
    say(&format!("turn {turn}: exit {code}, result {:?}", result.chars().take(200).collect::<String>()));
    assert!(!result.trim().is_empty(), "Claude gave no result; stderr: {}", String::from_utf8_lossy(&output.stderr));
    operations::mark_operation_finished(PROJECT, &op.operation_id, Some(result), ResultCapture::Authoritative, true).unwrap()
}

#[test]
#[ignore = "spends up to two real Claude turns in D:\\pm-demo"]
fn live_a_one_turn_budget_on_a_real_claude_run_then_the_followup_hits_the_pause_card() {
    assert!(std::path::Path::new(PROJECT).is_dir(), "{PROJECT} must exist");
    let handle = format!("budget-live-{}", &Uuid::new_v4().to_string()[..8]);
    let asked = Asked { turns: Some(1), minutes: None, parent: None };
    let record = admit(PROJECT, &handle, &asked, Defaults::default(), questions::now_ms()).unwrap().unwrap();
    say(&format!("budget admitted: {} turn, source {:?}", record.turns.unwrap(), record.source));

    check_next_turn(PROJECT, &handle).expect("the first turn may start");
    let first = real_turn(&handle, 0, "Reply with the single word KIWI. Do not use any tools.");
    say(&format!("turn 0 finished: {:?}, result {:?}", first.status, first.result));
    turn_ended(&first);
    say(&format!("view after turn 0: {}", view(PROJECT, &handle).unwrap()));

    // The pause card is in the Inbox the moment the last turn ends.
    let card = questions::store()
        .list_pending(questions::now_ms())
        .into_iter()
        .find(|q| q.kind.as_deref() == Some("budget") && run_of_card(&q.id).is_some_and(|run| run.1 == handle))
        .expect("the pause card");
    say(&format!("card: {:?} | {:?} | buttons {:?}", card.question, card.context, card.choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>()));
    assert!(card.question.contains("used its 1 turn"), "{}", card.question);

    // The followup (what send_agent runs first) is refused with guidance. No real turn is spent.
    let error = check_next_turn(PROJECT, &handle).unwrap_err();
    say(&format!("followup refused: {} | {}", error.code, error.message));
    assert_eq!(error.code, "BUDGET_EXHAUSTED");
    assert_eq!(operations::turns_for_handle(PROJECT, &handle).unwrap().len(), 1, "no second turn was created");

    // Add turns on the card, and the same followup goes through: a second real turn, then paused again.
    questions::store().answer(&card.id, Some("add_turns"), None, "desktop", questions::now_ms()).unwrap();
    say(&format!("after Add: {}", view(PROJECT, &handle).unwrap()));
    check_next_turn(PROJECT, &handle).expect("Add turns reopens the gate");
    let second = real_turn(&handle, 1, "Reply with the single word PLUM. Do not use any tools.");
    say(&format!("turn 1 finished: {:?}, result {:?}", second.status, second.result));
    turn_ended(&second);
    assert_eq!(check_next_turn(PROJECT, &handle).unwrap_err().code, "BUDGET_EXHAUSTED", "paused again at the new limit");
    say(&format!("final view: {}", view(PROJECT, &handle).unwrap()));
    say("OK");
}
