//! The first-run welcome (UX-PHASE1 section 14): what MUST hold no matter what the UI does lives here.
//!
//! - **Shown once.** The settings file carries `onboarding_done`: absent = never decided (the cockpit
//!   shows the welcome on a fresh profile), `true` = finished or skipped, `false` = the person asked
//!   to see it again (palette, Settings). [`state`] reads it; the UI writes it with the ordinary
//!   settings patch, so no new write path exists.
//! - **The first task cannot change anything.** [`plan_task`] builds the one request the welcome
//!   sends: always in a separate copy (`isolation: "worktree"`), read-only wherever the agent can
//!   enforce it (a headless run), and never with a flag that skips the agent's own questions. The
//!   folder goes through the same broad-folder guard as Simple mode ([`crate::plain_mode::guard`]).
//! - **Agent-agnostic.** Whether read-only can be enforced comes from the agent's features, passed in
//!   by the command; nothing here matches an agent name.
//!
//! The pure parts are tested without an app (`tests.rs`).

#[cfg(test)]
mod tests;

use crate::plain_mode::{self, guard, Refusal};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Settings key: `true` finished or skipped, `false` show again, absent never decided.
pub const DONE_KEY: &str = "onboarding_done";
/// Settings key: the step the person was on, so a restart resumes there.
pub const STEP_KEY: &str = "onboarding_step";

/// The suggested first task. Safe by construction: it asks for words, never for a change.
/// One line on purpose: `components/firstrun/copy.test.ts` reads this file and checks the UI shows the same words.
pub const TASK_PROMPT: &str = "Read this project and tell me, in a few plain sentences, what it does and how it is laid out. Change nothing: do not create, edit, move or delete any file.";

/// What the cockpit asks at start: should the welcome be on screen?
///
/// `auto` is true when the person never decided (`onboarding_done` absent). The UI adds one more
/// veto for that case only (a pane was opened here before), because this file cannot see panes.
pub fn state(settings: &Value) -> Value {
    let done = settings.get(DONE_KEY).and_then(Value::as_bool);
    let step = settings.get(STEP_KEY).and_then(Value::as_str).map(str::to_string);
    json!({
        "show": done != Some(true),
        "auto": done.is_none(),
        "step": step,
    })
}

/// Where the first task runs and what it asks, decided before anything is created.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskPlan {
    pub origin: PathBuf,
    /// `true`: read-only is enforced by the agent (a headless run). `false`: a visible pane in a
    /// separate copy; the folder is still untouched, and the UI says read-only is not enforced.
    pub enforced: bool,
    pub request: Value,
}

/// The run request of the first task.
pub fn task_request(origin: &Path, agent: &str, enforced: bool) -> Value {
    json!({
        "task": TASK_PROMPT,
        "project_path": origin.to_string_lossy(),
        "agent_type": agent,
        // Read-only can only be enforced on a headless run (agent_runs::capabilities).
        "headless": enforced,
        "background": true,
        "keep_pane": !enforced,
        "read_only": enforced,
        "isolation": "worktree",
        "context_policy": "fresh",
        "wait_ms": 1000,
        // The floor: nothing that skips the agent's own questions.
        "allow_shell": false,
        "trust_workspace": false,
        "allow_broad": false,
    })
}

/// Check the folder and build the request. `git_state` is `worktrees::folder::git_state` of it.
pub fn plan_task(
    path: &str,
    agent: &str,
    enforced: bool,
    known: &guard::KnownDirs,
    git_state: impl Fn(&Path) -> &'static str,
) -> Result<TaskPlan, Refusal> {
    let refuse = |code: &str, message: String| Refusal { code: code.to_string(), message };
    if agent.trim().is_empty() {
        return Err(refuse("AGENT_MISSING", "No agent was chosen.".into()));
    }
    let origin = crate::worktrees::folder::path_of(path);
    let text = origin.to_string_lossy().into_owned();
    if let Some(refusal) = plain_mode::refusal_for(guard::verdict(&text, known), &text) {
        return Err(refusal);
    }
    if !origin.is_dir() {
        return Err(refuse("FOLDER_MISSING", format!("{text} is not a folder.")));
    }
    if !guard::small_enough(&origin) {
        return Err(refuse("FOLDER_TOO_BIG", "This folder is too big to copy.".into()));
    }
    if git_state(&origin) != "repo" {
        return Err(refuse("FOLDER_NEEDS_GIT", "A separate copy needs Git in this folder. Set up Git here first.".into()));
    }
    Ok(TaskPlan { request: task_request(&origin, agent, enforced), origin, enforced })
}

/// What the welcome shows about the first task, from the run's view JSON (`AgentRunView`):
/// `{ status, result, error, receipt, pane_id }`. Nothing else leaves Rust.
pub fn summarize(view: &Value) -> Value {
    let pick = |key: &str| view.get(key).cloned().unwrap_or(Value::Null);
    json!({
        "status": pick("status"),
        "result": pick("result"),
        "error": pick("error"),
        "receipt": pick("receipt"),
        "pane_id": pick("pane_id"),
    })
}

/// Whether a run status is final (the welcome stops looking).
pub fn is_final(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}
