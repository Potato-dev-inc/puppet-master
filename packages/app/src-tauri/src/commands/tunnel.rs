//! Tauri commands for the bring-your-own-tunnel setting (see `tunnel_config.rs`).

use super::*;
use crate::mobile_tunnel::{self, ConnectionTest, TunnelStatus};
use crate::tunnel_config::TunnelConfigInput;

/// Mode, public URL, running/error state and the redacted saved setting (never the token).
#[tauri::command]
pub fn get_tunnel_status() -> TunnelStatus {
    mobile_tunnel::tunnel_status()
}

/// Save the tunnel setting and restart the tunnel so it takes effect.
#[tauri::command]
pub async fn set_tunnel_config(app: AppHandle, input: TunnelConfigInput) -> Result<TunnelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || mobile_tunnel::apply_tunnel_config(&app, input))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn restart_tunnel(app: AppHandle) -> Result<TunnelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || mobile_tunnel::restart_mobile_tunnel(&app))
        .await
        .map_err(|err| err.to_string())?
}

/// Fetch `<public>/bridge/health`. `url: None` tests the tunnel's current public URL.
#[tauri::command]
pub async fn test_tunnel_connection(url: Option<String>) -> Result<ConnectionTest, String> {
    tauri::async_runtime::spawn_blocking(move || mobile_tunnel::test_connection(url))
        .await
        .map_err(|err| err.to_string())?
}
