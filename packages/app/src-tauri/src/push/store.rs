//! Push subscriptions, one per paired device, in their own file (`puppet-master.push.json`).
//! Kept apart from the pairing store so a push problem can never corrupt pairing.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const MAX_ENDPOINT_LEN: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    /// The browser ECDH public key (65-byte uncompressed point, base64url).
    pub p256dh: String,
    /// The browser 16-byte auth secret (base64url).
    pub auth: String,
    pub created_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    /// The VAPID public key the subscriptions were created with. A different key invalidates them.
    #[serde(default)]
    vapid_public: String,
    #[serde(default)]
    subscriptions: BTreeMap<String, Subscription>,
}

pub struct SubscriptionStore {
    path: PathBuf,
    file: StoreFile,
}

/// Accept only what a real browser push service looks like. A paired phone is trusted to push to
/// the web, but not to make this computer call arbitrary hosts, so internal addresses are refused.
pub fn validate(sub: &Subscription) -> Result<(), String> {
    if sub.endpoint.len() > MAX_ENDPOINT_LEN {
        return Err("endpoint is too long".into());
    }
    let rest = sub
        .endpoint
        .strip_prefix("https://")
        .ok_or("endpoint must be https")?;
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.contains('@') {
        return Err("endpoint must not carry credentials".into());
    }
    let host = host_port
        .rsplit_once(':')
        .map_or(host_port, |(h, p)| {
            if p.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                host_port
            }
        })
        .to_ascii_lowercase();
    let looks_internal = host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || !host.contains('.')
        || host.chars().all(|c| c.is_ascii_digit() || c == '.');
    if looks_internal
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("endpoint host is not a public push service".into());
    }
    let key = URL_SAFE_NO_PAD
        .decode(&sub.p256dh)
        .map_err(|_| "p256dh is not base64url")?;
    if key.len() != 65 || key[0] != 4 {
        return Err("p256dh must be an uncompressed P-256 point".into());
    }
    if web_push_native::p256::PublicKey::from_sec1_bytes(&key).is_err() {
        return Err("p256dh is not a valid P-256 key".into());
    }
    let auth = URL_SAFE_NO_PAD
        .decode(&sub.auth)
        .map_err(|_| "auth is not base64url")?;
    if auth.len() != 16 {
        return Err("auth must be 16 bytes".into());
    }
    Ok(())
}

impl SubscriptionStore {
    /// Load the file. Subscriptions made under a different VAPID key are dropped (they can no
    /// longer be addressed).
    pub fn load(path: PathBuf, vapid_public: &str) -> Self {
        let mut file: StoreFile = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if file.vapid_public != vapid_public {
            file.subscriptions.clear();
            file.vapid_public = vapid_public.to_string();
        }
        Self { path, file }
    }

    fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create {}: {err}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(&self.file).map_err(|err| err.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|err| format!("write {}: {err}", tmp.display()))?;
        fs::rename(&tmp, &self.path)
            .map_err(|err| format!("replace {}: {err}", self.path.display()))
    }

    pub fn upsert(&mut self, device_id: &str, sub: Subscription) -> Result<(), String> {
        validate(&sub)?;
        self.file.subscriptions.insert(device_id.to_string(), sub);
        self.persist()
    }

    pub fn remove(&mut self, device_id: &str) -> Result<bool, String> {
        let existed = self.file.subscriptions.remove(device_id).is_some();
        if existed {
            self.persist()?;
        }
        Ok(existed)
    }

    /// Drop the subscription that uses `endpoint` (the push service said it is gone).
    pub fn remove_endpoint(&mut self, endpoint: &str) -> Result<(), String> {
        let before = self.file.subscriptions.len();
        self.file
            .subscriptions
            .retain(|_, sub| sub.endpoint != endpoint);
        if self.file.subscriptions.len() != before {
            self.persist()?;
        }
        Ok(())
    }

    pub fn get(&self, device_id: &str) -> Option<&Subscription> {
        self.file.subscriptions.get(device_id)
    }

    pub fn all(&self) -> Vec<(String, Subscription)> {
        self.file
            .subscriptions
            .iter()
            .map(|(id, sub)| (id.clone(), sub.clone()))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.file.subscriptions.is_empty()
    }
}

#[cfg(test)]
pub(crate) fn test_subscription(
    endpoint: &str,
) -> (Subscription, web_push_native::p256::SecretKey, [u8; 16]) {
    use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;
    let secret = web_push_native::p256::SecretKey::random(&mut rand::rngs::OsRng);
    let auth: [u8; 16] = rand::random();
    let sub = Subscription {
        endpoint: endpoint.to_string(),
        p256dh: URL_SAFE_NO_PAD.encode(secret.public_key().to_encoded_point(false).as_bytes()),
        auth: URL_SAFE_NO_PAD.encode(auth),
        created_at: 1,
    };
    (sub, secret, auth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("pm-push-{name}-{}", std::process::id()))
            .join("push.json")
    }

    #[test]
    fn subscriptions_persist_per_device_in_their_own_file() {
        let p = path("store-persist");
        let _ = fs::remove_file(&p);
        let (sub, _, _) = test_subscription("https://fcm.googleapis.com/fcm/send/abc");
        let mut store = SubscriptionStore::load(p.clone(), "VAPID-A");
        store.upsert("device-1", sub.clone()).unwrap();
        let reloaded = SubscriptionStore::load(p.clone(), "VAPID-A");
        assert_eq!(reloaded.get("device-1"), Some(&sub));
        assert_eq!(reloaded.get("device-2"), None);
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn removing_a_device_drops_only_its_subscription() {
        let p = path("store-remove");
        let (a, _, _) = test_subscription("https://web.push.apple.com/a");
        let (b, _, _) = test_subscription("https://updates.push.services.mozilla.com/b");
        let mut store = SubscriptionStore::load(p.clone(), "K");
        store.upsert("a", a).unwrap();
        store.upsert("b", b.clone()).unwrap();
        assert!(store.remove("a").unwrap());
        assert!(!store.remove("a").unwrap());
        let reloaded = SubscriptionStore::load(p.clone(), "K");
        assert_eq!(reloaded.all(), vec![("b".to_string(), b)]);
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_new_vapid_key_invalidates_old_subscriptions() {
        let p = path("store-rekey");
        let (a, _, _) = test_subscription("https://web.push.apple.com/a");
        let mut store = SubscriptionStore::load(p.clone(), "OLD");
        store.upsert("a", a).unwrap();
        let reloaded = SubscriptionStore::load(p.clone(), "NEW");
        assert!(reloaded.is_empty());
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn gone_endpoints_are_pruned() {
        let p = path("store-gone");
        let (a, _, _) = test_subscription("https://web.push.apple.com/a");
        let mut store = SubscriptionStore::load(p.clone(), "K");
        store.upsert("a", a).unwrap();
        store
            .remove_endpoint("https://web.push.apple.com/a")
            .unwrap();
        assert!(store.is_empty());
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn validation_refuses_internal_hosts_and_bad_keys() {
        let (ok, _, _) = test_subscription("https://fcm.googleapis.com/fcm/send/x");
        assert!(validate(&ok).is_ok());
        for bad in [
            "http://fcm.googleapis.com/x",
            "https://localhost/x",
            "https://127.0.0.1/x",
            "https://192.168.1.4:8443/x",
            "https://intranet/x",
            "https://user@evil.example.com/x",
            "https://[::1]/x",
            "https://printer.local/x",
        ] {
            let mut sub = ok.clone();
            sub.endpoint = bad.into();
            assert!(validate(&sub).is_err(), "{bad} should be refused");
        }
        let mut sub = ok.clone();
        sub.p256dh = "AAAA".into();
        assert!(validate(&sub).is_err());
        let mut sub = ok;
        sub.auth = "AAAA".into();
        assert!(validate(&sub).is_err());
    }
}
