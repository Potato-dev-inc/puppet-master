//! This install's identity on the link: a node key of its own, kept only in the OS credential store.
//!
//! - The **node key** (Ed25519, `KeyStore::p2p_node`) is the QUIC/TLS identity. It is not the team device signing key:
//!   one key never serves two protocols.
//! - A **binding** `{node_id, device_pub, issued_at}` signed by the team device key (domain prefix `BINDING_PREFIX`)
//!   says "this node belongs to this device". Pairing (DL-6) checks it with `NodeBinding::verify`.
//!
//! Both are read when the endpoint starts (never at app start), live in memory only, and have no `Display`, no
//! `Serialize`, and a `Debug` that prints public halves. Nothing in `p2p/` touches a file.

use super::endpoint::{NodeId, P2pError};
use crate::team::crypto::SigningKey;
use crate::team::keystore::KeyStore;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};

pub const BINDING_PREFIX: &[u8] = b"pm/p2p/node-binding/v1";

/// Proof that `node_id` was made by the device whose public key is `device_pub`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeBinding {
    pub node_id: NodeId,
    pub device_pub: [u8; 32],
    pub issued_at: u64,
    pub signature: [u8; 64],
}

fn message(node_id: &NodeId, device_pub: &[u8; 32], issued_at: u64) -> Vec<u8> {
    let mut m = Vec::with_capacity(BINDING_PREFIX.len() + 72);
    m.extend_from_slice(BINDING_PREFIX);
    m.extend_from_slice(&node_id.0);
    m.extend_from_slice(device_pub);
    m.extend_from_slice(&issued_at.to_be_bytes());
    m
}

impl NodeBinding {
    /// Checks the signature against the device public key inside the record. The caller then compares
    /// `device_pub` with the device it expects.
    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.device_pub) else { return false };
        vk.verify(&message(&self.node_id, &self.device_pub, self.issued_at), &Signature::from_bytes(&self.signature)).is_ok()
    }
}

#[derive(Clone)]
pub struct DeviceIdentity {
    node_key: SigningKey,
    id: NodeId,
    binding: NodeBinding,
}

impl DeviceIdentity {
    /// Reads (or makes, the first time) the node key and the device key through the keystore helpers.
    pub fn load(keys: &KeyStore) -> Result<Self, P2pError> {
        let node_key = keys.p2p_node().map_err(P2pError::Identity)?;
        let device = keys.device_sign().map_err(P2pError::Identity)?;
        let id = NodeId(node_key.verifying_key().to_bytes());
        let device_pub = device.verifying_key().to_bytes();
        let issued_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let signature = device.sign(&message(&id, &device_pub, issued_at)).to_bytes();
        Ok(Self { node_key, id, binding: NodeBinding { node_id: id, device_pub, issued_at, signature } })
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The signed node-to-device record, for pairing.
    pub fn binding(&self) -> NodeBinding {
        self.binding.clone()
    }

    /// For a transport that needs the secret to build its endpoint. Not `pub`: only code in `p2p/` can ask.
    pub(super) fn secret_bytes(&self) -> [u8; 32] {
        self.node_key.to_bytes()
    }
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceIdentity({})", self.id.short())
    }
}
