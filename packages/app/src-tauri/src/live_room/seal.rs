//! Seals and opens Live Room frames (SPEC-n32 3.6, 3.8). AES-256-GCM, built with `pm_relay_protocol::room`.
//! The content key lives only in the link's #fragment and in app memory. The relay stores what `seal` makes.
//!
//! Plaintext = 4-byte big-endian length + the bytes + zero padding up to a bucket, so the relay sees sizes
//! in steps only. Nonce = 4 random bytes per room (the salt) + an 8-byte counter, so a nonce never repeats
//! within a room. The AAD is `RoomFrame::aad()`: room, seq, kind and nonce are bound to the tag.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use pm_relay_protocol::room::{FrameKind, RoomFrame};
use pm_relay_protocol::{b64, unb64};
use rand::rngs::OsRng;
use rand::RngCore;

/// Bytes of the length prefix inside the padded plaintext.
const LEN_BYTES: usize = 4;
/// Plaintext buckets for delta and state frames (1, 4 or 16 KiB).
const DELTA_BUCKETS: [usize; 3] = [1024, 4 * 1024, 16 * 1024];
/// Plaintext buckets for snapshot frames (16 or 64 KiB).
const SNAP_BUCKETS: [usize; 2] = [16 * 1024, 64 * 1024];

/// Seals the frames of one room. One `Sealer` per room: a new room gets a new key and salt.
/// Holds the key, so it does not derive `Debug`.
pub struct Sealer {
    key: [u8; 32],
    salt: [u8; 4],
    /// The counter the next frame uses. The first frame of a room uses 1.
    counter: u64,
}

impl Sealer {
    /// A fresh random key and salt.
    pub fn new_random() -> Self {
        let mut key = [0u8; 32];
        let mut salt = [0u8; 4];
        OsRng.fill_bytes(&mut key);
        OsRng.fill_bytes(&mut salt);
        Self::from_parts(key, salt)
    }

    /// A sealer with a known key and salt (tests and the shared vector). The counter starts at 1.
    pub fn from_parts(key: [u8; 32], salt: [u8; 4]) -> Self {
        Self { key, salt, counter: 1 }
    }

    /// The content key as base64url (43 chars). Goes into the link's #fragment only.
    pub fn key_b64(&self) -> String {
        b64(&self.key)
    }

    /// Seals one frame of `plaintext` (the frame's JSON bytes). Refuses a plaintext too big for its
    /// bucket, a bad room id or seq 0 (the same checks the relay makes).
    pub fn seal(&mut self, room: &str, seq: u64, kind: FrameKind, plaintext: &[u8]) -> Result<RoomFrame, String> {
        let padded = pad(plaintext, kind)?;
        // The counter moves on before the cipher runs, so a nonce is never used twice, even if a later
        // check refuses the frame.
        if self.counter == u64::MAX {
            return Err("nonce counter used up".into());
        }
        let counter = self.counter;
        self.counter += 1;
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.salt);
        nonce[4..].copy_from_slice(&counter.to_be_bytes());

        let mut frame = RoomFrame {
            v: 1,
            room: room.to_string(),
            seq,
            kind,
            nonce: b64(&nonce),
            ct: String::new(),
        };
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| "bad content key".to_string())?;
        let aad = frame.aad();
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: &padded, aad: &aad })
            .map_err(|_| "seal failed".to_string())?;
        frame.ct = b64(&ct);
        frame.check_shape().map_err(str::to_string)?;
        Ok(frame)
    }
}

/// Length prefix, bytes and zero padding up to the smallest bucket that holds them.
pub fn pad(plain: &[u8], kind: FrameKind) -> Result<Vec<u8>, String> {
    let buckets: &[usize] = match kind {
        FrameKind::Snap => &SNAP_BUCKETS,
        FrameKind::Delta | FrameKind::State => &DELTA_BUCKETS,
    };
    let need = LEN_BYTES + plain.len();
    let size = buckets
        .iter()
        .copied()
        .find(|bucket| *bucket >= need)
        .ok_or_else(|| "frame too large for its bucket".to_string())?;
    let len = u32::try_from(plain.len()).map_err(|_| "frame too large".to_string())?;
    let mut out = vec![0u8; size];
    out[..LEN_BYTES].copy_from_slice(&len.to_be_bytes());
    out[LEN_BYTES..need].copy_from_slice(plain);
    Ok(out)
}

/// Decrypts one frame with the content key and returns its plaintext bytes (padding removed).
/// Fails if the tag, the AAD (room, seq, kind, nonce) or the padding does not check out.
pub fn open(key: &[u8; 32], frame: &RoomFrame) -> Result<Vec<u8>, String> {
    let nonce = unb64(&frame.nonce).ok_or("bad nonce")?;
    // `Nonce::from_slice` panics on a wrong length, so check it here.
    if nonce.len() != 12 {
        return Err("bad nonce length".into());
    }
    let ct = unb64(&frame.ct).ok_or("bad ciphertext")?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "bad content key".to_string())?;
    let aad = frame.aad();
    let padded = cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: &aad })
        .map_err(|_| "frame does not open".to_string())?;
    if padded.len() < LEN_BYTES {
        return Err("short frame".into());
    }
    let mut prefix = [0u8; LEN_BYTES];
    prefix.copy_from_slice(&padded[..LEN_BYTES]);
    let len = u32::from_be_bytes(prefix) as usize;
    if len > padded.len() - LEN_BYTES {
        return Err("bad padding length".into());
    }
    Ok(padded[LEN_BYTES..LEN_BYTES + len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_relay_protocol::room::room_id;
    use std::collections::HashSet;

    const ROOM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaa";
    const KEY: [u8; 32] = [7u8; 32];
    const SALT: [u8; 4] = [1, 2, 3, 4];
    /// The shared vector file, read by the viewer test too (`relay/viewer/test/vector.json`).
    const VECTOR: &str = include_str!("../../../../../relay/viewer/test/vector.json");

    fn sealer() -> Sealer {
        Sealer::from_parts(KEY, SALT)
    }

    #[test]
    fn round_trips_every_kind() {
        for kind in [FrameKind::Snap, FrameKind::Delta, FrameKind::State] {
            let mut s = sealer();
            let frame = s.seal(ROOM, 3, kind, br#"{"v":1,"items":[]}"#).unwrap();
            assert_eq!(frame.kind, kind);
            assert_eq!(open(&KEY, &frame).unwrap(), br#"{"v":1,"items":[]}"#);
        }
    }

    #[test]
    fn an_empty_plaintext_round_trips() {
        let frame = sealer().seal(ROOM, 1, FrameKind::Delta, b"").unwrap();
        assert_eq!(open(&KEY, &frame).unwrap(), b"");
    }

    #[test]
    fn delta_and_state_buckets_are_1_4_and_16_kib() {
        assert_eq!(pad(b"", FrameKind::Delta).unwrap().len(), 1024, "0 bytes -> 1 KiB");
        assert_eq!(pad(&[0u8; 1020], FrameKind::Delta).unwrap().len(), 1024, "1,020 bytes + prefix fit 1 KiB");
        assert_eq!(pad(&[0u8; 1021], FrameKind::Delta).unwrap().len(), 4096, "1,021 bytes -> 4 KiB");
        assert_eq!(pad(&[0u8; 1021], FrameKind::State).unwrap().len(), 4096, "state uses the same buckets");
        assert_eq!(pad(&[0u8; 4 * 1024], FrameKind::Delta).unwrap().len(), 16 * 1024, "4 KiB -> 16 KiB");
        assert_eq!(pad(&[0u8; 16 * 1024 - 4], FrameKind::Delta).unwrap().len(), 16 * 1024, "largest delta");
    }

    #[test]
    fn a_delta_over_16_kib_is_refused() {
        assert!(pad(&[0u8; 16 * 1024 - 3], FrameKind::Delta).is_err());
        assert!(sealer().seal(ROOM, 1, FrameKind::State, &[0u8; 17 * 1024]).is_err());
    }

    #[test]
    fn snapshot_buckets_are_16_and_64_kib() {
        assert_eq!(pad(b"", FrameKind::Snap).unwrap().len(), 16 * 1024);
        assert_eq!(pad(&[0u8; 20 * 1024], FrameKind::Snap).unwrap().len(), 64 * 1024);
        assert!(pad(&[0u8; 64 * 1024], FrameKind::Snap).is_err(), "64 KiB plus prefix is over the top bucket");
    }

    #[test]
    fn the_padding_is_zeros_and_the_length_is_big_endian() {
        let padded = pad(b"ABC", FrameKind::Delta).unwrap();
        assert_eq!(&padded[..4], &[0, 0, 0, 3]);
        assert_eq!(&padded[4..7], b"ABC");
        assert!(padded[7..].iter().all(|b| *b == 0));
    }

    #[test]
    fn changing_seq_kind_or_room_makes_open_fail() {
        let frame = sealer().seal(ROOM, 5, FrameKind::Delta, b"{}").unwrap();
        let mut seq = frame.clone();
        seq.seq = 6;
        let mut kind = frame.clone();
        kind.kind = FrameKind::State;
        let mut room = frame.clone();
        room.room = room_id(&[9u8; 32]);
        for bad in [seq, kind, room] {
            assert!(open(&KEY, &bad).is_err());
        }
        assert!(open(&KEY, &frame).is_ok());
    }

    #[test]
    fn a_wrong_key_or_a_changed_ciphertext_makes_open_fail() {
        let frame = sealer().seal(ROOM, 1, FrameKind::Delta, b"{}").unwrap();
        assert!(open(&[8u8; 32], &frame).is_err());
        let mut bytes = unb64(&frame.ct).unwrap();
        bytes[0] ^= 1;
        let mut tampered = frame.clone();
        tampered.ct = b64(&bytes);
        assert!(open(&KEY, &tampered).is_err());
    }

    #[test]
    fn a_nonce_of_the_wrong_length_is_refused_without_panicking() {
        let mut frame = sealer().seal(ROOM, 1, FrameKind::Delta, b"{}").unwrap();
        frame.nonce = b64(&[0u8; 8]);
        assert_eq!(open(&KEY, &frame), Err("bad nonce length".to_string()));
    }

    #[test]
    fn the_counter_makes_1000_distinct_nonces() {
        let mut s = sealer();
        let mut seen = HashSet::new();
        for seq in 1..=1000u64 {
            let frame = s.seal(ROOM, seq, FrameKind::Delta, b"{}").unwrap();
            assert!(seen.insert(frame.nonce), "nonce repeated at seq {seq}");
        }
        assert_eq!(seen.len(), 1000);
    }

    #[test]
    fn the_first_nonce_is_salt_then_counter_1() {
        let frame = sealer().seal(ROOM, 1, FrameKind::Delta, b"{}").unwrap();
        let expected: [u8; 12] = [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(unb64(&frame.nonce).unwrap(), expected);
    }

    #[test]
    fn key_b64_is_43_base64url_chars_and_new_random_keys_differ() {
        let text = sealer().key_b64();
        assert_eq!(text.len(), 43);
        assert!(text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(unb64(&text).unwrap(), KEY);
        assert_ne!(Sealer::new_random().key_b64(), Sealer::new_random().key_b64());
    }

    #[test]
    fn a_frame_with_a_bad_room_or_seq_zero_is_refused() {
        assert!(sealer().seal("not-a-room", 1, FrameKind::Delta, b"{}").is_err());
        assert!(sealer().seal(ROOM, 0, FrameKind::Delta, b"{}").is_err());
    }

    /// The shared vector (SPEC-n32 section 5): the file the viewer test opens must be exactly what this
    /// sealer makes, and it must open back to its plaintext.
    #[test]
    fn the_shared_vector_file_matches_the_sealer() {
        let v: serde_json::Value = serde_json::from_str(VECTOR).unwrap();
        assert_eq!(v["placeholder"], false);
        assert_eq!(v["keyB64"].as_str().unwrap(), sealer().key_b64());
        assert_eq!(v["room"].as_str().unwrap(), ROOM);
        let plain = serde_json::to_string(&v["plaintext"]).unwrap();
        let frame = sealer().seal(ROOM, 1, FrameKind::Delta, plain.as_bytes()).unwrap();
        assert_eq!(v["seq"].as_u64().unwrap(), frame.seq);
        assert_eq!(v["kind"].as_str().unwrap(), "delta");
        assert_eq!(v["nonce"].as_str().unwrap(), frame.nonce);
        assert_eq!(v["ct"].as_str().unwrap(), frame.ct);
        assert_eq!(open(&KEY, &frame).unwrap(), plain.as_bytes());
    }
}
