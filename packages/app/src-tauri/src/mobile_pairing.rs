//! Mobile PWA pairing — Ed25519 server identity + device bearer tokens.

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const PAIRING_CODE_TTL_SECS: u64 = 600;
const PAIRING_CODE_LEN: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDevice {
    id: String,
    name: String,
    token_hash_b64: String,
    public_key_b64: String,
    paired_at: u64,
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
}

#[derive(Debug, Clone)]
struct PendingPairing {
    code: String,
    expires_at: u64,
    bridge_url: String,
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
        })
    }

    fn persist_pending_fields(&self) -> (Option<String>, Option<u64>, Option<String>) {
        match &self.pending {
            Some(p) => (
                Some(p.code.clone()),
                Some(p.expires_at),
                Some(p.bridge_url.clone()),
            ),
            None => (None, None, None),
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
            return Ok(Self {
                path,
                signing_key,
                server_public_b64: persisted.server_public_b64,
                devices: persisted.devices,
                pending,
            });
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
        let (pending_code, pending_expires_at, pending_bridge_url) = self.persist_pending_fields();
        let data = PersistedStore {
            server_secret_b64: encode_b64(self.signing_key.as_bytes()),
            server_public_b64: self.server_public_b64.clone(),
            devices: self.devices.clone(),
            pending_code,
            pending_expires_at,
            pending_bridge_url,
        };
        let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&self.path, json).map_err(|e| e.to_string())
    }

    pub fn server_public_key_b64(&self) -> &str {
        &self.server_public_b64
    }

    pub fn list_devices(&self) -> Vec<PairedDeviceInfo> {
        self.devices
            .iter()
            .map(|d| PairedDeviceInfo {
                id: d.id.clone(),
                name: d.name.clone(),
                public_key: d.public_key_b64.clone(),
                paired_at: d.paired_at,
            })
            .collect()
    }

    pub fn revoke_device(&mut self, device_id: &str) -> Result<bool, String> {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != device_id);
        if self.devices.len() == before {
            return Ok(false);
        }
        self.persist()?;
        Ok(true)
    }

    pub fn create_pairing_session(&mut self, bridge_url: String) -> PairingSession {
        let code = random_pairing_code();
        let expires_at = now_secs() + PAIRING_CODE_TTL_SECS;
        self.pending = Some(PendingPairing {
            code: code.clone(),
            expires_at,
            bridge_url: bridge_url.clone(),
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

        let device_id = uuid::Uuid::new_v4().to_string();
        let mut token_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let device_token = encode_b64(&token_bytes);
        let token_hash = Sha256::digest(token_bytes);
        let token_hash_b64 = encode_b64(token_hash.as_slice());

        self.devices.push(PersistedDevice {
            id: device_id.clone(),
            name: req.device_name.trim().chars().take(120).collect(),
            token_hash_b64,
            public_key_b64: req.device_public_key.clone(),
            paired_at: now_secs(),
        });
        self.pending = None;
        self.persist()?;

        let message = format!(
            "{}|{}|{}",
            device_id, device_token, req.device_public_key
        );
        let signature = self.signing_key.sign(message.as_bytes());

        Ok(PairResponseBody {
            device_id,
            device_token,
            server_public_key: self.server_public_b64.clone(),
            server_signature: encode_b64(signature.to_bytes().as_slice()),
        })
    }

    pub fn validate_bearer_token(&self, token: &str) -> bool {
        let bytes = match decode_b64(token) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let hash = Sha256::digest(&bytes);
        let hash_b64 = encode_b64(hash.as_slice());
        self.devices.iter().any(|d| d.token_hash_b64 == hash_b64)
    }
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

/// Strip a leading `bridge` segment when a reverse proxy forwards `/bridge/*` to the
/// embedded bridge without rewriting the path (common with custom domains).
pub fn normalize_bridge_segments<'a>(segments: &'a [&'a str]) -> &'a [&'a str] {
    match segments.first() {
        Some(&"bridge") if segments.len() > 1 => &segments[1..],
        _ => segments,
    }
}

pub fn is_public_bridge_path(segments: &[&str], method: &str) -> bool {
    let segments = normalize_bridge_segments(segments);
    (segments == ["health"] && method == "GET")
        || (segments == ["pair"] && method == "POST")
        || (segments.len() == 3
            && segments[0] == "pair"
            && segments[1] == "session"
            && method == "GET")
}

pub fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

pub fn bearer_token(headers: &str) -> Option<String> {
    let auth = header_value(headers, "Authorization")?;
    auth.strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .map(|t| t.trim().to_string())
}

pub fn requires_pairing_token(headers: &str, peer_loopback: bool) -> bool {
    if header_value(headers, "X-PM-Proxied").is_some() {
        return true;
    }
    !peer_loopback
}

pub fn authorize_bridge_request(
    headers: &str,
    peer_loopback: bool,
    segments: &[&str],
    method: &str,
) -> Result<(), (u16, serde_json::Value)> {
    let segments = normalize_bridge_segments(segments);
    if is_public_bridge_path(segments, method) {
        return Ok(());
    }

    let needs_token = requires_pairing_token(headers, peer_loopback);
    if !needs_token {
        return Ok(());
    }

    let token = bearer_token(headers).ok_or_else(|| {
        (
            401,
            serde_json::json!({
                "error": "pairing_required",
                "hint": "Scan the desktop QR code to pair this device before remote control.",
            }),
        )
    })?;

    let store = pairing_store().ok_or_else(|| {
        (
            503,
            serde_json::json!({ "error": "pairing_unavailable" }),
        )
    })?;
    let valid = store.lock().validate_bearer_token(&token);
    if valid {
        Ok(())
    } else {
        Err((
            401,
            serde_json::json!({ "error": "invalid_token" }),
        ))
    }
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
mod tests {
    use super::*;

    #[test]
    fn public_paths_accept_bridge_prefix() {
        let segments = ["bridge", "health"];
        assert!(is_public_bridge_path(&segments, "GET"));

        let segments = ["bridge", "pair", "session", "CODE12"];
        assert!(is_public_bridge_path(&segments, "GET"));

        let segments = ["bridge", "pair"];
        assert!(is_public_bridge_path(&segments, "POST"));
    }

    #[test]
    fn proxied_bridge_prefix_does_not_bypass_pairing() {
        let segments = ["bridge", "panes"];
        let err = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &segments, "GET")
            .expect_err("panes should require auth");
        assert_eq!(err.0, 401);
    }
}
