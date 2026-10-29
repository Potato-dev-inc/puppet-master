use super::model::*;
use super::summary::*;
use crate::turn_summary::model::{EndState, ReceiptFacts};
use crate::turn_summary::TurnSummary;

/// 2026-10-14 12:00 UTC, the same fixed time the other focus tests use.
const T0: u64 = 1_791_979_200_000;
const MIN: u64 = MINUTE_MS;

fn session() -> Session {
    Session::new(T0, 45, By::Desktop, "fc-0123456789ab".into())
}

fn turn(pane: &str, turn: u32, at: u64, end: &str, outcome: Option<&str>) -> TurnFact {
    TurnFact { pane_id: pane.into(), turn, ended_at_ms: at, end: end.into(), outcome: outcome.map(String::from) }
}

/// Counts with the six numbers the line uses (agents and held left at zero).
fn c(done: u32, failed: u32, asked: u32, urgent: u32, still: u32) -> Counts {
    Counts { turns_done: done, agents: 0, failed, asked, urgent, still_waiting: still, held: HeldCounts::default() }
}

fn waiter(key: &str, tier: Tier) -> Waiter {
    Waiter { key: key.into(), tier, first_seen_ms: T0 + MIN, nudged: false }
}

#[test]
fn counts_come_from_the_turns_that_ended_in_the_session() {
    let turns = [
        turn("A", 1, T0 + MIN, "idle", None),
        turn("A", 2, T0 + 2 * MIN, "error", None),
        turn("B", 1, T0 + 3 * MIN, "waiting_input", None),
        turn("B", 2, T0 + 4 * MIN, "idle", Some("failed_checks")),
        turn("C", 1, T0 - MIN, "idle", None),
        turn("C", 2, T0 + 5 * MIN, "idle", Some("passed_checks")),
    ];
    let s = build(&session(), &Ledger::default(), &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    // Done: A1, B2 (idle), C2. C1 ended before the start and is ignored.
    assert_eq!(s.counts.turns_done, 3);
    // Agents: the distinct panes with a turn in the session (A, B, C).
    assert_eq!(s.counts.agents, 3);
    // Failed: A2 (error) and B2 (failed_checks). B1 is waiting, not failed.
    assert_eq!(s.counts.failed, 2);
    assert_eq!((s.counts.asked, s.counts.urgent, s.counts.still_waiting), (0, 0, 0));
}

#[test]
fn a_waiting_turn_is_not_done_and_not_failed() {
    let turns = [turn("A", 1, T0 + MIN, "waiting_input", None)];
    let s = build(&session(), &Ledger::default(), &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!((s.counts.turns_done, s.counts.failed, s.counts.agents), (0, 0, 1));
}

#[test]
fn the_same_pane_and_turn_is_counted_once() {
    let turns = [
        turn("A", 1, T0 + MIN, "idle", None),
        turn("A", 1, T0 + MIN, "idle", None),
        turn("A", 1, T0 + MIN, "idle", None),
        turn("B", 1, T0 + MIN, "idle", None),
        turn("A", 2, T0 + 2 * MIN, "error", None),
        turn("A", 2, T0 + 2 * MIN, "error", Some("failed")),
    ];
    let s = build(&session(), &Ledger::default(), &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    // A1 and B1 are two different (pane, turn) keys; the same key three times is one turn.
    assert_eq!(s.counts.turns_done, 2);
    assert_eq!(s.counts.agents, 2);
    // A2 is one failed turn, even though it appears twice and carries two signs of failure.
    assert_eq!(s.counts.failed, 1);
}

#[test]
fn failed_is_the_larger_of_failed_turns_and_held_failed_runs() {
    let turns = [turn("A", 2, T0 + MIN, "error", None), turn("B", 2, T0 + MIN, "idle", Some("failed"))];
    let mut ledger = Ledger::default();

    let two_turns = build(&session(), &ledger, &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!(two_turns.counts.failed, 2, "no held failed run: the turns decide");

    ledger.counts.run_failed = 1;
    let same_run = build(&session(), &ledger, &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!(same_run.counts.failed, 2, "a held run that is also a turn is not counted twice");

    ledger.counts.run_failed = 5;
    let more_held = build(&session(), &ledger, &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!(more_held.counts.failed, 5, "held failed runs above the turns count as well");

    let held_only = build(&session(), &Ledger { counts: HeldCounts { run_failed: 1, ..Default::default() }, ..Default::default() }, &[], 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!(held_only.counts.failed, 1, "a failed run that was only held still shows");
}

#[test]
fn asked_and_urgent_come_from_the_waiters_and_still_waiting_from_now() {
    let ledger = Ledger {
        waiters: vec![waiter("k1", Tier::Quiet), waiter("k2", Tier::Quiet), waiter("k3", Tier::Through)],
        counts: HeldCounts { run_done: 3, companion: 1, ..Default::default() },
        ..Default::default()
    };
    let s = build(&session(), &ledger, &[], 2, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!((s.counts.asked, s.counts.urgent, s.counts.still_waiting), (2, 1, 2));
    assert_eq!(s.counts.held, HeldCounts { run_done: 3, companion: 1, ..Default::default() });
}

#[test]
fn turns_before_the_start_leave_the_summary_empty() {
    let turns = [turn("A", 1, T0 - MIN, "idle", None), turn("A", 2, T0 - MIN, "error", None)];
    let s = build(&session(), &Ledger::default(), &turns, 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!(s.counts, Counts::default());
    assert!(is_empty(&s));
}

#[test]
fn line_for_a_full_timer_end() {
    assert_eq!(
        line(&c(4, 1, 2, 1, 1), 45, EndedBy::Timer, 45 * MIN),
        "While you focused (45 min): 4 turns done, 1 failed, 2 asked for you, 1 urgent. 1 still waits."
    );
}

#[test]
fn line_uses_singular_and_plural_forms() {
    assert_eq!(line(&c(1, 0, 1, 0, 2), 25, EndedBy::Timer, 25 * MIN), "While you focused (25 min): 1 turn done, 1 asked for you. 2 still wait.");
    assert_eq!(line(&c(1, 1, 0, 0, 0), 25, EndedBy::Timer, 25 * MIN), "While you focused (25 min): 1 turn done, 1 failed.");
    assert_eq!(line(&c(0, 2, 0, 0, 0), 25, EndedBy::Timer, 25 * MIN), "While you focused (25 min): 2 failed.");
    assert_eq!(line(&c(0, 0, 0, 0, 1), 45, EndedBy::Timer, 45 * MIN), "While you focused (45 min): 1 still waits.");
    assert_eq!(line(&c(0, 0, 0, 0, 2), 45, EndedBy::Timer, 45 * MIN), "While you focused (45 min): 2 still wait.");
    assert_eq!(line(&c(0, 0, 0, 1, 0), 45, EndedBy::Timer, 45 * MIN), "While you focused (45 min): 1 urgent.");
}

#[test]
fn line_for_nothing_says_quiet() {
    assert_eq!(line(&Counts::default(), 45, EndedBy::Timer, 45 * MIN), "Quiet. Nothing happened while you focused (45 min).");
}

#[test]
fn line_for_an_early_end_uses_the_actual_time() {
    let early = line(&c(4, 1, 2, 1, 1), 45, EndedBy::Early, 18 * MIN + 30_000);
    assert_eq!(early, "Focus ended after 18 min: 4 turns done, 1 failed, 2 asked for you, 1 urgent. 1 still waits.");
    assert_eq!(line(&Counts::default(), 45, EndedBy::Early, 18 * MIN), "Quiet. Nothing happened while you focused (18 min).");
}

#[test]
fn line_for_an_early_end_under_a_minute() {
    assert_eq!(line(&c(1, 0, 0, 0, 0), 45, EndedBy::Early, 20_000), "Focus ended after less than 1 min: 1 turn done.");
    assert_eq!(line(&Counts::default(), 45, EndedBy::Early, 20_000), "Quiet. Nothing happened while you focused (less than 1 min).");
}

#[test]
fn line_keeps_long_minutes_as_minutes() {
    assert_eq!(line(&c(12, 0, 0, 0, 0), 240, EndedBy::Timer, 240 * MIN), "While you focused (240 min): 12 turns done.");
    assert_eq!(line(&c(12, 0, 0, 0, 0), 240, EndedBy::Early, 135 * MIN), "Focus ended after 135 min: 12 turns done.");
}

#[test]
fn held_updates_are_counted_in_the_line_but_run_results_are_not_repeated() {
    let mut held = c(0, 0, 0, 0, 0);
    held.held = HeldCounts { companion: 1, review: 1, ..Default::default() };
    assert_eq!(line(&held, 45, EndedBy::Timer, 45 * MIN), "While you focused (45 min): 2 updates held.");

    held.held = HeldCounts { night: 1, ..Default::default() };
    assert_eq!(line(&held, 45, EndedBy::Timer, 45 * MIN), "While you focused (45 min): 1 update held.");

    // Finished and failed runs are already in the turn counts, so a held run result adds no part.
    held.held = HeldCounts { run_done: 3, run_failed: 1, ..Default::default() };
    assert_eq!(line(&held, 45, EndedBy::Timer, 45 * MIN), "Quiet. Nothing happened while you focused (45 min).");
}

#[test]
fn body_is_the_counts_part_without_a_header() {
    let counts = c(4, 1, 2, 0, 1);
    assert_eq!(body(&counts), "4 turns done, 1 failed, 2 asked for you. 1 still waits.");
    let text = body(&counts);
    assert!(!text.contains(':'), "no header colon");
    assert!(!text.contains("While you focused"));
    assert!(!text.contains("Focus ended"));
}

#[test]
fn body_with_only_a_waiting_count_and_empty_otherwise() {
    assert_eq!(body(&c(0, 0, 0, 0, 1)), "1 still waits.");
    assert_eq!(body(&Counts::default()), "");
}

#[test]
fn build_fills_the_summary_for_an_early_end() {
    let ledger = Ledger { waiters: vec![waiter("k1", Tier::Quiet)], ..Default::default() };
    let turns = [turn("A", 1, T0 + MIN, "idle", None)];
    let s = build(&session(), &ledger, &turns, 1, T0 + 18 * MIN, EndedBy::Early);
    assert_eq!(s.session_id, "fc-0123456789ab");
    assert_eq!((s.started_at_ms, s.ended_at_ms, s.minutes), (T0, T0 + 18 * MIN, 45));
    assert_eq!(s.ended_by, EndedBy::Early);
    assert_eq!(s.line, "Focus ended after 18 min: 1 turn done, 1 asked for you. 1 still waits.");
    assert_eq!(s.line, line(&s.counts, 45, EndedBy::Early, 18 * MIN));
}

#[test]
fn build_fills_the_summary_for_a_timer_end() {
    let s = build(&session(), &Ledger::default(), &[turn("A", 1, T0 + MIN, "idle", None)], 0, T0 + 45 * MIN, EndedBy::Timer);
    assert_eq!(s.line, "While you focused (45 min): 1 turn done.");
    assert_eq!(s.ended_by, EndedBy::Timer);
}

#[test]
fn is_empty_only_when_nothing_happened() {
    let empty = build(&session(), &Ledger::default(), &[], 0, T0 + 45 * MIN, EndedBy::Timer);
    assert!(is_empty(&empty));

    let turn_done = build(&session(), &Ledger::default(), &[turn("A", 1, T0 + MIN, "idle", None)], 0, T0 + 45 * MIN, EndedBy::Timer);
    assert!(!is_empty(&turn_done), "a turn is something");

    let waiting = build(&session(), &Ledger::default(), &[], 1, T0 + 45 * MIN, EndedBy::Timer);
    assert!(!is_empty(&waiting), "a card still waiting is something");

    let asked = build(&session(), &Ledger { waiters: vec![waiter("k1", Tier::Quiet)], ..Default::default() }, &[], 0, T0 + 45 * MIN, EndedBy::Timer);
    assert!(!is_empty(&asked), "a card that asked is something");

    let held = build(&session(), &Ledger { counts: HeldCounts { review: 1, ..Default::default() }, ..Default::default() }, &[], 0, T0 + 45 * MIN, EndedBy::Timer);
    assert!(!is_empty(&held), "a held update is something");
}

#[test]
fn turn_fact_is_read_from_a_turn_summary() {
    let mut t = sample_turn("idle", Some("passed_checks"));
    let fact = TurnFact::from(&t);
    assert_eq!(fact, TurnFact { pane_id: "pane-1".into(), turn: 7, ended_at_ms: T0 + MIN, end: "idle".into(), outcome: Some("passed_checks".into()) });

    t.receipt = None;
    t.end.kind = "error".into();
    let fact = TurnFact::from(&t);
    assert_eq!((fact.end.as_str(), fact.outcome), ("error", None));
}

fn sample_turn(end: &str, outcome: Option<&str>) -> TurnSummary {
    TurnSummary {
        pane_id: "pane-1".into(),
        agent: "Agent".into(),
        agent_id: None,
        account: None,
        turn: 7,
        started_at_ms: T0,
        ended_at_ms: T0 + MIN,
        duration_ms: 60_000,
        files: None,
        approvals: Vec::new(),
        end: EndState { kind: end.into(), prompt: None },
        receipt: outcome.map(|o| ReceiptFacts { checks: "2/2 passed".into(), outcome: o.into() }),
        sentence: String::new(),
    }
}

#[test]
fn ended_by_and_the_counts_serialize_in_snake_case() {
    let s = build(&session(), &Ledger::default(), &[turn("A", 1, T0 + MIN, "idle", None)], 0, T0 + 45 * MIN, EndedBy::Timer);
    let value = serde_json::to_value(&s).expect("summary serializes");
    assert_eq!(value["ended_by"], "timer");
    assert_eq!(value["session_id"], "fc-0123456789ab");
    assert_eq!(value["counts"]["turns_done"], 1);
    assert_eq!(value["counts"]["held"]["run_done"], 0);
    let early = serde_json::to_value(EndedBy::Early).expect("serializes");
    assert_eq!(early, "early");
}
