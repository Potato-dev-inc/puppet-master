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
