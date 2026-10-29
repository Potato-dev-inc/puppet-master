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

/// What the user sees when a folder is too big to checkpoint (the History card and any error).
pub const TOO_BIG_MESSAGE: &str = "Undo is off for this folder: it is too big. Add big folders to .gitignore.";

#[derive(Debug, Clone)]
pub enum CheckpointError {
    GitMissing,
    Disabled,
    TooLarge(u64),
    NotFound,
    Running,
    /// An agent run is still open on the pane.
    RunOpen,
    Other(String),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitMissing => f.write_str("Checkpoints need Git, and Git isn't installed on this computer."),
            Self::Disabled => f.write_str("Checkpoints are turned off in Settings."),
            Self::TooLarge(_) => f.write_str(TOO_BIG_MESSAGE),
            Self::NotFound => f.write_str("That checkpoint no longer exists."),
            Self::Running => f.write_str("The agent is working. Interrupt it first, then undo."),
            Self::RunOpen => f.write_str("A run is still open on this pane. Interrupt it first, then undo."),
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
    /// Safety copy taken right before "Stop all agents" interrupts a running pane.
    Stop,
    /// Safety copy taken right before a restore.
    Undo,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Before => "before",
            Kind::After => "after",
            Kind::Answer => "answer",
            Kind::Stop => "stop",
            Kind::Undo => "undo",
        }
    }
    fn parse(s: &str) -> Kind {
        match s {
            "after" => Kind::After,
            "answer" => Kind::Answer,
            "stop" => Kind::Stop,
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

pub(super) fn lock_for(shadow: &Shadow) -> Arc<Mutex<()>> {
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

pub(crate) fn list_in(shadow: &Shadow, pane: &str) -> Result<Vec<Checkpoint>, CheckpointError> {
    let prefix = format!("refs/pm/{}/", pane_key(pane));
    let mut items = Vec::new();
    for (name, commit, tree, body) in read_refs(shadow, &prefix)? {
        let id = name.strip_prefix(&prefix).unwrap_or(name.as_str()).to_string();
        if id.parse::<u64>().is_err() {
            continue;
        }
        items.push(parse_message(id, &commit, &tree, pane, &body));
    }
    items.sort_by_key(|c| c.id.parse::<u64>().unwrap_or(0));
    Ok(items)
}

/// Every checkpoint of every pane key in this folder (`refs/pm/*`, so the `wb-` and `mission-`
/// series too), oldest first. `pane_id` is the ref's key. A folder never snapshotted: empty.
// The What-broke-it engine (`checkpoints/bisect/`) is the first caller of the `pub fn`s below;
// until it lands, a non-test build has no user for them.
#[allow(dead_code)]
pub fn list_all(project: &Path) -> Result<Vec<Checkpoint>, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for (name, commit, tree, body) in read_refs(&shadow, "refs/pm/")? {
        // `refs/pm/<key>/<n>`: the key is already ref-safe (`pane_key`), so it has no `/`.
        let Some((key, id)) = name.strip_prefix("refs/pm/").and_then(|rest| rest.split_once('/')) else {
            continue;
        };
        if key.is_empty() || id.parse::<u64>().is_err() {
            continue;
        }
        items.push(parse_message(id.to_string(), &commit, &tree, key, &body));
    }
    items.sort_by_key(|c| (c.at_ms, c.id.parse::<u64>().unwrap_or(0)));
    Ok(items)
}

/// Refs under `prefix` as `(refname, commit, tree, message)`, in refname order.
fn read_refs(shadow: &Shadow, prefix: &str) -> Result<Vec<(String, String, String, String)>, CheckpointError> {
    let out = shadow.run(
        &[
            "for-each-ref".to_string(),
            "--format=%(refname)%1f%(objectname)%1f%(tree)%1f%(contents)%1e".to_string(),
            prefix.to_string(),
        ],
        None,
        None,
        &[],
    )?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut rows = Vec::new();
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
        rows.push((name.to_string(), commit.to_string(), tree.to_string(), body.to_string()));
    }
    Ok(rows)
}

/// A full 40-character lowercase object id (what `write-tree` prints). Anything else never
/// reaches a git command line.
fn is_object_id(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The shadow repository of `project`, once every tree id is a valid object id and the repository exists.
/// Read-only: nothing is created (unlike `prepare`).
fn shadow_at(project: &Path, trees: &[&str]) -> Result<Shadow, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    if trees.iter().any(|t| !is_object_id(t)) {
        return Err(CheckpointError::Other("That save point is not valid.".into()));
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Err(CheckpointError::NotFound);
    }
    Ok(shadow)
}

/// Bytes of the files in `tree`, from the object store (the folder itself is not read).
#[allow(dead_code)] // first caller: `checkpoints/bisect/` (see `list_all`)
pub fn tree_bytes(project: &Path, tree: &str) -> Result<u64, CheckpointError> {
    let shadow = shadow_at(project, &[tree])?;
    let out = shadow.run(&["ls-tree", "-r", "-l", "-z", tree], None, None, &[])?;
    let mut total = 0_u64;
    for record in out.stdout.split(|b| *b == 0) {
        // `<mode> <type> <object> <size>\t<path>`; a submodule shows `-` and counts as 0.
        let line = String::from_utf8_lossy(record);
        let Some((meta, _)) = line.split_once('\t') else { continue };
        let size = meta.split_whitespace().last().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        total = total.saturating_add(size);
    }
    Ok(total)
}

/// Largest file `file_at` reads.
const FILE_AT_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// The bytes of the file `path` in `tree`. `None` when the tree has no regular file of that
/// name (absent, a folder, a submodule). A file over 20 MiB is refused, not read.
#[allow(dead_code)] // first caller: `checkpoints/bisect/` (see `list_all`)
pub fn file_at(project: &Path, tree: &str, path: &str) -> Result<Option<Vec<u8>>, CheckpointError> {
    let shadow = shadow_at(project, &[tree])?;
    if safe_relative(path).is_none() {
        return Err(CheckpointError::Other("That file name is not allowed.".into()));
    }
    let listing = shadow.run(&["ls-tree", "-z", "-l", tree, "--", path], None, None, &[])?;
    for record in listing.stdout.split(|b| *b == 0) {
        let line = String::from_utf8_lossy(record);
        let Some((meta, name)) = line.split_once('\t') else { continue };
        if name != path {
            continue;
        }
        // `<mode> <type> <object> <size>`
        let mut fields = meta.split_whitespace();
        let (Some(_mode), Some(kind), Some(object), Some(size)) = (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if kind != "blob" {
            return Ok(None);
        }
        if size.parse::<u64>().unwrap_or(0) > FILE_AT_MAX_BYTES {
            return Err(CheckpointError::Other("That file is too big to read here (over 20 MB).".into()));
        }
        return Ok(Some(shadow.run(&["cat-file", "blob", object], None, None, &[])?.stdout));
    }
    Ok(None)
}

/// The unified diff of one file between two trees, cut at `max_bytes` on a character boundary.
/// The bool says whether it was cut. A file that did not change gives an empty patch.
#[allow(dead_code)] // first caller: `checkpoints/bisect/` (see `list_all`)
pub fn patch_between(project: &Path, from: &str, to: &str, path: &str, max_bytes: usize) -> Result<(String, bool), CheckpointError> {
    let shadow = shadow_at(project, &[from, to])?;
    if safe_relative(path).is_none() {
        return Err(CheckpointError::Other("That file name is not allowed.".into()));
    }
    let out = shadow.run(&["diff", "--no-color", "--no-renames", "-U3", from, to, "--", path], None, None, &[])?;
    let mut patch = String::from_utf8_lossy(&out.stdout).into_owned();
    let truncated = patch.len() > max_bytes;
    if truncated {
        let mut end = max_bytes;
        while end > 0 && !patch.is_char_boundary(end) {
            end -= 1;
        }
        patch.truncate(end);
    }
    Ok((patch, truncated))
}

/// Whether `tree` is a tree object in this folder's save points. Any problem reads as `false`.
#[allow(dead_code)] // first caller: `checkpoints/bisect/` (see `list_all`)
pub fn tree_exists(project: &Path, tree: &str) -> bool {
    let Ok(shadow) = shadow_at(project, &[tree]) else { return false };
    let spec = format!("{tree}^{{tree}}");
    shadow.run(&["cat-file", "-e", spec.as_str()], None, None, &[]).is_ok()
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
        Kind::Stop => format!("Before stopping (turn {turn})"),
        Kind::Undo => "Before undo".into(),
    }
}

/// Untracked + modified files (per the pane index): those over the per-file limit, and a guard
/// on the total size of the rest.
pub(super) fn scan(shadow: &Shadow, index: &Path, limits: &Limits) -> Result<Vec<String>, CheckpointError> {
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

pub(super) fn stage_all(shadow: &Shadow, index: &Path, big: &[String]) -> Result<(), CheckpointError> {
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

pub(crate) fn numstat(shadow: &Shadow, from: &str, to: &str) -> Result<Vec<(String, u32, u32, bool)>, CheckpointError> {
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
    snapshot_numbered(project, pane, kind, note, now_ms, limits, None)
}

/// Like `snapshot`, but the checkpoint carries the caller's turn number (the pane's real turn
/// count, which also counts turns that changed nothing and so left no checkpoint).
pub fn snapshot_numbered(
    project: &Path,
    pane: &str,
    kind: Kind,
    note: Option<&str>,
    now_ms: u64,
    limits: &Limits,
    turn: Option<u32>,
) -> Result<Snapshot, CheckpointError> {
    let shadow = prepare(project)?;
    let gate = lock_for(&shadow);
    let _held = gate.lock();
    snapshot_locked(&shadow, pane, kind, note, now_ms, limits, turn)
}

fn snapshot_locked(
    shadow: &Shadow,
    pane: &str,
    kind: Kind,
    note: Option<&str>,
    now_ms: u64,
    limits: &Limits,
    turn_hint: Option<u32>,
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
    let turn = turn_hint.filter(|t| *t > 0).unwrap_or_else(|| match (kind, newest.map(|c| c.kind.as_str())) {
        (Kind::Before, None | Some("after") | Some("undo")) => max_turn + 1,
        _ => max_turn.max(1),
    });
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
        // Occasional housekeeping, off the snapshot path: `git gc --auto` can run for seconds.
        schedule_gc(shadow);
    }
    Ok(Snapshot::Created(parse_message(n.to_string(), &commit, &tree, pane, &message)))
}

/// Shadow repositories with a `git gc` in flight (one at a time per repository).
static GC_RUNNING: Lazy<Mutex<std::collections::HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(Default::default()));

/// Thread that runs `git gc`. Never the caller's: snapshots, the status hooks and the PTY reader
/// must not wait for it.
pub(crate) const GC_THREAD: &str = "pm-checkpoint-gc";

/// Run `git gc --auto` on a short-lived worker thread. It does not take the snapshot lock: git
/// makes gc safe next to other git commands, and `gc.pruneExpire` (one day) keeps objects a
/// concurrent snapshot has written but not yet referenced. A second request while one is
/// running is dropped; the next tenth snapshot asks again.
fn schedule_gc(shadow: &Shadow) {
    if !GC_RUNNING.lock().insert(shadow.git_dir.clone()) {
        return;
    }
    let shadow = shadow.clone();
    let spawned = std::thread::Builder::new().name(GC_THREAD.into()).spawn({
        let shadow = shadow.clone();
        move || {
            #[cfg(test)]
            gc_test_log::record(&shadow.git_dir);
            // Failures are not worth surfacing. `autoDetach=false`: this thread is the background.
            let _ = shadow.run(&["-c", "gc.auto=6700", "-c", "gc.autoDetach=false", "gc", "--auto", "--quiet"], None, None, &[]);
            GC_RUNNING.lock().remove(&shadow.git_dir);
        }
    });
    if spawned.is_err() {
        GC_RUNNING.lock().remove(&shadow.git_dir);
    }
}

/// Test probe: which thread ran gc for which repository.
#[cfg(test)]
pub(crate) mod gc_test_log {
    use std::path::{Path, PathBuf};

    pub(crate) static RAN: parking_lot::Mutex<Vec<(PathBuf, Option<String>)>> = parking_lot::Mutex::new(Vec::new());

    pub(super) fn record(git_dir: &Path) {
        RAN.lock().push((git_dir.to_path_buf(), std::thread::current().name().map(str::to_string)));
    }
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

pub(crate) fn find(items: &[Checkpoint], id: &str) -> Result<usize, CheckpointError> {
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
pub(crate) fn changes_between(shadow: &Shadow, prev: &str, tree: &str) -> Result<Vec<FileChange>, CheckpointError> {
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

/// The patch of the newest finished turn of `pane`.
#[derive(Debug, Clone)]
pub struct TurnPatch {
    pub turn: u32,
    /// Unified diff of "before turn n" against "after turn n" (paths relative to the project).
    pub patch: String,
    /// Cut at `max_bytes`.
    pub truncated: bool,
}

/// The unified diff of the latest finished turn: the newest "after" checkpoint against the
/// newest "before" or "after" checkpoint taken earlier (the state the turn started from; a turn
/// whose "before" was skipped because nothing had changed since the previous "after" starts there).
/// `None` when there is no such pair (never snapshotted, the turn is still running, or nothing
/// changed). Used by "Second opinion" to show a reviewer what the turn did.
pub fn last_turn_patch(project: &Path, pane: &str, max_bytes: usize) -> Result<Option<TurnPatch>, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Ok(None);
    }
    let items = list_in(&shadow, pane)?;
    let Some(at) = items.iter().rposition(|c| c.kind == "after") else { return Ok(None) };
    let after = &items[at];
    let Some(before) = items[..at].iter().rev().find(|c| c.kind == "before" || c.kind == "after") else {
        return Ok(None);
    };
    if before.tree == after.tree {
        return Ok(None);
    }
    let turn = after.turn;
    let (patch, truncated) = unified_patch(&shadow, &before.tree, &after.tree, max_bytes)?;
    Ok(Some(TurnPatch { turn, patch, truncated }))
}

/// The unified diff between two trees, cut at `max_bytes` on a character boundary.
fn unified_patch(shadow: &Shadow, from: &str, to: &str, max_bytes: usize) -> Result<(String, bool), CheckpointError> {
    let out = shadow.run(&["diff", "--no-color", "--no-renames", "-U3", from, to], None, None, &[])?;
    let mut patch = String::from_utf8_lossy(&out.stdout).into_owned();
    let truncated = patch.len() > max_bytes;
    if truncated {
        let mut end = max_bytes;
        while end > 0 && !patch.is_char_boundary(end) {
            end -= 1;
        }
        patch.truncate(end);
    }
    Ok((patch, truncated))
}

/// The unified diff from checkpoint `from_id` to checkpoint `to_id` of `pane` (paths relative to the
/// project), for Review Desk's "Look" on a range of turns. `None` when the two trees are the same.
/// Unknown ids are `NotFound`. Cut at `max_bytes`, and `truncated` says so.
#[allow(dead_code)] // read by Review Desk (its sources and detail)
pub fn patch_between(
    project: &Path,
    pane: &str,
    from_id: &str,
    to_id: &str,
    max_bytes: usize,
) -> Result<Option<TurnPatch>, CheckpointError> {
    let (shadow, items) = two_checkpoints(project, pane)?;
    let from = &items[find(&items, from_id)?];
    let to = &items[find(&items, to_id)?];
    if from.tree == to.tree {
        return Ok(None);
    }
    let (patch, truncated) = unified_patch(&shadow, &from.tree, &to.tree, max_bytes)?;
    Ok(Some(TurnPatch { turn: to.turn, patch, truncated }))
}

/// Files that differ between checkpoint `from_id` and checkpoint `to_id` of `pane`, sorted by path.
/// Unknown ids are `NotFound`.
#[allow(dead_code)] // read by Review Desk (its sources and detail)
pub fn changes_between_ids(project: &Path, pane: &str, from_id: &str, to_id: &str) -> Result<Vec<FileChange>, CheckpointError> {
    let (shadow, items) = two_checkpoints(project, pane)?;
    let from = &items[find(&items, from_id)?];
    let to = &items[find(&items, to_id)?];
    changes_between(&shadow, &from.tree, &to.tree)
}

/// The shadow repository of `project` and the checkpoints of `pane`, oldest first. Shared by the
/// range functions above: no Git means `GitMissing`, no shadow repository means `NotFound`.
fn two_checkpoints(project: &Path, pane: &str) -> Result<(Shadow, Vec<Checkpoint>), CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let shadow = Shadow::new(project);
    if !shadow.exists() {
        return Err(CheckpointError::NotFound);
    }
    let items = list_in(&shadow, pane)?;
    Ok((shadow, items))
}

/// The checkpoint a turn that started at `started_at_ms` began from: the newest "before" or "after"
/// taken at (or within `TURN_START_SLACK_MS` of) the start. Items are oldest first.
#[allow(dead_code)] // read by Review Desk (its sources and detail)
pub fn before_of(items: &[Checkpoint], started_at_ms: u64) -> Option<&Checkpoint> {
    let limit = started_at_ms.saturating_add(TURN_START_SLACK_MS);
    items.iter().rev().find(|c| (c.kind == "before" || c.kind == "after") && c.at_ms <= limit)
}

/// The newest "after" checkpoint (the end of the latest finished turn). Items are oldest first.
#[allow(dead_code)] // read by Review Desk (its sources and detail)
pub fn latest_after(items: &[Checkpoint]) -> Option<&Checkpoint> {
    items.iter().rev().find(|c| c.kind == "after")
}

pub(crate) fn name_status(shadow: &Shadow, from: &str, to: &str) -> Result<Vec<(char, String)>, CheckpointError> {
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

pub(crate) fn safe_relative(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))) {
        Some(p.to_path_buf())
    } else {
        None
    }
}

pub(crate) fn remove_file_forced(path: &Path) -> std::io::Result<()> {
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
    let safety = snapshot_locked(&shadow, pane, Kind::Undo, None, now_ms, limits, None)?;
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

#[cfg(test)]
mod all_tests {
    use super::*;
    use crate::checkpoints::test_snapshot;

    /// A throw-away folder under the temp dir (its own copy of the checkpoint tests' helper).
    /// Git objects are read-only on Windows, so drop makes them writable before removing them.
    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("pm-ckpt-engine-{tag}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
        fn p(&self) -> &Path {
            &self.0
        }
        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.0.join(rel)).unwrap()
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            fn unlock(dir: &Path) {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            unlock(&p);
                        } else if let Ok(m) = std::fs::metadata(&p) {
                            let mut perms = m.permissions();
                            #[allow(clippy::permissions_set_readonly_false)]
                            perms.set_readonly(false);
                            let _ = std::fs::set_permissions(&p, perms);
                        }
                    }
                }
            }
            unlock(&self.0);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const T0: u64 = 1_000;

    fn snap(d: &Dir, pane: &str, kind: Kind, at: u64) -> Checkpoint {
        match test_snapshot(d.p(), pane, kind, None, at, &Limits::default()).expect("snapshot") {
            Snapshot::Created(c) => c,
            Snapshot::Unchanged(_) => panic!("expected a new checkpoint"),
        }
    }

    #[test]
    fn list_all_returns_every_pane_key_oldest_first() {
        let d = Dir::new("listall");
        d.write("a.txt", "1");
        snap(&d, "p1", Kind::Before, 1_000);
        d.write("b.txt", "2");
        let p2 = snap(&d, "p2", Kind::Before, 2_000);
        d.write("a.txt", "3");
        let wb = snap(&d, "wb-abc", Kind::After, 2_500);
        d.write("a.txt", "4");
        let p1b = snap(&d, "p1", Kind::After, 3_000);

        let all = list_all(d.p()).unwrap();
        let got: Vec<(&str, &str, u64)> = all.iter().map(|c| (c.pane_id.as_str(), c.id.as_str(), c.at_ms)).collect();
        assert_eq!(got, vec![("p1", "1", 1_000), ("p2", "1", 2_000), ("wb-abc", "1", 2_500), ("p1", "2", 3_000)]);
        assert_eq!(all[1].tree, p2.tree);
        assert_eq!(all[2].tree, wb.tree);
        assert_eq!(all[3].tree, p1b.tree);
        assert_eq!(all[3].kind, "after");
        assert_eq!(list(d.p(), "p1").unwrap().len(), 2, "the per-pane list is unchanged");
        assert!(list_all(Dir::new("listall-none").p()).unwrap().is_empty(), "a folder never snapshotted");
    }

    #[test]
    fn tree_bytes_adds_up_the_files_of_a_tree() {
        let d = Dir::new("treebytes");
        d.write("a.txt", "12345");
        d.write("sub/b.txt", "xyz");
        d.write("node_modules/skip.txt", "not in a save point");
        let c = snap(&d, "p1", Kind::Before, T0);
        assert_eq!(tree_bytes(d.p(), &c.tree).unwrap(), 8);
        assert_eq!(tree_bytes(d.p(), EMPTY_TREE).unwrap(), 0);
        assert!(tree_bytes(d.p(), &"0".repeat(40)).is_err(), "a tree that does not exist");
        assert!(tree_bytes(d.p(), "a.txt; rm").is_err(), "only object ids reach git");
    }

    #[test]
    fn file_at_reads_present_files_and_says_none_for_the_rest() {
        let d = Dir::new("fileat");
        d.write("a.txt", "12345");
        d.write("sub/b.txt", "xyz");
        let first = snap(&d, "p1", Kind::Before, T0);
        d.write("a.txt", "changed");
        snap(&d, "p1", Kind::After, 2_000);

        assert_eq!(file_at(d.p(), &first.tree, "a.txt").unwrap(), Some(b"12345".to_vec()), "the old tree keeps the old bytes");
        assert_eq!(file_at(d.p(), &first.tree, "sub/b.txt").unwrap(), Some(b"xyz".to_vec()));
        assert_eq!(file_at(d.p(), &first.tree, "missing.txt").unwrap(), None);
        assert_eq!(file_at(d.p(), &first.tree, "sub").unwrap(), None, "a folder is not a file");
        assert!(file_at(d.p(), &first.tree, "../a.txt").is_err());
        assert!(file_at(d.p(), &first.tree, "").is_err());
    }

    #[test]
    fn file_at_refuses_a_file_over_20_mib() {
        let d = Dir::new("fileatbig");
        let big = vec![b'x'; 21 * 1024 * 1024];
        std::fs::write(d.p().join("big.bin"), &big).unwrap();
        let roomy = Limits { max_file_bytes: 64 * 1024 * 1024, ..Limits::default() };
        let Snapshot::Created(c) = test_snapshot(d.p(), "p1", Kind::Before, None, T0, &roomy).unwrap() else {
            panic!("expected a checkpoint");
        };
        assert_eq!(tree_bytes(d.p(), &c.tree).unwrap(), big.len() as u64);
        assert!(file_at(d.p(), &c.tree, "big.bin").is_err(), "over the 20 MiB read cap");
    }

    #[test]
    fn patch_between_cuts_on_a_character_boundary_and_says_so() {
        let d = Dir::new("patchcut");
        d.write("a.txt", "one\ntwo\nthree\n");
        let before = snap(&d, "p1", Kind::Before, T0);
        d.write("a.txt", "one\ntwo\nthree\nfour é ü\n");
        let after = snap(&d, "p1", Kind::After, 2_000);

        let (full, cut) = patch_between(d.p(), &before.tree, &after.tree, "a.txt", 1_000_000).unwrap();
        assert!(!cut);
        assert!(full.contains("+four é ü"));
        // `é` is two bytes: a cut at the middle of it steps back to the character start.
        let at = full.find('é').unwrap();
        for (max, want) in [(0, 0), (at, at), (at + 1, at)] {
            let (short, cut) = patch_between(d.p(), &before.tree, &after.tree, "a.txt", max).unwrap();
            assert!(cut, "{max} bytes is less than the whole patch");
            assert_eq!(short.len(), want, "cut at {max} bytes");
            assert!(full.starts_with(&short));
        }
        let (none, cut) = patch_between(d.p(), &before.tree, &after.tree, "other.txt", 100).unwrap();
        assert_eq!((none.as_str(), cut), ("", false), "a file that did not change");
        assert!(patch_between(d.p(), &before.tree, &after.tree, "../a.txt", 100).is_err());
    }

    #[test]
    fn tree_exists_is_true_only_for_a_save_point_tree_in_this_folder() {
        let d = Dir::new("treeexists");
        d.write("a.txt", "1");
        let c = snap(&d, "p1", Kind::Before, T0);
        assert!(tree_exists(d.p(), &c.tree));
        assert!(!tree_exists(d.p(), &"0".repeat(40)));
        assert!(!tree_exists(d.p(), "not a tree"));
        assert!(!tree_exists(Dir::new("treeexists-none").p(), &c.tree), "a folder with no save points");
    }

    #[test]
    fn object_ids_must_be_40_lowercase_hex() {
        assert!(is_object_id(EMPTY_TREE));
        assert!(!is_object_id(&EMPTY_TREE.to_uppercase()));
        assert!(!is_object_id("4b825dc6"));
        assert!(!is_object_id("--output=/tmp/x"));
    }

    #[test]
    fn with_work_tree_checks_out_into_the_copy_and_never_the_project() {
        let d = Dir::new("worktree");
        d.write("a.txt", "saved");
        let c = snap(&d, "p1", Kind::Before, T0);
        d.write("a.txt", "edited since");
        let copy = Dir::new("worktree-copy");
        let scratch = Dir::new("worktree-index");
        let index = scratch.p().join("copy.idx");
        let shadow = Shadow::new(d.p());
        let on_copy = shadow.with_work_tree(copy.p());
        assert_eq!(on_copy.git_dir, shadow.git_dir, "the same repository");
        on_copy.run(&["read-tree", c.tree.as_str()], Some(index.as_path()), None, &[]).unwrap();
        on_copy.run(&["checkout-index", "-a", "-f"], Some(index.as_path()), None, &[]).unwrap();
        assert_eq!(copy.read("a.txt"), "saved");
        assert_eq!(d.read("a.txt"), "edited since", "the project folder is untouched");
        assert_eq!(list_all(d.p()).unwrap().len(), 1, "no save point was added");
    }
}
