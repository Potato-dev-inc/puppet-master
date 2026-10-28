//! Per-turn undo: put back only what one agent turn changed, all or nothing (SPEC n33 section 3.5, S8).
//!
//! `plan` compares, for every path the turn changed, the bytes before the turn (B), after it (A), and
//! on disk now (C). It works out what an undo writes or deletes. A path that cannot be put back
//! cleanly is a `Conflict`, and then `apply` writes nothing at all. Text files are merged with
//! `git merge-file` (the change A to B is applied onto C), so later edits on other lines stay.
//! A safety save point is taken before the first write.
//!
//! `decide` is the pure case table and does no I/O. Git is read in `plan`; the folder is read in
//! `plan` and written in `apply`. Symlinks and anything that is not a plain file are never followed
//! or written: a turn that changed one is a conflict. Only paths the turn itself changed are touched.
//! Module-level dead-code allowance: `live.rs` (WB-13) is the first caller of this API.
#![allow(dead_code)]

use super::words;
use crate::checkpoints::{
    engine::{self, CheckpointError},
    git::Shadow,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// The same cap `engine::file_at` uses (20 MiB). Bigger files are conflicts, never read.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// The series of the safety save point taken before an undo writes (SPEC n33 section 3.5).
pub const SAFETY_PANE: &str = "wb-undo";
const SAFETY_NOTE: &str = "What broke it: before undo";

/// What `apply` does to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Write these bytes (the file as it was before the turn). Creates missing folders.
    Write(Vec<u8>),
    /// Delete the file (the turn created it and nothing has changed it since).
    Remove,
}

/// One file of a plan. `was` holds what the file held when the plan was made: `apply` checks it
/// still holds it, and puts it back if a later write fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAction {
    pub path: String,
    pub op: Op,
    pub was: Option<Vec<u8>>,
}

/// A path an undo cannot put back cleanly. `why` is plain English (see `words`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub why: String,
}

/// The undo of one turn. Only `apply` writes; a plan with any conflict is refused as a whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevertPlan {
    pub files: Vec<FileAction>,
    pub conflicts: Vec<Conflict>,
}

/// What `apply` wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub written: u32,
    pub removed: u32,
    /// The safety save point taken first. `None` when nothing needed writing.
    pub safety_id: Option<String>,
}

/// Why `apply` did not undo. Each variant means nothing was left half done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertError {
    /// The plan has conflicts. Nothing was written.
    Refused(Vec<Conflict>),
    /// The file at this path changed after the plan was made. Nothing was written: plan again.
    Changed(String),
    /// The safety save point could not be taken. Nothing was written.
    NoSafetyPoint(String),
    /// A write failed at this path. Every earlier write was put back.
    Write(String),
}

/// What to do with one path, from its three versions (see `decide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Already as it was before the turn, or the turn never touched it.
    Nothing,
    /// The turn created the file and nothing changed it since: delete it.
    Remove,
    /// Nothing changed it since the turn: write the before bytes.
    Restore,
    /// Later work changed the file too, in text: merge the change away with `git merge-file`.
    Merge,
    /// Cannot be undone cleanly.
    Conflict(Why),
}

/// Why a path is a conflict. Each maps to one plain sentence in `words`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    SameLines,
    DeletedLater,
    CreatedThenEdited,
    Binary,
    Unreadable,
}

impl Why {
    fn words(self) -> &'static str {
        match self {
            Why::SameLines => words::conflict_same_lines(),
            Why::DeletedLater => words::conflict_deleted_later(),
            Why::CreatedThenEdited => words::conflict_created_then_edited(),
            Why::Binary => words::conflict_binary(),
            Why::Unreadable => words::conflict_unreadable(),
        }
    }
}

/// The case table of S8. `a` = the bytes after the turn, `b` = before it, `c` = on disk now.
/// `None` means the file is absent. The cases are checked in this order: already back, then
/// unchanged since the turn, then the cases where the turn and later work both touched the path.
pub fn decide(a: Option<&[u8]>, b: Option<&[u8]>, c: Option<&[u8]>) -> Action {
    if c == b {
        return Action::Nothing;
    }
    if c == a {
        return if b.is_none() { Action::Remove } else { Action::Restore };
    }
    match (a.is_some(), b.is_some(), c.is_some()) {
        // Neither side has the file: the turn never touched it.
        (false, false, true) => Action::Nothing,
        // The turn deleted it; later work made a new file at the same path.
        (false, true, true) => Action::Conflict(Why::SameLines),
        // The turn created it and later work edited it.
        (true, false, true) => Action::Conflict(Why::CreatedThenEdited),
        // The turn changed it and later work deleted it.
        (true, true, false) => Action::Conflict(Why::DeletedLater),
        (true, true, true) => {
            if [a, b, c].iter().flatten().any(|bytes| bytes.contains(&0)) {
                Action::Conflict(Why::Binary)
            } else {
                Action::Merge
            }
        }
        // Unreachable: `c == a` and `c == b` were handled above.
        _ => Action::Conflict(Why::SameLines),
    }
}

/// The git path as a path under the folder, or `None` when it is not a plain relative name. Refuses
/// `..`, absolute and drive paths, `:` (Windows streams), `\` (Windows would split it into folders),
/// and the app's own save point folder.
pub fn check_path(path: &str) -> Option<PathBuf> {
    let first = path.split('/').next().unwrap_or("");
    if path.contains('\\') || path.contains(':') || first.eq_ignore_ascii_case(".puppet-master") {
        return None;
    }
    engine::safe_relative(path)
}

/// A file as the turn saw it (a save point) or as it is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Version {
    Absent,
    Bytes(Vec<u8>),
    /// A symlink, a folder, a submodule, a file over the cap, or a read error. Never followed.
    Unreadable,
}

impl Version {
    fn as_slice(&self) -> Option<&[u8]> {
        match self {
            Version::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Version::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }
}

fn is_tree_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn conflict(path: &str, why: Why) -> Conflict {
    Conflict { path: path.to_string(), why: why.words().to_string() }
}

/// The file at `path` in `tree`. A git failure is an error; a symlink, a submodule, or an
/// oversized blob is `Unreadable`.
fn load_side(shadow: &Shadow, tree: &str, path: &str) -> Result<Version, CheckpointError> {
    let listing = shadow.run(&["ls-tree", "-z", "-l", tree, "--", path], None, None, &[])?;
    for record in listing.stdout.split(|b| *b == 0) {
        let line = String::from_utf8_lossy(record);
        let Some((meta, name)) = line.split_once('\t') else { continue };
        if name != path {
            continue;
        }
        // `<mode> <type> <object> <size>`. A symlink is a blob with mode 120000; a submodule is a commit.
        let fields: Vec<&str> = meta.split_whitespace().collect();
        let (mode, kind, object, size) = match fields.as_slice() {
            [mode, kind, object, size] => (*mode, *kind, *object, *size),
            _ => return Ok(Version::Unreadable),
        };
        if kind != "blob" || mode == "120000" {
            return Ok(Version::Unreadable);
        }
        if size.parse::<u64>().map_or(true, |n| n > MAX_FILE_BYTES) {
            return Ok(Version::Unreadable);
        }
        let bytes = shadow.run(&["cat-file", "blob", object], None, None, &[])?.stdout;
        return Ok(Version::Bytes(bytes));
    }
    Ok(Version::Absent)
}

/// The file at `rel` under `project` now. Any symlink on the way (or a folder where the file should
/// be, or a file over the cap) is `Unreadable`: it is never followed.
fn read_now(project: &Path, rel: &Path) -> Version {
    let parts: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    let mut here = project.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        here.push(part);
        let meta = match fs::symlink_metadata(&here) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Version::Absent,
            Err(_) => return Version::Unreadable,
        };
        if meta.file_type().is_symlink() {
            return Version::Unreadable;
        }
        if i + 1 < parts.len() {
            if !meta.is_dir() {
                return Version::Unreadable;
            }
            continue;
        }
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Version::Unreadable;
        }
        return match fs::read(&here) {
            Ok(bytes) => Version::Bytes(bytes),
            Err(_) => Version::Unreadable,
        };
    }
    Version::Unreadable
}

/// Applies the change `a` to `b` onto `c` with `git merge-file`. `None` when the merge is not clean.
/// The three texts go to temporary files in the shadow repo's private folder and are removed after.
fn merge_text(shadow: &Shadow, a: &[u8], b: &[u8], c: &[u8]) -> Option<Vec<u8>> {
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let paths: Vec<PathBuf> = ["cur", "base", "other"]
        .iter()
        .map(|name| shadow.git_dir.join(format!("pm-merge-{tag}-{name}")))
        .collect();
    let stored = paths.iter().zip([c, a, b]).all(|(path, bytes)| fs::write(path, bytes).is_ok());
    let merged = if stored {
        let mut args: Vec<OsString> = vec!["merge-file".into(), "-p".into()];
        args.extend(paths.iter().map(|p| p.clone().into_os_string()));
        // `merge-file` exits non-zero on a conflict, which `run` reports as an error: not clean.
        shadow.run(&args[..], None, None, &[]).ok().map(|out| out.stdout)
    } else {
        None
    };
    for path in &paths {
        let _ = fs::remove_file(path);
    }
    merged
}

/// Works out the undo of the turn that changed `before_tree` into `after_tree`. Reads only; writes
/// nothing. `Err` only when git cannot read the save points. Every problem with one file is a
/// `Conflict`, so the caller refuses the whole undo.
pub fn plan(shadow: &Shadow, before_tree: &str, after_tree: &str, project: &Path) -> Result<RevertPlan, CheckpointError> {
    if ![before_tree, after_tree].iter().all(|tree| is_tree_id(tree)) {
        return Err(CheckpointError::Other("That save point is not valid.".into()));
    }
    let mut out = RevertPlan::default();
    for (_, path) in engine::name_status(shadow, before_tree, after_tree)? {
        let Some(rel) = check_path(&path) else {
            out.conflicts.push(conflict(&path, Why::Unreadable));
            continue;
        };
        let before = load_side(shadow, before_tree, &path)?;
        let after = load_side(shadow, after_tree, &path)?;
        let now = read_now(project, &rel);
        if [&before, &after, &now].iter().any(|version| matches!(version, Version::Unreadable)) {
            out.conflicts.push(conflict(&path, Why::Unreadable));
            continue;
        }
        let (b, a, c) = (before.as_slice(), after.as_slice(), now.as_slice());
        let op = match decide(a, b, c) {
            Action::Nothing => continue,
            Action::Remove => Op::Remove,
            Action::Restore => match b {
                Some(bytes) => Op::Write(bytes.to_vec()),
                None => {
                    out.conflicts.push(conflict(&path, Why::Unreadable));
                    continue;
                }
            },
            Action::Merge => {
                let merged = match (a, b, c) {
                    (Some(after), Some(before), Some(now_bytes)) => {
                        merge_text(shadow, after, before, now_bytes).map(|bytes| (bytes, now_bytes))
                    }
                    _ => None,
                };
                match merged {
                    None => {
                        out.conflicts.push(conflict(&path, Why::SameLines));
                        continue;
                    }
                    // Later work already has exactly this result: nothing to write.
                    Some((bytes, now_bytes)) if bytes.as_slice() == now_bytes => continue,
                    Some((bytes, _)) => Op::Write(bytes),
                }
            }
            Action::Conflict(why) => {
                out.conflicts.push(conflict(&path, why));
                continue;
            }
        };
        out.files.push(FileAction { path, op, was: now.into_bytes() });
    }
    Ok(out)
}

/// Writes `bytes` at `rel` under `project` (creating folders), or deletes the file when `bytes` is
/// `None`, then removes the folders that the delete left empty. Never goes above `project`.
fn put(project: &Path, rel: &Path, bytes: Option<&[u8]>) -> std::io::Result<()> {
    let abs = project.join(rel);
    match bytes {
        Some(data) => {
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, data)
        }
        None => {
            engine::remove_file_forced(&abs)?;
            let mut dir = abs.parent().map(Path::to_path_buf);
            while let Some(d) = dir {
                if d.as_path() == project || !d.starts_with(project) || fs::remove_dir(&d).is_err() {
                    break;
                }
                dir = d.parent().map(Path::to_path_buf);
            }
            Ok(())
        }
    }
}

/// Applies a plan, all or nothing. It refuses a plan with conflicts, and refuses when any file has
/// changed since the plan was made (re-plan then). Only then does it take the safety save point
/// (`SAFETY_PANE`, kind undo), and write. If a write fails, every earlier write is put back.
/// `now_ms` and `limits` are the caller's clock and checkpoint budget.
pub fn apply(project: &Path, plan: &RevertPlan, now_ms: u64, limits: &engine::Limits) -> Result<ApplyReport, RevertError> {
    if !plan.conflicts.is_empty() {
        return Err(RevertError::Refused(plan.conflicts.clone()));
    }
    let mut rels = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let rel = check_path(&file.path).ok_or_else(|| RevertError::Changed(file.path.clone()))?;
        let now = read_now(project, &rel);
        if matches!(now, Version::Unreadable) || now.as_slice() != file.was.as_deref() {
            return Err(RevertError::Changed(file.path.clone()));
        }
        rels.push(rel);
    }
    if plan.files.is_empty() {
        return Ok(ApplyReport::default());
    }
    let safety = engine::snapshot(project, SAFETY_PANE, engine::Kind::Undo, Some(SAFETY_NOTE), now_ms, limits)
        .map_err(|e| RevertError::NoSafetyPoint(e.to_string()))?;
    let safety_id = match safety {
        engine::Snapshot::Created(cp) => Some(cp.id),
        engine::Snapshot::Unchanged(cp) => cp.map(|cp| cp.id),
    };
    let mut done: Vec<usize> = Vec::new();
    for (i, file) in plan.files.iter().enumerate() {
        done.push(i);
        let bytes = match &file.op {
            Op::Write(data) => Some(data.as_slice()),
            Op::Remove => None,
        };
        if put(project, &rels[i], bytes).is_err() {
            // Put every file back the way the plan found it, newest first.
            for &j in done.iter().rev() {
                let _ = put(project, &rels[j], plan.files[j].was.as_deref());
            }
            return Err(RevertError::Write(file.path.clone()));
        }
    }
    Ok(ApplyReport {
        written: plan.files.iter().filter(|f| matches!(f.op, Op::Write(_))).count() as u32,
        removed: plan.files.iter().filter(|f| f.op == Op::Remove).count() as u32,
        safety_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throw-away folder under the system temp dir, removed on drop.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("pm-wb-revert-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).expect("temp dir");
            Tmp(dir)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            clear_readonly(&self.0);
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[allow(clippy::permissions_set_readonly_false)]
    fn clear_readonly(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                clear_readonly(&path);
            }
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    perms.set_readonly(false);
                    let _ = fs::set_permissions(&path, perms);
                }
            }
        }
    }

    fn repo() -> (Tmp, PathBuf, Shadow) {
        let tmp = Tmp::new();
        let project = tmp.0.join("project");
        fs::create_dir_all(&project).expect("project dir");
        let shadow = Shadow::new(&project);
        shadow.ensure(&[]).expect("shadow repo");
        (tmp, project, shadow)
    }

    /// A tree holding exactly `files` (path, mode, text), built with git plumbing in a throw-away index.
    fn tree_of(shadow: &Shadow, files: &[(&str, &str, &str)]) -> String {
        let index = shadow.git_dir.join(format!("test-index-{}", uuid::Uuid::new_v4()));
        let mut info = Vec::new();
        for (path, mode, text) in files {
            let oid = shadow
                .run(&["hash-object", "-w", "--stdin"], None, Some(text.as_bytes()), &[])
                .expect("hash-object")
                .text();
            info.extend_from_slice(format!("{mode} {oid}\t{path}\0").as_bytes());
        }
        let index_path = index.as_path();
        shadow.run(&["update-index", "-z", "--index-info"], Some(index_path), Some(info.as_slice()), &[]).expect("update-index");
        let tree = shadow.run(&["write-tree"], Some(index_path), None, &[]).expect("write-tree").text();
        let _ = fs::remove_file(&index);
        tree
    }

    fn disk(project: &Path, rel: &str, text: &str) {
        let abs = project.join(rel);
        fs::create_dir_all(abs.parent().expect("parent")).expect("parent dir");
        fs::write(abs, text.as_bytes()).expect("write disk");
    }

    fn read(project: &Path, rel: &str) -> Vec<u8> {
        fs::read(project.join(rel)).expect("read disk")
    }

    fn limits() -> engine::Limits {
        engine::Limits::default()
    }

    fn no_safety_point(project: &Path) -> bool {
        engine::list_all(project).expect("list").iter().all(|cp| cp.kind != "undo")
    }

    #[cfg(windows)]
    fn try_symlink(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    #[cfg(not(windows))]
    fn try_symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[test]
    fn decide_covers_every_case() {
        let (x, y, z) = (b"one\n".to_vec(), b"two\n".to_vec(), b"three\n".to_vec());
        let nul = b"\0\x01\x02".to_vec();
        let cases: Vec<(Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>, Action)> = vec![
            // already back (c == b), and unchanged since the turn (c == a)
            (Some(x.clone()), Some(y.clone()), Some(y.clone()), Action::Nothing),
            (Some(x.clone()), Some(y.clone()), Some(x.clone()), Action::Restore),
            (Some(x.clone()), None, Some(x.clone()), Action::Remove),
            (None, Some(y.clone()), None, Action::Restore),
            (None, None, None, Action::Nothing),
            // later work touched the path
            (Some(x.clone()), Some(y.clone()), Some(z.clone()), Action::Merge),
            (Some(x.clone()), Some(y.clone()), None, Action::Conflict(Why::DeletedLater)),
            (Some(x.clone()), None, Some(z.clone()), Action::Conflict(Why::CreatedThenEdited)),
            (None, Some(y.clone()), Some(z.clone()), Action::Conflict(Why::SameLines)),
            (None, None, Some(z.clone()), Action::Nothing),
            (Some(nul.clone()), Some(y.clone()), Some(z.clone()), Action::Conflict(Why::Binary)),
        ];
        for (a, b, c, want) in cases {
            assert_eq!(decide(a.as_deref(), b.as_deref(), c.as_deref()), want, "a={a:?} b={b:?} c={c:?}");
        }
    }

    #[test]
    fn check_path_refuses_names_that_escape_or_split() {
        for bad in ["", "../x", "a/../b", "/abs", "C:x", "a:stream", "a\\b", ".puppet-master/x", ".PUPPET-MASTER/x"] {
            assert!(check_path(bad).is_none(), "{bad} must be refused");
        }
        assert_eq!(check_path("src/a.txt"), Some(PathBuf::from("src/a.txt")));
    }

    #[test]
    fn later_edit_on_other_lines_is_kept() {
        let (_t, project, shadow) = repo();
        // One unchanged line separates the two edits: git merges adjacent edits as one conflict.
        let before = tree_of(&shadow, &[("src/a.txt", "100644", "one\ntwo\nmid\nthree\n")]);
        let after = tree_of(&shadow, &[("src/a.txt", "100644", "one\nTWO\nmid\nthree\n")]);
        disk(&project, "src/a.txt", "one\nTWO\nmid\nTHREE\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert!(p.conflicts.is_empty());
        assert_eq!(p.files.len(), 1);
        let report = apply(&project, &p, 1, &limits()).expect("apply");
        assert_eq!(report.written, 1);
        assert_eq!(read(&project, "src/a.txt"), "one\ntwo\nmid\nTHREE\n".as_bytes());
    }

    #[test]
    fn adjacent_edits_are_refused_as_the_same_lines() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "one\ntwo\nthree\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "one\nTWO\nthree\n")]);
        disk(&project, "a.txt", "one\nTWO\nTHREE\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert!(p.files.is_empty());
        assert_eq!(p.conflicts, vec![Conflict { path: "a.txt".into(), why: words::conflict_same_lines().into() }]);
    }

    #[test]
    fn same_lines_later_edit_is_refused() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "x\ny\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "x\nY\n")]);
        disk(&project, "a.txt", "x\nQ\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert!(p.files.is_empty());
        assert_eq!(p.conflicts, vec![Conflict { path: "a.txt".into(), why: words::conflict_same_lines().into() }]);
        assert!(apply(&project, &p, 1, &limits()).is_err());
        assert_eq!(read(&project, "a.txt"), "x\nQ\n".as_bytes());
    }

    #[test]
    fn one_conflict_writes_nothing_at_all() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("ok.txt", "100644", "a\n"), ("bad.txt", "100644", "x\ny\n")]);
        let after = tree_of(&shadow, &[("ok.txt", "100644", "A\n"), ("bad.txt", "100644", "x\nY\n")]);
        disk(&project, "ok.txt", "A\n");
        disk(&project, "bad.txt", "x\nQ\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.files.len(), 1, "ok.txt could go back on its own");
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(apply(&project, &p, 1, &limits()), Err(RevertError::Refused(p.conflicts.clone())));
        assert_eq!(read(&project, "ok.txt"), "A\n".as_bytes());
        assert_eq!(read(&project, "bad.txt"), "x\nQ\n".as_bytes());
        assert!(no_safety_point(&project), "a refused undo takes no save point");
    }

    #[test]
    fn file_deleted_later_is_refused() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "one\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "two\n")]);
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.conflicts, vec![Conflict { path: "a.txt".into(), why: words::conflict_deleted_later().into() }]);
    }

    #[test]
    fn file_added_by_turn_then_edited_later_is_refused() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[]);
        let after = tree_of(&shadow, &[("new.txt", "100644", "new\n")]);
        disk(&project, "new.txt", "new\nmore\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(
            p.conflicts,
            vec![Conflict { path: "new.txt".into(), why: words::conflict_created_then_edited().into() }]
        );
        assert!(apply(&project, &p, 1, &limits()).is_err());
        assert_eq!(read(&project, "new.txt"), "new\nmore\n".as_bytes());
    }

    #[test]
    fn file_created_by_turn_and_untouched_is_removed() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[]);
        let after = tree_of(&shadow, &[("src/made/new.txt", "100644", "x\n")]);
        disk(&project, "src/made/new.txt", "x\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.files[0].op, Op::Remove);
        let report = apply(&project, &p, 1, &limits()).expect("apply");
        assert_eq!(report.removed, 1);
        assert!(!project.join("src/made/new.txt").exists());
        assert!(!project.join("src").exists(), "folders the delete emptied are removed");
    }

    #[test]
    fn file_deleted_by_turn_comes_back() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("keep.txt", "100644", "keep\n")]);
        let after = tree_of(&shadow, &[]);
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.files.len(), 1);
        apply(&project, &p, 1, &limits()).expect("apply");
        assert_eq!(read(&project, "keep.txt"), "keep\n".as_bytes());
    }

    #[test]
    fn crlf_file_merges_and_keeps_line_endings() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("win.txt", "100644", "a\r\nb\r\nmid\r\nc\r\n")]);
        let after = tree_of(&shadow, &[("win.txt", "100644", "a\r\nB\r\nmid\r\nc\r\n")]);
        disk(&project, "win.txt", "a\r\nB\r\nmid\r\nC\r\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert!(p.conflicts.is_empty(), "{:?}", p.conflicts);
        apply(&project, &p, 1, &limits()).expect("apply");
        assert_eq!(read(&project, "win.txt"), "a\r\nb\r\nmid\r\nC\r\n".as_bytes());
    }

    #[test]
    fn binary_file_changed_later_is_refused() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("data.bin", "100644", "\0\u{1}\u{2}")]);
        let after = tree_of(&shadow, &[("data.bin", "100644", "\0\u{9}\u{2}")]);
        disk(&project, "data.bin", "\0\u{7}\u{2}");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.conflicts, vec![Conflict { path: "data.bin".into(), why: words::conflict_binary().into() }]);
    }

    #[test]
    fn binary_file_untouched_since_turn_is_put_back_byte_exact() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("data.bin", "100644", "\0\u{1}\u{2}")]);
        let after = tree_of(&shadow, &[("data.bin", "100644", "\0\u{9}\u{2}")]);
        disk(&project, "data.bin", "\0\u{9}\u{2}");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert!(p.conflicts.is_empty());
        apply(&project, &p, 1, &limits()).expect("apply");
        assert_eq!(read(&project, "data.bin"), vec![0u8, 1, 2]);
    }

    #[test]
    fn symlink_changed_by_turn_is_refused_and_never_written() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[]);
        let after = tree_of(&shadow, &[("link.txt", "120000", "target.txt")]);
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.conflicts, vec![Conflict { path: "link.txt".into(), why: words::conflict_unreadable().into() }]);
        assert!(p.files.is_empty());
        assert!(!project.join("link.txt").exists());
    }

    #[test]
    fn symlink_on_disk_is_never_followed() {
        let (tmp, project, shadow) = repo();
        let outside = tmp.0.join("outside.txt");
        fs::write(&outside, b"outside\n").expect("outside file");
        let before = tree_of(&shadow, &[("note.txt", "100644", "v1\n")]);
        let after = tree_of(&shadow, &[("note.txt", "100644", "v2\n")]);
        if !try_symlink(&outside, &project.join("note.txt")) {
            eprintln!("skipped: this account cannot create symlinks");
            return;
        }
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.conflicts, vec![Conflict { path: "note.txt".into(), why: words::conflict_unreadable().into() }]);
        assert!(apply(&project, &p, 1, &limits()).is_err());
        assert_eq!(fs::read(&outside).expect("outside still there"), b"outside\n");
    }

    #[test]
    fn already_undone_file_is_a_no_op() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "old\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "new\n")]);
        disk(&project, "a.txt", "old\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert!(p.files.is_empty());
        assert!(p.conflicts.is_empty());
        let report = apply(&project, &p, 1, &limits()).expect("apply");
        assert_eq!(report, ApplyReport::default());
        assert!(no_safety_point(&project));
    }

    #[test]
    fn apply_takes_a_safety_save_point_first() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "one\ntwo\nmid\nthree\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "one\nTWO\nmid\nthree\n")]);
        disk(&project, "a.txt", "one\nTWO\nmid\nTHREE\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        let report = apply(&project, &p, 1, &limits()).expect("apply");
        let safety_id = report.safety_id.clone().expect("a safety save point");
        let point = engine::list_all(&project)
            .expect("list")
            .into_iter()
            .find(|cp| cp.id == safety_id)
            .expect("the save point is listed");
        assert_eq!(point.kind, "undo");
        assert_eq!(point.pane_id, SAFETY_PANE);
        let kept = engine::file_at(&project, &point.tree, "a.txt").expect("read save point");
        assert_eq!(kept.as_deref(), Some("one\nTWO\nmid\nTHREE\n".as_bytes()), "the save point holds the state before undo");
    }

    #[test]
    fn disk_changed_after_the_plan_refuses_and_writes_nothing() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "old\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "new\n")]);
        disk(&project, "a.txt", "new\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        disk(&project, "a.txt", "newer, edited after the plan\n");
        assert_eq!(apply(&project, &p, 1, &limits()), Err(RevertError::Changed("a.txt".into())));
        assert_eq!(read(&project, "a.txt"), "newer, edited after the plan\n".as_bytes());
    }

    #[test]
    fn failed_write_puts_the_earlier_files_back() {
        let (_t, project, shadow) = repo();
        let before = tree_of(&shadow, &[("a.txt", "100644", "old-a\n"), ("b.txt", "100644", "old-b\n")]);
        let after = tree_of(&shadow, &[("a.txt", "100644", "new-a\n"), ("b.txt", "100644", "new-b\n")]);
        disk(&project, "a.txt", "new-a\n");
        disk(&project, "b.txt", "new-b\n");
        let p = super::plan(&shadow, &before, &after, &project).expect("plan");
        assert_eq!(p.files.len(), 2);
        // A read-only file cannot be written: the second write fails after the first succeeded.
        let blocked = project.join("b.txt");
        let mut perms = fs::metadata(&blocked).expect("meta").permissions();
        perms.set_readonly(true);
        fs::set_permissions(&blocked, perms).expect("make read-only");
        let result = apply(&project, &p, 1, &limits());
        assert_eq!(result, Err(RevertError::Write("b.txt".into())));
        assert_eq!(read(&project, "a.txt"), "new-a\n".as_bytes(), "the first write was put back");
    }

    #[test]
    fn tree_ids_are_checked_before_git_sees_them() {
        let (_t, project, shadow) = repo();
        let good = tree_of(&shadow, &[]);
        let err = super::plan(&shadow, "--upload-pack=x", &good, &project).expect_err("bad id");
        assert!(matches!(err, CheckpointError::Other(_)));
    }
}
