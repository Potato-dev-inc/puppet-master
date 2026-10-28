//! Night Shift data model (SPEC n32 section 3.2). Pure data: serde types, constants and small helpers.
//! No I/O here. The files live in `store.rs`.

use crate::missions::model::AgentChoice;
use serde::{Deserialize, Serialize};

pub const MAX_TASKS: usize = 10;
pub const MAX_TASK_TEXT: usize = 2000; // = missions::model::MAX_OBJECTIVE
pub const MAX_PARALLEL: u8 = 2;
pub const MAX_ATTEMPTS: u8 = 2;
pub const DEFAULT_TURNS: u32 = 12;
pub const DEFAULT_MINUTES: u32 = 90;
pub const MIN_TURNS: u32 = 3;
pub const MAX_TURNS: u32 = 40;
pub const MIN_MINUTES: u32 = 20;
pub const MAX_MINUTES: u32 = 240;
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
pub const EVENTS_CAP: usize = 200;
pub const DEFAULT_STOP_HHMM: &str = "07:00";
pub const MAX_SETTINGS_PLANS: usize = 16;

/// `agent` or `agent@account`: the same plan key the handoff code uses.
pub fn plan_key(a: &AgentChoice) -> String {
    match a.account.as_deref().map(str::trim).filter(|account| !account.is_empty()) {
        Some(account) => format!("{}@{}", a.agent_type, account),
        None => a.agent_type.clone(),
    }
}

/// `ns-` + 12 lowercase hex characters: a run id. The only text that becomes a file name.
pub fn valid_run_id(id: &str) -> bool {
    has_hex_id(id, "ns-", 12)
}

/// `nt-` + 8 lowercase hex characters: a task id.
pub fn valid_task_id(id: &str) -> bool {
    has_hex_id(id, "nt-", 8)
}

fn has_hex_id(id: &str, prefix: &str, len: usize) -> bool {
    match id.strip_prefix(prefix) {
        Some(hex) => hex.len() == len && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}

/// Refuse a run whose own id or any task id is malformed, or whose task ids repeat.
pub fn validate_ids(run: &NightRun) -> Result<(), String> {
    if !valid_run_id(&run.id) {
        return Err("bad night run id".to_string());
    }
    for (index, task) in run.tasks.iter().enumerate() {
        if !valid_task_id(&task.spec.id) {
            return Err("bad night task id".to_string());
        }
        if run.tasks[..index].iter().any(|other| other.spec.id == task.spec.id) {
            return Err("duplicate night task id".to_string());
        }
    }
    Ok(())
}

/// `HH:MM` on a 24-hour clock. Anything else is refused.
pub fn valid_hhmm(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 5 || b[2] != b':' || ![b[0], b[1], b[3], b[4]].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let hour = (b[0] - b'0') as u32 * 10 + (b[1] - b'0') as u32;
    let minute = (b[3] - b'0') as u32 * 10 + (b[4] - b'0') as u32;
    hour < 24 && minute < 60
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "who", rename_all = "snake_case")]
pub enum Who {
    Any,
    Fixed { agent: AgentChoice },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,              // "nt-" + 8 lowercase hex
    pub project: String,         // origin project path (normalised; same key operations/missions use)
    pub text: String,            // the person's words, verbatim, <= 2000 chars (local only)
    pub who: Who,
    pub may_change_tests: bool,  // default false
    pub turns: u32,
    pub minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanChoice {
    pub agent: AgentChoice,
    pub label: String, // the plan's name as the person reads it (from the registry, never typed here)
    pub vendor: Option<String>,
}

/// Waiting = no plan with usage or plan out; Working = an attempt is open.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    #[default]
    Queued,
    Waiting { until_ms: Option<u64>, why: String },
    Working,
    Finished,
}

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
    pub mission_id: Option<String>, // mission id, or None for a single turn
    pub handle: Option<String>,     // "mission-<id>" or the run handle
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

/// Tests when parsed, else checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub passed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub class: Class,
    pub headline: String, // plain words, see SPEC 3.6
    pub reason: Option<String>, // why it needs a look or did not run (plain words)
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
pub struct CopyRef {
    pub project: String,
    pub handle: String,
    pub branch: String,
    pub base: String,
    pub pane_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "review", rename_all = "snake_case")]
pub enum Review {
    #[default]
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
    Stopped, // the person pressed Stop
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

/// `kind`: started | task_started | task_done | out_of_usage | moved | waiting | blocked | asked
/// | cut_off | slept | stopped | finished | awake_failed | note. `glyph`: one of the five UX 0.1 glyphs.
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
    pub events: Vec<Event>, // cap EVENTS_CAP
    pub last_tick_ms: u64,
    pub finished_ms: Option<u64>,
    pub seen_ms: Option<u64>,
    pub push_sent: bool,
    pub rev: u64,
}

/// Counts for the summary line (SPEC 3.6): D = done (Ready + NoChecks), G = ready, L = needs look,
/// X = didn't run, Z = no changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SummaryCounts {
    pub total: u32,
    pub done: u32,
    pub ready: u32,
    pub needs_look: u32,
    pub didnt_run: u32,
    pub no_changes: u32,
}

impl NightRun {
    /// A new run id: `ns-` + 12 random lowercase hex characters.
    pub fn new_id() -> String {
        format!("ns-{}", random_hex(12))
    }

    /// A new task id: `nt-` + 8 random lowercase hex characters.
    pub fn task_id() -> String {
        format!("nt-{}", random_hex(8))
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.spec.id == id)
    }

    /// Every attempt that has not ended yet, in list order.
    pub fn open_attempts(&self) -> Vec<&Attempt> {
        self.tasks
            .iter()
            .flat_map(|task| task.attempts.iter())
            .filter(|attempt| attempt.end.is_none())
            .collect()
    }

    pub fn summary_counts(&self) -> SummaryCounts {
        let mut counts = SummaryCounts { total: self.tasks.len() as u32, ..SummaryCounts::default() };
        for task in &self.tasks {
            let Some(outcome) = &task.outcome else { continue };
            match outcome.class {
                Class::Ready => {
                    counts.done += 1;
                    counts.ready += 1;
                }
                Class::NoChecks => counts.done += 1,
                Class::NeedsLook => counts.needs_look += 1,
                Class::DidntRun => counts.didnt_run += 1,
                Class::NoChanges => counts.no_changes += 1,
            }
        }
        counts
    }
}

/// The Tonight settings (`settings.json`). Missing fields read as the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub plans: Vec<String>,
    pub stop_hhmm: String,
    pub parallel: u8,
    pub turns: u32,
    pub minutes: u32,
    pub awake_asked: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            plans: Vec::new(),
            stop_hhmm: DEFAULT_STOP_HHMM.to_string(),
            parallel: MAX_PARALLEL,
            turns: DEFAULT_TURNS,
            minutes: DEFAULT_MINUTES,
            awake_asked: false,
        }
    }
}

impl Settings {
    /// The same settings with every value in range: parallel 1-2, turns 3-40, minutes 20-240, stop time
    /// `HH:MM` (else 07:00), plans trimmed, non-empty, unique and at most 16.
    pub fn clamped(&self) -> Settings {
        let mut plans: Vec<String> = Vec::new();
        for plan in &self.plans {
            let plan = plan.trim();
            if !plan.is_empty() && !plans.iter().any(|kept| kept == plan) && plans.len() < MAX_SETTINGS_PLANS {
                plans.push(plan.to_string());
            }
        }
        Settings {
            plans,
            stop_hhmm: if valid_hhmm(&self.stop_hhmm) { self.stop_hhmm.clone() } else { DEFAULT_STOP_HHMM.to_string() },
            parallel: self.parallel.clamp(1, MAX_PARALLEL),
            turns: self.turns.clamp(MIN_TURNS, MAX_TURNS),
            minutes: self.minutes.clamp(MIN_MINUTES, MAX_MINUTES),
            awake_asked: self.awake_asked,
        }
    }
}

fn random_hex(len: usize) -> String {
    uuid::Uuid::new_v4().simple().to_string()[..len].to_string()
}

#[cfg(test)]
pub fn sample_task(id: &str) -> Task {
    Task {
        spec: TaskSpec {
            id: id.to_string(),
            project: "/work/app".to_string(),
            text: "Fix the cart total rounding.".to_string(),
            who: Who::Any,
            may_change_tests: false,
            turns: DEFAULT_TURNS,
            minutes: DEFAULT_MINUTES,
        },
        checks: Vec::new(),
        state: TaskState::Queued,
        attempts: Vec::new(),
        outcome: None,
        review: Review::Pending,
    }
}

#[cfg(test)]
pub fn sample_run(id: &str) -> NightRun {
    NightRun {
        id: id.to_string(),
        created_ms: 1_000,
        start_at_ms: 1_000,
        stop_at_ms: 2_000_000,
        state: RunState::Running,
        plans: Vec::new(),
        parallel: 1,
        keep_awake: false,
        awake_ok: false,
        tasks: vec![sample_task("nt-00000001")],
        plan_out: Vec::new(),
        blocked: Vec::new(),
        events: Vec::new(),
        last_tick_ms: 1_000,
        finished_ms: None,
        seen_ms: None,
        push_sent: false,
        rev: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("serialize")
    }

    fn outcome(class: Class) -> Outcome {
        Outcome {
            class,
            headline: String::new(),
            reason: None,
            tests: None,
            tests_before: None,
            checks_declared: false,
            test_change_lines: Vec::new(),
            files: None,
            turns: 0,
            minutes: 0,
            summaries: Vec::new(),
            copy: None,
        }
    }

    fn agent(name: &str, account: Option<&str>) -> AgentChoice {
        AgentChoice { agent_type: name.to_string(), account: account.map(str::to_string), model: None }
    }

    #[test]
    fn task_state_tags_are_stable() {
        assert_eq!(json(&TaskState::Queued), r#""queued""#);
        assert_eq!(
            json(&TaskState::Waiting { until_ms: Some(5), why: "no plan".to_string() }),
            r#"{"waiting":{"until_ms":5,"why":"no plan"}}"#
        );
        assert_eq!(json(&TaskState::Working), r#""working""#);
        assert_eq!(json(&TaskState::Finished), r#""finished""#);
    }

    #[test]
    fn attempt_end_tags_are_stable_and_round_trip() {
        assert_eq!(json(&AttemptEnd::Completed), r#"{"end":"completed"}"#);
        assert_eq!(
            json(&AttemptEnd::OutOfUsage { reset_at_ms: None }),
            r#"{"end":"out_of_usage","reset_at_ms":null}"#
        );
        assert_eq!(
            json(&AttemptEnd::Gate { code: "WORKSPACE_TRUST_DECLINED".into(), sentence: "Asked to trust.".into() }),
            r#"{"end":"gate","code":"WORKSPACE_TRUST_DECLINED","sentence":"Asked to trust."}"#
        );
        assert_eq!(json(&AttemptEnd::CutOff), r#"{"end":"cut_off"}"#);
        assert_eq!(json(&AttemptEnd::StoppedByPerson), r#"{"end":"stopped_by_person"}"#);
        let all = vec![
            AttemptEnd::Completed,
            AttemptEnd::OutOfUsage { reset_at_ms: Some(9) },
            AttemptEnd::Asked { question: "q".into() },
            AttemptEnd::NeedsYou { why: "stuck".into(), sentence: "s".into() },
            AttemptEnd::Failed { code: "X".into(), sentence: "s".into() },
            AttemptEnd::CutOff,
            AttemptEnd::StoppedByPerson,
        ];
        for end in all {
            let back: AttemptEnd = serde_json::from_str(&json(&end)).expect("parses back");
            assert_eq!(back, end);
        }
    }

    #[test]
    fn review_tags_are_stable() {
        assert_eq!(json(&Review::Pending), r#"{"review":"pending"}"#);
        assert_eq!(
            json(&Review::Kept { at_ms: 9, into: "main".into() }),
            r#"{"review":"kept","at_ms":9,"into":"main"}"#
        );
        assert_eq!(json(&Review::Discarded { at_ms: 9 }), r#"{"review":"discarded","at_ms":9}"#);
        assert_eq!(
            json(&Review::KeepFailed { at_ms: 9, sentence: "conflict".into() }),
            r#"{"review":"keep_failed","at_ms":9,"sentence":"conflict"}"#
        );
    }

    #[test]
    fn run_state_tags_are_stable() {
        assert_eq!(json(&RunState::Scheduled { start_at_ms: 7 }), r#"{"scheduled":{"start_at_ms":7}}"#);
        assert_eq!(json(&RunState::Running), r#""running""#);
        assert_eq!(json(&RunState::Done), r#""done""#);
        assert_eq!(json(&RunState::Stopped), r#""stopped""#);
    }

    #[test]
    fn who_and_class_tags_are_stable() {
        assert_eq!(json(&Who::Any), r#"{"who":"any"}"#);
        assert_eq!(
            json(&Who::Fixed { agent: agent("codex", None) }),
            r#"{"who":"fixed","agent":{"agent_type":"codex"}}"#
        );
        assert_eq!(json(&Class::NeedsLook), r#""needs_look""#);
        assert_eq!(json(&Class::DidntRun), r#""didnt_run""#);
    }

    #[test]
    fn plan_key_is_agent_or_agent_at_account() {
        assert_eq!(plan_key(&agent("codex", None)), "codex");
        assert_eq!(plan_key(&agent("codex", Some("work"))), "codex@work");
        assert_eq!(plan_key(&agent("codex", Some("  "))), "codex");
    }

    #[test]
    fn run_and_task_ids_are_validated() {
        assert!(valid_run_id("ns-0123456789ab"));
        for bad in ["ns-0123456789AB", "ns-0123456789a", "ns-0123456789abc", "nt-0123456789ab", "ns-../../etc"] {
            assert!(!valid_run_id(bad), "{bad:?} must be refused");
        }
        assert!(valid_task_id("nt-0123abcd"));
        for bad in ["nt-0123abcdef", "nt-0123ABCD", "ns-0123abcd", "nt-..\\x"] {
            assert!(!valid_task_id(bad), "{bad:?} must be refused");
        }
        assert!(valid_run_id(&NightRun::new_id()));
        assert!(valid_task_id(&NightRun::task_id()));
    }

    #[test]
    fn validate_ids_refuses_bad_and_repeated_task_ids() {
        let mut run = sample_run("ns-0123456789ab");
        assert!(validate_ids(&run).is_ok());
        run.id = "ns-../x".to_string();
        assert!(validate_ids(&run).is_err());
        run.id = "ns-0123456789ab".to_string();
        run.tasks[0].spec.id = "nt-bad".to_string();
        assert!(validate_ids(&run).is_err());
        run.tasks[0].spec.id = "nt-00000001".to_string();
        run.tasks.push(sample_task("nt-00000001"));
        assert_eq!(validate_ids(&run), Err("duplicate night task id".to_string()));
    }

    #[test]
    fn hhmm_must_be_a_24_hour_clock_time() {
        assert!(valid_hhmm("07:00"));
        assert!(valid_hhmm("23:59"));
        for bad in ["24:00", "07:60", "7:00", "0700", "07-00", "ab:cd", "07:0x"] {
            assert!(!valid_hhmm(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn settings_clamp_into_range() {
        let wild = Settings {
            plans: vec![" codex ".into(), "codex".into(), "".into(), "claude".into()],
            stop_hhmm: "25:00".into(),
            parallel: 9,
            turns: 99,
            minutes: 1,
            awake_asked: true,
        };
        let clean = wild.clamped();
        assert_eq!(clean.plans, vec!["codex".to_string(), "claude".to_string()]);
        assert_eq!(clean.stop_hhmm, "07:00");
        assert_eq!(clean.parallel, 2);
        assert_eq!(clean.turns, 40);
        assert_eq!(clean.minutes, 20);
        assert!(clean.awake_asked);
        let low = Settings { parallel: 0, turns: 0, minutes: 999, ..Settings::default() }.clamped();
        assert_eq!((low.parallel, low.turns, low.minutes), (1, 3, 240));
    }

    #[test]
    fn open_attempts_and_summary_counts_follow_the_outcomes() {
        let mut run = sample_run("ns-0123456789ab");
        let attempt = Attempt {
            n: 1,
            plan: agent("codex", None),
            label: "Codex".to_string(),
            kind: AttemptKind::Single,
            mission_id: None,
            handle: Some("run-1".to_string()),
            pane_ids: Vec::new(),
            started_ms: 1,
            ended_ms: None,
            end: None,
        };
        run.tasks[0].attempts.push(attempt.clone());
        let mut ended = attempt.clone();
        ended.end = Some(AttemptEnd::Completed);
        run.tasks[0].attempts.push(ended);
        assert_eq!(run.open_attempts().len(), 1);

        let classes = [Class::Ready, Class::NoChecks, Class::NeedsLook, Class::DidntRun, Class::NoChanges];
        run.tasks = classes
            .iter()
            .enumerate()
            .map(|(i, class)| {
                let mut task = sample_task(&format!("nt-0000000{}", i + 1));
                task.outcome = Some(outcome(*class));
                task
            })
            .collect();
        run.tasks.push(sample_task("nt-00000009"));
        assert_eq!(
            run.summary_counts(),
            SummaryCounts { total: 6, done: 2, ready: 1, needs_look: 1, didnt_run: 1, no_changes: 1 }
        );
    }
}
