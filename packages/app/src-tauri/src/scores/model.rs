//! The types of "Scores on your code". Facts (internal) are plain Rust; the page's answer is
//! camel case JSON. No folder, pane id, run handle, prompt, reply, file name or approval text is in
//! any of these types, except the labels from the agent registry and accounts store.

use serde::Serialize;

/// A row is `ahead` in a metric only with at least this many runs in that metric.
pub const MIN_SAMPLE: u32 = 5;
/// A rate with at least this many runs is "steady": it shows a percentage as well.
pub const STEADY_SAMPLE: u32 = 10;
/// The "per merged change" column needs this many merged runs.
pub const MIN_MERGED_FOR_TYPICAL: u32 = 3;

// ---- facts (internal, not serialised) ----

/// How a turn ended, as far as the score cares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    Ok,
    Failed,
    /// Stopped by the person after it had started. Left out of "Ended OK".
    Stopped,
}

/// The state of a run's own copy (worktree), from its latest turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyState {
    NoCopy,
    Active,
    Waiting,
    Merged,
    Discarded,
}

/// One turn, cut down to the fields the scores need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreTurn {
    /// `comparable_project_path` of the ORIGIN project.
    pub project: String,
    pub agent_id: String,
    pub account: Option<String>,
    /// Join key only; never leaves the module.
    pub pane_id: Option<String>,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub end: End,
    /// Files changed. `None` = no summary.
    pub files_changed: Option<u32>,
    /// Lines added + deleted. `None` = no summary.
    pub lines: Option<u32>,
    /// `None` = no summary.
    pub asked_you: Option<bool>,
    /// `None` = no checks, or the checks did not get an answer.
    pub checks_green: Option<bool>,
    /// `None` = unknown (no files, not covered by the undo log, or no pane).
    pub undone: Option<bool>,
}

/// One run (all turns of one `agent_run_id`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreRun {
    pub project: String,
    pub agent_id: String,
    pub account: Option<String>,
    pub turns: u32,
    /// Sum of the run's finished turns, in milliseconds.
    pub busy_ms: u64,
    pub last_started_ms: u64,
    pub copy: CopyState,
}

/// One Undo that put files back. Turns of `pane` that started at or after `since_ms` and ended by
/// `at_ms` were undone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoMark {
    pub pane: String,
    pub since_ms: u64,
    pub at_ms: u64,
}

// ---- the page's answer ----

/// How much history a rate has. Decides what the page shows.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Sample {
    /// 0 runs: shown as `-`.
    None,
    /// 1 to 4 runs: shown as `-` with a tooltip.
    NotEnough,
    /// 5 to 9 runs: shown as `x of y`, muted, tagged `early`.
    Early,
    /// 10 or more runs: shown as `x of y` and a percentage.
    Steady,
}

/// One rate: `count` of `total`, with a 90% Wilson interval (`low`..`high`, 0.0..1.0).
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub count: u32,
    pub total: u32,
    pub sample: Sample,
    pub low: f32,
    pub high: f32,
}

/// Typical size of a merged run.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PerMerged {
    pub runs: u32,
    pub turns_median: u32,
    pub busy_ms_median: u64,
}

/// One row: an agent under one account.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRow {
    pub agent_id: String,
    pub agent_label: String,
    /// `None` = the agent's own login.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub account_label: String,
    pub turns: u32,
    /// `false` = the page shows "Not enough runs yet".
    pub enough: bool,
    pub ended_ok: Rate,
    pub checks_green: Rate,
    pub merged: Rate,
    pub undone: Rate,
    pub asked_you: Rate,
    pub per_merged: Option<PerMerged>,
    /// Runs whose copy is still waiting for review.
    pub waiting_review: u32,
    pub turn_ms_median: Option<u64>,
    pub lines_median: Option<u32>,
    /// Subset of `"endedOk"`, `"checksGreen"`, `"merged"`, `"undone"`.
    pub ahead: Vec<String>,
}

/// A project in the project list. `id` is the first 12 hex characters of a hash of the project key.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    /// The folder's last name only (duplicates get ` (2)`).
    pub label: String,
    pub turns: u32,
}

/// The whole answer of `agent_scores`. `PartialEq` only: the rates hold `f32`, so `Eq` cannot hold.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScoresReport {
    /// `"project"` or `"all"`.
    pub scope: String,
    /// Set when `scope == "project"`.
    pub project: Option<ProjectInfo>,
    /// Newest activity first, at most 50.
    pub projects: Vec<ProjectInfo>,
    /// `None` = all time.
    pub days: Option<u32>,
    pub since_ms: u64,
    pub min_sample: u32,
    pub rows: Vec<ScoreRow>,
    /// Plain sentences for the page.
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sample_tiers_use_snake_case_names() {
        assert_eq!(serde_json::to_value(Sample::None).unwrap(), json!("none"));
        assert_eq!(serde_json::to_value(Sample::NotEnough).unwrap(), json!("not_enough"));
        assert_eq!(serde_json::to_value(Sample::Early).unwrap(), json!("early"));
        assert_eq!(serde_json::to_value(Sample::Steady).unwrap(), json!("steady"));
    }

    #[test]
    fn rate_uses_camel_case_keys() {
        // Exact binary fractions: f32 values are checked without rounding noise.
        let rate = Rate { count: 8, total: 10, sample: Sample::Steady, low: 0.5, high: 0.75 };
        let v = serde_json::to_value(rate).unwrap();
        assert_eq!(v, json!({ "count": 8, "total": 10, "sample": "steady", "low": 0.5, "high": 0.75 }));
    }

    #[test]
    fn per_merged_uses_camel_case_keys() {
        let pm = PerMerged { runs: 3, turns_median: 2, busy_ms_median: 840_000 };
        let v = serde_json::to_value(pm).unwrap();
        assert_eq!(v, json!({ "runs": 3, "turnsMedian": 2, "busyMsMedian": 840000 }));
    }

    fn empty_rate() -> Rate {
        Rate { count: 0, total: 0, sample: Sample::None, low: 0.0, high: 1.0 }
    }

    #[test]
    fn score_row_uses_camel_case_and_skips_missing_account_id() {
        let row = ScoreRow {
            agent_id: "agent-a".into(),
            agent_label: "Agent A".into(),
            account_id: None,
            account_label: "Default account".into(),
            turns: 2,
            enough: false,
            ended_ok: empty_rate(),
            checks_green: empty_rate(),
            merged: empty_rate(),
            undone: empty_rate(),
            asked_you: empty_rate(),
            per_merged: None,
            waiting_review: 0,
            turn_ms_median: None,
            lines_median: None,
            ahead: vec!["endedOk".into()],
        };
        let v = serde_json::to_value(&row).unwrap();
        assert!(v.get("accountId").is_none(), "no account id key when the agent has its own login");
        assert_eq!(v["agentId"], json!("agent-a"));
        assert_eq!(v["agentLabel"], json!("Agent A"));
        assert_eq!(v["accountLabel"], json!("Default account"));
        assert_eq!(v["endedOk"]["sample"], json!("none"));
        assert_eq!(v["checksGreen"]["total"], json!(0));
        assert_eq!(v["perMerged"], json!(null));
        assert_eq!(v["waitingReview"], json!(0));
        assert_eq!(v["turnMsMedian"], json!(null));
        assert_eq!(v["linesMedian"], json!(null));
        assert_eq!(v["ahead"], json!(["endedOk"]));
    }

    #[test]
    fn score_row_keeps_account_id_when_present() {
        let row = ScoreRow {
            agent_id: "agent-a".into(),
            agent_label: "Agent A".into(),
            account_id: Some("acct-1".into()),
            account_label: "Work".into(),
            turns: 0,
            enough: false,
            ended_ok: empty_rate(),
            checks_green: empty_rate(),
            merged: empty_rate(),
            undone: empty_rate(),
            asked_you: empty_rate(),
            per_merged: None,
            waiting_review: 0,
            turn_ms_median: Some(60_000),
            lines_median: Some(38),
            ahead: vec![],
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["accountId"], json!("acct-1"));
        assert_eq!(v["turnMsMedian"], json!(60000));
        assert_eq!(v["linesMedian"], json!(38));
    }

    #[test]
    fn report_uses_camel_case_keys() {
        let report = ScoresReport {
            scope: "project".into(),
            project: Some(ProjectInfo { id: "0123456789ab".into(), label: "shop".into(), turns: 4 }),
            projects: vec![],
            days: Some(90),
            since_ms: 1_000,
            min_sample: MIN_SAMPLE,
            rows: vec![],
            notes: vec!["Counting Undo from now on.".into()],
        };
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["scope"], json!("project"));
        assert_eq!(v["project"]["id"], json!("0123456789ab"));
        assert_eq!(v["days"], json!(90));
        assert_eq!(v["sinceMs"], json!(1000));
        assert_eq!(v["minSample"], json!(5));
        assert_eq!(v["notes"][0], json!("Counting Undo from now on."));
    }

    #[test]
    fn sample_constants_match_the_spec() {
        assert_eq!(MIN_SAMPLE, 5);
        assert_eq!(STEADY_SAMPLE, 10);
        assert_eq!(MIN_MERGED_FOR_TYPICAL, 3);
    }
}
