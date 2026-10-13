//! The VAPID P-256 keypair that identifies this Puppet Master to the browsers' push services.
//!
//! It lives in its own file in the app data dir, next to (not inside) the pairing store. The same
//! key must be used for as long as phones are subscribed: a subscription is bound to the public
//! key it was created with.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use web_push_native::jwt_simple::algorithms::{
    ECDSAP256KeyPairLike, ECDSAP256PublicKeyLike, ES256KeyPair,
};

pub struct VapidKeys {
    pair: ES256KeyPair,
    public_b64: String,
}

#[derive(Serialize, Deserialize)]
struct VapidFile {
    /// Raw 32-byte P-256 private scalar, base64url without padding.
    private_b64url: String,
}

fn public_b64(pair: &ES256KeyPair) -> String {
    URL_SAFE_NO_PAD.encode(pair.public_key().public_key().to_bytes_uncompressed())
}

impl VapidKeys {
    /// Load the saved key, or create and save a new one. A damaged file is replaced (the old
    /// subscriptions are then dropped by the store because their key no longer matches).
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if let Some(pair) = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<VapidFile>(&text).ok())
            .and_then(|file| URL_SAFE_NO_PAD.decode(file.private_b64url).ok())
            .and_then(|bytes| ES256KeyPair::from_bytes(&bytes).ok())
        {
            let public_b64 = public_b64(&pair);
            return Ok(Self { pair, public_b64 });
        }
        let pair = ES256KeyPair::generate();
        let file = VapidFile {
            private_b64url: URL_SAFE_NO_PAD.encode(pair.to_bytes()),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        crate::secure_file::write_private(path, &text)?;
        let public_b64 = public_b64(&pair);
        Ok(Self { pair, public_b64 })
    }

    /// The `applicationServerKey` the PWA passes to `pushManager.subscribe` (65-byte uncompressed
    /// point, base64url).
    pub fn public_key_b64url(&self) -> &str {
        &self.public_b64
    }

    pub fn pair(&self) -> &ES256KeyPair {
        &self.pair
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("pm-push-{name}-{}", std::process::id()))
            .join("vapid.json")
    }

    #[test]
    fn key_is_created_once_and_survives_a_restart() {
        let path = temp_path("vapid-persist");
        let _ = fs::remove_file(&path);
        let first = VapidKeys::load_or_create(&path).unwrap();
        let second = VapidKeys::load_or_create(&path).unwrap();
        assert_eq!(first.public_key_b64url(), second.public_key_b64url());
        // 65-byte uncompressed P-256 point -> 87 base64url chars; 0x04 prefix -> starts with "B".
        assert_eq!(first.public_key_b64url().len(), 87);
        assert!(first.public_key_b64url().starts_with('B'));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_damaged_file_is_replaced_with_a_fresh_key() {
        let path = temp_path("vapid-damaged");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        let keys = VapidKeys::load_or_create(&path).unwrap();
        let again = VapidKeys::load_or_create(&path).unwrap();
        assert_eq!(keys.public_key_b64url(), again.public_key_b64url());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn different_installs_get_different_keys() {
        let a = VapidKeys::load_or_create(&temp_path("vapid-a")).unwrap();
        let b = VapidKeys::load_or_create(&temp_path("vapid-b")).unwrap();
        assert_ne!(a.public_key_b64url(), b.public_key_b64url());
        let _ = fs::remove_dir_all(temp_path("vapid-a").parent().unwrap());
        let _ = fs::remove_dir_all(temp_path("vapid-b").parent().unwrap());
    }
}
