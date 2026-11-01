//! The link layer on the in-memory net. No sockets, no real network, no real credential store.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::alpn;
use super::capability::{self, Capability, CapabilityHandler, MemoryPermissions, Permissions, RelayPolicy, Registry};
use super::endpoint::*;
use super::identity::DeviceIdentity;
use super::memory::MemoryNet;
use super::P2p;
use crate::secrets::{MemoryBackend, SecretBackend};
use crate::team::keystore::KeyStore;

const WAIT: Duration = Duration::from_secs(5);

/// Upper-cases whatever arrives on the first stream, and counts how often a connection reached it.
struct Echo {
    served: AtomicUsize,
}

#[async_trait]
impl CapabilityHandler for Echo {
    async fn serve(&self, _peer: NodeId, link: Arc<dyn Link>) {
        self.served.fetch_add(1, Ordering::SeqCst);
        let Ok(mut s) = link.accept_bi().await else { return };
        let mut buf = Vec::new();
        let _ = s.recv.read_to_end(&mut buf).await;
        buf.make_ascii_uppercase();
        let _ = s.send.write_all(&buf).await;
        let _ = s.send.shutdown().await;
        // Hold the link until the other side is done with it.
        link.closed().await;
    }
}

/// A secret store that counts how often it was asked anything.
struct Counting {
    inner: MemoryBackend,
    calls: AtomicUsize,
}

impl Counting {
    fn new() -> Arc<Self> {
        Arc::new(Self { inner: MemoryBackend::new(), calls: AtomicUsize::new(0) })
    }
}

impl SecretBackend for Counting {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.get(name)
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.set(name, value)
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.delete(name)
    }
    fn label(&self) -> &'static str {
        "test"
    }
    fn available(&self) -> bool {
        true
    }
}

struct Node {
    p2p: P2p,
    id: NodeId,
    echo: Arc<Echo>,
    perms: Arc<MemoryPermissions>,
}

/// A node with the capabilities "desk/input" (even though it asks for a relay: the protocol name decides), "files"
/// (relay allowed) and "spaces" (relay allowed). Labs on.
fn node(net: &MemoryNet, name: &str) -> Node {
    let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
    let keys = KeyStore::with_backend(backend, name);
    let id = DeviceIdentity::load(&keys).unwrap().id();
    let echo = Arc::new(Echo { served: AtomicUsize::new(0) });
    let registry = Arc::new(Registry::new());
    for (cid, a) in [(capability::DESK_INPUT, alpn::DESK), (capability::FILES, alpn::FILES), (capability::SPACES, alpn::SPACES)] {
        registry.register(Capability::new(cid, a, RelayPolicy::Allowed, echo.clone())).unwrap();
    }
    let perms = Arc::new(MemoryPermissions::new());
    let p2p = P2p::new(keys, net.factory(), registry, perms.clone());
    p2p.apply_settings(&json!({ "desk_enabled": true }));
    Node { p2p, id, echo, perms }
}

async fn echo_roundtrip(link: &Arc<dyn Link>, text: &str) -> String {
    let mut s = link.open_bi().await.unwrap();
    s.send.write_all(text.as_bytes()).await.unwrap();
    s.send.shutdown().await.unwrap();
    let mut out = String::new();
    tokio::time::timeout(WAIT, s.recv.read_to_string(&mut out)).await.expect("no answer").unwrap();
    out
}

async fn closed_code(link: &Arc<dyn Link>) -> u32 {
    tokio::time::timeout(WAIT, link.closed()).await.expect("the link was not closed")
}

// ----- two nodes talk -----

#[tokio::test]
async fn two_nodes_talk_over_an_alpn() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.perms.set(&a.id, capability::FILES, true);
    b.p2p.want(capability::FILES).await.unwrap();

    let link = a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(link.peer(), b.id);
    assert_eq!(link.alpn(), alpn::FILES);
    assert_eq!(echo_roundtrip(&link, "hello over the link").await, "HELLO OVER THE LINK");
    assert_eq!(b.echo.served.load(Ordering::SeqCst), 1);
    assert!(b.p2p.refusals().is_empty());
}

#[tokio::test]
async fn datagrams_flow_both_ways() {
    let net = MemoryNet::new();
    let (da, ra) = (NodeId([1; 32]), NodeId([2; 32]));
    let dialer = net.bind_raw(da, &[]);
    let listener = net.bind_raw(ra, &[alpn::FILES]);
    let out = dialer.dial(&PeerAddr::new(ra), alpn::FILES).await.unwrap();
    let inn = listener.accept().await.unwrap();
    assert_eq!(inn.peer(), da);
    out.send_datagram(b"ping".to_vec()).await.unwrap();
    assert_eq!(inn.recv_datagram().await.unwrap(), b"ping");
    inn.send_datagram(b"pong".to_vec()).await.unwrap();
    assert_eq!(out.recv_datagram().await.unwrap(), b"pong");
}

// ----- unknown ALPN -----

#[tokio::test]
async fn an_unknown_alpn_is_refused() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.perms.set(&a.id, capability::FILES, true);
    b.p2p.want(capability::FILES).await.unwrap();
    let t = a.p2p.want(capability::FILES).await.unwrap();

    // Not a name b speaks.
    assert_eq!(t.dial(&PeerAddr::new(b.id), b"pm/nope/1").await.err(), Some(P2pError::NoSharedAlpn));
    // A name b knows but nothing there wants: not advertised, so refused in the handshake too.
    assert_eq!(t.dial(&PeerAddr::new(b.id), alpn::DESK).await.err(), Some(P2pError::NoSharedAlpn));
    assert_eq!(b.echo.served.load(Ordering::SeqCst), 0);

    // A link that does arrive under a name the registry lacks is closed by admission.
    let odd = NodeId([9; 32]);
    let listener = net.bind_raw(odd, &[b"pm/odd/1"]);
    let dialer = net.bind_raw(NodeId([8; 32]), &[]);
    let out = dialer.dial(&PeerAddr::new(odd), b"pm/odd/1").await.unwrap();
    let inn = listener.accept().await.unwrap();
    let log: RefusalLog = Default::default();
    assert!(admit(&inn, b.p2p.registry(), b.p2p.permissions().as_ref(), &log).is_none());
    assert_eq!(closed_code(&out).await, CLOSE_UNKNOWN_ALPN);
    assert_eq!(log.lock().unwrap()[0].kind, RefusalKind::UnknownAlpn);
}

// ----- permission -----

#[tokio::test]
async fn permission_off_means_refused_and_default_is_off() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.p2p.want(capability::FILES).await.unwrap();
    assert!(!b.perms.allowed(&a.id, capability::FILES), "default is off");

    let link = a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(closed_code(&link).await, CLOSE_NOT_ALLOWED);
    assert!(link.open_bi().await.is_err());
    assert_eq!(b.echo.served.load(Ordering::SeqCst), 0, "the handler never saw the connection");
    let log = b.p2p.refusals();
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].peer, log[0].capability.as_deref(), log[0].kind), (a.id, Some("files"), RefusalKind::NotAllowed));

    // On for this device and this capability: it works.
    b.perms.set(&a.id, capability::FILES, true);
    let link = a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(echo_roundtrip(&link, "ok").await, "OK");

    // Another capability of the same device stays off.
    b.p2p.want(capability::SPACES).await.unwrap();
    let other = a.p2p.dial(&PeerAddr::new(b.id), capability::SPACES).await.unwrap();
    assert_eq!(closed_code(&other).await, CLOSE_NOT_ALLOWED);

    // And off again means refused again.
    b.perms.set(&a.id, capability::FILES, false);
    let again = a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(closed_code(&again).await, CLOSE_NOT_ALLOWED);
    assert_eq!(b.echo.served.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn permission_belongs_to_one_device() {
    let net = MemoryNet::new();
    let (a, b, c) = (node(&net, "a"), node(&net, "b"), node(&net, "c"));
    b.perms.set(&a.id, capability::FILES, true);
    b.p2p.want(capability::FILES).await.unwrap();
    let from_a = a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(echo_roundtrip(&from_a, "a").await, "A");
    let from_c = c.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(closed_code(&from_c).await, CLOSE_NOT_ALLOWED);
}

// ----- relays -----

#[tokio::test]
async fn a_relayed_path_is_refused_for_desk() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.perms.set(&a.id, capability::DESK_INPUT, true);
    b.perms.set(&a.id, capability::FILES, true);
    net.default_to_relayed(true);

    // The capability asked for relays; the protocol name says no, and the name wins.
    b.p2p.want(capability::DESK_INPUT).await.unwrap();
    assert!(!net.spec_of(&b.id).unwrap().relays, "a desk-only endpoint has no relay at all");
    assert_eq!(a.p2p.dial(&PeerAddr::new(b.id), capability::DESK_INPUT).await.err(), Some(P2pError::RelayForbidden));
    assert_eq!(b.echo.served.load(Ordering::SeqCst), 0);

    // Receiving side: a peer that dials by hand over a relayed path is closed by admission.
    let raw = net.bind_raw(NodeId([7; 32]), &[]);
    let out = raw.dial(&PeerAddr::new(b.id), alpn::DESK).await.unwrap();
    assert_eq!(closed_code(&out).await, CLOSE_RELAY);
    assert!(b.p2p.refusals().iter().any(|r| r.kind == RefusalKind::RelayForbidden));

    // Other capabilities may use a relay when they allow it, and then the endpoint is bound with relays.
    b.p2p.want(capability::FILES).await.unwrap();
    assert!(net.spec_of(&b.id).unwrap().relays);
    let link = a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.unwrap();
    assert_eq!(link.path(), PathKind::Relayed);
    assert_eq!(echo_roundtrip(&link, "via relay").await, "VIA RELAY");
}

#[tokio::test]
async fn a_desk_link_that_becomes_relayed_is_closed() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.perms.set(&a.id, capability::DESK_INPUT, true);
    b.p2p.want(capability::DESK_INPUT).await.unwrap();
    let link = a.p2p.dial(&PeerAddr::new(b.id), capability::DESK_INPUT).await.unwrap();
    assert_eq!(echo_roundtrip(&link, "direct").await, "DIRECT");
    net.set_path(a.id, b.id, PathKind::Relayed);
    assert_eq!(closed_code(&link).await, CLOSE_RELAY);
}

#[tokio::test]
async fn desk_alpns_are_never_relayable_whatever_the_capability_says() {
    assert!(!alpn::relay_possible(alpn::DESK));
    assert!(!alpn::relay_possible(alpn::DESK_CLIPBOARD));
    assert!(alpn::relay_possible(alpn::FILES));
    assert!(alpn::relay_possible(alpn::SPACES));
    let echo = Arc::new(Echo { served: AtomicUsize::new(0) });
    assert!(!Capability::new("x", alpn::DESK, RelayPolicy::Allowed, echo.clone()).relay_allowed());
    assert!(Capability::new("x", alpn::FILES, RelayPolicy::Allowed, echo.clone()).relay_allowed());
    assert!(!Capability::new("x", alpn::FILES, RelayPolicy::Never, echo).relay_allowed());
}

// ----- lifecycle -----

#[tokio::test]
async fn nothing_starts_at_init() {
    let net = MemoryNet::new();
    let counting = Counting::new();
    let keys = KeyStore::with_backend(counting.clone(), "idle");
    let registry = Arc::new(Registry::new());
    let echo = Arc::new(Echo { served: AtomicUsize::new(0) });
    registry.register(Capability::new(capability::FILES, alpn::FILES, RelayPolicy::Allowed, echo)).unwrap();
    let p2p = P2p::new(keys, net.factory(), registry, Arc::new(MemoryPermissions::new()));

    assert!(!p2p.is_running());
    assert_eq!(net.bind_count(), 0);
    assert_eq!(counting.calls.load(Ordering::SeqCst), 0, "the credential store is not even read at init");

    // Labs off: asking for the network is refused, and still nothing happens.
    assert_eq!(p2p.want(capability::FILES).await.err(), Some(P2pError::Off));
    p2p.apply_settings(&json!({}));
    p2p.apply_settings(&json!({ "desk_enabled": "yes" }));
    assert_eq!((net.bind_count(), p2p.is_running(), counting.calls.load(Ordering::SeqCst)), (0, false, 0));

    // Labs on alone starts nothing either: only a capability that wants the network does.
    p2p.apply_settings(&json!({ "desk_enabled": true }));
    assert_eq!((net.bind_count(), p2p.is_running()), (0, false));
    assert_eq!(p2p.want("no-such-capability").await.err(), Some(P2pError::UnknownCapability("no-such-capability".into())));
    assert_eq!(net.bind_count(), 0);

    p2p.want(capability::FILES).await.unwrap();
    assert_eq!((net.bind_count(), p2p.is_running()), (1, true));
    assert!(counting.calls.load(Ordering::SeqCst) > 0);
    // Asking again does not bind again.
    p2p.want(capability::FILES).await.unwrap();
    assert_eq!(net.bind_count(), 1);

    // Nobody wants it any more: closed.
    p2p.unwant(capability::FILES).await;
    assert!(!p2p.is_running());
}

#[tokio::test]
async fn labs_off_is_the_kill_switch() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.perms.set(&a.id, capability::FILES, true);
    b.p2p.want(capability::FILES).await.unwrap();
    assert!(b.p2p.is_running());
    b.p2p.apply_settings(&json!({ "desk_enabled": false }));
    assert!(!b.p2p.is_running());
    // The close runs on the runtime right after; give it a moment.
    let mut refused = false;
    for _ in 0..50 {
        if a.p2p.dial(&PeerAddr::new(b.id), capability::FILES).await.is_err() {
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(refused, "nobody is listening any more");
    assert_eq!(b.p2p.want(capability::FILES).await.err(), Some(P2pError::Off));
}

#[tokio::test]
async fn more_capabilities_widen_the_endpoint_without_dropping_links() {
    let net = MemoryNet::new();
    let (a, b) = (node(&net, "a"), node(&net, "b"));
    b.perms.set(&a.id, capability::DESK_INPUT, true);
    b.perms.set(&a.id, capability::FILES, true);
    b.p2p.want(capability::DESK_INPUT).await.unwrap();
    let desk = a.p2p.dial(&PeerAddr::new(b.id), capability::DESK_INPUT).await.unwrap();
    assert_eq!(net.bind_count(), 2, "a and b bound once each");

    // Files has the same network settings except the relay: that needs a rebind. Spaces next to it does not.
    b.p2p.want(capability::FILES).await.unwrap();
    assert_eq!(net.bind_count(), 3);
    b.p2p.want(capability::SPACES).await.unwrap();
    assert_eq!(net.bind_count(), 3);
    let spec = b.p2p.bound_spec().unwrap();
    assert_eq!(spec.alpns.len(), 3);
    assert!(spec.relays);

    // Narrowing back to desk only removes the relay and the other names.
    b.p2p.unwant(capability::FILES).await;
    b.p2p.unwant(capability::SPACES).await;
    let spec = b.p2p.bound_spec().unwrap();
    assert_eq!(spec.alpns.len(), 1);
    assert!(!spec.relays);
    drop(desk);
}

#[test]
fn the_registry_refuses_bad_and_duplicate_capabilities() {
    let r = Registry::new();
    let echo = Arc::new(Echo { served: AtomicUsize::new(0) });
    let mk = |id: &str, a: &[u8]| Capability::new(id, a, RelayPolicy::Never, echo.clone());
    assert!(r.register(mk("files", alpn::FILES)).is_ok());
    assert!(r.register(mk("files", b"pm/other/1")).is_err(), "duplicate id");
    assert!(r.register(mk("files2", alpn::FILES)).is_err(), "duplicate protocol name");
    assert!(r.register(mk("x", b"other/1")).is_err(), "not our prefix");
    assert!(r.register(mk("x", b"pm/")).is_err(), "empty name");
    assert!(r.register(mk("x", b"pm/has space/1")).is_err());
    assert!(r.register(mk("", b"pm/ok/1")).is_err());
    assert_eq!(r.ids(), vec!["files".to_string()]);
    assert!(r.by_alpn(alpn::FILES).is_some() && r.by_alpn(alpn::DESK).is_none());
}

// ----- the key -----

fn all_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            all_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[tokio::test]
async fn the_node_key_never_touches_the_disk() {
    // A stand-in for the app data dir: the endpoint is given no path at all, and nothing may appear in it.
    let data_dir = std::env::temp_dir().join(format!("pm-p2p-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_dir).unwrap();

    let net = MemoryNet::new();
    let backend = Counting::new();
    let keys = KeyStore::with_backend(backend.clone(), "disk-test");
    let registry = Arc::new(Registry::new());
    let echo = Arc::new(Echo { served: AtomicUsize::new(0) });
    registry.register(Capability::new(capability::FILES, alpn::FILES, RelayPolicy::Allowed, echo)).unwrap();
    let a_perms = Arc::new(MemoryPermissions::new());
    let p2p = P2p::new(keys.clone(), net.factory(), registry, a_perms);
    p2p.apply_settings(&json!({ "desk_enabled": true }));
    p2p.want(capability::FILES).await.unwrap();

    // The key is in the credential store, and only there.
    let secret = keys.p2p_node().unwrap().to_bytes();
    let b64 = crate::team::crypto::b64(&secret);
    let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(backend.get("team/disk-test/p2p-node").unwrap().as_deref(), Some(b64.as_str()));

    // Nothing printable carries it.
    let identity = DeviceIdentity::load(&keys).unwrap();
    let shown = format!("{identity:?} {p2p:?} {:?} {}", identity.id(), identity.id());
    assert!(!shown.contains(&b64) && !shown.contains(&hex));
    assert_eq!(p2p.node_id(), Some(identity.id()));

    // The data dir stays empty, and so does anything the process wrote to it.
    let mut files = Vec::new();
    all_files(&data_dir, &mut files);
    assert!(files.is_empty(), "p2p wrote {files:?}");
    for f in &files {
        let text = String::from_utf8_lossy(&std::fs::read(f).unwrap()).to_string();
        assert!(!text.contains(&b64) && !text.contains(&hex));
    }
    let _ = std::fs::remove_dir(&data_dir);
}

#[test]
fn identity_is_its_own_node_key_bound_to_the_device_key() {
    let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
    let keys = KeyStore::with_backend(backend, "same");
    let one = DeviceIdentity::load(&keys).unwrap();
    let two = DeviceIdentity::load(&keys).unwrap();
    assert_eq!(one.id(), two.id(), "one key per install");
    assert_ne!(one.id().0, keys.device_sign().unwrap().verifying_key().to_bytes(), "the node key is not the device key");
    assert_eq!(one.id().0, keys.p2p_node().unwrap().verifying_key().to_bytes());
    assert_ne!(one.secret_bytes(), keys.device_sign().unwrap().to_bytes());

    let other = KeyStore::with_backend(Arc::new(MemoryBackend::new()), "same");
    assert_ne!(DeviceIdentity::load(&other).unwrap().id(), one.id());

    let none = KeyStore::with_backend(Arc::new(MemoryBackend::unavailable()), "none");
    assert!(matches!(DeviceIdentity::load(&none), Err(P2pError::Identity(_))));
}

#[test]
fn the_binding_verifies_with_the_device_key_and_fails_when_tampered() {
    let keys = KeyStore::with_backend(Arc::new(MemoryBackend::new()), "bind");
    let identity = DeviceIdentity::load(&keys).unwrap();
    let b = identity.binding();
    assert_eq!(b.node_id, identity.id());
    assert_eq!(b.device_pub, keys.device_sign().unwrap().verifying_key().to_bytes());
    assert!(b.verify());

    let mut t = b.clone();
    t.node_id = NodeId([5; 32]);
    assert!(!t.verify(), "another node id");
    let mut t = b.clone();
    t.issued_at += 1;
    assert!(!t.verify(), "another date");
    let mut t = b.clone();
    t.device_pub = DeviceIdentity::load(&KeyStore::with_backend(Arc::new(MemoryBackend::new()), "o")).unwrap().binding().device_pub;
    assert!(!t.verify(), "another device");
    let mut t = b;
    t.signature[0] ^= 1;
    assert!(!t.verify(), "bad signature");
}
