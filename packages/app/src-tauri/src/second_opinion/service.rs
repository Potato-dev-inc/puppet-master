//! Live wiring for the Tauri commands: find the diff of a pane, start the read-only reviewer run
//! through the normal `run_agent` path, poll it, and send a comment back to the original agent.
//! The decisions (which diff, the prompt, the run request, the parse, the routing) live in the
//! sibling modules and are tested there; this file only connects them to the running app.

use super::prompt::{build_prompt, PromptInput};
use super::route::{self, Sender, Sent, Targets};
use super::source::{self, RawDiff, Sources, MAX_REVIEW_PATCH_BYTES};
use super::{ParsedReview, ReviewDiff, SourceKind};
use crate::agent_runs::{AgentRunRequest, AgentRunView};
use crate::operations::{OperationError, OperationSnapshot};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

type Registry = Arc<Mutex<PaneRegistry>>;

/// A reviewer run that has not answered after this long is given up on (the run's own timeout).
const REVIEW_TIMEOUT_MS: u64 = 600_000;

// ---------------------------------------------------------------- reviewers

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewerEntry {
    pub id: String,
    pub label: String,
    /// A warm standby worker is waiting, so the review starts at once.
    pub ready: bool,
    /// This is the agent that wrote the code (off by default in the picker).
    pub same: bool,
}

/// The agents that can take a read-only headless run and are installed, from the registry.
pub fn reviewers(author_agent: Option<&str>) -> Vec<ReviewerEntry> {
    let labels: std::collections::HashMap<String, String> =
        crate::tui::descriptors().into_iter().map(|d| (d.id.to_string(), d.label)).collect();
    let capable: Vec<(String, String)> = crate::tui::read_only_backends()
        .into_iter()
        .filter(|id| crate::tui::is_agent_backend(id))
        .map(|id| (id.to_string(), labels.get(id).cloned().unwrap_or_else(|| id.to_string())))
        .collect();
    let entries = crate::askbar::list_agents(
        &capable,
        &|id| crate::tui::profile_for(id).is_installed(),
        &crate::standby::ready_agents(),
    );
    entries_with_same(entries, author_agent)
}

/// Mark which entry is the author's own agent.
pub fn entries_with_same(entries: Vec<crate::askbar::AgentEntry>, author_agent: Option<&str>) -> Vec<ReviewerEntry> {
    entries
        .into_iter()
        .map(|e| ReviewerEntry {
            same: author_agent.is_some_and(|a| a.eq_ignore_ascii_case(&e.id)),
            id: e.id,
            label: e.label,
            ready: e.ready,
        })
        .collect()
}

pub fn label_of(agent: &str) -> String {
    crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id.eq_ignore_ascii_case(agent))
        .map(|d| d.label)
        .unwrap_or_else(|| agent.to_string())
}

// ---------------------------------------------------------------- the run request

/// The `run_agent` request for a review. Read-only is not optional here: the reviewer must be an
/// agent whose headless mode enforces it (`quick::select_backend` refuses any other), and the
/// request carries `read_only: true`, no shell, no trust grant. `run_agent` itself refuses to
/// start a read-only run it cannot enforce.
pub fn build_run_request(
    reviewer: &str,
    project: &str,
    prompt: &str,
    installed: &dyn Fn(&str) -> bool,
) -> Result<AgentRunRequest, OperationError> {
    if reviewer.trim().is_empty() {
        return Err(OperationError::new("INVALID_ARGUMENT", "Pick an agent to review.", false));
    }
    let backend = crate::agent_runs::quick::select_backend(Some(reviewer), installed)?;
    let body = json!({
        "task": prompt,
        "project_path": project,
        "agent_type": backend,
        "headless": true,
        "background": true,
        "read_only": true,
        "keep_pane": false,
        "context_policy": "fresh",
        "allow_shell": false,
        "trust_workspace": false,
        "timeout_ms": REVIEW_TIMEOUT_MS,
        "wait_ms": 0,
        "idempotency_key": uuid::Uuid::new_v4().to_string(),
    });
    serde_json::from_value(body).map_err(|e| OperationError::new("INVALID_ARGUMENT", e.to_string(), false))
}

// ---------------------------------------------------------------- live diff sources

struct LiveSources<'a> {
    registry: &'a Registry,
    pane_id: &'a str,
    cwd: &'a str,
}

impl Sources for LiveSources<'_> {
    fn worktree(&self) -> Option<RawDiff> {
        let item = crate::worktrees::service::for_pane(self.registry, self.pane_id)?;
        if !item.entry.exists {
            return None;
        }
        let view = crate::worktrees::service::diff(Some(&item.entry.worktree.repo), &item.entry.worktree.handle, None, self.registry).ok()?;
        Some(RawDiff { patch: view.patch, truncated: view.patch_truncated })
    }

    fn checkpoint(&self) -> Option<RawDiff> {
        let turn = crate::checkpoints::live().last_turn_patch(self.pane_id, MAX_REVIEW_PATCH_BYTES * 2).ok()??;
        Some(RawDiff { patch: turn.patch, truncated: turn.truncated })
    }

    fn git(&self) -> Option<RawDiff> {
        source::git_diff_head(Path::new(self.cwd))
    }
}

// ---------------------------------------------------------------- start

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInfo {
    pub pane_id: String,
    pub handle: String,
    /// The project key the reviewer run was started under (pass it back to `poll`).
    pub project: String,
    pub reviewer: String,
    pub reviewer_label: String,
    pub source: SourceKind,
    pub source_label: &'static str,
    pub patch: String,
    pub patch_truncated: bool,
    pub files: usize,
    pub additions: u32,
    pub deletions: u32,
    pub author_label: String,
}

fn facts_of(pane_id: &str) -> (Option<String>, Vec<(String, String)>) {
    let last = crate::turn_summary::tracker().summaries(pane_id).into_iter().last();
    let Some(summary) = last else { return (None, Vec::new()) };
    let mut facts = Vec::new();
    if let Some(receipt) = &summary.receipt {
        facts.push(("Checks".to_string(), receipt.checks.clone()));
        facts.push(("Outcome".to_string(), receipt.outcome.clone()));
    }
    if !summary.approvals.is_empty() {
        facts.push(("Approvals answered".to_string(), summary.approvals.len().to_string()));
    }
    (Some(summary.sentence), facts)
}

/// What the author was asked: the task of the run bound to the pane, if there is one.
fn author_task(registry: &Registry, pane_id: &str) -> Option<String> {
    let (project, handle) = crate::agent_runs::pane_api::run_of_pane(registry, pane_id)?;
    crate::operations::latest_agent_run(&project, &handle).ok().map(|s| s.task)
}

pub fn start(pane_id: &str, reviewer: &str, registry: Registry, app: tauri::AppHandle) -> Result<StartInfo, OperationError> {
    let (cwd, author_agent) = {
        let guard = registry.lock();
        let pane = guard.panes.get(pane_id).ok_or_else(|| {
            OperationError::new("AGENT_GONE", "That agent's pane is closed.", false)
        })?;
        (pane.info.cwd.clone(), pane.info.agent_type.clone())
    };
    let diff = source::select(&LiveSources { registry: &registry, pane_id, cwd: &cwd })?;
    let origin = crate::worktrees::origin_of_cwd(&cwd).unwrap_or_else(|| cwd.clone());
    let memory = crate::one_brain::read_memory(Path::new(&origin));
    let (sentence, facts) = facts_of(pane_id);
    let task = author_task(&registry, pane_id);
    let author_label = label_of(&author_agent);
    let prompt = build_prompt(
        &PromptInput {
            task: task.as_deref(),
            turn_summary: sentence.as_deref(),
            facts,
            context: Some(memory.as_str()),
            author: Some(&author_label),
        },
        &diff,
    );
    // The run is keyed on the origin project, like an isolated run's own operation (never on the
    // worktree folder, which a merge or discard deletes).
    let request = build_run_request(reviewer, &origin, &prompt, &|id| crate::tui::profile_for(id).is_installed())?;
    let backend = request.agent_type.clone();
    let view = crate::agent_runs::run_agent(request, registry, app, None)?;
    Ok(info_of(pane_id, view, &backend, &origin, diff, author_label))
}

fn info_of(pane_id: &str, view: AgentRunView, backend: &str, cwd: &str, diff: ReviewDiff, author_label: String) -> StartInfo {
    StartInfo {
        pane_id: pane_id.to_string(),
        handle: view.handle,
        project: cwd.to_string(),
        reviewer: backend.to_string(),
        reviewer_label: label_of(backend),
        source: diff.source,
        source_label: diff.source.label(),
        patch: diff.patch,
        patch_truncated: diff.truncated,
        files: diff.files,
        additions: diff.additions,
        deletions: diff.deletions,
        author_label,
    }
}

// ---------------------------------------------------------------- poll

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    /// `running`, `done`, `failed` or `needs_input`.
    pub state: &'static str,
    pub review: Option<ParsedReview>,
    pub error: Option<String>,
}

pub(super) fn poll_of(snapshot: &OperationSnapshot, reviewer_label: &str) -> Poll {
    let view = AgentRunView::from(snapshot.clone());
    match view.status.as_str() {
        "completed" => match view.result.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(answer) => Poll { state: "done", review: Some(super::parse::parse_review(answer)), error: None },
            None => Poll { state: "failed", review: None, error: Some(format!("{reviewer_label} gave an empty answer.")) },
        },
        "needs_input" => Poll {
            state: "needs_input",
            review: None,
            error: Some(format!("{reviewer_label} stopped to ask for permission. Pick another reviewer.")),
        },
        _ if snapshot.status.terminal() => {
            let (code, message) = view
                .error
                .as_ref()
                .map(|e| (e.code.clone(), e.message.clone()))
                .unwrap_or_else(|| ("REVIEW_FAILED".into(), format!("the run ended with status {}", view.status)));
            let (_, plain) = crate::agent_health::classify_failure(&code, &message);
            Poll { state: "failed", review: None, error: Some(plain) }
        }
        _ => Poll { state: "running", review: None, error: None },
    }
}

pub fn poll(project: &str, handle: &str) -> Result<Poll, OperationError> {
    let snapshot = crate::operations::resolve_agent_run(project, handle)?;
    Ok(poll_of(&snapshot, &label_of(&snapshot.agent_type)))
}

/// Stop a review that is still running.
pub fn cancel(project: &str, handle: &str, registry: Registry, app: tauri::AppHandle) -> Result<(), OperationError> {
    crate::agent_runs::cancel_agent(project, handle, None, registry, app).map(|_| ())
}

// ---------------------------------------------------------------- send back

struct LiveTargets<'a>(&'a Registry);

impl Targets for LiveTargets<'_> {
    fn run_of_pane(&self, pane_id: &str) -> Option<(String, String)> {
        crate::agent_runs::pane_api::run_of_pane(self.0, pane_id)
    }
    fn pane_exists(&self, pane_id: &str) -> bool {
        self.0.lock().panes.contains_key(pane_id)
    }
}

struct LiveSender {
    registry: Registry,
    app: tauri::AppHandle,
}

impl Sender for LiveSender {
    fn followup(&self, project: &str, handle: &str, task: &str) -> Result<(), OperationError> {
        let opts = crate::agent_runs::persist::FollowupOpts { background: true, ..Default::default() };
        crate::agent_runs::send_agent(project, handle, task.to_string(), None, self.registry.clone(), self.app.clone(), opts)
            .map(|_| ())
    }
    /// Through the pane outbox, like every message for an agent: it types only into a ready input box, closes a
    /// startup notice with the agent's own keys first and waits behind a question. Raw keys + Enter would answer
    /// whatever menu is on screen.
    fn pane_input(&self, pane_id: &str, text: &str) -> Result<(), String> {
        crate::pane_outbox::global(&self.registry, &self.app).send(pane_id, text).map(|_| ())
    }
}

pub fn send_to_author(
    pane_id: &str,
    file: &str,
    line: Option<u32>,
    text: &str,
    registry: Registry,
    app: tauri::AppHandle,
) -> Result<Sent, OperationError> {
    let targets = LiveTargets(&registry);
    let sender = LiveSender { registry: registry.clone(), app };
    route::deliver(&targets, &sender, pane_id, file, line, text)
}

/// Send a ready-made message to the agent behind a pane, the way a review comment goes (a follow-up
/// turn for a run, typed into the pane for one opened by hand). Used by "Send back" on a pull
/// request whose GitHub checks failed.
pub fn send_text_to_author(
    pane_id: &str,
    text: &str,
    registry: Registry,
    app: tauri::AppHandle,
) -> Result<Sent, OperationError> {
    let message = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if message.is_empty() {
        return Err(OperationError::new("INVALID_ARGUMENT", "There is nothing to send.", false));
    }
    let targets = LiveTargets(&registry);
    let sender = LiveSender { registry: registry.clone(), app };
    route::deliver_text(&targets, &sender, pane_id, &message)
}

/// The agent type running in a pane (for "exclude the same agent").
pub fn author_agent_of(registry: &Registry, pane_id: &str) -> Option<String> {
    registry.lock().panes.get(pane_id).map(|p| p.info.agent_type.clone())
}
