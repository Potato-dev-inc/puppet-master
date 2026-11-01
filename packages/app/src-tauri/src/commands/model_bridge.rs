//! Model Bridge commands (Labs `model_bridge_enabled`): list services, save or remove a key, add a service of your
//! own, preview and apply "Use in Codex", and Undo. Desktop only: these are Tauri commands, not bridge routes and
//! not MCP tools, so an agent, the companion or a paired phone cannot read a key or edit Codex's settings.
//!
//! Only an app window may call them (a web page the app shows has another label). A key goes in and never comes
//! back: nothing here returns it.

use crate::model_bridge::{self as mb, DoneView, ListView, PreviewView};

/// The windows that may use Model Bridge: the Settings window hosts it.
const WINDOWS: [&str; 3] = ["cockpit", "main", "settings"];

fn guard(webview: &tauri::Webview) -> Result<(), String> {
    if WINDOWS.contains(&webview.label()) {
        Ok(())
    } else {
        Err("Model Bridge works only in the app window.".to_string())
    }
}

/// Window check and switch check in one place.
fn on(webview: &tauri::Webview) -> Result<(), String> {
    guard(webview)?;
    if mb::enabled() {
        Ok(())
    } else {
        Err(mb::OFF.to_string())
    }
}

#[tauri::command]
pub async fn model_bridge_list(webview: tauri::Webview) -> Result<ListView, String> {
    guard(&webview)?;
    Ok(mb::with_real(mb::list))
}

/// Saves the pasted key in the credential store. Returns where it is kept (for "Key saved in …").
#[tauri::command]
pub async fn model_bridge_save_key(webview: tauri::Webview, adapter_id: String, key: String) -> Result<String, String> {
    on(&webview)?;
    mb::with_real(|d| mb::save_key(d, &adapter_id, &key))
}

#[tauri::command]
pub async fn model_bridge_remove(webview: tauri::Webview, adapter_id: String) -> Result<(), String> {
    on(&webview)?;
    mb::with_real(|d| mb::remove_key(d, &adapter_id))
}

/// Adds the person's own service (name and address). Returns its id.
#[tauri::command]
pub async fn model_bridge_add_custom(webview: tauri::Webview, name: String, url: String) -> Result<String, String> {
    on(&webview)?;
    mb::with_real(|d| mb::add_custom(d, &name, &url))
}

#[tauri::command]
pub async fn model_bridge_preview(webview: tauri::Webview, adapter_id: String) -> Result<PreviewView, String> {
    on(&webview)?;
    mb::with_real(|d| mb::preview(d, &adapter_id))
}

#[tauri::command]
pub async fn model_bridge_apply(webview: tauri::Webview, adapter_id: String) -> Result<DoneView, String> {
    on(&webview)?;
    mb::with_real(|d| mb::apply(d, &adapter_id))
}

#[tauri::command]
pub async fn model_bridge_undo(webview: tauri::Webview, adapter_id: String) -> Result<DoneView, String> {
    on(&webview)?;
    mb::with_real(|d| mb::undo(d, &adapter_id))
}

