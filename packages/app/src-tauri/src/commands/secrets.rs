//! Tauri commands for provider API keys kept in the OS credential store. See `crate::secrets`.

use crate::secrets::{self, SecretMask};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SecretStorageInfo {
    /// True when keys can be kept in the OS credential store.
    pub secure: bool,
    /// Where they are kept, in plain words.
    pub label: String,
}

fn guard(name: &str) -> Result<(), String> {
    if secrets::is_provider_secret(name) {
        Ok(())
    } else {
        Err(format!("'{name}' is not a known secret"))
    }
}

#[tauri::command]
pub fn secret_storage_info() -> SecretStorageInfo {
    let b = secrets::backend();
    SecretStorageInfo { secure: b.available(), label: b.label().to_string() }
}

#[tauri::command]
pub fn secret_set(name: String, value: String) -> Result<(), String> {
    guard(&name)?;
    let value = value.trim();
    if value.is_empty() {
        return Err("the key is empty".into());
    }
    let b = secrets::backend();
    b.set(&name, value)?;
    match b.get(&name)? {
        Some(back) if back == value => Ok(()),
        _ => Err("the credential store did not keep the key".into()),
    }
}

/// Whether the secret is set plus its last 4 characters. Never the value.
#[tauri::command]
pub fn secret_get_masked(name: String) -> Result<SecretMask, String> {
    guard(&name)?;
    secrets::get_masked(secrets::backend(), &name)
}

#[tauri::command]
pub fn secret_delete(name: String) -> Result<(), String> {
    guard(&name)?;
    secrets::backend().delete(&name)
}

/// INTERNAL: the only command that returns a raw key. The sidebar's API orchestrator runs its LLM
/// tool loop in the webview and needs the key for the provider request; no other UI reads it.
/// Restricted to the provider keys.
#[tauri::command]
pub fn secret_get_for_llm(name: String) -> Result<Option<String>, String> {
    guard(&name)?;
    Ok(secrets::read_secret(&name))
}
