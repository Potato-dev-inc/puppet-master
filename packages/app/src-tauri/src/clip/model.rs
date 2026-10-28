//! The data a clip is made from: `ClipPlan` and what it holds.
//!
//! Rust builds the plan (already redacted) and the webview draws it. Field names are the camelCase
//! JSON that the webview reads (`src/lib/clipTypes.ts` mirrors them). Only `Serialize`: no Rust code
//! reads a plan back.

use std::time::Duration;

use serde::Serialize;

/// Most distinct frames in one replay.
pub const MAX_FRAMES: usize = 180;
/// Largest plan JSON, in bytes.
pub const MAX_PLAN_BYTES: usize = 3 * 1024 * 1024;
/// Longest replay, in clip milliseconds.
pub const REPLAY_MAX_MS: u32 = 18_000;
/// Longest real pause kept between two terminal events. Longer pauses are squeezed to this.
pub const GAP_MAX_MS: u64 = 400;
/// Most steps (tool calls) in one plan.
pub const MAX_STEPS: usize = 40;
/// Largest staged video file, in bytes.
pub const MAX_STAGE_BYTES: usize = 48 * 1024 * 1024;
/// How long a plan token stays valid.
pub const PLAN_TTL: Duration = Duration::from_secs(30 * 60);

/// Everything the clip sheet needs for one turn. Every string in it is already redacted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPlan {
    /// From `Plans::issue`, one per plan, valid for `PLAN_TTL`.
    pub token: String,
    /// Display label of the agent (for example "Claude Code"), strict-redacted.
    pub agent: String,
    /// The person's message, one line, at most 140 characters.
    pub request: String,
    /// Real length of the turn, in milliseconds.
    pub turn_ms: u64,
    /// "completed" | "interrupted" | "failed".
    pub outcome: String,
    pub replay: Replay,
    /// At most `MAX_STEPS`, in order.
    pub steps: Vec<Step>,
    pub result: ResultFacts,
    /// Plain sentences for the sheet, at most 3.
    pub notes: Vec<String>,
    /// Distinct screen rows that redaction changed.
    pub hidden: u32,
}

/// What the video shows on the replay part. `kind` is the JSON tag.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Replay {
    /// The terminal record of the turn, drawn frame by frame.
    Terminal {
        cols: u16,
        rows: u16,
        /// First and last row that is not blank in any frame.
        row_from: u16,
        row_to: u16,
        /// How much faster than real time the replay runs. Shown as "Sped up Nx".
        speed_x: u32,
        length_ms: u32,
        /// Each entry is one screen row as runs of text. `Frame::rows` indexes into this.
        row_table: Vec<Vec<Run>>,
        frames: Vec<Frame>,
    },
    /// No terminal record: the video shows the steps as a list.
    Activity { length_ms: u32 },
    /// Nothing to show (for example a shell pane without turns). The sheet explains.
    None,
}

/// A run of text with the same style. `s` holds the style bits: 1 bold, 2 dim, 4 inverse,
/// 8 underline, 16 highlighted background.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub t: String,
    pub s: u8,
}

/// One screen at one clip time. `rows` are indexes into the replay's `row_table`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub at_ms: u32,
    pub rows: Vec<u32>,
    /// `(row, col)`, or `None` when the cursor is hidden.
    pub cursor: Option<(u16, u16)>,
}

/// One tool call of the turn, in clip time. The webview decides its kind (read, edit, run...).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub at_ms: u32,
    pub name: String,
    pub summary: String,
    /// Last names only.
    pub paths: Vec<String>,
    pub writes: bool,
    pub helper: bool,
    /// `None` when the result never came back.
    pub ok: Option<bool>,
}

/// The result scene: what the turn changed and whether checks passed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultFacts {
    pub files: Option<FilesFact>,
    /// For example "all checks passed (2/2)".
    pub checks: Option<String>,
    /// The turn's one-line summary, redacted.
    pub sentence: String,
}

/// File totals of the turn. `names` holds at most 6 last names.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesFact {
    pub count: u32,
    pub added: u32,
    pub deleted: u32,
    pub names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn to_json<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).expect("serializes")
    }

    #[test]
    fn run_and_frame_use_the_wire_names() {
        assert_eq!(to_json(&Run { t: "ok".into(), s: 3 }), json!({"t": "ok", "s": 3}));
        let frame = Frame { at_ms: 1200, rows: vec![0, 1], cursor: Some((2, 5)) };
        assert_eq!(to_json(&frame), json!({"atMs": 1200, "rows": [0, 1], "cursor": [2, 5]}));
        let hidden = Frame { at_ms: 0, rows: vec![], cursor: None };
        assert_eq!(to_json(&hidden), json!({"atMs": 0, "rows": [], "cursor": null}));
    }

    #[test]
    fn step_uses_the_wire_names() {
        let step = Step {
            at_ms: 900,
            name: "Read".into(),
            summary: "Read a.rs".into(),
            paths: vec!["a.rs".into()],
            writes: false,
            helper: false,
            ok: Some(true),
        };
        assert_eq!(
            to_json(&step),
            json!({
                "atMs": 900, "name": "Read", "summary": "Read a.rs", "paths": ["a.rs"],
                "writes": false, "helper": false, "ok": true
            })
        );
        let no_result = Step { ok: None, ..step };
        assert_eq!(to_json(&no_result)["ok"], json!(null));
    }

    #[test]
    fn result_facts_use_the_wire_names() {
        let facts = ResultFacts {
            files: Some(FilesFact { count: 2, added: 86, deleted: 12, names: vec!["a.rs".into()] }),
            checks: Some("all checks passed (2/2)".into()),
            sentence: "Fixed the login.".into(),
        };
        assert_eq!(
            to_json(&facts),
            json!({
                "files": {"count": 2, "added": 86, "deleted": 12, "names": ["a.rs"]},
                "checks": "all checks passed (2/2)",
                "sentence": "Fixed the login."
            })
        );
        let none = ResultFacts { files: None, checks: None, sentence: String::new() };
        assert_eq!(to_json(&none), json!({"files": null, "checks": null, "sentence": ""}));
    }

    #[test]
    fn terminal_replay_uses_the_wire_names() {
        let replay = Replay::Terminal {
            cols: 80,
            rows: 24,
            row_from: 0,
            row_to: 23,
            speed_x: 40,
            length_ms: 20_000,
            row_table: vec![vec![Run { t: "$ ls".into(), s: 1 }]],
            frames: vec![Frame { at_ms: 0, rows: vec![0], cursor: None }],
        };
        assert_eq!(
            to_json(&replay),
            json!({
                "kind": "terminal", "cols": 80, "rows": 24, "rowFrom": 0, "rowTo": 23,
                "speedX": 40, "lengthMs": 20000,
                "rowTable": [[{"t": "$ ls", "s": 1}]],
                "frames": [{"atMs": 0, "rows": [0], "cursor": null}]
            })
        );
    }

    #[test]
    fn activity_replay_uses_the_wire_names() {
        assert_eq!(
            to_json(&Replay::Activity { length_ms: 9000 }),
            json!({"kind": "activity", "lengthMs": 9000})
        );
    }

    #[test]
    fn replay_none_is_only_the_kind() {
        assert_eq!(to_json(&Replay::None), json!({"kind": "none"}));
    }

    #[test]
    fn clip_plan_uses_the_wire_names() {
        let plan = ClipPlan {
            token: "t1".into(),
            agent: "Claude Code".into(),
            request: "Fix it".into(),
            turn_ms: 5000,
            outcome: "completed".into(),
            replay: Replay::None,
            steps: vec![],
            result: ResultFacts { files: None, checks: None, sentence: "Done.".into() },
            notes: vec!["Nothing to show.".into()],
            hidden: 3,
        };
        assert_eq!(
            to_json(&plan),
            json!({
                "token": "t1", "agent": "Claude Code", "request": "Fix it", "turnMs": 5000,
                "outcome": "completed", "replay": {"kind": "none"}, "steps": [],
                "result": {"files": null, "checks": null, "sentence": "Done."},
                "notes": ["Nothing to show."], "hidden": 3
            })
        );
    }

    #[test]
    fn limits_match_the_spec() {
        assert_eq!(MAX_FRAMES, 180);
        assert_eq!(MAX_PLAN_BYTES, 3 * 1024 * 1024);
        assert_eq!(REPLAY_MAX_MS, 18_000);
        assert_eq!(GAP_MAX_MS, 400);
        assert_eq!(MAX_STEPS, 40);
        assert_eq!(MAX_STAGE_BYTES, 48 * 1024 * 1024);
        assert_eq!(PLAN_TTL, Duration::from_secs(30 * 60));
    }
}
