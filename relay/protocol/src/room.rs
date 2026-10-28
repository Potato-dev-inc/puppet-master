//! Live Room wire types (SPEC-n32 §3.7): a read-only room the owner's app publishes through the
//! relay. The relay stores ciphertext frames for viewers; the content key never reaches it.
//!
//! The link is `https://<relay>/r/#<room_id>.<read_token>.<content_key>`. Only `room_id` and the
//! read token travel to the relay (the token as a hash). The content key lives in the link's
//! `#fragment`, so no secret appears in a request line, a log or a `Referer`.
//!
//! No I/O here: the app (client) and both relay adapters (axum, Cloudflare) share these types.

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

use super::{b64, base32, key_id, sha256, sign, unb64, unb64_32, verify_sig};

/// Protocol name and version of Live Room frames.
pub const ROOM_PROTOCOL: &str = "pm-room/1";
/// Header carrying the read token (`x-pm-room-read`).
pub const ROOM_READ_HEADER: &str = "x-pm-room-read";
/// Header carrying a viewer's random seat id (`x-pm-room-seat`).
pub const ROOM_SEAT_HEADER: &str = "x-pm-room-seat";
/// Largest whole JSON body of a delta or state frame.
pub const MAX_ROOM_FRAME_BYTES: usize = 24 * 1024;
/// Largest whole JSON body of a snapshot frame.
pub const MAX_ROOM_SNAPSHOT_BYTES: usize = 96 * 1024;
/// The longest a room may live (relay hard max: 8 hours).
pub const MAX_ROOM_LIFETIME_MS: u64 = 8 * 60 * 60 * 1000;
/// The most viewers a room may allow.
pub const MAX_ROOM_VIEWERS: u32 = 50;
/// A viewer seat counts as present for this long after its last request.
pub const SEAT_ACTIVE_MS: u64 = 40_000;
/// A stopped room keeps a tombstone (410) for this long.
pub const ROOM_TOMBSTONE_MS: u64 = 60 * 60 * 1000;

/// Lowercase Crockford base32, the alphabet of a room id.
const ROOM_ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// The room id: 26 lowercase base32 chars of SHA-256(`pm-room-v1`, room public key).
pub fn room_id(room_pk: &[u8; 32]) -> String {
    let digest = sha256(&[b"pm-room-v1", room_pk]);
    base32(&digest)[..26].to_ascii_lowercase()
}

/// A plausible room id (shape only): 26 lowercase base32 chars.
pub fn is_room_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| ROOM_ALPHABET.contains(&b))
}

/// The hash of a read token that the relay stores: base64url of SHA-256(`pm-room-read-v1`, token).
pub fn read_hash(token: &[u8]) -> String {
    b64(&sha256(&[b"pm-room-read-v1", token]))
}

/// The owner's signed promise to the relay: this room exists, until when, how many viewers, and
/// which read token (as a hash) opens it. Signed by the room key, so it can only ever name its own room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomRegistration {
    /// Always 1.
    pub v: u8,
    pub room: String,
    /// Room public key, base64url (Ed25519).
    pub key: String,
    pub expires_at_ms: u64,
    pub max_viewers: u32,
    pub read_hash: String,
    /// Ed25519 by the room key over [`RoomRegistration::signing_bytes`], base64url.
    pub sig: String,
}

impl RoomRegistration {
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!(
            "pm-room-reg-v1\n{}\n{}\n{}\n{}\n{}\n{}",
            self.v, self.room, self.key, self.expires_at_ms, self.max_viewers, self.read_hash
        )
        .into_bytes()
    }

    /// A registration signed by `key`, for the room that `key` names.
    pub fn signed(key: &SigningKey, expires_at_ms: u64, max_viewers: u32, read_hash: String) -> Self {
        let mut reg = Self {
            v: 1,
            room: room_id(key.verifying_key().as_bytes()),
            key: key_id(key),
            expires_at_ms,
            max_viewers,
            read_hash,
            sig: String::new(),
        };
        reg.sig = sign(key, &reg.signing_bytes());
        reg
    }

    /// Room id matches the key, the expiry is in the future and at most 8 hours away, the viewer
    /// limit is 1..=50, and the room key signed all of it.
    pub fn verify(&self, now_ms: u64) -> Result<(), &'static str> {
        if self.v != 1 {
            return Err("unsupported room version");
        }
        let key: [u8; 32] = unb64_32(&self.key).ok_or("bad room key")?;
        if self.room != room_id(&key) {
            return Err("room id does not match its key");
        }
        if self.expires_at_ms <= now_ms {
            return Err("room expiry is in the past");
        }
        if self.expires_at_ms > now_ms.saturating_add(MAX_ROOM_LIFETIME_MS) {
            return Err("room lives longer than the relay allows");
        }
        if self.max_viewers == 0 || self.max_viewers > MAX_ROOM_VIEWERS {
            return Err("bad viewer limit");
        }
        if !verify_sig(&self.key, &self.signing_bytes(), &self.sig) {
            return Err("bad registration signature");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameKind {
    Snap,
    Delta,
    State,
}

impl FrameKind {
    /// The wire name, as serde writes it (`snap`, `delta`, `state`).
    pub fn as_str(self) -> &'static str {
        match self {
            FrameKind::Snap => "snap",
            FrameKind::Delta => "delta",
            FrameKind::State => "state",
        }
    }
}

/// One sealed frame. The relay stores and serves it; only the room's viewers, holding the content
/// key from the link, can open `ct`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomFrame {
    /// Always 1.
    pub v: u8,
    pub room: String,
    pub seq: u64,
    pub kind: FrameKind,
    /// AES-256-GCM nonce (12 bytes), base64url.
    pub nonce: String,
    /// Ciphertext (plaintext padded to a bucket, with its GCM tag), base64url.
    pub ct: String,
}

impl RoomFrame {
    /// The AEAD associated data: binds the frame to its room, sequence number, kind and nonce.
    pub fn aad(&self) -> Vec<u8> {
        format!(
            "pm-room-frame-v1\n{}\n{}\n{}\n{}\n{}",
            self.v,
            self.room,
            self.seq,
            self.kind.as_str(),
            self.nonce
        )
        .into_bytes()
    }

    /// Shape checks a relay applies before storing (no key needed). The size limit is on the whole
    /// JSON body, by kind.
    pub fn check_shape(&self) -> Result<(), &'static str> {
        if self.v != 1 {
            return Err("unsupported frame version");
        }
        if !is_room_id(&self.room) {
            return Err("bad room id");
        }
        if self.seq == 0 {
            return Err("bad sequence number");
        }
        if unb64(&self.nonce).map(|n| n.len()) != Some(12) {
            return Err("bad nonce");
        }
        if unb64(&self.ct).is_none() {
            return Err("bad ciphertext");
        }
        let limit = match self.kind {
            FrameKind::Snap => MAX_ROOM_SNAPSHOT_BYTES,
            FrameKind::Delta | FrameKind::State => MAX_ROOM_FRAME_BYTES,
        };
        let size = serde_json::to_vec(self).map_err(|_| "bad frame")?.len();
        if size > limit {
            return Err("frame too large");
        }
        Ok(())
    }
}

/// Why a room has no more frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomEnd {
    Stopped,
    Expired,
}

/// `GET …/frames` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomBatch {
    pub frames: Vec<RoomFrame>,
    /// Pass as `after` next time.
    pub next: u64,
    pub ended: Option<RoomEnd>,
}

/// `GET …/status` reply: two numbers, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomStatus {
    pub viewers: u32,
    pub expires_at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn reg(key: &SigningKey, expires_at_ms: u64, max_viewers: u32) -> RoomRegistration {
        RoomRegistration::signed(key, expires_at_ms, max_viewers, read_hash(b"token-one-16-bytes"))
    }

    fn frame(kind: FrameKind, nonce_len: usize, ct_bytes: usize) -> RoomFrame {
        RoomFrame {
            v: 1,
            room: ROOM.into(),
            seq: 1,
            kind,
            nonce: b64(&vec![7u8; nonce_len]),
            ct: b64(&vec![9u8; ct_bytes]),
        }
    }

    #[test]
    fn room_id_is_26_lowercase_chars_stable_and_per_key() {
        let pk = *key(1).verifying_key().as_bytes();
        let id = room_id(&pk);
        assert_eq!(id.len(), 26);
        assert!(is_room_id(&id));
        assert_eq!(id, room_id(&pk), "same key, same id");
        assert_ne!(id, room_id(key(2).verifying_key().as_bytes()), "another key, another id");
    }

    #[test]
    fn is_room_id_rejects_wrong_length_and_uppercase() {
        assert!(is_room_id(ROOM));
        assert!(!is_room_id(&ROOM[..25]));
        assert!(!is_room_id(&format!("{ROOM}a")));
        assert!(!is_room_id(&ROOM.to_ascii_uppercase()));
        assert!(!is_room_id(&format!("{}I", &ROOM[..25])), "the alphabet has no i");
    }

    #[test]
    fn read_hash_is_a_fixed_size_hash_of_the_token() {
        let hash = read_hash(b"token-one-16-bytes");
        assert_eq!(hash.len(), 43, "32 bytes, base64url without padding");
        assert_eq!(hash, read_hash(b"token-one-16-bytes"));
        assert_ne!(hash, read_hash(b"token-two-16-bytes"));
    }

    #[test]
    fn a_good_registration_verifies() {
        let r = reg(&key(1), 10_000_000, 10);
        assert_eq!(r.room, room_id(key(1).verifying_key().as_bytes()));
        assert!(r.verify(10_000_000 - 60_000).is_ok());
    }

    #[test]
    fn a_registration_for_another_room_fails() {
        let mut r = reg(&key(1), 10_000_000, 10);
        r.room = room_id(key(2).verifying_key().as_bytes());
        assert_eq!(r.verify(0), Err("room id does not match its key"));
    }

    #[test]
    fn an_expiry_in_the_past_fails() {
        let r = reg(&key(1), 1_000, 10);
        assert_eq!(r.verify(1_000), Err("room expiry is in the past"));
        assert_eq!(r.verify(2_000), Err("room expiry is in the past"));
    }

    #[test]
    fn an_expiry_more_than_eight_hours_away_fails() {
        let now = 1_000_000;
        let ok = reg(&key(1), now + MAX_ROOM_LIFETIME_MS, 10);
        assert!(ok.verify(now).is_ok(), "exactly 8 hours is allowed");
        let too_long = reg(&key(1), now + MAX_ROOM_LIFETIME_MS + 1, 10);
        assert_eq!(too_long.verify(now), Err("room lives longer than the relay allows"));
    }

    #[test]
    fn viewer_limit_must_be_1_to_50() {
        assert_eq!(reg(&key(1), 10_000_000, 0).verify(0), Err("bad viewer limit"));
        assert_eq!(reg(&key(1), 10_000_000, 51).verify(0), Err("bad viewer limit"));
        assert!(reg(&key(1), 10_000_000, 1).verify(0).is_ok());
        assert!(reg(&key(1), 10_000_000, 50).verify(0).is_ok());
    }

    #[test]
    fn a_tampered_field_breaks_the_signature() {
        let good = reg(&key(1), 10_000_000, 10);
        let tamper: [fn(&mut RoomRegistration); 3] = [
            |r| r.expires_at_ms += 1,
            |r| r.max_viewers = 11,
            |r| r.read_hash = read_hash(b"another-token-here"),
        ];
        for change in tamper {
            let mut bad = good.clone();
            change(&mut bad);
            assert_eq!(bad.verify(0), Err("bad registration signature"));
        }
    }

    #[test]
    fn a_bad_room_key_is_refused() {
        let mut r = reg(&key(1), 10_000_000, 10);
        r.key = "not-a-key".into();
        assert_eq!(r.verify(0), Err("bad room key"));
    }

    #[test]
    fn frame_nonce_must_be_12_bytes() {
        assert!(frame(FrameKind::Delta, 12, 16).check_shape().is_ok());
        assert_eq!(frame(FrameKind::Delta, 11, 16).check_shape(), Err("bad nonce"));
        assert_eq!(frame(FrameKind::Delta, 13, 16).check_shape(), Err("bad nonce"));
    }

    #[test]
    fn frame_ciphertext_must_be_base64url() {
        let mut f = frame(FrameKind::Delta, 12, 16);
        f.ct = "not base64!".into();
        assert_eq!(f.check_shape(), Err("bad ciphertext"));
    }

    #[test]
    fn frame_sequence_starts_at_one() {
        let mut f = frame(FrameKind::Delta, 12, 16);
        f.seq = 0;
        assert_eq!(f.check_shape(), Err("bad sequence number"));
    }

    #[test]
    fn frame_size_limit_depends_on_kind() {
        // 16 KiB of ciphertext (the largest delta bucket) fits a delta or state frame.
        assert!(frame(FrameKind::Delta, 12, 16 * 1024).check_shape().is_ok());
        assert!(frame(FrameKind::State, 12, 16 * 1024).check_shape().is_ok());
        // 20 KiB is too big for a delta but fine for a snapshot.
        assert_eq!(frame(FrameKind::Delta, 12, 20 * 1024).check_shape(), Err("frame too large"));
        assert_eq!(frame(FrameKind::State, 12, 20 * 1024).check_shape(), Err("frame too large"));
        assert!(frame(FrameKind::Snap, 12, 20 * 1024).check_shape().is_ok());
        // 64 KiB (the largest snapshot bucket) fits; 80 KiB does not.
        assert!(frame(FrameKind::Snap, 12, 64 * 1024).check_shape().is_ok());
        assert_eq!(frame(FrameKind::Snap, 12, 80 * 1024).check_shape(), Err("frame too large"));
    }

    #[test]
    fn aad_is_the_exact_bytes() {
        let f = frame(FrameKind::Delta, 12, 16);
        let nonce = b64(&[7u8; 12]);
        assert_eq!(
            f.aad(),
            format!("pm-room-frame-v1\n1\n{ROOM}\n1\ndelta\n{nonce}").into_bytes()
        );
    }

    #[test]
    fn aad_changes_with_kind_and_seq() {
        let mut f = frame(FrameKind::Delta, 12, 16);
        let before = f.aad();
        f.kind = FrameKind::State;
        assert_ne!(f.aad(), before);
        f.kind = FrameKind::Delta;
        f.seq = 2;
        assert_ne!(f.aad(), before);
    }

    #[test]
    fn frame_kind_is_lowercase_on_the_wire() {
        assert_eq!(serde_json::to_string(&FrameKind::Snap).unwrap(), "\"snap\"");
        assert_eq!(serde_json::to_string(&FrameKind::Delta).unwrap(), "\"delta\"");
        assert_eq!(serde_json::to_string(&FrameKind::State).unwrap(), "\"state\"");
        let f: FrameKind = serde_json::from_str("\"snap\"").unwrap();
        assert_eq!(f, FrameKind::Snap);
        let f = frame(FrameKind::Snap, 12, 16);
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"kind\":\"snap\""));
        assert_eq!(serde_json::from_str::<RoomFrame>(&json).unwrap(), f);
    }

    #[test]
    fn room_end_is_lowercase() {
        assert_eq!(serde_json::to_string(&RoomEnd::Stopped).unwrap(), "\"stopped\"");
        assert_eq!(serde_json::to_string(&RoomEnd::Expired).unwrap(), "\"expired\"");
        let end: RoomEnd = serde_json::from_str("\"expired\"").unwrap();
        assert_eq!(end, RoomEnd::Expired);
    }

    #[test]
    fn room_batch_is_camel_case_and_round_trips() {
        let batch = RoomBatch {
            frames: vec![frame(FrameKind::Delta, 12, 16)],
            next: 4,
            ended: Some(RoomEnd::Stopped),
        };
        let json = serde_json::to_string(&batch).unwrap();
        assert!(json.contains("\"frames\":["));
        assert!(json.contains("\"next\":4"));
        assert!(json.contains("\"ended\":\"stopped\""));
        assert_eq!(serde_json::from_str::<RoomBatch>(&json).unwrap(), batch);
        let open = RoomBatch { frames: vec![], next: 0, ended: None };
        let json = serde_json::to_string(&open).unwrap();
        assert_eq!(serde_json::from_str::<RoomBatch>(&json).unwrap(), open);
    }

    #[test]
    fn room_status_is_camel_case_and_round_trips() {
        let status = RoomStatus { viewers: 3, expires_at_ms: 1_761_700_000_000 };
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "{\"viewers\":3,\"expiresAtMs\":1761700000000}");
        assert_eq!(serde_json::from_str::<RoomStatus>(&json).unwrap(), status);
    }

    #[test]
    fn registration_round_trips_through_json() {
        let r = reg(&key(1), 10_000_000, 10);
        let json = serde_json::to_string(&r).unwrap();
        let back: RoomRegistration = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        assert!(back.verify(0).is_ok());
    }
}
