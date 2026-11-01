//! The real network: an iroh endpoint (QUIC, dialed by public key, hole punching, optional relay).
//!
//! - The node key is the device identity (`identity.rs`), handed to iroh in memory only.
//! - No address-lookup service is configured, so nothing is published about this computer and nothing is asked of
//!   n0's DNS: a peer is reached through the addresses pairing gave us (`PeerAddr.direct`).
//! - Relays are on only when the `BindSpec` says a wanted capability allows them. A desk-only endpoint has none.
//! - No test in this repository binds this transport: tests use `memory.rs`. The first real run is the two-computer
//!   script of DL-11.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use iroh::endpoint::{presets, Connection};
use iroh::{EndpointAddr, RelayMode, SecretKey};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use super::endpoint::{
    BiStream, BindSpec, Link, NodeId, P2pError, PathKind, PeerAddr, Transport, TransportFactory, HANDSHAKE_TIMEOUT,
};
use super::identity::DeviceIdentity;

/// How often a link looks at which path iroh has selected.
const PATH_POLL: Duration = Duration::from_millis(500);

fn net<E: std::fmt::Display>(e: E) -> P2pError {
    P2pError::Net(e.to_string())
}

pub struct IrohFactory;

#[async_trait]
impl TransportFactory for IrohFactory {
    async fn bind(&self, identity: &DeviceIdentity, spec: &BindSpec) -> Result<Arc<dyn Transport>, P2pError> {
        let secret = SecretKey::from_bytes(&identity.secret_bytes());
        let mut builder = iroh::Endpoint::builder(presets::Minimal)
            .secret_key(secret)
            .alpns(spec.alpns.iter().cloned().collect())
            .relay_mode(if spec.relays { RelayMode::Default } else { RelayMode::Disabled });
        if spec.loopback_only {
            builder = builder.clear_ip_transports().bind_addr("127.0.0.1:0").map_err(net)?;
        }
        let endpoint = builder.bind().await.map_err(net)?;

        // Handshakes run in their own tasks, so one slow peer cannot hold up the others.
        let (tx, rx) = mpsc::unbounded_channel::<Arc<dyn Link>>();
        let ep = endpoint.clone();
        tokio::spawn(async move {
            while let Some(incoming) = ep.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Ok(accepting) = incoming.accept() else { return };
                    if let Ok(Ok(conn)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, accepting).await {
                        let _ = tx.send(Arc::new(IrohLink::new(conn)));
                    }
                });
            }
        });
        Ok(Arc::new(IrohTransport { endpoint, incoming: AsyncMutex::new(rx) }))
    }
}

struct IrohTransport {
    endpoint: iroh::Endpoint,
    incoming: AsyncMutex<mpsc::UnboundedReceiver<Arc<dyn Link>>>,
}

#[async_trait]
impl Transport for IrohTransport {
    fn id(&self) -> NodeId {
        NodeId(*self.endpoint.id().as_bytes())
    }

    async fn set_alpns(&self, alpns: Vec<Vec<u8>>) {
        self.endpoint.set_alpns(alpns);
    }

    async fn dial(&self, peer: &PeerAddr, alpn: &[u8]) -> Result<Arc<dyn Link>, P2pError> {
        let id = iroh::PublicKey::from_bytes(&peer.id.0).map_err(net)?;
        let mut addr = EndpointAddr::new(id);
        for a in &peer.direct {
            addr = addr.with_ip_addr(*a);
        }
        let conn = tokio::time::timeout(HANDSHAKE_TIMEOUT, self.endpoint.connect(addr, alpn))
            .await
            .map_err(|_| P2pError::Net("timed out".into()))?
            .map_err(net)?;
        Ok(Arc::new(IrohLink::new(conn)))
    }

    async fn accept(&self) -> Option<Arc<dyn Link>> {
        self.incoming.lock().await.recv().await
    }

    async fn close(&self) {
        self.endpoint.close().await;
    }

    fn local_addrs(&self) -> Vec<std::net::SocketAddr> {
        // Where iroh says it can be reached; before it has found out, the sockets it bound (never "any" addresses).
        let mut v: Vec<std::net::SocketAddr> = self.endpoint.addr().ip_addrs().copied().collect();
        if v.is_empty() {
            v = self.endpoint.bound_sockets().into_iter().filter(|a| !a.ip().is_unspecified()).collect();
        }
        v
    }
}

struct IrohLink {
    conn: Connection,
}

impl IrohLink {
    fn new(conn: Connection) -> Self {
        Self { conn }
    }

    /// Unknown counts as relayed: the strict reading.
    fn selected(&self) -> PathKind {
        for p in self.conn.paths().iter() {
            if p.is_selected() {
                return if p.is_ip() { PathKind::Direct } else { PathKind::Relayed };
            }
        }
        PathKind::Relayed
    }
}

#[async_trait]
impl Link for IrohLink {
    fn peer(&self) -> NodeId {
        NodeId(*self.conn.remote_id().as_bytes())
    }

    fn alpn(&self) -> Vec<u8> {
        self.conn.alpn().to_vec()
    }

    fn path(&self) -> PathKind {
        self.selected()
    }

    async fn wait_path_change(&self, from: PathKind) -> Option<PathKind> {
        loop {
            if self.conn.close_reason().is_some() {
                return None;
            }
            let now = self.selected();
            if now != from {
                return Some(now);
            }
            tokio::time::sleep(PATH_POLL).await;
        }
    }

    async fn open_bi(&self) -> Result<BiStream, P2pError> {
        let (send, recv) = self.conn.open_bi().await.map_err(|_| P2pError::Closed)?;
        Ok(BiStream { send: Box::new(send), recv: Box::new(recv) })
    }

    async fn accept_bi(&self) -> Result<BiStream, P2pError> {
        let (send, recv) = self.conn.accept_bi().await.map_err(|_| P2pError::Closed)?;
        Ok(BiStream { send: Box::new(send), recv: Box::new(recv) })
    }

    async fn send_datagram(&self, data: Vec<u8>) -> Result<(), P2pError> {
        self.conn.send_datagram(Bytes::from(data)).map_err(net)
    }

    async fn recv_datagram(&self) -> Result<Vec<u8>, P2pError> {
        let d = self.conn.read_datagram().await.map_err(|_| P2pError::Closed)?;
        Ok(d.to_vec())
    }

    fn close(&self, code: u32, reason: &str) {
        self.conn.close(code.into(), reason.as_bytes());
    }

    async fn closed(&self) -> u32 {
        // The code is only informational here; iroh reports it inside the error.
        let _ = self.conn.closed().await;
        0
    }
}
