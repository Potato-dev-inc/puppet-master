//! Capabilities: the named things a linked computer may do for another ("desk/input", "files", later "agents",
//! "panes", "workspace"). The core knows only the registry: a name, the protocol name it speaks, whether a relay may
//! carry it, and a handler. Each capability is its own permission, per device, and every one starts off.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;

use super::alpn;
use super::endpoint::{Link, NodeId, P2pError};

pub const DESK_INPUT: &str = "desk/input";
pub const DESK_CLIPBOARD: &str = "desk/clipboard";
pub const FILES: &str = "files";
pub const SPACES: &str = "spaces";
/// Pairing is not a permission anyone is given: it is open to strangers only while the person shows a code.
pub const PAIR: &str = "pair";

/// What the person may turn on per computer, in the order the page shows them. Everything starts off.
pub const PERMISSIONABLE: [&str; 3] = [DESK_INPUT, DESK_CLIPBOARD, FILES];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayPolicy {
    /// Direct connections only.
    Never,
    /// A relay may carry it when no direct path exists (the relay sees only encrypted packets).
    Allowed,
}

/// What a capability does with a connection that passed every check. It gets the peer and the link; it never gets
/// the endpoint or the key.
#[async_trait]
pub trait CapabilityHandler: Send + Sync {
    async fn serve(&self, peer: NodeId, link: Arc<dyn Link>);
}

pub struct Capability {
    pub id: String,
    pub alpn: Vec<u8>,
    pub relay: RelayPolicy,
    pub handler: Arc<dyn CapabilityHandler>,
}

impl Capability {
    pub fn new(id: &str, alpn: &[u8], relay: RelayPolicy, handler: Arc<dyn CapabilityHandler>) -> Self {
        Self { id: id.to_string(), alpn: alpn.to_vec(), relay, handler }
    }

    /// The stricter of what the capability asks and what its protocol name allows (`alpn::relay_possible`).
    pub fn relay_allowed(&self) -> bool {
        self.relay == RelayPolicy::Allowed && alpn::relay_possible(&self.alpn)
    }
}

impl std::fmt::Debug for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Capability({} on {})", self.id, String::from_utf8_lossy(&self.alpn))
    }
}

#[derive(Default)]
pub struct Registry {
    by_id: RwLock<HashMap<String, Arc<Capability>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, cap: Capability) -> Result<(), P2pError> {
        if cap.id.is_empty() || !alpn::is_valid(&cap.alpn) {
            return Err(P2pError::BadCapability(format!("{}: bad id or protocol name", cap.id)));
        }
        let mut map = self.by_id.write().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(&cap.id) || map.values().any(|c| c.alpn == cap.alpn) {
            return Err(P2pError::BadCapability(format!("{}: id or protocol name already taken", cap.id)));
        }
        map.insert(cap.id.clone(), Arc::new(cap));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Arc<Capability>> {
        self.by_id.read().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    pub fn by_alpn(&self, alpn: &[u8]) -> Option<Arc<Capability>> {
        self.by_id.read().unwrap_or_else(|p| p.into_inner()).values().find(|c| c.alpn == alpn).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.by_id.read().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect();
        v.sort();
        v
    }
}

/// Who may use which capability on this computer. Default: nobody, nothing. Changed only by the person (the pairing
/// sheet and the device list, DL-6), never by a peer, an agent, or a request that arrives over the link.
pub trait Permissions: Send + Sync {
    fn allowed(&self, peer: &NodeId, capability: &str) -> bool;
    fn set(&self, peer: &NodeId, capability: &str, on: bool);
}

/// Permissions held in memory. DL-6 puts the trust list on disk behind the same trait.
#[derive(Default)]
pub struct MemoryPermissions {
    on: Mutex<HashSet<(NodeId, String)>>,
}

impl MemoryPermissions {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Permissions for MemoryPermissions {
    fn allowed(&self, peer: &NodeId, capability: &str) -> bool {
        self.on.lock().unwrap_or_else(|p| p.into_inner()).contains(&(*peer, capability.to_string()))
    }

    fn set(&self, peer: &NodeId, capability: &str, on: bool) {
        let mut set = self.on.lock().unwrap_or_else(|p| p.into_inner());
        if on {
            set.insert((*peer, capability.to_string()));
        } else {
            set.remove(&(*peer, capability.to_string()));
        }
    }
}
