//! Branch from here: everything the flow touches outside itself, behind one trait, so
//! `engine_tests.rs` can run the whole flow without an app or an agent (SPEC n33 section 5, BF-8).
//!
//! The live implementation is `live_app::AppIo`. The fake one in the tests records the order of
//! the calls (the copy is made before the agent starts) and the exact run request.

use super::copy::Made;
use super::model::Branch;
use super::offers::Candidate;
use super::store::Store;
use crate::audit::Who;
use crate::checkpoints::Checkpoint;
use crate::handoff::keep_going::live_io::label_with;
use crate::handoff::keep_going::model::Target;
use crate::operations::OperationError;
use crate::turn_summary::TurnSummary;
use crate::worktrees::WorktreeRef;
use serde_json::Value;

// One branch being made per pane: the same guard Keep going uses (a plain set of pane ids).
pub use crate::handoff::keep_going::live_io::{FlightGuard, InFlight, Started};

/// A pane as a branch needs to know it.
#[derive(Debug, Clone)]
pub struct PaneFacts {
    pub pane_id: String,
    pub agent_id: String,
    pub account: Option<String>,
    /// The agent's name from the registry ("Agent A").
    pub agent_label: String,
    /// The saved login's label, when the pane runs under one.
    pub account_label: Option<String>,
    /// The pane's own working directory. The save points are read from its shadow repository.
    pub cwd: String,
    /// The origin project (run project key): the pane's folder, or the project its copy belongs to.
    pub origin: String,
    /// A companion's pane.
    pub companion: bool,
    /// A mission round's pane (missions have their own save points).
    pub mission: bool,
    /// Automatic save points are turned on.
    pub checkpoints_on: bool,
    /// The agent's profile takes part in save points (plain shells do not).
    pub takes_part: bool,
}

impl PaneFacts {
    /// "Agent A" or "Agent A · Work".
    pub fn label(&self) -> String {
        label_with(&self.agent_label, self.account_label.as_deref())
    }

    /// The pane's agent and login as a picker target.
    pub fn target(&self) -> Target {
        Target { agent_id: self.agent_id.clone(), account: self.account.clone() }
    }
}

/// What making the copy needs (flow B3). The live side adds the start point and the git environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyRequest {
    pub origin: String,
    /// The branch id: also the copy's handle.
    pub handle: String,
    /// The source pane's folder: its save points are read from there.
    pub source_cwd: String,
    pub pane_id: String,
    pub checkpoint_id: String,
    /// The one start commit's message.
    pub commit_message: String,
}

/// The run that starts the new agent (flow B4). Plain data so a test can assert every field; the live
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
    pub handle: String,
    pub trust_workspace: bool,
    pub allow_shell: bool,
    pub allowed_commands: Option<Vec<String>>,
    pub read_only: bool,
    /// `fresh`: a new conversation, not the old one's history.
    pub context_policy: String,
    /// The env guard the pane starts under (`pty::env_guard::NO_PUSH`).
    pub guard_env: String,
    pub join_worktree: WorktreeRef,
}

/// Everything outside the flow. See the module doc. The source pane is only ever read through it.
pub trait LiveIo: Send + Sync {
    fn now_ms(&self) -> u64;
    fn store(&self) -> &Store;
    fn in_flight(&self) -> &InFlight;

    fn pane(&self, pane_id: &str) -> Option<PaneFacts>;
    /// The pane's own save points, oldest first. `None` when there are none to read (Undo is off).
    fn checkpoints(&self, pane_id: &str) -> Option<Vec<Checkpoint>>;
    fn summaries(&self, pane_id: &str) -> Vec<TurnSummary>;
    /// `repo` | `no_commits` | `folder` | `missing` (`worktrees::folder::git_state`).
    fn git_state(&self, origin: &str) -> &'static str;
    /// Agents that could run a branch from `source`, in registry order, with their live limits.
    fn candidates(&self, source: &PaneFacts) -> Vec<Candidate>;
    /// "17:00" or "Oct 16 17:00" for a reset time.
    fn when_text(&self, ms: u64) -> String;
    /// Strict redaction, e-mails masked and the home folder as `~`, for every string that reaches a brief.
    fn clean(&self, text: &str) -> String;

    /// Make the separate copy (copy, write the save point, one start commit). Throws a half-made copy away.
    fn make_copy(&self, request: &CopyRequest) -> Result<Made, OperationError>;
    fn start(&self, request: &StartRequest) -> Result<Started, OperationError>;
    /// Nest `child` under `parent` in the sidebar (display only).
    fn set_parent(&self, child: &str, parent: &str);
    /// `kept` or `thrown_away` once the branch copy was merged or thrown away in Changes; `None` while open.
    fn worktree_closed(&self, origin: &str, handle: &str) -> Option<&'static str>;

    fn audit(&self, who: Who, summary: &str, detail: Value);
    /// Records changed: tell the windows (`branch://changed`).
    fn changed(&self, pane_ids: &[&str]);
}

/// A branch record that still takes up one of the pane's open slots (`OPEN_PER_PANE_CAP`): its copy
/// exists and was not kept or thrown away yet.
pub fn is_open(io: &dyn LiveIo, record: &Branch) -> bool {
    record.copy.is_some() && io.worktree_closed(&record.project, &record.id).is_none()
}
