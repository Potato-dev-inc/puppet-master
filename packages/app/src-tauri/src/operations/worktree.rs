//! Recording the state of a run's git worktree on the operation (`worker.worktree`).

use super::model::*;
use super::store::{bump, update_lock};
use super::{get_operation, latest_agent_run, list_operations};

/// The handle's latest turn is still working (so its worktree must not be merged or discarded).
/// A run that JOINED this handle's copy (a Keep going hand-over, a mission round) counts too: any
/// non-terminal run whose worktree is this handle.
pub fn handle_is_active(project: &str, handle: &str) -> bool {
    if latest_agent_run(project, handle).map_or(false, |op| !op.status.terminal()) {
        return true;
    }
    if handle.is_empty() {
        return false;
    }
    list_operations(project).map_or(false, |ops| {
        ops.iter().any(|op| {
            !op.status.terminal()
                && op.worker.worktree.as_ref().is_some_and(|worktree| worktree.handle == handle)
        })
    })
}

/// Mark the handle's worktree `merged` or `discarded` on its latest operation, so a later turn
/// on the same handle falls back to the project folder instead of a deleted checkout.
pub fn close_worktree(
    project: &str,
    handle: &str,
    how: &str,
) -> Result<OperationSnapshot, OperationError> {
    let latest = latest_agent_run(project, handle)?;
    let _serial = update_lock();
    let mut op = get_operation(&latest.project_path, &latest.operation_id)?;
    if let Some(worktree) = op.worker.worktree.as_mut() {
        worktree.closed = Some(how.to_string());
        bump(&mut op)?;
    }
    Ok(op)
}

/// Store the changed-files summary on the operation so the receipt can show it without git.
pub fn record_worktree_changes(
    project: &str,
    id: &str,
    summary: crate::worktrees::ChangeSummary,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if let Some(worktree) = op.worker.worktree.as_mut() {
        worktree.changes = Some(summary);
        bump(&mut op)?;
    }
    Ok(op)
}

/// Every project that has ever had an operation (where isolated worktrees may live).
pub fn indexed_project_paths() -> Vec<String> {
    super::store::indexed_projects().unwrap_or_default()
}
