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
