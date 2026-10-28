//! The throw-away test copy of a save point (SPEC n33 section 3.4).
//!
//! A `Copy` is a folder `<app data>/what-broke-it/copies/<8 hex>/` that is filled from the shadow repository,
//! never from the person's folder (S1). Its git index is private (`copies/<8 hex>.idx`, outside the work
//! tree). The first `set_tree` writes every file of the tree; later calls write and delete only the paths
//! that differ, so packages installed in the copy (not in any save point) stay where they are.
//!
//! Guards: the copy root must not be equal to, inside or around the project folder, and must not hold `..`.
//! Links in a save point are refused (a link could point into the person's folder). Removal only ever touches
//! the copy's own folder and its three side files, after checking they sit directly in the copies root.
//! `sweep_stale` removes old copies at start, never one this process still holds.
//!
//! Output of the setup steps is kept by `preset::apply` in the log file, and only its tail is returned in
//! `CopyError::SetupFailed`. The caller redacts that tail before anything is stored (S9).

// Nothing calls this module until `live.rs` (WB-13) lands; the tests exercise every item.
#![allow(dead_code)]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use parking_lot::Mutex;

use super::model::{MAX_COPY_BYTES, STALE_COPY_AGE_MS};
use crate::checkpoints::engine::{remove_file_forced, safe_relative};
use crate::checkpoints::git::{git_available, GitError, Shadow};
use crate::secure_file::create_private_dir;
use crate::worktrees::preset::{self, SetupReport, Target, WorktreePreset};
use crate::worktrees::proc::ToolEnv;

/// Folders this process made and has not finished. `sweep_stale` never touches them.
static LIVE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Why a copy could not be made, moved or prepared. `Display` is plain English and never shows a folder path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyError {
    /// Git is missing on this PC.
    GitMissing,
    /// The folder has no save points yet.
    NoSavePoints,
    /// The copy root would be equal to, inside or around the project folder, or it holds `..`.
    UnsafeRoot,
    /// The save point holds more bytes than a copy may hold (`MAX_COPY_BYTES`).
    TooBig { bytes: u64 },
    /// The save point holds a link at `path`; links are never copied.
    Link { path: String },
    /// A setup step stopped the copy. `step` is the command as typed; `tail` is raw output (redact it).
    SetupFailed { step: String, tail: String },
    /// An earlier move stopped half way, so the copy's files are not known. Make a new copy.
    Broken,
    /// A request this module refuses (bad id, unsafe path, wrong folder). The text is safe to show.
    Refused(String),
    /// A git call failed. The raw message is kept for logs only.
    Git(String),
    /// A file could not be written or removed. The raw message is kept for logs only.
    Io(String),
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyError::GitMissing => f.write_str("Git is not available, so the test copy cannot be made."),
            CopyError::NoSavePoints => f.write_str("There are no save points for this folder yet."),
            CopyError::UnsafeRoot => f.write_str("The test copies cannot be kept inside or around your project folder."),
            CopyError::TooBig { bytes } => write!(f, "The files are {} MB, too big to copy for a test.", bytes >> 20),
            CopyError::Link { path } => write!(f, "{path} is a link, and links are not copied for a test."),
            CopyError::SetupFailed { .. } => f.write_str("Getting the copy ready failed."),
            CopyError::Broken => f.write_str("The test copy is in an unknown state. It cannot be used again."),
            CopyError::Refused(text) => f.write_str(text),
            CopyError::Git(_) => f.write_str("Git could not prepare the test copy."),
            CopyError::Io(_) => f.write_str("The test copy could not be changed on disk."),
        }
    }
}

impl std::error::Error for CopyError {}

/// What one `set_tree` wrote and removed (paths, not bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Moved {
    pub written: u32,
    pub removed: u32,
}

/// One file or folder change between two trees, from `diff-tree`.
struct Change {
    status: char,
    mode: String,
    path: String,
}

/// One row of `ls-tree -r -l`.
struct Entry {
    mode: String,
    bytes: u64,
    path: String,
}

/// A throw-away folder holding one save point's files. Removed by `finish`, or on drop.
pub struct Copy {
    id: String,
    dir: PathBuf,
    root: PathBuf,
    project: PathBuf,
    shadow: Shadow,
    index: PathBuf,
    exclude: PathBuf,
    current: Option<String>,
    broken: bool,
    prepared: bool,
    removed: bool,
}

impl Copy {
    /// A new, empty copy under `root`. `project_key` is the project folder, `shadow_git_dir` its shadow
    /// repository (`.puppet-master/checkpoints.git`). Nothing is created inside the project folder.
    pub fn create(project_key: &Path, shadow_git_dir: &Path, root: &Path) -> Result<Copy, CopyError> {
        if !git_available() {
            return Err(CopyError::GitMissing);
        }
        if root.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(CopyError::UnsafeRoot);
        }
        let project = project_key.canonicalize().map_err(|e| CopyError::Io(e.to_string()))?;
        if !shadow_git_dir.join("HEAD").is_file() {
            return Err(CopyError::NoSavePoints);
        }
        // Checked before `root` is created, so a root inside the project never creates folders there.
        let real_root = real_path(root).ok_or(CopyError::UnsafeRoot)?;
        if fold_case(&real_root).starts_with(fold_case(&project)) || fold_case(&project).starts_with(fold_case(&real_root)) {
            return Err(CopyError::UnsafeRoot);
        }
        create_private_dir(root).map_err(|e| CopyError::Io(e.to_string()))?;
        let (id, dir) = make_folder(root)?;
        let copy = Copy {
            dir: dir.clone(),
            root: root.to_path_buf(),
            project,
            shadow: Shadow { work_tree: dir.clone(), git_dir: shadow_git_dir.to_path_buf() },
            index: root.join(format!("{id}.idx")),
            exclude: root.join(format!("{id}.exclude")),
            id,
            current: None,
            broken: false,
            prepared: false,
            removed: false,
        };
        LIVE.lock().push(dir);
        Ok(copy)
    }

    /// The copy folder (the test command runs here).
    pub fn folder(&self) -> &Path {
        &self.dir
    }

    /// Where `prepare` writes the setup output: `copies/<8 hex>.setup.log`.
    pub fn setup_log(&self) -> PathBuf {
        self.root.join(format!("{}.setup.log", self.id))
    }

    /// Bytes of the files in `tree` (from `ls-tree -l`). Over `MAX_COPY_BYTES` is `TooBig`.
    pub fn tree_bytes(&self, tree: &str) -> Result<u64, CopyError> {
        let bytes = self.listing(tree)?.iter().map(|e| e.bytes).sum();
        check_size(bytes, MAX_COPY_BYTES)?;
        Ok(bytes)
    }

    /// Make the copy show `tree`. The first call writes every file; later calls write and delete only the
    /// paths that differ from the last tree. A failure before any write leaves the copy usable; a failure
    /// during writes makes it `Broken`.
    pub fn set_tree(&mut self, tree: &str) -> Result<Moved, CopyError> {
        if self.removed {
            return Err(CopyError::Refused("The test copy was already removed.".into()));
        }
        if self.broken {
            return Err(CopyError::Broken);
        }
        if !is_object_id(tree) {
            return Err(CopyError::Refused("That save point is not valid.".into()));
        }
        if self.current.as_deref() == Some(tree) {
            return Ok(Moved::default());
        }
        let moved = match self.current.clone() {
            None => self.populate(tree)?,
            Some(old) => self.move_to(&old, tree)?,
        };
        self.current = Some(tree.to_string());
        Ok(moved)
    }

    /// Copy the listed files and run the setup steps once, in the copy. `None` when the preset asks for
    /// nothing. A failing step is `SetupFailed` (the copy is kept until `finish`). `project` must be the
    /// folder this copy was made for, and `log` must be `setup_log()`.
    pub fn prepare(&mut self, project: &Path, preset: &WorktreePreset, log: &Path) -> Result<Option<SetupReport>, CopyError> {
        if self.removed {
            return Err(CopyError::Refused("The test copy was already removed.".into()));
        }
        if self.prepared {
            return Err(CopyError::Refused("The setup steps already ran for this copy.".into()));
        }
        let given = project.canonicalize().map_err(|e| CopyError::Io(e.to_string()))?;
        if fold_case(&given) != fold_case(&self.project) {
            return Err(CopyError::Refused("The setup must run from the project this copy was made for.".into()));
        }
        if log != self.setup_log().as_path() {
            return Err(CopyError::Refused("The setup log must sit next to the copy.".into()));
        }
        self.prepared = true;
        if preset.is_empty() {
            return Ok(None);
        }
        let env = ToolEnv::default();
        let target = Target { project: &self.project, cwd: &self.dir, prefix: "", exclude_file: &self.exclude, log_file: log, env: &env };
        match preset::apply(&target, preset) {
            Some(report) if report.failed() => {
                let (step, tail) = report
                    .steps
                    .iter()
                    .find(|s| !s.ok)
                    .map(|s| (s.command.clone(), s.tail.clone()))
                    .unwrap_or_else(|| ("setup".into(), String::new()));
                Err(CopyError::SetupFailed { step, tail })
            }
            other => Ok(other),
        }
    }

    /// Remove the copy now. Same as dropping it, but the error is returned.
    pub fn finish(mut self) -> Result<(), CopyError> {
        self.remove()
    }

    /// First fill: every regular file of `tree`. Links are refused before anything is written.
    fn populate(&mut self, tree: &str) -> Result<Moved, CopyError> {
        let entries = self.listing(tree)?;
        check_size(entries.iter().map(|e| e.bytes).sum(), MAX_COPY_BYTES)?;
        let mut files = Vec::new();
        for entry in &entries {
            match entry.mode.as_str() {
                "100644" | "100755" => {
                    if safe_relative(&entry.path).is_none() {
                        return Err(unsafe_path());
                    }
                    files.push(entry.path.clone());
                }
                "120000" => return Err(CopyError::Link { path: entry.path.clone() }),
                _ => {}
            }
        }
        self.broken = true;
        self.read_tree(tree)?;
        let written = self.checkout(&files)?;
        self.broken = false;
        Ok(Moved { written, removed: 0 })
    }

    /// Later moves: only the paths that differ between `old` and `new`.
    fn move_to(&mut self, old: &str, new: &str) -> Result<Moved, CopyError> {
        let mut removals: Vec<String> = Vec::new();
        let mut writes: Vec<String> = Vec::new();
        for change in self.changes(old, new)? {
            if safe_relative(&change.path).is_none() {
                return Err(unsafe_path());
            }
            if change.status == 'D' {
                removals.push(change.path);
                continue;
            }
            match change.mode.as_str() {
                "120000" => return Err(CopyError::Link { path: change.path }),
                "100644" | "100755" => {
                    if change.status == 'T' {
                        removals.push(change.path.clone());
                    }
                    writes.push(change.path);
                }
                // A submodule (`160000`) has no files here.
                _ => {
                    if change.status == 'T' {
                        removals.push(change.path);
                    }
                }
            }
        }
        self.broken = true;
        let mut removed = 0;
        for path in &removals {
            let abs = self.dir.join(safe_relative(path).ok_or_else(unsafe_path)?);
            if abs.symlink_metadata().is_ok() {
                remove_file_forced(&abs).map_err(|e| CopyError::Io(e.to_string()))?;
                removed += 1;
            }
            self.prune_empty_parents(&abs);
        }
        self.read_tree(new)?;
        let written = self.checkout(&writes)?;
        self.broken = false;
        Ok(Moved { written, removed })
    }

    /// Remove folders above `file` that are now empty, up to (not including) the copy folder.
    fn prune_empty_parents(&self, file: &Path) {
        let mut dir = file.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if d == self.dir || !d.starts_with(&self.dir) || fs::remove_dir(&d).is_err() {
                break;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }

    /// Every row of `ls-tree -r -l` for `tree`. The id is checked first.
    fn listing(&self, tree: &str) -> Result<Vec<Entry>, CopyError> {
        if !is_object_id(tree) {
            return Err(CopyError::Refused("That save point is not valid.".into()));
        }
        let out = self.shadow.run(&["ls-tree", "-r", "-l", "-z", tree], None, None, &[]).map_err(git_error)?;
        let mut rows = Vec::new();
        for record in out.stdout.split(|b| *b == 0) {
            let text = String::from_utf8_lossy(record);
            let Some((meta, path)) = text.split_once('\t') else { continue };
            let mut fields = meta.split_whitespace();
            let (Some(mode), Some(_kind), Some(_sha), size) = (fields.next(), fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            // A gitlink has no size (`-`), which reads as 0.
            let bytes = size.and_then(|s| s.parse().ok()).unwrap_or(0);
            rows.push(Entry { mode: mode.to_string(), bytes, path: path.to_string() });
        }
        Ok(rows)
    }

    /// Paths that differ between two trees, with the new mode. Renames are off, so each path is one row.
    fn changes(&self, old: &str, new: &str) -> Result<Vec<Change>, CopyError> {
        let out = self
            .shadow
            .run(&["diff-tree", "-r", "-z", "--no-renames", old, new], None, None, &[])
            .map_err(git_error)?;
        let parts: Vec<String> = out.stdout.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned()).collect();
        let mut rows = Vec::new();
        let mut i = 0;
        while i < parts.len() {
            let meta = &parts[i];
            i += 1;
            // Raw record: `:<old mode> <new mode> <old id> <new id> <status>`, then the path.
            let Some(fields) = meta.strip_prefix(':') else { continue };
            let fields: Vec<&str> = fields.split_whitespace().collect();
            let (Some(mode), Some(status)) = (fields.get(1), fields.get(4)) else { continue };
            let Some(path) = parts.get(i) else { break };
            i += 1;
            rows.push(Change {
                status: status.chars().next().unwrap_or('?'),
                mode: mode.to_string(),
                path: path.clone(),
            });
        }
        Ok(rows)
    }

    fn read_tree(&self, tree: &str) -> Result<(), CopyError> {
        self.shadow.run(&["read-tree", tree], Some(&self.index), None, &[]).map_err(git_error)?;
        Ok(())
    }

    /// Write the listed paths from the index (already set to the tree) into the copy.
    fn checkout(&self, files: &[String]) -> Result<u32, CopyError> {
        if files.is_empty() {
            return Ok(0);
        }
        let mut stdin = Vec::new();
        for file in files {
            stdin.extend_from_slice(file.as_bytes());
            stdin.push(0);
        }
        self.shadow
            .run(&["checkout-index", "-f", "-z", "--stdin"], Some(&self.index), Some(stdin.as_slice()), &[])
            .map_err(git_error)?;
        Ok(files.len() as u32)
    }

    /// Remove the copy folder and its side files. Guarded: only a folder named like a copy, directly in the root.
    fn remove(&mut self) -> Result<(), CopyError> {
        if self.removed {
            return Ok(());
        }
        self.removed = true;
        LIVE.lock().retain(|p| p != &self.dir);
        if self.dir.parent() != Some(self.root.as_path()) || !is_copy_name(&self.id) {
            return Err(CopyError::Refused("That folder is not a test copy, so it was not removed.".into()));
        }
        let mut result = Ok(());
        if let Err(e) = remove_tree(&self.dir) {
            result = Err(CopyError::Io(e.to_string()));
        }
        for file in [self.index.clone(), self.exclude.clone(), self.setup_log()] {
            if let Err(e) = remove_file_forced(&file) {
                result = Err(CopyError::Io(e.to_string()));
            }
        }
        result
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

/// Remove copies older than `STALE_COPY_AGE_MS` (by modified time) in `root`, at start. Only entries named
/// `<8 hex>`, or `<8 hex>` + `.idx` / `.exclude` / `.setup.log`, are touched, and never a copy this process holds.
/// Returns how many entries were removed. A missing root is 0.
pub fn sweep_stale(root: &Path, now_ms: u64) -> u32 {
    let Ok(entries) = fs::read_dir(root) else { return 0 };
    let live = LIVE.lock().clone();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = stale_stem(&name) else { continue };
        if live.contains(&root.join(stem)) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Some(modified) = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()) else { continue };
        let age = now_ms.saturating_sub(modified.as_millis() as u64);
        if age <= STALE_COPY_AGE_MS {
            continue;
        }
        let path = entry.path();
        let done = if meta.is_dir() {
            stem == name && path.parent() == Some(root) && remove_tree(&path).is_ok()
        } else {
            remove_file_forced(&path).is_ok()
        };
        if done {
            removed += 1;
        }
    }
    removed
}

/// The copy name a sweep entry belongs to: `<8 hex>` with an optional known side-file suffix.
fn stale_stem(name: &str) -> Option<&str> {
    let stem = name.get(..8)?;
    if !is_eight_hex(stem) {
        return None;
    }
    match &name[8..] {
        "" | ".idx" | ".exclude" | ".setup.log" => Some(stem),
        _ => None,
    }
}

/// Make `<root>/<8 hex>` (a name not in use yet) and return the name and the folder.
fn make_folder(root: &Path) -> Result<(String, PathBuf), CopyError> {
    for _ in 0..8 {
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let dir = root.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((id, dir)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(CopyError::Io(e.to_string())),
        }
    }
    Err(CopyError::Io("no free copy name".into()))
}

fn check_size(bytes: u64, limit: u64) -> Result<(), CopyError> {
    if bytes > limit {
        Err(CopyError::TooBig { bytes })
    } else {
        Ok(())
    }
}

fn unsafe_path() -> CopyError {
    CopyError::Refused("A file name in the save point is not safe to write.".into())
}

fn git_error(error: GitError) -> CopyError {
    CopyError::Git(error.0)
}

/// Lowercase, 40 characters, a git object id (what `write-tree` prints). Nothing else reaches a git command.
fn is_object_id(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_eight_hex(text: &str) -> bool {
    text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_copy_name(id: &str) -> bool {
    is_eight_hex(id)
}

/// The real path of `path`, even when it does not exist yet: the nearest existing ancestor is canonicalised
/// and the missing part is added back. Nothing is created.
fn real_path(path: &Path) -> Option<PathBuf> {
    let mut missing: Vec<OsString> = Vec::new();
    let mut here = path.to_path_buf();
    loop {
        if let Ok(real) = here.canonicalize() {
            let mut out = real;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        missing.push(here.file_name()?.to_os_string());
        here = here.parent()?.to_path_buf();
    }
}

/// On Windows, paths compare without case (`C:\Users` = `c:\users`).
fn fold_case(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// Remove a folder tree without following links: a link is removed itself, read-only files are made
/// writable first (Windows), and a missing path is not an error.
fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        return fs::remove_file(path).or_else(|_| fs::remove_dir(path));
    }
    if !meta.is_dir() {
        return remove_file_forced(path);
    }
    clear_read_only(path);
    for entry in fs::read_dir(path)? {
        remove_tree(&entry?.path())?;
    }
    fs::remove_dir(path)
}

fn clear_read_only(dir: &Path) {
    if let Ok(meta) = fs::metadata(dir) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(dir, perms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoints::engine::{file_at, snapshot, Kind, Limits, Snapshot};
    use std::collections::BTreeMap;
    use std::time::SystemTime;

    /// A throw-away folder under the temp dir, removed on drop. Only test folders are ever removed.
    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Dir {
            let dir = std::env::temp_dir().join(format!("pm-wb-copy-{tag}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
        fn p(&self) -> &Path {
            &self.0
        }
        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn read(&self, rel: &str) -> String {
            fs::read_to_string(self.0.join(rel)).unwrap()
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = remove_tree(&self.0);
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
    }

    /// Save the folder as a new save point and return its tree id.
    fn save(project: &Path, at: u64) -> String {
        match snapshot(project, "pane-a", Kind::After, None, at, &Limits::default()).expect("snapshot") {
            Snapshot::Created(c) => c.tree,
            Snapshot::Unchanged(c) => c.expect("a save point").tree,
        }
    }

    /// The project's shadow repository and a copy root next to the project (`<d>/copies`).
    fn setup(d: &Dir) -> (PathBuf, PathBuf, PathBuf) {
        let project = d.p().join("project");
        let root = d.p().join("copies");
        fs::create_dir_all(&project).unwrap();
        (project.clone(), Shadow::new(&project).git_dir.clone(), root)
    }

    fn create(d: &Dir) -> Copy {
        let (project, shadow, root) = setup(d);
        Copy::create(&project, &shadow, &root).expect("copy")
    }

    /// Every file under `root` (except the shadow repository's own folder), as bytes, in order.
    fn listing(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.file_name().is_some_and(|n| n == ".puppet-master") {
                    continue;
                }
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                    out.insert(rel, fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    /// A tree made from raw entries (any mode), without touching the project folder.
    fn craft_tree(d: &Dir, project: &Path, entries: &[(&str, &str, &[u8])]) -> String {
        let shadow = Shadow::new(project);
        let index = d.p().join("craft.idx");
        let mut lines = String::new();
        for (mode, path, bytes) in entries {
            let sha = shadow.run(&["hash-object", "-w", "--stdin"], None, Some(*bytes), &[]).unwrap().text();
            lines.push_str(&format!("{mode} {sha}\t{path}\n"));
        }
        shadow.run(&["update-index", "--index-info"], Some(&index), Some(lines.as_bytes()), &[]).unwrap();
        shadow.run(&["write-tree"], Some(&index), None, &[]).unwrap().text()
    }

    #[test]
    fn contents_equal_the_tree() {
        let d = Dir::new("contents");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        d.write("project/sub/b.txt", "two");
        let a = save(&project, 1_000);
        d.write("project/a.txt", "changed");
        let mut copy = create(&d);
        assert!(copy.folder().starts_with(d.p().join("copies")), "the copy is under the copies root");
        assert_eq!(copy.folder().file_name().unwrap().to_str().unwrap().len(), 8, "a short generated name");
        let moved = copy.set_tree(&a).expect("set tree");
        assert_eq!(moved, Moved { written: 2, removed: 0 });
        for path in ["a.txt", "sub/b.txt"] {
            let want = file_at(&project, &a, path).unwrap().expect("in the tree");
            assert_eq!(fs::read(copy.folder().join(path)).unwrap(), want, "{path} equals the tree");
        }
        assert_eq!(d.read("project/a.txt"), "changed", "the project folder is not touched");
    }

    #[test]
    fn moving_writes_only_the_paths_that_differ() {
        let d = Dir::new("move");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        d.write("project/b.txt", "two");
        d.write("project/c.txt", "three");
        d.write("project/old/f.txt", "gone later");
        let a = save(&project, 1_000);
        d.write("project/a.txt", "ONE");
        fs::remove_file(d.p().join("project/b.txt")).unwrap();
        fs::remove_dir_all(d.p().join("project/old")).unwrap();
        d.write("project/new/e.txt", "fresh");
        let b = save(&project, 2_000);
        let mut copy = create(&d);
        copy.set_tree(&a).unwrap();
        fs::create_dir_all(copy.folder().join("node_modules/x")).unwrap();
        fs::write(copy.folder().join("node_modules/x/index.js"), "installed").unwrap();
        let moved = copy.set_tree(&b).unwrap();
        assert_eq!(moved, Moved { written: 2, removed: 2 }, "a.txt and new/e.txt written; b.txt and old/f.txt removed");
        assert_eq!(fs::read_to_string(copy.folder().join("a.txt")).unwrap(), "ONE");
        assert_eq!(fs::read_to_string(copy.folder().join("c.txt")).unwrap(), "three", "an unchanged file stays");
        assert_eq!(fs::read_to_string(copy.folder().join("new/e.txt")).unwrap(), "fresh");
        assert!(!copy.folder().join("b.txt").exists());
        assert!(!copy.folder().join("old").exists(), "an emptied folder is removed");
        assert_eq!(
            fs::read_to_string(copy.folder().join("node_modules/x/index.js")).unwrap(),
            "installed",
            "an untracked folder is left alone"
        );
        assert_eq!(copy.set_tree(&b).unwrap(), Moved::default(), "the same tree again writes nothing");
    }

    #[test]
    fn unsafe_roots_are_refused_and_the_project_is_not_touched() {
        let d = Dir::new("roots");
        let (project, shadow, _) = setup(&d);
        d.write("project/a.txt", "one");
        save(&project, 1_000);
        let before = listing(&project);
        let cases = [
            ("equal", project.clone()),
            ("inside", project.join("copies")),
            ("deep inside", project.join("x").join("y").join("copies")),
            ("containing", d.p().to_path_buf()),
            ("dot dot", project.join("..").join("elsewhere")),
        ];
        for (name, root) in cases {
            let result = Copy::create(&project, &shadow, &root);
            assert_eq!(result.err(), Some(CopyError::UnsafeRoot), "{name} root is refused");
        }
        assert_eq!(listing(&project), before, "no folder was created in the project");
        assert!(!project.join("x").exists());
    }

    #[test]
    fn a_read_only_file_is_removed_on_finish() {
        let d = Dir::new("finish");
        let (project, _, _) = setup(&d);
        d.write("project/locked.txt", "keep out");
        let a = save(&project, 1_000);
        let mut copy = create(&d);
        copy.set_tree(&a).unwrap();
        let dir = copy.folder().to_path_buf();
        let root = d.p().join("copies");
        let locked = dir.join("locked.txt");
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&locked, perms).unwrap();
        copy.finish().expect("finish");
        assert!(!dir.exists(), "the copy folder is gone");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0, "no side files are left behind");
        assert_eq!(d.read("project/locked.txt"), "keep out");
    }

    #[test]
    fn dropping_a_copy_removes_it() {
        let d = Dir::new("drop");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        let a = save(&project, 1_000);
        let dir;
        {
            let mut copy = create(&d);
            copy.set_tree(&a).unwrap();
            dir = copy.folder().to_path_buf();
            assert!(dir.join("a.txt").is_file());
        }
        assert!(!dir.exists(), "drop removed the copy");
    }

    #[test]
    fn finish_never_removes_a_folder_outside_the_copies_root() {
        let d = Dir::new("guard");
        let (project, _, root) = setup(&d);
        let victim = d.p().join("victim");
        d.write("victim/keep.txt", "mine");
        let copy = Copy {
            id: "abcdef12".into(),
            dir: victim.clone(),
            root: root.clone(),
            project: project.clone(),
            shadow: Shadow::new(&project).with_work_tree(&victim),
            index: root.join("abcdef12.idx"),
            exclude: root.join("abcdef12.exclude"),
            current: None,
            broken: false,
            prepared: false,
            removed: false,
        };
        assert!(matches!(copy.finish(), Err(CopyError::Refused(_))));
        assert_eq!(d.read("victim/keep.txt"), "mine", "a folder outside the root is never removed");
    }

    #[test]
    fn a_link_in_the_save_point_is_refused_and_the_copy_still_works() {
        let d = Dir::new("link");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        let a = save(&project, 1_000);
        let bad = craft_tree(&d, &project, &[("100644", "a.txt", b"one"), ("120000", "link", b"../../secret")]);
        let mut copy = create(&d);
        assert_eq!(copy.set_tree(&bad), Err(CopyError::Link { path: "link".into() }));
        assert!(!copy.folder().join("link").exists(), "nothing was written");
        assert!(copy.set_tree(&a).is_ok(), "a refused tree leaves the copy usable");
        assert!(copy.set_tree("not-a-tree").is_err(), "a bad id is refused");
    }

    #[test]
    fn tree_bytes_adds_up_and_the_limit_is_checked() {
        let d = Dir::new("bytes");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "12345");
        d.write("project/sub/b.txt", "123");
        let a = save(&project, 1_000);
        let copy = create(&d);
        assert_eq!(copy.tree_bytes(&a).unwrap(), 8);
        assert_eq!(check_size(11, 10), Err(CopyError::TooBig { bytes: 11 }));
        assert_eq!(check_size(10, 10), Ok(()));
    }

    #[test]
    fn a_failing_setup_step_is_reported_with_its_tail() {
        let d = Dir::new("setup-fail");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        let a = save(&project, 1_000);
        let mut copy = create(&d);
        copy.set_tree(&a).unwrap();
        let preset = WorktreePreset {
            setup: vec![vec!["git".into(), "no-such-subcommand-wb9".into()], vec!["git".into(), "--version".into()]],
            ..Default::default()
        };
        let log = copy.setup_log();
        match copy.prepare(&project, &preset, &log) {
            Err(CopyError::SetupFailed { step, tail }) => {
                assert_eq!(step, "git no-such-subcommand-wb9");
                assert!(tail.contains("[exit"), "the tail keeps the exit line: {tail}");
            }
            other => panic!("expected SetupFailed, got {other:?}"),
        }
        assert!(matches!(copy.prepare(&project, &preset, &log), Err(CopyError::Refused(_))), "setup runs once");
    }

    #[test]
    fn a_passing_setup_copies_the_listed_files_into_the_copy_only() {
        let d = Dir::new("setup-ok");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        d.write("project/.env", "TOKEN=test");
        let a = save(&project, 1_000);
        let mut copy = create(&d);
        copy.set_tree(&a).unwrap();
        let preset = WorktreePreset {
            setup: vec![vec!["git".into(), "--version".into()]],
            copy: vec![".env".into()],
            ..Default::default()
        };
        let log = copy.setup_log();
        let report = copy.prepare(&project, &preset, &log).expect("prepare").expect("a report");
        assert!(!report.failed());
        assert_eq!(fs::read_to_string(copy.folder().join(".env")).unwrap(), "TOKEN=test");
        assert!(!copy.folder().join("node_modules").exists());
        assert_eq!(d.read("project/.env"), "TOKEN=test");
    }

    #[test]
    fn a_setup_must_name_the_copys_own_project_and_log() {
        let d = Dir::new("setup-guard");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        save(&project, 1_000);
        let other = d.p().join("other");
        fs::create_dir_all(&other).unwrap();
        let mut copy = create(&d);
        let preset = WorktreePreset { setup: vec![vec!["git".into(), "--version".into()]], ..Default::default() };
        let log = copy.setup_log();
        assert!(matches!(copy.prepare(&other, &preset, &log), Err(CopyError::Refused(_))));
        assert!(matches!(copy.prepare(&project, &preset, &d.p().join("elsewhere.log")), Err(CopyError::Refused(_))));
    }

    #[test]
    fn sweep_removes_only_old_eight_hex_names_and_never_a_live_copy() {
        let d = Dir::new("sweep");
        let (project, _, root) = setup(&d);
        d.write("project/a.txt", "one");
        let a = save(&project, 1_000);
        let mut live = create(&d);
        live.set_tree(&a).unwrap();
        let live_dir = live.folder().to_path_buf();
        // Old-looking leftovers from an earlier run, and names the sweep must never touch.
        fs::create_dir_all(root.join("abcdef12")).unwrap();
        fs::write(root.join("abcdef12").join("left.txt"), "x").unwrap();
        fs::write(root.join("abcdef12.idx"), "x").unwrap();
        fs::write(root.join("12345678.setup.log"), "x").unwrap();
        fs::create_dir_all(root.join("project01")).unwrap();
        fs::create_dir_all(root.join("abc")).unwrap();
        fs::write(root.join("zzzzzzzz.idx"), "x").unwrap();
        fs::write(root.join("FEDCBA98"), "x").unwrap();
        assert_eq!(sweep_stale(&root, now_ms()), 0, "young entries are kept");
        let later = now_ms() + STALE_COPY_AGE_MS * 2;
        assert_eq!(sweep_stale(&root, later), 3, "the three old 8-hex leftovers are removed");
        assert!(!root.join("abcdef12").exists());
        assert!(!root.join("abcdef12.idx").exists());
        assert!(!root.join("12345678.setup.log").exists());
        assert!(root.join("project01").is_dir(), "a name that is not 8 hex is kept");
        assert!(root.join("abc").is_dir());
        assert!(root.join("zzzzzzzz.idx").is_file());
        assert!(root.join("FEDCBA98").is_file());
        assert!(live_dir.join("a.txt").is_file(), "a copy this process holds is never swept");
        live.finish().unwrap();
        assert!(!live_dir.exists());
    }

    #[test]
    fn the_project_folder_is_unchanged_by_a_full_run() {
        let d = Dir::new("unchanged");
        let (project, _, _) = setup(&d);
        d.write("project/a.txt", "one");
        d.write("project/.env", "TOKEN=test");
        d.write("project/node_modules/pkg/index.js", "module");
        let a = save(&project, 1_000);
        d.write("project/a.txt", "two");
        let b = save(&project, 2_000);
        let before = listing(&project);
        let mut copy = create(&d);
        copy.set_tree(&a).unwrap();
        let preset = WorktreePreset {
            setup: vec![vec!["git".into(), "--version".into()]],
            copy: vec![".env".into()],
            ..Default::default()
        };
        let log = copy.setup_log();
        copy.prepare(&project, &preset, &log).unwrap();
        copy.set_tree(&b).unwrap();
        copy.finish().unwrap();
        assert_eq!(listing(&project), before, "every file of the project folder is the same");
    }
}
