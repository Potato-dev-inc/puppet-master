//! The answer to a pane window's "Close and stop it?" sheet.

use tauri::AppHandle;

/// "Stop and close": kills the pane and closes its window. "Keep open" needs no command.
#[tauri::command]
pub fn confirm_pane_window_close(app: AppHandle, pane_id: String) {
    crate::pane_windows::close_confirmed(&app, &pane_id);
}
