//! Which diff a reviewer sees. Best source first:
//!
//! 1. the pane or run's isolated git worktree (everything it changed, `worktree_diff`),
//! 2. the last turn's checkpoint diff ("before turn n" against "after turn n"),
//! 3. `git diff HEAD` of the pane's folder.
//!
//! The choice is a pure function over [`Sources`], so it is tested with fakes. The size cap is
//! applied here, once, whatever the source.

use super::{ReviewDiff, SourceKind};
use crate::operations::OperationError;

/// The patch handed to a reviewer is cut to this many bytes (about 12k tokens).
pub const MAX_REVIEW_PATCH_BYTES: usize = 48 * 1024;

/// A patch a source produced, before the review cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDiff {
    pub patch: String,
    /// The source itself already cut it.
    pub truncated: bool,
}

/// The three places a diff can come from. `None` = this source has nothing (not applicable,
/// unavailable or no changes), so the next one is tried.
pub trait Sources {
    fn worktree(&self) -> Option<RawDiff>;
    fn checkpoint(&self) -> Option<RawDiff>;
    fn git(&self) -> Option<RawDiff>;
}

pub fn no_changes() -> OperationError {
    OperationError::new(
        "NOTHING_TO_REVIEW",
        "There is nothing to review yet: no changes were found for this agent.",
        false,
    )
}

/// First non-empty source, capped. Errors with `NOTHING_TO_REVIEW` when all three are empty.
pub fn select(sources: &dyn Sources) -> Result<ReviewDiff, OperationError> {
    let attempts: [(SourceKind, &dyn Fn() -> Option<RawDiff>); 3] = [
        (SourceKind::Worktree, &|| sources.worktree()),
        (SourceKind::Checkpoint, &|| sources.checkpoint()),
        (SourceKind::Git, &|| sources.git()),
    ];
    for (kind, fetch) in attempts {
        if let Some(raw) = fetch().filter(|raw| !raw.patch.trim().is_empty()) {
            return Ok(build(kind, raw, MAX_REVIEW_PATCH_BYTES));
        }
    }
    Err(no_changes())
}

/// Apply the cap and count what is left.
pub fn build(source: SourceKind, raw: RawDiff, max_bytes: usize) -> ReviewDiff {
    let (patch, cut) = cap_patch(&raw.patch, max_bytes);
    let (files, additions, deletions) = count(&patch);
    ReviewDiff { source, patch, truncated: raw.truncated || cut, files, additions, deletions }
}

/// Cut a patch at `max` bytes, preferring a file boundary (`diff --git`) so the reviewer never
/// gets half a file header; a single huge file is cut at a line end.
pub fn cap_patch(patch: &str, max: usize) -> (String, bool) {
    if patch.len() <= max {
        return (patch.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !patch.is_char_boundary(end) {
        end -= 1;
    }
    let head = &patch[..end];
    let cut_at = match head.rfind("\ndiff --git ") {
        Some(at) if at > 0 => at + 1,
        _ => head.rfind('\n').map(|at| at + 1).unwrap_or(end),
    };
    (patch[..cut_at].to_string(), true)
}

/// `(files, additions, deletions)` of a unified diff.
pub fn count(patch: &str) -> (usize, u32, u32) {
    let (mut files, mut add, mut del) = (0usize, 0u32, 0u32);
    let mut in_body = false;
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            files += 1;
            in_body = false;
        } else if line.starts_with("@@") {
            in_body = true;
        } else if in_body {
            if line.starts_with('+') {
                add += 1;
            } else if line.starts_with('-') {
                del += 1;
            }
        }
    }
    (files, add, del)
}

// ---------------------------------------------------------------- git

/// Run `git` in `dir` and read at most `max` bytes of stdout (the child is killed at the cap).
/// `None` when git is missing or exits non-zero (not a repo, no commits yet).
pub fn run_git_capped(dir: &std::path::Path, args: &[&str], max: usize) -> Option<RawDiff> {
    use std::io::Read;
    use std::process::Stdio;
    let mut cmd = crate::proc_util::hidden_command("git");
    cmd.arg("-c").arg("core.quotepath=false").args(args);
    cmd.current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut buf = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(max as u64 + 1).read_to_end(&mut buf);
    }
    let truncated = buf.len() > max;
    if truncated {
        let _ = child.kill();
        buf.truncate(max);
    }
    let status = child.wait().ok()?;
    if !truncated && !status.success() {
        return None;
    }
    let mut patch = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        // The lossy conversion may have ended on a replacement char from a split sequence.
        while patch.ends_with('\u{fffd}') {
            patch.pop();
        }
    }
    Some(RawDiff { patch, truncated })
}

/// Uncommitted changes of tracked files in `dir` against HEAD.
pub fn git_diff_head(dir: &std::path::Path) -> Option<RawDiff> {
    run_git_capped(dir, &["diff", "HEAD", "--no-color", "--no-renames"], MAX_REVIEW_PATCH_BYTES * 2)
}
