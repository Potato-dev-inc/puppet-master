//! Tauri commands for the loop guard and the panic button.

use super::*;

/// The signal currently shown for one pane, or `null`.
#[tauri::command]
pub fn loop_status(pane_id: String) -> Option<crate::loop_guard::LoopSignal> {
    crate::loop_guard::status(&pane_id)
}

/// Every pane that currently shows a stuck warning.
#[tauri::command]
pub fn loop_signals() -> Vec<crate::loop_guard::LoopSignal> {
    crate::loop_guard::all_active()
}

/// "Dismiss": hide the warning for the rest of this episode.
#[tauri::command]
pub fn loop_dismiss(pane_id: String) -> bool {
    crate::loop_guard::dismiss(&pane_id)
}

#[tauri::command]
pub fn loop_settings_get() -> crate::loop_guard::Settings {
    crate::loop_guard::settings()
}

#[tauri::command]
pub fn loop_settings_set(app: AppHandle, settings: crate::loop_guard::Settings) -> crate::loop_guard::Settings {
    let applied = crate::loop_guard::set_settings(settings);
    crate::panic::apply_shortcut(&app, applied.panic_shortcut);
    applied
}

/// Panic button: interrupt every running agent pane and say what was stopped.
#[tauri::command]
pub async fn stop_all_agents(app: AppHandle, state: State<'_, AppState>) -> Result<crate::panic::StopReport, String> {
    Ok(crate::panic::stop_all(&state.registry, &app))
}
