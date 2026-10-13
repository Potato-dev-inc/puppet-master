//! Tauri commands for ambient state and pane titles (docs/features/ambient-and-titles.md).

use super::*;

/// Rename a pane. A blank name clears it and automatic titling resumes. Returns the stored title.
#[tauri::command]
pub async fn pane_set_title(app: AppHandle, state: State<'_, AppState>, pane_id: String, title: String) -> Result<Option<String>, String> {
    if !state.registry.lock().panes.contains_key(&pane_id) {
        return Err(format!("unknown pane {pane_id}"));
    }
    let stored = crate::ambient::store::set_user_title(&pane_id, &title);
    crate::ambient::announce_title_change(&app, &pane_id);
    Ok(stored)
}

#[tauri::command]
pub async fn ambient_get_prefs() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "animated_tray_icon": crate::ambient::prefs::animated() }))
}

#[tauri::command]
pub async fn ambient_set_animated_tray(enabled: bool) -> Result<(), String> {
    crate::ambient::prefs::set_animated(enabled).map_err(|e| e.to_string())?;
    crate::ambient::notify_changed();
    Ok(())
}
