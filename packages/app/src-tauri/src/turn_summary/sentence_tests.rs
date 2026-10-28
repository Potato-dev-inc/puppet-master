//! Sentence and assembly tests for `turn_summary::model`.

use super::model::*;
use crate::checkpoints::FileChange;

fn facts(agent: &str) -> Facts {
    Facts {
        pane_id: "p1".into(),
        agent: agent.into(),
        agent_id: None,
        account: None,
        turn: 1,
        started_at_ms: 0,
        ended_at_ms: 90_000,
        files: None,
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: None,
    }
}

fn files(count: u32, added: u32, deleted: u32) -> Option<FileStats> {
    Some(FileStats { count, added, deleted, paths: vec![] })
}

fn approval(kind: &str, decision: &str) -> Approval {
    Approval {
        at_ms: 1,
        prompt_id: format!("{kind}-{decision}"),
        prompt: "x".into(),
        kind: kind.into(),
        decision: decision.into(),
    }
}

fn receipt(checks: &str, outcome: &str) -> Option<ReceiptFacts> {
    Some(ReceiptFacts { checks: checks.into(), outcome: outcome.into() })
}

#[test]
fn edits_approvals_and_waiting_prompt() {
    let mut f = facts("Claude Code");
    f.files = files(4, 86, 12);
    f.approvals = vec![approval("command", "allow")];
    f.end_status = "waiting_input".into();
    f.end_prompt = Some("allow `npm test`?".into());
    assert_eq!(
        assemble(f).sentence,
        "Claude Code edited 4 files (+86 \u{2212}12), you approved 1 command, now waiting: allow `npm test`?"
    );
}

#[test]
fn no_file_changes_idle() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    assert_eq!(assemble(f).sentence, "Claude Code made no file changes, now idle.");
}

#[test]
fn unknown_files_uses_duration() {
    let mut f = facts("Claude Code");
    f.files = None;
    f.ended_at_ms = 200_000;
    assert_eq!(assemble(f).sentence, "Claude Code worked for 3m, now idle.");
}

#[test]
fn approvals_are_counted_as_commands() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.approvals = vec![approval("command", "allow"), approval("command", "allow")];
    assert_eq!(
        assemble(f).sentence,
        "Claude Code made no file changes, you approved 2 commands, now idle."
    );
}

#[test]
fn codex_error_singular_file() {
    let mut f = facts("Codex");
    f.files = files(1, 3, 0);
    f.end_status = "error".into();
    assert_eq!(
        assemble(f).sentence,
        "Codex edited 1 file (+3 \u{2212}0), ended with an error."
    );
}

#[test]
fn waiting_without_prompt() {
    let mut f = facts("Claude Code");
    f.files = files(2, 5, 5);
    f.end_status = "waiting_input".into();
    f.end_prompt = None;
    assert_eq!(
        assemble(f).sentence,
        "Claude Code edited 2 files (+5 \u{2212}5), now waiting for you."
    );
}

#[test]
fn approved_and_declined_requests() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.approvals = vec![approval("edit", "allow"), approval("command", "deny")];
    assert_eq!(
        assemble(f).sentence,
        "Claude Code made no file changes, you approved 1 request, you declined 1 command, now idle."
    );
}

#[test]
fn receipt_all_checks_passed() {
    let mut f = facts("Claude Code");
    f.files = files(2, 10, 1);
    f.receipt = receipt("3/3 passed", "passed_checks");
    assert_eq!(
        assemble(f).sentence,
        "Claude Code edited 2 files (+10 \u{2212}1), all checks passed (3/3), now idle."
    );
}

#[test]
fn receipt_checks_failed() {
    let mut f = facts("Claude Code");
    f.files = files(2, 10, 1);
    f.receipt = receipt("1/3 passed", "failed_checks");
    assert_eq!(
        assemble(f).sentence,
        "Claude Code edited 2 files (+10 \u{2212}1), checks failed (1/3 passed), now idle."
    );
}

#[test]
fn receipt_no_checks_configured() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.receipt = receipt("none configured", "completed_unchecked");
    assert_eq!(
        assemble(f).sentence,
        "Claude Code made no file changes, no checks ran, now idle."
    );
}

#[test]
fn receipt_failed_run_with_error() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.receipt = receipt("none configured", "failed");
    f.end_status = "error".into();
    assert_eq!(
        assemble(f).sentence,
        "Claude Code made no file changes, the run failed, ended with an error."
    );
}

#[test]
fn receipt_cancelled_run() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.receipt = receipt("none configured", "cancelled");
    assert_eq!(
        assemble(f).sentence,
        "Claude Code made no file changes, the run was cancelled, now idle."
    );
}

#[test]
fn answers_to_questions_are_not_approvals() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.approvals = vec![approval("question", "answer")];
    assert_eq!(assemble(f).sentence, "Claude Code made no file changes, now idle.");
}

#[test]
fn long_waiting_prompt_is_shortened() {
    let mut f = facts("Claude Code");
    f.files = files(0, 0, 0);
    f.end_status = "waiting_input".into();
    f.end_prompt = Some("x".repeat(200));
    assert_eq!(
        assemble(f).sentence,
        format!("Claude Code made no file changes, now waiting: {}\u{2026}.", "x".repeat(79))
    );
}

#[test]
fn durations() {
    assert_eq!(short_duration(59_000), "59s");
    assert_eq!(short_duration(60_000), "1m");
    assert_eq!(short_duration(3_599_000), "59m");
    assert_eq!(short_duration(3_600_000), "1h");
    assert_eq!(short_duration(4_320_000), "1h 12m");
}

#[test]
fn shorten_collapses_whitespace() {
    assert_eq!(shorten("  run \n  npm   test "), "run npm test");
}

#[test]
fn file_stats_total_and_cap() {
    let changes: Vec<FileChange> = (0..10)
        .map(|i| FileChange {
            path: format!("f{i}"),
            status: "M".into(),
            added: 2,
            deleted: 1,
            binary: false,
        })
        .collect();
    let stats = FileStats::from_changes(&changes);
    assert_eq!(stats.count, 10);
    assert_eq!(stats.added, 20);
    assert_eq!(stats.deleted, 10);
    assert_eq!(stats.paths.len(), 8);
    assert_eq!(stats.paths[0], "f0");
}

#[test]
fn summary_fields_are_assembled() {
    let mut f = facts("Claude Code");
    f.started_at_ms = 1_000;
    f.ended_at_ms = 61_000;
    f.turn = 3;
    f.end_status = "weird".into();
    let summary = assemble(f);
    assert_eq!(summary.duration_ms, 60_000);
    assert_eq!(summary.turn, 3);
    assert_eq!(summary.end.kind, "idle");
    assert_eq!(summary.end.prompt, None);
}
