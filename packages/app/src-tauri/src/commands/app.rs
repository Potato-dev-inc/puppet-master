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
) -> Result<crate::mobile_pairing::PairingSession, String> {
    let store = crate::mobile_pairing::pairing_store()
        .ok_or_else(|| "pairing store not initialized".to_string())?;
    let session = store.lock().create_pairing_session(bridge_url);
    crate::audit::record(
        crate::audit::Kind::Pairing,
        crate::audit::Who::Desktop,
        "You made a pairing code for a phone",
        serde_json::json!({ "via": "settings" }),
    );
    Ok(session)
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
