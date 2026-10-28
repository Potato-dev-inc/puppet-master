//! Tests for `classify`: one per row of SPEC n32 3.6, with the exact strings.

use super::*;

use crate::agent_runs::checks::counts::TestCounts;
use crate::missions::model::{AgentChoice, TestChanges};
use crate::night_shift::model::{AttemptKind, Review, TaskSpec, TaskState, Who};
use crate::tamper::{Finding, Kind, Severity};

const LABEL: &str = "Fake A";

fn agent() -> AgentChoice {
    AgentChoice { agent_type: "fake_a".into(), account: None, model: None }
}

fn attempt(kind: AttemptKind, end: Option<AttemptEnd>) -> Attempt {
    Attempt {
        n: 1,
        plan: agent(),
        label: LABEL.into(),
        kind,
        mission_id: None,
        handle: None,
        pane_ids: vec![],
        started_ms: 0,
        ended_ms: Some(120_000),
        end,
    }
}

fn task(attempts: Vec<Attempt>, declared: bool) -> Task {
    Task {
        spec: TaskSpec {
            id: "nt-00000001".into(),
            project: "C:/work/app".into(),
            text: "Fix the thing".into(),
            who: Who::Any,
            may_change_tests: false,
            turns: 12,
            minutes: 90,
        },
        checks: if declared { vec![vec!["npm".into(), "test".into()]] } else { vec![] },
        state: TaskState::Finished,
        attempts,
        outcome: None,
        review: Review::Pending,
    }
}

fn counts(passed: u32, total: u32) -> Option<TestCounts> {
    Some(TestCounts { passed, failed: total - passed, skipped: 0, total })
}

/// A check result with parsed test counts.
fn verdict(passed: u32, total: u32) -> Verdict {
    Verdict {
        counts: counts(passed, total),
        checks_passed: u32::from(passed == total),
        checks_total: 1,
        ..Verdict::default()
    }
}

/// A check result with no counts: "2 of 2 checks".
fn checks_only(passed: u32, total: u32) -> Verdict {
    Verdict { checks_passed: passed, checks_total: total, ..Verdict::default() }
}

/// A green check result whose test files were changed (a blocking finding).
fn tampered() -> Verdict {
    let mut v = verdict(12, 12);
    v.test_changes = TestChanges {
        blocks: true,
        findings: vec![Finding::new(Kind::TestsRemoved, Severity::Blocks, "tests/a.test.ts", "Removed a test".into())],
    };
    v
}

fn changed(files: usize) -> Option<Changes> {
    Some(Changes { files_changed: files, additions: 5, deletions: 2, ..Changes::default() })
}

fn mission(state: MissionState, needs: Option<Needs>, verdict: Option<Verdict>, turns: u32) -> MissionFacts {
    MissionFacts { state, needs, verdict, baseline: None, turns_used: turns }
}

fn facts(mission: Option<MissionFacts>, changes: Option<Changes>) -> Facts {
    Facts {
        mission,
        changes,
        summaries: vec![],
        copy: None,
        stop_hhmm: "07:00".into(),
        not_run: None,
    }
}

fn mission_task(end: Option<AttemptEnd>, declared: bool) -> Task {
    task(vec![attempt(AttemptKind::Mission, end)], declared)
}

// ---- Ready, NoChecks, NoChanges

#[test]
fn green_with_counts_is_ready() {
    let f = facts(Some(mission(MissionState::Green, None, Some(verdict(42, 42)), 2)), changed(3));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::Ready);
    assert_eq!(o.headline, "Tests pass 42 of 42");
    assert_eq!(o.reason, None);
    assert_eq!(o.tests, Some(Counts { passed: 42, total: 42 }));
    assert_eq!(o.files, Some((3, 5, 2)));
    assert!(o.checks_declared);
}

#[test]
fn green_without_counts_says_checks() {
    let f = facts(Some(mission(MissionState::Green, None, Some(checks_only(2, 2)), 1)), changed(1));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::Ready);
    assert_eq!(o.headline, "Tests pass 2 of 2 checks");
    assert_eq!(o.tests, None);
}

#[test]
fn single_turn_with_changes_is_no_checks() {
    let t = task(vec![attempt(AttemptKind::Single, Some(AttemptEnd::Completed))], false);
    let o = classify(&t, &facts(None, changed(2)));
    assert_eq!(o.class, Class::NoChecks);
    assert_eq!(o.headline, "Done. No tests declared, nothing was checked.");
    assert_eq!(o.reason, None);
    assert!(!o.checks_declared);
    assert_eq!(o.turns, 1);
    assert_eq!(o.minutes, 2);
}

#[test]
fn green_with_empty_diff_is_no_changes() {
    let f = facts(Some(mission(MissionState::Green, None, Some(verdict(3, 3)), 1)), changed(0));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NoChanges);
    assert_eq!(o.headline, "Made no changes.");
}

#[test]
fn single_turn_with_empty_diff_is_no_changes() {
    let t = task(vec![attempt(AttemptKind::Single, Some(AttemptEnd::Completed))], false);
    let o = classify(&t, &facts(None, changed(0)));
    assert_eq!(o.class, Class::NoChanges);
    assert_eq!(o.headline, "Made no changes.");
}

#[test]
fn unreadable_changes_need_a_look() {
    let f = facts(Some(mission(MissionState::Green, None, Some(verdict(3, 3)), 1)), None);
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Couldn't read the changes");
}

// ---- NeedsLook: tamper, stuck, budget, tests

#[test]
fn green_with_blocking_test_change_is_never_ready() {
    let f = facts(Some(mission(MissionState::Green, None, Some(tampered()), 2)), changed(2));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "The checks pass, but the tests were changed");
    assert_eq!(o.reason.as_deref(), Some(REASON_TAMPER));
    assert_eq!(o.test_change_lines, vec!["Removed a test".to_string()]);
}

#[test]
fn needs_tamper_is_the_same_headline() {
    let f = facts(Some(mission(MissionState::NeedsYou, Some(Needs::Tamper), Some(tampered()), 2)), changed(2));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, TAMPER);
}

#[test]
fn many_test_changes_cut_after_three() {
    let mut v = verdict(12, 12);
    let finding = || Finding::new(Kind::TestsRemoved, Severity::Blocks, "t.ts", "Removed".into());
    v.test_changes = TestChanges { blocks: true, findings: vec![finding(), finding(), finding(), finding(), finding()] };
    let f = facts(Some(mission(MissionState::Green, None, Some(v), 2)), changed(2));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(
        o.test_change_lines,
        vec![
            "Removed".to_string(),
            "Removed".to_string(),
            "Removed".to_string(),
            "and 2 more changes".to_string()
        ]
    );
}

#[test]
fn stuck_says_how_many_turns() {
    let f = facts(Some(mission(MissionState::NeedsYou, Some(Needs::Stuck), Some(verdict(9, 12)), 3)), changed(1));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "It stopped making progress after 3 turns");
    assert_eq!(o.reason.as_deref(), Some(REASON_STUCK));
}

#[test]
fn budget_says_used_of_turns() {
    let f = facts(Some(mission(MissionState::NeedsYou, Some(Needs::Budget), Some(verdict(9, 12)), 12)), changed(1));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Used its budget: 12 of 12 turns");
}

#[test]
fn checks_fail_at_the_end() {
    let f = facts(Some(mission(MissionState::Stopped, None, Some(verdict(9, 12)), 4)), changed(2));
    let o = classify(&mission_task(Some(AttemptEnd::Completed), true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Tests fail: 9 of 12 pass");
    assert_eq!(o.reason.as_deref(), Some(REASON_TESTS));
}

#[test]
fn checks_fail_on_a_red_baseline_show_both_numbers() {
    let mut m = mission(MissionState::Stopped, None, Some(verdict(9, 12)), 4);
    m.baseline = Some(verdict(7, 12));
    let o = classify(&mission_task(Some(AttemptEnd::Completed), true), &facts(Some(m), changed(2)));
    assert_eq!(o.headline, "Tests still fail (9 of 12, was 7 of 12)");
    assert_eq!(o.tests_before, Some(Counts { passed: 7, total: 12 }));
}

#[test]
fn baseline_that_passed_is_not_still_failing() {
    let mut m = mission(MissionState::Stopped, None, Some(verdict(9, 12)), 4);
    m.baseline = Some(verdict(12, 12));
    let o = classify(&mission_task(Some(AttemptEnd::Completed), true), &facts(Some(m), changed(2)));
    assert_eq!(o.headline, "Tests fail: 9 of 12 pass");
    assert_eq!(o.tests_before, None);
}

// ---- NeedsLook: the attempt's end

#[test]
fn cut_off_with_a_last_check() {
    let f = facts(Some(mission(MissionState::Working, None, Some(verdict(9, 12)), 5)), changed(2));
    let o = classify(&mission_task(Some(AttemptEnd::CutOff), true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Cut off at 07:00 while working. Last check: 9 of 12 pass");
    assert_eq!(o.reason.as_deref(), Some(REASON_RUN_AGAIN));
}

#[test]
fn cut_off_without_a_check() {
    let t = task(vec![attempt(AttemptKind::Single, Some(AttemptEnd::CutOff))], false);
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Cut off at 07:00 while working.");
}

#[test]
fn cut_off_uses_the_stop_time_it_was_given() {
    let mut f = facts(None, None);
    f.stop_hhmm = "06:30".into();
    let t = task(vec![attempt(AttemptKind::Single, Some(AttemptEnd::CutOff))], false);
    assert_eq!(classify(&t, &f).headline, "Cut off at 06:30 while working.");
}

#[test]
fn asked_a_question() {
    let t = task(
        vec![attempt(AttemptKind::Single, Some(AttemptEnd::Asked { question: "Which file?".into() }))],
        false,
    );
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Stopped to ask a question");
    assert_eq!(o.reason.as_deref(), Some(REASON_OPEN_TERMINAL));
}

#[test]
fn trust_gate_names_the_agent_from_its_label() {
    let end = AttemptEnd::Gate { code: "WORKSPACE_TRUST_DECLINED".into(), sentence: "ignored".into() };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Fake A asked to trust the folder.");
    assert_eq!(o.reason.as_deref(), Some(REASON_OPEN_TERMINAL));
}

#[test]
fn gate_sentences_exact() {
    assert_eq!(
        gate_sentence("STARTUP_SCREEN_NOT_COMPLETED", LABEL).as_deref(),
        Some("Fake A stopped at a question before it started.")
    );
    assert_eq!(
        gate_sentence("PANE_BLOCKED_BY_PROMPT", LABEL).as_deref(),
        Some("Fake A stopped at a question before it started.")
    );
    assert_eq!(gate_sentence("AGENT_NOT_INSTALLED", LABEL).as_deref(), Some("Fake A isn't installed."));
    assert_eq!(
        gate_sentence("ACCOUNT_MISMATCH", LABEL).as_deref(),
        Some("Fake A isn't signed in to that account.")
    );
    assert_eq!(gate_sentence("SOMETHING_ELSE", LABEL), None);
}

#[test]
fn gate_code_not_listed_uses_its_sentence() {
    let end = AttemptEnd::Gate { code: "SOMETHING_ELSE".into(), sentence: "Needs a login.".into() };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    assert_eq!(classify(&t, &facts(None, changed(1))).headline, "Needs a login.");
}

#[test]
fn agent_not_installed_is_didnt_run() {
    let end = AttemptEnd::Gate { code: "AGENT_NOT_INSTALLED".into(), sentence: String::new() };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    let o = classify(&t, &facts(None, None));
    assert_eq!(o.class, Class::DidntRun);
    assert_eq!(o.headline, "Didn't run: Fake A isn't installed");
}

#[test]
fn out_of_usage_once() {
    let end = AttemptEnd::OutOfUsage { reset_at_ms: Some(1) };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Ran out of usage");
    assert_eq!(o.reason.as_deref(), Some(REASON_USAGE));
}

#[test]
fn out_of_usage_twice() {
    let end = || Some(AttemptEnd::OutOfUsage { reset_at_ms: None });
    let t = task(vec![attempt(AttemptKind::Single, end()), attempt(AttemptKind::Single, end())], false);
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.headline, "Ran out of usage twice");
}

#[test]
fn agent_failed_uses_its_sentence() {
    let end = AttemptEnd::Failed { code: "PANE_LOST".into(), sentence: "The agent window closed.".into() };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "The agent window closed.");
}

#[test]
fn needs_you_setup_falls_back_to_its_reason() {
    let end = AttemptEnd::NeedsYou { why: "setup".into(), sentence: "  ".into() };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    assert_eq!(classify(&t, &facts(None, changed(1))).headline, "setup");
}

#[test]
fn stopped_by_the_person() {
    let t = task(vec![attempt(AttemptKind::Single, Some(AttemptEnd::StoppedByPerson))], false);
    let o = classify(&t, &facts(None, changed(1)));
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Stopped by you");
}

// ---- DidntRun

#[test]
fn no_attempt_is_didnt_run_with_the_reason() {
    let t = task(vec![], false);
    let mut f = facts(None, None);
    f.not_run = Some("No time left".into());
    let o = classify(&t, &f);
    assert_eq!(o.class, Class::DidntRun);
    assert_eq!(o.headline, "Didn't run: No time left");
    assert_eq!(o.reason, None);
}

#[test]
fn no_attempt_and_no_reason_says_it_never_started() {
    let o = classify(&task(vec![], false), &facts(None, None));
    assert_eq!(o.class, Class::DidntRun);
    assert_eq!(o.headline, "Didn't run: it never started");
}

// ---- Shared fields

#[test]
fn summaries_are_cut_to_eight() {
    let mut f = facts(None, changed(1));
    f.summaries = (0..12).map(|i| format!("Turn {i}.")).collect();
    let t = task(vec![attempt(AttemptKind::Single, Some(AttemptEnd::Completed))], false);
    let o = classify(&t, &f);
    assert_eq!(o.summaries.len(), MAX_SUMMARIES);
    assert_eq!(o.summaries[0], "Turn 0.");
}

#[test]
fn copy_and_minutes_come_through() {
    let copy = CopyRef {
        project: "C:/work/app".into(),
        handle: "mission-abc".into(),
        branch: "pm/mission-abc".into(),
        base: "main".into(),
        pane_id: None,
    };
    let mut f = facts(Some(mission(MissionState::Green, None, Some(verdict(3, 3)), 2)), changed(1));
    f.copy = Some(copy.clone());
    let mut t = mission_task(None, true);
    t.attempts[0].ended_ms = Some(9 * 60_000 + 30_000);
    let o = classify(&t, &f);
    assert_eq!(o.copy, Some(copy));
    assert_eq!(o.minutes, 9);
    assert_eq!(o.turns, 2);
}

#[test]
fn mission_green_that_fails_a_check_is_not_ready() {
    let f = facts(Some(mission(MissionState::Green, None, Some(checks_only(1, 2)), 1)), changed(1));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "Tests fail: 1 of 2 checks pass");
}

#[test]
fn no_check_result_on_green_needs_a_look() {
    let f = facts(Some(mission(MissionState::Green, None, None, 1)), changed(1));
    let o = classify(&mission_task(None, true), &f);
    assert_eq!(o.class, Class::NeedsLook);
    assert_eq!(o.headline, "No check result was recorded");
}

#[test]
fn headlines_carry_no_agent_name_except_the_label() {
    let end = AttemptEnd::Gate { code: "ACCOUNT_MISMATCH".into(), sentence: String::new() };
    let t = task(vec![attempt(AttemptKind::Single, Some(end))], false);
    let o = classify(&t, &facts(None, changed(1)));
    let lower = o.headline.to_lowercase();
    for name in ["claude", "codex", "cursor", "opencode", "agy"] {
        assert!(!lower.contains(name), "agent name leaked: {name}");
    }
    assert!(o.headline.starts_with(LABEL));
}
