//! Push subscriptions, one per paired device, in their own file (`puppet-master.push.json`).
//! Kept apart from the pairing store so a push problem can never corrupt pairing.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    /// Devices whose notifications show no details on a lock screen ("Hide details on lock screen").
    /// Off for everyone by default, which is how notifications always read.
    #[serde(default)]
    hide_details: BTreeSet<String>,
}

pub struct SubscriptionStore {
    path: PathBuf,
    file: StoreFile,
}

/// Host suffixes of the browser push services. An endpoint must be one of these (or a subdomain),
/// so a paired phone cannot make this computer call an arbitrary or internal host.
/// Chrome/Android: FCM. Firefox: Mozilla autopush. Safari/iOS: Apple. Edge: WNS.
/// Not verified against live subscriptions in this build (Apple and Edge are from memory);
/// `PUPPET_MASTER_PUSH_HOSTS` (comma separated suffixes) adds more without a rebuild.
pub const PUSH_HOST_SUFFIXES: &[&str] = &[
    "fcm.googleapis.com",
    "push.apple.com",
    "push.services.mozilla.com",
    "notify.windows.com",
    "wns.windows.com",
];

fn allowed_suffixes() -> Vec<String> {
    let mut all: Vec<String> = PUSH_HOST_SUFFIXES.iter().map(|s| s.to_string()).collect();
    if let Ok(extra) = std::env::var("PUPPET_MASTER_PUSH_HOSTS") {
        all.extend(extra.split(',').map(|s| s.trim().trim_start_matches("*.").to_ascii_lowercase()).filter(|s| s.contains('.')));
    }
    all
}

/// A host label that a URL parser or the OS resolver may read as a number: decimal (`2130706433`),
/// octal/leading-zero (`0177`) or hex (`0x7f`). Such hosts are IPv4 in disguise.
fn label_is_numeric_ish(label: &str) -> bool {
    let l = label.to_ascii_lowercase();
    if l.is_empty() {
        return false;
    }
    if let Some(hex) = l.strip_prefix("0x") {
        return hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    l.chars().all(|c| c.is_ascii_digit())
}

/// The host of an https endpoint, or why it is refused.
pub fn endpoint_host(endpoint: &str) -> Result<String, String> {
    if endpoint.len() > MAX_ENDPOINT_LEN {
        return Err("endpoint is too long".into());
    }
    let rest = endpoint.strip_prefix("https://").ok_or("endpoint must be https")?;
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.contains('@') {
        return Err("endpoint must not carry credentials".into());
    }
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (host_port, None),
    };
    if port.is_some_and(|p| p != "443") {
        return Err("endpoint port must be 443".into());
    }
    let host = host.to_ascii_lowercase();
    if host.is_empty() || host.ends_with('.') {
        return Err("endpoint host must not end with a dot".into());
    }
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err("endpoint host is not a public push service".into());
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty() || label_is_numeric_ish(l)) {
        return Err("endpoint host must be a name, not an address".into());
    }
    if !allowed_suffixes().iter().any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}"))) {
        return Err("endpoint host is not a known browser push service".into());
    }
    Ok(host)
}

pub fn validate(sub: &Subscription) -> Result<(), String> {
    endpoint_host(&sub.endpoint)?;
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
        let text = serde_json::to_string_pretty(&self.file).map_err(|err| err.to_string())?;
        crate::secure_file::write_private(&self.path, &text)
    }

    pub fn upsert(&mut self, device_id: &str, sub: Subscription) -> Result<(), String> {
        validate(&sub)?;
        self.file.subscriptions.insert(device_id.to_string(), sub);
        self.persist()
    }

    pub fn remove(&mut self, device_id: &str) -> Result<bool, String> {
        let existed = self.file.subscriptions.remove(device_id).is_some();
        let had_preference = self.file.hide_details.remove(device_id);
        if existed || had_preference {
            self.persist()?;
        }
        Ok(existed)
    }

    /// Whether this device asked for notifications without details on its lock screen.
    pub fn hide_details(&self, device_id: &str) -> bool {
        self.file.hide_details.contains(device_id)
    }

    pub fn set_hide_details(&mut self, device_id: &str, hide: bool) -> Result<(), String> {
        let changed = if hide { self.file.hide_details.insert(device_id.to_string()) } else { self.file.hide_details.remove(device_id) };
        if changed {
            self.persist()?;
        }
        Ok(())
    }

    /// Every subscription with its device's "hide details" switch.
    pub fn all_with_preferences(&self) -> Vec<(String, Subscription, bool)> {
        self.file
            .subscriptions
            .iter()
            .map(|(id, sub)| (id.clone(), sub.clone(), self.file.hide_details.contains(id)))
            .collect()
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

    #[test]
    fn ssrf_bypasses_are_refused() {
        for bad in [
            "https://localhost./x",                 // trailing dot
            "https://fcm.googleapis.com./x",        // trailing dot on an allowed name
            "https://0x7f.0.0.1/x",                 // hex label
            "https://0177.0.0.1/x",                 // octal label
            "https://2130706433/x",                 // decimal form
            "https://127.1/x",
            "https://evil.example.com/x",           // public name, not a push service
            "https://fcm.googleapis.com.evil.com/x", // suffix trick
            "https://notfcm.googleapis.com.evil.com/x",
            "https://evilfcm.googleapis.com/x",     // not a subdomain boundary
            "https://fcm.googleapis.com:8443/x",    // odd port
            "https://metadata.google.internal/x",
        ] {
            assert!(endpoint_host(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn real_push_services_and_their_subdomains_pass() {
        for good in [
            "https://fcm.googleapis.com/fcm/send/a",
            "https://fcm.googleapis.com:443/wp/a",
            "https://web.push.apple.com/Q",
            "https://updates.push.services.mozilla.com/wpush/v2/a",
            "https://wns2-par02p.notify.windows.com/w/?token=a",
            "https://dm3p.wns.windows.com/x",
        ] {
            assert!(endpoint_host(good).is_ok(), "{good} should pass");
        }
    }

    #[test]
    fn hide_details_is_off_by_default_per_device_and_forgotten_with_the_device() {
        let dir = std::env::temp_dir().join(format!("pm-push-hide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("push.json");
        let mut store = SubscriptionStore::load(path.clone(), "v1");
        assert!(!store.hide_details("dev-a"), "off by default: notifications read as they always did");
        store.set_hide_details("dev-a", true).unwrap();
        assert!(store.hide_details("dev-a") && !store.hide_details("dev-b"));
        // It survives a restart.
        let store2 = SubscriptionStore::load(path.clone(), "v1");
        assert!(store2.hide_details("dev-a"));
        let mut store = store2;
        let (sub, _, _) = test_subscription("https://fcm.googleapis.com/a");
        store.upsert("dev-a", sub).unwrap();
        assert_eq!(store.all_with_preferences().iter().map(|(id, _, hide)| (id.as_str(), *hide)).collect::<Vec<_>>(), [("dev-a", true)]);
        // A revoked device takes its switch with it.
        store.remove("dev-a").unwrap();
        assert!(!SubscriptionStore::load(path, "v1").hide_details("dev-a"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
