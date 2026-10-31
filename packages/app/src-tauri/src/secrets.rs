//! Provider API keys live in the OS credential store (Windows Credential Manager, macOS Keychain,
//! Linux keyutils) through the `keyring` crate (MIT OR Apache-2.0), not in `settings.json`.
//!
//! - The UI never receives a full key: `mask` returns "is it set" plus the last 4 characters.
//! - Rust code reads keys with `read_secret`.
//! - The only path that hands a raw key to the webview is `secret_get_for_llm`
//!   (commands/secrets.rs): the sidebar's API orchestrator still runs its LLM tool loop in the
//!   webview and needs the key for its request headers. It is restricted to the provider keys.
//! - `migrate_settings_file` moves keys that an older version wrote into `settings.json`.
//! - If the credential store is unavailable nothing here claims otherwise: callers fall back to
//!   the file and the UI says so.

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub const SERVICE: &str = "com.puppetmaster.app";

/// Secrets the UI may set, read masked and delete. Everything else is rejected.
pub const PROVIDER_SECRETS: &[&str] = &["anthropic_api_key", "openai_api_key", "openrouter_api_key"];

pub fn is_provider_secret(name: &str) -> bool {
    PROVIDER_SECRETS.contains(&name)
}

/// Secrets only a Potato Bot brain process reads (the model key, a chat app's bot token). The UI may set,
/// read masked and delete them like a provider key; but they are NEVER handed to the webview:
/// `secret_get_for_llm` stays provider-only. The brain gets them in its own environment at spawn.
pub const SIDECAR_SECRETS: &[&str] = &["deepseek_api_key", "discord_bot_token"];

pub fn is_sidecar_secret(name: &str) -> bool {
    SIDECAR_SECRETS.contains(&name)
}

/// Names the UI may set, read masked and delete.
pub fn is_ui_secret(name: &str) -> bool {
    is_provider_secret(name) || is_sidecar_secret(name)
}

pub trait SecretBackend: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    fn delete(&self, name: &str) -> Result<(), String>;
    /// Plain-language name of where the secret is kept, shown in the UI.
    fn label(&self) -> &'static str;
    /// False when the credential store cannot be used at all (headless Linux, locked-down policy).
    fn available(&self) -> bool;
}

/// The real OS credential store.
pub struct KeyringBackend;

impl KeyringBackend {
    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
    }
}

impl SecretBackend for KeyringBackend {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match Self::entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        Self::entry(name)?.set_password(value).map_err(|e| e.to_string())
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn label(&self) -> &'static str {
        if cfg!(windows) {
            "Windows Credential Manager"
        } else if cfg!(target_os = "macos") {
            "the macOS Keychain"
        } else {
            "the system keyring"
        }
    }
    fn available(&self) -> bool {
        // A missing entry is a healthy answer; a platform failure means there is no store.
        self.get("__availability_probe__").is_ok()
    }
}

/// In-memory backend: unit tests, and a deterministic stand-in anywhere a store is injected.
#[derive(Default)]
pub struct MemoryBackend {
    values: Mutex<HashMap<String, String>>,
    pub unavailable: bool,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn unavailable() -> Self {
        Self { values: Mutex::default(), unavailable: true }
    }
}

impl SecretBackend for MemoryBackend {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        if self.unavailable {
            return Err("no credential store".into());
        }
        Ok(self.values.lock().get(name).cloned())
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if self.unavailable {
            return Err("no credential store".into());
        }
        self.values.lock().insert(name.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        if self.unavailable {
            return Err("no credential store".into());
        }
        self.values.lock().remove(name);
        Ok(())
    }
    fn label(&self) -> &'static str {
        "memory"
    }
    fn available(&self) -> bool {
        !self.unavailable
    }
}

/// The process-wide store. Unit tests never touch the user's real credential store.
#[cfg(not(test))]
pub fn backend() -> &'static dyn SecretBackend {
    static B: KeyringBackend = KeyringBackend;
    &B
}

#[cfg(test)]
pub fn backend() -> &'static dyn SecretBackend {
    static B: once_cell::sync::Lazy<MemoryBackend> = once_cell::sync::Lazy::new(MemoryBackend::new);
    &*B
}

/// Read a secret for Rust-side consumers (empty values count as unset).
pub fn read_secret(name: &str) -> Option<String> {
    backend().get(name).ok().flatten().filter(|v| !v.is_empty())
}

/// What the UI may know about a secret: never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretMask {
    pub set: bool,
    /// Last 4 characters, only for keys long enough that 4 characters reveal little.
    pub last4: Option<String>,
}

pub fn mask(value: Option<&str>) -> SecretMask {
    match value.filter(|v| !v.is_empty()) {
        None => SecretMask { set: false, last4: None },
        Some(v) => {
            let n = v.chars().count();
            let last4 = (n >= 12).then(|| v.chars().skip(n - 4).collect());
            SecretMask { set: true, last4 }
        }
    }
}

pub fn get_masked(b: &dyn SecretBackend, name: &str) -> Result<SecretMask, String> {
    Ok(mask(b.get(name)?.as_deref()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    /// Keys moved into the credential store and removed from the file.
    pub migrated: Vec<String>,
    /// The credential store was unusable: the keys stay in the file.
    pub store_unavailable: bool,
}

/// One-time, idempotent move of provider keys from `settings.json` (`{"settings": {...}}`) into
/// the credential store. A key is removed from the file only after reading it back from the store.
pub fn migrate_settings_file(path: &Path, b: &dyn SecretBackend) -> Result<MigrationReport, String> {
    let mut report = MigrationReport::default();
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(report) };
    let Ok(mut file) = serde_json::from_str::<Value>(&text) else { return Ok(report) };
    let Some(settings) = file.get_mut("settings").and_then(Value::as_object_mut) else { return Ok(report) };

    let pending: Vec<(String, String)> = PROVIDER_SECRETS
        .iter()
        .filter_map(|name| {
            let v = settings.get(*name)?.as_str()?.trim().to_string();
            (!v.is_empty()).then(|| ((*name).to_string(), v))
        })
        .collect();
    // Blank values are just noise from older saves: drop them.
    let mut changed = false;
    for name in PROVIDER_SECRETS {
        if settings.get(*name).and_then(Value::as_str).is_some_and(|v| v.trim().is_empty()) {
            settings.remove(*name);
            changed = true;
        }
    }
    if !pending.is_empty() && !b.available() {
        report.store_unavailable = true;
        return Ok(report); // keep keys in the file, never claim they are secure
    }
    for (name, value) in pending {
        let stored = b.set(&name, &value).and_then(|_| b.get(&name));
        match stored {
            Ok(Some(back)) if back == value => {
                settings.remove(&name);
                report.migrated.push(name);
                changed = true;
            }
            _ => report.store_unavailable = true, // write did not stick: keep the file copy
        }
    }
    if changed {
        let tmp = path.with_extension("json.tmp");
        let out = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    }
    Ok(report)
}

/// Startup hook: migrate `<app data>/settings.json` with the real store.
pub fn migrate_on_startup() {
    let path = crate::app_paths::app_data_dir().join("settings.json");
    match migrate_settings_file(&path, backend()) {
        Ok(r) if !r.migrated.is_empty() => tracing::info!(moved = r.migrated.len(), "API keys moved to the OS credential store"),
        Ok(r) if r.store_unavailable => tracing::warn!("OS credential store unavailable: API keys stay in settings.json"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "API key migration failed"),
    }
    crate::opencode::keys::migrate_profiles_to_secret_store();
    crate::toolbelt::store::migrate_on_startup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KEY: &str = "sk-ant-api03-abcdefghijklmnop-WXYZ";

    fn temp_file(content: &Value) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-secrets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, serde_json::to_string(content).unwrap()).unwrap();
        p
    }

    fn file_json(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn migration_moves_the_key_to_the_store_and_blanks_the_file() {
        let b = MemoryBackend::new();
        let p = temp_file(&json!({"settings": {"anthropic_api_key": KEY, "openai_api_key": "", "theme": "dark"}}));
        let r = migrate_settings_file(&p, &b).unwrap();
        assert_eq!(r.migrated, vec!["anthropic_api_key".to_string()]);
        assert_eq!(b.get("anthropic_api_key").unwrap().as_deref(), Some(KEY));
        let f = file_json(&p);
        assert!(f["settings"].get("anthropic_api_key").is_none());
        assert!(f["settings"].get("openai_api_key").is_none());
        assert_eq!(f["settings"]["theme"], "dark");
        assert!(!std::fs::read_to_string(&p).unwrap().contains(KEY));
    }

    #[test]
    fn migration_is_idempotent() {
        let b = MemoryBackend::new();
        let p = temp_file(&json!({"settings": {"anthropic_api_key": KEY}}));
        migrate_settings_file(&p, &b).unwrap();
        let after_first = std::fs::read_to_string(&p).unwrap();
        let r = migrate_settings_file(&p, &b).unwrap();
        assert!(r.migrated.is_empty() && !r.store_unavailable);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), after_first);
        assert_eq!(b.get("anthropic_api_key").unwrap().as_deref(), Some(KEY));
    }

    #[test]
    fn migration_keeps_the_key_in_the_file_when_the_store_is_unavailable() {
        let b = MemoryBackend::unavailable();
        let p = temp_file(&json!({"settings": {"anthropic_api_key": KEY}}));
        let r = migrate_settings_file(&p, &b).unwrap();
        assert!(r.store_unavailable && r.migrated.is_empty());
        assert_eq!(file_json(&p)["settings"]["anthropic_api_key"], KEY);
    }

    #[test]
    fn migration_keeps_the_key_when_the_readback_does_not_match() {
        struct Lossy(MemoryBackend);
        impl SecretBackend for Lossy {
            fn get(&self, n: &str) -> Result<Option<String>, String> {
                self.0.get(n).map(|v| v.map(|_| "different".into()))
            }
            fn set(&self, n: &str, v: &str) -> Result<(), String> {
                self.0.set(n, v)
            }
            fn delete(&self, n: &str) -> Result<(), String> {
                self.0.delete(n)
            }
            fn label(&self) -> &'static str {
                "lossy"
            }
            fn available(&self) -> bool {
                true
            }
        }
        let p = temp_file(&json!({"settings": {"openai_api_key": KEY}}));
        let r = migrate_settings_file(&p, &Lossy(MemoryBackend::new())).unwrap();
        assert!(r.migrated.is_empty());
        assert_eq!(file_json(&p)["settings"]["openai_api_key"], KEY);
    }

    #[test]
    fn migration_ignores_missing_or_unrelated_files() {
        let b = MemoryBackend::new();
        assert_eq!(migrate_settings_file(Path::new("Z:/nope/settings.json"), &b).unwrap(), MigrationReport::default());
        let p = temp_file(&json!({"other": 1}));
        assert!(migrate_settings_file(&p, &b).unwrap().migrated.is_empty());
    }

    #[test]
    fn masked_get_never_returns_the_full_key() {
        let b = MemoryBackend::new();
        b.set("anthropic_api_key", KEY).unwrap();
        let m = get_masked(&b, "anthropic_api_key").unwrap();
        assert!(m.set);
        assert_eq!(m.last4.as_deref(), Some("WXYZ"));
        let encoded = serde_json::to_string(&m).unwrap();
        assert!(!encoded.contains(&KEY[..KEY.len() - 4]));
        assert_eq!(get_masked(&b, "openai_api_key").unwrap(), SecretMask { set: false, last4: None });
    }

    #[test]
    fn short_secrets_expose_no_characters() {
        assert_eq!(mask(Some("abcd1234")), SecretMask { set: true, last4: None });
        assert_eq!(mask(Some("")), SecretMask { set: false, last4: None });
    }

    #[test]
    fn only_provider_secrets_are_ui_addressable() {
        assert!(is_provider_secret("openrouter_api_key"));
        assert!(!is_provider_secret("opencode_profile_a"));
        assert!(!is_provider_secret("../etc"));
    }

    #[test]
    fn sidecar_secrets_are_ui_addressable_but_not_provider_secrets() {
        for name in SIDECAR_SECRETS {
            assert!(is_sidecar_secret(name), "{name}");
            assert!(is_ui_secret(name), "{name}");
            assert!(!is_provider_secret(name), "{name} must not be a provider key");
        }
        assert!(is_ui_secret("anthropic_api_key"));
        assert!(!is_ui_secret("../deepseek_api_key"));
        assert!(!is_sidecar_secret("openai_api_key"));
    }

    #[test]
    fn set_get_delete_roundtrip() {
        let b = MemoryBackend::new();
        b.set("openai_api_key", KEY).unwrap();
        assert_eq!(b.get("openai_api_key").unwrap().as_deref(), Some(KEY));
        b.delete("openai_api_key").unwrap();
        assert_eq!(b.get("openai_api_key").unwrap(), None);
        b.delete("openai_api_key").unwrap();
    }
}
