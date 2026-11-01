//! The trust list: which computers this one has been paired with, and what each may do. It implements
//! `Permissions`, so the link layer's admission check reads it directly.
//!
//! - **No secrets.** An entry holds the peer's node id, its device public key, a name, the capabilities the person
//!   turned on, and when it was added. Public halves only.
//! - **Signed.** The whole list is signed with this device's key (`TrustSigner`). A list that was edited outside the
//!   app, copied from another computer, or cut short fails the check on load: it is ignored and `tampered()` says so.
//! - **No file here.** Bytes go through `TrustStorage`; the app's file lives in `pairing_commands.rs`, tests use memory.
//! - **Everything off.** A new entry has no capability. Only the person turns one on (`set`), never a peer.
//! - Pairing itself is not a permission: `allowed(_, "pair")` is true only while the person shows a code.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde_json::{json, Value};

use super::capability::{self, Permissions};
use super::endpoint::NodeId;
use crate::team::crypto::SigningKey;
use crate::team::keystore::KeyStore;

const LIST_PREFIX: &[u8] = b"pm/p2p/trust-list/v1";
const NAME_MAX: usize = 40;

/// Where the signed bytes live.
pub trait TrustStorage: Send + Sync {
    fn load(&self) -> Option<Vec<u8>>;
    fn save(&self, bytes: &[u8]) -> Result<(), String>;
}

/// Signs the list with this device's key.
pub trait TrustSigner: Send + Sync {
    fn device_pub(&self) -> Result<[u8; 32], String>;
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], String>;
}

/// Reads the device key from the keystore the first time it is needed, never at app start.
pub struct KeyStoreSigner {
    keys: KeyStore,
    key: OnceLock<SigningKey>,
}

impl KeyStoreSigner {
    pub fn new(keys: KeyStore) -> Self {
        Self { keys, key: OnceLock::new() }
    }

    fn key(&self) -> Result<&SigningKey, String> {
        if let Some(k) = self.key.get() {
            return Ok(k);
        }
        let k = self.keys.device_sign()?;
        Ok(self.key.get_or_init(|| k))
    }
}

impl TrustSigner for KeyStoreSigner {
    fn device_pub(&self) -> Result<[u8; 32], String> {
        Ok(self.key()?.verifying_key().to_bytes())
    }

    fn sign(&self, message: &[u8]) -> Result<[u8; 64], String> {
        Ok(self.key()?.sign(message).to_bytes())
    }
}

#[derive(Default)]
pub struct MemoryTrustStorage {
    bytes: Mutex<Option<Vec<u8>>>,
}

impl MemoryTrustStorage {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Replace the stored bytes (a test playing the person who edits the file).
    pub fn put(&self, bytes: Vec<u8>) {
        *self.bytes.lock().unwrap_or_else(|p| p.into_inner()) = Some(bytes);
    }
}

impl TrustStorage for MemoryTrustStorage {
    fn load(&self) -> Option<Vec<u8>> {
        self.bytes.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn save(&self, bytes: &[u8]) -> Result<(), String> {
        self.put(bytes.to_vec());
        Ok(())
    }
}

/// One paired computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedDevice {
    pub node: NodeId,
    pub device_pub: [u8; 32],
    pub name: String,
    pub added_at: u64,
    pub allowed: BTreeSet<String>,
}

pub fn clean_name(raw: &str) -> String {
    let s: String = raw.chars().filter(|c| !c.is_control()).collect();
    let s = s.trim();
    let s: String = s.chars().take(NAME_MAX).collect();
    if s.is_empty() {
        "Another computer".to_string()
    } else {
        s
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// The bytes that are signed: a fixed layout, independent of JSON spacing or key order.
fn signed_message(owner: &[u8; 32], entries: &[TrustedDevice]) -> Vec<u8> {
    let mut sorted: Vec<&TrustedDevice> = entries.iter().collect();
    sorted.sort_by_key(|e| e.node);
    let mut m = Vec::new();
    m.extend_from_slice(LIST_PREFIX);
    m.extend_from_slice(owner);
    m.extend_from_slice(&(sorted.len() as u32).to_be_bytes());
    for e in sorted {
        m.extend_from_slice(&e.node.0);
        m.extend_from_slice(&e.device_pub);
        m.extend_from_slice(&e.added_at.to_be_bytes());
        m.extend_from_slice(&(e.name.len() as u32).to_be_bytes());
        m.extend_from_slice(e.name.as_bytes());
        m.extend_from_slice(&(e.allowed.len() as u32).to_be_bytes());
        for c in &e.allowed {
            m.extend_from_slice(&(c.len() as u32).to_be_bytes());
            m.extend_from_slice(c.as_bytes());
        }
    }
    m
}

fn encode(owner: &[u8; 32], entries: &[TrustedDevice], sig: &[u8; 64]) -> Vec<u8> {
    let items: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "node": e.node.to_hex(),
                "device": hex(&e.device_pub),
                "name": e.name,
                "added_at": e.added_at,
                "allowed": e.allowed.iter().collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "v": 1, "owner": hex(owner), "devices": items, "sig": hex(sig) }).to_string().into_bytes()
}

fn decode(bytes: &[u8]) -> Option<([u8; 32], Vec<TrustedDevice>, [u8; 64])> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    if v.get("v")?.as_u64()? != 1 {
        return None;
    }
    let owner = unhex::<32>(v.get("owner")?.as_str()?)?;
    let sig = unhex::<64>(v.get("sig")?.as_str()?)?;
    let mut out = Vec::new();
    for d in v.get("devices")?.as_array()? {
        let allowed = d.get("allowed")?.as_array()?.iter().map(|c| c.as_str().map(str::to_string)).collect::<Option<BTreeSet<_>>>()?;
        out.push(TrustedDevice {
            node: NodeId(unhex::<32>(d.get("node")?.as_str()?)?),
            device_pub: unhex::<32>(d.get("device")?.as_str()?)?,
            name: d.get("name")?.as_str()?.to_string(),
            added_at: d.get("added_at")?.as_u64()?,
            allowed,
        });
    }
    Some((owner, out, sig))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrustError {
    #[error("not a paired computer")]
    Unknown,
    #[error("that cannot be turned on")]
    NotPermissionable,
    #[error("could not save the list: {0}")]
    Save(String),
}

pub struct TrustList {
    storage: Arc<dyn TrustStorage>,
    signer: Arc<dyn TrustSigner>,
    entries: Mutex<Vec<TrustedDevice>>,
    tampered: AtomicBool,
    pairing_open: AtomicBool,
}

impl TrustList {
    /// Reads and checks the stored list. A list that fails the check (wrong signature, another device's list,
    /// garbage) is ignored: the list starts empty and `tampered()` is true until the next change is saved.
    pub fn open(storage: Arc<dyn TrustStorage>, signer: Arc<dyn TrustSigner>) -> Arc<Self> {
        let mut entries = Vec::new();
        let mut tampered = false;
        if let Some(bytes) = storage.load() {
            match Self::verified(&bytes, signer.as_ref()) {
                Some(list) => entries = list,
                None => tampered = true,
            }
        }
        Arc::new(Self {
            storage,
            signer,
            entries: Mutex::new(entries),
            tampered: AtomicBool::new(tampered),
            pairing_open: AtomicBool::new(false),
        })
    }

    fn verified(bytes: &[u8], signer: &dyn TrustSigner) -> Option<Vec<TrustedDevice>> {
        let (owner, entries, sig) = decode(bytes)?;
        if owner != signer.device_pub().ok()? {
            return None;
        }
        let vk = VerifyingKey::from_bytes(&owner).ok()?;
        vk.verify(&signed_message(&owner, &entries), &Signature::from_bytes(&sig)).ok()?;
        Some(entries)
    }

    pub fn tampered(&self) -> bool {
        self.tampered.load(Ordering::SeqCst)
    }

    pub fn list(&self) -> Vec<TrustedDevice> {
        let mut v = self.entries.lock().unwrap_or_else(|p| p.into_inner()).clone();
        v.sort_by(|a, b| a.added_at.cmp(&b.added_at).then(a.node.cmp(&b.node)));
        v
    }

    pub fn get(&self, node: &NodeId) -> Option<TrustedDevice> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|e| e.node == *node).cloned()
    }

    pub fn is_trusted(&self, node: &NodeId) -> bool {
        self.get(node).is_some()
    }

    /// Opened and closed by the pairing manager: strangers may connect to the pairing protocol only while a code
    /// is on screen.
    pub fn set_pairing_open(&self, open: bool) {
        self.pairing_open.store(open, Ordering::SeqCst);
    }

    pub fn pairing_open(&self) -> bool {
        self.pairing_open.load(Ordering::SeqCst)
    }

    /// Adds (or refreshes the name of) a computer both people confirmed. A new entry has nothing turned on. The
    /// same node with another device key is refused: that is not the computer that was paired.
    pub fn add(&self, node: NodeId, device_pub: [u8; 32], name: &str, now_secs: u64) -> Result<(), TrustError> {
        self.change(|entries| {
            if let Some(e) = entries.iter_mut().find(|e| e.node == node) {
                if e.device_pub != device_pub {
                    return Err(TrustError::Unknown);
                }
                e.name = clean_name(name);
            } else {
                entries.push(TrustedDevice { node, device_pub, name: clean_name(name), added_at: now_secs, allowed: BTreeSet::new() });
            }
            Ok(())
        })
    }

    pub fn remove(&self, node: &NodeId) -> Result<(), TrustError> {
        self.change(|entries| {
            let before = entries.len();
            entries.retain(|e| e.node != *node);
            if entries.len() == before {
                Err(TrustError::Unknown)
            } else {
                Ok(())
            }
        })
    }

    pub fn set_permission(&self, node: &NodeId, capability: &str, on: bool) -> Result<(), TrustError> {
        if !capability::PERMISSIONABLE.contains(&capability) {
            return Err(TrustError::NotPermissionable);
        }
        self.change(|entries| {
            let e = entries.iter_mut().find(|e| e.node == *node).ok_or(TrustError::Unknown)?;
            if on {
                e.allowed.insert(capability.to_string());
            } else {
                e.allowed.remove(capability);
            }
            Ok(())
        })
    }

    /// Applies a change to a copy, signs and saves it, and only then keeps it: a failed save changes nothing.
    fn change(&self, f: impl FnOnce(&mut Vec<TrustedDevice>) -> Result<(), TrustError>) -> Result<(), TrustError> {
        let mut guard = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = guard.clone();
        f(&mut next)?;
        let owner = self.signer.device_pub().map_err(TrustError::Save)?;
        let sig = self.signer.sign(&signed_message(&owner, &next)).map_err(TrustError::Save)?;
        self.storage.save(&encode(&owner, &next, &sig)).map_err(TrustError::Save)?;
        *guard = next;
        self.tampered.store(false, Ordering::SeqCst);
        Ok(())
    }
}

impl Permissions for TrustList {
    fn allowed(&self, peer: &NodeId, capability: &str) -> bool {
        if capability == capability::PAIR {
            return self.pairing_open();
        }
        self.entries.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|e| e.node == *peer && e.allowed.contains(capability))
    }

    /// The person's switch. Unknown computers and capabilities that are not switchable are ignored.
    fn set(&self, peer: &NodeId, capability: &str, on: bool) {
        let _ = self.set_permission(peer, capability, on);
    }
}
