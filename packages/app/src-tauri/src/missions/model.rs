//! Mission types (DESIGN-MISSIONS 2.1, 2.5). Pure data: what is stored in `mission.json`.

use crate::agent_runs::checks::counts::TestCounts;
use crate::agent_runs::checks::Check;
use crate::worktrees::WorktreeRef;
use serde::{Deserialize, Serialize};

/// Mission handles start with this; the budget key and the checkpoint key are the handle too.
pub const HANDLE_PREFIX: &str = "mission-";
/// Rounds in a row without progress before the agent is considered stuck (3..=10).
pub const DEFAULT_STALL: u32 = 3;
/// Most missions working at once, app-wide, while missions are a Preview.
pub const MAX_ACTIVE: usize = 2;
pub const MAX_OBJECTIVE: usize = 2000;
pub const MAX_BACKUPS: usize = 4;
/// Default and largest time a turn may take before it is stopped.
pub const DEFAULT_ROUND_TIMEOUT_MS: u64 = 30 * 60_000;
pub const DEFAULT_CHECK_TIMEOUT_MS: u64 = 5 * 60_000;

/// One check of the mission, with the name a person reads ("Tests", "Type check").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionCheck {
    pub name: String,
    pub check: Check,
}

impl MissionCheck {
    pub fn argv(&self) -> Option<&[String]> {
        match &self.check {
            Check::CommandExits { command, .. } => Some(command),
            _ => None,
        }
    }

    /// The command as a person would type it, or the predicate in words.
    pub fn display(&self) -> String {
        match &self.check {
            Check::CommandExits { command, .. } => crate::worktrees::preset::display(command),
            Check::FileExists { path } => format!("{path} exists"),
            Check::FileAbsent { path } => format!("{path} is gone"),
            Check::FileContains { path, .. } => format!("{path} contains the text"),
            Check::ResultContains { .. } => "the reply contains the text".into(),
        }
    }
}

/// An agent of the mission: `[0]` starts, the rest are backups, one at a time in this order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChoice {
    pub agent_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl AgentChoice {
    /// `agent_type` or `agent_type@account`.
    pub fn parse(text: &str) -> Option<AgentChoice> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (agent, account) = match text.split_once('@') {
            Some((a, b)) => (a.trim(), Some(b.trim().to_string()).filter(|s| !s.is_empty())),
            None => (text, None),
        };
        (!agent.is_empty()).then(|| AgentChoice { agent_type: agent.to_string(), account, model: None })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    /// Making the copy, the first check run, the test snapshot.
    Planning,
    /// A turn is open on one agent.
    Working,
    /// The turn ended; checks and the test-change scan run.
    Checking,
    /// Every check passes and no blocking test change is left (or the person accepted it).
    Green,
    /// Waiting for the person: see `Mission::needs`.
    NeedsYou,
    Paused,
    /// The person took it over ("Hand it to me").
    HandedOff,
    Stopped,
    /// Something Puppet Master can't recover from (the copy vanished, the record is unreadable).
    Failed,
}

impl MissionState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Green | Self::HandedOff | Self::Stopped | Self::Failed)
    }

    /// Counts toward the "two missions at once" limit.
    pub fn active(self) -> bool {
        !self.terminal()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Working => "working",
            Self::Checking => "checking",
            Self::Green => "green",
            Self::NeedsYou => "needs_you",
            Self::Paused => "paused",
            Self::HandedOff => "handed_off",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// What the person is asked to decide while the mission is `needs_you`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Checks pass but tests or check config changed.
    Tamper,
    /// The budget is used up.
    Budget,
    /// The checks or the agent can't start.
    Setup,
    /// No backup left to try.
    Stuck,
}

impl Needs {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tamper => "tamper",
            Self::Budget => "budget",
            Self::Setup => "setup",
            Self::Stuck => "stuck",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandOffWhy {
    NoProgress,
    Exited,
    /// Two rollbacks in a row: the agent keeps making it worse.
    Regressed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoundKind {
    First,
    FollowUp,
    HandOff { from: usize, why: HandOffWhy },
    /// Same agent, new conversation: its pane died (exit, restart), or the list wrapped.
    Resume { after_restart: bool },
    AfterRollback { to_round: u32 },
    /// The person undid the test changes: the next turn is told.
    AfterTestUndo,
    /// "Try 5 more turns": a fresh start with the first agent.
    Retry,
}

impl RoundKind {
    /// A round that continues the previous agent's conversation (same handle).
    pub fn same_handle(&self) -> bool {
        matches!(self, Self::FollowUp | Self::AfterRollback { .. } | Self::AfterTestUndo)
    }
}

/// How the round's operation ended, as the engine read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RoundEnd {
    /// `completed` | `failed` | `cancelled`, or `not_started` when the round could not be created.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

impl RoundEnd {
    pub fn code_is(&self, codes: &[&str]) -> bool {
        self.code.as_deref().is_some_and(|c| codes.contains(&c))
    }
}

/// One check's result after a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutcome {
    pub name: String,
    pub passed: bool,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<TestCounts>,
    /// The program of the check could not be started (not installed, not on PATH).
    #[serde(default)]
    pub not_started: bool,
    /// Last lines of the output when it failed (raw: fenced before it goes into a prompt).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tail: String,
}

/// What the test-change guard found, cumulative against the start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TestChanges {
    pub blocks: bool,
    #[serde(default)]
    pub findings: Vec<crate::tamper::Finding>,
}

impl TestChanges {
    pub fn blocking_sentences(&self) -> Vec<String> {
        self.findings
            .iter()
            .filter(|f| f.severity == crate::tamper::Severity::Blocks)
            .map(|f| f.summary.clone())
            .collect()
    }
}

/// The state of the copy after a turn (or at the start, round 0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Verdict {
    pub checks: Vec<CheckOutcome>,
    /// Summed over the checks that reported counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<TestCounts>,
    pub checks_passed: u32,
    pub checks_total: u32,
    #[serde(default)]
    pub test_changes: TestChanges,
    /// The git tree of the copy's files (a mission checkpoint): equal trees mean no file changed.
    #[serde(default)]
    pub tree: String,
    /// The mission checkpoint id of these files (the rollback target).
    #[serde(default)]
    pub checkpoint: String,
    /// The failing output, normalised, to tell "the same failure" from "a new one".
    #[serde(default)]
    pub failing_key: String,
}

impl Verdict {
    pub fn all_pass(&self) -> bool {
        self.checks_total > 0 && self.checks_passed == self.checks_total
    }

    /// A check could not even start (program missing): nothing an agent turn can fix.
    pub fn cant_run(&self) -> Option<&CheckOutcome> {
        self.checks.iter().find(|c| c.not_started)
    }

    /// `(checks passed, tests passed)`: how good the files are, for "best" and "worse".
    pub fn score(&self) -> (u32, u32) {
        (self.checks_passed, self.counts.map(|c| c.passed).unwrap_or(0))
    }

    /// "9 of 12" (tests) or "1 of 2 checks".
    pub fn short(&self) -> String {
        match self.counts.filter(|c| c.total > 0) {
            Some(c) => format!("{} of {}", c.passed, c.total),
            None => format!("{} of {} checks", self.checks_passed, self.checks_total),
        }
    }
}

/// What the engine decided after a turn, with the reason in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Stop,
    Pause,
    Green,
    NeedsYou { why: Needs },
    FollowUp,
    /// Same agent, new conversation (its pane is gone).
    Resume { after_restart: bool },
    HandOff { to: usize, why: HandOffWhy },
    /// Put the files back to round `to_round`'s (0 = the start), then do `next`.
    Rollback { to_round: u32, next: Box<Action> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    #[serde(flatten)]
    pub action: Action,
    /// The timeline line (DESIGN-MISSIONS 3.2, exact copy).
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    /// 1-based: the "Turn 7" a person reads.
    pub n: u32,
    /// Index into `Mission::agents`.
    pub agent: usize,
    #[serde(flatten)]
    pub kind: RoundKind,
    /// Saved before the turn is started; finds the turn again after a restart.
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<RoundEnd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    /// A person edited files in the copy between the last turn and this one.
    #[serde(default)]
    pub outside_edit: bool,
}

/// A timeline row that is not a turn (started, paused, handed off, went back, accepted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub at_ms: u64,
    /// `started` | `paused` | `resumed` | `restarted` | `handed_off` | `rollback` | `budget` |
    /// `tests_accepted` | `tests_undone` | `stopped` | `stuck` | `setup` | `done` | `note`.
    pub kind: String,
    /// One of the five glyphs (UX 0.1): `~` working, `+` needs you, `v` done, `:` paused, `x` ended.
    pub glyph: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
}

/// Who started it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Starter {
    Desktop,
    Agent { session: Option<String>, label: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Roster {
    /// Index of the agent working now.
    pub current: usize,
    /// Agents that are out for this mission (stuck, exited twice).
    pub out: Vec<usize>,
    /// The list went round once already (the first agent got a fresh try).
    pub wrapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accepted {
    pub at_ms: u64,
    pub by: String,
    pub summary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    /// The origin project (the key for operations, budgets and this store).
    pub project: String,
    pub objective: String,
    pub checks: Vec<MissionCheck>,
    pub agents: Vec<AgentChoice>,
    #[serde(default)]
    pub may_change_tests: bool,
    pub rollback: bool,
    pub budget_turns: Option<u32>,
    pub budget_minutes: Option<u32>,
    /// The orchestrator's own budget the mission draws from, when it was started by an agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_parent: Option<String>,
    pub round_timeout_ms: u64,
    pub check_timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeRef>,
    pub state: MissionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<Needs>,
    /// The card's first sentence while `needs_you` (and the sentence of `failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_from: Option<MissionState>,
    #[serde(default)]
    pub pause_requested: bool,
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// The checks at the start (round 0) and the start checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Verdict>,
    #[serde(default)]
    pub roster: Roster,
    /// The files' tree when the next turn starts, as the mission left them (after the last
    /// verdict, a rollback or an undo). A different tree means someone edited the copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_tree: Option<String>,
    /// Files a person changed in the copy between turns: test changes there are notes about them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outside_paths: Vec<String>,
    #[serde(default)]
    pub events: Vec<Event>,
    pub started_by: Starter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_accepted: Option<Accepted>,
    /// The caller's key for an identical retry of the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    pub rev: u64,
}

impl Mission {
    /// `mission-<id>`: the MCP handle, the budget key and the checkpoint key.
    pub fn handle(&self) -> String {
        format!("{HANDLE_PREFIX}{}", self.id)
    }

    pub fn last_round(&self) -> Option<&Round> {
        self.rounds.last()
    }

    pub fn agent_label(&self, index: usize) -> String {
        self.agents.get(index).map(|a| agent_label(&a.agent_type)).unwrap_or_else(|| "The agent".into())
    }

    /// The newest verdict (the start when no turn has one yet).
    pub fn latest_verdict(&self) -> Option<&Verdict> {
        self.rounds.iter().rev().find_map(|r| r.verdict.as_ref()).or(self.baseline.as_ref())
    }

    /// Turns spent: rounds whose turn was created.
    pub fn turns_used(&self) -> u32 {
        self.rounds.iter().filter(|r| r.operation_id.is_some()).count() as u32
    }

    pub fn push_event(&mut self, at_ms: u64, kind: &str, glyph: &str, text: impl Into<String>, round: Option<u32>) {
        self.events.push(Event { at_ms, kind: kind.into(), glyph: glyph.into(), text: text.into(), round });
    }
}

/// The name a person reads for an agent type ("Claude Code"); never the raw id when a label exists.
/// Labels are looked up once a minute at most: the timeline and the ledger ask for them per row.
pub fn agent_label(agent_type: &str) -> String {
    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    static CACHE: Lazy<Mutex<(Option<Instant>, HashMap<String, String>)>> = Lazy::new(|| Mutex::new((None, HashMap::new())));
    let mut cache = CACHE.lock();
    if cache.0.is_none_or(|at| at.elapsed() > Duration::from_secs(60)) {
        cache.1 = crate::tui::descriptors()
            .into_iter()
            .filter(|d| !d.label.is_empty())
            .map(|d| (d.id.to_string(), d.label))
            .collect();
        cache.0 = Some(Instant::now());
    }
    cache.1.get(agent_type).cloned().unwrap_or_else(|| agent_type.to_string())
}
