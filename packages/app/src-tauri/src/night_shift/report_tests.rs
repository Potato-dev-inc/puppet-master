//! Tests for `report`: the summary line with exact strings, the push body and the time texts.

use super::*;

use crate::night_shift::model::{Attempt, Outcome, Review, RunState, Task, TaskSpec, TaskState, Who};

fn outcome(class: Class) -> Outcome {
    Outcome {
        class,
        headline: String::new(),
        reason: None,
        tests: None,
        tests_before: None,
        checks_declared: true,
        test_change_lines: vec![],
        files: None,
        turns: 1,
        minutes: 1,
        summaries: vec![],
        copy: None,
    }
}

fn task(outcome: Option<Outcome>) -> Task {
    Task {
        spec: TaskSpec {
            id: "nt-00000001".into(),
            project: "C:/work/app".into(),
            text: "Task text stays local".into(),
            who: Who::Any,
            may_change_tests: false,
            turns: 12,
            minutes: 90,
        },
        checks: vec![],
        state: TaskState::Finished,
        attempts: Vec::<Attempt>::new(),
        outcome,
        review: Review::Pending,
    }
}

/// A run with one task per entry. `None` is a task with no outcome yet.
fn run(classes: &[Option<Class>]) -> NightRun {
    NightRun {
        id: "ns-0123456789ab".into(),
        created_ms: 0,
        start_at_ms: 0,
        stop_at_ms: 0,
        state: RunState::Done,
        plans: vec![],
        parallel: 2,
        keep_awake: false,
        awake_ok: false,
        tasks: classes.iter().map(|c| task(c.map(outcome))).collect(),
        plan_out: vec![],
        blocked: vec![],
        events: vec![],
        last_tick_ms: 0,
        finished_ms: None,
        seen_ms: None,
        push_sent: false,
        rev: 0,
    }
}

fn s(total: u32, done: u32, green: u32, needs_you: u32, didnt_run: u32, no_changes: u32) -> Summary {
    Summary { total, done, green, needs_you, didnt_run, no_changes }
}

#[test]
fn five_of_five_green() {
    let r = run(&[Some(Class::Ready); 5]);
    let sum = summary(&r);
    assert_eq!(sum, s(5, 5, 5, 0, 0, 0));
    assert_eq!(line(&sum), "5 of 5 done, tests green");
}

#[test]
fn four_of_five_with_one_needing_you() {
    let r = run(&[Some(Class::Ready), Some(Class::Ready), Some(Class::Ready), Some(Class::Ready), Some(Class::NeedsLook)]);
    assert_eq!(line(&summary(&r)), "4 of 5 done, tests green. 1 needs you.");
}

#[test]
fn zero_of_five_done() {
    let r = run(&[
        Some(Class::NeedsLook),
        Some(Class::NeedsLook),
        Some(Class::NeedsLook),
        Some(Class::DidntRun),
        Some(Class::DidntRun),
    ]);
    assert_eq!(line(&summary(&r)), "0 of 5 done. 3 need you. 2 didn't run.");
}

#[test]
fn one_need_you_is_singular() {
    let r = run(&[Some(Class::NeedsLook)]);
    assert_eq!(line(&summary(&r)), "0 of 1 done. 1 needs you.");
}

#[test]
fn only_no_checks_says_no_tests_ran() {
    let r = run(&[Some(Class::NoChecks), Some(Class::NoChecks)]);
    assert_eq!(line(&summary(&r)), "2 of 2 done, no tests ran");
}

#[test]
fn mix_of_ready_and_no_checks() {
    let r = run(&[Some(Class::Ready), Some(Class::Ready), Some(Class::NoChecks), Some(Class::NeedsLook)]);
    let sum = summary(&r);
    assert_eq!(sum, s(4, 3, 2, 1, 0, 0));
    assert_eq!(line(&sum), "3 of 4 done, tests green on 2. 1 needs you.");
}

#[test]
fn no_changes_are_not_done() {
    let r = run(&[Some(Class::Ready), Some(Class::NoChanges)]);
    assert_eq!(line(&summary(&r)), "1 of 2 done, tests green. 1 changed nothing.");
}

#[test]
fn didnt_run_starts_a_sentence_when_nothing_else_did() {
    let r = run(&[Some(Class::Ready), Some(Class::DidntRun), Some(Class::NoChanges)]);
    assert_eq!(line(&summary(&r)), "1 of 3 done, tests green. 1 didn't run. 1 changed nothing.");
    let r = run(&[Some(Class::NoChecks), Some(Class::DidntRun)]);
    assert_eq!(line(&summary(&r)), "1 of 2 done, no tests ran. 1 didn't run.");
}

#[test]
fn zero_tasks() {
    let r = run(&[]);
    assert_eq!(summary(&r), s(0, 0, 0, 0, 0, 0));
    assert_eq!(line(&summary(&r)), "0 of 0 done");
}

#[test]
fn tasks_without_an_outcome_count_in_total_only() {
    let r = run(&[Some(Class::Ready), None, None]);
    assert_eq!(summary(&r), s(3, 1, 1, 0, 0, 0));
    assert_eq!(line(&summary(&r)), "1 of 3 done, tests green");
}

#[test]
fn push_body_counts_only_or_hidden() {
    let sum = s(5, 4, 4, 1, 0, 0);
    assert_eq!(push_body(&sum, false), "4 of 5 done, tests green. 1 needs you.");
    assert_eq!(push_body(&sum, true), "Night Shift is ready");
}

#[test]
fn duration_text_cases() {
    assert_eq!(duration_text(0), "under a minute");
    assert_eq!(duration_text(59_999), "under a minute");
    assert_eq!(duration_text(45 * 60_000), "45 min");
    assert_eq!(duration_text(120 * 60_000), "2 h");
    assert_eq!(duration_text(125 * 60_000), "2 h 5 min");
}

/// 2026-10-28 07:00 UTC, in milliseconds.
const OCT_28_0700_UTC: u64 = (20_754 * 86_400 + 7 * 3_600) * 1000;

#[test]
fn civil_day_numbers() {
    assert_eq!(day_text(20_754), "28 Oct");
    assert_eq!(day_text(20_760), "3 Nov");
    assert_eq!(day_text(0), "1 Jan");
}

#[test]
fn hhmm_uses_the_local_offset() {
    assert_eq!(hhmm(OCT_28_0700_UTC, 0), "07:00");
    assert_eq!(hhmm(OCT_28_0700_UTC, 120), "09:00");
    assert_eq!(hhmm(OCT_28_0700_UTC, -300), "02:00");
}

#[test]
fn when_text_today_tomorrow_later() {
    let at = OCT_28_0700_UTC;
    let evening_before = at + 15 * 3_600_000;
    assert_eq!(when_text(at, at - 3_600_000, 0), "today at 07:00");
    assert_eq!(when_text(at + 86_400_000, evening_before, 0), "tomorrow at 07:00");
    assert_eq!(when_text(at + 6 * 86_400_000, at, 0), "on 3 Nov at 07:00");
}
