//! Worktree-isolated runs: every subagent can work in its own `git worktree` on its own branch, so
//! parallel agents never clobber each other's files. The user (or an orchestrator) then reviews the
//! diff and merges or discards it.
//!
//! This module is a filesystem/git concern only. It knows nothing about agents, so it serves every
//! agent type and every MCP host. It drives the `git` CLI (like the rest of the codebase): `gix` is
//! MIT/Apache-2.0 but has no complete `worktree add` / `merge --no-ff` / abort story yet, and
//! `git2` links a C libgit2 build that is slow and fragile on Windows. The CLI is always present
//! where an agent CLI is, and it is exactly what the developer would run by hand.
//!
//! Layout, per origin project `P`:
//! - `P/.puppet-master/worktrees/<short>/`      the worktree (short paths: Windows MAX_PATH)
//! - `P/.puppet-master/worktrees/<short>.json`  its [`WorktreeRef`] (survives restarts)
//! - branch `pm/<handle>`, based on the origin's HEAD at creation time
//!
//! `.puppet-master/worktrees/` is added to the repo's `.git/info/exclude`, never to the user's
//! `.gitignore`.
//!
//! Safety rules this module enforces:
//! - un-merged work is never deleted implicitly; only [`discard`] (explicit) and a successful
//!   [`merge`] remove a worktree;
//! - [`merge`] refuses when the origin has uncommitted changes to the same files, and a conflicting
//!   merge is aborted so the origin is never left mid-merge.

mod git;
pub mod folder;
pub mod harden;
pub mod pr;
pub mod preset;
pub mod proc;
pub mod routes;
pub mod service;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ship;
#[cfg(test)]
mod tests_issue;
#[cfg(test)]
mod tests_branch;

use crate::operations::OperationError;
use git::{git_ok, git_raw, git_limited, GitOut};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Relative to the origin project.
pub const WORKTREES_DIR: &str = ".puppet-master/worktrees";
/// Cap of the unified patch returned to a viewer.
pub const MAX_PATCH_BYTES: usize = 256 * 1024;
/// Cap of files listed in a [`Changes`].
pub const MAX_LISTED_FILES: usize = 500;

/// A worktree created for one agent run handle. Persisted next to the worktree and on the
/// operation's [`WorkerPersist`](crate::operations::WorkerPersist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRef {
    /// Root of the worktree checkout.
    pub path: String,
    /// Where the agent runs: `path` joined with the origin project's offset inside its repo.
    pub cwd: String,
    pub branch: String,
    /// Branch the origin was on when the worktree was created (`HEAD` when detached).
    pub base: String,
    pub base_commit: String,
    /// The origin project this worktree belongs to (also the run's project key).
    pub repo: String,
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub created_at_ms: u64,
    /// `merged` or `discarded` once reviewed; the checkout is gone then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<String>,
    /// Changed-files summary recorded when the run finished (receipt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<ChangeSummary>,
    /// What preparing the copy did (files copied in, setup steps): see [`preset`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<preset::SetupReport>,
    /// The pull request made from this branch and the last result of its checks: see [`pr`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<pr::PrRef>,
}

impl WorktreeRef {
    /// Directory the agent works in, or `None` once the worktree was merged or discarded.
    pub fn active_cwd(&self) -> Option<&str> {
        self.closed.is_none().then_some(self.cwd.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub files_changed: usize,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// `added` | `modified` | `deleted`
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Changes {
    pub files: Vec<FileChange>,
    pub files_changed: usize,
    pub additions: u32,
    pub deletions: u32,
    /// More files changed than [`MAX_LISTED_FILES`]; `files` is cut, the totals are not.
    pub truncated: bool,
}

impl Changes {
    pub fn summary(&self) -> ChangeSummary {
        ChangeSummary {
            files_changed: self.files_changed,
            additions: self.additions,
            deletions: self.deletions,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffView {
    pub branch: String,
    pub base: String,
    pub changes: Changes,
    /// Unified diff of every change (committed and uncommitted) against the base commit.
    pub patch: String,
    pub patch_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeOutcome {
    pub merged: bool,
    /// Why nothing was merged (`no_changes`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub merge_commit: Option<String>,
    pub files: Vec<String>,
    pub branch: String,
    /// The origin branch that received the merge.
    pub into: String,
}

/// A worktree found on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeEntry {
    #[serde(flatten)]
    pub worktree: WorktreeRef,
    /// The checkout directory still exists.
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SweepReport {
    /// Stale git registrations pruned.
    pub pruned: usize,
    /// Directories under the worktrees folder that were neither registered nor described.
    pub removed_dirs: Vec<String>,
    /// Descriptions whose checkout vanished (the branch is kept when it holds commits).
    pub removed_meta: Vec<String>,
}

fn err(code: &str, message: impl Into<String>) -> OperationError {
    OperationError::new(code, message, false)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lowercase `[a-z0-9-]`, collapsed, at most `max` chars; never empty.
fn slug(input: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed: String = out.trim_matches('-').chars().take(max).collect();
    let trimmed = trimmed.trim_end_matches('-').to_string();
    if trimmed.is_empty() {
        "run".into()
    } else {
        trimmed
    }
}

fn fnv16(input: &str) -> u16 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in input.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    ((hash >> 16) ^ (hash & 0xffff)) as u16
}

fn worktrees_root(project: &Path) -> PathBuf {
    project.join(".puppet-master").join("worktrees")
}

fn meta_path(project: &Path, dir_name: &str) -> PathBuf {
    worktrees_root(project).join(format!("{dir_name}.json"))
}

/// Facts about the origin's repository.
struct RepoInfo {
    /// `""` or `sub/dir/`: the project's offset inside the repo.
    prefix: String,
    head: String,
    branch: String,
    exclude_file: PathBuf,
}

fn repo_info(project: &Path) -> Result<RepoInfo, OperationError> {
    let probe = git_raw(project, &["rev-parse", "--is-inside-work-tree"], &[])?;
    if !probe.ok || probe.stdout.trim() != "true" {
        return Err(err(
            "NOT_A_GIT_REPO",
            format!(
                "{} is not inside a git repository. Isolation needs git: run `git init` and make a first commit, or run without isolation.",
                project.display()
            ),
        ));
    }
    let prefix = git_ok(project, &["rev-parse", "--show-prefix"], "GIT_FAILED")?;
    let head = git_raw(project, &["rev-parse", "--verify", "-q", "HEAD"], &[])?;
    if !head.ok || head.stdout.trim().is_empty() {
        return Err(err(
            "NO_COMMITS",
            "the repository has no commits yet; make a first commit so an isolated worktree has something to start from",
        ));
    }
    let branch = git_raw(project, &["symbolic-ref", "-q", "--short", "HEAD"], &[])?;
    let branch = if branch.ok && !branch.stdout.trim().is_empty() {
        branch.stdout.trim().to_string()
    } else {
        "HEAD".into()
    };
    let exclude = git_ok(
        project,
        &["rev-parse", "--git-path", "info/exclude"],
        "GIT_FAILED",
    )?;
    let exclude_file = {
        let candidate = PathBuf::from(&exclude);
        if candidate.is_absolute() {
            candidate
        } else {
            project.join(candidate)
        }
    };
    Ok(RepoInfo {
        prefix,
        head: head.stdout.trim().to_string(),
        branch,
        exclude_file,
    })
}

/// Ignore the worktrees folder for this repo only (`.git/info/exclude`), never the user's
/// `.gitignore`. Best effort: failing to write only makes `git status` noisier.
fn ensure_excluded(info: &RepoInfo) {
    let line = ".puppet-master/worktrees/";
    let existing = std::fs::read_to_string(&info.exclude_file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == line) {
        return;
    }
    if let Some(parent) = info.exclude_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(line);
    next.push('\n');
    if let Err(error) = std::fs::write(&info.exclude_file, next) {
        tracing::warn!(%error, file = %info.exclude_file.display(), "could not exclude .puppet-master/worktrees");
    }
}

fn branch_exists(project: &Path, branch: &str) -> bool {
    git_raw(
        project,
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
        &[],
    )
    .map(|out| out.ok)
    .unwrap_or(false)
}

fn read_meta(path: &Path) -> Option<WorktreeRef> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_meta(project: &Path, dir_name: &str, reference: &WorktreeRef) -> Result<(), OperationError> {
    let path = meta_path(project, dir_name);
    let json = serde_json::to_vec_pretty(reference)
        .map_err(|error| err("WORKTREE_CREATE_FAILED", error.to_string()))?;
    std::fs::write(&path, json).map_err(|error| {
        err(
            "WORKTREE_CREATE_FAILED",
            format!("could not write {}: {error}", path.display()),
        )
    })
}

fn dir_name_of(reference: &WorktreeRef) -> String {
    Path::new(&reference.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The worktree already created for `handle` in `project`, if its checkout still exists.
pub fn find_by_handle(project: &str, handle: &str) -> Option<WorktreeRef> {
    list(project)
        .into_iter()
        .find(|entry| entry.worktree.handle == handle)
        .map(|entry| entry.worktree)
}

/// The agent will start in the copy's folder (the project's offset inside the repo included).
/// Refuse a copy Windows could not start a program in (`PATH_TOO_LONG`) before anything is made
/// for it.
fn check_copy_fits(root: &Path, dir_name: &str, prefix: &str) -> Result<(), OperationError> {
    let planned = root.join(dir_name);
    let planned = if prefix.is_empty() {
        planned
    } else {
        planned.join(prefix.trim_end_matches('/'))
    };
    crate::path_limit::check_error(&planned)
}

/// How [`create_with`] makes a copy. The default is what [`create`] has always done.
#[derive(Debug, Clone)]
pub struct CreateOptions {
    /// The commit to start from (for example `origin/main`). `None` = the origin's HEAD.
    pub start_point: Option<String>,
    /// `false` skips the preset's `copy` list (env files and the like). Setup steps still run.
    pub copy_files: bool,
    /// Environment for every git call this creation makes (see [`harden::git_env`]).
    pub git_env: Vec<(String, String)>,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self { start_point: None, copy_files: true, git_env: Vec::new() }
    }
}

/// Create `<project>/.puppet-master/worktrees/<short>` on a new branch `pm/<handle>` from the
/// origin's current HEAD. Idempotent per handle: a retry gets the same worktree back.
///
/// Uncommitted changes in the origin are not in the new worktree (it starts from `HEAD`).
pub fn create(project: &str, handle: &str) -> Result<WorktreeRef, OperationError> {
    create_with(project, handle, &CreateOptions::default())
}

/// [`create`] with options: a start point, no copy of the preset's files, and the environment
/// for its git calls.
pub fn create_with(
    project: &str,
    handle: &str,
    opts: &CreateOptions,
) -> Result<WorktreeRef, OperationError> {
    let project_path = PathBuf::from(project);
    let info = repo_info(&project_path)?;
    if let Some(existing) = find_by_handle(project, handle).filter(|w| w.closed.is_none()) {
        if Path::new(&existing.path).is_dir() {
            return Ok(existing);
        }
    }
    // The commit the copy starts from. A named start point is resolved here, so the record says
    // which commit the diff is measured against.
    let (start, base_commit) = match &opts.start_point {
        Some(point) => {
            harden::check_name(point)?;
            let commit = git_ok(
                &project_path,
                &["rev-parse", "--verify", "-q", &format!("{point}^{{commit}}")],
                "GIT_FAILED",
            )?;
            (point.clone(), commit)
        }
        None => (info.head.clone(), info.head.clone()),
    };
    let root = worktrees_root(&project_path);
    let base_name = format!("{}-{:04x}", slug(handle, 10), fnv16(handle));
    check_copy_fits(&root, &base_name, &info.prefix)?;
    ensure_excluded(&info);
    crate::git_exclude::ensure_project_storage_excluded(&project_path);
    std::fs::create_dir_all(&root).map_err(|error| {
        err(
            "WORKTREE_CREATE_FAILED",
            format!("could not create {}: {error}", root.display()),
        )
    })?;
    let mut dir_name = base_name.clone();
    let mut dir = root.join(&dir_name);
    let mut suffix = 1;
    while dir.exists() {
        suffix += 1;
        dir_name = format!("{base_name}-{suffix}");
        dir = root.join(&dir_name);
    }
    let base_branch = format!("pm/{}", slug(handle, 40));
    let mut branch = base_branch.clone();
    let mut n = 1;
    while branch_exists(&project_path, &branch) {
        n += 1;
        branch = format!("{base_branch}-{n}");
    }
    let dir_str = dir.to_string_lossy().into_owned();
    // Checking out can run filter drivers from the repo's own config, so the hardened env also
    // blanks those (see harden::with_filters_off).
    let add_env = harden::with_filters_off(&project_path, &opts.git_env)?;
    let created = git_raw(
        &project_path,
        &[
            "-c",
            "core.longpaths=true",
            "worktree",
            "add",
            "-b",
            &branch,
            &dir_str,
            &start,
        ],
        &harden::refs(&add_env),
    )?;
    if !created.ok {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(err(
            "WORKTREE_CREATE_FAILED",
            format!("git worktree add failed: {}", created.stderr.trim()),
        ));
    }
    let cwd = if info.prefix.is_empty() {
        dir.clone()
    } else {
        let nested = dir.join(info.prefix.trim_end_matches('/'));
        let _ = std::fs::create_dir_all(&nested);
        nested
    };
    let reference = WorktreeRef {
        path: dir_str,
        cwd: cwd.to_string_lossy().into_owned(),
        branch,
        base: info.branch,
        base_commit,
        repo: project.to_string(),
        handle: handle.to_string(),
        created_at_ms: now_ms(),
        closed: None,
        changes: None,
        setup: None,
        pr: None,
    };
    write_meta(&project_path, &dir_name, &reference)?;
    // A project that declared a preset gets a copy that starts ready. The result (good or bad) is
    // recorded; a failing setup never deletes the copy.
    let mut reference = reference;
    match preset::load(&project_path) {
        Ok(mut wanted) => {
            // An issue copy skips the preset's files (`.env` and the like); its setup still runs.
            if !opts.copy_files {
                wanted.copy.clear();
            }
            if !wanted.is_empty() {
                let log_file = worktrees_root(&project_path).join(format!("{dir_name}.setup.log"));
                let _ = std::fs::remove_file(&log_file);
                let env = proc::ToolEnv::default();
                reference.setup = preset::apply(
                    &preset::Target {
                        project: &project_path,
                        cwd: Path::new(&reference.cwd),
                        prefix: &info.prefix,
                        exclude_file: &info.exclude_file,
                        log_file: &log_file,
                        env: &env,
                    },
                    &wanted,
                );
                write_meta(&project_path, &dir_name, &reference)?;
            }
        }
        Err(error) => tracing::warn!(%error, project, "ignoring an unreadable .puppet-master/project.toml"),
    }
    Ok(reference)
}

/// Every described worktree of `project` (checkout present or not), oldest first.
pub fn list(project: &str) -> Vec<WorktreeEntry> {
    let root = worktrees_root(Path::new(project));
    let Ok(read) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut entries: Vec<WorktreeEntry> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| read_meta(&path))
        .map(|worktree| WorktreeEntry {
            exists: Path::new(&worktree.path).is_dir(),
            worktree,
        })
        .collect();
    entries.sort_by_key(|entry| entry.worktree.created_at_ms);
    entries
}

fn require_checkout(reference: &WorktreeRef) -> Result<PathBuf, OperationError> {
    let path = PathBuf::from(&reference.path);
    if reference.closed.is_some() || !path.is_dir() {
        return Err(err(
            "WORKTREE_NOT_FOUND",
            format!(
                "the worktree for {} is gone ({})",
                reference.handle,
                reference.closed.as_deref().unwrap_or("directory missing")
            ),
        ));
    }
    Ok(path)
}

/// Pathspec that keeps Puppet Master's own data (`.puppet-master/`: the checkpoint store, turn
/// summaries, operation records) out of everything a worktree stages, whether or not the repo
/// ignores it.
///
/// Not `:(exclude).puppet-master`: git's `add` treats an exclude pathspec that NAMES an ignored
/// directory as if the caller had asked to add it, and fails with "The following paths are ignored
/// by one of your .gitignore files: .puppet-master" (live: every isolated run lost its file list,
/// Merge and Discard). Only a pattern that does not name the directory itself is left alone, so
/// this one matches what is inside it, in any `.puppet-master` folder.
const INTERNAL_PATHSPEC: &str = ":(exclude,glob)**/.puppet-master/**";

/// `git add -A` of the worktree except Puppet Master's own data, into the index `env` selects (the
/// worktree's real one when empty).
fn stage_all_but_internal(wt: &Path, env: &[(&str, &std::ffi::OsStr)]) -> Result<(), OperationError> {
    let add = git_raw(wt, &["add", "-A", "--", ".", INTERNAL_PATHSPEC], env)?;
    if add.ok {
        Ok(())
    } else {
        Err(err("GIT_FAILED", format!("git add failed: {}", add.stderr.trim())))
    }
}

/// Snapshot the worktree's full state (committed + uncommitted + untracked, honouring ignore
/// rules) as a tree object, through a temporary copy of the index: the agent's own index is never
/// touched.
fn snapshot_tree(wt: &Path, env: &[(String, String)]) -> Result<String, OperationError> {
    let git_dir = PathBuf::from(git_ok(wt, &["rev-parse", "--absolute-git-dir"], "GIT_FAILED")?);
    let tmp = git_dir.join(format!(
        "pm-index-{}-{}",
        std::process::id(),
        now_ms() ^ u64::from(fnv16(&wt.to_string_lossy()))
    ));
    let real = git_dir.join("index");
    if real.is_file() {
        std::fs::copy(&real, &tmp).map_err(|error| err("GIT_FAILED", error.to_string()))?;
    }
    let mut env_refs = harden::refs(env);
    env_refs.push(("GIT_INDEX_FILE", tmp.as_os_str()));
    let result = (|| {
        stage_all_but_internal(wt, &env_refs)?;
        let tree = git_raw(wt, &["write-tree"], &env_refs)?;
        if !tree.ok {
            return Err(err("GIT_FAILED", format!("git write-tree failed: {}", tree.stderr.trim())));
        }
        Ok(tree.stdout.trim().to_string())
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

fn parse_changes(numstat: &str, name_status: &str) -> Changes {
    let mut statuses = std::collections::HashMap::new();
    let mut parts = name_status.split('\0').filter(|p| !p.is_empty());
    while let (Some(code), Some(path)) = (parts.next(), parts.next()) {
        let status = match code.chars().next() {
            Some('A') => "added",
            Some('D') => "deleted",
            _ => "modified",
        };
        statuses.insert(path.to_string(), status);
    }
    let mut changes = Changes::default();
    for record in numstat.split('\0').filter(|r| !r.is_empty()) {
        let mut fields = record.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let binary = add == "-" || del == "-";
        let additions = add.parse::<u32>().unwrap_or(0);
        let deletions = del.parse::<u32>().unwrap_or(0);
        changes.files_changed += 1;
        changes.additions += additions;
        changes.deletions += deletions;
        if changes.files.len() < MAX_LISTED_FILES {
            changes.files.push(FileChange {
                path: path.to_string(),
                status: statuses.get(path).copied().unwrap_or("modified").to_string(),
                additions,
                deletions,
                binary,
            });
        } else {
            changes.truncated = true;
        }
    }
    changes.files.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// Files changed in the worktree against its base commit (committed or not).
pub fn changes(reference: &WorktreeRef) -> Result<Changes, OperationError> {
    changes_with(reference, &[])
}

/// [`changes`] with the environment of PM's own git calls on the copy (see [`harden::git_env`]):
/// the staging step runs with blank filter drivers and no fsmonitor.
pub fn changes_with(reference: &WorktreeRef, env: &[(String, String)]) -> Result<Changes, OperationError> {
    let wt = require_checkout(reference)?;
    let env = harden::with_filters_off(&wt, env)?;
    let tree = snapshot_tree(&wt, &env)?;
    let numstat = git_ok_raw(&wt, &["diff", "--numstat", "-z", "--no-renames", &reference.base_commit, &tree])?;
    let names = git_ok_raw(&wt, &["diff", "--name-status", "-z", "--no-renames", &reference.base_commit, &tree])?;
    Ok(parse_changes(&numstat, &names))
}

fn git_ok_raw(dir: &Path, args: &[&str]) -> Result<String, OperationError> {
    let out: GitOut = git_raw(dir, args, &[])?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(err(
            "GIT_FAILED",
            format!("git {} failed: {}", args.first().unwrap_or(&""), out.stderr.trim()),
        ))
    }
}

/// Stat plus the unified patch (capped at [`MAX_PATCH_BYTES`]); `path` narrows the patch to one file.
pub fn diff(reference: &WorktreeRef, path: Option<&str>) -> Result<DiffView, OperationError> {
    let wt = require_checkout(reference)?;
    let tree = snapshot_tree(&wt, &[])?;
    let numstat = git_ok_raw(&wt, &["diff", "--numstat", "-z", "--no-renames", &reference.base_commit, &tree])?;
    let names = git_ok_raw(&wt, &["diff", "--name-status", "-z", "--no-renames", &reference.base_commit, &tree])?;
    let mut args = vec![
        "diff".to_string(),
        "--no-color".into(),
        "--no-renames".into(),
        reference.base_commit.clone(),
        tree,
    ];
    if let Some(path) = path.filter(|p| !p.is_empty()) {
        args.push("--".into());
        args.push(path.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (patch, truncated) = git_limited(&wt, &arg_refs, MAX_PATCH_BYTES)?;
    Ok(DiffView {
        branch: reference.branch.clone(),
        base: reference.base.clone(),
        changes: parse_changes(&numstat, &names),
        patch,
        patch_truncated: truncated,
    })
}

fn porcelain_paths(status_z: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let mut parts = status_z.split('\0').filter(|p| !p.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        paths.insert(path.to_string());
        // Renames and copies carry the origin path as the next record.
        if code.starts_with('R') || code.starts_with('C') || code[1..2].contains(['R', 'C']) {
            if let Some(from) = parts.next() {
                paths.insert(from.to_string());
            }
        }
    }
    paths
}

fn is_internal(path: &str) -> bool {
    path == ".puppet-master" || path.starts_with(".puppet-master/")
}

/// Merge the worktree's branch into the origin's current branch with `--no-ff`.
///
/// Refuses (changing nothing) when the origin is detached, is mid-merge, or has uncommitted
/// changes to files the worktree also changed. A merge that conflicts is aborted: the origin is
/// restored to exactly where it was and the conflicted files are reported. On success the worktree
/// and its branch are removed.
pub fn merge(reference: &WorktreeRef) -> Result<MergeOutcome, OperationError> {
    let outcome = merge_keep(reference)?;
    if outcome.merged {
        remove_checkout(reference, true)?;
    }
    Ok(outcome)
}

/// [`merge`] without removing the checkout, for a caller that must first stop processes whose
/// working directory is inside it (Windows cannot delete a directory in use). Follow a
/// `merged: true` outcome with [`cleanup_merged`].
pub fn merge_keep(reference: &WorktreeRef) -> Result<MergeOutcome, OperationError> {
    let wt = require_checkout(reference)?;
    let origin = PathBuf::from(&reference.repo);
    let in_progress = git_raw(&origin, &["rev-parse", "-q", "--verify", "MERGE_HEAD"], &[])?;
    if in_progress.ok {
        return Err(err(
            "ORIGIN_MERGE_IN_PROGRESS",
            "the project is in the middle of a merge; finish or abort it first",
        ));
    }
    let into = git_raw(&origin, &["symbolic-ref", "-q", "--short", "HEAD"], &[])?;
    if !into.ok || into.stdout.trim().is_empty() {
        return Err(err(
            "ORIGIN_DETACHED",
            "the project is on a detached HEAD; check out a branch to merge into",
        ));
    }
    let into = into.stdout.trim().to_string();
    let changed = changes(reference)?;
    if changed.files_changed == 0 {
        return Ok(MergeOutcome {
            merged: false,
            reason: Some("no_changes".into()),
            merge_commit: None,
            files: Vec::new(),
            branch: reference.branch.clone(),
            into,
        });
    }
    // Uncommitted work in the origin that touches the same files would be mixed into (or lost
    // to) the merge: stop before anything moves.
    let dirty = git_ok_raw(
        &origin,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let dirty = porcelain_paths(&dirty);
    let overlap: Vec<String> = changed
        .files
        .iter()
        .map(|f| f.path.clone())
        .filter(|path| !is_internal(path) && dirty.contains(path))
        .collect();
    if !overlap.is_empty() {
        let mut error = err(
            "ORIGIN_DIRTY_OVERLAP",
            format!(
                "the project has uncommitted changes to {} file(s) this worktree also changed; commit or stash them first",
                overlap.len()
            ),
        );
        error.context = serde_json::json!({ "files": overlap });
        return Err(error);
    }
    // Record the agent's work on its branch (it normally leaves it uncommitted).
    commit_all(&wt, &format!("Puppet Master: changes from {}", reference.handle), &[])?;
    let head_before = git_ok(&origin, &["rev-parse", "HEAD"], "GIT_FAILED")?;
    // The person's git history uses the app's words: a "separate copy", never an "isolated run".
    let message = format!("Merge separate copy {} (Puppet Master, {})", reference.branch, reference.handle);
    let merged = git_raw(
        &origin,
        &identity_args(&origin, &["merge", "--no-ff", "--no-edit", "-m", &message, &reference.branch]),
        &[("GIT_MERGE_AUTOEDIT", std::ffi::OsStr::new("no"))],
    )?;
    if !merged.ok {
        let conflicted = git_raw(&origin, &["diff", "--name-only", "--diff-filter=U", "-z"], &[])?;
        let files: Vec<String> = conflicted
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        let merging = git_raw(&origin, &["rev-parse", "-q", "--verify", "MERGE_HEAD"], &[])?.ok;
        if merging {
            let abort = git_raw(&origin, &["merge", "--abort"], &[])?;
            if !abort.ok {
                let _ = git_raw(&origin, &["reset", "--merge"], &[]);
            }
        }
        let head_after = git_ok(&origin, &["rev-parse", "HEAD"], "GIT_FAILED")?;
        if head_after != head_before {
            return Err(err(
                "ORIGIN_NOT_RESTORED",
                format!(
                    "the merge failed and the project is at {head_after}, not {head_before}; run `git reset --merge` and check it"
                ),
            ));
        }
        if !files.is_empty() {
            let mut error = err(
                "MERGE_CONFLICT",
                format!(
                    "{} file(s) conflict with {into}; nothing was merged and the project is unchanged. The work is safe on branch {}",
                    files.len(),
                    reference.branch
                ),
            );
            error.context = serde_json::json!({ "files": files, "branch": reference.branch, "into": into });
            return Err(error);
        }
        let mut error = err(
            "MERGE_FAILED",
            format!("git merge failed: {}", merged.stderr.trim()),
        );
        error.context = serde_json::json!({ "branch": reference.branch, "into": into });
        return Err(error);
    }
    let merge_commit = git_ok(&origin, &["rev-parse", "HEAD"], "GIT_FAILED").ok();
    Ok(MergeOutcome {
        merged: true,
        reason: None,
        merge_commit,
        files: changed.files.into_iter().map(|f| f.path).collect(),
        branch: reference.branch.clone(),
        into,
    })
}

/// `-c user.name/-c user.email` only when the repo has no identity configured, then `args`.
fn identity_args<'a>(dir: &Path, args: &[&'a str]) -> Vec<&'a str> {
    let has = |key: &str| {
        git_raw(dir, &["config", "--get", key], &[])
            .map(|out| out.ok && !out.stdout.trim().is_empty())
            .unwrap_or(false)
    };
    let mut full: Vec<&'a str> = Vec::new();
    if !has("user.name") {
        full.extend(["-c", "user.name=Puppet Master"]);
    }
    if !has("user.email") {
        full.extend(["-c", "user.email=puppet-master@localhost"]);
    }
    full.extend(args);
    full
}

fn commit_all(wt: &Path, message: &str, env: &[(String, String)]) -> Result<(), OperationError> {
    let env = harden::with_filters_off(wt, env)?;
    let env = harden::refs(&env);
    stage_all_but_internal(wt, &env)?;
    let staged = git_raw(wt, &["diff", "--cached", "--quiet"], &env)?;
    if staged.ok {
        return Ok(()); // nothing staged: the branch already holds everything
    }
    let commit = git_raw(wt, &identity_args(wt, &["commit", "--no-verify", "-m", message]), &env)?;
    if commit.ok {
        Ok(())
    } else {
        Err(err("GIT_FAILED", format!("git commit failed: {}", commit.stderr.trim())))
    }
}

/// Record the agent's loose edits as one commit on the worktree's branch (the same commit a merge
/// makes first). `true` when something was committed. `.puppet-master/` is never included.
pub(crate) fn commit_pending(reference: &WorktreeRef) -> Result<bool, OperationError> {
    commit_pending_with(reference, &[])
}

/// [`commit_pending`] with the environment of PM's own git calls on the copy (see
/// [`harden::git_env`]): no hooks run, not even `post-commit`, and no fsmonitor or filter drivers.
pub fn commit_pending_with(reference: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError> {
    let wt = require_checkout(reference)?;
    let before = git_ok(&wt, &["rev-parse", "HEAD"], "GIT_FAILED")?;
    commit_all(&wt, &format!("Puppet Master: changes from {}", reference.handle), env)?;
    let after = git_ok(&wt, &["rev-parse", "HEAD"], "GIT_FAILED")?;
    Ok(before != after)
}

/// The one start commit of a branch copy (spec n33 §3.2, flow B): the copy's files as they were at
/// the save point, before the new agent does anything. Same staging and hardening as
/// [`commit_pending_with`], with the given one-line `message` (at most 120 characters, no control
/// characters). `.puppet-master/` is never included. Returns the new HEAD, or `None` when nothing
/// differs from HEAD.
pub fn commit_start(
    reference: &WorktreeRef,
    message: &str,
    env: &[(String, String)],
) -> Result<Option<String>, OperationError> {
    let plain = !message.is_empty()
        && message.chars().count() <= 120
        && !message.chars().any(char::is_control);
    if !plain {
        return Err(err("INVALID_ARGUMENT", "the start commit needs one plain line of at most 120 characters"));
    }
    let wt = require_checkout(reference)?;
    let before = git_ok(&wt, &["rev-parse", "HEAD"], "GIT_FAILED")?;
    commit_all(&wt, message, env)?;
    let after = git_ok(&wt, &["rev-parse", "HEAD"], "GIT_FAILED")?;
    Ok((before != after).then_some(after))
}

/// The worktree has changes that are not committed yet (`.puppet-master/` aside).
pub(crate) fn has_uncommitted(reference: &WorktreeRef) -> Result<bool, OperationError> {
    let wt = require_checkout(reference)?;
    let out = git_ok_raw(&wt, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    Ok(porcelain_paths(&out).iter().any(|p| !is_internal(p)))
}

/// Write `reference` back to its description file (after the PR or its checks changed).
pub(crate) fn update_meta(reference: &WorktreeRef) -> Result<(), OperationError> {
    write_meta(Path::new(&reference.repo), &dir_name_of(reference), reference)
}

/// Remove the checkout and branch of a worktree whose branch was merged by [`merge_keep`].
pub fn cleanup_merged(reference: &WorktreeRef) -> Result<(), OperationError> {
    remove_checkout(reference, true)
}

/// The origin project a working directory belongs to, when it lies inside
/// `<project>/.puppet-master/worktrees/<name>/...`.
pub fn origin_of_cwd(cwd: &str) -> Option<String> {
    let normalized = cwd.replace('\\', "/");
    let marker = "/.puppet-master/worktrees/";
    let at = normalized.find(marker)?;
    let origin = &normalized[..at];
    if origin.is_empty() {
        None
    } else {
        // keep the platform spelling the caller used for the prefix
        Some(cwd[..at].to_string())
    }
}

/// Throw the worktree and its branch away. Explicit user/orchestrator action only.
pub fn discard(reference: &WorktreeRef) -> Result<(), OperationError> {
    remove_checkout(reference, false)
}

/// Remove the checkout, the branch (`-d` after a merge, `-D` for a discard) and the description.
fn remove_checkout(reference: &WorktreeRef, merged: bool) -> Result<(), OperationError> {
    let origin = PathBuf::from(&reference.repo);
    let root = worktrees_root(&origin);
    let path = PathBuf::from(&reference.path);
    // Never delete anything outside this project's worktrees folder.
    let inside = crate::project_path::workspace_covers(&root, &path)
        && crate::project_path::comparable_project_path(&root)
            != crate::project_path::comparable_project_path(&path);
    if !inside {
        return Err(err(
            "WORKTREE_PATH_REFUSED",
            format!("{} is not inside {}", path.display(), root.display()),
        ));
    }
    if path.exists() {
        let removed = git_raw(
            &origin,
            &["worktree", "remove", "--force", "--force", &reference.path],
            &[],
        )?;
        if !removed.ok || path.exists() {
            // Locked files on Windows or a broken checkout: fall back to deleting the folder.
            remove_dir_all_retry(&path).map_err(|error| {
                err(
                    "WORKTREE_REMOVE_FAILED",
                    format!("could not remove {}: {error}", path.display()),
                )
            })?;
        }
    }
    let _ = git_raw(&origin, &["worktree", "prune"], &[]);
    if branch_exists(&origin, &reference.branch) {
        let flag = if merged { "-d" } else { "-D" };
        let deleted = git_raw(&origin, &["branch", flag, &reference.branch], &[])?;
        if !deleted.ok && merged {
            let _ = git_raw(&origin, &["branch", "-D", &reference.branch], &[]);
        }
    }
    let dir_name = dir_name_of(reference);
    let _ = std::fs::remove_file(meta_path(&origin, &dir_name));
    // The setup output and a leftover pull request text belong to the copy and go with it.
    for suffix in ["setup.log", "pr-body.md"] {
        let _ = std::fs::remove_file(worktrees_root(&origin).join(format!("{dir_name}.{suffix}")));
    }
    Ok(())
}

fn remove_dir_all_retry(path: &Path) -> std::io::Result<()> {
    let mut last = Ok(());
    for attempt in 0..5 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                // Read-only files (git objects) block deletion on Windows.
                clear_readonly(path);
                last = Err(error);
                std::thread::sleep(std::time::Duration::from_millis(80 * (attempt + 1)));
            }
        }
    }
    last
}

fn clear_readonly(path: &Path) {
    let Ok(read) = std::fs::read_dir(path) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let p = entry.path();
        if let Ok(meta) = std::fs::symlink_metadata(&p) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&p, perms);
            }
            if meta.is_dir() {
                clear_readonly(&p);
            }
        }
    }
}

/// Whether git tracks `relative` (a file path relative to `dir`) in the repository containing `dir`.
/// `false` when `dir` is not in a repository or git is not installed.
pub(crate) fn is_git_tracked(dir: &Path, relative: &str) -> bool {
    git_raw(dir, &["ls-files", "--error-unmatch", "--", relative], &[]).map(|out| out.ok).unwrap_or(false)
}

/// Discard `reference` only if the agent changed nothing in it (a run that never started).
pub fn discard_if_pristine(reference: &WorktreeRef) -> bool {
    match changes(reference) {
        Ok(found) if found.files_changed == 0 => discard(reference).is_ok(),
        _ => false,
    }
}

/// Tidy `project` after a crash or an interrupted create. Never removes work:
/// - prunes git registrations whose checkout is gone;
/// - removes folders under the worktrees directory that git does not know and no description names
///   (a half-created checkout);
/// - drops descriptions whose checkout vanished (the branch stays unless it holds no commits).
pub fn sweep(project: &str) -> SweepReport {
    let mut report = SweepReport::default();
    let project_path = Path::new(project);
    let root = worktrees_root(project_path);
    if !root.is_dir() {
        return report;
    }
    let before = git_raw(project_path, &["worktree", "list", "--porcelain"], &[])
        .map(|o| o.stdout.matches("\nworktree ").count() + usize::from(o.stdout.starts_with("worktree ")))
        .unwrap_or(0);
    let _ = git_raw(project_path, &["worktree", "prune"], &[]);
    let after_list = git_raw(project_path, &["worktree", "list", "--porcelain"], &[])
        .map(|o| o.stdout)
        .unwrap_or_default();
    let after = after_list.matches("\nworktree ").count() + usize::from(after_list.starts_with("worktree "));
    report.pruned = before.saturating_sub(after);
    let registered: BTreeSet<String> = after_list
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|p| crate::project_path::comparable_project_path(Path::new(p)))
        .collect();
    let described: BTreeSet<String> = list(project)
        .iter()
        .map(|entry| crate::project_path::comparable_project_path(Path::new(&entry.worktree.path)))
        .collect();
    let Ok(read) = std::fs::read_dir(&root) else {
        return report;
    };
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            let key = crate::project_path::comparable_project_path(&path);
            if !registered.contains(&key) && !described.contains(&key) {
                if remove_dir_all_retry(&path).is_ok() {
                    report.removed_dirs.push(path.display().to_string());
                }
            }
        }
    }
    for item in list(project) {
        if item.exists {
            continue;
        }
        let reference = &item.worktree;
        let ahead = git_raw(
            project_path,
            &[
                "rev-list",
                "--count",
                &format!("{}..{}", reference.base_commit, reference.branch),
            ],
            &[],
        )
        .ok()
        .and_then(|o| o.stdout.trim().parse::<u32>().ok());
        if branch_exists(project_path, &reference.branch) && ahead == Some(0) {
            let _ = git_raw(project_path, &["branch", "-D", &reference.branch], &[]);
        }
        let _ = std::fs::remove_file(meta_path(project_path, &dir_name_of(reference)));
        report.removed_meta.push(reference.handle.clone());
    }
    report
}
