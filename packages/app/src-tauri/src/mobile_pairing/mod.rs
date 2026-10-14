//! Mobile PWA pairing — Ed25519 server identity + device bearer tokens.

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

mod auth;
mod caps;
pub mod events;
pub mod local_secret;

pub use auth::*;
pub use caps::*;

use events::SecurityEvent;

const PAIRING_CODE_TTL_SECS: u64 = 600;
const PAIRING_CODE_LEN: usize = 8;

/// A paired device is signed out after this long without a single request. Every request slides
/// the window, so a phone in use never expires.
pub const DEVICE_IDLE_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
/// The sliding expiry is written to disk at most this often per device (it moves on every request).
const SLIDE_PERSIST_EVERY_SECS: u64 = 60 * 60;

fn all_caps() -> Caps {
    Caps::all()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDevice {
    id: String,
    name: String,
    token_hash_b64: String,
    public_key_b64: String,
    paired_at: u64,
    /// What this device may do. Pairings made before caps existed have none stored and get all
    /// of them: they keep doing exactly what they did.
    #[serde(default = "all_caps")]
    caps: Caps,
    /// Unix seconds after which the token is refused (sliding: last use + 30 days). `None` only
    /// in a file written before expiry existed; loading sets it to now + 30 days.
    #[serde(default)]
    exp: Option<u64>,
    #[serde(default)]
    last_seen: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStore {
    server_secret_b64: String,
    server_public_b64: String,
    devices: Vec<PersistedDevice>,
    #[serde(default)]
    pending_code: Option<String>,
    #[serde(default)]
    pending_expires_at: Option<u64>,
    #[serde(default)]
    pending_bridge_url: Option<String>,
    #[serde(default)]
    pending_caps: Option<Caps>,
}

#[derive(Debug, Clone)]
struct PendingPairing {
    code: String,
    expires_at: u64,
    bridge_url: String,
    /// What the device that redeems this code will be allowed to do (chosen on the desktop).
    caps: Caps,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingSessionInfo {
    pub pairing_code: String,
    pub expires_at: u64,
    pub server_public_key: String,
    pub bridge_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairedDeviceInfo {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub paired_at: u64,
    pub caps: Caps,
    /// Unix seconds when the device is signed out unless it is used before then.
    pub expires_at: u64,
    pub last_seen: Option<u64>,
    /// Signed out after going unused for the whole window; it must pair again.
    pub expired: bool,
    /// The length of the sliding window, for the "expires if unused for 30 days" copy.
    pub idle_expiry_secs: u64,
}

/// The outcome of presenting a bearer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenCheck {
    Valid { device_id: String, caps: Caps },
    /// The token belongs to a device that went unused for the whole expiry window.
    Expired { device_id: String },
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingSession {
    pub pairing_code: String,
    pub expires_at: u64,
    pub server_public_key: String,
    pub bridge_url: String,
    pub qr_payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct PairRequestBody {
    pub pairing_code: String,
    pub device_name: String,
    pub device_public_key: String,
}

#[derive(Debug, Serialize)]
pub struct PairResponseBody {
    pub device_id: String,
    pub device_token: String,
    pub server_public_key: String,
    pub server_signature: String,
}

pub struct MobilePairingStore {
    path: PathBuf,
    signing_key: SigningKey,
    server_public_b64: String,
    devices: Vec<PersistedDevice>,
    pending: Option<PendingPairing>,
}

impl MobilePairingStore {
    fn restore_pending(persisted: &PersistedStore) -> Option<PendingPairing> {
        let code = persisted.pending_code.as_ref()?;
        let expires_at = persisted.pending_expires_at?;
        let bridge_url = persisted.pending_bridge_url.clone()?;
        if now_secs() > expires_at {
            return None;
        }
        Some(PendingPairing {
            code: code.clone(),
            expires_at,
            bridge_url,
            caps: persisted.pending_caps.unwrap_or_else(Caps::all),
        })
    }

    #[allow(clippy::type_complexity)]
    fn persist_pending_fields(
        &self,
    ) -> (Option<String>, Option<u64>, Option<String>, Option<Caps>) {
        match &self.pending {
            Some(p) => (
                Some(p.code.clone()),
                Some(p.expires_at),
                Some(p.bridge_url.clone()),
                Some(p.caps),
            ),
            None => (None, None, None, None),
        }
    }

    pub fn load_or_create(path: PathBuf) -> Result<Self, String> {
        if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let persisted: PersistedStore =
                serde_json::from_str(&raw).map_err(|e| format!("parse pairing store: {e}"))?;
            let secret_bytes = decode_b64(&persisted.server_secret_b64)?;
            if secret_bytes.len() != 32 {
                return Err("invalid server secret length".into());
            }
            let mut arr = [0_u8; 32];
            arr.copy_from_slice(&secret_bytes);
            let signing_key = SigningKey::from_bytes(&arr);
            let pending = Self::restore_pending(&persisted);
            // Migration from a file written before caps and expiry existed: every device keeps all
            // caps (serde default) and starts its 30-day window now. Written back once.
            let needs_migration = persisted.devices.iter().any(|d| d.exp.is_none());
            let now = now_secs();
            let mut devices = persisted.devices;
            for device in devices.iter_mut().filter(|d| d.exp.is_none()) {
                device.exp = Some(now + DEVICE_IDLE_EXPIRY_SECS);
            }
            let store = Self {
                path,
                signing_key,
                server_public_b64: persisted.server_public_b64,
                devices,
                pending,
            };
            if needs_migration {
                store.persist()?;
            }
            store.publish_view();
            return Ok(store);
        }

        let signing_key = SigningKey::generate(&mut OsRng);
        let server_public_b64 = encode_b64(signing_key.verifying_key().as_bytes());
        let store = Self {
            path,
            signing_key,
            server_public_b64: server_public_b64.clone(),
            devices: Vec::new(),
            pending: None,
        };
        store.persist()?;
        Ok(store)
    }

    fn persist(&self) -> Result<(), String> {
        let (pending_code, pending_expires_at, pending_bridge_url, pending_caps) =
            self.persist_pending_fields();
        let data = PersistedStore {
            server_secret_b64: encode_b64(self.signing_key.as_bytes()),
            server_public_b64: self.server_public_b64.clone(),
            devices: self.devices.clone(),
            pending_code,
            pending_expires_at,
            pending_bridge_url,
            pending_caps,
        };
        let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // The file holds the server signing key and the token hashes: private, written atomically.
        crate::secure_file::write_private(&self.path, &json)
    }

    /// Mirror the live devices' caps for the event-stream fanout (see [`stream_caps`]). Expired
    /// devices are left out: their open streams close on the next payload.
    fn publish_view(&self) {
        let now = now_secs();
        let mut view = DEVICE_VIEW.write();
        for device in &self.devices {
            if device_expired(device, now) {
                view.remove(&device.id);
            } else {
                view.insert(device.id.clone(), (device.caps, device.exp));
            }
        }
    }

    pub fn list_devices(&self) -> Vec<PairedDeviceInfo> {
        self.list_devices_at(now_secs())
    }

    pub fn list_devices_at(&self, now: u64) -> Vec<PairedDeviceInfo> {
        self.devices
            .iter()
            .map(|d| PairedDeviceInfo {
                id: d.id.clone(),
                name: d.name.clone(),
                public_key: d.public_key_b64.clone(),
                paired_at: d.paired_at,
                caps: d.caps,
                expires_at: d.exp.unwrap_or(now + DEVICE_IDLE_EXPIRY_SECS),
                last_seen: d.last_seen,
                expired: device_expired(d, now),
                idle_expiry_secs: DEVICE_IDLE_EXPIRY_SECS,
            })
            .collect()
    }

    /// True when the device is paired and not signed out by expiry.
    pub fn is_active_device(&self, device_id: &str) -> bool {
        let now = now_secs();
        self.devices.iter().any(|d| d.id == device_id && !device_expired(d, now))
    }

    /// The caps of a paired, unexpired device.
    pub fn device_caps(&self, device_id: &str) -> Option<Caps> {
        let now = now_secs();
        self.devices
            .iter()
            .find(|d| d.id == device_id && !device_expired(d, now))
            .map(|d| d.caps)
    }

    pub fn revoke_device(&mut self, device_id: &str) -> Result<bool, String> {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != device_id);
        if self.devices.len() == before {
            return Ok(false);
        }
        self.persist()?;
        DEVICE_VIEW.write().remove(device_id);
        crate::push::on_device_revoked(device_id);
        events::emit(SecurityEvent::DeviceRevoked { device_id: device_id.to_string() });
        Ok(true)
    }

    /// Change what a paired device may do. It applies to the device's next request and to the
    /// next payload of any event stream it has open. `Ok(false)`: no such device.
    pub fn set_device_caps(&mut self, device_id: &str, caps: Caps) -> Result<bool, String> {
        let Some(device) = self.devices.iter_mut().find(|d| d.id == device_id) else {
            return Ok(false);
        };
        if device.caps == caps {
            return Ok(true);
        }
        device.caps = caps;
        self.persist()?;
        self.publish_view();
        events::emit(SecurityEvent::DeviceCapsChanged {
            device_id: device_id.to_string(),
            caps: caps.list(),
        });
        Ok(true)
    }

    /// A pairing code whose device gets every capability (today's behaviour).
    pub fn create_pairing_session(&mut self, bridge_url: String) -> PairingSession {
        self.create_pairing_session_with_caps(bridge_url, Caps::all())
    }

    /// A pairing code; the device that redeems it gets `caps` (chosen on the desktop).
    pub fn create_pairing_session_with_caps(
        &mut self,
        bridge_url: String,
        caps: Caps,
    ) -> PairingSession {
        let code = random_pairing_code();
        let expires_at = now_secs() + PAIRING_CODE_TTL_SECS;
        self.pending = Some(PendingPairing {
            code: code.clone(),
            expires_at,
            bridge_url: bridge_url.clone(),
            caps,
        });
        let _ = self.persist();
        let qr_payload = serde_json::json!({
            "v": 1,
            "u": bridge_url,
            "pk": self.server_public_b64,
            "c": code,
            "e": expires_at,
        });
        PairingSession {
            pairing_code: code,
            expires_at,
            server_public_key: self.server_public_b64.clone(),
            bridge_url,
            qr_payload,
        }
    }

    pub fn lookup_pairing_session(&self, code: &str) -> Result<PairingSessionInfo, String> {
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| "no active pairing session — refresh QR on desktop".to_string())?;
        if pending.code != code {
            return Err("invalid pairing code".into());
        }
        if now_secs() > pending.expires_at {
            return Err("pairing code expired".into());
        }
        Ok(PairingSessionInfo {
            pairing_code: pending.code.clone(),
            expires_at: pending.expires_at,
            server_public_key: self.server_public_b64.clone(),
            bridge_url: pending.bridge_url.clone(),
        })
    }

    pub fn pair_device(&mut self, req: PairRequestBody) -> Result<PairResponseBody, String> {
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| "no active pairing session — refresh QR on desktop".to_string())?;
        if pending.code != req.pairing_code {
            return Err("invalid pairing code".into());
        }
        if now_secs() > pending.expires_at {
            return Err("pairing code expired".into());
        }

        let device_pk_bytes = decode_b64(&req.device_public_key)?;
        if device_pk_bytes.len() != 32 {
            return Err("device public key must be 32 bytes".into());
        }
        let _verify = VerifyingKey::from_bytes(
            device_pk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| "invalid device public key")?,
        )
        .map_err(|e| format!("invalid device public key: {e}"))?;

        let caps = pending.caps;
        let device_id = uuid::Uuid::new_v4().to_string();
        let mut token_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let device_token = encode_b64(&token_bytes);
        let token_hash = Sha256::digest(token_bytes);
        let token_hash_b64 = encode_b64(token_hash.as_slice());

        let now = now_secs();
        self.devices.push(PersistedDevice {
            id: device_id.clone(),
            name: req.device_name.trim().chars().take(120).collect(),
            token_hash_b64,
            public_key_b64: req.device_public_key.clone(),
            paired_at: now,
            caps,
            exp: Some(now + DEVICE_IDLE_EXPIRY_SECS),
            last_seen: Some(now),
        });
        self.pending = None;
        self.persist()?;
        DEVICE_VIEW.write().insert(device_id.clone(), (caps, Some(now + DEVICE_IDLE_EXPIRY_SECS)));
        events::emit(SecurityEvent::DevicePaired { device_id: device_id.clone(), caps: caps.list() });

        let message = format!("{}|{}|{}", device_id, device_token, req.device_public_key);
        let signature = self.signing_key.sign(message.as_bytes());

        Ok(PairResponseBody {
            device_id,
            device_token,
            server_public_key: self.server_public_b64.clone(),
            server_signature: encode_b64(signature.to_bytes().as_slice()),
        })
    }

    /// The device that owns `token`, if any (push routes act on behalf of exactly this device).
    pub fn device_id_for_token(&self, token: &str) -> Option<String> {
        let bytes = decode_b64(token).ok()?;
        let hash_b64 = encode_b64(Sha256::digest(&bytes).as_slice());
        self.devices.iter().find(|d| d.token_hash_b64 == hash_b64).map(|d| d.id.clone())
    }

    /// True for the token of a paired, unexpired device. Not a use: the expiry does not slide.
    pub fn validate_bearer_token(&self, token: &str) -> bool {
        let Some(hash_b64) = token_hash(token) else { return false };
        let now = now_secs();
        self.devices
            .iter()
            .any(|d| d.token_hash_b64 == hash_b64 && !device_expired(d, now))
    }

    /// Present a bearer token: whose it is and what that device may do. A valid token is a use of
    /// the device, so its expiry slides to now + 30 days.
    pub fn check_token(&mut self, token: &str) -> TokenCheck {
        self.check_token_at(token, now_secs())
    }

    pub fn check_token_at(&mut self, token: &str, now: u64) -> TokenCheck {
        let Some(hash_b64) = token_hash(token) else { return TokenCheck::Unknown };
        let Some(device) = self.devices.iter_mut().find(|d| d.token_hash_b64 == hash_b64) else {
            return TokenCheck::Unknown;
        };
        if device_expired(device, now) {
            let device_id = device.id.clone();
            if DEVICE_VIEW.write().remove(&device_id).is_some() {
                events::emit(SecurityEvent::DeviceExpired { device_id: device_id.clone() });
            }
            return TokenCheck::Expired { device_id };
        }
        let stored_exp = device.exp.unwrap_or(0);
        let new_exp = (now + DEVICE_IDLE_EXPIRY_SECS).max(stored_exp);
        device.last_seen = Some(now);
        device.exp = Some(new_exp);
        let (device_id, caps) = (device.id.clone(), device.caps);
        // The fanout's mirror slides with the device, so an open stream ends when the device expires.
        if let Some(entry) = DEVICE_VIEW.write().get_mut(&device_id) {
            entry.1 = Some(new_exp);
        }
        // The window moves on every request; the file only needs it roughly right.
        if new_exp >= stored_exp + SLIDE_PERSIST_EVERY_SECS {
            if let Err(err) = self.persist() {
                tracing::warn!(%err, "could not save a paired device's new expiry");
            }
        }
        TokenCheck::Valid { device_id, caps }
    }
}

#[cfg(test)]
impl MobilePairingStore {
    /// Put a device's expiry at `exp` (unix secs), as if it had gone unused.
    pub fn set_device_expiry_for_test(&mut self, device_id: &str, exp: u64) {
        if let Some(device) = self.devices.iter_mut().find(|d| d.id == device_id) {
            device.exp = Some(exp);
        }
    }
}

#[cfg(test)]
pub fn now_secs_for_test() -> u64 {
    now_secs()
}

fn token_hash(token: &str) -> Option<String> {
    let bytes = decode_b64(token).ok()?;
    Some(encode_b64(Sha256::digest(&bytes).as_slice()))
}

fn device_expired(device: &PersistedDevice, now: u64) -> bool {
    device.exp.is_some_and(|exp| now > exp)
}

/// device id -> (caps, expiry) of every live paired device. The event-stream fanout reads it
/// instead of taking the store lock (revoking holds the store lock while it talks to push, which
/// publishes).
static DEVICE_VIEW: once_cell::sync::Lazy<
    parking_lot::RwLock<std::collections::HashMap<String, (Caps, Option<u64>)>>,
> = once_cell::sync::Lazy::new(Default::default);

/// The caps of a live paired device, for the event-stream fanout. `None`: revoked or expired, so
/// its open streams close. The expiry is checked here too (review R12 2.3): a lost phone holding a
/// stream open past its 30 days stops receiving without making a new request. An open stream is not
/// "use": the window slides on requests (the long poll makes one every 25 s; SSE and WS reconnect).
pub fn stream_caps(device_id: &str) -> Option<Caps> {
    stream_caps_at(device_id, now_secs())
}

pub(crate) fn stream_caps_at(device_id: &str, now: u64) -> Option<Caps> {
    let (caps, exp) = DEVICE_VIEW.read().get(device_id).copied()?;
    if exp.is_some_and(|exp| now > exp) {
        return None;
    }
    Some(caps)
}

pub type SharedPairingStore = Arc<Mutex<MobilePairingStore>>;

static PAIRING_STORE: once_cell::sync::OnceCell<SharedPairingStore> =
    once_cell::sync::OnceCell::new();

pub fn init_pairing_store(path: PathBuf) -> Result<SharedPairingStore, String> {
    let store = Arc::new(Mutex::new(MobilePairingStore::load_or_create(path)?));
    let _ = PAIRING_STORE.set(store.clone());
    Ok(store)
}

pub fn pairing_store() -> Option<SharedPairingStore> {
    PAIRING_STORE.get().cloned()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_b64(value: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|e| format!("base64 decode: {e}"))
}

fn random_pairing_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0_u8; PAIRING_CODE_LEN];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char)
        .collect()
}

#[cfg(test)]
mod tests;
