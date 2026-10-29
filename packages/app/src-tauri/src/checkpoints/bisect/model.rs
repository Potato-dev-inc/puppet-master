//! Data model of What broke it (SPEC n33 section 3.2): the types and limits that the pure parts and the
//! saved findings share. Plain data and serde only; nothing here does I/O.
//!
//! Serde: `Verdict`, `Outcome` and `UnsureWhy` have fixed tags (`v`, `kind`, snake_case words) that the
//! golden tests below pin. `Limits` is camelCase because the window reads it. The persisted `Finding`
//! keeps snake_case field names, like the other files in the app data folder.

use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_RUNS: u32 = 16;
pub const DEFAULT_MAX_MINUTES: u32 = 20;
pub const MIN_RUNS: u32 = 1;
pub const MAX_RUNS: u32 = 30;
pub const MIN_MINUTES: u32 = 1;
pub const MAX_MINUTES: u32 = 60;
/// Last save points searched, at most.
pub const MAX_POINTS: usize = 100;
/// A copy bigger than this is refused before it is made.
pub const MAX_COPY_BYTES: u64 = 1_500 * 1024 * 1024;
/// Saved findings, newest first.
pub const FINDINGS_CAP: usize = 100;
/// Lines of the first failing run kept (redacted).
pub const TAIL_LINES: usize = 40;
/// A copy older than this, not owned by a live job, is swept at start.
pub const STALE_COPY_AGE_MS: u64 = 60 * 60_000;

/// Why a run gave no answer. Every value stops the search with plain words (words.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsureWhy {
    Flaky,
    TimedOut,
    NotStarted,
    SetupFailed,
    Stopped,
    OutOfTime,
    OutOfRuns,
}

/// The answer of one run at one save point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "v", rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
    Unsure { why: UnsureWhy },
}

/// The person's budget for one job. Clamped on every read and write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_runs: u32,
    pub max_minutes: u32,
}

impl Limits {
    /// The limits pulled into range: runs `1..=30`, minutes `1..=60`.
    pub fn clamped(max_runs: u32, max_minutes: u32) -> Limits {
        Limits {
            max_runs: max_runs.clamp(MIN_RUNS, MAX_RUNS),
            max_minutes: max_minutes.clamp(MIN_MINUTES, MAX_MINUTES),
        }
    }
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_runs: DEFAULT_MAX_RUNS, max_minutes: DEFAULT_MAX_MINUTES }
    }
}

/// One state of the folder worth testing: the end of a turn, or "now".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    /// Git tree id in the shadow repository.
    pub tree: String,
    pub at_ms: u64,
    /// `None` for "now".
    pub pane_key: Option<String>,
    pub turn: Option<u32>,
    pub checkpoint_id: Option<String>,
}

/// One agent turn as the save points show it (before .. after), possibly still open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub pane_key: String,
    pub turn: u32,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub before_tree: Option<String>,
    pub after_tree: Option<String>,
    pub before_id: Option<String>,
    pub after_id: Option<String>,
}

/// File counts of one turn, as the turn summary has them (no paths).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatsLite {
    pub count: u32,
    pub added: u32,
    pub deleted: u32,
}

/// Test counts of the first failing run, as `agent_runs::checks::counts` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountsLite {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub total: u32,
}

/// A span joined with its turn summary (when the summary is still kept).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suspect {
    pub pane_key: String,
    pub turn: u32,
    /// The turn's agent label, or "An agent" when the summary is gone. Never a name typed in generic code.
    pub agent_label: String,
    pub agent_id: Option<String>,
    pub account: Option<String>,
    /// End of the turn (or its start when still open).
    pub at_ms: u64,
    /// The turn summary sentence, already scrubbed.
    pub sentence: Option<String>,
    pub files: Option<FileStatsLite>,
}

/// The turns left as suspects after a narrowed search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Narrowed {
    /// First turn in the range.
    pub from: Suspect,
    pub to: Suspect,
    pub turns: u32,
}

/// What a finished job says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Culprit { suspect: Suspect, failing: Vec<String>, counts: Option<CountsLite>, last_good_ms: u64 },
    Several { suspects: Vec<Suspect>, last_good_ms: u64, first_bad_ms: u64 },
    BetweenTurns { last_good_ms: u64, first_bad_ms: u64 },
    /// Passes in the clean copy at "now".
    TestsPass,
    AlwaysRed { oldest_ms: u64 },
    /// The save point the person picked fails.
    GoodWasRed { at_ms: u64 },
    NotSure { why: UnsureWhy, narrowed: Option<Narrowed> },
    /// The sanity gate: the copy cannot be trusted.
    CopyDiffers { detail: String },
    /// Codes are listed in SPEC n33 section 3.6.
    Failed { code: String, message: String },
}

/// One cached answer, so "Keep looking" does not run a tree again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub tree: String,
    pub verdict: Verdict,
}

/// One saved result of a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// `wb-` + 12 lowercase hex. Checked by `store::valid_id` before any lookup.
    pub id: String,
    /// Comparable project key (local only).
    pub project: String,
    /// The folder searched (local only).
    pub folder: String,
    /// The pane the person pressed the button from.
    pub pane_key: String,
    pub started_ms: u64,
    pub finished_ms: u64,
    pub check_name: String,
    pub check_argv: Vec<String>,
    pub runs: u32,
    pub minutes_x10: u32,
    pub points: u32,
    pub outcome: Outcome,
    /// Last `TAIL_LINES` lines of the first failing run, redacted, with `~` for the home folder.
    pub tail: String,
    pub probes: Vec<Probe>,
    /// Set by an undo.
    pub undone_ms: Option<u64>,
    /// Narrowed range, for "Keep looking".
    pub good_idx: Option<u32>,
    pub bad_idx: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn suspect() -> Suspect {
        Suspect {
            pane_key: "p1".into(),
            turn: 7,
            agent_label: "An agent".into(),
            agent_id: None,
            account: None,
            at_ms: 42,
            sentence: None,
            files: None,
        }
    }

    #[test]
    fn verdict_tags_are_stable() {
        assert_eq!(serde_json::to_value(Verdict::Pass).unwrap(), json!({"v": "pass"}));
        assert_eq!(serde_json::to_value(Verdict::Fail).unwrap(), json!({"v": "fail"}));
        assert_eq!(
            serde_json::to_value(Verdict::Unsure { why: UnsureWhy::Flaky }).unwrap(),
            json!({"v": "unsure", "why": "flaky"})
        );
        let back: Verdict = serde_json::from_value(json!({"v": "unsure", "why": "out_of_runs"})).unwrap();
        assert_eq!(back, Verdict::Unsure { why: UnsureWhy::OutOfRuns });
    }

    #[test]
    fn unsure_why_words_are_stable() {
        let words = [
            (UnsureWhy::Flaky, "flaky"),
            (UnsureWhy::TimedOut, "timed_out"),
            (UnsureWhy::NotStarted, "not_started"),
            (UnsureWhy::SetupFailed, "setup_failed"),
            (UnsureWhy::Stopped, "stopped"),
            (UnsureWhy::OutOfTime, "out_of_time"),
            (UnsureWhy::OutOfRuns, "out_of_runs"),
        ];
        for (why, word) in words {
            assert_eq!(serde_json::to_value(why).unwrap(), json!(word));
        }
    }

    #[test]
    fn outcome_tags_are_stable() {
        assert_eq!(serde_json::to_value(Outcome::TestsPass).unwrap(), json!({"kind": "tests_pass"}));
        assert_eq!(
            serde_json::to_value(Outcome::BetweenTurns { last_good_ms: 1, first_bad_ms: 2 }).unwrap(),
            json!({"kind": "between_turns", "last_good_ms": 1, "first_bad_ms": 2})
        );
        assert_eq!(
            serde_json::to_value(Outcome::NotSure { why: UnsureWhy::Stopped, narrowed: None }).unwrap(),
            json!({"kind": "not_sure", "why": "stopped", "narrowed": null})
        );
        assert_eq!(
            serde_json::to_value(Outcome::Failed { code: "WB_BUSY".into(), message: "m".into() }).unwrap(),
            json!({"kind": "failed", "code": "WB_BUSY", "message": "m"})
        );
        let culprit = Outcome::Culprit { suspect: suspect(), failing: vec!["a".into()], counts: None, last_good_ms: 9 };
        let value = serde_json::to_value(&culprit).unwrap();
        assert_eq!(value["kind"], json!("culprit"));
        assert_eq!(value["last_good_ms"], json!(9));
        assert_eq!(value["suspect"]["turn"], json!(7));
        let back: Outcome = serde_json::from_value(value).unwrap();
        assert_eq!(back, culprit);
    }

    #[test]
    fn limits_are_clamped_and_camel_case() {
        assert_eq!(Limits::clamped(0, 999), Limits { max_runs: 1, max_minutes: 60 });
        assert_eq!(Limits::clamped(99, 0), Limits { max_runs: 30, max_minutes: 1 });
        assert_eq!(Limits::clamped(16, 20), Limits::default());
        assert_eq!(Limits::default().max_runs, 16);
        assert_eq!(Limits::default().max_minutes, 20);
        assert_eq!(serde_json::to_value(Limits::default()).unwrap(), json!({"maxRuns": 16, "maxMinutes": 20}));
    }

    #[test]
    fn finding_round_trips_through_json() {
        let finding = Finding {
            id: "wb-0123456789ab".into(),
            project: "C:/you/app".into(),
            folder: "C:/you/app".into(),
            pane_key: "p1".into(),
            started_ms: 1,
            finished_ms: 2,
            check_name: "npm test".into(),
            check_argv: vec!["npm".into(), "test".into()],
            runs: 5,
            minutes_x10: 12,
            points: 9,
            outcome: Outcome::TestsPass,
            tail: "line".into(),
            probes: vec![Probe { tree: "t".into(), verdict: Verdict::Fail }],
            undone_ms: None,
            good_idx: Some(0),
            bad_idx: None,
        };
        let text = serde_json::to_string(&finding).unwrap();
        let back: Finding = serde_json::from_str(&text).unwrap();
        assert_eq!(back, finding);
    }
}
