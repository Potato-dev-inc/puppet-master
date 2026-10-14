//! Wire types of the Puppet Master team relay ("Heads-up", COWORK-PLAN §3, §8; SAAS-PLAN §3).
//!
//! The relay is a dumb, untrusted post office. Everything a teammate says travels inside an
//! [`Envelope`] whose payload is encrypted end to end with the team's epoch key; the relay sees the
//! team id, the sending device's public key, the epoch, a per-sender counter, the size and the time.
//! It never sees names, branches, notes, keys or code.
//!
//! What the relay can check without any secret:
//! - an envelope is signed by the device that claims to send it ([`Envelope::verify`]);
//! - a device request is signed by that device ([`verify_request`]) and the device is in the team's
//!   access list;
//! - the access list and the two state slots (`roster`, `keys`) are signed by the team's root key,
//!   and the team id is bound to that root key ([`team_id`]).
//!
//! What it cannot do: read anything, add a member, or forge a teammate's event. It can drop or
//! delay, which clients notice through per-sender counters (gap detection, in the app).
//!
//! No I/O here: the app (client) and the relay servers (axum, Cloudflare) share these types.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Protocol name and version, sent as `x-pm-relay` and returned by `/v1/health`.
pub const PROTOCOL: &str = "pm-relay/1";
/// Largest envelope (JSON) a relay accepts.
pub const MAX_ENVELOPE_BYTES: usize = 24 * 1024;
/// Largest join request a relay accepts (unauthenticated, so small).
pub const MAX_JOIN_BYTES: usize = 4 * 1024;
/// Largest signed state blob (`roster`, `keys`).
pub const MAX_STATE_BYTES: usize = 256 * 1024;
/// How long a relay keeps an envelope.
pub const MAILBOX_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// How long a join request waits for the admin.
pub const JOIN_TTL_MS: u64 = 24 * 60 * 60 * 1000;
/// A signed device request older or newer than this is refused.
pub const AUTH_SKEW_MS: u64 = 5 * 60 * 1000;
/// Header carrying a device's request signature: `v1 <device> <ts_ms> <sig>`.
pub const AUTH_HEADER: &str = "x-pm-auth";
/// The two state slots a team keeps on the relay.
pub const SLOTS: [&str; 2] = ["roster", "keys"];

// ------------------------------------------------------------------------------------ encoding

pub fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn unb64(text: &str) -> Option<Vec<u8>> {
    B64.decode(text.trim()).ok()
}

/// A 32-byte value from base64url (public keys).
pub fn unb64_32(text: &str) -> Option<[u8; 32]> {
    unb64(text)?.try_into().ok()
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Crockford base32 (no padding) of `bytes`.
pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The team id: 26 characters of Crockford base32 over SHA-256(`pm-team-id-v1`, root key, salt).
/// It binds the team to its root key, so a relay (or anyone) cannot swap in another admin.
pub fn team_id(root_pk: &[u8; 32], salt: &[u8]) -> String {
    let digest = sha256(&[b"pm-team-id-v1\0", root_pk, salt]);
    base32(&digest[..16])
}

/// A plausible team id (shape only).
pub fn is_team_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| CROCKFORD.contains(&b))
}

/// A plausible device or member id: base64url of a 32-byte Ed25519 public key.
pub fn is_key_id(id: &str) -> bool {
    id.len() == 43 && unb64_32(id).is_some()
}

fn verifying_key(id: &str) -> Option<VerifyingKey> {
    VerifyingKey::from_bytes(&unb64_32(id)?).ok()
}

/// Check an Ed25519 signature (`sig_b64`) by the key `key_id` over `message`. Strict verification
/// (no malleable signatures, no small-order keys).
pub fn verify_sig(key_id: &str, message: &[u8], sig_b64: &str) -> bool {
    let Some(key) = verifying_key(key_id) else { return false };
    let Some(bytes) = unb64(sig_b64) else { return false };
    let Ok(sig) = Signature::from_slice(&bytes) else { return false };
    key.verify_strict(message, &sig).is_ok()
}

pub fn sign(key: &SigningKey, message: &[u8]) -> String {
    b64(&key.sign(message).to_bytes())
}

pub fn key_id(key: &SigningKey) -> String {
    b64(key.verifying_key().as_bytes())
}

// ------------------------------------------------------------------------------------ envelope

/// One sealed message between teammates. The relay stores and forwards it; only team devices
/// holding the epoch key can open it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// Always 1.
    pub v: u8,
    /// ULID (26 Crockford characters): unique, sortable by time.
    pub id: String,
    pub team: String,
    /// Which team key sealed it.
    pub epoch: u64,
    /// Sending device: base64url of its Ed25519 public key.
    pub from: String,
    /// Per-sending-device counter, from 1. Gaps tell a recipient something was dropped.
    pub seq: u64,
    /// XChaCha20-Poly1305 nonce, base64url (24 bytes).
    pub nonce: String,
    /// Ciphertext, base64url. The plaintext is padded to a size bucket.
    pub ct: String,
    /// Ed25519 by `from` over [`Envelope::signing_bytes`], base64url.
    pub sig: String,
}

impl Envelope {
    /// Everything but the ciphertext and signature: the AEAD associated data, so a relay cannot
    /// move a ciphertext under another header.
    pub fn header_bytes(&self) -> Vec<u8> {
        format!(
            "pm-envelope-v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.v, self.id, self.team, self.epoch, self.from, self.seq, self.nonce
        )
        .into_bytes()
    }

    /// What the sender signs: the header and the ciphertext.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = self.header_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(self.ct.as_bytes());
        bytes
    }

    /// Shape checks a relay applies before storing (no secrets needed).
    pub fn check_shape(&self) -> Result<(), &'static str> {
        if self.v != 1 {
            return Err("unsupported envelope version");
        }
        if self.id.len() != 26 || !self.id.bytes().all(|b| CROCKFORD.contains(&b)) {
            return Err("bad envelope id");
        }
        if !is_team_id(&self.team) {
            return Err("bad team id");
        }
        if !is_key_id(&self.from) {
            return Err("bad sender");
        }
        if self.seq == 0 {
            return Err("bad sequence number");
        }
        if unb64(&self.nonce).map(|n| n.len()) != Some(24) {
            return Err("bad nonce");
        }
        if unb64(&self.ct).is_none() {
            return Err("bad ciphertext");
        }
        Ok(())
    }

    /// The shape is right and `from` signed it. Says nothing about team membership.
    pub fn verify(&self) -> bool {
        self.check_shape().is_ok() && verify_sig(&self.from, &self.signing_bytes(), &self.sig)
    }

    /// Sign as `key` (which must be the `from` device).
    pub fn sign_with(&mut self, key: &SigningKey) {
        self.from = key_id(key);
        self.sig = sign(key, &self.signing_bytes());
    }
}

/// An envelope as the relay hands it out, with its place in the team's mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivered {
    /// The relay's own counter for this team (cursor for `after=`).
    pub seq: u64,
    pub at_ms: u64,
    pub envelope: Envelope,
}

/// `GET …/envelopes` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Batch {
    pub items: Vec<Delivered>,
    /// Pass as `after` next time.
    pub next: u64,
    /// The team's newest mailbox position (the relay's high-water mark). Older relays leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<u64>,
    /// `after` was past `head`: the relay's counter is behind this client (a restored or replaced
    /// database). `items` and `next` start again from the beginning of what this device may read;
    /// the client must take `next` as its cursor even though it is lower, and rely on its replay
    /// guard for envelopes it has already seen.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reset: bool,
}

/// The one control frame a relay WebSocket sends besides [`Delivered`] items: the `after` the
/// client connected with is past the team's `head`, so the relay starts again from the beginning
/// of what this device may read (see [`Batch::reset`]). `{"reset":true,"head":N}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resync {
    pub reset: bool,
    pub head: u64,
}

// ------------------------------------------------------------------------------- signed state

/// Which devices may use the team's mailbox, signed by the root key. Newer `version` wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedAcl {
    pub team: String,
    pub version: u64,
    /// Device ids (base64url Ed25519 public keys).
    pub devices: Vec<String>,
    pub sig: String,
}

impl SignedAcl {
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!("pm-acl-v1\n{}\n{}\n{}", self.team, self.version, self.devices.join(",")).into_bytes()
    }

    pub fn signed(team: &str, version: u64, devices: Vec<String>, root: &SigningKey) -> Self {
        let mut acl = Self { team: team.into(), version, devices, sig: String::new() };
        acl.sig = sign(root, &acl.signing_bytes());
        acl
    }

    pub fn verify(&self, root_id: &str) -> bool {
        self.devices.iter().all(|d| is_key_id(d)) && verify_sig(root_id, &self.signing_bytes(), &self.sig)
    }
}

/// `POST /v1/teams`: a new team on this relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRegistration {
    pub team: String,
    /// Root (admin) public key, base64url.
    pub root: String,
    /// The salt that, with `root`, gives `team` ([`team_id`]), base64url.
    pub salt: String,
    pub acl: SignedAcl,
}

impl TeamRegistration {
    /// The team id is bound to the root key and the access list is signed by it.
    pub fn verify(&self) -> bool {
        let (Some(root), Some(salt)) = (unb64_32(&self.root), unb64(&self.salt)) else { return false };
        team_id(&root, &salt) == self.team && self.acl.team == self.team && self.acl.verify(&self.root)
    }
}

/// A small team-wide value kept by the relay (no TTL), signed by the root key. `body` is opaque to
/// the relay (the roster is encrypted with the team key; the key grant is sealed per device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedBlob {
    pub team: String,
    /// `roster` or `keys`.
    pub slot: String,
    pub version: u64,
    /// base64url.
    pub body: String,
    pub sig: String,
}

impl SignedBlob {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let digest = sha256(&[self.body.as_bytes()]);
        format!("pm-blob-v1\n{}\n{}\n{}\n{}", self.team, self.slot, self.version, hex(&digest)).into_bytes()
    }

    pub fn signed(team: &str, slot: &str, version: u64, body: &[u8], root: &SigningKey) -> Self {
        let mut blob = Self { team: team.into(), slot: slot.into(), version, body: b64(body), sig: String::new() };
        blob.sig = sign(root, &blob.signing_bytes());
        blob
    }

    pub fn verify(&self, root_id: &str) -> bool {
        SLOTS.contains(&self.slot.as_str()) && verify_sig(root_id, &self.signing_bytes(), &self.sig)
    }

    pub fn body_bytes(&self) -> Option<Vec<u8>> {
        unb64(&self.body)
    }
}

/// A request to join, sealed to the admin's device. The relay sees only the team and the invite id
/// (to count attempts per invite).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinBox {
    pub team: String,
    /// The invite's public id (not its code).
    pub invite: String,
    /// HPKE encapsulated key, base64url.
    pub enc: String,
    /// HPKE ciphertext, base64url.
    pub ct: String,
    pub at_ms: u64,
}

impl JoinBox {
    pub fn check_shape(&self) -> Result<(), &'static str> {
        if !is_team_id(&self.team) {
            return Err("bad team id");
        }
        if self.invite.is_empty() || self.invite.len() > 32 || !self.invite.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("bad invite id");
        }
        if unb64(&self.enc).is_none() || unb64(&self.ct).is_none() {
            return Err("bad sealed request");
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------- device request auth

/// What a device signs to call the relay: method, path with query, time and the body's hash.
pub fn auth_message(method: &str, path_and_query: &str, ts_ms: u64, body: &[u8]) -> Vec<u8> {
    format!(
        "pm-relay-auth-v1\n{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path_and_query,
        ts_ms,
        hex(&sha256(&[body]))
    )
    .into_bytes()
}

/// The `x-pm-auth` header value for one request.
pub fn auth_header(device: &SigningKey, method: &str, path_and_query: &str, ts_ms: u64, body: &[u8]) -> String {
    let sig = sign(device, &auth_message(method, path_and_query, ts_ms, body));
    format!("v1 {} {} {}", key_id(device), ts_ms, sig)
}

/// A verified device request: who sent it, when, and the signature (for a replay cache).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthedRequest {
    pub device: String,
    pub ts_ms: u64,
    pub sig: String,
}

/// Check an `x-pm-auth` header against the request. The caller still checks the device is in the
/// team's access list and that the signature was not seen before.
pub fn verify_request(
    header: &str,
    method: &str,
    path_and_query: &str,
    body: &[u8],
    now_ms: u64,
) -> Result<AuthedRequest, &'static str> {
    let mut parts = header.split_whitespace();
    if parts.next() != Some("v1") {
        return Err("unknown auth scheme");
    }
    let (Some(device), Some(ts), Some(sig), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err("malformed auth header");
    };
    let ts_ms: u64 = ts.parse().map_err(|_| "bad timestamp")?;
    if ts_ms.abs_diff(now_ms) > AUTH_SKEW_MS {
        return Err("request time too far from the relay's clock");
    }
    if !is_key_id(device) {
        return Err("bad device id");
    }
    if !verify_sig(device, &auth_message(method, path_and_query, ts_ms, body), sig) {
        return Err("bad request signature");
    }
    Ok(AuthedRequest { device: device.to_string(), ts_ms, sig: sig.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn envelope(device: &SigningKey) -> Envelope {
        let root = key(9);
        let mut env = Envelope {
            v: 1,
            id: "01J0000000000000000000000A".into(),
            team: team_id(root.verifying_key().as_bytes(), b"salt"),
            epoch: 1,
            from: String::new(),
            seq: 1,
            nonce: b64(&[7u8; 24]),
            ct: b64(b"opaque"),
            sig: String::new(),
        };
        env.sign_with(device);
        env
    }

    #[test]
    fn an_envelope_verifies_and_any_change_breaks_it() {
        let device = key(1);
        let env = envelope(&device);
        assert!(env.verify());
        for tamper in [
            |e: &mut Envelope| e.seq = 2,
            |e: &mut Envelope| e.epoch = 2,
            |e: &mut Envelope| e.ct = b64(b"other"),
            |e: &mut Envelope| e.id = "01J0000000000000000000000B".into(),
            |e: &mut Envelope| e.from = key_id(&SigningKey::from_bytes(&[2; 32])),
        ] {
            let mut bad = env.clone();
            tamper(&mut bad);
            assert!(!bad.verify());
        }
    }

    #[test]
    fn team_ids_bind_the_root_key() {
        let root = key(3);
        let reg = TeamRegistration {
            team: team_id(root.verifying_key().as_bytes(), b"s1"),
            root: key_id(&root),
            salt: b64(b"s1"),
            acl: SignedAcl::signed(&team_id(root.verifying_key().as_bytes(), b"s1"), 1, vec![key_id(&key(4))], &root),
        };
        assert!(reg.verify());
        assert!(is_team_id(&reg.team));
        let mut other_root = reg.clone();
        other_root.root = key_id(&key(5));
        assert!(!other_root.verify(), "another root key does not fit the team id");
        let mut forged_acl = reg.clone();
        forged_acl.acl.devices.push(key_id(&key(6)));
        assert!(!forged_acl.verify(), "an access list change needs the root signature");
    }

    #[test]
    fn blobs_are_signed_over_their_body() {
        let root = key(3);
        let blob = SignedBlob::signed("T", "keys", 2, b"sealed", &root);
        assert!(blob.verify(&key_id(&root)));
        let mut changed = blob.clone();
        changed.body = b64(b"other");
        assert!(!changed.verify(&key_id(&root)));
        let mut rolled = blob.clone();
        rolled.version = 1;
        assert!(!rolled.verify(&key_id(&root)));
    }

    #[test]
    fn request_auth_checks_signature_time_and_body() {
        let device = key(1);
        let header = auth_header(&device, "POST", "/v1/teams/T/envelopes", 1_000_000, b"{}");
        let ok = verify_request(&header, "POST", "/v1/teams/T/envelopes", b"{}", 1_000_500).unwrap();
        assert_eq!(ok.device, key_id(&device));
        assert!(verify_request(&header, "POST", "/v1/teams/T/envelopes", b"{ }", 1_000_500).is_err());
        assert!(verify_request(&header, "GET", "/v1/teams/T/envelopes", b"{}", 1_000_500).is_err());
        assert!(verify_request(&header, "POST", "/v1/teams/U/envelopes", b"{}", 1_000_500).is_err());
        assert!(verify_request(&header, "POST", "/v1/teams/T/envelopes", b"{}", 1_000_000 + AUTH_SKEW_MS + 1).is_err());
    }

    #[test]
    fn base32_is_crockford() {
        assert_eq!(base32(&[0xFF; 5]), "ZZZZZZZZ");
        assert_eq!(base32(&[0; 5]), "00000000");
    }
}
