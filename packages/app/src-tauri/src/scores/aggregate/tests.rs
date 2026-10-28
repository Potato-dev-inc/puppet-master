//! The roll-up with fixtures (SC-7). Every column of spec 3.2 has a test, and the rules that keep
//! small samples honest: 4 runs are "not enough", 10 vs 5 is the only kind of gap that gets a mark,
//! and Asked you is never marked. The tiers and the Wilson interval are tested in `stats`.

use super::*;
use crate::scores::model::{CopyState, End, Sample, ScoreRow, ScoreRun, ScoreTurn};

const T0: u64 = 1_800_000_000_000;
const MIN: u64 = 60_000;
const PROJECT_A: &str = "c:/users/you/shop";
const PROJECT_B: &str = "c:/users/you/blog";

fn agent_label(id: &str) -> String {
    match id {
        "agent-a" => "Agent A".to_string(),
        "agent-b" => "Agent B".to_string(),
        other => other.to_string(),
    }
}

/// Only `acct-1` still exists; any other id is a removed account.
fn account_label(id: &str) -> Option<String> {
    (id == "acct-1").then(|| "Work".to_string())
}

fn turn(agent: &str, account: Option<&str>, project: &str, started_ms: u64, end: End) -> ScoreTurn {
    ScoreTurn {
        project: project.to_string(),
        agent_id: agent.to_string(),
        account: account.map(str::to_string),
        pane_id: None,
        started_ms,
        ended_ms: started_ms + MIN,
        end,
        files_changed: None,
        lines: None,
        asked_you: None,
        checks_green: None,
        undone: None,
    }
}

/// `n` turns of one agent in project A, ten minutes apart from `T0`, all ending `end`.
fn turns_of(agent: &str, end: End, n: u32) -> Vec<ScoreTurn> {
    (0..n)
        .map(|i| turn(agent, None, PROJECT_A, T0 + u64::from(i) * 10 * MIN, end))
        .collect()
}

fn run(agent: &str, project: &str, last_started_ms: u64, copy: CopyState, turns: u32) -> ScoreRun {
    ScoreRun {
        project: project.to_string(),
        agent_id: agent.to_string(),
        account: None,
        turns,
        busy_ms: u64::from(turns) * MIN,
        last_started_ms,
        copy,
    }
}

fn roll(
    turns: &[ScoreTurn],
    runs: &[ScoreRun],
    project: Option<&str>,
    since_ms: u64,
) -> Vec<ScoreRow> {
    let labels = Labels {
        agent: &agent_label,
        account: &account_label,
    };
    aggregate(turns, runs, project, since_ms, &labels)
}

fn row<'a>(rows: &'a [ScoreRow], agent: &str) -> &'a ScoreRow {
    rows.iter()
        .find(|r| r.agent_id == agent)
        .expect("a row for this agent")
}

fn no_marks(rows: &[ScoreRow]) -> bool {
    rows.iter().all(|r| r.ahead.is_empty())
}

#[test]
fn no_history_gives_no_rows() {
    assert!(roll(&[], &[], None, 0).is_empty());
}

#[test]
fn four_turns_are_not_enough() {
    let rows = roll(&turns_of("agent-a", End::Ok, 4), &[], None, 0);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.turns, 4);
    assert!(!r.enough, "four runs must show the not-enough line");
    assert_eq!(r.ended_ok.sample, Sample::NotEnough);
}

#[test]
fn five_turns_are_enough_and_early() {
    let rows = roll(&turns_of("agent-a", End::Ok, 5), &[], None, 0);
    assert!(rows[0].enough);
    assert_eq!(rows[0].ended_ok.sample, Sample::Early);
    assert_eq!((rows[0].ended_ok.count, rows[0].ended_ok.total), (5, 5));
}

#[test]
fn stopped_turns_are_left_out_of_ended_ok() {
    // 6 ended OK, 2 failed, 3 stopped by the person: 6 of 8. The stopped ones still count as turns.
    let mut turns = turns_of("agent-a", End::Ok, 6);
    turns.extend((6..8).map(|i| turn("agent-a", None, PROJECT_A, T0 + i * 10 * MIN, End::Failed)));
    turns
        .extend((8..11).map(|i| turn("agent-a", None, PROJECT_A, T0 + i * 10 * MIN, End::Stopped)));
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(rows[0].turns, 11);
    assert_eq!((rows[0].ended_ok.count, rows[0].ended_ok.total), (6, 8));
    assert_eq!(rows[0].ended_ok.sample, Sample::Early);
}

#[test]
fn undone_turns_count_in_undone_only() {
    // 2 undone, 6 kept, 2 with no file information: Undone is 2 of 8. Ended OK is not touched.
    let mut turns = turns_of("agent-a", End::Ok, 10);
    for (i, t) in turns.iter_mut().enumerate() {
        t.files_changed = Some(1);
        t.undone = match i {
            0 | 1 => Some(true),
            2..=7 => Some(false),
            _ => None,
        };
    }
    let rows = roll(&turns, &[], None, 0);
    let r = &rows[0];
    assert_eq!((r.undone.count, r.undone.total), (2, 8));
    assert_eq!((r.ended_ok.count, r.ended_ok.total), (10, 10));
}

#[test]
fn checks_green_counts_only_turns_that_got_an_answer() {
    // 4 green and 1 red out of 5 answered; the other 5 had no checks.
    let mut turns = turns_of("agent-a", End::Ok, 10);
    for (i, t) in turns.iter_mut().enumerate() {
        t.checks_green = match i {
            0..=3 => Some(true),
            4 => Some(false),
            _ => None,
        };
    }
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(
        (rows[0].checks_green.count, rows[0].checks_green.total),
        (4, 5)
    );
    assert_eq!(rows[0].checks_green.sample, Sample::Early);
}

#[test]
fn merged_counts_kept_copies_out_of_kept_and_thrown_away() {
    // 2 merged, 1 thrown away; 3 waiting for review, 1 still active, 1 with no copy: none of those count.
    let turns = turns_of("agent-a", End::Ok, 1);
    let runs = [
        run("agent-a", PROJECT_A, T0, CopyState::Merged, 1),
        run("agent-a", PROJECT_A, T0 + MIN, CopyState::Merged, 1),
        run("agent-a", PROJECT_A, T0 + 2 * MIN, CopyState::Discarded, 1),
        run("agent-a", PROJECT_A, T0 + 3 * MIN, CopyState::Waiting, 1),
        run("agent-a", PROJECT_A, T0 + 4 * MIN, CopyState::Waiting, 1),
        run("agent-a", PROJECT_A, T0 + 5 * MIN, CopyState::Waiting, 1),
        run("agent-a", PROJECT_A, T0 + 6 * MIN, CopyState::Active, 1),
        run("agent-a", PROJECT_A, T0 + 7 * MIN, CopyState::NoCopy, 1),
    ];
    let rows = roll(&turns, &runs, None, 0);
    assert_eq!((rows[0].merged.count, rows[0].merged.total), (2, 3));
    assert_eq!(rows[0].waiting_review, 3);
}

#[test]
fn per_merged_needs_three_merged_runs() {
    let turns = turns_of("agent-a", End::Ok, 1);
    let two = [
        run("agent-a", PROJECT_A, T0, CopyState::Merged, 2),
        run("agent-a", PROJECT_A, T0 + MIN, CopyState::Merged, 4),
    ];
    assert!(roll(&turns, &two, None, 0)[0].per_merged.is_none());

    let three = [
        run("agent-a", PROJECT_A, T0, CopyState::Merged, 2),
        run("agent-a", PROJECT_A, T0 + MIN, CopyState::Merged, 4),
        run("agent-a", PROJECT_A, T0 + 2 * MIN, CopyState::Merged, 10),
    ];
    let pm = roll(&turns, &three, None, 0)[0]
        .per_merged
        .expect("three merged runs give a typical run");
    assert_eq!(pm.runs, 3);
    assert_eq!(pm.turns_median, 4);
    assert_eq!(pm.busy_ms_median, 4 * MIN);
}

#[test]
fn asked_you_is_never_marked() {
    // Agent A was asked on all 10 turns, agent B on none. The gap is large, but Asked you is neutral.
    // Both agents ended OK every time, so no other metric separates them either.
    let mut a = turns_of("agent-a", End::Ok, 10);
    for t in &mut a {
        t.asked_you = Some(true);
    }
    let mut b = turns_of("agent-b", End::Ok, 10);
    for t in &mut b {
        t.asked_you = Some(false);
    }
    let turns: Vec<ScoreTurn> = a.into_iter().chain(b).collect();
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(
        (
            row(&rows, "agent-a").asked_you.count,
            row(&rows, "agent-a").asked_you.total
        ),
        (10, 10)
    );
    assert!(no_marks(&rows), "Asked you is never a reason for a mark");
}

#[test]
fn a_clear_gap_in_ended_ok_is_marked_and_a_close_one_is_not() {
    // 10 of 10 against 5 of 10: the intervals are clearly apart, so agent A is ahead.
    let mut turns = turns_of("agent-a", End::Ok, 10);
    let mut b = turns_of("agent-b", End::Ok, 5);
    b.extend((5..10).map(|i| turn("agent-b", None, PROJECT_A, T0 + i * 10 * MIN, End::Failed)));
    turns.extend(b);
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(row(&rows, "agent-a").ahead, vec!["endedOk".to_string()]);
    assert!(row(&rows, "agent-b").ahead.is_empty());

    // 9 of 10 against 8 of 10 overlap: nobody is marked.
    let mut close = turns_of("agent-a", End::Ok, 9);
    close.extend((9..10).map(|i| turn("agent-a", None, PROJECT_A, T0 + i * 10 * MIN, End::Failed)));
    let mut other = turns_of("agent-b", End::Ok, 8);
    other.extend((8..10).map(|i| turn("agent-b", None, PROJECT_A, T0 + i * 10 * MIN, End::Failed)));
    close.extend(other);
    let rows = roll(&close, &[], None, 0);
    assert!(no_marks(&rows));
}

#[test]
fn undone_is_marked_when_lower_is_clearly_better() {
    // Agent A: 0 of 10 undone. Agent B: 5 of 10. Lower is better, so A is ahead and B is not.
    let mut a = turns_of("agent-a", End::Ok, 10);
    for t in &mut a {
        t.files_changed = Some(1);
        t.undone = Some(false);
    }
    let mut b = turns_of("agent-b", End::Ok, 10);
    for (i, t) in b.iter_mut().enumerate() {
        t.files_changed = Some(1);
        t.undone = Some(i < 5);
    }
    let turns: Vec<ScoreTurn> = a.into_iter().chain(b).collect();
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(row(&rows, "agent-a").ahead, vec!["undone".to_string()]);
    assert!(row(&rows, "agent-b").ahead.is_empty());
}

#[test]
fn windows_use_the_turn_start_and_the_run_last_start() {
    // Three turns started before the window, two after it. The run that last started before the
    // window is out; the one that last started after it is in.
    let since = T0 + 5 * MIN;
    let turns: Vec<ScoreTurn> = (0..5)
        .map(|i| {
            turn(
                "agent-a",
                None,
                PROJECT_A,
                if i < 3 { T0 } else { T0 + 10 * MIN + i * MIN },
                End::Ok,
            )
        })
        .collect();
    let runs = [
        run("agent-a", PROJECT_A, T0, CopyState::Merged, 1),
        run("agent-a", PROJECT_A, T0 + 10 * MIN, CopyState::Discarded, 1),
    ];
    let rows = roll(&turns, &runs, None, since);
    assert_eq!(rows[0].turns, 2);
    assert_eq!((rows[0].merged.count, rows[0].merged.total), (0, 1));
}

#[test]
fn the_project_filter_keeps_one_project() {
    let mut turns = turns_of("agent-a", End::Ok, 3);
    turns.extend((0..4).map(|i| turn("agent-a", None, PROJECT_B, T0 + i * MIN, End::Ok)));
    let runs = [run("agent-a", PROJECT_B, T0, CopyState::Merged, 1)];
    let only_a = roll(&turns, &runs, Some(PROJECT_A), 0);
    assert_eq!(only_a[0].turns, 3);
    assert_eq!(only_a[0].merged.total, 0);
    let all = roll(&turns, &runs, None, 0);
    assert_eq!(all[0].turns, 7);
    assert_eq!(all[0].merged.total, 1);
}

#[test]
fn two_accounts_of_one_agent_stay_two_rows() {
    let mut turns: Vec<ScoreTurn> = (0..2)
        .map(|i| turn("agent-a", None, PROJECT_A, T0 + i * MIN, End::Ok))
        .collect();
    turns.extend((0..3).map(|i| turn("agent-a", Some("acct-1"), PROJECT_A, T0 + i * MIN, End::Ok)));
    turns.push(turn("agent-a", Some("acct-gone"), PROJECT_A, T0, End::Ok));
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(rows.len(), 3);
    // Most turns first: the account with 3 turns, then the default login, then the removed account.
    assert_eq!(rows[0].account_id.as_deref(), Some("acct-1"));
    assert_eq!(rows[0].account_label, "Work");
    assert_eq!(rows[1].account_id, None);
    assert_eq!(rows[1].account_label, DEFAULT_ACCOUNT);
    assert_eq!(rows[2].account_label, REMOVED_ACCOUNT);
}

#[test]
fn rows_are_sorted_by_turns_never_by_score() {
    // Agent B has fewer turns but a perfect score; it comes second.
    let mut turns = turns_of("agent-a", End::Failed, 3);
    turns.extend(turns_of("agent-b", End::Ok, 2));
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(rows[0].agent_id, "agent-a");
    assert_eq!(rows[1].agent_id, "agent-b");
}

#[test]
fn typical_turn_time_and_lines_are_medians() {
    // Turn times: 1, 2 and 3 minutes; lines 10, 30 and 20. Medians: 2 minutes and 20 lines.
    let mut turns = turns_of("agent-a", End::Ok, 3);
    for (i, t) in turns.iter_mut().enumerate() {
        t.ended_ms = t.started_ms + (i as u64 + 1) * MIN;
        t.lines = Some([10, 30, 20][i]);
    }
    let rows = roll(&turns, &[], None, 0);
    assert_eq!(rows[0].turn_ms_median, Some(2 * MIN));
    assert_eq!(rows[0].lines_median, Some(20));
}

#[test]
fn labels_come_from_the_callbacks() {
    let rows = roll(&turns_of("agent-a", End::Ok, 1), &[], None, 0);
    assert_eq!(rows[0].agent_label, "Agent A");
    assert_eq!(rows[0].account_label, DEFAULT_ACCOUNT);
}
