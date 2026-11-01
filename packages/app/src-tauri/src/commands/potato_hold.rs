//! Tauri commands of "Stop everything" for Potato Bot (UX-1): the hold state, the same pause the panic button runs,
//! and "Start bots again". Desktop only. Errors are one string, `CODE: plain sentence`.

use std::time::Duration;

use crate::potato::hold::{self, Hold};
use crate::potato::live;

/// What is held right now (the panic sheet reads this).
#[tauri::command]
pub fn potato_hold_state() -> Hold {
    hold::load(&live::root())
}

/// Pauses every bot, holds every routine and puts Fish's computer to sleep. The panic button does the same.
#[tauri::command]
pub async fn potato_pause_all() -> Result<Hold, String> {
    tauri::async_runtime::spawn_blocking(|| live::pause_all(Duration::from_millis(300))).await.map_err(|e| format!("INTERNAL: {e}"))
}

/// "Start bots again": clears the hold. Bots and routines run again; agents stay stopped.
#[tauri::command]
pub fn potato_resume_all() -> Result<Hold, String> {
    hold::resume(&live::root()).map_err(|e| format!("POTATO_STORE_INVALID: could not save the hold: {e}"))
}
