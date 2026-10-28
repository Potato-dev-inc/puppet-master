//! Night Shift data model (SPEC n32 section 3.2). Pure data, serde only.

use serde::{Deserialize, Serialize};

use crate::missions::model::AgentChoice;

/// Who runs a task: any ticked plan, or one fixed plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "who", rename_all = "snake_case")]
pub enum Who {
    Any,
    Fixed { agent: AgentChoice },
}

/// What the person asked for, verbatim (local only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    /// "nt-" + 8 lowercase hex.
    pub id: String,
    /// Origin project path (normalised; the key operations and missions use).
    pub project: String,
    /// The person's words, at most 2000 characters.
    pub text: String,
    pub who: Who,
    pub may_change_tests: bool,
    pub turns: u32,
    pub minutes: u32,
}

/// A plan the person ticked for the run, in the person's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanChoice {
    pub agent: AgentChoice,
    pub label: String,
    pub vendor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    #[default]
    Queued,
    Waiting {
        until_ms: Option<u64>,
        why: String,
    },
    Working,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    Mission,
    Single,
}

/// How one try ended. Plain words only in `sentence` fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "end", rename_all = "snake_case")]
pub enum AttemptEnd {
    Completed,
    OutOfUsage { reset_at_ms: Option<u64> },
    /// Trust, login or permission gate: `code` is the gate code, never answered.
    Gate { code: String, sentence: String },
    /// The agent asked a question. `question` is redacted and at most 400 characters.
    Asked { question: String },
    NeedsYou { why: String, sentence: String },
    Failed { code: String, sentence: String },
    /// The hard stop time came first.
    CutOff,
    StoppedByPerson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub n: u8,
    pub plan: AgentChoice,
    /// The plan's label for people, taken from the registry. Data, never a code name.
    pub label: String,
    pub kind: AttemptKind,
    pub mission_id: Option<String>,
    /// "mission-<id>" or the run handle.
    pub handle: Option<String>,
    pub pane_ids: Vec<String>,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub end: Option<AttemptEnd>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Ready,
    NoChecks,
    NeedsLook,
    NoChanges,
    DidntRun,
}

/// Test counts: passed of total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub passed: u32,
    pub total: u32,
}

/// Where the separate copy is. Used by Keep and Throw away (NS-11 and later tasks).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyRef {
    pub project: String,
    pub handle: String,
    pub branch: String,
    pub base: String,
    pub pane_id: Option<String>,
}

/// The result of one task, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub class: Class,
    pub headline: String,
    /// What to do next, one line. None when nothing is needed.
    pub reason: Option<String>,
    pub tests: Option<Counts>,
    pub tests_before: Option<Counts>,
    pub checks_declared: bool,
    pub test_change_lines: Vec<String>,
    /// Files, added, deleted.
    pub files: Option<(u32, u32, u32)>,
    pub turns: u32,
    pub minutes: u32,
    /// Turn sentences, redacted, at most 8.
    pub summaries: Vec<String>,
    /// The attempt that holds the result.
    pub copy: Option<CopyRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "review", rename_all = "snake_case")]
pub enum Review {
    Pending,
    Kept { at_ms: u64, into: String },
    Discarded { at_ms: u64 },
    KeepFailed { at_ms: u64, sentence: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub spec: TaskSpec,
    /// Declared argv snapshot taken at Start. Empty means no tests were declared.
    pub checks: Vec<Vec<String>>,
    pub state: TaskState,
    pub attempts: Vec<Attempt>,
    pub outcome: Option<Outcome>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Scheduled { start_at_ms: u64 },
    Running,
    Done,
    /// The person pressed Stop.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanOut {
    pub plan_key: String,
    pub until_ms: Option<u64>,
    pub since_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocked {
    pub plan_key: String,
    pub project: String,
    pub sentence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub at_ms: u64,
    pub kind: String,
    pub glyph: String,
    pub text: String,
    pub task: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NightRun {
    /// "ns-" + 12 lowercase hex.
    pub id: String,
    pub created_ms: u64,
    pub start_at_ms: u64,
    pub stop_at_ms: u64,
    pub state: RunState,
    pub plans: Vec<PlanChoice>,
    pub parallel: u8,
    pub keep_awake: bool,
    pub awake_ok: bool,
    pub tasks: Vec<Task>,
    pub plan_out: Vec<PlanOut>,
    pub blocked: Vec<Blocked>,
    /// Cap 200.
    pub events: Vec<Event>,
    pub last_tick_ms: u64,
    pub finished_ms: Option<u64>,
    pub seen_ms: Option<u64>,
    pub push_sent: bool,
    pub rev: u64,
}
