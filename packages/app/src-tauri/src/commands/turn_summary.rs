//! Tauri commands for turn summaries and the away digest (see `docs/features/turn-summary-and-away-digest.md`).

use crate::turn_summary::{self, Digest, TurnSummary};

/// The last turn summaries of a pane, oldest first (at most 100).
#[tauri::command]
pub async fn turn_summaries(pane_id: String) -> Result<Vec<TurnSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || turn_summary::tracker().summaries(&pane_id))
        .await
        .map_err(|e| e.to_string())
}

/// The cockpit window gained or lost focus (`true` also when it is first opened).
#[tauri::command]
pub async fn presence_cockpit_focus(focused: bool) -> Result<(), String> {
    turn_summary::cockpit_focus(focused);
    Ok(())
}

/// The digest waiting for the cockpit after the user came back, if any.
#[tauri::command]
pub async fn away_digest_pending() -> Result<Option<Digest>, String> {
    Ok(turn_summary::pending_digest())
}

#[tauri::command]
pub async fn away_digest_dismiss() -> Result<(), String> {
    turn_summary::dismiss_digest();
    Ok(())
}

/// Who last changed each of `paths` (folder-relative) in the project folder `project`, from the
/// turn summaries saved there. Read-only; a file no summary lists has no entry.
#[tauri::command]
pub async fn changes_blame(
    project: String,
    paths: Vec<String>,
) -> Result<std::collections::HashMap<String, turn_summary::blame::FileBlame>, String> {
    if !std::path::Path::new(&project).is_absolute() {
        return Err(format!("INVALID_ARGUMENT: project must be an absolute folder, got {project}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let summaries = turn_summary::store::read_project(std::path::Path::new(&project));
        let title_of = |pane: &str| crate::ambient::store::get(pane);
        turn_summary::blame::blame(&summaries, &paths, &title_of)
    })
    .await
    .map_err(|e| e.to_string())
}
