//! The joins with fixtures. A wrong join gives a believable but false number, so every edge has a
//! test: same pane and two runs, a summary just outside a run's time, a run with no summaries, an
//! Undo after a merge, the 2 s Undo edge, and the merge fact that lives only on a run's last record.

use super::*;
use crate::operations::OperationSnapshot;
use crate::turn_summary::model::{assemble, Approval, Facts, FileStats, ReceiptFacts};
use serde_json::json;

const SEC: u64 = 1_000;
const T0: u64 = 1_800_000_000_000;
const ORIGIN: &str = "C:/Users/you/shop";

fn proj(path: &str) -> String {
    crate::project_path::comparable_project_path(Path::new(path))
}

fn no_label(_: &str) -> Option<String> {
    None
}

/// A finished record of a run's turn: `status` at `start..end` (ms after T0 is not applied: pass
/// absolute times).
fn op(id: &str, run: &str, pane: Option<&str>, status: &str, start: u64, end: Option<u64>) -> OperationSnapshot {
    serde_json::from_value(json!({
        "operation_id": id,
        "project_path": ORIGIN,
        "task": "SECRET TASK TEXT fix the login",
        "agent_type": "alpha",
        "pane_id": pane,
        "idempotency_key": format!("key-{id}"),
        "exclusive": false,
        "locks": [],
        "status": status,
        "revision": 1,
        "created_at_ms": start,
        "updated_at_ms": end.unwrap_or(start),
        "source": "native",
        "result": "SECRET RESULT TEXT all done",
        "agent_run_id": run,
        "started_at_ms": start,
        "finished_at_ms": end,
    }))
    .expect("fixture op")
}

fn done(id: &str, run: &str, pane: &str, start: u64, end: u64) -> OperationSnapshot {
    op(id, run, Some(pane), "completed", start, Some(end))
}

fn with_copy(mut o: OperationSnapshot, closed: Option<&str>) -> OperationSnapshot {
    o.worker.worktree = Some(crate::worktrees::WorktreeRef {
        path: "C:/Users/you/shop/.puppet-master/worktrees/run-1".into(),
        cwd: "C:/Users/you/shop/.puppet-master/worktrees/run-1".into(),
        branch: "pm/run-1".into(),
        base: "main".into(),
        base_commit: "abc".into(),
        repo: ORIGIN.into(),
        handle: o.agent_run_id.clone(),
        created_at_ms: 0,
        closed: closed.map(String::from),
        changes: None,
        setup: None,
        pr: None,
    });
    o
}

fn files(count: u32, added: u32, deleted: u32) -> FileStats {
    FileStats { count, added, deleted, paths: vec!["C:/Users/you/shop/SECRET_FILE_NAME.ts".into()] }
}

fn facts(pane: &str, turn: u32, start: u64, end: u64, files: Option<FileStats>) -> Facts {
    Facts {
        pane_id: pane.into(),
        agent: "Alpha Code".into(),
        agent_id: Some("alpha".into()),
        account: None,
        turn,
        started_at_ms: start,
        ended_at_ms: end,
        files,
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: None,
    }
}

fn sum(f: Facts) -> ProjectSummary {
    ProjectSummary { project: ORIGIN.into(), summary: assemble(f) }
}

fn plain(pane: &str, turn: u32, start: u64, end: u64) -> ProjectSummary {
    sum(facts(pane, turn, start, end, Some(files(2, 10, 5))))
}

fn mark(pane: &str, since: u64, at: u64) -> UndoMark {
    UndoMark { pane: pane.into(), since_ms: since, at_ms: at }
}

fn run_join(ops: &[OperationSnapshot], sums: &[ProjectSummary]) -> (Vec<ScoreTurn>, Vec<ScoreRun>) {
    join(ops, sums, &[], None, &no_label)
}

// ---- pairing a summary with its record ----

#[test]
fn a_turn_in_both_records_is_one_turn_with_the_records_facts_and_the_summarys_files() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 60 * SEC)];
    let sums = [plain("p1", 1, T0 + 2 * SEC, T0 + 58 * SEC)];
    let (turns, _) = run_join(&ops, &sums);
    assert_eq!(turns.len(), 1);
    let t = &turns[0];
    assert_eq!((t.agent_id.as_str(), t.started_ms, t.ended_ms), ("alpha", T0, T0 + 60 * SEC));
    assert_eq!(t.project, proj(ORIGIN));
    assert_eq!(t.end, End::Ok);
    assert_eq!((t.files_changed, t.lines), (Some(2), Some(15)));
    assert_eq!(t.asked_you, Some(false));
}

#[test]
fn the_record_wins_over_a_summary_that_names_another_agent() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 60 * SEC)];
    let mut s = facts("p1", 1, T0, T0 + 60 * SEC, None);
    s.agent_id = Some("beta".into());
    s.account = Some("acct-summary".into());
    let (turns, _) = run_join(&ops, &[sum(s)]);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].agent_id, "alpha");
    // The record has no account, so the summary's fills the gap.
    assert_eq!(turns[0].account.as_deref(), Some("acct-summary"));
}

#[test]
fn a_summary_up_to_15_seconds_from_a_run_is_its_turn_and_one_millisecond_further_is_not() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 60 * SEC)];
    // The summary ended exactly 15 s before the record started, and one ms earlier than that.
    let inside = [plain("p1", 1, T0 - 40 * SEC, T0 - 15 * SEC)];
    let outside = [plain("p1", 1, T0 - 40 * SEC, T0 - 15 * SEC - 1)];
    assert_eq!(run_join(&ops, &inside).0.len(), 1, "inside the slack: one turn");
    let (turns, _) = run_join(&ops, &outside);
    assert_eq!(turns.len(), 2, "outside the slack: two different turns, not one");
    assert!(turns.iter().any(|t| t.files_changed.is_none()), "the record alone has no files");
    assert!(turns.iter().any(|t| t.files_changed == Some(2)), "the summary alone has them");
}

#[test]
fn a_summary_just_after_a_run_ends_is_checked_the_same_way() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 60 * SEC)];
    let inside = [plain("p1", 1, T0 + 75 * SEC, T0 + 90 * SEC)];
    let outside = [plain("p1", 1, T0 + 75 * SEC + 1, T0 + 90 * SEC)];
    assert_eq!(run_join(&ops, &inside).0.len(), 1);
    assert_eq!(run_join(&ops, &outside).0.len(), 2);
}

#[test]
fn a_summary_on_another_pane_is_never_paired() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 60 * SEC)];
    let (turns, _) = run_join(&ops, &[plain("p2", 1, T0, T0 + 60 * SEC)]);
    assert_eq!(turns.len(), 2);
}

#[test]
fn a_record_without_a_pane_is_never_paired() {
    let ops = [op("o1", "r1", None, "completed", T0, Some(T0 + 60 * SEC))];
    let (turns, _) = run_join(&ops, &[plain("p1", 1, T0, T0 + 60 * SEC)]);
    assert_eq!(turns.len(), 2);
}

#[test]
fn one_record_takes_one_summary_only() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 60 * SEC)];
    let sums = [plain("p1", 1, T0, T0 + 30 * SEC), plain("p1", 2, T0 + 31 * SEC, T0 + 60 * SEC)];
    let (turns, _) = run_join(&ops, &sums);
    assert_eq!(turns.len(), 2, "the second summary is a turn of its own, not a second copy of the record");
}

#[test]
fn the_same_summary_from_memory_and_from_the_ring_is_one_turn() {
    let s = plain("p1", 1, T0, T0 + 30 * SEC);
    let (turns, _) = run_join(&[], &[s.clone(), s]);
    assert_eq!(turns.len(), 1);
}

#[test]
fn of_two_copies_of_one_turn_the_later_summary_wins() {
    // "Waiting" so far, then the final one: the same turn, the same start.
    let mut early = facts("p1", 1, T0, T0 + 20 * SEC, Some(files(1, 1, 0)));
    early.end_status = "waiting_input".into();
    let late = facts("p1", 1, T0, T0 + 90 * SEC, Some(files(4, 40, 0)));
    for order in [[sum(early.clone()), sum(late.clone())], [sum(late), sum(early)]] {
        let (turns, _) = run_join(&[], &order);
        assert_eq!(turns.len(), 1);
        assert_eq!((turns[0].files_changed, turns[0].ended_ms), (Some(4), T0 + 90 * SEC));
        assert_eq!(turns[0].asked_you, Some(false));
    }
}

// ---- same pane, two runs ----

fn two_runs_on_one_pane() -> ([OperationSnapshot; 2], [ProjectSummary; 2]) {
    // The second run starts 12 s after the first ends: each summary overlaps BOTH records.
    let ops = [done("o1", "run-a", "p1", T0, T0 + 10 * SEC), done("o2", "run-b", "p1", T0 + 22 * SEC, T0 + 40 * SEC)];
    let sums = [
        sum(facts("p1", 1, T0, T0 + 10 * SEC, Some(files(1, 3, 0)))),
        sum(facts("p1", 2, T0 + 22 * SEC, T0 + 40 * SEC, Some(files(7, 70, 0)))),
    ];
    (ops, sums)
}

#[test]
fn two_runs_on_one_pane_keep_their_own_summaries() {
    let (ops, sums) = two_runs_on_one_pane();
    let (turns, runs) = run_join(&ops, &sums);
    assert_eq!(turns.len(), 2);
    assert_eq!((turns[0].started_ms, turns[0].files_changed), (T0, Some(1)));
    assert_eq!((turns[1].started_ms, turns[1].files_changed), (T0 + 22 * SEC, Some(7)));
    assert_eq!(runs.len(), 2);
}

#[test]
fn the_pairing_does_not_depend_on_the_order_of_the_input() {
    let (ops, sums) = two_runs_on_one_pane();
    let forward = run_join(&ops, &sums);
    let reversed_ops: Vec<_> = ops.iter().rev().cloned().collect();
    let reversed_sums: Vec<_> = sums.iter().rev().cloned().collect();
    assert_eq!(forward, run_join(&reversed_ops, &reversed_sums));
}

#[test]
fn a_lost_summary_does_not_let_the_next_one_take_its_record() {
    let (ops, sums) = two_runs_on_one_pane();
    // Only the second run's summary survived. It must pair with the second record.
    let (turns, _) = run_join(&ops, &sums[1..]);
    assert_eq!(turns.len(), 2);
    assert_eq!((turns[0].started_ms, turns[0].files_changed), (T0, None), "the first record stays alone");
    assert_eq!((turns[1].started_ms, turns[1].files_changed), (T0 + 22 * SEC, Some(7)));
}

#[test]
fn a_late_starting_agent_still_pairs_with_its_own_record() {
    // The record was sent at T0 and the agent began 20 s later; the next record is right behind.
    let ops = [done("o1", "run-a", "p1", T0, T0 + 60 * SEC), done("o2", "run-b", "p1", T0 + 61 * SEC, T0 + 100 * SEC)];
    let sums = [
        sum(facts("p1", 1, T0 + 20 * SEC, T0 + 60 * SEC, Some(files(1, 1, 1)))),
        sum(facts("p1", 2, T0 + 62 * SEC, T0 + 100 * SEC, Some(files(9, 9, 9)))),
    ];
    let (turns, _) = run_join(&ops, &sums);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].files_changed, Some(1));
    assert_eq!(turns[1].files_changed, Some(9));
}

// ---- summary-only and record-only turns ----

#[test]
fn a_turn_typed_into_a_pane_has_only_its_summary() {
    let mut f = facts("p9", 3, T0, T0 + 30 * SEC, Some(files(2, 4, 6)));
    f.agent_id = None;
    f.account = Some("acct-1".into());
    let s = ProjectSummary { project: ORIGIN.into(), summary: assemble(f) };
    let label = |l: &str| (l == "Alpha Code").then(|| "alpha".to_string());
    let (turns, runs) = join(&[], &[s], &[], None, &label);
    assert!(runs.is_empty(), "a hand-typed turn is no run");
    let t = &turns[0];
    assert_eq!((t.agent_id.as_str(), t.account.as_deref()), ("alpha", Some("acct-1")));
    assert_eq!((t.started_ms, t.ended_ms, t.end), (T0, T0 + 30 * SEC, End::Ok));
    assert_eq!((t.files_changed, t.lines, t.asked_you), (Some(2), Some(10), Some(false)));
    assert_eq!(t.checks_green, None);
    assert_eq!(t.pane_id.as_deref(), Some("p9"));
}

#[test]
fn an_unknown_label_stays_visible_as_its_own_name() {
    let mut f = facts("p9", 1, T0, T0 + SEC, None);
    f.agent_id = None;
    let (turns, _) = join(&[], &[sum(f)], &[], None, &no_label);
    assert_eq!(turns[0].agent_id, "Alpha Code");
}

#[test]
fn a_summary_only_turn_that_ended_in_error_failed() {
    let mut f = facts("p1", 1, T0, T0 + SEC, None);
    f.end_status = "error".into();
    assert_eq!(run_join(&[], &[sum(f)]).0[0].end, End::Failed);
}

#[test]
fn a_summary_only_turn_waiting_for_an_answer_is_ok_and_asked_you() {
    let mut f = facts("p1", 1, T0, T0 + SEC, None);
    f.end_status = "waiting_input".into();
    let t = &run_join(&[], &[sum(f)]).0[0];
    assert_eq!((t.end, t.asked_you), (End::Ok, Some(true)));
}

#[test]
fn a_receipt_on_a_summary_without_a_record_still_tells_the_outcome() {
    let receipt = |outcome: &str| ReceiptFacts { checks: "1/2 passed".into(), outcome: outcome.into() };
    let one = |outcome: &str| {
        let mut f = facts("p1", 1, T0, T0 + SEC, None);
        f.receipt = Some(receipt(outcome));
        run_join(&[], &[sum(f)]).0.remove(0)
    };
    let (failed_checks, passed, cancelled, failed) = (one("failed_checks"), one("passed_checks"), one("cancelled"), one("failed"));
    assert_eq!((failed_checks.end, failed_checks.checks_green), (End::Failed, Some(false)));
    assert_eq!((passed.end, passed.checks_green), (End::Ok, Some(true)));
    assert_eq!((cancelled.end, cancelled.checks_green), (End::Stopped, None));
    assert_eq!((failed.end, failed.checks_green), (End::Failed, None));
    assert_eq!(one("completed_unchecked").checks_green, None);
}

#[test]
fn a_summary_without_checkpoints_has_unknown_files_but_a_known_answer_count() {
    let f = facts("p1", 1, T0, T0 + SEC, None);
    let t = &run_join(&[], &[sum(f)]).0[0];
    assert_eq!((t.files_changed, t.lines), (None, None));
    assert_eq!(t.asked_you, Some(false));
}

#[test]
fn approved_prompts_make_a_turn_asked_you() {
    let mut f = facts("p1", 1, T0, T0 + SEC, None);
    f.approvals = vec![Approval { at_ms: T0, prompt_id: "x".into(), prompt: "allow rm?".into(), kind: "command".into(), decision: "deny".into() }];
    assert_eq!(run_join(&[], &[sum(f)]).0[0].asked_you, Some(true));
}

#[test]
fn a_record_without_a_summary_has_unknown_files_and_answers() {
    let ops = [done("o1", "r1", "p1", T0, T0 + 5 * SEC)];
    let (turns, _) = run_join(&ops, &[]);
    let t = &turns[0];
    assert_eq!((t.files_changed, t.lines, t.asked_you, t.undone), (None, None, None, None));
    assert_eq!((t.end, t.pane_id.as_deref()), (End::Ok, Some("p1")));
}

#[test]
fn a_record_with_a_blank_account_has_no_account() {
    let mut o = done("o1", "r1", "p1", T0, T0 + SEC);
    o.worker.account = Some("  ".into());
    assert_eq!(run_join(&[o], &[]).0[0].account, None);
    let mut o = done("o2", "r2", "p1", T0, T0 + SEC);
    o.worker.account = Some("acct-1".into());
    assert_eq!(run_join(&[o], &[]).0[0].account.as_deref(), Some("acct-1"));
}

// ---- outcomes ----

#[test]
fn outcomes_follow_the_record_status() {
    let one = |status: &str, started: bool| {
        let mut o = op("o1", "r1", Some("p1"), status, T0, Some(T0 + SEC));
        if !started {
            o.started_at_ms = None;
        }
        run_join(&[o], &[]).0
    };
    assert_eq!(one("completed", true)[0].end, End::Ok);
    assert_eq!(one("failed", true)[0].end, End::Failed);
    assert_eq!(one("cancelled", true)[0].end, End::Stopped, "stopped after it started");
    assert!(one("cancelled", false).is_empty(), "withdrawn before it started: not a turn");
    assert_eq!(one("failed", false)[0].end, End::Failed, "a start that failed is still a failure");
}

#[test]
fn turns_that_have_not_finished_are_left_out_with_their_summary() {
    for status in ["queued", "starting", "running", "waiting_input", "cancelling"] {
        let ops = [op("o1", "r1", Some("p1"), status, T0, None)];
        let mut f = facts("p1", 1, T0, T0 + 20 * SEC, Some(files(1, 1, 1)));
        f.end_status = "waiting_input".into();
        let (turns, runs) = run_join(&ops, &[sum(f)]);
        assert!(turns.is_empty(), "{status}: no outcome yet, no turn (and no second copy from the summary)");
        assert_eq!(runs.len(), 1, "{status}: the run exists");
        assert_eq!(runs[0].turns, 0);
    }
}

#[test]
fn a_withdrawn_turn_neither_counts_nor_takes_a_summary() {
    let mut o = op("o1", "r1", Some("p1"), "cancelled", T0, Some(T0 + SEC));
    o.started_at_ms = None;
    let (turns, runs) = run_join(&[o], &[plain("p1", 1, T0, T0 + SEC)]);
    assert_eq!(turns.len(), 1, "only the summary");
    assert_eq!(turns[0].files_changed, Some(2));
    assert!(runs.is_empty(), "nothing ran");
}

// ---- checks ----

fn check(path: &str) -> crate::agent_runs::checks::Check {
    crate::agent_runs::checks::Check::FileExists { path: path.into() }
}

fn result(path: &str, passed: bool) -> crate::agent_runs::checks::CheckResult {
    crate::agent_runs::checks::CheckResult { check: check(path), passed, detail: "SECRET DETAIL".into() }
}

fn checked(status: &str, checks: usize, results: &[bool], code: Option<&str>) -> Option<bool> {
    let mut o = op("o1", "r1", Some("p1"), status, T0, Some(T0 + SEC));
    o.checks = (0..checks).map(|i| check(&format!("f{i}"))).collect();
    o.check_results = results.iter().enumerate().map(|(i, p)| result(&format!("f{i}"), *p)).collect();
    o.error = code.map(|c| crate::operations::OperationError::new(c, "SECRET ERROR TEXT", false));
    run_join(&[o], &[]).0[0].checks_green
}

#[test]
fn checks_are_green_only_when_all_ran_and_passed() {
    assert_eq!(checked("completed", 2, &[true, true], None), Some(true));
    assert_eq!(checked("completed", 2, &[true], None), None, "one check never answered");
    assert_eq!(checked("completed", 1, &[], None), None, "no results at all");
    assert_eq!(checked("completed", 1, &[false], None), None, "cannot happen; never shown as proven");
}

#[test]
fn a_failed_check_is_red() {
    assert_eq!(checked("failed", 2, &[true, false], Some("ACCEPTANCE_FAILED")), Some(false));
}

#[test]
fn a_run_that_failed_for_another_reason_never_ran_its_checks() {
    assert_eq!(checked("failed", 2, &[], Some("TIMEOUT")), None);
    assert_eq!(checked("failed", 2, &[], None), None);
    assert_eq!(checked("cancelled", 2, &[], None), None);
}

#[test]
fn a_turn_without_checks_has_none() {
    assert_eq!(checked("completed", 0, &[], None), None);
    assert_eq!(checked("failed", 0, &[], Some("ACCEPTANCE_FAILED")), None, "no checks asked: not a check result");
}

// ---- undo ----

fn undo_case(marks: &[UndoMark], began: Option<u64>, summary_files: Option<FileStats>, pane: &str) -> Option<bool> {
    let ops = [done("o1", "r1", "p1", T0 + 100 * SEC, T0 + 160 * SEC)];
    let f = facts(pane, 2, T0 + 100 * SEC, T0 + 160 * SEC, summary_files);
    join(&ops, &[sum(f)], marks, began, &no_label).0[0].undone
}

#[test]
fn an_undo_after_the_turn_that_went_back_to_before_it_marks_it_undone() {
    let m = [mark("p1", T0 + 99 * SEC, T0 + 200 * SEC)];
    assert_eq!(undo_case(&m, Some(T0), Some(files(2, 1, 1)), "p1"), Some(true));
}

#[test]
fn no_undo_means_not_undone_when_the_log_covers_the_turn() {
    assert_eq!(undo_case(&[], Some(T0), Some(files(2, 1, 1)), "p1"), Some(false));
}

#[test]
fn undos_that_do_not_cover_the_turn_miss_it() {
    let f = || Some(files(2, 1, 1));
    // Another pane.
    assert_eq!(undo_case(&[mark("p2", T0, T0 + 200 * SEC)], Some(T0), f(), "p1"), Some(false));
    // The undo went back only to the point after this turn: the turn is kept.
    assert_eq!(undo_case(&[mark("p1", T0 + 160 * SEC, T0 + 200 * SEC)], Some(T0), f(), "p1"), Some(false));
    // The undo happened before the turn ended.
    assert_eq!(undo_case(&[mark("p1", T0, T0 + 120 * SEC)], Some(T0), f(), "p1"), Some(false));
}

#[test]
fn a_turn_before_an_undo_and_one_after_it_are_not_undone() {
    // Turn 1 (T0..T0+60) was kept, turn 2 (T0+100..T0+160) was undone, turn 3 came after.
    let ops = [done("o1", "r", "p1", T0, T0 + 60 * SEC), done("o2", "r", "p1", T0 + 100 * SEC, T0 + 160 * SEC), done("o3", "r", "p1", T0 + 300 * SEC, T0 + 360 * SEC)];
    let sums = [plain("p1", 1, T0, T0 + 60 * SEC), plain("p1", 2, T0 + 100 * SEC, T0 + 160 * SEC), plain("p1", 3, T0 + 300 * SEC, T0 + 360 * SEC)];
    let m = [mark("p1", T0 + 100 * SEC, T0 + 200 * SEC)];
    let (turns, _) = join(&ops, &sums, &m, Some(T0 - SEC), &no_label);
    let undone: Vec<_> = turns.iter().map(|t| t.undone).collect();
    assert_eq!(undone, vec![Some(false), Some(true), Some(false)]);
}

#[test]
fn an_undo_of_several_turns_marks_each_of_them() {
    let ops = [done("o1", "r", "p1", T0, T0 + 60 * SEC), done("o2", "r", "p1", T0 + 100 * SEC, T0 + 160 * SEC)];
    let sums = [plain("p1", 1, T0, T0 + 60 * SEC), plain("p1", 2, T0 + 100 * SEC, T0 + 160 * SEC)];
    let m = [mark("p1", T0 - SEC, T0 + 200 * SEC)];
    let (turns, _) = join(&ops, &sums, &m, Some(T0 - 10 * SEC), &no_label);
    assert_eq!(turns.iter().map(|t| t.undone).collect::<Vec<_>>(), vec![Some(true), Some(true)]);
}

#[test]
fn undo_is_unknown_without_files_a_pane_or_a_log_that_covers_the_turn() {
    let m = [mark("p1", T0 + 99 * SEC, T0 + 200 * SEC)];
    assert_eq!(undo_case(&m, Some(T0), Some(files(0, 0, 0)), "p1"), None, "nothing changed, nothing to undo");
    assert_eq!(undo_case(&m, Some(T0), None, "p1"), None, "no checkpoints: files unknown");
    assert_eq!(undo_case(&m, None, Some(files(2, 1, 1)), "p1"), None, "no log yet");
    assert_eq!(undo_case(&m, Some(T0 + 101 * SEC), Some(files(2, 1, 1)), "p1"), None, "turn started before the log began");
    assert_eq!(undo_case(&m, Some(T0 + 100 * SEC), Some(files(2, 1, 1)), "p1"), Some(true), "started exactly when the log began: covered");
}

#[test]
fn a_summary_with_no_pane_is_never_undone() {
    let m = [mark("", T0, T0 + 200 * SEC)];
    let t = &join(&[], &[plain("", 1, T0 + 100 * SEC, T0 + 160 * SEC)], &m, Some(T0), &no_label).0[0];
    assert_eq!((t.pane_id.as_deref(), t.undone), (None, None));
}

#[test]
fn the_start_edge_of_an_undo_is_two_seconds() {
    let start = T0 + 100 * SEC;
    let at_edge = [mark("p1", start + 2 * SEC, T0 + 200 * SEC)];
    let past_edge = [mark("p1", start + 2 * SEC + 1, T0 + 200 * SEC)];
    assert_eq!(undo_case(&at_edge, Some(T0), Some(files(1, 1, 0)), "p1"), Some(true));
    assert_eq!(undo_case(&past_edge, Some(T0), Some(files(1, 1, 0)), "p1"), Some(false));
}

#[test]
fn the_end_edge_of_an_undo_is_two_seconds() {
    let end = T0 + 160 * SEC;
    let at_edge = [mark("p1", T0, end - 2 * SEC)];
    let past_edge = [mark("p1", T0, end - 2 * SEC - 1)];
    assert_eq!(undo_case(&at_edge, Some(T0), Some(files(1, 1, 0)), "p1"), Some(true));
    assert_eq!(undo_case(&past_edge, Some(T0), Some(files(1, 1, 0)), "p1"), Some(false));
}

#[test]
fn a_very_short_turn_right_before_an_undo_to_its_end_reads_as_undone() {
    // The known limit: an `after` target sits at the turn's end, and 2 s of slack reaches back over it.
    let (start, end) = (T0 + 100 * SEC, T0 + 101 * SEC + 500);
    let ops = [done("o1", "r", "p1", start, end)];
    let sums = [sum(facts("p1", 1, start, end, Some(files(1, 1, 0))))];
    let m = [mark("p1", end, end + 10 * SEC)];
    assert_eq!(join(&ops, &sums, &m, Some(T0), &no_label).0[0].undone, Some(true), "documented false positive");
    // A turn of 3 s or more is safe.
    let (start, end) = (T0 + 100 * SEC, T0 + 103 * SEC);
    let ops = [done("o1", "r", "p1", start, end)];
    let sums = [sum(facts("p1", 1, start, end, Some(files(1, 1, 0))))];
    let m = [mark("p1", end, end + 10 * SEC)];
    assert_eq!(join(&ops, &sums, &m, Some(T0), &no_label).0[0].undone, Some(false));
}

#[test]
fn the_undo_uses_the_summarys_clock_when_the_record_started_earlier() {
    // The prompt was sent 5 s before the pane began to work; the checkpoint is from the pane's start.
    let ops = [done("o1", "r", "p1", T0 + 95 * SEC, T0 + 160 * SEC)];
    let sums = [sum(facts("p1", 1, T0 + 100 * SEC, T0 + 160 * SEC, Some(files(1, 1, 0))))];
    let m = [mark("p1", T0 + 100 * SEC, T0 + 200 * SEC)];
    let (turns, _) = join(&ops, &sums, &m, Some(T0), &no_label);
    assert_eq!(turns[0].undone, Some(true));
    assert_eq!(turns[0].started_ms, T0 + 95 * SEC, "the turn itself keeps the record's start");
}

#[test]
fn an_undo_after_a_merge_still_marks_the_turn_and_leaves_the_merge_alone() {
    // Merged at some time, undone later: both are facts, and neither erases the other.
    let ops = [with_copy(done("o1", "r1", "p1", T0 + 100 * SEC, T0 + 160 * SEC), Some("merged"))];
    let sums = [plain("p1", 1, T0 + 100 * SEC, T0 + 160 * SEC)];
    let m = [mark("p1", T0 + 100 * SEC, T0 + 900 * SEC)];
    let (turns, runs) = join(&ops, &sums, &m, Some(T0), &no_label);
    assert_eq!(turns[0].undone, Some(true));
    assert_eq!(runs[0].copy, CopyState::Merged);
}

#[test]
fn a_mark_alone_makes_no_turns() {
    let (turns, runs) = join(&[], &[], &[mark("p1", T0, T0 + SEC)], Some(T0), &no_label);
    assert!(turns.is_empty() && runs.is_empty());
}

// ---- runs and their copies ----

fn copy_state(closed: Option<&str>, status: &str) -> CopyState {
    let o = with_copy(op("o1", "r1", Some("p1"), status, T0, Some(T0 + SEC)), closed);
    run_join(&[o], &[]).1[0].copy
}

#[test]
fn a_copy_is_merged_discarded_waiting_or_active_by_its_latest_record() {
    assert_eq!(copy_state(Some("merged"), "completed"), CopyState::Merged);
    assert_eq!(copy_state(Some("discarded"), "completed"), CopyState::Discarded);
    assert_eq!(copy_state(None, "completed"), CopyState::Waiting);
    assert_eq!(copy_state(None, "failed"), CopyState::Waiting, "a failed run's copy still waits for review");
    assert_eq!(copy_state(None, "cancelled"), CopyState::Waiting);
    assert_eq!(copy_state(None, "running"), CopyState::Active);
    assert_eq!(copy_state(None, "waiting_input"), CopyState::Active);
    assert_eq!(copy_state(Some("merged"), "running"), CopyState::Merged, "a follow-up after the merge changes nothing");
    assert_eq!(copy_state(Some("moved-somewhere"), "completed"), CopyState::NoCopy, "an unknown mark is counted nowhere");
}

#[test]
fn a_run_without_a_copy_has_no_copy() {
    let (_, runs) = run_join(&[done("o1", "r1", "p1", T0, T0 + SEC)], &[]);
    assert_eq!(runs[0].copy, CopyState::NoCopy);
}

#[test]
fn follow_up_turns_are_one_run_and_the_merge_is_on_its_last_record_only() {
    let ops = [
        with_copy(done("o1", "run-a", "p1", T0, T0 + 10 * SEC), None),
        with_copy(done("o2", "run-a", "p1", T0 + 20 * SEC, T0 + 50 * SEC), None),
        with_copy(done("o3", "run-a", "p1", T0 + 60 * SEC, T0 + 90 * SEC), Some("merged")),
    ];
    let mut ops = ops;
    for (i, o) in ops.iter_mut().enumerate() {
        o.turn_index = i as u32;
    }
    let (turns, runs) = run_join(&ops, &[]);
    assert_eq!(turns.len(), 3);
    assert_eq!(runs.len(), 1);
    let r = &runs[0];
    assert_eq!((r.turns, r.copy), (3, CopyState::Merged));
    assert_eq!(r.busy_ms, (10 + 30 + 30) * SEC);
    assert_eq!(r.last_started_ms, T0 + 60 * SEC);
    assert_eq!((r.agent_id.as_str(), r.project.as_str()), ("alpha", proj(ORIGIN).as_str()));
}

#[test]
fn the_latest_record_is_found_by_turn_number_not_by_the_order_of_the_list() {
    let mut first = with_copy(done("o1", "run-a", "p1", T0, T0 + 10 * SEC), None);
    first.turn_index = 0;
    let mut last = with_copy(done("o2", "run-a", "p1", T0 + 20 * SEC, T0 + 30 * SEC), Some("discarded"));
    last.turn_index = 1;
    for ops in [[first.clone(), last.clone()], [last, first]] {
        assert_eq!(run_join(&ops, &[]).1[0].copy, CopyState::Discarded);
    }
}

#[test]
fn an_old_record_marked_closed_does_not_decide_when_the_latest_is_not() {
    let mut first = with_copy(done("o1", "run-a", "p1", T0, T0 + 10 * SEC), Some("merged"));
    first.turn_index = 0;
    let mut last = with_copy(done("o2", "run-a", "p1", T0 + 20 * SEC, T0 + 30 * SEC), None);
    last.turn_index = 1;
    assert_eq!(run_join(&[first, last], &[]).1[0].copy, CopyState::Waiting);
}

#[test]
fn a_withdrawn_last_record_still_carries_the_merge_but_is_no_turn() {
    let mut first = with_copy(done("o1", "run-a", "p1", T0, T0 + 10 * SEC), None);
    first.turn_index = 0;
    let mut withdrawn = with_copy(op("o2", "run-a", Some("p1"), "cancelled", T0 + 20 * SEC, Some(T0 + 21 * SEC)), Some("merged"));
    withdrawn.started_at_ms = None;
    withdrawn.turn_index = 1;
    let (turns, runs) = run_join(&[first, withdrawn], &[]);
    assert_eq!(turns.len(), 1);
    assert_eq!((runs[0].turns, runs[0].copy), (1, CopyState::Merged));
}

#[test]
fn a_run_with_no_summaries_is_still_a_run() {
    let (turns, runs) = run_join(&[done("o1", "run-a", "p1", T0, T0 + 10 * SEC)], &[]);
    assert_eq!((turns.len(), runs.len()), (1, 1));
    assert_eq!(runs[0].turns, 1);
}

#[test]
fn a_record_without_a_handle_is_a_run_of_its_own() {
    let ops = [done("o1", "", "p1", T0, T0 + SEC), done("o2", "", "p1", T0 + 100 * SEC, T0 + 101 * SEC)];
    let (_, runs) = run_join(&ops, &[]);
    assert_eq!(runs.len(), 2, "two handle-less records are not one run");
}

#[test]
fn the_same_handle_in_two_projects_is_two_runs() {
    let mut other = done("o2", "run-a", "p2", T0, T0 + SEC);
    other.project_path = "C:/Users/you/blog".into();
    let (turns, runs) = run_join(&[done("o1", "run-a", "p1", T0, T0 + SEC), other], &[]);
    assert_eq!(runs.len(), 2);
    assert_ne!(turns[0].project, turns[1].project);
}

#[test]
fn a_run_counts_finished_turns_and_their_time_only() {
    let mut running = op("o3", "run-a", Some("p1"), "running", T0 + 100 * SEC, None);
    running.turn_index = 2;
    let mut a = done("o1", "run-a", "p1", T0, T0 + 10 * SEC);
    a.turn_index = 0;
    let mut stopped = op("o2", "run-a", Some("p1"), "cancelled", T0 + 20 * SEC, Some(T0 + 25 * SEC));
    stopped.turn_index = 1;
    let (turns, runs) = run_join(&[a, stopped, running], &[]);
    assert_eq!(turns.len(), 2);
    assert_eq!(runs[0].turns, 2, "the stopped turn did work; the running one has not finished");
    assert_eq!(runs[0].busy_ms, 15 * SEC);
    assert_eq!(runs[0].copy, CopyState::NoCopy);
    assert_eq!(runs[0].last_started_ms, T0 + 100 * SEC);
}

#[test]
fn busy_time_never_goes_below_zero() {
    let mut o = done("o1", "run-a", "p1", T0 + 10 * SEC, T0);
    o.finished_at_ms = Some(T0);
    let (turns, runs) = run_join(&[o], &[]);
    assert_eq!(runs[0].busy_ms, 0);
    assert!(turns[0].ended_ms >= turns[0].started_ms);
}

#[test]
fn a_run_belongs_to_the_agent_and_account_of_its_latest_turn() {
    let mut a = done("o1", "run-a", "p1", T0, T0 + SEC);
    a.turn_index = 0;
    a.agent_type = "beta".into();
    a.worker.account = Some("acct-old".into());
    let mut b = done("o2", "run-a", "p1", T0 + 10 * SEC, T0 + 11 * SEC);
    b.turn_index = 1;
    b.worker.account = Some("acct-new".into());
    let (_, runs) = run_join(&[a, b], &[]);
    assert_eq!((runs[0].agent_id.as_str(), runs[0].account.as_deref()), ("alpha", Some("acct-new")));
}

// ---- projects ----

#[test]
fn a_summary_from_a_copy_belongs_to_the_origin_project() {
    let s = ProjectSummary { project: format!("{ORIGIN}/.puppet-master/worktrees/run-1"), summary: assemble(facts("p1", 1, T0, T0 + SEC, None)) };
    let (turns, _) = run_join(&[], &[s]);
    assert_eq!(turns[0].project, proj(ORIGIN));
}

#[test]
fn a_record_that_names_a_copy_belongs_to_the_origin_project() {
    let mut o = done("o1", "run-a", "p1", T0, T0 + SEC);
    o.project_path = format!("{ORIGIN}/.puppet-master/worktrees/run-1");
    let (turns, runs) = run_join(&[o], &[]);
    assert_eq!((turns[0].project.as_str(), runs[0].project.as_str()), (proj(ORIGIN).as_str(), proj(ORIGIN).as_str()));
}

#[test]
fn a_paired_turn_takes_its_project_from_the_record() {
    let mut s = plain("p1", 1, T0, T0 + 10 * SEC);
    s.project = "C:/Users/you/elsewhere".into();
    let (turns, _) = run_join(&[done("o1", "r1", "p1", T0, T0 + 10 * SEC)], &[s]);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].project, proj(ORIGIN));
}

// ---- order and privacy ----

#[test]
fn the_turns_come_out_oldest_first_whatever_the_input_order() {
    let ops = [done("o3", "c", "p1", T0 + 300 * SEC, T0 + 301 * SEC), done("o1", "a", "p1", T0, T0 + SEC), done("o2", "b", "p1", T0 + 100 * SEC, T0 + 101 * SEC)];
    let starts: Vec<u64> = run_join(&ops, &[]).0.iter().map(|t| t.started_ms).collect();
    assert_eq!(starts, vec![T0, T0 + 100 * SEC, T0 + 300 * SEC]);
}

#[test]
fn nothing_private_reaches_the_output() {
    let mut o = with_copy(done("o-secret-id", "handle-secret", "p1", T0, T0 + 60 * SEC), Some("merged"));
    o.checks = vec![check("SECRET_CHECK_PATH")];
    o.check_results = vec![result("SECRET_CHECK_PATH", true)];
    o.worker.name = Some("secret worker name".into());
    o.worker.workspace = Some("D:/secret-workspace-name".into());
    o.warnings = vec![crate::operations::OperationWarning { code: "W".into(), detail: "SECRET WARNING".into() }];
    let mut f = facts("p1", 1, T0, T0 + 60 * SEC, Some(files(3, 4, 5)));
    f.approvals = vec![Approval { at_ms: T0, prompt_id: "prompt-secret-id".into(), prompt: "SECRET APPROVAL allow rm -rf?".into(), kind: "command".into(), decision: "allow".into() }];
    let (turns, runs) = run_join(&[o], &[sum(f)]);
    let text = format!("{turns:?} {runs:?}");
    for private in [
        "SECRET TASK TEXT",
        "SECRET RESULT TEXT",
        "SECRET APPROVAL",
        "SECRET_FILE_NAME",
        "SECRET_CHECK_PATH",
        "SECRET DETAIL",
        "SECRET WARNING",
        "secret worker name",
        "secret-workspace-name",
        "o-secret-id",
        "handle-secret",
        "prompt-secret-id",
        "key-o-secret-id",
        "Alpha Code edited",
        "you approved",
    ] {
        assert!(!text.contains(private), "{private} reached the output: {text}");
    }
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].asked_you, Some(true), "the fact is kept, the words are not");
}
