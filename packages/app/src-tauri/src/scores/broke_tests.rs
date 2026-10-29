//! What broke it's count on the Scores rows (`broke_tests`). Covered: one count per row (agent and
//! account), zero when there is none, an unreadable findings file reads as zero with no error, only
//! culprit turns with an agent count, project and window are respected, and the count never changes
//! `ahead`, a rate or `enough`.

use super::aggregate::{aggregate, count_broke, Labels};
use super::gather::{broke_facts, broke_facts_in};
use super::model::{BrokeFact, End, ScoreRow, ScoreTurn};
use super::{build_in, Inputs};
use crate::checkpoints::bisect::model::{Finding, Outcome, Suspect, UnsureWhy};
use crate::checkpoints::bisect::store::Store;
use chrono::Utc;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

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

/// Only `acct-1` still exists.
fn account_label(id: &str) -> Option<String> {
    (id == "acct-1").then(|| "Work".to_string())
}

fn labels() -> Labels<'static> {
    Labels { agent: &agent_label, account: &account_label }
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

/// `n` turns of one agent and account, ten minutes apart from `T0`.
fn turns_of(agent: &str, account: Option<&str>, project: &str, end: End, n: u32) -> Vec<ScoreTurn> {
    (0..n).map(|i| turn(agent, account, project, T0 + u64::from(i) * 10 * MIN, end)).collect()
}

fn suspect(agent: Option<&str>, account: Option<&str>) -> Suspect {
    Suspect {
        pane_key: "p1".into(),
        turn: 3,
        agent_label: "An agent".into(),
        agent_id: agent.map(str::to_string),
        account: account.map(str::to_string),
        at_ms: T0,
        sentence: None,
        files: None,
    }
}

fn finding(id: &str, project: &str, outcome: Outcome, finished_ms: u64) -> Finding {
    Finding {
        id: id.into(),
        project: project.into(),
        folder: "C:/users/you/shop".into(),
        pane_key: "p1".into(),
        started_ms: finished_ms - 1_000,
        finished_ms,
        check_name: "npm test".into(),
        check_argv: vec!["npm".into(), "test".into()],
        runs: 3,
        minutes_x10: 5,
        points: 4,
        outcome,
        tail: String::new(),
        probes: Vec::new(),
        undone_ms: None,
        good_idx: None,
        bad_idx: None,
    }
}

/// A finding that blamed one turn.
fn culprit(id: &str, project: &str, agent: Option<&str>, account: Option<&str>, finished_ms: u64) -> Finding {
    let outcome = Outcome::Culprit {
        suspect: suspect(agent, account),
        failing: vec!["cart rounds".into()],
        counts: None,
        last_good_ms: T0,
    };
    finding(id, project, outcome, finished_ms)
}

fn fact(agent: &str, account: Option<&str>, project: &str, at_ms: u64) -> BrokeFact {
    BrokeFact { project: project.into(), agent_id: agent.into(), account: account.map(str::to_string), at_ms }
}

fn row<'a>(rows: &'a [ScoreRow], agent: &str, account: Option<&str>) -> &'a ScoreRow {
    rows.iter()
        .find(|r| r.agent_id == agent && r.account_id.as_deref() == account)
        .expect("a row for this agent and account")
}

/// A temp folder for one test, removed at the end (the same pattern as the store tests).
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        Scratch(std::env::temp_dir().join(format!("pm-wb-scores-{}", uuid::Uuid::new_v4().simple())))
    }

    fn store(&self) -> Store {
        Store::at(self.0.clone())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn no_findings_means_zero_for_every_row() {
    let turns: Vec<ScoreTurn> = turns_of("agent-a", None, PROJECT_A, End::Ok, 3);
    let mut rows = aggregate(&turns, &[], Some(PROJECT_A), 0, &labels());
    count_broke(&mut rows, &[], Some(PROJECT_A), 0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].broke_tests, 0);
    let v = serde_json::to_value(&rows[0]).unwrap();
    assert_eq!(v["brokeTests"], json!(0));
}

#[test]
fn each_row_counts_its_own_agent_and_account() {
    let mut turns = turns_of("agent-a", None, PROJECT_A, End::Ok, 5);
    turns.extend(turns_of("agent-a", Some("acct-1"), PROJECT_A, End::Ok, 5));
    turns.extend(turns_of("agent-b", None, PROJECT_A, End::Ok, 5));
    let facts = vec![
        fact("agent-a", None, PROJECT_A, T0 + MIN),
        fact("agent-a", None, PROJECT_A, T0 + 2 * MIN),
        fact("agent-a", Some("acct-1"), PROJECT_A, T0 + MIN),
        fact("agent-b", None, PROJECT_A, T0 + MIN),
        // An agent with no row in this report is not counted anywhere.
        fact("agent-z", None, PROJECT_A, T0 + MIN),
    ];
    let mut rows = aggregate(&turns, &[], Some(PROJECT_A), 0, &labels());
    count_broke(&mut rows, &facts, Some(PROJECT_A), 0);
    assert_eq!(row(&rows, "agent-a", None).broke_tests, 2);
    assert_eq!(row(&rows, "agent-a", Some("acct-1")).broke_tests, 1);
    assert_eq!(row(&rows, "agent-b", None).broke_tests, 1);
    assert_eq!(rows.len(), 3, "no row is made for the agent without turns");
}

#[test]
fn only_culprit_turns_with_an_agent_become_facts() {
    let findings = vec![
        culprit("wb-000000000001", PROJECT_A, Some("agent-a"), None, T0),
        culprit("wb-000000000002", PROJECT_A, None, None, T0),
        finding("wb-000000000003", PROJECT_A, Outcome::TestsPass, T0),
        finding(
            "wb-000000000004",
            PROJECT_A,
            Outcome::Several { suspects: vec![suspect(Some("agent-a"), None)], last_good_ms: T0, first_bad_ms: T0 },
            T0,
        ),
        finding("wb-000000000005", PROJECT_A, Outcome::BetweenTurns { last_good_ms: T0, first_bad_ms: T0 }, T0),
        finding("wb-000000000006", PROJECT_A, Outcome::NotSure { why: UnsureWhy::Flaky, narrowed: None }, T0),
    ];
    let facts = broke_facts(&findings);
    assert_eq!(facts.len(), 1, "only the culprit with an agent id");
    assert_eq!(facts[0].agent_id, "agent-a");
    assert_eq!(facts[0].project, PROJECT_A);
    assert_eq!(facts[0].at_ms, T0);
}

#[test]
fn an_unreadable_findings_file_counts_as_zero_without_an_error() {
    let scratch = Scratch::new();
    let store = scratch.store();
    fs::create_dir_all(store.root()).unwrap();
    for text in ["{broken", "{\"not\": \"a list\"}", "", "[1, 2]"] {
        fs::write(store.root().join("findings.json"), text).unwrap();
        assert!(broke_facts_in(&store).is_empty(), "{text}");
    }

    let turns = turns_of("agent-a", None, PROJECT_A, End::Ok, 3);
    let inputs = Inputs { turns, broke: broke_facts_in(&store), ..Inputs::default() };
    let report = build_in(&inputs, Some(PROJECT_A), None, T0 + MIN * 60, &Utc, &labels());
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].broke_tests, 0);
}

#[test]
fn a_missing_findings_file_counts_as_zero() {
    let scratch = Scratch::new();
    assert!(broke_facts_in(&scratch.store()).is_empty());
}

#[test]
fn a_saved_findings_file_is_read_into_facts() {
    let scratch = Scratch::new();
    let store = scratch.store();
    fs::create_dir_all(store.root()).unwrap();
    let saved = vec![culprit("wb-0123456789ab", PROJECT_A, Some("agent-a"), Some("acct-1"), T0 + MIN)];
    fs::write(store.root().join("findings.json"), serde_json::to_string(&saved).unwrap()).unwrap();
    let facts = broke_facts_in(&store);
    assert_eq!(facts, vec![fact("agent-a", Some("acct-1"), PROJECT_A, T0 + MIN)]);
}

#[test]
fn the_count_never_changes_ahead_rates_or_enough() {
    // agent-a passes every turn, agent-b fails every turn: endedOk puts agent-a ahead.
    let mut turns = turns_of("agent-a", None, PROJECT_A, End::Ok, 10);
    turns.extend(turns_of("agent-b", None, PROJECT_A, End::Failed, 10));
    let mut facts: Vec<BrokeFact> = (0..40).map(|i| fact("agent-a", None, PROJECT_A, T0 + i * MIN)).collect();
    facts.extend((0..3).map(|i| fact("agent-b", None, PROJECT_A, T0 + i * MIN)));

    let without = aggregate(&turns, &[], Some(PROJECT_A), 0, &labels());
    let mut with = aggregate(&turns, &[], Some(PROJECT_A), 0, &labels());
    count_broke(&mut with, &facts, Some(PROJECT_A), 0);

    assert!(!row(&without, "agent-a", None).ahead.is_empty(), "the fixture must rank agent-a ahead");
    assert_eq!(row(&with, "agent-a", None).broke_tests, 40);
    assert_eq!(row(&with, "agent-b", None).broke_tests, 3);
    assert_eq!(row(&with, "agent-a", None).ahead, row(&without, "agent-a", None).ahead);
    // Every field but the count is the same, so rates, `enough` and `ahead` are unchanged.
    let mut stripped = with.clone();
    for r in &mut stripped {
        r.broke_tests = 0;
    }
    assert_eq!(stripped, without);
}

#[test]
fn facts_of_another_project_or_before_the_window_are_not_counted() {
    let turns = turns_of("agent-a", None, PROJECT_A, End::Ok, 3);
    let facts = vec![
        fact("agent-a", None, PROJECT_A, T0 + MIN),
        fact("agent-a", None, PROJECT_B, T0 + MIN),
        fact("agent-a", None, PROJECT_A, T0 - 1),
    ];
    let mut rows = aggregate(&turns, &[], Some(PROJECT_A), T0, &labels());
    count_broke(&mut rows, &facts, Some(PROJECT_A), T0);
    assert_eq!(rows[0].broke_tests, 1);
}

#[test]
fn build_puts_the_count_on_the_report_row_and_keeps_the_ranking() {
    let mut turns = turns_of("agent-a", None, PROJECT_A, End::Ok, 10);
    turns.extend(turns_of("agent-b", None, PROJECT_A, End::Failed, 10));
    let facts = vec![fact("agent-a", None, PROJECT_A, T0 + MIN), fact("agent-a", None, PROJECT_A, T0 + 2 * MIN)];
    let now = T0 + 60 * MIN;

    let plain = Inputs { turns: turns.clone(), ..Inputs::default() };
    let counted = Inputs { turns, broke: facts, ..Inputs::default() };
    let before = build_in(&plain, Some(PROJECT_A), None, now, &Utc, &labels());
    let after = build_in(&counted, Some(PROJECT_A), None, now, &Utc, &labels());

    let order: Vec<&str> = after.rows.iter().map(|r| r.agent_id.as_str()).collect();
    assert_eq!(order, before.rows.iter().map(|r| r.agent_id.as_str()).collect::<Vec<_>>());
    assert_eq!(row(&after.rows, "agent-a", None).broke_tests, 2);
    assert_eq!(row(&after.rows, "agent-b", None).broke_tests, 0);
    let v = serde_json::to_value(&after.rows[0]).unwrap();
    assert!(v.get("brokeTests").is_some(), "camel case key on the page answer");
}
