//! Automatic checkpoints and one-click undo, for every agent, in any folder.
//!
//! A *shadow repository* (`<project>/.puppet-master/checkpoints.git`, work tree = the project)
//! snapshots the files at each turn boundary. The user's own `.git`, if any, is never touched.
//! See `docs/features/checkpoints.md`.

pub mod bisect;
pub mod engine;
pub mod http;
mod git;
mod runtime;
#[cfg(test)]
mod tests;

pub use engine::{Checkpoint, CheckpointError, FileChange, RestoreReport};
/// For tests in other modules that need a real checkpoint store inside a folder.
#[cfg(test)]
pub(crate) use engine::{snapshot as test_snapshot, Kind as TestKind, Limits as TestLimits};
pub use runtime::{before_answer, before_dispatch, before_stop, live, note_dispatched, on_pane_status, PaneFacts, Runtime};

/// What the History tab and `GET /panes/{id}/checkpoints` show.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    /// `ok`, `off` (turned off in Settings), `no_git` (git is not installed) or `too_big` (the
    /// folder has too much new data to snapshot; Undo is off for it).
    pub state: &'static str,
    pub message: String,
    /// Newest first.
    pub items: Vec<Checkpoint>,
}

pub fn list_view(pane_id: &str) -> Result<ListView, String> {
    list_view_with(live(), pane_id)
}

pub(crate) fn list_view_with(rt: &Runtime, pane_id: &str) -> Result<ListView, String> {
    match rt.list(pane_id) {
        Ok(mut items) => {
            items.reverse();
            if rt.folder_too_big(pane_id).is_some() {
                return Ok(ListView { state: "too_big", message: engine::TOO_BIG_MESSAGE.to_string(), items });
            }
            Ok(ListView { state: "ok", message: String::new(), items })
        }
        Err(CheckpointError::Disabled) => {
            Ok(ListView { state: "off", message: CheckpointError::Disabled.to_string(), items: vec![] })
        }
        Err(CheckpointError::GitMissing) => {
            Ok(ListView { state: "no_git", message: CheckpointError::GitMissing.to_string(), items: vec![] })
        }
        Err(e) => Err(e.to_string()),
    }
}

/// A mission's own checkpoint series (DESIGN-MISSIONS 3.4): taken after each turn's checks under
/// the key `mission-<id>` in the copy's shadow repository, independent of panes (so it survives
/// hand-offs and closed panes, and no per-pane debounce applies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionPoint {
    /// The checkpoint id to restore later.
    pub id: String,
    /// The git tree of the files at that point: equal trees mean no file changed.
    pub tree: String,
}

/// Snapshot `copy` for a mission's turn `round` (0 = the start). When nothing changed since the
/// newest point, that point is returned again.
pub fn mission_snapshot(copy: &std::path::Path, key: &str, round: u32, note: &str) -> Result<MissionPoint, CheckpointError> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let made = engine::snapshot_numbered(copy, key, engine::Kind::After, Some(note), now, &engine::Limits::default(), Some(round.max(1)))?;
    let point = match made {
        engine::Snapshot::Created(c) => c,
        engine::Snapshot::Unchanged(Some(c)) => c,
        engine::Snapshot::Unchanged(None) => return Err(CheckpointError::NotFound),
    };
    Ok(MissionPoint { id: point.id, tree: point.tree })
}

/// Put the copy's files back to a mission point. The caller has made sure no pane is working in
/// the copy. A safety point of the current files is taken first, so a rollback can be undone.
pub fn mission_restore(copy: &std::path::Path, key: &str, id: &str) -> Result<RestoreReport, CheckpointError> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    engine::restore(copy, key, id, false, now, &engine::Limits::default())
}

/// Paths that changed in mission point `id` compared with the point before it.
pub fn mission_changes(copy: &std::path::Path, key: &str, id: &str) -> Result<Vec<String>, CheckpointError> {
    Ok(engine::diff(copy, key, id)?.into_iter().map(|c| c.path).collect())
}

/// Git is installed (checkpoints and missions need it).
pub fn git_available() -> bool {
    git::git_available()
}
