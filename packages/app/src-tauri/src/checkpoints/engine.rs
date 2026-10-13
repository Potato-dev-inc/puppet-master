//! Snapshot, list, diff and restore on the shadow repository.
//!
//! A snapshot is `add -A` into a private per-pane index, `write-tree`, `commit-tree` (an orphan
//! commit: no parents, so pruning a ref really frees its objects) and `update-ref
//! refs/pm/<pane>/<n>`. The user's own `.git` is never named in any call.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;

use super::git::{git_available, GitError, Shadow};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone)]
pub struct Limits {
    /// Checkpoints kept per pane.
    pub keep: usize,
    /// Files larger than this are left out of snapshots.
    pub max_file_bytes: u64,
    /// A snapshot whose new or changed files add up to more than this is refused.
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { keep: 50, max_file_bytes: 20 * 1024 * 1024, max_total_bytes: 1024 * 1024 * 1024 }
    }
}

#[derive(Debug, Clone)]
pub enum CheckpointError {
    GitMissing,
    Disabled,
    TooLarge(u64),
    NotFound,
    Running,
    Other(String),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitMissing => f.write_str("Checkpoints need Git, and Git isn't installed on this computer."),
            Self::Disabled => f.write_str("Checkpoints are turned off in Settings."),
            Self::TooLarge(bytes) => write!(
                f,
                "This folder has too much new data to checkpoint ({} MB). Add big folders to .gitignore.",
                bytes / (1024 * 1024)
            ),
            Self::NotFound => f.write_str("That checkpoint no longer exists."),
            Self::Running => f.write_str("The agent is working. Interrupt it first, then undo."),
            Self::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for CheckpointError {}

impl From<GitError> for CheckpointError {
    fn from(e: GitError) -> Self {
        Self::Other(e.0)
    }
}

/// Why a snapshot was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Just before a turn starts (prompt dispatched, or the pane started running).
    Before,
    /// A turn ended (running -> idle / waiting_input).
    After,
    /// Just before a prompt answer is sent.
    Answer,
    /// Safety copy taken right before a restore.
    Undo,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Before => "before",
            Kind::After => "after",
            Kind::Answer => "answer",
            Kind::Undo => "undo",
        }
    }
    fn parse(s: &str) -> Kind {
        match s {
            "after" => Kind::After,
            "answer" => Kind::Answer,
            "undo" => Kind::Undo,
            _ => Kind::Before,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    /// The `<n>` of `refs/pm/<pane>/<n>`.
    pub id: String,
    pub commit: String,
    pub pane_id: String,
    pub turn: u32,
    pub kind: String,
    pub label: String,
    pub note: String,
    pub at_ms: u64,
    pub files: u32,
    pub added: u32,
    pub deleted: u32,
    #[serde(skip)]
    pub tree: String,
}

#[derive(Debug, Clone)]
pub enum Snapshot {
    Created(Checkpoint),
    /// The tree matches the newest checkpoint; nothing was written.
    Unchanged(Option<Checkpoint>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// `A` added, `M` modified, `D` deleted.
    pub status: String,
    pub added: u32,
    pub deleted: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    /// The "before undo" checkpoint (or the newest one when nothing had changed).
    pub safety_id: Option<String>,
    pub written: u32,
    pub removed: u32,
    pub failed: Vec<String>,
}

static LOCKS: Lazy<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn lock_for(shadow: &Shadow) -> Arc<Mutex<()>> {
    LOCKS.lock().entry(shadow.git_dir.clone()).or_default().clone()
}

/// Ref-safe key for a pane id.
pub fn pane_key(pane: &str) -> String {
    let key: String = pane
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if key.is_empty() {
        "_".into()
    } else {
        key
    }
}

fn index_path(shadow: &Shadow, pane: &str) -> PathBuf {
    shadow.git_dir.join(format!("pm-index-{}", pane_key(pane)))
}

fn prepare(project: &Path) -> Result<Shadow, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    if !project.is_dir() {
        return Err(CheckpointError::Other("The project folder doesn't exist.".into()));
    }
    let shadow = Shadow::new(project);
    shadow.ensure(&[])?;
    Ok(shadow)
}

/// Checkpoints of one pane, oldest first.
pub fn list(project: &Path, pane: &str) -> Result<Vec<Checkpoint>, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Ok(Vec::new());
    }
    list_in(&shadow, pane)
}

fn list_in(shadow: &Shadow, pane: &str) -> Result<Vec<Checkpoint>, CheckpointError> {
    let prefix = format!("refs/pm/{}/", pane_key(pane));
    let out = shadow.run(
        &[
            "for-each-ref".to_string(),
            "--format=%(refname)%1f%(objectname)%1f%(tree)%1f%(contents)%1e".to_string(),
            prefix.clone(),
        ],
        None,
        None,
        &[],
    )?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut items = Vec::new();
    for record in text.split('\u{1e}') {
        let record = record.trim_matches(|c| c == '\n' || c == '\r');
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(4, '\u{1f}');
        let (Some(name), Some(commit), Some(tree), Some(body)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let id = name.strip_prefix(&prefix).unwrap_or(name).to_string();
        if id.parse::<u64>().is_err() {
            continue;
        }
        items.push(parse_message(id, commit, tree, pane, body));
    }
    items.sort_by_key(|c| c.id.parse::<u64>().unwrap_or(0));
    Ok(items)
}

fn parse_message(id: String, commit: &str, tree: &str, pane: &str, body: &str) -> Checkpoint {
    let mut c = Checkpoint {
        id,
        commit: commit.into(),
        pane_id: pane.into(),
        turn: 0,
        kind: "before".into(),
        label: String::new(),
        note: String::new(),
        at_ms: 0,
        files: 0,
        added: 0,
        deleted: 0,
        tree: tree.into(),
    };
    for (i, line) in body.lines().enumerate() {
        if i == 0 {
            c.label = line.strip_prefix("checkpoint: ").unwrap_or(line).to_string();
            continue;
        }
        let Some((k, v)) = line.split_once(": ") else { continue };
        match k {
            "pm-turn" => c.turn = v.parse().unwrap_or(0),
            "pm-kind" => c.kind = Kind::parse(v).as_str().into(),
            "pm-note" => c.note = v.into(),
            "pm-at" => c.at_ms = v.parse().unwrap_or(0),
            "pm-files" => c.files = v.parse().unwrap_or(0),
            "pm-added" => c.added = v.parse().unwrap_or(0),
            "pm-deleted" => c.deleted = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    c
}

fn label_for(kind: Kind, turn: u32) -> String {
    match kind {
        Kind::Before => format!("Before turn {turn}"),
        Kind::After => format!("After turn {turn}"),
        Kind::Answer => format!("Before answering a prompt (turn {turn})"),
        Kind::Undo => "Before undo".into(),
    }
}

/// Untracked + modified files (per the pane index): those over the per-file limit, and a guard
/// on the total size of the rest.
fn scan(shadow: &Shadow, index: &Path, limits: &Limits) -> Result<Vec<String>, CheckpointError> {
    let out = shadow.run(&["ls-files", "-z", "-o", "-m", "--exclude-standard"], Some(index), None, &[])?;
    let mut big = Vec::new();
    let mut total = 0_u64;
    for raw in out.stdout.split(|b| *b == 0) {
        if raw.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(raw).to_string();
        let Ok(meta) = std::fs::symlink_metadata(shadow.work_tree.join(&rel)) else { continue };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > limits.max_file_bytes {
            big.push(rel);
        } else {
            total += meta.len();
        }
    }
    if total > limits.max_total_bytes {
        return Err(CheckpointError::TooLarge(total));
    }
    Ok(big)
}

fn stage_all(shadow: &Shadow, index: &Path, big: &[String]) -> Result<(), CheckpointError> {
    let run = || -> Result<(), GitError> {
        if big.is_empty() {
            shadow.run(&["add", "-A"], Some(index), None, &[])?;
        } else {
            let mut spec: Vec<u8> = b".".to_vec();
            for path in big {
                spec.push(0);
                spec.extend_from_slice(b":(exclude,literal)");
                spec.extend_from_slice(path.as_bytes());
            }
            shadow.run(
                &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
                Some(index),
                Some(&spec),
                &[],
            )?;
        }
        Ok(())
    };
    // A file the agent is writing right now can be briefly unreadable on Windows: retry once.
    match run() {
        Ok(()) => Ok(()),
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_millis(400));
            run().map_err(Into::into)
        }
    }
}

fn numstat(shadow: &Shadow, from: &str, to: &str) -> Result<Vec<(String, u32, u32, bool)>, CheckpointError> {
    let out = shadow.run(&["diff-tree", "-r", "-z", "--numstat", "--no-renames", from, to], None, None, &[])?;
    let mut rows = Vec::new();
    for rec in out.stdout.split(|b| *b == 0) {
        let line = String::from_utf8_lossy(rec);
        let mut it = line.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (it.next(), it.next(), it.next()) else { continue };
        let binary = a == "-";
        rows.push((path.to_string(), a.parse().unwrap_or(0), d.parse().unwrap_or(0), binary));
    }
    Ok(rows)
}

/// Take a checkpoint of `project` for `pane`. Honours the file-size guard and retention.
pub fn snapshot(
    project: &Path,
    pane: &str,
    kind: Kind,
    note: Option<&str>,
    now_ms: u64,
    limits: &Limits,
) -> Result<Snapshot, CheckpointError> {
    let shadow = prepare(project)?;
    let gate = lock_for(&shadow);
    let _held = gate.lock();
    snapshot_locked(&shadow, pane, kind, note, now_ms, limits)
}

fn snapshot_locked(
    shadow: &Shadow,
    pane: &str,
    kind: Kind,
    note: Option<&str>,
    now_ms: u64,
    limits: &Limits,
) -> Result<Snapshot, CheckpointError> {
    let index = index_path(shadow, pane);
    let big = scan(shadow, &index, limits)?;
    stage_all(shadow, &index, &big)?;
    let tree = shadow.run(&["write-tree"], Some(&index), None, &[])?.text();
    let existing = list_in(shadow, pane)?;
    let newest = existing.last();
    if newest.is_some_and(|c| c.tree == tree) {
        return Ok(Snapshot::Unchanged(newest.cloned()));
    }
    let prev_tree = newest.map(|c| c.tree.clone()).unwrap_or_else(|| EMPTY_TREE.into());
    let rows = numstat(shadow, &prev_tree, &tree)?;
    let files = rows.len() as u32;
    let added = rows.iter().map(|r| r.1).sum::<u32>();
    let deleted = rows.iter().map(|r| r.2).sum::<u32>();
    let max_turn = existing.iter().map(|c| c.turn).max().unwrap_or(0);
    let turn = match (kind, newest.map(|c| c.kind.as_str())) {
        (Kind::Before, None | Some("after") | Some("undo")) => max_turn + 1,
        _ => max_turn.max(1),
    };
    let n = existing.iter().filter_map(|c| c.id.parse::<u64>().ok()).max().unwrap_or(0) + 1;
    let note_clean: String = note.unwrap_or("").chars().filter(|c| !c.is_control()).take(80).collect();
    let mut message = format!(
        "checkpoint: {}\n\npm-pane: {}\npm-n: {n}\npm-turn: {turn}\npm-kind: {}\n",
        label_for(kind, turn),
        pane_key(pane),
        kind.as_str()
    );
    if !note_clean.trim().is_empty() {
        message.push_str(&format!("pm-note: {}\n", note_clean.trim()));
    }
    message.push_str(&format!("pm-at: {now_ms}\npm-files: {files}\npm-added: {added}\npm-deleted: {deleted}\n"));
    let date = format!("@{} +0000", now_ms / 1000);
    let env = [("GIT_AUTHOR_DATE", date.clone()), ("GIT_COMMITTER_DATE", date)];
    let commit = shadow.run(&["commit-tree", tree.as_str(), "-m", message.as_str()], None, None, &env)?.text();
    let refname = format!("refs/pm/{}/{n}", pane_key(pane));
    shadow.run(&["update-ref", refname.as_str(), commit.as_str()], None, None, &[])?;
    prune_locked(shadow, pane, limits.keep);
    if n % 10 == 0 {
        // Occasional housekeeping; failures are not worth surfacing.
        let _ = shadow.run(&["-c", "gc.auto=6700", "gc", "--auto", "--quiet"], None, None, &[]);
    }
    Ok(Snapshot::Created(parse_message(n.to_string(), &commit, &tree, pane, &message)))
}

/// Delete refs beyond the newest `keep`. Returns how many were removed.
#[allow(dead_code)] // retention runs inside `snapshot`; this is the explicit entry point (tests, tooling)
pub fn prune(project: &Path, pane: &str, keep: usize) -> Result<usize, CheckpointError> {
    let shadow = prepare(project)?;
    let gate = lock_for(&shadow);
    let _held = gate.lock();
    Ok(prune_locked(&shadow, pane, keep))
}

fn prune_locked(shadow: &Shadow, pane: &str, keep: usize) -> usize {
    let keep = keep.max(1);
    let Ok(items) = list_in(shadow, pane) else { return 0 };
    if items.len() <= keep {
        return 0;
    }
    let mut removed = 0;
    for old in &items[..items.len() - keep] {
        let name = format!("refs/pm/{}/{}", pane_key(pane), old.id);
        if shadow.run(&["update-ref", "-d", name.as_str()], None, None, &[]).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn find(items: &[Checkpoint], id: &str) -> Result<usize, CheckpointError> {
    items.iter().position(|c| c.id == id).ok_or(CheckpointError::NotFound)
}

/// Files that changed in checkpoint `id` compared with the one before it (or an empty folder).
pub fn diff(project: &Path, pane: &str, id: &str) -> Result<Vec<FileChange>, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Err(CheckpointError::NotFound);
    }
    let items = list_in(&shadow, pane)?;
    let at = find(&items, id)?;
    let prev = if at == 0 { EMPTY_TREE.to_string() } else { items[at - 1].tree.clone() };
    changes_between(&shadow, &prev, &items[at].tree)
}

/// Files that differ between two trees, with line counts, sorted by path.
fn changes_between(shadow: &Shadow, prev: &str, tree: &str) -> Result<Vec<FileChange>, CheckpointError> {
    let stats: HashMap<String, (u32, u32, bool)> = numstat(shadow, prev, tree)?
        .into_iter()
        .map(|(p, a, d, b)| (p, (a, d, b)))
        .collect();
    let mut changes: Vec<FileChange> = name_status(shadow, prev, tree)?
        .into_iter()
        .map(|(status, path)| {
            let (added, deleted, binary) = stats.get(&path).copied().unwrap_or((0, 0, false));
            let status = if status == 'T' { 'M' } else { status };
            FileChange { path, status: status.to_string(), added, deleted, binary }
        })
        .collect();
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// How far after a turn started a checkpoint may still count as that turn's "before" state.
const TURN_START_SLACK_MS: u64 = 2_000;

/// Files a turn changed: the newest checkpoint taken at (or within a moment of) the turn start,
/// compared with the newest checkpoint now. `None` when no baseline exists (checkpoints were off
/// or the folder was never snapshotted), so callers can say "unknown" instead of guessing.
pub fn turn_changes(project: &Path, pane: &str, started_at_ms: u64) -> Result<Option<Vec<FileChange>>, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Ok(None);
    }
    let items = list_in(&shadow, pane)?;
    let Some(after) = items.last() else { return Ok(None) };
    let Some(before) = items.iter().rev().find(|c| c.at_ms <= started_at_ms.saturating_add(TURN_START_SLACK_MS)) else {
        return Ok(None);
    };
    if before.tree == after.tree {
        return Ok(Some(Vec::new()));
    }
    changes_between(&shadow, &before.tree, &after.tree).map(Some)
}

fn name_status(shadow: &Shadow, from: &str, to: &str) -> Result<Vec<(char, String)>, CheckpointError> {
    let out = shadow.run(&["diff-tree", "-r", "-z", "--name-status", "--no-renames", from, to], None, None, &[])?;
    let parts: Vec<String> = out.stdout.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).to_string()).collect();
    let mut rows = Vec::new();
    let mut i = 0;
    while i + 1 < parts.len() {
        if let Some(status) = parts[i].trim().chars().next() {
            rows.push((status, parts[i + 1].clone()));
        }
        i += 2;
    }
    Ok(rows)
}

fn safe_relative(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))) {
        Some(p.to_path_buf())
    } else {
        None
    }
}

fn remove_file_forced(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            if let Ok(meta) = std::fs::metadata(path) {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    let _ = std::fs::set_permissions(path, perms);
                    return std::fs::remove_file(path);
                }
            }
            Err(e)
        }
    }
}

/// Put the project's files back to checkpoint `id`.
///
/// `pane_running` is the caller's view of the pane: restoring under a working agent would race
/// it, so it is refused. A safety checkpoint of the current state is taken first. Only files the
/// shadow repo tracks are written or deleted; ignored and oversized files are never touched.
pub fn restore(
    project: &Path,
    pane: &str,
    id: &str,
    pane_running: bool,
    now_ms: u64,
    limits: &Limits,
) -> Result<RestoreReport, CheckpointError> {
    if pane_running {
        return Err(CheckpointError::Running);
    }
    let shadow = prepare(project)?;
    let gate = lock_for(&shadow);
    let _held = gate.lock();
    let items = list_in(&shadow, pane)?;
    let target = items[find(&items, id)?].clone();
    let mut report = RestoreReport::default();
    let safety = snapshot_locked(&shadow, pane, Kind::Undo, None, now_ms, limits)?;
    report.safety_id = match &safety {
        Snapshot::Created(c) => Some(c.id.clone()),
        Snapshot::Unchanged(c) => c.as_ref().map(|c| c.id.clone()),
    };
    let index = index_path(&shadow, pane);
    // The index now mirrors the disk, so this tree is "what is there now".
    let current = shadow.run(&["write-tree"], Some(&index), None, &[])?.text();
    let rows = name_status(&shadow, &current, &target.tree)?;
    let mut to_write: Vec<String> = Vec::new();
    for (status, path) in &rows {
        let Some(rel) = safe_relative(path) else {
            report.failed.push(path.clone());
            continue;
        };
        let abs = shadow.work_tree.join(&rel);
        match status {
            'D' => match remove_file_forced(&abs) {
                Ok(()) => {
                    report.removed += 1;
                    let mut dir = abs.parent().map(Path::to_path_buf);
                    while let Some(d) = dir {
                        if d == shadow.work_tree || std::fs::remove_dir(&d).is_err() {
                            break;
                        }
                        dir = d.parent().map(Path::to_path_buf);
                    }
                }
                Err(_) => report.failed.push(path.clone()),
            },
            'T' => {
                let _ = remove_file_forced(&abs);
                to_write.push(path.clone());
            }
            _ => to_write.push(path.clone()),
        }
    }
    shadow.run(&["read-tree", target.tree.as_str()], Some(&index), None, &[])?;
    if !to_write.is_empty() {
        let mut stdin = Vec::new();
        for p in &to_write {
            stdin.extend_from_slice(p.as_bytes());
            stdin.push(0);
        }
        if shadow.run(&["checkout-index", "-f", "-z", "--stdin"], Some(&index), Some(&stdin), &[]).is_ok() {
            report.written = to_write.len() as u32;
        } else {
            // One at a time, so a single locked file does not block the rest.
            for p in &to_write {
                if shadow.run(&["checkout-index", "-f", "--", p.as_str()], Some(&index), None, &[]).is_ok() {
                    report.written += 1;
                } else {
                    report.failed.push(p.clone());
                }
            }
        }
    }
    Ok(report)
}
