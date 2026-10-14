//! Automatic checkpoints and one-click undo, for every agent, in any folder.
//!
//! A *shadow repository* (`<project>/.puppet-master/checkpoints.git`, work tree = the project)
//! snapshots the files at each turn boundary. The user's own `.git`, if any, is never touched.
//! See `docs/features/checkpoints.md`.

mod engine;
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
