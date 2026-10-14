//! Freezing a pane's ring to disk: `<project>/.puppet-master/operations/<op>/flight.cast`, and the
//! hook that does it when a run fails.

use super::{cast, get};
use crate::redact::Redacted;
use std::path::{Path, PathBuf};

pub const CAST_FILE: &str = "flight.cast";
/// Frozen casts kept per project; the oldest are deleted past this.
const KEEP_CASTS: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Frozen {
    pub path: String,
    pub bytes: u64,
    pub events: usize,
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 96 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn operations_dir(project: &str) -> PathBuf {
    Path::new(project).join(".puppet-master").join("operations")
}

pub fn cast_path(project: &str, operation_id: &str) -> Result<PathBuf, String> {
    if !safe_id(operation_id) {
        return Err("operation id has characters that are not allowed in a folder name".into());
    }
    Ok(operations_dir(project).join(operation_id).join(CAST_FILE))
}

/// The pane's recording as a redacted cast, for an export. `note` says why it was taken.
pub fn export(pane_id: &str, note: &str) -> Result<(Redacted<String>, usize), String> {
    let ring = get(pane_id).ok_or_else(|| format!("no recording for pane {pane_id}: it was not started by this app run"))?;
    let snap = ring.snapshot();
    let events = snap.events.len();
    Ok((cast::render(&snap, note, Some(note)), events))
}

/// Write a redacted cast to the operation's folder (atomically).
pub fn write(project: &str, operation_id: &str, body: &Redacted<String>, events: usize) -> Result<Frozen, String> {
    let path = cast_path(project, operation_id)?;
    let dir = path.parent().expect("cast path has a parent").to_path_buf();
    crate::git_exclude::ensure_project_storage_excluded(Path::new(project));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let tmp = path.with_extension("cast.tmp");
    std::fs::write(&tmp, body.as_str()).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    if std::fs::rename(&tmp, &path).is_err() {
        // Windows refuses to rename over a file another process has open: replace in place.
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    }
    prune(project);
    Ok(Frozen { path: path.to_string_lossy().into_owned(), bytes: body.len() as u64, events })
}

/// Freeze a pane's ring into an operation's folder now.
pub fn freeze_now(project: &str, operation_id: &str, pane_id: &str, reason: &str) -> Result<Frozen, String> {
    let (body, events) = export(pane_id, reason)?;
    write(project, operation_id, &body, events)
}

/// Frozen casts of a project, newest first: (operation id, path, size, modified ms).
pub fn list_casts(project: &str) -> Vec<(String, PathBuf, u64, u64)> {
    let Ok(entries) = std::fs::read_dir(operations_dir(project)) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf, u64, u64)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path().join(CAST_FILE);
            let meta = std::fs::metadata(&path).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Some((entry.file_name().to_string_lossy().into_owned(), path, meta.len(), modified))
        })
        .collect();
    out.sort_by_key(|(_, _, _, modified)| std::cmp::Reverse(*modified));
    out
}

fn prune(project: &str) {
    for (_, path, _, _) in list_casts(project).into_iter().skip(KEEP_CASTS) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir(dir); // only if nothing else is in it
        }
    }
}

/// Called after an operation record is saved. A run that just failed gets its pane's ring frozen
/// next to its record. The ring is copied right away (it keeps moving), the redaction and the file
/// write happen on a short-lived thread so the caller, which holds the operation lock, never waits
/// on them. Does nothing for a run that has no pane, no recording, or already has a cast.
pub fn on_operation_saved(op: &crate::operations::OperationSnapshot) {
    use crate::operations::OperationStatus;
    if op.status != OperationStatus::Failed {
        return;
    }
    let Some(error) = op.error.as_ref() else { return };
    let Some(pane_id) = op.pane_id.as_deref() else { return };
    let Ok(path) = cast_path(&op.project_path, &op.operation_id) else { return };
    if path.exists() {
        return;
    }
    let Some(ring) = get(pane_id) else { return };
    let snap = ring.snapshot();
    let (project, operation_id, code) = (op.project_path.clone(), op.operation_id.clone(), error.code.clone());
    let spawned = std::thread::Builder::new().name("flight-freeze".into()).spawn(move || {
        let events = snap.events.len();
        let body = cast::render(&snap, &code, Some(&format!("run failed: {code}")));
        if let Err(error) = write(&project, &operation_id, &body, events) {
            tracing::warn!(%error, operation = %operation_id, "could not save the flight recording");
        }
    });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the flight recording thread");
    }
}
