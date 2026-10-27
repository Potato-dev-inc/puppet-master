//! Tauri commands for the "Project memory" cockpit view (`crate::one_brain`).

use super::*;
use crate::one_brain::{self, MemoryView};

fn project_dir(state: &State<'_, AppState>, project: Option<String>) -> Result<PathBuf, String> {
    let chosen = project
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| state.registry.lock().project_path.clone());
    let path = PathBuf::from(&chosen);
    if chosen.trim().is_empty() || !path.is_dir() {
        return Err("No project folder is open. Start an agent in a project first.".into());
    }
    Ok(path)
}

#[tauri::command]
pub async fn memory_get(state: State<'_, AppState>, project: Option<String>) -> Result<MemoryView, String> {
    Ok(one_brain::view(&project_dir(&state, project)?, None))
}

/// Save the memory text, then sync every agent file.
#[tauri::command]
pub async fn memory_save(state: State<'_, AppState>, project: Option<String>, text: String) -> Result<MemoryView, String> {
    let dir = project_dir(&state, project)?;
    one_brain::write_memory(&dir, &text).map_err(|e| format!("Could not save the memory: {e}"))?;
    let report = one_brain::sync(&dir, &one_brain::targets(), one_brain::global_enabled());
    Ok(one_brain::view(&dir, Some(report)))
}

#[tauri::command]
pub async fn memory_sync(state: State<'_, AppState>, project: Option<String>) -> Result<MemoryView, String> {
    let dir = project_dir(&state, project)?;
    let report = one_brain::sync(&dir, &one_brain::targets(), one_brain::global_enabled());
    Ok(one_brain::view(&dir, Some(report)))
}

/// Per-project opt-out. Turning it off removes the blocks right away.
#[tauri::command]
pub async fn memory_set_project_enabled(
    state: State<'_, AppState>,
    project: Option<String>,
    enabled: bool,
) -> Result<MemoryView, String> {
    let dir = project_dir(&state, project)?;
    one_brain::set_project_enabled(&dir, enabled).map_err(|e| e.to_string())?;
    let report = one_brain::sync(&dir, &one_brain::targets(), one_brain::global_enabled());
    Ok(one_brain::view(&dir, Some(report)))
}

#[tauri::command]
pub async fn memory_global_enabled() -> Result<bool, String> {
    Ok(one_brain::global_enabled())
}

/// The Settings switch. Turning it off removes the blocks from the current project now and from
/// other projects the next time an agent starts there.
#[tauri::command]
pub async fn memory_set_global_enabled(state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    one_brain::set_global_enabled(enabled).map_err(|e| e.to_string())?;
    if let Ok(dir) = project_dir(&state, None) {
        one_brain::sync(&dir, &one_brain::targets(), enabled);
    }
    Ok(enabled)
}

/// Answer of `one_brain_remember`: the line as stored, whether it was new, and the memory after it.
#[derive(serde::Serialize)]
pub struct RememberedView {
    pub text: String,
    pub added: bool,
    pub view: MemoryView,
}

/// The folder whose memory a pane reads: its project, not a separate copy (`origin_of_cwd`, as the sync does).
fn pane_project_dir(state: &State<'_, AppState>, pane_id: &str) -> Result<PathBuf, String> {
    let cwd = state
        .registry
        .lock()
        .list()
        .into_iter()
        .find(|p| p.id == pane_id)
        .map(|p| p.cwd)
        .ok_or_else(|| "PANE_GONE: That pane is closed.".to_string())?;
    let folder = crate::worktrees::origin_of_cwd(&cwd).unwrap_or(cwd);
    let path = PathBuf::from(&folder);
    if folder.trim().is_empty() || !path.is_dir() {
        return Err("FOLDER_MISSING: The folder of this pane is gone.".into());
    }
    Ok(path)
}

/// "Remember for all agents": adds the selected text as one line of the pane's project memory, then syncs
/// every agent file, as a save does. Refused with a coded error when the memory is off or the text is empty
/// or holds a secret.
#[tauri::command]
pub async fn one_brain_remember(state: State<'_, AppState>, pane_id: String, text: String) -> Result<RememberedView, String> {
    let dir = pane_project_dir(&state, &pane_id)?;
    let global = one_brain::global_enabled();
    let ctx = crate::redact::known::context();
    let done = one_brain::remember_with(&dir, &text, global, &ctx)?;
    let report = one_brain::sync(&dir, &one_brain::targets(), global);
    Ok(RememberedView { text: done.text, added: done.added, view: one_brain::view(&dir, Some(report)) })
}

/// Undo of `one_brain_remember`: removes that one line (and nothing else) and syncs.
#[tauri::command]
pub async fn one_brain_forget(state: State<'_, AppState>, pane_id: String, text: String) -> Result<MemoryView, String> {
    let dir = pane_project_dir(&state, &pane_id)?;
    one_brain::forget_with(&dir, &text)?;
    let global = one_brain::global_enabled();
    let report = one_brain::sync(&dir, &one_brain::targets(), global);
    Ok(one_brain::view(&dir, Some(report)))
}

/// The Turn on button of the Chat toast: the same per-project switch the Memory view has, for the pane's project.
#[tauri::command]
pub async fn one_brain_set_project_enabled(state: State<'_, AppState>, pane_id: String, enabled: bool) -> Result<MemoryView, String> {
    let dir = pane_project_dir(&state, &pane_id)?;
    one_brain::set_project_enabled(&dir, enabled).map_err(|e| e.to_string())?;
    let report = one_brain::sync(&dir, &one_brain::targets(), one_brain::global_enabled());
    Ok(one_brain::view(&dir, Some(report)))
}
