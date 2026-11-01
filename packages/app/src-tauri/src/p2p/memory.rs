//! An in-memory network for tests: no sockets, no clock. Nodes bind to a shared `MemoryNet` and dial each other by
//! `NodeId`. A test can make the path between two nodes direct or relayed (and change it while a link is open) to
//! exercise the relay rules.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::io::duplex;
use tokio::sync::{mpsc, watch, Mutex as AsyncMutex};

use super::endpoint::{BiStream, BindSpec, Link, NodeId, P2pError, PathKind, PeerAddr, Transport, TransportFactory};
use super::identity::DeviceIdentity;

const STREAM_BUF: usize = 64 * 1024;

struct Slot {
    alpns: BTreeSet<Vec<u8>>,
    incoming: mpsc::UnboundedSender<Arc<MemLink>>,
    spec: BindSpec,
}

#[derive(Default)]
struct Hub {
    nodes: HashMap<NodeId, Slot>,
    paths: HashMap<(NodeId, NodeId), watch::Sender<PathKind>>,
    default_relayed: bool,
    binds: usize,
    /// Pairs whose datagrams are silently dropped (a pulled network cable).
    holes: std::collections::HashSet<(NodeId, NodeId)>,
}

fn pair_key(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[derive(Clone, Default)]
pub struct MemoryNet {
    hub: Arc<Mutex<Hub>>,
}

impl MemoryNet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn factory(&self) -> Arc<dyn TransportFactory> {
        Arc::new(self.clone())
    }

    fn hub(&self) -> std::sync::MutexGuard<'_, Hub> {
        self.hub.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// How many endpoints were ever bound on this net.
    pub fn bind_count(&self) -> usize {
        self.hub().binds
    }

    /// The spec a node is currently bound with.
    pub fn spec_of(&self, id: &NodeId) -> Option<BindSpec> {
        self.hub().nodes.get(id).map(|s| s.spec.clone())
    }

    /// New pairs of nodes start relayed instead of direct.
    pub fn default_to_relayed(&self, on: bool) {
        self.hub().default_relayed = on;
    }

    /// Datagrams between two nodes vanish (or come back): the cable is pulled. Streams are not touched.
    pub fn blackhole(&self, a: NodeId, b: NodeId, on: bool) {
        let mut hub = self.hub();
        if on {
            hub.holes.insert(pair_key(a, b));
        } else {
            hub.holes.remove(&pair_key(a, b));
        }
    }

    fn blackholed(&self, a: NodeId, b: NodeId) -> bool {
        self.hub().holes.contains(&pair_key(a, b))
    }

    /// Sets the path between two nodes, for links already open and for new ones.
    pub fn set_path(&self, a: NodeId, b: NodeId, kind: PathKind) {
        let mut hub = self.hub();
        let tx = hub.paths.entry(pair_key(a, b)).or_insert_with(|| watch::channel(kind).0);
        tx.send_replace(kind);
    }

    /// A bare transport with its own id and protocol names, for tests that play the other computer by hand.
    pub fn bind_raw(&self, id: NodeId, alpns: &[&[u8]]) -> Arc<dyn Transport> {
        let spec = BindSpec { alpns: alpns.iter().map(|a| a.to_vec()).collect(), relays: false, loopback_only: true };
        self.bind_spec(id, spec)
    }

    fn bind_spec(&self, id: NodeId, spec: BindSpec) -> Arc<dyn Transport> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut hub = self.hub();
        hub.binds += 1;
        hub.nodes.insert(id, Slot { alpns: spec.alpns.clone(), incoming: tx, spec });
        Arc::new(MemTransport { net: self.clone(), id, incoming: AsyncMutex::new(rx) })
    }

    fn path_rx(hub: &mut Hub, a: NodeId, b: NodeId) -> watch::Receiver<PathKind> {
        let start = if hub.default_relayed { PathKind::Relayed } else { PathKind::Direct };
        hub.paths.entry(pair_key(a, b)).or_insert_with(|| watch::channel(start).0).subscribe()
    }
}

#[async_trait]
impl TransportFactory for MemoryNet {
    async fn bind(&self, identity: &DeviceIdentity, spec: &BindSpec) -> Result<Arc<dyn Transport>, P2pError> {
        Ok(self.bind_spec(identity.id(), spec.clone()))
    }
}

struct MemTransport {
    net: MemoryNet,
    id: NodeId,
    incoming: AsyncMutex<mpsc::UnboundedReceiver<Arc<MemLink>>>,
}

#[async_trait]
impl Transport for MemTransport {
    fn id(&self) -> NodeId {
        self.id
    }

    async fn set_alpns(&self, alpns: Vec<Vec<u8>>) {
        if let Some(slot) = self.net.hub().nodes.get_mut(&self.id) {
            slot.alpns = alpns.iter().cloned().collect();
            slot.spec.alpns = slot.alpns.clone();
        }
    }

    async fn dial(&self, peer: &PeerAddr, alpn: &[u8]) -> Result<Arc<dyn Link>, P2pError> {
        let (incoming, path) = {
            let mut hub = self.net.hub();
            let Some(slot) = hub.nodes.get(&peer.id) else {
                return Err(P2pError::Net("unreachable".into()));
            };
            if !slot.alpns.contains(alpn) {
                return Err(P2pError::NoSharedAlpn);
            }
            let incoming = slot.incoming.clone();
            (incoming, MemoryNet::path_rx(&mut hub, self.id, peer.id))
        };
        let (mine, theirs) = MemLink::pair(self.net.clone(), self.id, peer.id, alpn, path);
        incoming.send(theirs).map_err(|_| P2pError::Net("unreachable".into()))?;
        Ok(mine)
    }

    async fn accept(&self) -> Option<Arc<dyn Link>> {
        let link = self.incoming.lock().await.recv().await?;
        Some(link as Arc<dyn Link>)
    }

    async fn close(&self) {
        // Dropping the slot drops the sender: `accept` returns None.
        self.net.hub().nodes.remove(&self.id);
    }
}

struct MemLink {
    net: MemoryNet,
    local: NodeId,
    peer: NodeId,
    alpn: Vec<u8>,
    path: watch::Receiver<PathKind>,
    bi_out: mpsc::UnboundedSender<BiStream>,
    bi_in: AsyncMutex<mpsc::UnboundedReceiver<BiStream>>,
    dg_out: mpsc::UnboundedSender<Vec<u8>>,
    dg_in: AsyncMutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    closed: Arc<watch::Sender<Option<u32>>>,
}

impl MemLink {
    /// (the dialer's end, the acceptor's end)
    fn pair(net: MemoryNet, dialer: NodeId, acceptor: NodeId, alpn: &[u8], path: watch::Receiver<PathKind>) -> (Arc<MemLink>, Arc<MemLink>) {
        let (bi_a_tx, bi_a_rx) = mpsc::unbounded_channel();
        let (bi_b_tx, bi_b_rx) = mpsc::unbounded_channel();
        let (dg_a_tx, dg_a_rx) = mpsc::unbounded_channel();
        let (dg_b_tx, dg_b_rx) = mpsc::unbounded_channel();
        let closed = Arc::new(watch::channel(None).0);
        let a = Arc::new(MemLink {
            net: net.clone(),
            local: dialer,
            peer: acceptor,
            alpn: alpn.to_vec(),
            path: path.clone(),
            bi_out: bi_b_tx,
            bi_in: AsyncMutex::new(bi_a_rx),
            dg_out: dg_b_tx,
            dg_in: AsyncMutex::new(dg_a_rx),
            closed: closed.clone(),
        });
        let b = Arc::new(MemLink {
            net,
            local: acceptor,
            peer: dialer,
            alpn: alpn.to_vec(),
            path,
            bi_out: bi_a_tx,
            bi_in: AsyncMutex::new(bi_b_rx),
            dg_out: dg_a_tx,
            dg_in: AsyncMutex::new(dg_b_rx),
            closed,
        });
        (a, b)
    }

    fn is_closed(&self) -> bool {
        self.closed.borrow().is_some()
    }

    async fn wait_closed(&self) -> u32 {
        let mut rx = self.closed.subscribe();
        let code = match rx.wait_for(|v| v.is_some()).await {
            Ok(v) => v.unwrap_or(0),
            Err(_) => 0,
        };
        code
    }
}

#[async_trait]
impl Link for MemLink {
    fn peer(&self) -> NodeId {
        self.peer
    }

    fn alpn(&self) -> Vec<u8> {
        self.alpn.clone()
    }

    fn path(&self) -> PathKind {
        *self.path.borrow()
    }

    async fn wait_path_change(&self, from: PathKind) -> Option<PathKind> {
        let mut rx = self.path.clone();
        let mut closed = self.closed.subscribe();
        tokio::select! {
            r = rx.wait_for(|k| *k != from) => r.ok().map(|k| *k),
            _ = closed.wait_for(|v| v.is_some()) => None,
        }
    }

    async fn open_bi(&self) -> Result<BiStream, P2pError> {
        if self.is_closed() {
            return Err(P2pError::Closed);
        }
        let (x, y) = duplex(STREAM_BUF);
        let (xr, xw) = tokio::io::split(x);
        let (yr, yw) = tokio::io::split(y);
        self.bi_out.send(BiStream { send: Box::new(yw), recv: Box::new(yr) }).map_err(|_| P2pError::Closed)?;
        Ok(BiStream { send: Box::new(xw), recv: Box::new(xr) })
    }

    async fn accept_bi(&self) -> Result<BiStream, P2pError> {
        if self.is_closed() {
            return Err(P2pError::Closed);
        }
        let mut rx = self.bi_in.lock().await;
        tokio::select! {
            s = rx.recv() => s.ok_or(P2pError::Closed),
            _ = self.wait_closed() => Err(P2pError::Closed),
        }
    }

    async fn send_datagram(&self, data: Vec<u8>) -> Result<(), P2pError> {
        if self.is_closed() {
            return Err(P2pError::Closed);
        }
        if self.net.blackholed(self.local, self.peer) {
            return Ok(());
        }
        self.dg_out.send(data).map_err(|_| P2pError::Closed)
    }

    async fn recv_datagram(&self) -> Result<Vec<u8>, P2pError> {
        let mut rx = self.dg_in.lock().await;
        tokio::select! {
            d = rx.recv() => d.ok_or(P2pError::Closed),
            _ = self.wait_closed() => Err(P2pError::Closed),
        }
    }

    fn close(&self, code: u32, _reason: &str) {
        self.closed.send_if_modified(|v| {
            if v.is_none() {
                *v = Some(code);
                true
            } else {
                false
            }
        });
    }

    async fn closed(&self) -> u32 {
        self.wait_closed().await
    }
}
