//! The `AgentRunRequest` body accepted by `run_agent`, its serde defaults, and the
//! `context_mode`/`context_policy` alias merge applied before deserializing.

use crate::agent_runs::persist::{self};
use crate::operations::{ContextPolicy, OperationError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_TIMEOUT_MS: u64 = 900_000;

pub(in crate::agent_runs) fn default_agent() -> String {
    "codex".into()
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}
fn default_wait() -> u64 {
    persist::FOREGROUND_WAIT_MS
}
/// Visible-first: a run opens a pane unless the caller passes `headless: true` (or the run falls
/// back to headless, see `headless::fallback`).
fn default_headless() -> bool {
    false
}

/// Merge `context_mode` (alias) into `context_policy` before deserializing bodies.
/// Either key alone, neither, or both with the same value are accepted. Two different values
/// are an error unless one is the legacy `packet` schema default, which yields to the other.
/// Null counts as absent.
pub(crate) fn normalize_context_policy_fields(value: &mut Value) -> Result<(), OperationError> {
    let map = match value.as_object_mut() {
        Some(map) => map,
        None => return Ok(()),
    };
    let mode = map.remove("context_mode").filter(|v| !v.is_null());
    let policy = map.remove("context_policy").filter(|v| !v.is_null());
    let merged = match (mode, policy) {
        (None, None) => return Ok(()),
        (Some(v), None) | (None, Some(v)) => v,
        (Some(mode), Some(policy)) => {
            // `packet` was the schema default of both fields. A host (or cached schema) that
            // fills the untouched alias with it must not turn one explicit choice into a conflict.
            let is_default = |v: &Value| v.as_str() == Some("packet");
            if mode != policy && is_default(&policy) {
                mode
            } else if mode != policy && is_default(&mode) {
                policy
            } else if mode != policy {
                return Err(OperationError::new(
                    "INVALID_ARGUMENT",
                    format!(
                        "context_mode and context_policy disagree ({} vs {}); they are aliases, set only one",
                        mode.as_str().unwrap_or("?"),
                        policy.as_str().unwrap_or("?")
                    ),
                    false,
                ));
            } else {
                policy
            }
        }
    };
    map.insert("context_policy".into(), merged);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRequest {
    #[serde(default)]
    pub project_path: Option<String>,
    pub task: String,
    #[serde(default = "default_agent")]
    pub agent_type: String,
    #[serde(default = "default_headless")]
    pub headless: bool,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub keep_pane: bool,
    /// `auto` | `grid` | `window`: where a visible pane opens.
    #[serde(default)]
    pub placement: Option<String>,
    /// `none` (default) | `worktree`: run the worker in its own git worktree on its own branch
    /// (`pm/<handle>`) so parallel agents never touch the same files. The run's project key stays
    /// the origin repo. Review, merge or discard through the worktree routes / cockpit.
    #[serde(default)]
    pub isolation: Option<String>,
    #[serde(default)]
    pub worker_has_mcp_tools: bool,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_wait")]
    pub wait_ms: u64,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Option<Vec<String>>,
    /// State-based checks (file_exists / file_contains / file_absent) evaluated at completion.
    #[serde(default)]
    pub checks: Option<Vec<crate::agent_runs::checks::Check>>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub allow_broad: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub agent_run_id: Option<String>,
    #[serde(default)]
    pub turn_index: u32,
    #[serde(default)]
    pub context_policy: ContextPolicy,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "reasoning_effort")]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub selected_history: Option<Vec<String>>,
    /// Resource locks held for the run (`type:name`, e.g. `file:src/a.rs`).
    #[serde(default)]
    pub locks: Vec<String>,
    /// Opt-in: pass `--trust` to a headless `cursor_agent` run, trusting `project_path`.
    #[serde(default)]
    pub trust_workspace: bool,
    /// Opt-in: pass `--force` to a headless `cursor_agent` run so its shell commands execute.
    #[serde(default)]
    pub allow_shell: bool,
    /// Opt-in: bare program names answered allow-once on Cursor `Not in allowlist` prompts.
    #[serde(default)]
    pub allowed_commands: Option<Vec<String>>,
    /// Run the agent under this user account (id from the accounts store, `crate::accounts`).
    /// Absent = the CLI's own login.
    #[serde(default)]
    pub account: Option<String>,
    /// Budget for the new run, in turns (`crate::budgets`). Absent: the Settings default, which is
    /// off unless the person set one.
    #[serde(default)]
    pub budget_turns: Option<u32>,
    /// Budget for the new run, in wall-clock minutes.
    #[serde(default)]
    pub budget_minutes: Option<u32>,
    /// The run that is starting this one as a helper, so the new run draws from its budget. The
    /// MCP shim fills it from `PUPPET_MASTER_PARENT_RUN`; an agent may pass it itself.
    #[serde(default)]
    pub budget_parent: Option<String>,
    /// Keep working in rounds until every check passes: the run becomes a mission
    /// (`crate::missions`, Preview). Needs checks, a budget and a visible pane; never merges.
    #[serde(default)]
    pub until_checks_pass: bool,
    /// With `until_checks_pass`: agents to hand the mission to, in order, when the current one
    /// stops making progress (`agent_type` or `agent_type@account`).
    #[serde(default)]
    pub backup_agents: Option<Vec<String>>,
    /// Crate-internal (never in the MCP schema): run in this existing separate copy instead of
    /// creating one. Every agent of a mission works in the mission's one copy.
    #[serde(skip)]
    pub(crate) join_worktree: Option<crate::worktrees::WorktreeRef>,
    /// Crate-internal (never in the MCP schema): the caller did not pass `headless`, so the
    /// agent's "How agent runs work" setting may decide it (`crate::agent_run_mode`). Set by the
    /// `/agents/run` route only; every other path (follow-ups, quick_ask, missions, the app's own
    /// runs) keeps the mode it names.
    #[serde(skip)]
    pub(crate) headless_unset: bool,
    /// Crate-internal (never in the MCP schema, never read from a JSON body): the env guard the
    /// run's pane or process starts under (`pty::env_guard`). Only Rust code inside the app sets
    /// it. A guarded run never takes a standby or an existing pane and never adopts a pane id.
    #[serde(skip)]
    pub(crate) guard_env: Option<String>,
    /// Crate-internal (never in the MCP schema, never read from a JSON body): the pane id of the companion
    /// this run is started for (`companion-<id>`), when the person said yes to a helper the companion asked
    /// for (Potato Bot). The run gets the house rules of a companion's helper: the default turn budget, never
    /// the companion's home folder, a `delegated` feed line.
    #[serde(skip)]
    pub on_behalf_of_companion: Option<String>,
}

impl AgentRunRequest {
    /// The companion this run is a helper of: the one the app named (`on_behalf_of_companion`), else the one
    /// `from_session` finds (the companion whose agent asked over the bridge). It decides the helper's house
    /// rules: the default turn budget, never the companion's home folder, a `delegated` feed line.
    pub fn companion_pane_with(&self, from_session: impl FnOnce() -> Option<String>) -> Option<String> {
        self.on_behalf_of_companion.clone().or_else(from_session)
    }
}
