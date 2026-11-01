//! Pairing, the trust list and "found nearby" on the in-memory net. No sockets, no mDNS, no real credential store.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;

use super::alpn;
use super::capability::{self, Capability, CapabilityHandler, Permissions, Registry, RelayPolicy};
use super::discovery::{Discovery, MemoryLan};
use super::endpoint::*;
use super::identity::DeviceIdentity;
use super::memory::MemoryNet;
use super::pairing::*;
use super::trust::{KeyStoreSigner, MemoryTrustStorage, TrustList, TrustStorage};
use super::P2p;
use crate::secrets::{MemoryBackend, SecretBackend};
use crate::team::keystore::KeyStore;

/// Counts how often a connection reached the (pretend) mouse-and-keyboard capability.
struct Probe(AtomicUsize);

#[async_trait]
impl CapabilityHandler for Probe {
    async fn serve(&self, _peer: NodeId, link: Arc<dyn Link>) {
        self.0.fetch_add(1, Ordering::SeqCst);
        link.closed().await;
    }
}

struct Machine {
    name: String,
    id: NodeId,
    keys: KeyStore,
    storage: Arc<MemoryTrustStorage>,
    p2p: Arc<P2p>,
    trust: Arc<TrustList>,
    mgr: Arc<PairManager>,
    probe: Arc<Probe>,
}

struct World {
    net: MemoryNet,
    lan: MemoryLan,
    clock: Arc<ManualClock>,
}

impl World {
    fn new() -> Self {
        Self { net: MemoryNet::new(), lan: MemoryLan::new(), clock: ManualClock::new(1_000_000) }
    }

    fn machine(&self, name: &str) -> Machine {
        let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
        let keys = KeyStore::with_backend(backend, name);
        self.machine_with(name, keys, MemoryTrustStorage::new())
    }

    fn machine_with(&self, name: &str, keys: KeyStore, storage: Arc<MemoryTrustStorage>) -> Machine {
        let id = DeviceIdentity::load(&keys).unwrap().id();
        let probe = Arc::new(Probe(AtomicUsize::new(0)));
        let registry = Arc::new(Registry::new());
        registry.register(Capability::new(capability::DESK_INPUT, alpn::DESK, RelayPolicy::Never, probe.clone())).unwrap();
        let trust = TrustList::open(storage.clone(), Arc::new(KeyStoreSigner::new(keys.clone())));
        let p2p = Arc::new(P2p::new(keys.clone(), self.net.factory(), registry, trust.clone()));
        p2p.apply_settings(&json!({ "desk_enabled": true }));
        let mgr = PairManager::new(p2p.clone(), trust.clone(), keys.clone(), self.clock.clone(), Arc::new(self.lan.clone()), name).unwrap();
        Machine { name: name.to_string(), id, keys, storage, p2p, trust, mgr, probe }
    }
}

impl Machine {
    /// The app is closed and opened again: same keys, same stored list, new objects.
    fn restart(&self, world: &World) -> Machine {
        self.p2p.stop();
        world.machine_with(&self.name, self.keys.clone(), self.storage.clone())
    }

    async fn host(&self) -> (String, String) {
        let v = self.mgr.start_hosting().await.unwrap();
        (v["hosting"]["ticket"].as_str().unwrap().to_string(), v["hosting"]["code"].as_str().unwrap().to_string())
    }

    /// Answers the next question that shows up on this computer. Returns the code the sheet showed.
    fn answer_next(&self, allow: bool) -> JoinHandle<Option<String>> {
        let mgr = self.mgr.clone();
        tokio::spawn(async move {
            for _ in 0..1000 {
                let p = mgr.view()["pending"].clone();
                if let Some(id) = p["id"].as_u64() {
                    let code = p["code"].as_str().map(str::to_string);
                    mgr.answer(id, allow).unwrap();
                    return code;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            None
        })
    }

    fn asked(&self) -> bool {
        self.mgr.view()["pending"]["id"].is_u64()
    }
}

async fn pair(host: &Machine, joiner: &Machine) -> Result<String, PairError> {
    let (ticket, code) = host.host().await;
    let h = host.answer_next(true);
    let j = joiner.answer_next(true);
    let r = joiner.mgr.join(&ticket, &code).await;
    let _ = h.await;
    let _ = j.await;
    r
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for {what}");
}

fn nothing_trusted(m: &Machine) {
    assert!(m.trust.list().is_empty(), "{} trusts someone", m.name);
}

// ----- the happy path -----

#[tokio::test]
async fn two_computers_pair_with_a_code_and_both_confirm() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    assert!(ticket.starts_with("pm1:") && code.len() == 9 && code.as_bytes()[4] == b'-');
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    let joined = b.mgr.join(&ticket, &code).await.unwrap();
    assert_eq!(joined, "Ren-PC");
    // Both sheets showed the same code.
    assert_eq!(ha.await.unwrap(), Some(code.clone()));
    assert_eq!(hb.await.unwrap(), Some(code));
    let (on_a, on_b) = (a.trust.list(), b.trust.list());
    assert_eq!((on_a.len(), on_b.len()), (1, 1));
    assert_eq!((on_a[0].node, on_a[0].name.as_str()), (b.id, "Laptop"));
    assert_eq!((on_b[0].node, on_b[0].name.as_str()), (a.id, "Ren-PC"));
    // Everything starts off.
    assert!(on_a[0].allowed.is_empty() && on_b[0].allowed.is_empty());
    for cap in capability::PERMISSIONABLE {
        assert!(!a.trust.allowed(&b.id, cap) && !b.trust.allowed(&a.id, cap));
    }
    // The code is gone and pairing is closed again.
    assert!(a.mgr.view()["hosting"].is_null());
    assert!(!a.trust.pairing_open());
}

// ----- the code -----

#[tokio::test]
async fn a_wrong_code_is_refused_and_the_right_one_still_works() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    let other = if code == "AAAA-AAAA" { "BBBB-BBBB" } else { "AAAA-AAAA" };
    assert_eq!(b.mgr.join(&ticket, other).await, Err(PairError::WrongCode));
    nothing_trusted(&a);
    nothing_trusted(&b);
    assert!(!a.asked() && !b.asked(), "no sheet may open for a wrong code");
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    assert!(b.mgr.join(&ticket, &code).await.is_ok());
    let _ = (ha.await, hb.await);
    assert_eq!((a.trust.list().len(), b.trust.list().len()), (1, 1));
}

#[tokio::test]
async fn a_malformed_code_is_caught_before_anything_is_sent() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, _) = a.host().await;
    assert_eq!(b.mgr.join(&ticket, "12").await, Err(PairError::BadCode));
    assert_eq!(b.mgr.join("garbage", "ABCD-EFGH").await, Err(PairError::BadTicket));
    assert_eq!(normalize_code("4f7q 9k2m").as_deref(), Some("4F7Q9K2M"));
    assert_eq!(normalize_code("4F7Q-9K2O"), None, "no letter O");
}

#[tokio::test]
async fn an_expired_code_is_refused_and_withdrawn() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    w.clock.advance(CODE_TTL_MS + 1);
    assert!(a.mgr.view()["hosting"].is_null(), "an old code is not shown");
    assert_eq!(b.mgr.join(&ticket, &code).await, Err(PairError::Refused));
    nothing_trusted(&a);
    nothing_trusted(&b);
    assert!(!a.trust.pairing_open(), "pairing closed itself");
    assert!(w.lan.nearby().is_empty(), "and stopped announcing");
}

#[tokio::test]
async fn a_code_works_once() {
    let w = World::new();
    let (a, b, c) = (w.machine("Ren-PC"), w.machine("Laptop"), w.machine("Stranger"));
    let (ticket, code) = a.host().await;
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    b.mgr.join(&ticket, &code).await.unwrap();
    let _ = (ha.await, hb.await);
    // Somebody who saw the code afterwards gets nowhere.
    assert!(c.mgr.join(&ticket, &code).await.is_err());
    nothing_trusted(&c);
    assert!(!a.trust.is_trusted(&c.id));
    assert!(!a.asked());
}

#[tokio::test]
async fn nobody_can_join_when_no_code_is_shown() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    a.mgr.cancel_hosting();
    assert!(b.mgr.join(&ticket, &code).await.is_err());
    assert!(w.lan.nearby().is_empty());
    nothing_trusted(&a);
    assert!(a.p2p.refusals().iter().any(|r| r.peer == b.id && r.capability.as_deref() == Some("pair") && r.kind == RefusalKind::NotAllowed));
}

// ----- the two answers -----

#[tokio::test]
async fn when_the_host_says_no_nothing_is_trusted_on_either_side() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    let (ha, hb) = (a.answer_next(false), b.answer_next(true));
    assert_eq!(b.mgr.join(&ticket, &code).await, Err(PairError::TheyDeclined));
    let _ = (ha.await, hb.await);
    nothing_trusted(&a);
    nothing_trusted(&b);
}

#[tokio::test]
async fn when_the_joiner_says_no_nothing_is_trusted_on_either_side() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    let (ha, hb) = (a.answer_next(true), b.answer_next(false));
    assert_eq!(b.mgr.join(&ticket, &code).await, Err(PairError::YouDeclined));
    let _ = (ha.await, hb.await);
    nothing_trusted(&a);
    nothing_trusted(&b);
}

#[tokio::test]
async fn nothing_is_trusted_and_no_input_is_delivered_before_both_people_confirm() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, code) = a.host().await;
    a.p2p.want(capability::DESK_INPUT).await.unwrap();
    let join = {
        let (b_mgr, ticket) = (b.mgr.clone(), ticket.clone());
        tokio::spawn(async move { b_mgr.join(&ticket, &code).await })
    };
    wait_for("both sheets", || a.asked() && b.asked()).await;
    // Both sheets are open and nobody has answered: nothing is trusted, nothing is allowed.
    nothing_trusted(&a);
    nothing_trusted(&b);
    let _ = b.p2p.dial(&PeerAddr::new(a.id), capability::DESK_INPUT).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(a.probe.0.load(Ordering::SeqCst), 0, "input capability was reached before pairing");
    assert!(a.p2p.refusals().iter().any(|r| r.peer == b.id && r.kind == RefusalKind::NotAllowed));
    // One yes is not enough.
    let id = a.mgr.view()["pending"]["id"].as_u64().unwrap();
    a.mgr.answer(id, true).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    nothing_trusted(&a);
    nothing_trusted(&b);
    // The second yes finishes it.
    let id = b.mgr.view()["pending"]["id"].as_u64().unwrap();
    b.mgr.answer(id, true).unwrap();
    join.await.unwrap().unwrap();
    wait_for("trust on the host", || a.trust.is_trusted(&b.id)).await;
    // Paired, but still nothing allowed: the capability is refused until the person turns it on.
    let _ = b.p2p.dial(&PeerAddr::new(a.id), capability::DESK_INPUT).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(a.probe.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_question_is_answered_only_by_its_id() {
    let w = World::new();
    let a = w.machine("Ren-PC");
    assert_eq!(a.mgr.answer(1, true), Err(PairError::NoSuchQuestion));
    assert_eq!(a.mgr.answer(0, false), Err(PairError::NoSuchQuestion));
}

// ----- limits -----

#[tokio::test]
async fn three_wrong_codes_lock_pairing_for_ten_minutes() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ticket, _) = a.host().await;
    for _ in 0..3 {
        assert_eq!(b.mgr.join(&ticket, "ZZZZ-ZZZZ").await, Err(PairError::WrongCode));
    }
    // Both computers now refuse, and the shown code is gone.
    assert!(matches!(b.mgr.join(&ticket, "ZZZZ-ZZZZ").await, Err(PairError::Locked(_))));
    assert!(matches!(a.mgr.start_hosting().await, Err(PairError::Locked(_))));
    assert!(a.mgr.view()["hosting"].is_null() && a.mgr.view()["locked_until"].is_u64());
    nothing_trusted(&a);
    w.clock.advance(LOCKOUT_MS + 1);
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    assert!(pair(&a, &b).await.is_ok(), "after ten minutes pairing works again");
    let _ = (ha.await, hb.await);
}

#[tokio::test]
async fn three_no_answers_lock_pairing_for_ten_minutes() {
    let w = World::new();
    let a = w.machine("Ren-PC");
    for i in 0..3 {
        let b = w.machine(&format!("Stranger{i}"));
        let (ticket, code) = a.host().await;
        let (ha, hb) = (a.answer_next(false), b.answer_next(true));
        assert_eq!(b.mgr.join(&ticket, &code).await, Err(PairError::TheyDeclined));
        let _ = (ha.await, hb.await);
    }
    assert!(matches!(a.mgr.start_hosting().await, Err(PairError::Locked(_))));
    nothing_trusted(&a);
    w.clock.advance(LOCKOUT_MS + 1);
    assert!(a.mgr.start_hosting().await.is_ok());
}

// ----- identity -----

/// Plays a joiner by hand: connects as `claimed` and sends a hello with `binding`. Returns the first frame back.
async fn raw_hello(w: &World, host: &Machine, link_id: NodeId, binding: &[(&str, Value)]) -> Value {
    let raw = w.net.bind_raw(link_id, &[]);
    let link = raw.dial(&PeerAddr::new(host.id), alpn::PAIR).await.unwrap();
    let mut s = link.open_bi().await.unwrap();
    let mut b = json!({ "node": link_id.to_hex(), "device": "00".repeat(32), "issued": 1, "sig": "00".repeat(64) });
    for (k, v) in binding {
        b[*k] = v.clone();
    }
    let hello = json!({ "t": "hello", "nonce": "00".repeat(16), "name": "Evil", "binding": b }).to_string().into_bytes();
    s.send.write_all(&(hello.len() as u32).to_be_bytes()).await.unwrap();
    s.send.write_all(&hello).await.unwrap();
    let mut len = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), s.recv.read_exact(&mut len)).await.unwrap().unwrap();
    let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
    s.recv.read_exact(&mut buf).await.unwrap();
    serde_json::from_slice(&buf).unwrap()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[tokio::test]
async fn a_tampered_or_borrowed_binding_is_refused() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    a.host().await;
    let real = DeviceIdentity::load(&b.keys).unwrap().binding();

    // The signature is damaged.
    let mut sig = real.signature;
    sig[0] ^= 1;
    let r = raw_hello(
        &w,
        &a,
        b.id,
        &[("device", json!(hex(&real.device_pub))), ("issued", json!(real.issued_at)), ("sig", json!(hex(&sig)))],
    )
    .await;
    assert_eq!(r["t"], "refused");

    // A genuine binding, but of another node than the one this link was made with.
    let c = w.machine("Other");
    let r = raw_hello(
        &w,
        &a,
        c.id,
        &[
            ("node", json!(real.node_id.to_hex())),
            ("device", json!(hex(&real.device_pub))),
            ("issued", json!(real.issued_at)),
            ("sig", json!(hex(&real.signature))),
        ],
    )
    .await;
    assert_eq!(r["t"], "refused");
    nothing_trusted(&a);
    assert!(!a.asked());
    // The host stayed open for the real thing, and the bad tries did not count as wrong codes.
    assert!(a.mgr.view()["hosting"].is_object() && a.mgr.view()["locked_until"].is_null());
}

// ----- the trust list -----

#[tokio::test]
async fn the_trust_list_survives_a_restart_with_its_permissions() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    let (ticket, code) = a.host().await;
    b.mgr.join(&ticket, &code).await.unwrap();
    let _ = (ha.await, hb.await);
    a.trust.set_permission(&b.id, capability::FILES, true).unwrap();
    let before = a.trust.list();
    let a2 = a.restart(&w);
    assert_eq!(a2.trust.list(), before);
    assert!(!a2.trust.tampered());
    assert!(a2.trust.allowed(&b.id, capability::FILES));
    assert!(!a2.trust.allowed(&b.id, capability::DESK_INPUT));
}

#[tokio::test]
async fn an_edited_trust_list_is_detected_and_not_used() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    pair_quick(&a, &b).await;
    let bytes = a.storage.load().unwrap();
    // Somebody edits the stored file to give the laptop more rights.
    let edited = String::from_utf8(bytes.clone()).unwrap().replace("\"allowed\":[]", "\"allowed\":[\"desk/input\"]");
    assert_ne!(edited.as_bytes(), bytes.as_slice());
    a.storage.put(edited.into_bytes());
    let a2 = a.restart(&w);
    assert!(a2.trust.tampered());
    assert!(a2.trust.list().is_empty());
    assert!(!a2.trust.allowed(&b.id, capability::DESK_INPUT));
    // Renaming is caught too, and so is a cut-short file.
    a.storage.put(String::from_utf8(bytes.clone()).unwrap().replace("Laptop", "Mallory").into_bytes());
    assert!(a.restart(&w).trust.tampered());
    a.storage.put(bytes[..bytes.len() / 2].to_vec());
    assert!(a.restart(&w).trust.tampered());
}

#[tokio::test]
async fn a_list_copied_from_another_computer_is_not_accepted() {
    let w = World::new();
    let (a, b, c) = (w.machine("Ren-PC"), w.machine("Laptop"), w.machine("Tower"));
    pair_quick(&a, &b).await;
    // The tower is given a's perfectly signed file.
    c.storage.put(a.storage.load().unwrap());
    let c2 = c.restart(&w);
    assert!(c2.trust.tampered() && c2.trust.list().is_empty());
}

#[tokio::test]
async fn removing_a_computer_forgets_it_and_its_rights() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    pair_quick(&a, &b).await;
    a.trust.set_permission(&b.id, capability::DESK_INPUT, true).unwrap();
    assert!(a.trust.allowed(&b.id, capability::DESK_INPUT));
    a.mgr.remove(&b.id).unwrap();
    assert!(a.trust.list().is_empty());
    assert!(!a.trust.allowed(&b.id, capability::DESK_INPUT));
    assert!(a.mgr.remove(&b.id).is_err());
    assert!(a.restart(&w).trust.list().is_empty(), "the removal was saved");
    // The laptop cannot reach the input capability any more.
    a.p2p.want(capability::DESK_INPUT).await.unwrap();
    let _ = b.p2p.dial(&PeerAddr::new(a.id), capability::DESK_INPUT).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(a.probe.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn permissions_start_off_and_only_the_person_turns_them_on() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    pair_quick(&a, &b).await;
    assert!(a.p2p.want(capability::DESK_INPUT).await.is_ok());
    b.p2p.apply_settings(&json!({ "desk_enabled": true }));
    // Off: refused before the handler.
    let _ = b.p2p.dial(&PeerAddr::new(a.id), capability::DESK_INPUT).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(a.probe.0.load(Ordering::SeqCst), 0);
    assert!(a.p2p.refusals().iter().any(|r| r.peer == b.id && r.kind == RefusalKind::NotAllowed));
    // On: reaches it.
    a.mgr.set_permission(&b.id, capability::DESK_INPUT, true).unwrap();
    let _link = b.p2p.dial(&PeerAddr::new(a.id), capability::DESK_INPUT).await.unwrap();
    wait_for("the handler", || a.probe.0.load(Ordering::SeqCst) == 1).await;
    // Pairing is not something to switch on, and strangers cannot be given rights.
    assert!(a.trust.set_permission(&b.id, capability::PAIR, true).is_err());
    assert!(a.trust.set_permission(&NodeId([9; 32]), capability::FILES, true).is_err());
    a.trust.set(&NodeId([9; 32]), capability::FILES, true);
    assert!(!a.trust.allowed(&NodeId([9; 32]), capability::FILES));
}

async fn pair_quick(a: &Machine, b: &Machine) {
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    let (ticket, code) = a.host().await;
    b.mgr.join(&ticket, &code).await.unwrap();
    let _ = (ha.await, hb.await);
}

// ----- found nearby -----

#[tokio::test]
async fn found_nearby_lists_only_computers_that_show_a_code_and_can_be_joined() {
    let w = World::new();
    let (a, b) = (w.machine("Ren-PC"), w.machine("Laptop"));
    assert!(b.mgr.nearby().as_array().unwrap().is_empty(), "nothing is announced while idle");
    let (_, code) = a.host().await;
    let near = b.mgr.nearby();
    assert_eq!(near[0]["name"], "Ren-PC");
    assert!(a.mgr.nearby().as_array().unwrap().is_empty(), "a computer does not find itself");
    let node = near[0]["node"].as_str().unwrap().to_string();
    let (ha, hb) = (a.answer_next(true), b.answer_next(true));
    b.mgr.join(&node, &code).await.unwrap();
    let _ = (ha.await, hb.await);
    assert!(w.lan.nearby().is_empty(), "withdrawn once the code is used");
    // A paired computer is not offered again.
    let (_, _) = a.host().await;
    assert!(b.mgr.nearby().as_array().unwrap().is_empty());
    // A node that is not nearby cannot be joined.
    assert_eq!(b.mgr.join(&"ab".repeat(32), "ABCD-EFGH").await, Err(PairError::NotNearby));
}

#[tokio::test]
async fn the_ticket_round_trips() {
    let id = NodeId([7; 32]);
    let addrs: Vec<std::net::SocketAddr> = vec!["192.168.1.5:4000".parse().unwrap(), "[fe80::1]:4001".parse().unwrap()];
    let t = make_ticket(&id, &addrs);
    assert_eq!(parse_ticket(&t), Some(PeerAddr { id, direct: addrs }));
    assert_eq!(parse_ticket("pm1:zz"), None);
    assert_eq!(parse_ticket(&format!("pm1:{}@notanaddress", id.to_hex())), None);
}
