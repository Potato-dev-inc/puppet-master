//! "My agent just pushed" from plain git, for any remote and any agent (plan §6.1 step 1).
//!
//! `git push` updates the remote-tracking ref (`refs/remotes/origin/<branch>`) in the pushing
//! repository and writes a reflog line `update by push`. So a push is a remote-tracking ref that
//! moved with that reflog message. A `git fetch` moves the same refs with a `fetch` message and is
//! ignored here (teammates' pushes are their facts, not ours to announce).
//!
//! Attribution: if an agent pane working in that repository was running at that moment, or its
//! turn ended within [`TURN_GRACE_MS`], the push is "made with" that agent's display label (from
//! the agent registry). Otherwise it is the person's own. Generic code: nothing here knows any
//! agent's name.

use crate::team::event::{AgentTag, DiffStat};
use crate::team::git;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A push this soon after an agent's turn ended still counts as that agent's.
pub const TURN_GRACE_MS: u64 = 60_000;
/// Repo-relative paths carried on one push event.
const MAX_PATHS: usize = 200;

/// `refs/remotes/<remote>/<branch>` → commit.
pub type RefSnapshot = BTreeMap<String, String>;

/// Every remote-tracking ref of the repository at `dir` (symbolic `…/HEAD` left out).
pub fn snapshot(dir: &Path) -> Result<RefSnapshot, String> {
    let text = git::run(dir, &["for-each-ref", "--format=%(refname) %(objectname)", "refs/remotes"])?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(name, _)| !name.ends_with("/HEAD"))
        .map(|(name, sha)| (name.to_string(), sha.to_string()))
        .collect())
}

/// The message of the newest reflog entry of `refname`, read from the repository's shared git
/// directory (`logs/refs/remotes/...`).
pub fn last_reflog_message(common_dir: &Path, refname: &str) -> Option<String> {
    let text = std::fs::read_to_string(common_dir.join("logs").join(refname)).ok()?;
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?;
    Some(line.split_once('\t').map(|(_, msg)| msg).unwrap_or("").trim().to_string())
}

/// What git writes when a push updates the tracking ref.
pub fn is_push_message(message: &str) -> bool {
    message.starts_with("update by push")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushFact {
    pub remote: String,
    pub branch: String,
    /// The tip before the push (`None` for a new branch).
    pub old: Option<String>,
    pub commit: String,
    pub forced: bool,
    pub stats: DiffStat,
    pub paths: Vec<String>,
}

/// `refs/remotes/<remote>/<branch>` → (`remote`, `branch`).
fn split_remote_ref(refname: &str) -> Option<(String, String)> {
    let rest = refname.strip_prefix("refs/remotes/")?;
    let (remote, branch) = rest.split_once('/')?;
    (!remote.is_empty() && !branch.is_empty()).then(|| (remote.to_string(), branch.to_string()))
}

/// Refs that are new or moved between two snapshots (deleted refs are not pushes of work).
pub fn moved(old: &RefSnapshot, new: &RefSnapshot) -> Vec<(String, Option<String>, String)> {
    new.iter()
        .filter(|(name, sha)| old.get(*name) != Some(*sha))
        .map(|(name, sha)| (name.clone(), old.get(name).cloned(), sha.clone()))
        .collect()
}

/// The commit a new branch's changes are counted from: where it left the remote's default branch.
fn base_for_new_branch(dir: &Path, remote: &str, commit: &str, snap: &RefSnapshot) -> Option<String> {
    let default = git::run(dir, &["symbolic-ref", "-q", &format!("refs/remotes/{remote}/HEAD")])
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            ["main", "master", "trunk", "develop"]
                .iter()
                .map(|b| format!("refs/remotes/{remote}/{b}"))
                .find(|r| snap.contains_key(r))
        })?;
    let base = git::run(dir, &["merge-base", commit, &default]).ok()?;
    (!base.is_empty() && base != commit).then_some(base)
}

fn diff_stat(dir: &Path, from: Option<&str>, to: &str) -> (DiffStat, Vec<String>) {
    let parent = format!("{to}^");
    let from = match from {
        Some(f) => f.to_string(),
        None if git::rev_parse(dir, &parent).is_some() => parent,
        None => return (DiffStat::default(), Vec::new()),
    };
    let Ok(text) = git::run(dir, &["diff", "--numstat", "--no-renames", &from, to]) else {
        return (DiffStat::default(), Vec::new());
    };
    let mut stat = DiffStat::default();
    let mut paths = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        stat.files += 1;
        stat.additions += add.parse::<u32>().unwrap_or(0);
        stat.deletions += del.parse::<u32>().unwrap_or(0);
        if paths.len() < MAX_PATHS {
            paths.push(path.to_string());
        }
    }
    (stat, paths)
}

/// Pushes between two snapshots of the repository at `dir`.
pub fn pushes(dir: &Path, common_dir: &Path, old: &RefSnapshot, new: &RefSnapshot) -> Vec<PushFact> {
    let mut out = Vec::new();
    for (refname, before, after) in moved(old, new) {
        let Some((remote, branch)) = split_remote_ref(&refname) else { continue };
        let by_push = last_reflog_message(common_dir, &refname).is_some_and(|m| is_push_message(&m));
        if !by_push {
            continue;
        }
        let forced = before.as_deref().is_some_and(|b| !git::is_ancestor(dir, b, &after));
        let from = match (&before, forced) {
            (Some(b), false) => Some(b.clone()),
            _ => base_for_new_branch(dir, &remote, &after, new),
        };
        let (stats, paths) = diff_stat(dir, from.as_deref(), &after);
        out.push(PushFact { remote, branch, old: before, commit: after, forced, stats, paths });
    }
    out
}

/// One repository watched for pushes.
#[derive(Debug)]
pub struct RepoWatch {
    pub dir: PathBuf,
    pub common_dir: PathBuf,
    pub last: RefSnapshot,
}

impl RepoWatch {
    /// Start watching `dir` (any folder inside a repository). The current refs are the baseline:
    /// pushes made before the watch started are not announced.
    pub fn open(dir: &Path) -> Option<Self> {
        let top = git::toplevel(dir)?;
        let common_dir = git::common_dir(&top)?;
        let last = snapshot(&top).ok()?;
        Some(Self { dir: top, common_dir, last })
    }

    /// Pushes since the previous call.
    pub fn poll(&mut self) -> Vec<PushFact> {
        let Ok(now) = snapshot(&self.dir) else { return Vec::new() };
        if now == self.last {
            return Vec::new();
        }
        let found = pushes(&self.dir, &self.common_dir, &self.last, &now);
        self.last = now;
        found
    }
}

/// What attribution needs to know about one pane.
#[derive(Debug, Clone)]
pub struct PaneFacts {
    pub cwd: String,
    pub status: String,
    /// Display label from the agent registry ("Claude Code").
    pub label: String,
    /// An agent (not a plain shell). A push typed in a shell pane is the person's own.
    pub is_agent: bool,
    /// When this pane's last turn ended.
    pub last_turn_end_ms: Option<u64>,
}

/// The agent a push in `repo` should be credited to, or `None` for the person.
pub fn attribute(repo: &Path, now_ms: u64, panes: &[PaneFacts]) -> Option<AgentTag> {
    let in_repo = |p: &&PaneFacts| p.is_agent && crate::project_path::workspace_covers(repo, Path::new(&p.cwd));
    let candidates: Vec<&PaneFacts> = panes.iter().filter(in_repo).collect();
    let running = candidates.iter().filter(|p| p.status == "running").min_by(|a, b| a.label.cmp(&b.label));
    let recent = || {
        candidates
            .iter()
            .filter(|p| p.last_turn_end_ms.is_some_and(|t| now_ms.saturating_sub(t) <= TURN_GRACE_MS))
            .max_by_key(|p| p.last_turn_end_ms)
    };
    running
        .or_else(recent)
        .map(|p| AgentTag { label: p.label.clone(), role: None })
}
