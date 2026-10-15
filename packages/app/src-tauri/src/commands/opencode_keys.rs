//! Tauri commands for OpenCode key status, automation settings, profiles and rotation.

use super::*;

#[tauri::command]
pub fn get_opencode_key_status() -> Result<crate::opencode::keys::OpenCodeKeyStatus, String> {
    crate::opencode::keys::status()
}

#[tauri::command]
pub fn set_opencode_automation_settings(
    mode: String,
    restart_pane_on_rotate: bool,
) -> Result<crate::opencode::keys::OpenCodeKeyStatus, String> {
    let parsed = crate::opencode::keys::KeyAutomationMode::parse(&mode)
        .ok_or_else(|| "mode must be auto_if_backup or notify_only".to_string())?;
    crate::opencode::keys::set_automation_settings(Some(parsed), Some(restart_pane_on_rotate))
}

#[tauri::command]
pub fn set_opencode_key_settings(
    key_automation_mode: Option<String>,
    restart_pane_on_rotate: Option<bool>,
) -> Result<crate::opencode::keys::OpenCodeKeyStatus, String> {
    crate::opencode::keys::patch_settings(&crate::opencode::keys::OpenCodeKeySettingsPatch {
        key_automation_mode,
        restart_pane_on_rotate,
    })
}

#[tauri::command]
pub fn set_opencode_key_profile(
    profile_id: String,
    api_key: String,
    label: Option<String>,
) -> Result<(), String> {
    crate::opencode::keys::set_profile_api_key(&profile_id, &api_key, label.as_deref())
}

#[tauri::command]
pub fn capture_opencode_key_profile(
    profile_id: String,
    label: Option<String>,
) -> Result<(), String> {
    crate::opencode::keys::capture_profile_from_disk(&profile_id, label.as_deref())
}

#[derive(Debug, Clone, Serialize)]
pub struct RotateOpenCodeKeyResult {
    pub ok: bool,
    pub active_profile: String,
    pub profiles: Vec<crate::opencode::keys::OpenCodeKeyProfileInfo>,
    pub restarted_panes: Vec<String>,
}

#[tauri::command]
pub fn rotate_opencode_key(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Option<String>,
    pane_id: Option<String>,
) -> Result<RotateOpenCodeKeyResult, String> {
    let target = crate::opencode::keys::RotateTarget::parse(profile.as_deref().unwrap_or("next"))
        .ok_or_else(|| "profile must be next, a, or b".to_string())?;
    let status = crate::opencode::keys::rotate(target)?;
    let restarted = if let Some(pane_id) = pane_id.filter(|id| !id.is_empty()) {
        vec![crate::opencode::native::restart_native_pane(
            state.registry.clone(),
            &app,
            &pane_id,
        )?]
    } else {
        crate::opencode::native::restart_all_native_panes(state.registry.clone(), &app)?
    };
    Ok(RotateOpenCodeKeyResult {
        ok: true,
        active_profile: status.active_profile,
        profiles: status.profiles,
        restarted_panes: restarted,
    })
}
