//! The folder step of the "+" menu: which project the new agent works in.
//!
//! - **Recent projects**: folders the person chose here (most recent first) followed by every
//!   project that ever had a run (the operation index). A folder that was deleted stays listed,
//!   marked `missing`, so the person can remove it from the list.
//! - **Git state** of a folder: `repo`, `no_commits`, `folder` (no Git) or `missing`.
//! - **Set up Git here**: for a folder without Git, `git init` plus a first snapshot. The plan
//!   ([`init_plan`]) lists exactly what will run and how many files it will include, and is shown
//!   before anything runs ([`init`]).
//!
//! The recents list is a small JSON file in the app data folder; every function takes that folder
//! so tests use a temporary one.

use super::git::git_raw;
use super::proc::tail_lines;
use super::{err, identity_args};
use crate::operations::OperationError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "recent-projects.json";
const KEEP: usize = 12;
/// Files counted before a folder is called too big for a first snapshot.
pub const MAX_SNAPSHOT_FILES: usize = 20_000;
const MAX_DEPTH: usize = 14;

/// Lines of the `.gitignore` written when the folder has none (shown in the plan).
pub const DEFAULT_GITIGNORE: &[&str] = &[
    "node_modules/",
    "target/",
    "dist/",
    "build/",
    ".venv/",
    "__pycache__/",
    ".env",
    ".env.*",
    "!.env.example",
    ".DS_Store",
    "Thumbs.db",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: String,
    /// The folder's own name.
    pub name: String,
    /// `repo`, `no_commits`, `folder` or `missing`.
    pub state: String,
    pub last_used_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stored {
    path: String,
    last_used_ms: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn key(path: &str) -> String {
    crate::project_path::comparable_project_path(Path::new(path))
}

/// The folder a person would call the project: a path inside a separate copy
/// (`<project>/.puppet-master/worktrees/<id>`) maps back to the project it came from; anything
/// else is returned as it is (trimmed). A copy is never a project, so no list of projects holds one.
pub fn real_folder(text: &str) -> String {
    let text = text.trim();
    crate::worktrees::origin_of_cwd(text).unwrap_or_else(|| text.to_string())
}

fn load(store: &Path) -> Vec<Stored> {
    std::fs::read_to_string(store.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(store: &Path, items: &[Stored]) {
    if std::fs::create_dir_all(store).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(store.join(FILE), text);
    }
}

/// Put `project` at the top of the recents.
pub fn remember(store: &Path, project: &str) {
    let project = real_folder(project);
    let project = project.as_str();
    if project.is_empty() {
        return;
    }
    let mut items = load(store);
    items.retain(|item| key(&item.path) != key(project));
    items.insert(0, Stored { path: project.to_string(), last_used_ms: now_ms() });
    items.truncate(KEEP * 2);
    save(store, &items);
}

/// Take `project` off the list (the folder itself is never touched).
pub fn forget(store: &Path, project: &str) {
    let project = real_folder(project);
    let project = project.as_str();
    let mut items = load(store);
    items.retain(|item| key(&item.path) != key(project));
    save(store, &items);
}

fn name_of(path: &str) -> String {
    Path::new(path.trim_end_matches(['/', '\\']))
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.to_string())
}

/// `repo` (inside a work tree with a first commit), `no_commits`, `folder` (no Git) or `missing`.
pub fn git_state(path: &Path) -> &'static str {
    if !path.is_dir() {
        return "missing";
    }
    let inside = git_raw(path, &["rev-parse", "--is-inside-work-tree"], &[])
        .map(|out| out.ok && out.stdout.trim() == "true")
        .unwrap_or(false);
    if !inside {
        return "folder";
    }
    let has_head = git_raw(path, &["rev-parse", "--verify", "-q", "HEAD"], &[]).map(|out| out.ok).unwrap_or(false);
    if has_head {
        "repo"
    } else {
        "no_commits"
    }
}

/// The recents shown in the menu: the person's own list first, then `others` (projects that had
/// runs, the default project), each once.
pub fn recent(store: &Path, others: &[String], limit: usize) -> Vec<RecentProject> {
    recent_with(store, others, limit, git_state)
}

/// Like [`recent`], but asks Git nothing: `state` is only `folder` or `missing`. For callers that need the names and
/// paths quickly (a helper request is answered while a brain waits) and do not show the Git state.
pub fn recent_light(store: &Path, others: &[String], limit: usize) -> Vec<RecentProject> {
    recent_with(store, others, limit, |path| if path.is_dir() { "folder" } else { "missing" })
}

fn recent_with(store: &Path, others: &[String], limit: usize, state_of: impl Fn(&Path) -> &'static str) -> Vec<RecentProject> {
    let mut out: Vec<RecentProject> = Vec::new();
    let wanted = limit.max(1);
    let mut add = |path: &str, last_used_ms: u64| {
        // Enough found: the Git state of the rest is never asked for.
        if out.len() >= wanted {
            return;
        }
        // A copy's folder is listed as the project it came from.
        let path = real_folder(path);
        let path = path.as_str();
        if path.is_empty() || out.iter().any(|seen| key(&seen.path) == key(path)) {
            return;
        }
        // Anything else inside `.puppet-master/` (operations, storage) is not a project either.
        if path.replace('\\', "/").contains("/.puppet-master/") {
            return;
        }
        out.push(RecentProject {
            path: path.to_string(),
            name: name_of(path),
            state: state_of(Path::new(path)).into(),
            last_used_ms,
        });
    };
    for item in load(store) {
        add(&item.path, item.last_used_ms);
    }
    for path in others {
        add(path, 0);
    }
    out.truncate(limit.max(1));
    out
}

// ----------------------------------------------------------------------------- Set up Git here

/// What "Set up Git here" will do, shown before it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitPlan {
    pub path: String,
    pub name: String,
    /// `folder` or `no_commits`: the state it starts from.
    pub state: String,
    /// The commands, as they would be typed.
    pub commands: Vec<String>,
    /// Files the first snapshot will include (counted up to [`MAX_SNAPSHOT_FILES`]).
    pub files: usize,
    /// More than [`MAX_SNAPSHOT_FILES`]: Puppet Master will not snapshot this folder.
    pub too_big: bool,
    /// Lines of a `.gitignore` that will be written (empty when the folder has one).
    pub gitignore: Vec<String>,
}

fn count_files(root: &Path, skip_default_ignored: bool) -> usize {
    fn walk(dir: &Path, depth: usize, skip: bool, count: &mut usize) {
        if depth > MAX_DEPTH || *count > MAX_SNAPSHOT_FILES {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() || name == ".git" || name == ".puppet-master" {
                continue;
            }
            if kind.is_dir() {
                let ignored = skip
                    && DEFAULT_GITIGNORE.iter().any(|line| line.strip_suffix('/').is_some_and(|d| d == name));
                if !ignored {
                    walk(&entry.path(), depth + 1, skip, count);
                }
            } else {
                let ignored = skip && (name == ".env" || name.starts_with(".env.") && name != ".env.example" || name == ".DS_Store" || name == "Thumbs.db");
                if !ignored {
                    *count += 1;
                }
            }
            if *count > MAX_SNAPSHOT_FILES {
                return;
            }
        }
    }
    let mut count = 0;
    walk(root, 0, skip_default_ignored, &mut count);
    count
}

pub fn init_plan(path: &Path) -> Result<InitPlan, OperationError> {
    let state = git_state(path);
    match state {
        "missing" => return Err(err("FOLDER_MISSING", format!("{} does not exist.", path.display()))),
        "repo" => return Err(err("ALREADY_GIT", "This folder already uses Git.")),
        _ => {}
    }
    let write_ignore = !path.join(".gitignore").exists();
    let files = count_files(path, write_ignore);
    let mut commands = Vec::new();
    if state == "folder" {
        commands.push("git init".to_string());
    }
    commands.push("git add -A".to_string());
    commands.push("git commit -m \"First snapshot\"".to_string());
    Ok(InitPlan {
        path: path.to_string_lossy().into_owned(),
        name: name_of(&path.to_string_lossy()),
        state: state.into(),
        commands,
        files,
        too_big: files > MAX_SNAPSHOT_FILES,
        gitignore: if write_ignore { DEFAULT_GITIGNORE.iter().map(|s| (*s).to_string()).collect() } else { Vec::new() },
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitOutcome {
    pub commit: String,
    pub files: usize,
    pub wrote_gitignore: bool,
}

fn must(dir: &Path, args: &[&str], what: &str) -> Result<String, OperationError> {
    let out = git_raw(dir, args, &[])?;
    if out.ok {
        Ok(out.stdout.trim().to_string())
    } else {
        Err(err("GIT_INIT_FAILED", format!("{what} failed: {}", tail_lines(&format!("{}\n{}", out.stderr, out.stdout), 3))))
    }
}

/// Run the plan: `git init` (when the folder has no Git), a `.gitignore` when it has none, then
/// one commit of everything (`.puppet-master/` aside). Refuses a folder that is too big.
pub fn init(path: &Path) -> Result<InitOutcome, OperationError> {
    let plan = init_plan(path)?;
    if plan.too_big {
        return Err(err(
            "FOLDER_TOO_BIG",
            format!("This folder has more than {MAX_SNAPSHOT_FILES} files. Pick the project's own folder instead."),
        ));
    }
    if plan.state == "folder" {
        must(path, &["init"], "git init")?;
        // A fresh repository starts on `main` whatever the global default is.
        let _ = git_raw(path, &["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
    }
    let wrote_gitignore = !plan.gitignore.is_empty() && !path.join(".gitignore").exists();
    if wrote_gitignore {
        let mut text = plan.gitignore.join("\n");
        text.push('\n');
        std::fs::write(path.join(".gitignore"), text)
            .map_err(|error| err("GIT_INIT_FAILED", format!("could not write .gitignore: {error}")))?;
    }
    // Keep Puppet Master's own folder out of the snapshot and out of `git status`.
    crate::git_exclude::ensure_project_storage_excluded(path);
    must(path, &["add", "-A", "--", ".", ":(exclude).puppet-master"], "git add")?;
    must(
        path,
        &identity_args(path, &["commit", "--allow-empty", "--no-verify", "-q", "-m", "First snapshot"]),
        "git commit",
    )?;
    let commit = must(path, &["rev-parse", "--short", "HEAD"], "git rev-parse")?;
    Ok(InitOutcome { commit, files: plan.files, wrote_gitignore })
}

/// Keep `PathBuf` in the public API of this module for callers that hold one.
pub fn path_of(text: &str) -> PathBuf {
    crate::project_path::expand_user_path(Path::new(text.trim()))
}
