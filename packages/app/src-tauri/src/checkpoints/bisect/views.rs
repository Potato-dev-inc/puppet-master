//! What the window reads (SPEC n33 section 3.6): the shapes `commands/what_broke_it.rs` returns and
//! `src/lib/what-broke-it.ts` mirrors. Plain data, plus the builders that turn a saved `Finding` into the
//! `FindingView` (title, subtitle, kind, suspects). No I/O.
//!
//! Field spelling follows the window's types: `DeclaredView`, `SuggestionView` and `LimitsView` are
//! snake_case (the window reads `command_text`, `max_runs`); every other view is camelCase.
//! A view never carries the folder path or the command's argv: only `command_text` for a declared test.
//! Test output reaches a view only as `Finding.tail`, which was redacted before it was saved (S9).

use serde::{Deserialize, Serialize};

use super::failnames;
use super::model::{FileStatsLite, Finding, Limits, Narrowed, Outcome, Suspect};
use super::words;
use crate::checkpoints::FileChange;

/// The code of a "Check it again" run that still fails (not one of the spec's codes: it is an answer, not an error).
pub const CODE_STILL_FAILING: &str = "WB_STILL_FAILING";

/// A declared test, as the sheet lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclaredView {
    pub index: usize,
    pub name: String,
    pub command_text: String,
    pub timeout_min: u32,
    pub warnings: Vec<String>,
}

/// A test the app found in the folder. Not used until the person presses `Use this`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuggestionView {
    pub name: String,
    pub command_text: String,
    pub source: String,
}

/// `Limits` spelled the way the window reads and writes it (`max_runs`). The camelCase spelling is accepted too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsView {
    #[serde(alias = "maxRuns")]
    pub max_runs: u32,
    #[serde(alias = "maxMinutes")]
    pub max_minutes: u32,
}

impl From<Limits> for LimitsView {
    fn from(limits: Limits) -> LimitsView {
        LimitsView { max_runs: limits.max_runs, max_minutes: limits.max_minutes }
    }
}

impl LimitsView {
    /// The limits pulled into range (runs 1..=30, minutes 1..=60).
    pub fn clamped(self) -> Limits {
        Limits::clamped(self.max_runs, self.max_minutes)
    }
}

/// One save point in the "Tests last passed" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointView {
    /// `<pane key>:<save point id>`: an id is only unique inside one pane. Opaque to the window.
    pub checkpoint_id: String,
    pub label: String,
    pub at_ms: u64,
}

/// The first thing the sheet shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub tests: Vec<DeclaredView>,
    pub suggestions: Vec<SuggestionView>,
    pub points: Vec<PointView>,
    pub limits: LimitsView,
    pub can: bool,
    pub why_not: Option<String>,
    pub busy: Option<String>,
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Preparing,
    Running,
    Finishing,
    Done,
}

/// One job as the window polls it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub state: JobState,
    pub point_no: u32,
    pub points: u32,
    pub run: u32,
    pub max_runs: u32,
    pub elapsed_ms: u64,
    pub max_ms: u64,
    pub line: String,
    pub finding_id: Option<String>,
}

/// One suspect turn (the window's `SuspectView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspectView {
    pub pane_key: String,
    pub turn: u32,
    pub agent_label: String,
    pub agent_id: Option<String>,
    pub account: Option<String>,
    pub at_ms: u64,
    pub sentence: Option<String>,
    pub files: Option<FileStatsLite>,
}

impl From<&Suspect> for SuspectView {
    fn from(s: &Suspect) -> SuspectView {
        SuspectView {
            pane_key: s.pane_key.clone(),
            turn: s.turn,
            agent_label: s.agent_label.clone(),
            agent_id: s.agent_id.clone(),
            account: s.account.clone(),
            at_ms: s.at_ms,
            sentence: s.sentence.clone(),
            files: s.files.clone(),
        }
    }
}

/// One finished search, as the card shows it. `title` and `subtitle` carry the plain words of the kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// The snake_case tag of the outcome (`culprit`, `several`, ...).
    pub kind: String,
    pub suspects: Vec<SuspectView>,
    pub failing: Vec<String>,
    pub tail: String,
    pub can_undo: bool,
    pub undone_at_ms: Option<u64>,
    /// When the tests last passed, for a culprit (the window writes the `Last time they passed` line).
    pub last_good_ms: Option<u64>,
}

/// The file list of the culprit turn and, when one file was asked for, its text diff.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffView {
    pub files: Vec<FileChange>,
    pub patch: Option<String>,
    pub truncated: bool,
}

/// A path an undo cannot put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictView {
    pub path: String,
    pub why: String,
}

/// The dry run of an undo. Writes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoPlanView {
    pub writes: Vec<String>,
    pub removes: Vec<String>,
    pub conflicts: Vec<ConflictView>,
    pub blocked_by: Option<String>,
}

/// What a written undo did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReportView {
    pub undone_at_ms: u64,
    pub written: u32,
    pub removed: u32,
}

/// Local clock time as `14:10`.
pub fn clock(ms: u64) -> String {
    use chrono::{DateTime, Local};
    match DateTime::from_timestamp_millis(ms as i64) {
        Some(utc) => utc.with_timezone(&Local).format("%H:%M").to_string(),
        None => "--:--".to_string(),
    }
}

/// The `kind` word of an outcome: the serde tag.
pub fn kind_of(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Culprit { .. } => "culprit",
        Outcome::Several { .. } => "several",
        Outcome::BetweenTurns { .. } => "between_turns",
        Outcome::TestsPass => "tests_pass",
        Outcome::AlwaysRed { .. } => "always_red",
        Outcome::GoodWasRed { .. } => "good_was_red",
        Outcome::NotSure { .. } => "not_sure",
        Outcome::CopyDiffers { .. } => "copy_differs",
        Outcome::Failed { .. } => "failed",
    }
}

/// The agent labels of a narrowed range, each once.
fn narrowed_labels(n: &Narrowed) -> Vec<String> {
    vec![n.from.agent_label.clone(), n.to.agent_label.clone()]
}

/// Builds the card's view of a saved finding. `no_setup` says the project sets no copy setup steps (it
/// adds a note to an "already failing" result). `Finding.tail` is already redacted.
pub fn finding_view(f: &Finding, no_setup: bool) -> FindingView {
    let mut view = FindingView {
        id: f.id.clone(),
        title: String::new(),
        subtitle: String::new(),
        kind: kind_of(&f.outcome).to_string(),
        suspects: Vec::new(),
        failing: Vec::new(),
        tail: f.tail.clone(),
        can_undo: false,
        undone_at_ms: f.undone_ms,
        last_good_ms: None,
    };
    match &f.outcome {
        Outcome::Culprit { suspect, failing, last_good_ms, .. } => {
            view.title = words::found_title(&suspect.agent_label, suspect.turn);
            view.subtitle = words::found_subtitle(&clock(suspect.at_ms), suspect.sentence.as_deref(), suspect.files.as_ref());
            view.suspects = vec![SuspectView::from(suspect)];
            view.failing = failing.clone();
            view.can_undo = f.undone_ms.is_none();
            view.last_good_ms = Some(*last_good_ms);
        }
        Outcome::Several { suspects, last_good_ms, .. } => {
            view.title = words::several_intro(suspects.len());
            view.suspects = suspects.iter().map(SuspectView::from).collect();
            view.last_good_ms = Some(*last_good_ms);
        }
        Outcome::BetweenTurns { last_good_ms, first_bad_ms } => {
            view.title = words::between(&clock(*last_good_ms), &clock(*first_bad_ms));
            view.last_good_ms = Some(*last_good_ms);
        }
        Outcome::TestsPass => view.title = words::tests_pass_note().to_string(),
        Outcome::AlwaysRed { oldest_ms } => {
            view.title = words::always_red(&clock(*oldest_ms));
            if no_setup {
                view.subtitle = words::no_setup_note().to_string();
            }
        }
        Outcome::GoodWasRed { at_ms } => view.title = words::good_was_red(&clock(*at_ms)),
        Outcome::NotSure { why, narrowed } => {
            view.title = words::not_sure(*why);
            if let Some(n) = narrowed {
                view.subtitle = words::narrowed(n.from.turn, n.to.turn, &narrowed_labels(n));
            }
        }
        Outcome::CopyDiffers { detail } => view.title = words::copy_differs(detail),
        Outcome::Failed { code, message } => {
            view.title = message.clone();
            if code == CODE_STILL_FAILING {
                view.failing = failnames::failing_tests(&f.tail);
            }
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoints::bisect::model::UnsureWhy;

    fn suspect(agent: &str, turn: u32) -> Suspect {
        Suspect {
            pane_key: "p1".into(),
            turn,
            agent_label: agent.into(),
            agent_id: Some("a1".into()),
            account: None,
            at_ms: 1_800_000_000_000,
            sentence: Some("refactor cart".into()),
            files: Some(FileStatsLite { count: 4, added: 52, deleted: 20 }),
        }
    }

    fn finding(outcome: Outcome) -> Finding {
        Finding {
            id: "wb-0123456789ab".into(),
            project: "c:/you/app".into(),
            folder: "C:\\Users\\you\\app".into(),
            pane_key: "p1".into(),
            started_ms: 1,
            finished_ms: 2,
            check_name: "npm test".into(),
            check_argv: vec!["npm".into(), "test".into()],
            runs: 3,
            minutes_x10: 5,
            points: 9,
            outcome,
            tail: "FAIL cart.test.ts".into(),
            probes: Vec::new(),
            undone_ms: None,
            good_idx: None,
            bad_idx: None,
        }
    }

    #[test]
    fn culprit_view_uses_the_spec_words() {
        let f = finding(Outcome::Culprit {
            suspect: suspect("Agent X", 7),
            failing: vec!["cart.test.ts".into()],
            counts: None,
            last_good_ms: 1_799_999_000_000,
        });
        let v = finding_view(&f, false);
        assert_eq!(v.title, "Turn 7 by Agent X broke the tests");
        assert!(v.subtitle.contains("\"refactor cart\" · 4 files, +52 −20"), "{}", v.subtitle);
        assert_eq!(v.kind, "culprit");
        assert!(v.can_undo);
        assert_eq!(v.last_good_ms, Some(1_799_999_000_000));
        assert_eq!(v.suspects.len(), 1);
    }

    #[test]
    fn an_undone_culprit_cannot_be_undone_again() {
        let mut f = finding(Outcome::Culprit { suspect: suspect("Agent X", 7), failing: vec![], counts: None, last_good_ms: 1 });
        f.undone_ms = Some(5);
        let v = finding_view(&f, false);
        assert!(!v.can_undo);
        assert_eq!(v.undone_at_ms, Some(5));
    }

    #[test]
    fn budget_stops_read_keep_looking_text() {
        let n = Narrowed { from: suspect("Agent X", 5), to: suspect("Agent Y", 8), turns: 4 };
        let f = finding(Outcome::NotSure { why: UnsureWhy::OutOfRuns, narrowed: Some(n) });
        let v = finding_view(&f, false);
        assert_eq!(v.title, "Not sure. Ran out of runs.");
        assert_eq!(v.subtitle, "It was one of turns 5 to 8 (Agent X, Agent Y).");
        assert!(!v.can_undo);
    }

    #[test]
    fn every_kind_has_a_title_and_the_serde_tag() {
        let outcomes = vec![
            Outcome::Several { suspects: vec![suspect("A", 1), suspect("B", 2)], last_good_ms: 1, first_bad_ms: 2 },
            Outcome::BetweenTurns { last_good_ms: 1, first_bad_ms: 2 },
            Outcome::TestsPass,
            Outcome::AlwaysRed { oldest_ms: 1 },
            Outcome::GoodWasRed { at_ms: 1 },
            Outcome::CopyDiffers { detail: "A link was found.".into() },
            Outcome::Failed { code: "WB_BUSY".into(), message: "Already looking in app.".into() },
        ];
        for outcome in outcomes {
            let tag = serde_json::to_value(&outcome).unwrap()["kind"].as_str().unwrap().to_string();
            let v = finding_view(&finding(outcome), true);
            assert_eq!(v.kind, tag);
            assert!(!v.title.is_empty(), "{tag}");
            assert!(!v.can_undo, "{tag}");
        }
    }

    #[test]
    fn always_red_adds_the_setup_note_only_without_setup() {
        let f = finding(Outcome::AlwaysRed { oldest_ms: 1 });
        assert_eq!(finding_view(&f, true).subtitle, words::no_setup_note());
        assert_eq!(finding_view(&f, false).subtitle, "");
    }

    #[test]
    fn a_failed_recheck_lists_the_failing_names_from_the_tail() {
        let mut f = finding(Outcome::Failed { code: CODE_STILL_FAILING.into(), message: words::still_failing().into() });
        f.tail = "not ok 1 - cart adds\nnot ok 2 - cart removes".into();
        let v = finding_view(&f, false);
        assert_eq!(v.failing, vec!["cart adds".to_string(), "cart removes".to_string()]);
    }

    #[test]
    fn views_keep_the_window_spelling_and_no_folder() {
        let v = serde_json::to_value(DeclaredView {
            index: 0,
            name: "npm test".into(),
            command_text: "npm test".into(),
            timeout_min: 10,
            warnings: vec![],
        })
        .unwrap();
        assert!(v.get("command_text").is_some() && v.get("timeout_min").is_some());
        let limits = serde_json::to_value(LimitsView::from(Limits::default())).unwrap();
        assert_eq!(limits, serde_json::json!({"max_runs": 16, "max_minutes": 20}));
        let camel: LimitsView = serde_json::from_value(serde_json::json!({"maxRuns": 3, "maxMinutes": 4})).unwrap();
        assert_eq!(camel.clamped(), Limits { max_runs: 3, max_minutes: 4 });
        let finding_json = serde_json::to_string(&finding_view(&finding(Outcome::TestsPass), false)).unwrap();
        assert!(!finding_json.contains("Users") && !finding_json.contains("argv"), "{finding_json}");
        let job = serde_json::to_value(JobView {
            id: "j".into(),
            state: JobState::Running,
            point_no: 1,
            points: 2,
            run: 1,
            max_runs: 16,
            elapsed_ms: 0,
            max_ms: 1,
            line: String::new(),
            finding_id: None,
        })
        .unwrap();
        assert_eq!(job["state"], "running");
        assert!(job.get("pointNo").is_some() && job.get("findingId").is_some());
    }
}
