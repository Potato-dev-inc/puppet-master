//! Keep going: everything the hand-over touches outside itself, behind one trait, so
//! `engine_tests.rs` can run the whole flow without an app (SPEC n32 section 3.5).
//!
//! The live implementation is `live_app::AppIo`. A fake one in the tests records the order of
//! the calls (the old run ends before the old agent is stopped, the old agent is gone before the
//! new one starts). Runs and cards still go through the real `operations` and `questions`
//! code in tests, against a temp folder and a private store.

use super::brief::{DiffStat, TurnLine};
use super::model::{Candidate, Handover, Target};
use super::store::Store;
use crate::audit::Who;
use crate::handoff::reset::Clock;
use crate::operations::{OperationError, OperationSnapshot};
use crate::plans_report::limit_log::Entry;
use crate::pty::agent_stop::StopError;
use crate::questions::QuestionStore;
use crate::worktrees::WorktreeRef;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

/// A pane as the hand-over needs to know it.
#[derive(Debug, Clone)]
pub struct PaneFacts {
    pub pane_id: String,
    pub agent_id: String,
    pub account: Option<String>,
    /// The agent's name ("Agent A").
    pub agent_label: String,
    /// The account's label, when the pane runs under a saved login.
    pub account_label: Option<String>,
    /// The pane's own working directory.
    pub cwd: String,
    /// The pane's folder as a run project key (normalized `cwd`).
    pub project: String,
    pub status: String,
    /// A companion's pane (it has "Wake on..." instead).
    pub companion: bool,
    /// A mission round's pane (missions have their own backup agents).
    pub mission: bool,
    /// The separate copy the pane works in, when it does.
    pub worktree: Option<WorktreeRef>,
}

impl PaneFacts {
    /// "Agent A" or "Agent A · Work".
    pub fn label(&self) -> String {
        label_with(&self.agent_label, self.account_label.as_deref())
    }

    /// Where the pane's runs are stored: the origin repo of its copy, else its own folder.
    pub fn run_projects(&self) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(copy) = &self.worktree {
            found.push(copy.repo.clone());
        }
        if !found.contains(&self.project) {
            found.push(self.project.clone());
        }
        found
    }
}

/// "Agent A" or "Agent A · Work" (the account only when one is set).
pub fn label_with(agent_label: &str, account_label: Option<&str>) -> String {
    match account_label.map(str::trim).filter(|label| !label.is_empty()) {
        Some(account) => format!("{agent_label} · {account}"),
        None => agent_label.to_string(),
    }
}

/// What the old pane's work looks like, read once. Every string is the agent's own text and is
/// cleaned by the brief builder, never used raw.
#[derive(Default)]
pub struct Gathered {
    /// The task as the person first gave it.
    pub goal: Option<String>,
    pub turns: Vec<TurnLine>,
    pub diff: Option<DiffStat>,
    pub open_plan: Vec<String>,
    pub last_message: Option<String>,
    /// A question the agent asked that nobody answered.
    pub open_question: Option<String>,
}

/// The run that starts the new agent. Plain data so a test can assert every field; the live
/// implementation turns it into an `AgentRunRequest`.
#[derive(Debug, Clone, PartialEq)]
pub struct StartRequest {
    pub project_path: String,
    pub task: String,
    pub agent_type: String,
    pub account: Option<String>,
    pub headless: bool,
    pub background: bool,
    pub keep_pane: bool,
    pub wait_ms: u64,
    pub timeout_ms: u64,
    pub idempotency_key: String,
    pub name: String,
    pub trust_workspace: bool,
    pub allow_shell: bool,
    pub allowed_commands: Option<Vec<String>>,
    pub read_only: bool,
    pub budget_turns: Option<u32>,
    pub budget_minutes: Option<u32>,
    pub join_worktree: Option<WorktreeRef>,
}

/// The new run, as `run_agent` answered.
#[derive(Debug, Clone)]
pub struct Started {
    pub handle: String,
    pub operation_id: String,
    pub pane_id: Option<String>,
}

/// Panes with a hand-over under way. One at a time per pane.
#[derive(Clone, Default)]
pub struct InFlight {
    panes: Arc<Mutex<HashSet<String>>>,
}

impl InFlight {
    /// Mark `pane_id`; `None` when a hand-over for it is already under way. The mark ends when the
    /// guard is dropped, on every path.
    pub fn enter(&self, pane_id: &str) -> Option<FlightGuard> {
        if !self.panes.lock().insert(pane_id.to_string()) {
            return None;
        }
        Some(FlightGuard { panes: self.panes.clone(), pane_id: pane_id.to_string() })
    }

    #[cfg(test)]
    pub fn holds(&self, pane_id: &str) -> bool {
        self.panes.lock().contains(pane_id)
    }
}

pub struct FlightGuard {
    panes: Arc<Mutex<HashSet<String>>>,
    pane_id: String,
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        self.panes.lock().remove(&self.pane_id);
    }
}

/// Everything outside the flow. See the module doc.
pub trait LiveIo {
    fn now_ms(&self) -> u64;
    fn clock(&self) -> &dyn Clock;
    fn store(&self) -> &Store;
    /// All records, newest first. The live implementation caches them (`status_line` asks every second).
    fn records(&self) -> Vec<Handover> {
        self.store().records()
    }
    fn questions(&self) -> &QuestionStore;
    fn in_flight(&self) -> &InFlight;

    fn pane(&self, pane_id: &str) -> Option<PaneFacts>;
    /// The billing vendor of an agent (`limits.vendor`); `None` = no `[limits]` table.
    fn vendor_of(&self, agent_id: &str) -> Option<String>;
    /// Agents that could take over in `folder`, in registry order, plus the saved-login targets in
    /// `extra`. `limited_until` stays `None`: the caller fills it in.
    fn candidates(&self, folder: &str, extra: &[Target]) -> Vec<Candidate>;
    /// The hand-off engine's note for an agent under an account (`handoff::agent_limit`).
    fn engine_limit(&self, agent_id: &str, account: Option<&str>) -> Option<Option<u64>>;
    /// The limit is on this pane's screen right now.
    fn at_limit(&self, pane_id: &str) -> bool;
    fn gather(&self, pane: &PaneFacts, run: Option<&OperationSnapshot>) -> Gathered;
    /// Strict redaction and the home folder as `~`, for every agent string that reaches a brief.
    fn clean(&self, text: &str) -> String;

    /// End the agent process in the pane (by its own pid). The pane stays.
    fn stop_agent(&self, pane_id: &str) -> Result<(), StopError>;
    fn start(&self, request: &StartRequest) -> Result<Started, OperationError>;
    /// Nest `child` under `parent` in the sidebar (display only).
    fn set_parent(&self, child: &str, parent: &str);
    /// Turns and minutes left on the old run's budget, when it had one.
    fn budget_left(&self, project: &str, handle: &str) -> (Option<u32>, Option<u32>);

    fn publish(&self, snapshot: &OperationSnapshot);
    fn log_limit(&self, entry: Entry);
    fn audit(&self, who: Who, summary: &str, detail: Value);
    fn mark(&self, pane_id: &str, text: &str);
    /// Records changed: refresh the status lines and tell the windows (`keep-going://changed`).
    fn changed(&self, pane_ids: &[&str]);
}

/// Everything `Gathered` needs from a record list, for the sake of one place that says "the goal".
pub fn goal_of(records: &[Handover], pane_id: &str) -> Option<String> {
    records.iter().find(|record| record.to.pane_id == pane_id).map(|record| record.goal.clone()).filter(|goal| !goal.is_empty())
}
