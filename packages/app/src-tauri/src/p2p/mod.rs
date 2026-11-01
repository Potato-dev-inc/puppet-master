//! The PC link: one peer-to-peer layer for everything that talks between Puppet Master installs. My computers
//! (DevHop) is its first client; Spaces (teams, voice, shared folders) and later "agents", "panes", "workspace"
//! are others. DL-5 in PLAN-R17-DESKTOPLAB, D-0 in PLAN-R17-TEAM-SOCIAL.
//!
//! - **One endpoint per app**, started only when a capability wants it. Nothing here runs at app start: no socket,
//!   no key read, no network. Switching Labs off closes it.
//! - **Identity**: the device key from the OS credential store is the node key (`identity.rs`). No file, no log, no
//!   command line, no webview ever holds it.
//! - **Capabilities** (`capability.rs`): a name, a protocol name (`alpn.rs`), a relay policy and a handler. Each is its
//!   own permission per device, off until the person turns it on. A connection for a capability that device may not
//!   use is closed before any handler sees it. Anything under `pm/desk` is direct-only: a relayed path is refused,
//!   and a link that becomes relayed is closed.
//! - **Two networks behind `Transport`**: iroh (`iroh_net.rs`) and an in-memory one for tests (`memory.rs`).
//! - **Not reachable by agents**: nothing in the bridge, the MCP server, the tool registry or the phone routes may
//!   mention this module (`guard_tests.rs`).
//!
//! Not here (on purpose): pairing and the trust list (DL-6), the desk link and input forwarding (DL-7), and anything
//! that knows what a capability does.

// Nothing calls this yet: DL-6 (pairing) and DL-7 (desk link) are its first users.
#![allow(dead_code)]

pub mod alpn;
pub mod capability;
pub mod endpoint;
pub mod identity;
pub mod iroh_net;
#[cfg(test)]
pub mod memory;

#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::task::JoinHandle;

use self::capability::{Capability, Permissions, Registry};
use self::endpoint::{
    accept_loop, check_dialed, BindSpec, Link, NodeId, P2pError, PeerAddr, Refusal, RefusalLog, Transport,
    TransportFactory,
};
use self::identity::DeviceIdentity;
use crate::team::keystore::KeyStore;

/// The Labs switch (for now the same one as My computers).
pub const LABS_KEY: &str = "desk_enabled";

pub fn labs_on(public: &Value) -> bool {
    public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false)
}

struct Running {
    transport: Arc<dyn Transport>,
    spec: BindSpec,
    task: JoinHandle<()>,
}

pub struct P2p {
    labs: Arc<AtomicBool>,
    keys: KeyStore,
    factory: Arc<dyn TransportFactory>,
    registry: Arc<Registry>,
    perms: Arc<dyn Permissions>,
    refusals: RefusalLog,
    loopback_only: bool,
    wanted: Mutex<BTreeSet<String>>,
    /// Serializes start / restart, which await.
    start_lock: tokio::sync::Mutex<()>,
    running: Mutex<Option<Running>>,
}

impl P2p {
    /// Builds the service. Starts nothing, reads no key, opens no socket.
    pub fn new(keys: KeyStore, factory: Arc<dyn TransportFactory>, registry: Arc<Registry>, perms: Arc<dyn Permissions>) -> Self {
        Self {
            labs: Arc::new(AtomicBool::new(false)),
            keys,
            factory,
            registry,
            perms,
            refusals: Arc::new(Mutex::new(Vec::new())),
            loopback_only: false,
            wanted: Mutex::new(BTreeSet::new()),
            start_lock: tokio::sync::Mutex::new(()),
            running: Mutex::new(None),
        }
    }

    /// Bind to loopback only.
    pub fn loopback_only(mut self, on: bool) -> Self {
        self.loopback_only = on;
        self
    }

    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    pub fn permissions(&self) -> &Arc<dyn Permissions> {
        &self.perms
    }

    pub fn labs(&self) -> bool {
        self.labs.load(Ordering::SeqCst)
    }

    /// Called when settings change. Turning Labs off is the kill switch: the endpoint closes and nothing is wanted.
    pub fn apply_settings(&self, public: &Value) {
        let on = labs_on(public);
        self.labs.store(on, Ordering::SeqCst);
        if !on {
            self.stop();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).is_some()
    }

    pub fn node_id(&self) -> Option<NodeId> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|r| r.transport.id())
    }

    /// The spec the running endpoint was bound with.
    pub fn bound_spec(&self) -> Option<BindSpec> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|r| r.spec.clone())
    }

    pub fn refusals(&self) -> Vec<Refusal> {
        self.refusals.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// A capability needs the network: start the endpoint (or widen it) and listen for it.
    pub async fn want(&self, capability: &str) -> Result<Arc<dyn Transport>, P2pError> {
        if !self.labs() {
            return Err(P2pError::Off);
        }
        if self.registry.get(capability).is_none() {
            return Err(P2pError::UnknownCapability(capability.to_string()));
        }
        let _guard = self.start_lock.lock().await;
        self.wanted.lock().unwrap_or_else(|p| p.into_inner()).insert(capability.to_string());
        self.reconcile().await
    }

    /// A capability no longer needs the network. When none does, the endpoint closes.
    pub async fn unwant(&self, capability: &str) {
        let _guard = self.start_lock.lock().await;
        self.wanted.lock().unwrap_or_else(|p| p.into_inner()).remove(capability);
        let _ = self.reconcile().await;
    }

    /// Connects to a device for one capability. Starts the endpoint when it is not running.
    pub async fn dial(&self, peer: &PeerAddr, capability: &str) -> Result<Arc<dyn Link>, P2pError> {
        let transport = self.want(capability).await?;
        let cap = self.registry.get(capability).ok_or_else(|| P2pError::UnknownCapability(capability.to_string()))?;
        let link = transport.dial(peer, &cap.alpn).await?;
        check_dialed(&link, &cap)?;
        Ok(link)
    }

    /// Closes the endpoint and forgets what was wanted. Safe to call from settings code.
    pub fn stop(&self) {
        self.wanted.lock().unwrap_or_else(|p| p.into_inner()).clear();
        let old = self.running.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(old) = old {
            old.task.abort();
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let transport = old.transport;
                rt.spawn(async move { transport.close().await });
            }
        }
    }

    fn wanted_spec(&self) -> Option<BindSpec> {
        let wanted = self.wanted.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let caps: Vec<Arc<Capability>> = wanted.iter().filter_map(|id| self.registry.get(id)).collect();
        if caps.is_empty() {
            return None;
        }
        Some(BindSpec {
            alpns: caps.iter().map(|c| c.alpn.clone()).collect(),
            relays: caps.iter().any(|c| c.relay_allowed()),
            loopback_only: self.loopback_only,
        })
    }

    /// Makes the running endpoint match what is wanted. Caller holds `start_lock`.
    async fn reconcile(&self) -> Result<Arc<dyn Transport>, P2pError> {
        let Some(spec) = self.wanted_spec() else {
            self.stop();
            return Err(P2pError::UnknownCapability("nothing wants the network".into()));
        };
        let current = self.running.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|r| (r.transport.clone(), r.spec.clone()));
        match current {
            Some((transport, have)) if have == spec => Ok(transport),
            Some((transport, have)) if have.relays == spec.relays && have.loopback_only == spec.loopback_only => {
                // Same network settings, other protocol names: widen or narrow in place, links stay up.
                transport.set_alpns(spec.alpns.iter().cloned().collect()).await;
                if let Some(r) = self.running.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                    r.spec = spec;
                }
                Ok(transport)
            }
            other => {
                if let Some((old, _)) = other {
                    // A relay setting cannot change on a live endpoint: rebind (open links end).
                    if let Some(r) = self.running.lock().unwrap_or_else(|p| p.into_inner()).take() {
                        r.task.abort();
                    }
                    old.close().await;
                }
                let identity = DeviceIdentity::load(&self.keys)?;
                let transport = self.factory.bind(&identity, &spec).await?;
                let task = tokio::spawn(accept_loop(
                    transport.clone(),
                    self.registry.clone(),
                    self.perms.clone(),
                    self.refusals.clone(),
                    self.labs.clone(),
                ));
                *self.running.lock().unwrap_or_else(|p| p.into_inner()) = Some(Running { transport: transport.clone(), spec, task });
                Ok(transport)
            }
        }
    }
}

impl std::fmt::Debug for P2p {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P2p(running: {})", self.is_running())
    }
}
