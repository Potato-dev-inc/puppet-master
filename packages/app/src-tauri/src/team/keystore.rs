//! Team keys live in the OS credential store (Windows Credential Manager through `keyring`, the
//! same store as provider API keys, `crate::secrets`), never in a file, a log or a message:
//! - the **member key** (Ed25519): who I am on every team; also the root (admin) key of the teams I
//!   created;
//! - the **device keys**: an Ed25519 signing key and an X25519 key for HPKE, per install;
//! - the **epoch keys** of each team (32 bytes each, one per epoch);
//! - the **codes** of my open invites.
//!
//! Every name is prefixed with the profile (`PUPPET_MASTER_PROFILE`), so two profiles on one
//! computer are two separate people. Tests use the in-memory backend.

use super::crypto::{b64, new_signing_key, unb64_32, KemSecret, SigningKey};
use crate::secrets::SecretBackend;
use std::sync::Arc;

#[derive(Clone)]
pub struct KeyStore {
    backend: Arc<dyn SecretBackend>,
    prefix: String,
}

impl std::fmt::Debug for KeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyStore({})", self.prefix)
    }
}

/// The app's own store: the OS credential store (in tests, the in-memory one).
struct Global;

impl SecretBackend for Global {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        crate::secrets::backend().get(name)
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        crate::secrets::backend().set(name, value)
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        crate::secrets::backend().delete(name)
    }
    fn label(&self) -> &'static str {
        crate::secrets::backend().label()
    }
    fn available(&self) -> bool {
        crate::secrets::backend().available()
    }
}

impl KeyStore {
    /// The app's keys for this profile.
    pub fn app() -> Self {
        let profile = crate::app_paths::profile().unwrap_or_else(|| "default".into());
        Self { backend: Arc::new(Global), prefix: format!("team/{profile}/") }
    }

    /// A store over any backend (tests: `MemoryBackend`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_backend(backend: Arc<dyn SecretBackend>, prefix: &str) -> Self {
        Self { backend, prefix: format!("team/{prefix}/") }
    }

    pub fn label(&self) -> &'static str {
        self.backend.label()
    }

    fn name(&self, what: &str) -> String {
        format!("{}{what}", self.prefix)
    }

    fn get32(&self, what: &str) -> Result<Option<[u8; 32]>, String> {
        Ok(self.backend.get(&self.name(what))?.and_then(|v| unb64_32(&v)))
    }

    fn set32(&self, what: &str, value: &[u8; 32]) -> Result<(), String> {
        self.backend.set(&self.name(what), &b64(value))
    }

    fn get_or_make(&self, what: &str, make: impl FnOnce() -> [u8; 32]) -> Result<[u8; 32], String> {
        if let Some(v) = self.get32(what)? {
            return Ok(v);
        }
        let v = make();
        self.set32(what, &v)?;
        // Read back: never use a key the store did not keep.
        self.get32(what)?.filter(|back| back == &v).ok_or_else(|| "the credential store did not keep the key".into())
    }

    /// My member key (made on first use).
    pub fn member(&self) -> Result<SigningKey, String> {
        self.get_or_make("member", || new_signing_key().to_bytes()).map(|b| SigningKey::from_bytes(&b))
    }

    /// This install's signing key (made on first use).
    pub fn device_sign(&self) -> Result<SigningKey, String> {
        self.get_or_make("device-sign", || new_signing_key().to_bytes()).map(|b| SigningKey::from_bytes(&b))
    }

    /// This install's HPKE key (made on first use).
    pub fn device_kem(&self) -> Result<KemSecret, String> {
        self.get_or_make("device-kem", || KemSecret::generate().to_bytes()).map(KemSecret::from_bytes)
    }

    pub fn epoch_key(&self, team: &str, epoch: u64) -> Result<Option<[u8; 32]>, String> {
        self.get32(&format!("{team}/epoch/{epoch}"))
    }

    pub fn set_epoch_key(&self, team: &str, epoch: u64, key: &[u8; 32]) -> Result<(), String> {
        self.set32(&format!("{team}/epoch/{epoch}"), key)
    }

    pub fn invite_code(&self, team: &str, invite: &str) -> Result<Option<String>, String> {
        self.backend.get(&self.name(&format!("{team}/invite/{invite}")))
    }

    pub fn set_invite_code(&self, team: &str, invite: &str, code: &str) -> Result<(), String> {
        self.backend.set(&self.name(&format!("{team}/invite/{invite}")), code)
    }

    pub fn forget_invite(&self, team: &str, invite: &str) -> Result<(), String> {
        self.backend.delete(&self.name(&format!("{team}/invite/{invite}")))
    }

    /// Forget a team's epoch keys up to `last_epoch` (leaving a team).
    pub fn forget_team(&self, team: &str, last_epoch: u64) {
        for epoch in 1..=last_epoch {
            let _ = self.backend.delete(&self.name(&format!("{team}/epoch/{epoch}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryBackend;

    #[test]
    fn keys_are_made_once_and_kept_per_profile() {
        let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
        let maya = KeyStore::with_backend(backend.clone(), "maya");
        let leo = KeyStore::with_backend(backend.clone(), "leo");
        let first = maya.member().unwrap();
        assert_eq!(first.to_bytes(), maya.member().unwrap().to_bytes(), "made once");
        assert_ne!(first.to_bytes(), leo.member().unwrap().to_bytes(), "profiles are separate people");
        assert_eq!(maya.device_kem().unwrap().public_b64(), maya.device_kem().unwrap().public_b64());
        maya.set_epoch_key("T", 2, &[9; 32]).unwrap();
        assert_eq!(maya.epoch_key("T", 2).unwrap(), Some([9; 32]));
        assert_eq!(leo.epoch_key("T", 2).unwrap(), None);
    }

    #[test]
    fn no_store_means_no_keys() {
        let ks = KeyStore::with_backend(Arc::new(MemoryBackend::unavailable()), "x");
        assert!(ks.member().is_err());
    }
}
