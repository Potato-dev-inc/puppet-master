//! This install's identity on the link: one node key, kept only in the OS credential store.
//!
//! It is the team layer's device signing key (`team::keystore`, Ed25519): the same key is the QUIC/TLS identity, so a
//! peer's `NodeId` is its public key and cannot be impersonated. The key is read from the store when the endpoint
//! starts (never at app start), lives in memory only, and has no `Display`, no `Serialize`, and a `Debug` that
//! prints the public half. Nothing in `p2p/` touches a file.

use super::endpoint::{NodeId, P2pError};
use crate::team::crypto::SigningKey;
use crate::team::keystore::KeyStore;

#[derive(Clone)]
pub struct DeviceIdentity {
    key: SigningKey,
    id: NodeId,
}

impl DeviceIdentity {
    /// Reads (or makes, the first time) this install's key through the keystore helpers.
    pub fn load(keys: &KeyStore) -> Result<Self, P2pError> {
        let key = keys.device_sign().map_err(P2pError::Identity)?;
        let id = NodeId(key.verifying_key().to_bytes());
        Ok(Self { key, id })
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// For a transport that needs the secret to build its endpoint. Not `pub`: only code in `p2p/` can ask.
    pub(super) fn secret_bytes(&self) -> [u8; 32] {
        self.key.to_bytes()
    }
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceIdentity({})", self.id.short())
    }
}
