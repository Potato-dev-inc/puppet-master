//! Tauri commands for app-level concerns: settings sync, UI events, mobile pairing, worker launch, exit.

use super::*;

#[tauri::command]
pub async fn sync_public_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(&settings_json)
        .map_err(|err| format!("invalid settings json: {err}"))?;
    let merged =
        settings_store::merge_public_settings(&settings_store::default_public_settings(), &parsed);
    *state.public_settings.lock() = merged;
    let _ = app;
    Ok(())
}

/// Push a JSON chat event to all SSE clients (mobile PWA and any desktop browser).
/// The frontend calls this after each LLM chunk so the mobile PWA receives it.
#[tauri::command]
pub async fn push_chat_event(event_json: String) -> Result<(), String> {
    let payload = format!("event: chat\ndata: {event_json}\n\n");
    crate::bridge::push_sse(payload);
    Ok(())
}

/// Push public settings to mobile SSE clients after desktop-side changes.
#[tauri::command]
pub async fn push_settings_event(settings_json: String) -> Result<(), String> {
    let payload = format!("event: settings\ndata: {settings_json}\n\n");
    crate::bridge::push_sse(payload);
    Ok(())
}

#[tauri::command]
pub async fn create_mobile_pairing_session(
    bridge_url: String,
    caps: Option<Vec<String>>,
) -> Result<crate::mobile_pairing::PairingSession, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    // No choice sent (older UI): every cap, which is today's behaviour.
    let caps = match caps {
        Some(names) => crate::mobile_pairing::Caps::parse_names(&names)?,
        None => crate::mobile_pairing::Caps::all(),
    };
    // One line in the log, saying so when it replaced a code that was still good.
    let session = crate::mobile_pairing::mint::mint(&store, bridge_url, caps, crate::mobile_pairing::mint::Maker::Settings);
    Ok(session)
}

/// Settings › Phone › Devices: change what one paired device may do.
#[tauri::command]
pub async fn set_paired_mobile_device_caps(
    device_id: String,
    caps: Vec<String>,
) -> Result<bool, String> {
    let caps = crate::mobile_pairing::Caps::parse_names(&caps)?;
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let changed = store.lock().set_device_caps(&device_id, caps)?;
    Ok(changed)
}

/// This launch's bridge local secret for the app's own webview (IPC only, never over HTTP).
#[tauri::command]
pub fn bridge_local_secret() -> Option<String> {
    crate::mobile_pairing::local_secret::current().map(str::to_string)
}

/// How many local callers reached the bridge without the local secret (log-only mode).
#[tauri::command]
pub fn bridge_local_secret_status() -> crate::mobile_pairing::local_secret::Status {
    crate::mobile_pairing::local_secret::status()
}

#[tauri::command]
pub async fn list_paired_mobile_devices(
) -> Result<Vec<crate::mobile_pairing::PairedDeviceInfo>, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let devices = store.lock().list_devices();
    Ok(devices)
}

#[tauri::command]
pub async fn revoke_paired_mobile_device(device_id: String) -> Result<bool, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let revoked = store.lock().revoke_device(&device_id)?;
    if revoked {
        crate::audit::record(
            crate::audit::Kind::Pairing,
            crate::audit::Who::Desktop,
            "You removed a paired phone",
            serde_json::json!({ "device_id": device_id }),
        );
    }
    Ok(revoked)
}

#[tauri::command]
pub fn get_mobile_tunnel_info() -> crate::pwa_server::DevInfoPayload {
    crate::mobile_tunnel::get_mobile_tunnel_info()
}

#[tauri::command]
pub fn get_worker_launch() -> Option<crate::worker_launch::WorkerLaunch> {
    crate::worker_launch::worker_launch()
}

#[tauri::command]
pub fn exit_app(app: AppHandle) {
    app.exit(0);
}

/// A non-default profile's own bridge (`PUPPET_MASTER_PROFILE`), so its windows never talk to
/// another Puppet Master on this computer. `None` for the default profile, which keeps scanning.
#[tauri::command]
pub fn profile_bridge_url() -> Option<String> {
    crate::app_paths::profile()?;
    let text = std::fs::read_to_string(crate::app_paths::bridge_port_file()).ok()?;
    crate::bridge::port_file::parse_port(&text).map(|port| format!("http://127.0.0.1:{port}"))
}
