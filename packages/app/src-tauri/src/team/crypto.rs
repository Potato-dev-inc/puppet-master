//! The cryptography of Team, in one place (COWORK-PLAN §8.2). Only audited, permissively licensed
//! crates, no home-made primitives:
//! - **Signatures:** Ed25519, `ed25519-dalek` 2 (BSD-3-Clause), strict verification. Member keys,
//!   device keys and the team root (admin) key are Ed25519.
//! - **Envelope encryption:** XChaCha20-Poly1305, `chacha20poly1305` 0.10 (RustCrypto, Apache-2.0 OR
//!   MIT), with a random 24-byte nonce and the envelope header as associated data.
//! - **Sealing a key or a join request to one device:** HPKE (RFC 9180) base mode,
//!   DHKEM(X25519, HKDF-SHA256) + HKDF-SHA256 + ChaCha20-Poly1305, `hpke` 0.12 (MIT OR Apache-2.0).
//! - **Invite code proof:** HMAC-SHA256 (`hmac` 0.12, RustCrypto), so the relay never sees the code.
//!
//! Padding: an envelope's plaintext is padded to 1, 4 or 16 KiB, so its size says little.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hpke::aead::ChaCha20Poly1305 as HpkeChaCha;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as _, OpModeR, OpModeS, Serializable};
use rand::RngCore;

pub use ed25519_dalek::SigningKey;
pub use pm_relay_protocol::{b64, key_id, sign, unb64, unb64_32, verify_sig};

/// Plaintext size buckets of a sealed envelope.
pub const PAD_BUCKETS: [usize; 3] = [1024, 4096, 16384];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptoError(pub &'static str);

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut out);
    out
}

pub fn new_signing_key() -> SigningKey {
    SigningKey::from_bytes(&random_bytes::<32>())
}

// ------------------------------------------------------------------------------ device KEM keys

/// A device's X25519 secret for HPKE (32 bytes). Its public half is published in the roster.
#[derive(Clone)]
pub struct KemSecret([u8; 32]);

impl KemSecret {
    pub fn generate() -> Self {
        let (sk, _) = X25519HkdfSha256::gen_keypair(&mut rand::rngs::OsRng);
        let bytes: [u8; 32] = sk.to_bytes().into();
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// base64url of the public key.
    pub fn public_b64(&self) -> String {
        let sk = <X25519HkdfSha256 as hpke::Kem>::PrivateKey::from_bytes(&self.0).expect("32-byte X25519 key");
        let pk = X25519HkdfSha256::sk_to_pk(&sk);
        b64(&pk.to_bytes())
    }
}

impl std::fmt::Debug for KemSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KemSecret(…)")
    }
}

/// HPKE-seal `plaintext` to the device whose X25519 public key is `recipient_b64`. Returns the
/// encapsulated key and the ciphertext.
pub fn hpke_seal(recipient_b64: &str, info: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    let pk_bytes = unb64_32(recipient_b64).ok_or(CryptoError("bad recipient key"))?;
    let pk = <X25519HkdfSha256 as hpke::Kem>::PublicKey::from_bytes(&pk_bytes).map_err(|_| CryptoError("bad recipient key"))?;
    let (enc, ct) = hpke::single_shot_seal::<HpkeChaCha, HkdfSha256, X25519HkdfSha256, _>(
        &OpModeS::Base,
        &pk,
        info,
        plaintext,
        b"",
        &mut rand::rngs::OsRng,
    )
    .map_err(|_| CryptoError("could not seal"))?;
    Ok((enc.to_bytes().to_vec(), ct))
}

/// Open what [`hpke_seal`] sealed to this device.
pub fn hpke_open(secret: &KemSecret, enc: &[u8], info: &[u8], ct: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let sk = <X25519HkdfSha256 as hpke::Kem>::PrivateKey::from_bytes(&secret.0).map_err(|_| CryptoError("bad key"))?;
    let enc = <X25519HkdfSha256 as hpke::Kem>::EncappedKey::from_bytes(enc).map_err(|_| CryptoError("bad sealed key"))?;
    hpke::single_shot_open::<HpkeChaCha, HkdfSha256, X25519HkdfSha256>(&OpModeR::Base, &sk, &enc, info, ct, b"")
        .map_err(|_| CryptoError("could not open: not sealed to this device, or changed"))
}

// ------------------------------------------------------------------------------------ the AEAD

fn pad(plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let needed = plaintext.len() + 4;
    let bucket = PAD_BUCKETS.iter().copied().find(|b| *b >= needed).ok_or(CryptoError("too large to send"))?;
    let mut out = Vec::with_capacity(bucket);
    out.extend_from_slice(&(plaintext.len() as u32).to_be_bytes());
    out.extend_from_slice(plaintext);
    out.resize(bucket, 0);
    Ok(out)
}

fn unpad(padded: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if padded.len() < 4 {
        return Err(CryptoError("bad padding"));
    }
    let len = u32::from_be_bytes([padded[0], padded[1], padded[2], padded[3]]) as usize;
    if len > padded.len() - 4 {
        return Err(CryptoError("bad padding"));
    }
    Ok(padded[4..4 + len].to_vec())
}

/// Encrypt with a team epoch key: XChaCha20-Poly1305, padded, `header` as associated data, with a
/// nonce the caller picked (an envelope's header carries its own nonce).
pub fn seal_with_key_and_nonce(key: &[u8; 32], header: &[u8], plaintext: &[u8], nonce: [u8; 24]) -> Result<Vec<u8>, CryptoError> {
    XChaCha20Poly1305::new(key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: &pad(plaintext)?, aad: header })
        .map_err(|_| CryptoError("could not encrypt"))
}

/// [`seal_with_key_and_nonce`] with a random nonce.
#[cfg_attr(not(test), allow(dead_code))]
pub fn seal_with_key(key: &[u8; 32], header: &[u8], plaintext: &[u8]) -> Result<([u8; 24], Vec<u8>), CryptoError> {
    let nonce = random_bytes::<24>();
    Ok((nonce, seal_with_key_and_nonce(key, header, plaintext, nonce)?))
}

fn decrypt(key: &[u8; 32], nonce: &[u8], header: &[u8], ct: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if nonce.len() != 24 {
        return Err(CryptoError("bad nonce"));
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: header })
        .map_err(|_| CryptoError("could not decrypt: wrong key, or the message was changed"))
}

/// Open what [`seal_with_key`] sealed.
pub fn open_with_key(key: &[u8; 32], nonce: &[u8], header: &[u8], ct: &[u8]) -> Result<Vec<u8>, CryptoError> {
    unpad(&decrypt(key, nonce, header, ct)?)
}

/// No padding, no size cap: for the roster blob, which can outgrow an envelope.
pub fn seal_unpadded(key: &[u8; 32], header: &[u8], plaintext: &[u8]) -> Result<([u8; 24], Vec<u8>), CryptoError> {
    let nonce = random_bytes::<24>();
    let ct = XChaCha20Poly1305::new(key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad: header })
        .map_err(|_| CryptoError("could not encrypt"))?;
    Ok((nonce, ct))
}

pub fn open_unpadded(key: &[u8; 32], nonce: &[u8], header: &[u8], ct: &[u8]) -> Result<Vec<u8>, CryptoError> {
    decrypt(key, nonce, header, ct)
}

// ------------------------------------------------------------------------------------- HMAC

/// HMAC-SHA256 of `message` under `key`.
pub fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// Constant-time equality for two MACs.
pub fn same_mac(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_key_round_trip_and_tamper() {
        let key = random_bytes::<32>();
        let (nonce, ct) = seal_with_key(&key, b"header", b"hello team").unwrap();
        assert_eq!(open_with_key(&key, &nonce, b"header", &ct).unwrap(), b"hello team");
        assert!(open_with_key(&key, &nonce, b"other header", &ct).is_err(), "header is bound");
        let mut bad = ct.clone();
        bad[3] ^= 1;
        assert!(open_with_key(&key, &nonce, b"header", &bad).is_err(), "ciphertext is authenticated");
        assert!(open_with_key(&random_bytes::<32>(), &nonce, b"header", &ct).is_err(), "another key fails");
    }

    #[test]
    fn plaintext_is_padded_to_buckets() {
        let key = random_bytes::<32>();
        let (_, small) = seal_with_key(&key, b"", b"x").unwrap();
        let (_, also_small) = seal_with_key(&key, b"", &[7u8; 900]).unwrap();
        let (_, medium) = seal_with_key(&key, b"", &[7u8; 2000]).unwrap();
        assert_eq!(small.len(), also_small.len(), "sizes inside a bucket look the same");
        assert_eq!(small.len(), 1024 + 16);
        assert_eq!(medium.len(), 4096 + 16);
        assert!(seal_with_key(&key, b"", &[0u8; 16384]).is_err(), "over the largest bucket is refused");
    }

    #[test]
    fn hpke_seals_to_one_device_only() {
        let leo = KemSecret::generate();
        let sam = KemSecret::generate();
        let (enc, ct) = hpke_seal(&leo.public_b64(), b"info", b"epoch key").unwrap();
        assert_eq!(hpke_open(&leo, &enc, b"info", &ct).unwrap(), b"epoch key");
        assert!(hpke_open(&sam, &enc, b"info", &ct).is_err());
        assert!(hpke_open(&leo, &enc, b"other info", &ct).is_err(), "the context is bound");
    }

    #[test]
    fn hmac_compares_in_constant_time() {
        let a = hmac(b"CODE1234", b"msg");
        assert!(same_mac(&a, &hmac(b"CODE1234", b"msg")));
        assert!(!same_mac(&a, &hmac(b"CODE1235", b"msg")));
    }
}
