//! Tauri commands for the Background card (`pane_jobs`): the jobs agents left running in panes, and
//! a way to stop one.

use super::*;
use crate::pane_jobs::{Job, PaneRef};
use std::collections::HashMap;

fn pane_refs(state: &State<'_, AppState>, only: Option<&str>) -> Vec<PaneRef> {
    state
        .registry
        .lock()
        .list()
        .into_iter()
        .filter(|p| only.is_none_or(|id| p.id == id))
        .map(|p| PaneRef { id: p.id, agent_type: p.agent_type, pid: p.pid })
        .collect()
}

/// The long-lived processes under one pane's agent (empty when there are none).
#[tauri::command(async)]
pub fn pane_jobs(state: State<'_, AppState>, pane_id: String) -> Vec<Job> {
    crate::pane_jobs::jobs_for_panes(&pane_refs(&state, Some(&pane_id))).remove(&pane_id).unwrap_or_default()
}

/// The jobs of every pane that has any, keyed by pane id.
#[tauri::command(async)]
pub fn pane_jobs_all(state: State<'_, AppState>) -> HashMap<String, Vec<Job>> {
    crate::pane_jobs::jobs_for_panes(&pane_refs(&state, None))
}

/// Stop one job (and its tree). Refused unless `pid` is below that pane's process right now and started at
/// `started_at_ms` (the start time the card listed it with), so a reused process id is never stopped.
#[tauri::command(async)]
pub fn pane_job_stop(state: State<'_, AppState>, pane_id: String, pid: u32, started_at_ms: u64) -> Result<(), String> {
    let pane = pane_refs(&state, Some(&pane_id)).pop().ok_or("That pane is not open any more.")?;
    crate::pane_jobs::stop_job(&pane, pid, started_at_ms)
}
