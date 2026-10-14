//! "Undo the test changes": put the files behind blocking findings back to the bytes the snapshot
//! holds. It touches only the files it is given, never implementation files, and it is the only
//! part of the guard that writes to the project. Missions call it when the person asks.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::model::Snapshot;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    /// Files put back to how they were at the start.
    pub restored: Vec<String>,
    /// Test files that did not exist at the start and were removed.
    pub removed: Vec<String>,
    /// Files that could not be put back, with a plain reason.
    pub skipped: Vec<(String, String)>,
}

/// A project-relative path with no `..`, no root and no drive: safe to join onto the project.
fn safe_relative(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if path.is_empty() || p.is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

pub fn undo(root: &Path, snap: &Snapshot, files: &[String]) -> UndoReport {
    let mut report = UndoReport::default();
    let root_real = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    for file in files {
        let skip = |report: &mut UndoReport, why: &str| report.skipped.push((file.clone(), why.to_string()));
        let Some(rel) = safe_relative(file) else {
            skip(&mut report, "not a path inside the project");
            continue;
        };
        let target = root.join(&rel);
        // Never write through a link that leads out of the project.
        let parent_real = target.parent().and_then(|p| fs::canonicalize(p).ok());
        if target.exists() && parent_real.as_ref().is_some_and(|p| !p.starts_with(&root_real)) {
            skip(&mut report, "outside the project");
            continue;
        }
        match snap.files.get(file) {
            Some(before) => match &before.content {
                Some(text) => {
                    if let Some(dir) = target.parent() {
                        if fs::create_dir_all(dir).is_err() {
                            skip(&mut report, "couldn't create its folder");
                            continue;
                        }
                    }
                    match fs::write(&target, text.as_bytes()) {
                        Ok(()) => report.restored.push(file.clone()),
                        Err(_) => skip(&mut report, "couldn't write it"),
                    }
                }
                None => skip(&mut report, "its original text was too big to keep"),
            },
            None => match fs::remove_file(&target) {
                Ok(()) => report.removed.push(file.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => skip(&mut report, "couldn't remove it"),
            },
        }
    }
    report
}
