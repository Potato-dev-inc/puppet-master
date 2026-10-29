//! The end-of-Focus digest: the one JSON shape the card reads (`DigestFocus` in `lib/cockpit-api.ts`), the
//! normal digest unchanged, and the away card staying quiet while Focus is on.

use super::digest::{self, Digest, DigestFocus};
use super::model::{assemble, Facts, TurnSummary};
use super::{away_digest, focus_digest};
use crate::attention::focus::model::HeldCounts;
use crate::attention::focus::summary::{Counts, EndedBy, FocusSummary};
use serde_json::Value;
use std::path::PathBuf;

const T0: u64 = 1_793_265_600_000;
const MIN: u64 = 60_000;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/turn_summary/golden").join(format!("{name}.json"))
}

fn check(name: &str, actual: &Value) {
    let path = golden_path(name);
    if std::env::var("PM_UPDATE_GOLDEN").is_ok() {
        std::fs::write(&path, serde_json::to_string_pretty(actual).unwrap() + "\n").unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {}", path.display()));
    let expected: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(&expected, actual, "golden {name} differs; actual:\n{}", serde_json::to_string_pretty(actual).unwrap());
}

fn focus_summary() -> FocusSummary {
    FocusSummary {
        session_id: "fc-0123456789ab".into(),
        started_at_ms: T0,
        ended_at_ms: T0 + 45 * MIN,
        minutes: 45,
        ended_by: EndedBy::Timer,
        counts: Counts {
            turns_done: 4,
            agents: 2,
            failed: 1,
            asked: 2,
            urgent: 1,
            still_waiting: 1,
            held: HeldCounts { run_done: 3, run_failed: 1, companion: 2, review: 1, night: 0 },
        },
        line: "While you focused (45 min): 4 turns done, 1 failed, 2 asked for you, 1 urgent. 1 still waits.".into(),
    }
}

fn turn(pane: &str, n: u32, ended: u64) -> TurnSummary {
    assemble(Facts {
        pane_id: pane.into(),
        agent: "Fake Agent".into(),
        agent_id: None,
        account: None,
        turn: n,
        started_at_ms: ended - 10,
        ended_at_ms: ended,
        files: None,
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: None,
    })
}

#[test]
fn digest_focus_serializes_to_the_shape_the_card_reads() {
    // Golden: camelCase, endedBy a word, counts.held one number (the total of pings held: 3+1+2+1+0 = 7).
    let json = serde_json::to_value(DigestFocus::from(&focus_summary())).unwrap();
    check("digest_focus", &json);
    assert_eq!(json["counts"]["held"], 7);
    assert_eq!(json["endedBy"], "timer");
    assert!(json.get("ended_by").is_none() && json["counts"].get("turns_done").is_none(), "no snake_case keys");
}

#[test]
fn an_early_end_says_early() {
    let mut summary = focus_summary();
    summary.ended_by = EndedBy::Early;
    assert_eq!(serde_json::to_value(DigestFocus::from(&summary)).unwrap()["endedBy"], "early");
}

#[test]
fn a_normal_digest_has_no_focus_key_and_round_trips() {
    let digest = digest::build(&[turn("p1", 1, T0 + MIN)], vec![], T0, T0 + 10 * MIN);
    let json = serde_json::to_value(&digest).unwrap();
    assert!(json.get("focus").is_none(), "the away digest is unchanged: {json}");
    let back: Digest = serde_json::from_value(json).unwrap();
    assert_eq!(back, digest);
    // An older saved digest without the key still loads.
    let old = r#"{"sinceMs":1,"nowMs":2,"agentsWorked":0,"turns":0,"lines":[],"needsYou":[]}"#;
    assert!(serde_json::from_str::<Digest>(old).unwrap().focus.is_none());
}

#[test]
fn a_focus_digest_is_never_empty_and_an_empty_away_digest_still_is() {
    let away = digest::build(&[], vec![], T0, T0 + MIN);
    assert!(away.is_empty());
    let focus = focus_digest(&focus_summary(), &[], vec![]);
    assert!(!focus.is_empty(), "held updates and asks are worth a card even with no agent line");
    assert_eq!(focus.focus, Some(DigestFocus::from(&focus_summary())));
}

#[test]
fn the_focus_digest_lists_the_agent_lines_of_the_window_only() {
    let summary = focus_summary();
    let turns = [turn("p1", 1, T0 + 5 * MIN), turn("p1", 2, T0 + 9 * MIN), turn("p2", 1, T0 + 46 * MIN), turn("p3", 1, T0 - MIN)];
    let digest = focus_digest(&summary, &turns, vec![]);
    let panes: Vec<&str> = digest.lines.iter().map(|l| l.pane_id.as_str()).collect();
    assert_eq!(panes, vec!["p1"], "after the end and before the start are left out");
    assert_eq!(digest.lines[0].turns, 2);
    assert_eq!((digest.since_ms, digest.now_ms), (summary.started_at_ms, summary.ended_at_ms));
}

#[test]
fn the_away_card_stays_quiet_while_focus_is_on() {
    assert!(away_digest(T0, true).is_none(), "the Focus summary replaces it");
}
