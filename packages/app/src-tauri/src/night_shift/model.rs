//! Night Shift data model (spec 3.2). Pure data with serde; no I/O.
//! Only the types the scheduler needs live on this branch. NS-1 owns the id helpers and the store.

use serde::{Deserialize, Serialize};

use crate::missions::model::AgentChoice;

pub const MAX_TASKS: usize = 10;
pub const MAX_TASK_TEXT: usize = 2000; // = missions::model::MAX_OBJECTIVE
pub const MAX_PARALLEL: u8 = 2;
pub const MAX_ATTEMPTS: u8 = 2;
pub const DEFAULT_TURNS: u32 = 12;
pub const DEFAULT_MINUTES: u32 = 90;
pub const ROUND_TIMEOUT_MS: u64 = 30 * 60_000;
pub const CHECK_TIMEOUT_MS: u64 = 10 * 60_000;
pub const MIN_RUN_MS: u64 = 30 * 60_000;
pub const MAX_RUN_MS: u64 = 14 * 3_600_000;
pub const MIN_START_LEFT_MS: u64 = 25 * 60_000;
pub const LIMIT_GRACE_MS: u64 = 3 * 60_000; // a parked limit must hold this long before it ends a try
pub const SELF_RESUME_SLACK_MS: u64 = 10 * 60_000;
pub const ASK_GRACE_MS: u64 = 5 * 60_000;
pub const NO_RESET_WAIT_MS: u64 = 60 * 60_000; // = handoff::card::REMIND_FALLBACK_MS
pub const TICK_MS: u64 = 5_000;
pub const HISTORY_CAP: usize = 14;
pub const SLEEP_GAP_MS: u64 = 5 * 60_000; // a tick gap this long is reported as "the computer slept"

/// Plan key: `agent` or `agent@account`. Two logins of one agent are two plans.
pub fn plan_key(a: &AgentChoice) -> String {
    match a.account.as_deref() {
        Some(account) if !account.is_empty() => format!("{}@{}", a.agent_type, account),
        _ => a.agent_type.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "who", rename_all = "snake_case")]
pub enum Who {
    Any,
    Fixed { agent: AgentChoice },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,             // "nt-" + 8 lowercase hex
    pub project: String,        // origin project path (normalised; same key operations/missions use)
    pub text: String,           // the person's words, verbatim, <= 2000 chars (local only)
    pub who: Who,
    pub may_change_tests: bool, // default false
    pub turns: u32,
    pub minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanChoice {
    pub agent: AgentChoice,
    pub label: String,         // the name shown, from the plan (agent plus login, when two logins)
    pub vendor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    #[default]
    Queued,
    Waiting { until_ms: Option<u64>, why: String },
    Working,
    Finished,
}
// Waiting = no plan with usage / plan out; Working = an attempt is open.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    Mission,
    Single,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "end", rename_all = "snake_case")]
pub enum AttemptEnd {
    Completed,
    OutOfUsage { reset_at_ms: Option<u64> },
    Gate { code: String, sentence: String }, // trust / login / permission; plain words
    Asked { question: String },              // redacted, <= 400 chars
    NeedsYou { why: String, sentence: String }, // mission Needs::{Tamper,Stuck,Budget,Setup} as text
    Failed { code: String, sentence: String },
    CutOff, // hard stop
    StoppedByPerson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub n: u8,
    pub plan: AgentChoice,
    pub label: String,
    pub kind: AttemptKind,
    pub mission_id: Option<String>, // "mission-<id>" or the run handle
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub passed: u32,
    pub total: u32, // tests when parsed, else checks
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyRef {
    pub project: String,
    pub handle: String,
    pub branch: String,
    pub base: String,
    pub pane_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub class: Class,
    pub headline: String, // plain words, see 3.6
    pub reason: Option<String>,
    pub tests: Option<Counts>,
    pub tests_before: Option<Counts>,
    pub checks_declared: bool,
    pub test_change_lines: Vec<String>, // from missions::cards::tamper_lines
    pub files: Option<(u32, u32, u32)>, // files, added, deleted
    pub turns: u32,
    pub minutes: u32,
    pub summaries: Vec<String>, // turn sentences, redacted, <= 8
    pub copy: Option<CopyRef>,  // the attempt that holds the result
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
    pub checks: Vec<Vec<String>>, // declared argv snapshot taken at Start (empty = none declared)
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
    Stopped, // person pressed Stop
}

/// A plan that hit its usage limit during this run, until its reset (or the fallback wait).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanOut {
    pub plan_key: String,
    pub until_ms: Option<u64>,
    pub since_ms: u64,
}

/// A plan that hit a gate on one project; the pair is skipped for the rest of the night.
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
    pub id: String, // "ns-" + 12 lowercase hex
    pub created_ms: u64,
    pub start_at_ms: u64,
    pub stop_at_ms: u64,
    pub state: RunState,
    pub plans: Vec<PlanChoice>, // ticked plans, the person's order
    pub parallel: u8,
    pub keep_awake: bool,
    pub awake_ok: bool, // asked / holding
    pub tasks: Vec<Task>,
    pub plan_out: Vec<PlanOut>,
    pub blocked: Vec<Blocked>,
    pub events: Vec<Event>, // cap 200
    pub last_tick_ms: u64,
    pub finished_ms: Option<u64>,
    pub seen_ms: Option<u64>,
    pub push_sent: bool,
    pub rev: u64,
}
