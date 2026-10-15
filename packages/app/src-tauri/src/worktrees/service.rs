//! The review API over worktrees: list, diff, merge and discard by `(project, handle)`.
//! Shared by the Tauri commands (cockpit) and the `/worktrees/*` bridge routes, so both behave
//! identically. It adds what the git layer cannot know: whether the run is still working, which
//! panes sit in the worktree, and recording the outcome on the operation.

use super::{ChangeSummary, DiffView, MergeOutcome, WorktreeEntry, WorktreeRef};
use crate::operations::{self, OperationError};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

type Registry = Arc<Mutex<PaneRegistry>>;

/// One worktree as shown in the cockpit and returned by `GET /worktrees`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListItem {
    #[serde(flatten)]
    pub entry: WorktreeEntry,
    /// `active` (an agent is still working in it), `unreviewed` (finished or its pane closed, not
    /// merged or discarded) or `missing` (the checkout vanished).
    pub state: String,
    pub agent_type: Option<String>,
    pub pane_id: Option<String>,
    pub task: Option<String>,
    /// Live changed-files totals (only for `unreviewed` worktrees).
    pub summary: Option<ChangeSummary>,
}

/// The projects to look in: the one asked for, else every known project plus the default one.
fn projects(project: Option<&str>, registry: &Registry) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut add = |path: String| {
        let key = crate::project_path::comparable_project_path(Path::new(&path));
        if !path.trim().is_empty()
            && !found
                .iter()
                .any(|seen| crate::project_path::comparable_project_path(Path::new(seen)) == key)
        {
            found.push(path);
        }
    };
    if let Some(project) = project.filter(|p| !p.trim().is_empty()) {
        add(project.to_string());
        return found;
    }
    add(registry.lock().project_path.clone());
    for path in operations::indexed_project_paths() {
        add(path);
    }
    found
}

/// Live panes whose working directory is inside the worktree: `(pane id, status)`.
fn panes_in(registry: &Registry, worktree: &WorktreeRef) -> Vec<(String, String)> {
    registry
        .lock()
        .list()
        .into_iter()
        .filter(|pane| {
            crate::project_path::workspace_covers(Path::new(&worktree.path), Path::new(&pane.cwd))
        })
        .map(|pane| (pane.id, pane.status))
        .collect()
}

fn item_of(entry: WorktreeEntry, project: &str, registry: &Registry) -> ListItem {
    let worktree = &entry.worktree;
    let op = operations::latest_agent_run(project, &worktree.handle).ok();
    let panes = panes_in(registry, worktree);
    let active = op.as_ref().is_some_and(|op| !op.status.terminal())
        || panes.iter().any(|(_, status)| status == "running");
    let state = if !entry.exists {
        "missing"
    } else if active {
        "active"
    } else {
        "unreviewed"
    };
    let summary = if state == "unreviewed" {
        super::changes(worktree).ok().map(|c| c.summary())
    } else {
        None
    };
    ListItem {
        state: state.into(),
        agent_type: op.as_ref().map(|op| op.agent_type.clone()),
        pane_id: op
            .as_ref()
            .and_then(|op| op.pane_id.clone())
            .or_else(|| panes.first().map(|(id, _)| id.clone())),
        task: op.as_ref().map(|op| op.task.chars().take(160).collect()),
        summary,
        entry,
    }
}

/// Every open worktree of `project` (all known projects when `None`).
pub fn list(project: Option<&str>, registry: &Registry) -> Vec<ListItem> {
    projects(project, registry)
        .into_iter()
        .flat_map(|project| {
            super::list(&project)
                .into_iter()
                .map(|entry| item_of(entry, &project, registry))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The worktree whose checkout contains `pane_id`'s working directory, if any. This is how the
/// cockpit finds the "Changes" of a pane, run-backed or opened by hand.
pub fn for_pane(registry: &Registry, pane_id: &str) -> Option<ListItem> {
    let cwd = registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id)?
        .cwd;
    let origin = super::origin_of_cwd(&cwd)?;
    super::list(&origin)
        .into_iter()
        .find(|entry| {
            crate::project_path::workspace_covers(Path::new(&entry.worktree.path), Path::new(&cwd))
        })
        .map(|entry| item_of(entry, &origin, registry))
}

fn resolve(
    project: Option<&str>,
    handle: &str,
    registry: &Registry,
) -> Result<(String, WorktreeRef), OperationError> {
    projects(project, registry)
        .into_iter()
        .find_map(|project| super::find_by_handle(&project, handle).map(|w| (project, w)))
        .ok_or_else(|| {
            OperationError::new(
                "WORKTREE_NOT_FOUND",
                format!("no worktree for {handle}; it may already be merged or discarded"),
                false,
            )
        })
}

/// Refuse to touch a worktree an agent is still working in.
fn ensure_idle(
    project: &str,
    worktree: &WorktreeRef,
    registry: &Registry,
) -> Result<(), OperationError> {
    let working = operations::handle_is_active(project, &worktree.handle)
        || panes_in(registry, worktree)
            .iter()
            .any(|(_, status)| status == "running");
    if working {
        return Err(OperationError::new(
            "RUN_ACTIVE",
            "the agent is still working in this worktree; wait for it to finish or stop it first",
            true,
        ));
    }
    Ok(())
}

/// Close the panes sitting in the worktree (their directory is about to disappear).
fn close_panes(registry: &Registry, worktree: &WorktreeRef) {
    for (pane_id, _) in panes_in(registry, worktree) {
        let _ = crate::pty::registry_kill_pane_with_reason(
            registry,
            &pane_id,
            Some("worktree_closed"),
        );
    }
}

fn record_closed(project: &str, handle: &str, how: &str) {
    if let Ok(snapshot) = operations::close_worktree(project, handle, how) {
        crate::bridge::push_operation_sse(&snapshot);
    }
}

pub fn diff(
    project: Option<&str>,
    handle: &str,
    path: Option<&str>,
    registry: &Registry,
) -> Result<DiffView, OperationError> {
    let (_, worktree) = resolve(project, handle, registry)?;
    super::diff(&worktree, path)
}

/// Merge into the project's current branch. `merged: false` (`reason: no_changes`) leaves the
/// worktree in place.
pub fn merge(
    project: Option<&str>,
    handle: &str,
    registry: &Registry,
) -> Result<MergeOutcome, OperationError> {
    let (project, worktree) = resolve(project, handle, registry)?;
    ensure_idle(&project, &worktree, registry)?;
    let outcome = super::merge_keep(&worktree)?;
    if outcome.merged {
        close_panes(registry, &worktree);
        super::cleanup_merged(&worktree)?;
        record_closed(&project, handle, "merged");
    }
    Ok(outcome)
}

/// Throw the worktree and its branch away.
pub fn discard(
    project: Option<&str>,
    handle: &str,
    registry: &Registry,
) -> Result<(), OperationError> {
    let (project, worktree) = resolve(project, handle, registry)?;
    ensure_idle(&project, &worktree, registry)?;
    close_panes(registry, &worktree);
    super::discard(&worktree)?;
    record_closed(&project, handle, "discarded");
    Ok(())
}

// ------------------------------------------------------------------------------ pull requests

use super::pr::{self, ChecksReport, PrFacts, PrRef, PrStatus};
use super::proc::ToolEnv;
use std::time::Duration;

/// What the Create PR sheet is opened with: the facts, and a title and body made from the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrPrepared {
    #[serde(flatten)]
    pub status: PrStatus,
    pub title: String,
    pub body: String,
    pub base: String,
    pub branch: String,
    /// Display name of the agent that wrote the changes, for "Saved <agent>'s changes".
    pub agent: Option<String>,
    /// Loose edits that Create PR will save as one commit first.
    pub has_uncommitted: bool,
}

fn entry_of(worktree: &WorktreeRef) -> WorktreeEntry {
    WorktreeEntry { worktree: worktree.clone(), exists: Path::new(&worktree.path).is_dir() }
}

/// The facts a pull request is described with, read from what was recorded about the run.
fn pr_facts(project: &str, worktree: &WorktreeRef, item: &ListItem) -> PrFacts {
    let mut facts = PrFacts::default();
    if let Ok(op) = operations::latest_agent_run(project, &worktree.handle) {
        facts.task = Some(op.task.clone()).filter(|t| !t.trim().is_empty());
        let total = op.checks.len();
        if total > 0 {
            let passed = op.check_results.iter().filter(|r| r.passed).count();
            facts.checks = Some(format!("{passed}/{total} passed"));
        }
    }
    if let Some(pane_id) = item.pane_id.as_deref() {
        if let Some(last) = crate::turn_summary::tracker().summaries(pane_id).into_iter().last() {
            facts.summary = Some(last.sentence.clone()).filter(|s| !s.is_empty());
            facts.agent = Some(last.agent.clone());
            if facts.checks.is_none() {
                facts.checks = last.receipt.as_ref().map(|r| r.checks.clone());
            }
        }
    }
    if let Ok(changes) = super::changes(worktree) {
        facts.files = changes.files.iter().map(|f| (f.path.clone(), f.additions, f.deletions)).collect();
        facts.files_total = changes.files_changed;
        facts.additions = changes.additions;
        facts.deletions = changes.deletions;
    }
    facts.setup = worktree.setup.as_ref().map(|s| s.sentence());
    facts
}

pub fn pr_prepare(
    project: Option<&str>,
    handle: &str,
    registry: &Registry,
    env: &ToolEnv,
) -> Result<PrPrepared, OperationError> {
    let (project, worktree) = resolve(project, handle, registry)?;
    let item = item_of(entry_of(&worktree), &project, registry);
    let status = pr::status(&worktree, env);
    let facts = pr_facts(&project, &worktree, &item);
    let base = status.default_branch.clone().unwrap_or_else(|| worktree.base.clone());
    let has_uncommitted = super::has_uncommitted(&worktree).unwrap_or(false);
    Ok(PrPrepared {
        title: pr::default_title(&facts, &worktree.branch),
        body: pr::render_body(&facts),
        base,
        branch: worktree.branch.clone(),
        agent: facts.agent.clone(),
        has_uncommitted,
        status,
    })
}

/// Push the branch and make (or update) the pull request, then remember it on the worktree.
pub fn pr_create(
    project: Option<&str>,
    handle: &str,
    title: &str,
    body: &str,
    base: &str,
    registry: &Registry,
    env: &ToolEnv,
) -> Result<PrRef, OperationError> {
    let (project, worktree) = resolve(project, handle, registry)?;
    ensure_idle(&project, &worktree, registry)?;
    let mut stored = super::find_by_handle(&project, handle).unwrap_or(worktree);
    // Saves the agent's loose edits as one commit. A branch that may not be pushed is refused
    // inside `pr::create`, after this only touched the copy's own branch.
    super::commit_pending(&stored)?;
    let pr_ref = pr::create(&stored, title, body, base, env)?;
    stored.pr = Some(pr_ref.clone());
    super::update_meta(&stored)?;
    Ok(pr_ref)
}

/// Read the PR's checks (polling for at most `wait` while some are pending) and keep the result on
/// the worktree so the receipt shows it.
pub fn pr_checks(
    project: Option<&str>,
    handle: &str,
    wait: Duration,
    registry: &Registry,
    env: &ToolEnv,
) -> Result<PrRef, OperationError> {
    let (project, _) = resolve(project, handle, registry)?;
    let mut stored = stored_with_pr(&project, handle)?;
    let mut pr_ref = stored.pr.clone().expect("checked by stored_with_pr");
    let interval = Duration::from_secs(5);
    let polls = (wait.as_secs() / interval.as_secs()) as u32 + 1;
    let report = pr::poll_checks(&stored, env, interval, polls)?;
    pr_ref.checks = Some(report);
    stored.pr = Some(pr_ref.clone());
    super::update_meta(&stored)?;
    Ok(pr_ref)
}

fn stored_with_pr(project: &str, handle: &str) -> Result<WorktreeRef, OperationError> {
    let stored = super::find_by_handle(project, handle)
        .ok_or_else(|| OperationError::new("WORKTREE_NOT_FOUND", format!("no worktree for {handle}"), false))?;
    if stored.pr.is_none() {
        return Err(OperationError::new("NO_PR", "This copy has no pull request yet.", false));
    }
    Ok(stored)
}

/// What "Send back" would tell the agent: shown first, sent only after the person agrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SendBackPreview {
    pub text: String,
    pub failing: Vec<String>,
    /// Lines of the failing job's log that are included.
    pub excerpt_lines: usize,
}

pub fn pr_send_back_preview(
    project: Option<&str>,
    handle: &str,
    registry: &Registry,
    env: &ToolEnv,
) -> Result<SendBackPreview, OperationError> {
    let (project, _) = resolve(project, handle, registry)?;
    let stored = stored_with_pr(&project, handle)?;
    let pr_ref = stored.pr.clone().expect("checked by stored_with_pr");
    let report: ChecksReport = pr::checks_once(&stored, env)?;
    if report.failing().is_empty() {
        return Err(OperationError::new("NO_FAILED_CHECKS", "No GitHub check has failed.", false));
    }
    let excerpt = pr::failure_excerpt(&stored, &report, env);
    let text = pr::send_back_text(&pr_ref, &report, &excerpt);
    Ok(SendBackPreview {
        failing: report.failing().iter().map(|i| i.name.clone()).collect(),
        excerpt_lines: excerpt.lines().count(),
        text,
    })
}

/// The pane (or run) a message for the agent in this copy goes to.
fn author_pane(project: Option<&str>, handle: &str, registry: &Registry) -> Result<String, OperationError> {
    let (project, worktree) = resolve(project, handle, registry)?;
    item_of(entry_of(&worktree), &project, registry).pane_id.ok_or_else(|| {
        OperationError::new("AGENT_GONE", "That agent's pane is closed, so there is nobody to send this to.", false)
    })
}

/// Deliver `text` to the agent that works in this copy: a follow-up turn for a run, typed into the
/// pane for one opened by hand.
pub fn pr_send_back(
    project: Option<&str>,
    handle: &str,
    text: &str,
    registry: &Registry,
    app: tauri::AppHandle,
) -> Result<crate::second_opinion::route::Sent, OperationError> {
    let pane_id = author_pane(project, handle, registry)?;
    crate::second_opinion::service::send_text_to_author(&pane_id, text, registry.clone(), app)
}
