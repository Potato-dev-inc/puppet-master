//! The link layer's two seams and the rules that sit on top of them.
//!
//! - `Transport` / `Link` / `TransportFactory`: what a network has to do (dial, accept, streams and datagrams per
//!   protocol name). Two implementations: iroh (`iroh_net.rs`) and an in-memory net for tests (`memory.rs`).
//! - `admit`: what every incoming connection goes through before a capability sees it: known protocol name, no relay
//!   for a direct-only capability, and the per-device permission. Anything else is closed with a neutral reason.
//!
//! Nothing here knows what a capability does.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use super::capability::{Capability, Permissions, Registry};
use super::identity::DeviceIdentity;

/// A device on the link: its Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn short(&self) -> String {
        self.to_hex()[..8].to_string()
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeId({})", self.short())
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// How to reach a device: its id plus whatever addresses are known (LAN addresses from pairing or discovery).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAddr {
    pub id: NodeId,
    pub direct: Vec<SocketAddr>,
}

impl PeerAddr {
    pub fn new(id: NodeId) -> Self {
        Self { id, direct: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Straight to the other computer.
    Direct,
    /// Through a relay server. Also what an unknown path counts as: unknown is treated as the stricter case.
    Relayed,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum P2pError {
    #[error("FEATURE_OFF: My computers is switched off in Labs")]
    Off,
    #[error("no such capability: {0}")]
    UnknownCapability(String),
    #[error("capability not usable: {0}")]
    BadCapability(String),
    #[error("the other computer does not speak this protocol")]
    NoSharedAlpn,
    #[error("a relay may not carry this capability")]
    RelayForbidden,
    #[error("the link is closed")]
    Closed,
    #[error("device key: {0}")]
    Identity(String),
    #[error("network: {0}")]
    Net(String),
}

/// Both halves of a stream.
pub struct BiStream {
    pub send: Box<dyn AsyncWrite + Send + Unpin>,
    pub recv: Box<dyn AsyncRead + Send + Unpin>,
}

/// An established, authenticated, encrypted connection to one device for one protocol name.
#[async_trait]
pub trait Link: Send + Sync {
    fn peer(&self) -> NodeId;
    fn alpn(&self) -> Vec<u8>;
    fn path(&self) -> PathKind;
    /// Resolves when the path is no longer `from` (returns the new kind), or `None` when the link has closed.
    async fn wait_path_change(&self, from: PathKind) -> Option<PathKind>;
    async fn open_bi(&self) -> Result<BiStream, P2pError>;
    async fn accept_bi(&self) -> Result<BiStream, P2pError>;
    async fn send_datagram(&self, data: Vec<u8>) -> Result<(), P2pError>;
    async fn recv_datagram(&self) -> Result<Vec<u8>, P2pError>;
    fn close(&self, code: u32, reason: &str);
    /// Resolves with the close code when the link is closed by either side.
    async fn closed(&self) -> u32;
}

/// A bound endpoint on some network.
#[async_trait]
pub trait Transport: Send + Sync {
    fn id(&self) -> NodeId;
    /// The protocol names accepted from now on.
    async fn set_alpns(&self, alpns: Vec<Vec<u8>>);
    /// Fails with `NoSharedAlpn` when the other side does not speak `alpn`.
    async fn dial(&self, peer: &PeerAddr, alpn: &[u8]) -> Result<Arc<dyn Link>, P2pError>;
    /// The next incoming link, or `None` once the transport is closed.
    async fn accept(&self) -> Option<Arc<dyn Link>>;
    async fn close(&self);
    /// LAN addresses this endpoint can be reached at, for the pairing ticket. Empty when unknown.
    fn local_addrs(&self) -> Vec<SocketAddr> {
        Vec::new()
    }
}

/// What an endpoint is started with. Computed from the capabilities that are wanted, nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindSpec {
    pub alpns: BTreeSet<Vec<u8>>,
    /// Use relay servers. False unless a wanted capability allows a relay: a desk-only endpoint has none at all.
    pub relays: bool,
    /// Bind to loopback only (tests and local demos).
    pub loopback_only: bool,
}

#[async_trait]
pub trait TransportFactory: Send + Sync {
    async fn bind(&self, identity: &DeviceIdentity, spec: &BindSpec) -> Result<Arc<dyn Transport>, P2pError>;
}

// ----- admission -----

pub const CLOSE_UNKNOWN_ALPN: u32 = 1;
pub const CLOSE_NOT_ALLOWED: u32 = 2;
pub const CLOSE_RELAY: u32 = 3;
pub const CLOSE_OFF: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    UnknownAlpn,
    NotAllowed,
    RelayForbidden,
    Off,
}

/// One refused connection, for the device list ("Fish's laptop tried to use files").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub peer: NodeId,
    pub capability: Option<String>,
    pub kind: RefusalKind,
}

pub type RefusalLog = Arc<Mutex<Vec<Refusal>>>;

const LOG_CAP: usize = 50;

fn refuse(link: &Arc<dyn Link>, log: &RefusalLog, capability: Option<&Arc<Capability>>, kind: RefusalKind) {
    let code = match kind {
        RefusalKind::UnknownAlpn => CLOSE_UNKNOWN_ALPN,
        RefusalKind::NotAllowed => CLOSE_NOT_ALLOWED,
        RefusalKind::RelayForbidden => CLOSE_RELAY,
        RefusalKind::Off => CLOSE_OFF,
    };
    // The reason sent to the other side is the same for every refusal: it learns nothing about this computer.
    link.close(code, "refused");
    let mut l = log.lock().unwrap_or_else(|p| p.into_inner());
    if l.len() >= LOG_CAP {
        l.remove(0);
    }
    l.push(Refusal { peer: link.peer(), capability: capability.map(|c| c.id.clone()), kind });
}

/// Closes a direct-only link the moment its path turns into a relay.
pub fn spawn_path_guard(link: Arc<dyn Link>) {
    tokio::spawn(async move {
        let mut now = link.path();
        loop {
            if now == PathKind::Relayed {
                link.close(CLOSE_RELAY, "refused");
                return;
            }
            match link.wait_path_change(now).await {
                Some(next) => now = next,
                None => return,
            }
        }
    });
}

/// Every incoming link goes through here. Returns the capability when a handler may have the link; otherwise the
/// link is already closed and the refusal logged.
pub fn admit(link: &Arc<dyn Link>, registry: &Registry, perms: &dyn Permissions, log: &RefusalLog) -> Option<Arc<Capability>> {
    let Some(cap) = registry.by_alpn(&link.alpn()) else {
        refuse(link, log, None, RefusalKind::UnknownAlpn);
        return None;
    };
    if !cap.relay_allowed() && link.path() != PathKind::Direct {
        refuse(link, log, Some(&cap), RefusalKind::RelayForbidden);
        return None;
    }
    if !perms.allowed(&link.peer(), &cap.id) {
        refuse(link, log, Some(&cap), RefusalKind::NotAllowed);
        return None;
    }
    if !cap.relay_allowed() {
        spawn_path_guard(link.clone());
    }
    Some(cap)
}

/// Dial-side rule: a direct-only capability never keeps a relayed link.
pub fn check_dialed(link: &Arc<dyn Link>, cap: &Capability) -> Result<(), P2pError> {
    if !cap.relay_allowed() {
        if link.path() != PathKind::Direct {
            link.close(CLOSE_RELAY, "refused");
            return Err(P2pError::RelayForbidden);
        }
        spawn_path_guard(link.clone());
    }
    Ok(())
}

/// The accept loop of a running endpoint.
pub async fn accept_loop(
    transport: Arc<dyn Transport>,
    registry: Arc<Registry>,
    perms: Arc<dyn Permissions>,
    log: RefusalLog,
    labs: Arc<AtomicBool>,
) {
    while let Some(link) = transport.accept().await {
        if !labs.load(Ordering::SeqCst) {
            refuse(&link, &log, None, RefusalKind::Off);
            continue;
        }
        let (registry, perms, log) = (registry.clone(), perms.clone(), log.clone());
        tokio::spawn(async move {
            if let Some(cap) = admit(&link, &registry, perms.as_ref(), &log) {
                cap.handler.serve(link.peer(), link).await;
            }
        });
    }
}

/// How long a handshake may take before the attempt is dropped.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
