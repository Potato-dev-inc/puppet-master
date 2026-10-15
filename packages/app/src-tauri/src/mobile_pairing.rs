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

    #[allow(dead_code)]
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

        let message = format!("{}|{}|{}", device_id, device_token, req.device_public_key);
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

/// Request headers that only a proxy, tunnel or CDN adds. The desktop app's own callers (webview,
/// CLI, MCP shim, Vite dev server) never send any of these to the bridge, so seeing one on a
/// loopback connection means a remote client reached us through a forwarder on this machine.
const REMOTE_INDICATOR_HEADERS: &[&str] = &[
    // our own PWA proxy and the Vite dev proxy
    "X-PM-Proxied",
    // Cloudflare (cloudflared tunnels)
    "Cf-Connecting-Ip",
    "Cf-Connecting-Ipv6",
    "Cf-Ray",
    "Cf-Visitor",
    "Cf-Ipcountry",
    "Cdn-Loop",
    "True-Client-Ip",
    // generic reverse proxies
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",
    "X-Forwarded-Port",
    "X-Real-Ip",
    "Forwarded",
];

/// True when the request carries any header a forwarder adds.
pub fn has_remote_indicators(headers: &str) -> bool {
    REMOTE_INDICATOR_HEADERS
        .iter()
        .any(|name| header_value(headers, name).is_some())
}

fn is_loopback_host(value: &str) -> bool {
    let value = value.trim();
    let host = match value.strip_prefix('[') {
        // bracketed IPv6, with or without a port
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => match value.matches(':').count() {
            0 => value,
            1 => value.split(':').next().unwrap_or(""),
            _ => value, // bare IPv6
        },
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// True when a `Host` header names something other than this machine. A browser page reaching the
/// bridge by DNS rebinding, or a reverse proxy pointed straight at the bridge port, arrives from
/// loopback but carries its own hostname. No `Host` at all (HTTP/1.0 tooling) is not suspicious.
fn names_foreign_host(headers: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.trim().eq_ignore_ascii_case("host") && !is_loopback_host(value)
        })
    })
}

/// True when an `Origin` header says the request was made by a web page that is not the desktop
/// app: any site open in the user's browser can fetch `http://127.0.0.1:<port>` (the bridge sends
/// `Access-Control-Allow-Origin: *`), so a loopback peer alone does not make it the app. The app's
/// own webview (`tauri://localhost`, `http(s)://tauri.localhost`) and the dev server on localhost
/// are loopback origins; CLI and MCP clients send no `Origin`.
fn names_foreign_origin(headers: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            if !key.trim().eq_ignore_ascii_case("origin") {
                return false;
            }
            let value = value.trim();
            let authority = value.split_once("://").map(|(_, rest)| rest);
            match authority {
                // `null` (sandboxed frames, file://) and anything unparsable is foreign
                None => true,
                Some(rest) => !is_loopback_host(rest.split('/').next().unwrap_or("")),
            }
        })
    })
}

/// Whether the caller must present a paired-device bearer token. Fails closed: only a loopback
/// peer with no sign of having been forwarded (see `REMOTE_INDICATOR_HEADERS`), no foreign `Host` and no
/// foreign `Origin` is treated as the desktop app itself.
pub fn requires_pairing_token(headers: &str, peer_loopback: bool) -> bool {
    !peer_loopback
        || has_remote_indicators(headers)
        || names_foreign_host(headers)
        || names_foreign_origin(headers)
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

    let store = pairing_store()
        .ok_or_else(|| (503, serde_json::json!({ "error": "pairing_unavailable" })))?;
    let valid = store.lock().validate_bearer_token(&token);
    if valid {
        Ok(())
    } else {
        Err((401, serde_json::json!({ "error": "invalid_token" })))
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

    #[test]
    fn any_forwarding_header_on_a_loopback_peer_requires_a_token() {
        for header in [
            "X-PM-Proxied: 1",
            "x-pm-proxied: 0",
            "Cf-Connecting-Ip: 203.0.113.9",
            "CF-RAY: 8a1b2c3d-AMS",
            "Cf-Visitor: {\"scheme\":\"https\"}",
            "Cdn-Loop: cloudflare",
            "True-Client-Ip: 203.0.113.9",
            "X-Forwarded-For: 203.0.113.9",
            "X-Forwarded-Host: example.com",
            "X-Forwarded-Proto: https",
            "X-Real-Ip: 203.0.113.9",
            "Forwarded: for=203.0.113.9",
        ] {
            let headers = format!("GET /panes HTTP/1.1\r\nHost: 127.0.0.1:17321\r\n{header}\r\n");
            assert!(requires_pairing_token(&headers, true), "{header}");
        }
    }

    #[test]
    fn local_tooling_without_forwarding_headers_stays_trusted() {
        for headers in [
            "GET /panes HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nAccept: */*\r\n",
            "GET /panes HTTP/1.1\r\nHost: localhost:17321\r\nOrigin: http://tauri.localhost\r\n",
            "GET /panes HTTP/1.1\r\nHost: [::1]:17321\r\n",
            "GET /panes HTTP/1.1\r\nHost: ::1\r\n",
            "GET /panes HTTP/1.1\r\nHost: app.localhost\r\nX-Puppet-Master-Session: s1\r\n",
            // HTTP/1.0-style client with no Host at all
            "GET /panes HTTP/1.0\r\nAccept: */*\r\n",
        ] {
            assert!(!requires_pairing_token(headers, true), "{headers:?}");
        }
        // a non-loopback peer always needs one, whatever it claims
        assert!(requires_pairing_token("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n", false));
    }

    #[test]
    fn a_foreign_host_name_on_a_loopback_peer_requires_a_token() {
        for host in [
            "evil.example",
            "evil.example:17321",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "192.168.1.20:17321",
            "",
        ] {
            let headers = format!("GET /panes HTTP/1.1\r\nHost: {host}\r\n");
            assert!(requires_pairing_token(&headers, true), "Host: {host:?}");
        }
        // a second Host header cannot hide behind a loopback first one
        assert!(requires_pairing_token(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nHost: evil.example\r\n",
            true
        ));
    }

    #[test]
    fn forwarded_loopback_request_is_refused_without_a_bearer() {
        let segments = ["panes"];
        for headers in [
            "GET /panes HTTP/1.1\r\nHost: 127.0.0.1\r\nCf-Connecting-Ip: 203.0.113.9\r\n",
            "GET /panes HTTP/1.1\r\nHost: tunnel.example\r\n",
        ] {
            let err = authorize_bridge_request(headers, true, &segments, "GET")
                .expect_err("a forwarded caller needs a token");
            assert_eq!(err.0, 401, "{headers:?}");
        }
        // public paths stay open for the pairing handshake and health checks
        assert!(authorize_bridge_request("Cf-Ray: x\r\n", true, &["health"], "GET").is_ok());
    }

    #[test]
    fn a_web_page_origin_on_a_loopback_peer_requires_a_token() {
        for origin in [
            "https://evil.example",
            "http://evil.example:8080",
            "null",
            "https://localhost.evil.example",
            "file://",
        ] {
            let headers = format!(
                "POST /panes/p1/input HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nOrigin: {origin}\r\n"
            );
            assert!(requires_pairing_token(&headers, true), "Origin: {origin}");
        }
        for origin in [
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "http://localhost:1420",
            "http://127.0.0.1:1420",
            "http://[::1]:4173",
        ] {
            let headers = format!(
                "POST /panes/p1/input HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nOrigin: {origin}\r\n"
            );
            assert!(!requires_pairing_token(&headers, true), "Origin: {origin}");
        }
    }
}
