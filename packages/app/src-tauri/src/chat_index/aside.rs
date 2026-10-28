//! Old copies of a damaged index. A file that cannot be opened is moved aside as `chat-index.bad-<ms>`, together with
//! its `-wal` and `-shm` files (a log left behind would be replayed into the new, empty file). The copies hold old
//! chat text, so "Clear search index" removes them, and the status counts them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `path` with `suffix` added to its file name (`chat-index.sqlite` + `-wal`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The log and shared-memory files SQLite keeps next to `path`.
fn companions(path: &Path) -> [PathBuf; 2] {
    [with_suffix(path, "-wal"), with_suffix(path, "-shm")]
}

/// Moves the index file at `path` aside as `<name>.bad-<stamp>`, with its log and shared-memory files under the same
/// name plus their endings. The main file moves first; if it cannot move, nothing else does.
pub fn move_aside(path: &Path, stamp: i64) -> std::io::Result<PathBuf> {
    let aside = path.with_extension(format!("bad-{stamp}"));
    std::fs::rename(path, &aside)?;
    for (from, to) in companions(path).into_iter().zip(companions(&aside)) {
        if from.exists() && std::fs::rename(&from, &to).is_err() {
            // A log that cannot move must not stay: it would be attached to the new file. Drop it instead.
            let _ = std::fs::remove_file(&from);
        }
    }
    Ok(aside)
}

/// Every file of the old copies next to `path`: `<stem>.bad-*`, and their `-wal` / `-shm` files.
fn old_files(path: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else { return Vec::new() };
    let prefix = format!("{stem}.bad-");
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(|name| name.starts_with(&prefix)))
        .map(|e| e.path())
        .collect()
}

/// How many old copies are there (the log and shared-memory files of a copy do not count again).
pub fn count(path: &Path) -> usize {
    old_files(path)
        .iter()
        .filter(|file| file.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.ends_with("-wal") && !n.ends_with("-shm")))
        .count()
}

/// Removes the old copies and their log files; returns how many files went. A file that cannot be removed is left.
pub fn remove_all(path: &Path) -> usize {
    old_files(path).into_iter().filter(|file| std::fs::remove_file(file).is_ok()).count()
}
