//! Making the separate copy of a branch and its one start commit (SPEC n33 section 3.7, flow B3).
//!
//! The copy is an ordinary worktree copy (`worktrees::create_with`), started from the source folder's
//! current commit, or from the source copy's branch when the source pane works in a copy. The save
//! point's files are written over it with `checkpoints::export::export_into`, and PM commits that as
//! one start commit. The original folder is only read. When a step fails, PM throws away the half-made
//! copy: only PM wrote to it, and no agent has run in it yet.

#[cfg(test)]
#[path = "copy_tests.rs"]
mod copy_tests;

use std::path::Path;

use crate::checkpoints::export::{export_into, ExportReport};
use crate::checkpoints::CheckpointError;
use crate::operations::OperationError;
use crate::worktrees::{self, CreateOptions, WorktreeRef};

use super::model::{ERR_BRANCH_COPY_FAILED, MSG_BRANCH_COPY_FAILED};

/// What [`make`] produced. The caller keeps the copy (the branch's run starts in it next).
#[derive(Debug, Clone)]
pub struct Made {
    pub worktree: WorktreeRef,
    /// Files written into the copy from the save point. Paths that failed are listed, never hidden.
    pub export: ExportReport,
    /// The one start commit, or `None` when the save point matches the copy's starting commit.
    pub start_commit: Option<String>,
}

/// Make the separate copy for one branch. Steps: create the copy from `start_point` (or the origin's
/// HEAD when `None`), write save point `checkpoint_id` of `pane` (read from `source_cwd`) over it,
/// and commit that once as `commit_message`. Any failure after the copy exists throws the copy away
/// and returns `BRANCH_COPY_FAILED`; the original folder is never written.
#[allow(clippy::too_many_arguments)]
pub fn make(
    origin: &str,
    start_point: Option<&str>,
    handle: &str,
    source_cwd: &Path,
    pane: &str,
    checkpoint_id: &str,
    commit_message: &str,
    git_env: &[(String, String)],
) -> Result<Made, OperationError> {
    let opts = CreateOptions {
        start_point: start_point.map(str::to_string),
        copy_files: true,
        git_env: git_env.to_vec(),
    };
    // Nothing exists yet when this fails, so there is nothing to throw away.
    let copy = worktrees::create_with(origin, handle, &opts).map_err(|e| copy_failed(&e.message))?;
    match fill(&copy, source_cwd, pane, checkpoint_id, commit_message, git_env) {
        Ok((export, start_commit)) => Ok(Made { worktree: copy, export, start_commit }),
        Err(reason) => {
            // Only PM wrote to this copy. `discard` removes the checkout, its branch and its description.
            if let Err(e) = worktrees::discard(&copy) {
                tracing::warn!(error = %e.message, "could not throw away a half-made branch copy");
            }
            Err(copy_failed(&reason))
        }
    }
}

/// The two steps that fill a new copy. The error is the plain reason, cleaned later by `copy_failed`.
fn fill(
    copy: &WorktreeRef,
    source_cwd: &Path,
    pane: &str,
    checkpoint_id: &str,
    commit_message: &str,
    git_env: &[(String, String)],
) -> Result<(ExportReport, Option<String>), String> {
    // The default limits (files over 20 MB are left out, as in every save point).
    let export = export_into(source_cwd, pane, checkpoint_id, Path::new(&copy.cwd), &Default::default())
        .map_err(|e: CheckpointError| e.to_string())?;
    let start_commit = worktrees::commit_start(copy, commit_message, git_env).map_err(|e| e.message)?;
    Ok((export, start_commit))
}

/// The branch a new copy starts from when the source pane works in a copy: that copy's branch, so the
/// new copy also holds the source copy's commits. `None` when the source is a folder that is not a
/// copy (the origin's HEAD is used) or when that copy is gone.
pub fn start_point_for(source_cwd: &str) -> Option<String> {
    let origin = worktrees::origin_of_cwd(source_cwd)?;
    let here = comparable(source_cwd);
    worktrees::list(&origin)
        .into_iter()
        .find(|entry| entry.exists && entry.worktree.closed.is_none() && covers(&comparable(&entry.worktree.path), &here))
        .map(|entry| entry.worktree.branch)
}

/// A folder path in one spelling for comparing: forward slashes, no trailing slash, lower case.
fn comparable(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// True when `folder` is `root` or lies inside it.
fn covers(root: &str, folder: &str) -> bool {
    folder == root || folder.starts_with(&format!("{root}/"))
}

/// The error for a copy that could not be made. Git's reason has the home folder shown as `~`.
fn copy_failed(reason: &str) -> OperationError {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok();
    let cleaned = crate::session_chat::home_to_tilde(reason.trim(), home.as_deref());
    OperationError::new(
        ERR_BRANCH_COPY_FAILED,
        MSG_BRANCH_COPY_FAILED.replace("<git reason, cleaned>", &cleaned),
        true,
    )
}
