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
