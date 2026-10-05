//! Tauri commands for the warm standby pool (Settings > Performance).

use crate::standby::{self, StandbyConfig, StatusView};

#[tauri::command]
pub fn standby_get_status() -> StatusView {
    standby::status()
}

/// Save the pool settings, apply them at once (standbys that are no longer allowed are shut
/// down) and return the new state.
#[tauri::command]
pub fn standby_set_config(config: StandbyConfig) -> Result<StatusView, String> {
    standby::set_config(config)?;
    Ok(standby::status())
}
