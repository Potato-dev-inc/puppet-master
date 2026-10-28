//! Write one save point's files into another folder (the copy made by "Branch from here").
//!
//! The source folder is only read. Its shadow repository gets no new refs, its per-pane index is
//! not touched and no safety snapshot is taken. The target must be a Puppet Master copy folder of
//! the source's origin project (`<origin>/.puppet-master/worktrees/<name>`, or the project's own
//! sub-folder inside such a copy); files are written and removed only inside it.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::engine::{self, CheckpointError, Limits};
use super::git::{git_available, Shadow};

/// What `export_into` did to the copy.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExportReport {
    pub written: u32,
    pub removed: u32,
    /// Paths that were neither written nor removed (unsafe paths, locked files). Never hidden.
    pub failed: Vec<String>,
    /// The git tree of the save point that was written.
    pub tree: String,
}

/// Make the files under `dest` match checkpoint `id` of `pane` in the shadow repository of `project`.
///
/// `project` is the folder whose save points are read: the original folder, or a copy the pane works
/// in (a branch of a branch). Copies are always made under the ORIGIN project's worktrees folder, so
/// `dest` must be a copy folder of that origin once links are resolved:
/// `<origin>/.puppet-master/worktrees/<name>`, or a sub-folder of it when the origin is a sub-folder
/// of a git repo (the copy's own `cwd`; the copy folder then holds a `.git`). Refused before a file is
/// touched: the source folder, a folder above it, the copy the source lives in, the origin, a folder
/// anywhere else and a link to any of them. Paths that would go through a link inside the copy are
/// reported in `failed` and left alone.
pub fn export_into(project: &Path, pane: &str, id: &str, dest: &Path, limits: &Limits) -> Result<ExportReport, CheckpointError> {
    if !git_available() {
        return Err(CheckpointError::GitMissing);
    }
    let canon_dest = copy_folder(project, dest)?;
    let shadow = Shadow::new(project);
    let gate = engine::lock_for(&shadow);
    let _held = gate.lock();
    // No save points yet gives the same answer as an unknown id. Nothing is created in `project`.
    let items = if shadow.exists() { engine::list_in(&shadow, pane)? } else { Vec::new() };
    let at = engine::find(&items, id).map_err(|_| CheckpointError::Other("That save point is gone.".into()))?;
    let target = items[at].tree.clone();

    let dest_shadow = Shadow { work_tree: dest.to_path_buf(), git_dir: shadow.git_dir.clone() };
    let index = dest_shadow.git_dir.join(format!(
        "pm-export-{}-{}.index",
        engine::pane_key(pane),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    let _index = IndexFile(index.clone());
    let big = engine::scan(&dest_shadow, &index, limits)?;
    engine::stage_all(&dest_shadow, &index, &big)?;
    // The index now mirrors the copy, so this tree is "what is there now".
    let now = dest_shadow.run(&["write-tree"], Some(&index), None, &[])?.text();
    let rows = engine::name_status(&dest_shadow, &now, &target)?;

    let mut report = ExportReport { tree: target.clone(), ..Default::default() };
    let mut to_write: Vec<String> = Vec::new();
    for (status, path) in &rows {
        let Some(rel) = engine::safe_relative(path).filter(|rel| inside_copy(dest, &canon_dest, rel)) else {
            report.failed.push(path.clone());
            continue;
        };
        let abs = dest.join(&rel);
        match status {
            'D' => match engine::remove_file_forced(&abs) {
                Ok(()) => {
                    report.removed += 1;
                    remove_empty_folders(dest, &abs);
                }
                Err(_) => report.failed.push(path.clone()),
            },
            'T' => {
                let _ = engine::remove_file_forced(&abs);
                to_write.push(path.clone());
            }
            _ => to_write.push(path.clone()),
        }
    }
    dest_shadow.run(&["read-tree", target.as_str()], Some(&index), None, &[])?;
    if !to_write.is_empty() {
        write_files(&dest_shadow, &index, &to_write, &mut report);
    }
    Ok(report)
}

/// Split `canon_project` (canonical) into its origin project and the copy folder it lives in.
/// A copy folder (or a folder inside one) is `<origin>/.puppet-master/worktrees/<name>[/...]`; any
/// other folder is its own origin and lives in no copy.
fn origin_of(canon_project: &Path) -> (PathBuf, Option<PathBuf>) {
    let parts: Vec<_> = canon_project.components().collect();
    for i in 0..parts.len().saturating_sub(2) {
        let is = |at: usize, name: &str| parts[at].as_os_str().to_string_lossy().eq_ignore_ascii_case(name);
        if is(i, ".puppet-master") && is(i + 1, "worktrees") {
            let origin: PathBuf = parts[..i].iter().collect();
            let copy: PathBuf = parts[..=i + 2].iter().collect();
            return (origin, Some(copy));
        }
    }
    (canon_project.to_path_buf(), None)
}

/// Resolve `dest` and check it is a copy folder of the origin of `project`: a direct child of
/// `<origin>/.puppet-master/worktrees`, or a folder inside such a child that holds a `.git` (a copy's
/// prefixed `cwd`), with links resolved. Never the source, a folder above it, or the copy the source
/// works in. Anything else is refused.
fn copy_folder(project: &Path, dest: &Path) -> Result<PathBuf, CheckpointError> {
    let refused = || CheckpointError::Other("not a separate copy".into());
    let canon_project = std::fs::canonicalize(project).map_err(|_| refused())?;
    let (origin, source_copy) = origin_of(&canon_project);
    let canon_origin = std::fs::canonicalize(&origin).map_err(|_| refused())?;
    let worktrees =
        std::fs::canonicalize(canon_origin.join(".puppet-master").join("worktrees")).map_err(|_| refused())?;
    let canon_dest = std::fs::canonicalize(dest).map_err(|_| refused())?;
    if !worktrees.starts_with(&canon_origin) {
        return Err(refused());
    }
    // The first folder below the worktrees folder is the copy; `dest` is that folder or one inside it.
    let below = canon_dest.strip_prefix(&worktrees).map_err(|_| refused())?;
    let mut parts = below.components();
    let Some(first) = parts.next() else { return Err(refused()) };
    let copy_root = worktrees.join(first);
    if parts.next().is_some() && !copy_root.join(".git").exists() {
        return Err(refused());
    }
    // Never the source itself, a folder above it, or the copy the source works in.
    if canon_project.starts_with(&canon_dest) || source_copy.as_ref().is_some_and(|copy| *copy == copy_root) {
        return Err(refused());
    }
    Ok(canon_dest)
}

/// True when `rel`, a path inside the copy, may be written or removed: no folder or file on its way
/// is a link or junction, and everything on the way resolves inside the copy.
fn inside_copy(dest: &Path, canon_dest: &Path, rel: &Path) -> bool {
    let mut cur = dest.to_path_buf();
    for part in rel.components() {
        cur.push(part);
        match std::fs::symlink_metadata(&cur) {
            Ok(meta) => {
                let resolved_inside = std::fs::canonicalize(&cur).is_ok_and(|p| p.starts_with(canon_dest));
                if meta.file_type().is_symlink() || !resolved_inside {
                    return false;
                }
            }
            // A part that does not exist yet has nothing behind it to escape through.
            Err(e) => return e.kind() == std::io::ErrorKind::NotFound,
        }
    }
    true
}

/// Remove the folders a deleted file leaves empty, stopping at the copy folder itself.
fn remove_empty_folders(dest: &Path, file: &Path) {
    let mut dir = file.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d == dest || !d.starts_with(dest) || std::fs::remove_dir(&d).is_err() {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}

/// Check out `paths` from the index into the copy. One bad file does not block the rest.
fn write_files(dest_shadow: &Shadow, index: &Path, paths: &[String], report: &mut ExportReport) {
    let mut stdin = Vec::new();
    for p in paths {
        stdin.extend_from_slice(p.as_bytes());
        stdin.push(0);
    }
    if dest_shadow.run(&["checkout-index", "-f", "-z", "--stdin"], Some(index), Some(&stdin), &[]).is_ok() {
        report.written = paths.len() as u32;
    } else {
        for p in paths {
            if dest_shadow.run(&["checkout-index", "-f", "--", p.as_str()], Some(index), None, &[]).is_ok() {
                report.written += 1;
            } else {
                report.failed.push(p.clone());
            }
        }
    }
}

/// The private index of one export. Removed however the export ends.
struct IndexFile(PathBuf);

impl Drop for IndexFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
