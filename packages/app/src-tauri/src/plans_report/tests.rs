//! Aggregation with fixtures: two agents, two accounts, a month boundary in local time, a limit log.
//! Times are written as local clock times in a +08:00 zone so a boundary mistake shows up.

use super::aggregate::{aggregate, Labels, DEFAULT_ACCOUNT, REMOVED_ACCOUNT};
use super::limit_log::{self, Entry};
use super::merge::{merge, OpTurn};
use super::model::{FilesFact, LimitFact, TurnFact};
use super::month::YearMonth;
use super::{build, Inputs};
use crate::turn_summary::model::{assemble, Facts, FileStats};
use chrono::{FixedOffset, TimeZone};

const HOUR: u64 = 3_600_000;
const MIN: u64 = 60_000;

fn tz() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

/// Milliseconds for a local clock time in the test zone.
fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> u64 {
    tz().with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap().timestamp_millis() as u64
}

fn agent_label(id: &str) -> String {
    match id {
        "alpha" => "Alpha Code".to_string(),
        "beta" => "Beta".to_string(),
        other => other.to_string(),
    }
}

fn account_label(id: &str) -> Option<String> {
    match id {
        "acct-work" => Some("Work".to_string()),
        "acct-home" => Some("Home".to_string()),
        _ => None,
    }
}

fn turn(agent: &str, account: Option<&str>, started_ms: u64, busy_min: u64, files: Option<(u32, u32, u32)>) -> TurnFact {
    TurnFact {
        agent_id: agent.into(),
        account: account.map(String::from),
        started_ms,
        busy_ms: busy_min * MIN,
        files: files.map(|(count, added, deleted)| FilesFact { count, added, deleted }),
        merged: false,
    }
}

fn limit(agent: &str, account: Option<&str>, start_ms: u64, end_ms: Option<u64>) -> LimitFact {
    LimitFact { agent_id: agent.into(), account: account.map(String::from), start_ms, end_ms }
}

fn report(inputs: &Inputs, month: Option<&str>, now_ms: u64) -> super::model::PlansReport {
    let agent = |id: &str| agent_label(id);
    let account = |id: &str| account_label(id);
    build(inputs, month.and_then(YearMonth::parse), now_ms, &tz(), &Labels { agent: &agent, account: &account })
}

fn october() -> (u64, u64) {
    (YearMonth { year: 2026, month: 10 }.start_ms(&tz()).unwrap(), YearMonth { year: 2026, month: 10 }.end_ms(&tz()).unwrap())
}

#[test]
fn each_agent_and_account_is_its_own_row_with_its_own_numbers() {
    let turns = vec![
        turn("alpha", Some("acct-work"), at(2026, 10, 3, 9, 0), 10, Some((4, 80, 10))),
        turn("alpha", Some("acct-work"), at(2026, 10, 4, 9, 0), 5, Some((1, 5, 0))),
        turn("alpha", Some("acct-home"), at(2026, 10, 5, 9, 0), 20, None),
        turn("beta", None, at(2026, 10, 6, 9, 0), 3, Some((0, 0, 0))),
        turn("beta", Some("acct-gone"), at(2026, 10, 7, 9, 0), 1, None),
    ];
    let (from, to) = october();
    let agent = |id: &str| agent_label(id);
    let account = |id: &str| account_label(id);
    let (rows, totals) = aggregate(&turns, &[], from, to, &Labels { agent: &agent, account: &account });

    assert_eq!(rows.len(), 4);
    let work = &rows[0];
    assert_eq!((work.agent_label.as_str(), work.account_label.as_str()), ("Alpha Code", "Work"));
    assert_eq!((work.turns, work.busy_ms), (2, 15 * MIN));
    assert_eq!((work.files.changed, work.files.added, work.files.deleted, work.files.known_turns), (5, 85, 10, 2));

    let home = rows.iter().find(|r| r.account_label == "Home").unwrap();
    assert_eq!((home.turns, home.files.known_turns), (1, 0), "a turn with no summary adds no files and says so");

    let default = rows.iter().find(|r| r.agent_id == "beta" && r.account_id.is_none()).unwrap();
    assert_eq!(default.account_label, DEFAULT_ACCOUNT);
    let removed = rows.iter().find(|r| r.account_id.as_deref() == Some("acct-gone")).unwrap();
    assert_eq!(removed.account_label, REMOVED_ACCOUNT);

    assert_eq!((totals.turns, totals.busy_ms), (5, 39 * MIN));
    assert_eq!((totals.files_changed, totals.added, totals.deleted), (5, 85, 10));
}

#[test]
fn the_month_boundary_is_local_midnight_not_utc() {
    // 2026-10-31 23:30 local is still October; 2026-11-01 00:30 local is November.
    // In UTC the second one is 2026-10-31 16:30, which a UTC split would call October.
    let late_october = at(2026, 10, 31, 23, 30);
    let early_november = at(2026, 11, 1, 0, 30);
    assert!(early_november < YearMonth { year: 2026, month: 11 }.start_ms(&FixedOffset::east_opt(0).unwrap()).unwrap());
    let inputs = Inputs {
        turns: vec![turn("alpha", None, late_october, 1, None), turn("alpha", None, early_november, 1, None)],
        ..Inputs::default()
    };
    let now = at(2026, 11, 15, 12, 0);
    assert_eq!(report(&inputs, Some("2026-10"), now).totals.turns, 1);
    assert_eq!(report(&inputs, Some("2026-11"), now).totals.turns, 1);
    // The exact edges: the first moment of November is November's, the moment before it October's.
    let edge = YearMonth { year: 2026, month: 11 }.start_ms(&tz()).unwrap();
    let edge_inputs = Inputs { turns: vec![turn("alpha", None, edge - 1, 1, None), turn("alpha", None, edge, 1, None)], ..Inputs::default() };
    assert_eq!(report(&edge_inputs, Some("2026-10"), now).totals.turns, 1);
    assert_eq!(report(&edge_inputs, Some("2026-11"), now).totals.turns, 1);
}

#[test]
fn december_rolls_into_january_and_months_have_their_own_length() {
    let dec = YearMonth { year: 2026, month: 12 };
    assert_eq!(dec.next(), YearMonth { year: 2027, month: 1 });
    assert_eq!(YearMonth { year: 2027, month: 1 }.previous(), dec);
    let feb = YearMonth { year: 2027, month: 2 };
    assert_eq!(feb.end_ms(&tz()).unwrap() - feb.start_ms(&tz()).unwrap(), 28 * 24 * HOUR);
    assert_eq!(YearMonth::parse("2026-10"), Some(YearMonth { year: 2026, month: 10 }));
    assert_eq!(YearMonth::parse("2026-13"), None);
    assert_eq!(YearMonth::parse("26-1"), None);
    assert_eq!(YearMonth::parse("October"), None);
    assert_eq!(YearMonth { year: 2026, month: 10 }.label(), "October 2026");
}

#[test]
fn a_limit_counts_in_the_month_it_began_and_its_waiting_time_is_split_at_the_boundary() {
    // Hit at 22:00 on 31 October, over at 03:00 on 1 November: 2 h of it in October, 3 h in November.
    let hits = vec![limit("alpha", Some("acct-work"), at(2026, 10, 31, 22, 0), Some(at(2026, 11, 1, 3, 0)))];
    let inputs = Inputs { limits: hits, limits_since_ms: Some(at(2026, 10, 1, 0, 0)), ..Inputs::default() };
    let now = at(2026, 11, 20, 12, 0);

    let oct = report(&inputs, Some("2026-10"), now);
    assert_eq!((oct.totals.limit_hits, oct.totals.limited_ms), (1, 2 * HOUR));
    let nov = report(&inputs, Some("2026-11"), now);
    assert_eq!((nov.totals.limit_hits, nov.totals.limited_ms), (0, 3 * HOUR), "no new hit, but the wait carries over");
    assert_eq!(nov.rows[0].limits.hits, 0);
    assert_eq!(nov.rows[0].turns, 0, "a row can exist for a limit alone");
}

#[test]
fn the_limit_log_pairs_starts_and_ends_and_caps_the_wait_at_the_reset_time() {
    let dir = std::env::temp_dir().join(format!("pm-plans-{}", uuid::Uuid::new_v4()));
    let path = dir.join(limit_log::FILE_NAME);
    let t0 = at(2026, 10, 10, 9, 0);
    let write = |entry: Entry| limit_log::append(&path, &entry).unwrap();
    write(Entry::Begin { at_ms: t0 - HOUR });
    // p1: ended normally after 2 h.
    write(Entry::Start { at_ms: t0, pane: "p1".into(), agent: "alpha".into(), account: Some("acct-work".into()), reset_at_ms: Some(t0 + 5 * HOUR) });
    write(Entry::End { at_ms: t0 + 2 * HOUR, pane: "p1".into() });
    // p2: the screen still showed the limit 9 h later, but the plan renewed after 5 h.
    write(Entry::Start { at_ms: t0, pane: "p2".into(), agent: "beta".into(), account: None, reset_at_ms: Some(t0 + 5 * HOUR) });
    write(Entry::End { at_ms: t0 + 9 * HOUR, pane: "p2".into() });
    // p3: no end, its pane is gone, no reset time: unknown.
    write(Entry::Start { at_ms: t0, pane: "p3".into(), agent: "beta".into(), account: None, reset_at_ms: None });
    // p4: no end, its pane is gone, but the reset time passed: ends there.
    write(Entry::Start { at_ms: t0, pane: "p4".into(), agent: "alpha".into(), account: None, reset_at_ms: Some(t0 + 3 * HOUR) });
    // p5: no end and still on screen now: lasts until now.
    write(Entry::Start { at_ms: t0, pane: "p5".into(), agent: "alpha".into(), account: None, reset_at_ms: None });
    // A damaged line and a line from a newer version are skipped.
    std::fs::write(&path, format!("{}not json\n{{\"event\":\"from_the_future\"}}\n", std::fs::read_to_string(&path).unwrap())).unwrap();

    let entries = limit_log::read(&path);
    assert_eq!(entries.len(), 8, "begin + 5 starts + 2 ends; the two bad lines are skipped");
    assert_eq!(limit_log::counting_since(&entries), Some(t0 - HOUR));

    let now = t0 + 12 * HOUR;
    let found = limit_log::hits(&entries, now, &|pane| pane == "p5");
    let wait = |pane_agent: &str, account: Option<&str>, start_n: usize| {
        found.iter().filter(|h| h.agent_id == pane_agent && h.account.as_deref() == account).nth(start_n).unwrap().end_ms
    };
    assert_eq!(found.len(), 5);
    assert_eq!(wait("alpha", Some("acct-work"), 0), Some(t0 + 2 * HOUR));
    assert_eq!(wait("beta", None, 0).map(|e| e - t0), Some(5 * HOUR), "capped at the reset time, not the 9 h on screen");
    assert_eq!(wait("beta", None, 1), None, "p3: unknown");
    assert_eq!(wait("alpha", None, 0), Some(t0 + 3 * HOUR), "p4: the passed reset time");
    assert_eq!(wait("alpha", None, 1), Some(now), "p5: still limited");
}

#[test]
fn a_newer_limit_on_the_same_pane_closes_the_older_one() {
    let t0 = at(2026, 10, 10, 9, 0);
    let entries = vec![
        Entry::Start { at_ms: t0, pane: "p".into(), agent: "alpha".into(), account: None, reset_at_ms: None },
        Entry::Start { at_ms: t0 + 6 * HOUR, pane: "p".into(), agent: "alpha".into(), account: None, reset_at_ms: None },
        Entry::End { at_ms: t0 + 8 * HOUR, pane: "p".into() },
    ];
    let found = limit_log::hits(&entries, t0 + 20 * HOUR, &|_| false);
    assert_eq!(found.iter().map(|h| h.end_ms).collect::<Vec<_>>(), [Some(t0 + 6 * HOUR), Some(t0 + 8 * HOUR)]);
}

#[test]
fn the_report_lists_months_says_since_when_limits_count_and_flags_what_it_cannot_know() {
    let since = at(2026, 10, 12, 8, 0);
    let inputs = Inputs {
        turns: vec![
            turn("alpha", None, at(2026, 9, 2, 9, 0), 1, None),
            turn("alpha", None, at(2026, 10, 2, 9, 0), 1, None),
        ],
        limits: vec![limit("alpha", None, at(2026, 10, 20, 9, 0), None)],
        limits_since_ms: Some(since),
        ..Inputs::default()
    };
    let now = at(2026, 10, 28, 12, 0);

    let this = report(&inputs, None, now);
    assert_eq!(this.month.id, "2026-10");
    assert_eq!(this.months.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["2026-10", "2026-09"]);
    assert_eq!(this.limits_counted_since.as_deref(), Some("2026-10-12"));
    assert_eq!(this.notes[0], "Counting limits since 12 October 2026. Earlier limits were not recorded.");
    assert!(this.notes.iter().any(|n| n == "1 limit has no end time, so no waiting time is added for it."), "{:?}", this.notes);
    assert!(this.notes.iter().any(|n| n == "1 turn had no file information, so they add no files."), "{:?}", this.notes);
    assert!(this.notes.iter().any(|n| n.starts_with("Tokens are not counted")));

    let september = report(&inputs, Some("2026-09"), now);
    assert_eq!(september.notes[0], "Counting limits since 12 October 2026. Earlier limits were not recorded.");
    assert_eq!(september.totals.turns, 1);

    // A month that began after the log did has nothing missing to apologise for.
    let november = report(&inputs, Some("2026-11"), now);
    assert_eq!(november.notes[0], "Counting limits since 12 October 2026.");
}

#[test]
fn with_no_log_yet_the_report_says_limits_count_from_now_on() {
    let r = report(&Inputs::default(), None, at(2026, 10, 28, 12, 0));
    assert_eq!(r.notes[0], "Counting limits from now on. Earlier limits were not recorded.");
    assert!(r.rows.is_empty());
    assert_eq!(r.totals.turns, 0);
    assert_eq!(r.months.len(), 1, "this month is always there to switch to");
    assert_eq!(r.limits_counted_since, None);
}

#[test]
fn a_month_with_no_data_is_selectable_and_empty() {
    let inputs = Inputs { turns: vec![turn("alpha", None, at(2026, 10, 2, 9, 0), 1, None)], ..Inputs::default() };
    let r = report(&inputs, Some("2026-07"), at(2026, 10, 28, 12, 0));
    assert_eq!(r.month.id, "2026-07");
    assert!(r.rows.is_empty());
    assert!(r.months.iter().any(|m| m.id == "2026-07"));
}

// ---- joining operation records and turn summaries ----

fn summary(pane: &str, turn_no: u32, started: u64, ended: u64, agent_id: Option<&str>, account: Option<&str>, files: Option<FileStats>) -> crate::turn_summary::TurnSummary {
    assemble(Facts {
        pane_id: pane.into(),
        agent: "Alpha Code".into(),
        agent_id: agent_id.map(String::from),
        account: account.map(String::from),
        turn: turn_no,
        started_at_ms: started,
        ended_at_ms: ended,
        files,
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: None,
    })
}

fn op(pane: Option<&str>, agent: &str, account: Option<&str>, start: u64, end: Option<u64>, merged: bool) -> OpTurn {
    OpTurn { pane_id: pane.map(String::from), agent_id: agent.into(), account: account.map(String::from), start_ms: start, end_ms: end, merged }
}

fn files(count: u32, added: u32, deleted: u32) -> FileStats {
    FileStats { count, added, deleted, paths: vec!["C:\\Users\\you\\proj\\a.ts".into()] }
}

#[test]
fn a_runs_turn_in_both_records_is_one_turn_with_the_records_agent_and_the_summarys_files() {
    let t = at(2026, 10, 3, 9, 0);
    let ops = [op(Some("p1"), "alpha", Some("acct-work"), t, Some(t + 4 * MIN), true)];
    let sums = [summary("p1", 1, t + 2_000, t + 4 * MIN + 3_000, None, None, Some(files(3, 30, 4)))];
    let turns = merge(&ops, &sums, &|_| None);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].agent_id, "alpha");
    assert_eq!(turns[0].account.as_deref(), Some("acct-work"));
    assert_eq!(turns[0].busy_ms, 4 * MIN, "the run's own times");
    assert_eq!(turns[0].files, Some(FilesFact { count: 3, added: 30, deleted: 4 }));
    assert!(turns[0].merged);
}

#[test]
fn a_turn_typed_by_hand_and_a_run_with_no_summary_each_count_once() {
    let t = at(2026, 10, 3, 9, 0);
    let ops = [op(Some("p1"), "alpha", None, t, Some(t + MIN), false)];
    let sums = [
        // Same pane, hours later: not the run's turn.
        summary("p1", 2, t + 5 * HOUR, t + 5 * HOUR + 2 * MIN, Some("alpha"), Some("acct-home"), Some(files(1, 2, 3))),
        // Another pane entirely, an old summary without ids: the label finds the agent.
        summary("p9", 1, t + 6 * HOUR, t + 6 * HOUR + MIN, None, None, None),
    ];
    let turns = merge(&ops, &sums, &|label| (label == "Alpha Code").then(|| "alpha".to_string()));
    assert_eq!(turns.len(), 3);
    let by_start = |ms: u64| turns.iter().find(|t| t.started_ms == ms).unwrap();
    assert_eq!(by_start(t).files, None);
    assert_eq!(by_start(t + 5 * HOUR).account.as_deref(), Some("acct-home"));
    assert_eq!(by_start(t + 5 * HOUR).busy_ms, 2 * MIN);
    assert_eq!(by_start(t + 6 * HOUR).agent_id, "alpha");
}

#[test]
fn the_same_summary_from_memory_and_from_a_project_file_counts_once() {
    let t = at(2026, 10, 3, 9, 0);
    let one = summary("p1", 1, t, t + MIN, Some("alpha"), None, None);
    let turns = merge(&[], &[one.clone(), one], &|_| None);
    assert_eq!(turns.len(), 1);
}

#[test]
fn a_summary_without_a_known_agent_keeps_its_display_name_as_the_agent() {
    let t = at(2026, 10, 3, 9, 0);
    let turns = merge(&[], &[summary("p1", 1, t, t + MIN, None, None, None)], &|_| None);
    assert_eq!(turns[0].agent_id, "Alpha Code");
}

#[test]
fn the_report_has_no_folder_pane_or_chat_text() {
    let t = at(2026, 10, 3, 9, 0);
    let ops = [op(Some("pane-secret-1"), "alpha", Some("acct-work"), t, Some(t + MIN), false)];
    let sums = [summary("pane-secret-1", 1, t, t + MIN, Some("alpha"), Some("acct-work"), Some(files(1, 1, 1)))];
    let inputs = Inputs {
        turns: merge(&ops, &sums, &|_| None),
        limits: vec![limit("alpha", Some("acct-work"), t, Some(t + HOUR))],
        limits_since_ms: Some(t),
        ..Inputs::default()
    };
    let json = serde_json::to_string(&report(&inputs, None, t + HOUR)).unwrap();
    for secret in ["pane-secret", "C:\\\\Users", "a.ts", "pane", "path", "cwd", "prompt", "sentence"] {
        assert!(!json.contains(secret), "{secret} leaked into {json}");
    }
    assert!(json.contains("\"agentLabel\":\"Alpha Code\"") && json.contains("\"accountLabel\":\"Work\""));
}

#[test]
fn unreadable_records_get_a_note_and_none_means_no_note() {
    let t = at(2026, 10, 3, 9, 0);
    let note = |n: usize| {
        let inputs = Inputs { unreadable_records: n, ..Inputs::default() };
        report(&inputs, None, t).notes.into_iter().find(|l| l.contains("could not be read"))
    };
    assert_eq!(note(0), None);
    assert_eq!(note(1).as_deref(), Some("1 record could not be read. Its turns are not counted."));
    assert_eq!(note(3).as_deref(), Some("3 records could not be read. Their turns are not counted."));
}

#[test]
fn the_limit_hits_come_out_in_the_same_order_every_time() {
    // Two panes start at the same moment and are both still open: the order must not depend on hashing.
    let t0 = at(2026, 10, 10, 9, 0);
    let entries = vec![
        Entry::Start { at_ms: t0, pane: "pane-b".into(), agent: "alpha".into(), account: None, reset_at_ms: None },
        Entry::Start { at_ms: t0, pane: "pane-a".into(), agent: "alpha".into(), account: None, reset_at_ms: None },
        Entry::Start { at_ms: t0 - HOUR, pane: "pane-c".into(), agent: "beta".into(), account: None, reset_at_ms: None },
    ];
    let first = limit_log::hits(&entries, t0 + HOUR, &|_| true);
    for _ in 0..20 {
        assert_eq!(limit_log::hits(&entries, t0 + HOUR, &|_| true), first);
    }
    assert_eq!(first.iter().map(|h| h.agent_id.as_str()).collect::<Vec<_>>(), ["beta", "alpha", "alpha"]);
}
