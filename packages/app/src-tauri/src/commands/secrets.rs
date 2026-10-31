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

/// Set, read masked and delete: the provider keys and the Potato Bot sidecar secrets.
fn guard(name: &str) -> Result<(), String> {
    if secrets::is_ui_secret(name) {
        Ok(())
    } else {
        Err(format!("'{name}' is not a known secret"))
    }
}

/// Reading the raw value is for the provider keys only. A sidecar secret goes from the credential store
/// into the brain's environment and nowhere else; the webview never gets it back.
fn guard_for_llm(name: &str) -> Result<(), String> {
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
    guard_for_llm(&name)?;
    Ok(secrets::read_secret(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_get_for_llm_refuses_both_sidecar_names_and_the_masked_commands_take_them() {
        for name in secrets::SIDECAR_SECRETS {
            // Even with a value stored, the command refuses the name and returns no value.
            secrets::backend().set(name, "sk-test-sidecar-value-000000").unwrap();
            let result = secret_get_for_llm((*name).to_string());
            assert!(result.is_err(), "{name} must not be readable through secret_get_for_llm");
            assert!(!format!("{result:?}").contains("sk-test-sidecar-value"), "the refusal must not carry the value");
            secrets::backend().delete(name).unwrap();
        }
        // The masked commands do accept them, and still refuse strangers. (One test: both touch the one
        // process-wide in-memory store, so they must not run side by side.)
        let name = "deepseek_api_key".to_string();
        secret_set(name.clone(), "sk-test-deepseek-0000000000".to_string()).unwrap();
        let mask = secret_get_masked(name.clone()).unwrap();
        assert!(mask.set);
        assert_eq!(mask.last4.as_deref(), Some("0000"));
        secret_delete(name.clone()).unwrap();
        assert!(!secret_get_masked(name).unwrap().set);
        assert!(secret_set("anything_else".into(), "x".into()).is_err());
        assert!(secret_get_masked("anything_else".into()).is_err());
        assert!(secret_delete("anything_else".into()).is_err());
    }

    #[test]
    fn the_provider_keys_are_still_readable_for_the_orchestrator() {
        secrets::backend().set("openai_api_key", "sk-test-provider-key-000000").unwrap();
        assert_eq!(secret_get_for_llm("openai_api_key".into()).unwrap().as_deref(), Some("sk-test-provider-key-000000"));
        secrets::backend().delete("openai_api_key").unwrap();
    }
}
