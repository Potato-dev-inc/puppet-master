//! Tests for `point::resolve` (SPEC n33 section 3.4): the save point and the chat cut of a branch.

use super::*;
use crate::checkpoints::Checkpoint;
use crate::session_chat::model::TurnOutcome;

/// Unix ms of 2026-10-28 10:00:00 UTC.
fn base_ms() -> u64 {
    chrono::DateTime::parse_from_rfc3339("2026-10-28T10:00:00Z").unwrap().timestamp_millis() as u64
}

/// RFC 3339 text of `sec` seconds after 10:00:00 UTC (under an hour).
fn at(sec: u64) -> String {
    format!("2026-10-28T10:{:02}:{:02}Z", sec / 60, sec % 60)
}

/// Unix ms of `sec` seconds after 10:00:00 UTC.
fn ms(sec: u64) -> u64 {
    base_ms() + sec * 1000
}

fn user(id: &str, sec: Option<u64>, text: &str) -> ChatItem {
    ChatItem::User { id: id.into(), at: sec.map(at), text: text.into() }
}

fn reply(id: &str, sec: Option<u64>) -> ChatItem {
    ChatItem::Assistant { id: id.into(), at: sec.map(at), text: "done".into(), model: None, final_answer: true }
}

fn turn_end(id: &str, sec: Option<u64>) -> ChatItem {
    ChatItem::TurnEnd { id: id.into(), at: sec.map(at), outcome: TurnOutcome::Completed, duration_ms: None }
}

/// A save point taken `sec` seconds after 10:00:00. `kind` is `before` or `after`.
fn checkpoint(id: &str, turn: u32, kind: &str, sec: u64) -> Checkpoint {
    let title = if kind == "before" { "Before" } else { "After" };
    Checkpoint {
        id: id.into(),
        commit: "0123456789abcdef".into(),
        pane_id: "pane-1".into(),
        turn,
        kind: kind.into(),
        label: format!("{title} turn {turn}"),
        note: String::new(),
        at_ms: ms(sec),
        files: 1,
        added: 0,
        deleted: 0,
        tree: "tree".into(),
    }
}

fn message(id: &str, when: When) -> BranchFrom {
    BranchFrom::Message { user_item_id: id.into(), when }
}

fn refused(result: Result<Resolved, (&'static str, String)>) -> &'static str {
    result.err().expect("expected a refusal").0
}

#[test]
fn before_picks_the_checkpoint_at_turn_start_with_two_seconds_of_slack() {
    let items = vec![user("u1", Some(100), "first ask"), reply("a1", Some(104)), turn_end("e1", Some(105))];
    // A save point 3 s after the turn started is too late for "before".
    let late = vec![checkpoint("1", 1, "before", 90), checkpoint("2", 2, "after", 103)];
    let resolved = resolve(&message("u1", When::Before), &items, &[], &late).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "1");
    assert_eq!(resolved.cut, Cut::BeforeTurnAt(0));
    // A save point exactly 2 s after the turn started still counts.
    let slack = vec![checkpoint("1", 1, "before", 90), checkpoint("2", 2, "after", 102)];
    let resolved = resolve(&message("u1", When::Before), &items, &[], &slack).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "2");
    assert_eq!(resolved.point.label, "After turn 2");
    assert_eq!(resolved.point.at_ms, ms(102));
}

#[test]
fn after_picks_the_after_snapshot_that_landed_ten_seconds_late() {
    let items = vec![user("u1", Some(100), "first ask"), reply("a1", Some(103)), turn_end("e1", Some(104))];
    // The turn ended at 104 s; its after-snapshot landed at 114 s (10 s late, within the 12 s slack).
    let cps = vec![checkpoint("1", 1, "before", 100), checkpoint("2", 1, "after", 114)];
    let resolved = resolve(&message("u1", When::After), &items, &[], &cps).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "2");
    assert_eq!(resolved.point.label, "After turn 1");
    assert_eq!(resolved.cut, Cut::ThroughTurnAt(0));
    assert_eq!(resolved.prefill, None);
    // One second more than the slack is too late: the before-snapshot is the answer.
    let too_late = vec![checkpoint("1", 1, "before", 100), checkpoint("2", 1, "after", 117)];
    let resolved = resolve(&message("u1", When::After), &items, &[], &too_late).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "1");
}

#[test]
fn after_is_bounded_by_the_next_message() {
    let items = vec![
        user("u1", Some(100), "first ask"),
        reply("a1", Some(103)),
        turn_end("e1", Some(104)),
        user("u2", Some(112), "second ask"),
    ];
    // The next message came at 112 s, so the after-bound is 114 s (next + 2 s), not 116 s (end + 12 s).
    let cps = vec![checkpoint("2", 1, "after", 105), checkpoint("3", 2, "before", 115)];
    let resolved = resolve(&message("u1", When::After), &items, &[], &cps).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "2");
    assert_eq!(resolved.cut, Cut::ThroughTurnAt(0));
    // Without the next message the same late snapshot would be taken.
    let alone = vec![user("u1", Some(100), "first ask"), reply("a1", Some(103)), turn_end("e1", Some(104))];
    let resolved = resolve(&message("u1", When::After), &alone, &[], &cps).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "3");
}

#[test]
fn a_running_turn_is_refused_for_after_and_allowed_for_before() {
    let items = vec![
        user("u1", Some(100), "first ask"),
        reply("a1", Some(103)),
        turn_end("e1", Some(104)),
        user("u2", Some(110), "second ask"),
        reply("a2", Some(112)),
    ];
    let cps = vec![checkpoint("4", 1, "after", 105), checkpoint("5", 2, "before", 111)];
    let (code, message_text) = resolve(&message("u2", When::After), &items, &[], &cps).err().unwrap();
    assert_eq!(code, ERR_BRANCH_STILL_RUNNING);
    assert_eq!(message_text, MSG_BRANCH_STILL_RUNNING);
    let resolved = resolve(&message("u2", When::Before), &items, &[], &cps).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "5");
    assert_eq!(resolved.cut, Cut::BeforeTurnAt(3));
}

#[test]
fn a_message_without_a_time_is_refused_with_no_times() {
    // Before: the user item's own time is needed.
    let items = vec![user("u1", None, "first ask"), reply("a1", Some(3)), turn_end("e1", Some(4))];
    let cps = vec![checkpoint("1", 1, "before", 0)];
    let (code, message_text) = resolve(&message("u1", When::Before), &items, &[], &cps).err().unwrap();
    assert_eq!(code, ERR_BRANCH_NO_TIMES);
    assert_eq!(message_text, MSG_BRANCH_NO_TIMES);
    // After: an older turn whose items carry no times cannot be placed.
    let older = vec![
        user("u1", None, "first ask"),
        reply("a1", None),
        user("u2", Some(50), "second ask"),
        reply("a2", Some(51)),
        turn_end("e2", Some(52)),
    ];
    assert_eq!(refused(resolve(&message("u1", When::After), &older, &[], &cps)), ERR_BRANCH_NO_TIMES);
}

#[test]
fn no_save_point_in_reach_is_refused() {
    let items = vec![user("u1", Some(100), "first ask"), reply("a1", Some(103)), turn_end("e1", Some(104))];
    // Before: no save point at all, and one only 3 s after the start.
    assert_eq!(refused(resolve(&message("u1", When::Before), &items, &[], &[])), ERR_BRANCH_NO_SAVE_POINT);
    let late = vec![checkpoint("2", 2, "after", 103)];
    assert_eq!(refused(resolve(&message("u1", When::Before), &items, &[], &late)), ERR_BRANCH_NO_SAVE_POINT);
    // After: the only save point landed after the bound (end + 12 s).
    let far = vec![checkpoint("2", 1, "after", 130)];
    let (code, message_text) = resolve(&message("u1", When::After), &items, &[], &far).err().unwrap();
    assert_eq!(code, ERR_BRANCH_NO_SAVE_POINT);
    assert_eq!(message_text, MSG_BRANCH_NO_SAVE_POINT);
}

#[test]
fn an_unknown_user_item_is_refused_with_the_find_turn_text() {
    let items = vec![user("u1", Some(100), "first ask"), turn_end("e1", Some(101))];
    let cps = vec![checkpoint("1", 1, "before", 100)];
    let (code, message_text) = resolve(&message("nope", When::Before), &items, &[], &cps).err().unwrap();
    assert_eq!(code, ERR_BRANCH_NO_SAVE_POINT);
    assert_eq!(message_text, "This message is not in this chat.");
    // An agent item is not the start of a turn either.
    assert_eq!(refused(resolve(&message("a1", When::After), &items, &[], &cps)), ERR_BRANCH_NO_SAVE_POINT);
}

#[test]
fn a_save_point_id_is_one_to_nine_digits() {
    assert!(checkpoint_id_ok("12"));
    assert!(checkpoint_id_ok("123456789"));
    assert!(!checkpoint_id_ok("../1"));
    assert!(!checkpoint_id_ok("1a"));
    assert!(!checkpoint_id_ok("1234567890"), "10 digits");
    assert!(!checkpoint_id_ok(""));

    let cps = vec![checkpoint("12", 3, "after", 200), checkpoint("1", 1, "before", 10)];
    let resolved = resolve(&BranchFrom::SavePoint { checkpoint_id: "12".into() }, &[], &[], &cps).unwrap();
    assert_eq!(resolved.point.checkpoint_id, "12");
    assert_eq!(resolved.point.turn, 3);
    assert_eq!(resolved.cut, Cut::UpToMs(ms(200)));
    assert_eq!(resolved.prefill, None);

    for bad in ["../1", "1a", "1234567890"] {
        let from = BranchFrom::SavePoint { checkpoint_id: bad.into() };
        assert_eq!(refused(resolve(&from, &[], &[], &cps)), ERR_BRANCH_NO_SAVE_POINT, "{bad}");
    }
    let unknown = BranchFrom::SavePoint { checkpoint_id: "99".into() };
    assert_eq!(refused(resolve(&unknown, &[], &[], &cps)), ERR_BRANCH_NO_SAVE_POINT);
}

#[test]
fn before_prefills_the_turns_own_message() {
    let items = vec![user("u1", Some(100), "try the parser again"), turn_end("e1", Some(101))];
    let cps = vec![checkpoint("1", 1, "before", 100)];
    let resolved = resolve(&message("u1", When::Before), &items, &[], &cps).unwrap();
    assert_eq!(resolved.prefill.as_deref(), Some("try the parser again"));
}
