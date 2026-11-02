//! The desk link on the in-memory net with a pretend PC. No socket, no hook, no real pointer, no real clipboard, no
//! real key store. Time is simulated (`start_paused`): 30 seconds of motion take no wall-clock time.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desk_core::Side;
use desk_proto::{OsKind, StreamMsg, PROTO_VERSION};
use serde_json::json;
use tokio::time::{sleep, Instant};

use super::clipboard::{self, Assembler, ClipError, CAP, CHUNK};
use super::mock::MockRemote;
use super::remote::{Capture, ClipItem, Inject, RemoteHost};
use super::wire::{read_frame, write_frame, write_kind, CONTROL_FRAME_MAX, KIND_CONTROL};
use super::{DeskLink, MemLinkStore, CLOSE_PROTOCOL};
use crate::p2p::alpn;
use crate::p2p::capability::{self, Registry};
use crate::p2p::endpoint::{BiStream, Link, NodeId, PeerAddr, RefusalKind, Transport};
use crate::p2p::identity::DeviceIdentity;
use crate::p2p::memory::MemoryNet;
use crate::p2p::trust::{KeyStoreSigner, MemoryTrustStorage, TrustList};
use crate::p2p::P2p;
use crate::secrets::{MemoryBackend, SecretBackend};
use crate::team::keystore::KeyStore;

// ----- the world -----

struct Pc {
    name: String,
    id: NodeId,
    device_pub: [u8; 32],
    trust: Arc<TrustList>,
    p2p: Arc<P2p>,
    link: Arc<DeskLink>,
    host: MockRemote,
}

fn pc(net: &MemoryNet, name: &str) -> Pc {
    let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
    let keys = KeyStore::with_backend(backend, name);
    let ident = DeviceIdentity::load(&keys).unwrap();
    let trust = TrustList::open(MemoryTrustStorage::new(), Arc::new(KeyStoreSigner::new(keys.clone())));
    let p2p = Arc::new(P2p::new(keys, net.factory(), Arc::new(Registry::new()), trust.clone()));
    p2p.apply_settings(&json!({ "desk_enabled": true }));
    let host = MockRemote::standard();
    let link = DeskLink::new(p2p.clone(), trust.clone(), Arc::new(host.clone()), Arc::new(MemLinkStore::default()), name).unwrap();
    link.apply_settings(true);
    Pc { name: name.to_string(), id: ident.id(), device_pub: ident.binding().device_pub, trust, p2p, link, host }
}

fn pair(a: &Pc, b: &Pc) {
    a.trust.add(b.id, b.device_pub, &b.name, 1).unwrap();
    b.trust.add(a.id, a.device_pub, &a.name, 1).unwrap();
}

fn allow(a: &Pc, b: &Pc, caps: &[&str]) {
    for c in caps {
        a.trust.set_permission(&b.id, c, true).unwrap();
    }
}

const ALL: [&str; 2] = [capability::DESK_INPUT, capability::DESK_CLIPBOARD];

/// Two paired computers with both switches on for each other.
fn together() -> (MemoryNet, Pc, Pc) {
    let net = MemoryNet::new();
    let (a, b) = (pc(&net, "Ren-PC"), pc(&net, "Laptop"));
    pair(&a, &b);
    allow(&a, &b, &ALL);
    allow(&b, &a, &ALL);
    (net, a, b)
}

async fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if f() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn state_of(p: &Pc, other: &Pc) -> &'static str {
    p.link.view().devices.iter().find(|d| d.node == other.id.to_hex()).map_or("none", |d| d.state)
}

async fn up(a: &Pc, b: &Pc) {
    a.link.start();
    b.link.start();
    wait_until("both computers online", || state_of(a, b) == "online" && state_of(b, a) == "online").await;
    // The endpoints listen for exactly what some switch wants.
    for p in [a, b] {
        let clip = p.trust.list().iter().any(|d| d.allowed.contains(capability::DESK_CLIPBOARD));
        wait_until("endpoint listens", || *p.link.inner.wanted.lock().unwrap() == (true, clip)).await;
    }
}

/// The pointer rests on a's right edge long enough to cross, and b has taken it.
async fn cross(a: &Pc, b: &Pc) {
    let (_, y) = a.host.cursor();
    a.host.put_cursor(1919, y);
    wait_until("the pointer crosses", || a.host.capturing() && b.link.view().devices[0].controls_me).await;
}

fn big(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

fn png(n: usize, seed: u8) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend(big(n.saturating_sub(8), seed));
    v
}

// ----- coming up -----

#[tokio::test(start_paused = true)]
async fn two_paired_computers_link_up_and_say_so() {
    let (_net, a, b) = together();
    assert_eq!(state_of(&a, &b), "offline");
    up(&a, &b).await;
    let v = a.link.view();
    assert_eq!(v.devices[0].name, "Laptop");
    assert!(v.sharing && v.supported);
    assert_eq!(v.tray, Some("Mouse sharing is on"));
    assert_eq!(v.admin_note, "Can't control windows that run as admin.");
    assert_eq!(v.mouse_on, None);
}

#[tokio::test(start_paused = true)]
async fn nothing_is_dialed_while_the_switch_is_off() {
    let net = MemoryNet::new();
    let (a, b) = (pc(&net, "Ren-PC"), pc(&net, "Laptop"));
    pair(&a, &b);
    // Only the other side has it on.
    allow(&b, &a, &ALL);
    a.link.start();
    b.link.start();
    sleep(Duration::from_secs(3)).await;
    assert_eq!(state_of(&a, &b), "off");
    assert!(!a.p2p.is_running(), "no switch wants the network: the endpoint is not even listening");
    // The other side's dial is refused by the permission check, and says so to the person.
    assert!(a.p2p.refusals().is_empty());
    assert!(b.p2p.refusals().is_empty());
    assert_ne!(state_of(&b, &a), "online");
}

#[tokio::test(start_paused = true)]
async fn a_computer_that_may_not_is_refused_before_any_handler_sees_it() {
    let net = MemoryNet::new();
    let (a, b) = (pc(&net, "Ren-PC"), pc(&net, "Laptop"));
    pair(&a, &b);
    // b turned it on for a; a never turned it on for b.
    allow(&b, &a, &ALL);
    allow(&a, &b, &[capability::FILES]);
    a.link.start();
    b.link.start();
    a.p2p.want(capability::DESK_INPUT).await.unwrap();
    wait_until("b is turned away", || a.p2p.refusals().iter().any(|r| r.kind == RefusalKind::NotAllowed)).await;
    assert_ne!(state_of(&b, &a), "online");
    assert_ne!(state_of(&a, &b), "online");
}

#[tokio::test(start_paused = true)]
async fn a_stranger_cannot_reach_the_input_capability() {
    let net = MemoryNet::new();
    let a = pc(&net, "Ren-PC");
    let b = pc(&net, "Laptop");
    pair(&a, &b);
    allow(&a, &b, &ALL);
    a.link.start();
    wait_until("a listens", || a.p2p.is_running()).await;
    let stranger = NodeId([0x5A; 32]);
    let t = net.bind_raw(stranger, &[alpn::DESK]);
    let link = t.dial(&PeerAddr::new(a.id), alpn::DESK).await.unwrap();
    assert_eq!(link.closed().await, crate::p2p::endpoint::CLOSE_NOT_ALLOWED);
    assert!(a.p2p.refusals().iter().any(|r| r.peer == stranger && r.kind == RefusalKind::NotAllowed));
}

#[tokio::test(start_paused = true)]
async fn both_computers_dialing_at_once_settle_on_one_link_and_stay() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    // A third and fourth link, dialed on purpose from both sides while the session is live (duplicate dial).
    for (x, y) in [(&a, &b), (&b, &a)] {
        let inner = x.link.inner.clone();
        let peer = y.id;
        let link = x.p2p.dial(&PeerAddr::new(peer), capability::DESK_INPUT).await.unwrap();
        tokio::spawn(super::io::run_link(inner, peer, link, true));
    }
    for _ in 0..50 {
        sleep(Duration::from_millis(20)).await;
        assert_eq!(state_of(&a, &b), "online", "a duplicate link must never take the session down");
        assert_eq!(state_of(&b, &a), "online");
    }
    // And the session still works end to end.
    cross(&a, &b).await;
    assert!(a.host.person(Capture::Key { hid: 0x04, down: true }));
    wait_until("key arrives", || b.host.held_keys().contains(&0x04)).await;
}

// ----- crossing -----

/// A stray link that shows up after the first moments (a restarted dial, a duplicate) must never take the pointer
/// away from the computer that has it. The key order of the two computers is random, so this runs both ways.
#[tokio::test(start_paused = true)]
async fn a_stray_duplicate_link_never_takes_the_pointer_away() {
    for _ in 0..6 {
        let (_net, a, b) = together();
        up(&a, &b).await;
        sleep(Duration::from_millis(2_500)).await; // the session is settled
        cross(&a, &b).await;
        for (x, y) in [(&a, &b), (&b, &a)] {
            let inner = x.link.inner.clone();
            let peer = y.id;
            let link = x.p2p.dial(&PeerAddr::new(peer), capability::DESK_INPUT).await.unwrap();
            let stray = link.clone();
            tokio::spawn(super::io::run_link(inner, peer, link, true));
            // The stray is turned away, not the session.
            assert_eq!(stray.closed().await, super::CLOSE_DUPLICATE);
        }
        assert!(a.host.capturing() && b.link.view().devices[0].controls_me, "the stray link took the pointer away");
        a.host.person(Capture::Key { hid: 0x04, down: true });
        wait_until("key arrives", || b.host.held_keys().contains(&0x04)).await;
    }
}

/// Both people push at their edges in the same moment: each side says no to the other and both come home.
#[tokio::test(start_paused = true)]
async fn two_pointers_crossing_at_once_both_come_home() {
    let (_net, a, b) = together();
    b.link.set_side(&a.id, "left").unwrap();
    up(&a, &b).await;
    a.host.put_cursor(1919, 540);
    b.host.put_cursor(0, 540);
    sleep(Duration::from_millis(800)).await;
    assert!(!a.host.capturing() && !b.host.capturing(), "neither may be left holding the mouse");
    assert!(a.link.view().mouse_on.is_none() && b.link.view().mouse_on.is_none());
    assert!(a.host.held_keys().is_empty() && b.host.held_keys().is_empty());
    // And a normal crossing still works afterwards.
    a.host.put_cursor(1919, 300);
    wait_until("crosses", || a.host.capturing()).await;
}

#[tokio::test(start_paused = true)]
async fn the_pointer_crosses_the_edge_and_the_other_computer_is_driven() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(200)).await;
    assert!(!a.host.capturing(), "the middle of the screen is not an edge");
    cross(&a, &b).await;
    // It appears just inside b's left edge at the same relative height; b says who has the mouse.
    let (bx, by) = b.host.cursor();
    assert!((bx - 3).abs() <= 1 && (by - 540).abs() <= 2, "{bx},{by}");
    assert_eq!(a.link.view().mouse_on.as_deref(), Some("Laptop"));
    assert_eq!(b.link.view().controlled_by.as_deref(), Some("Ren-PC"));
    // Motion, keys, buttons and wheel arrive.
    a.host.person(Capture::Motion { dx: 40, dy: 10 });
    a.host.person(Capture::Key { hid: 0x04, down: true });
    a.host.person(Capture::Button { button: 0, down: true });
    a.host.person(Capture::Wheel { dx: 0, dy: -120 });
    wait_until("input arrives", || b.host.held_keys().contains(&0x04) && b.host.cursor().0 == bx + 40).await;
    assert!(b.host.with(|s| s.held_buttons.contains(&0)));
    assert!(b.host.with(|s| s.log.contains(&Inject::Wheel { dx: 0, dy: -120 })));
    a.host.person(Capture::Key { hid: 0x04, down: false });
    a.host.person(Capture::Button { button: 0, down: false });
    wait_until("released", || b.host.held_keys().is_empty()).await;
}

#[tokio::test(start_paused = true)]
async fn pushing_back_through_the_edge_brings_the_pointer_home_at_the_same_height() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    // Move left across b; nothing happens until the pointer rests on the edge and pushes on.
    for _ in 0..3 {
        a.host.person(Capture::Motion { dx: 30, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    assert!(a.host.capturing());
    for _ in 0..8 {
        a.host.person(Capture::Motion { dx: -30, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    wait_until("pointer home", || !a.host.capturing()).await;
    let (x, y) = a.host.cursor();
    assert!(x >= 1900 && (y - 300).abs() <= 3, "came back to {x},{y}");
    assert_eq!(a.link.view().mouse_on, None);
    assert_eq!(b.link.view().controlled_by, None);
    // It does not cross again by itself: it is inside the edge.
    sleep(Duration::from_millis(300)).await;
    assert!(!a.host.capturing());
}

#[tokio::test(start_paused = true)]
async fn the_layout_decides_which_edge_crosses() {
    let (_net, a, b) = together();
    a.link.set_side(&b.id, "left").unwrap();
    assert!(a.link.set_side(&b.id, "sideways").is_err());
    up(&a, &b).await;
    a.host.put_cursor(1919, 540);
    sleep(Duration::from_millis(200)).await;
    assert!(!a.host.capturing(), "b is on the left now");
    a.host.put_cursor(0, 540);
    wait_until("left edge crosses", || a.host.capturing()).await;
    // b is entered from its right side.
    assert!(b.host.cursor().0 >= 1900);
    assert_eq!(a.link.view().devices[0].side, "left");
}

#[tokio::test(start_paused = true)]
async fn a_held_button_does_not_carry_a_drag_over_the_edge() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.with(|s| s.buttons_down = true);
    a.host.put_cursor(1919, 540);
    sleep(Duration::from_millis(300)).await;
    assert!(!a.host.capturing());
    a.host.with(|s| s.buttons_down = false);
    wait_until("crosses after release", || a.host.capturing()).await;
}

#[tokio::test(start_paused = true)]
async fn a_paused_computer_refuses_the_pointer_and_the_sender_comes_home() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    b.link.pause(true);
    sleep(Duration::from_millis(100)).await;
    a.host.put_cursor(1919, 540);
    // a takes the pointer, b says no, a is home again, and b was never driven.
    sleep(Duration::from_millis(400)).await;
    assert!(!a.host.capturing());
    assert!(b.host.with(|s| s.log.is_empty()));
    assert!(!b.link.view().devices[0].controls_me);
}

// ----- safety -----

#[tokio::test(start_paused = true)]
async fn a_held_key_is_released_at_once_when_the_link_is_cut() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Key { hid: 0xE0, down: true });
    a.host.person(Capture::Key { hid: 0x06, down: true });
    wait_until("held on b", || b.host.held_keys().len() == 2).await;
    let t0 = Instant::now();
    a.link.stop(); // closes every link
    wait_until("released", || b.host.held_keys().is_empty()).await;
    for (_, at) in b.host.with(|s| s.key_up_at.clone()) {
        assert!(at.duration_since(t0) <= Duration::from_millis(300), "released after {:?}", at.duration_since(t0));
    }
    assert!(!a.host.capturing(), "the sender's pointer is home too");
}

#[tokio::test(start_paused = true)]
async fn a_pulled_cable_releases_held_keys_within_300_ms() {
    let (net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Key { hid: 0xE2, down: true });
    a.host.person(Capture::Key { hid: 0x2B, down: true });
    a.host.person(Capture::Button { button: 0, down: true });
    wait_until("held on b", || b.host.held_keys().len() == 2).await;
    sleep(Duration::from_millis(130)).await;
    let t0 = Instant::now();
    net.blackhole(a.id, b.id, true); // the cable
    wait_until("released", || b.host.held_keys().is_empty() && b.host.with(|s| s.held_buttons.is_empty())).await;
    let ups = b.host.with(|s| s.key_up_at.clone());
    assert_eq!(ups.len(), 2);
    for (hid, at) in ups {
        assert!(at.duration_since(t0) <= Duration::from_millis(300), "key {hid:#x} released after {:?}", at.duration_since(t0));
    }
    // The sender's pointer comes home on its own too, with nobody to ask.
    wait_until("sender home", || !a.host.capturing()).await;
    assert!(Instant::now().duration_since(t0) <= Duration::from_millis(400));
    // The session is still there (not dead yet), then dies after 3 s of silence.
    assert_eq!(state_of(&b, &a), "online");
    wait_until("offline after the silence", || state_of(&b, &a) != "online").await;
}

#[tokio::test(start_paused = true)]
async fn the_panic_key_brings_the_pointer_home_and_releases_everything() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 700);
    wait_until("crossed", || a.host.capturing()).await;
    a.host.person(Capture::Key { hid: 0xE0, down: true });
    a.host.person(Capture::Key { hid: 0xE2, down: true });
    a.host.person(Capture::Key { hid: 0x04, down: true });
    wait_until("held on b", || b.host.held_keys().len() == 3).await;
    a.host.person(Capture::Key { hid: 0x29, down: true }); // Ctrl+Alt+Esc
    wait_until("home", || !a.host.capturing()).await;
    wait_until("released on b", || b.host.held_keys().is_empty()).await;
    let (x, y) = a.host.cursor();
    assert!(x > 1800 && (y - 700).abs() <= 2, "{x},{y}");
    assert!(!b.link.view().devices[0].controls_me);
    // Esc itself was never sent.
    assert!(!b.host.with(|s| s.log.iter().any(|e| matches!(e, Inject::Key { hid: 0x29, down: true }))));
}

#[tokio::test(start_paused = true)]
async fn scroll_lock_pauses_and_the_pointer_stays_home_until_resumed() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Key { hid: 0x47, down: true });
    wait_until("home", || !a.host.capturing()).await;
    assert!(a.link.view().paused);
    a.host.put_cursor(1919, 540);
    sleep(Duration::from_millis(300)).await;
    assert!(!a.host.capturing(), "paused: no crossing");
    a.link.pause(false);
    wait_until("crosses again", || a.host.capturing()).await;
    assert!(!a.link.view().paused);
}

#[tokio::test(start_paused = true)]
async fn quitting_brings_the_pointer_home_and_lets_go_of_everything() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 420);
    wait_until("crossed", || a.host.capturing()).await;
    a.host.person(Capture::Key { hid: 0x04, down: true });
    wait_until("held", || !b.host.held_keys().is_empty()).await;
    a.link.shutdown();
    // Synchronously, before anything else runs: the pointer is home and the keyboard is back.
    assert!(!a.host.capturing());
    let (x, _) = a.host.cursor();
    assert!(x < 1919);
    wait_until("b released", || b.host.held_keys().is_empty()).await;
}

#[tokio::test(start_paused = true)]
async fn turning_labs_off_is_the_kill_switch() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.link.apply_settings(false);
    assert!(!a.host.capturing());
    wait_until("b sees it end", || state_of(&b, &a) != "online").await;
    assert!(b.host.held_keys().is_empty());
}

#[tokio::test(start_paused = true)]
async fn turning_the_switch_off_closes_the_open_link() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Key { hid: 0x04, down: true });
    wait_until("held", || !b.host.held_keys().is_empty()).await;
    let t0 = Instant::now();
    // The person turns "Mouse and keyboard" off for Laptop on Ren-PC.
    a.trust.set_permission(&b.id, capability::DESK_INPUT, false).unwrap();
    a.link.permission_changed();
    wait_until("link closed", || state_of(&a, &b) == "off" && state_of(&b, &a) != "online").await;
    wait_until("pointer home", || !a.host.capturing()).await;
    assert!(Instant::now().duration_since(t0) <= Duration::from_millis(400));
    wait_until("released", || b.host.held_keys().is_empty()).await;
    // And it stays off: no redial.
    sleep(Duration::from_secs(5)).await;
    assert_ne!(state_of(&b, &a), "online");
    assert_eq!(state_of(&a, &b), "off");
}

#[tokio::test(start_paused = true)]
async fn removing_a_computer_closes_the_link_too() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.trust.remove(&b.id).unwrap();
    wait_until("closed", || state_of(&b, &a) != "online").await;
    assert!(a.link.view().devices.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_link_that_goes_away_comes_back_by_itself() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    b.link.stop();
    wait_until("a sees it end", || state_of(&a, &b) != "online").await;
    b.link.start();
    wait_until("back", || state_of(&a, &b) == "online" && state_of(&b, &a) == "online").await;
}

// ----- a hand-played peer: protocol rules -----

struct Raw {
    link: Arc<dyn Link>,
    ctl: BiStream,
}

async fn raw_peer(net: &MemoryNet, dut: &Pc, os: OsKind, id: NodeId) -> Raw {
    dut.trust.add(id, [9; 32], "Raw", 1).unwrap();
    dut.trust.set_permission(&id, capability::DESK_INPUT, true).unwrap();
    dut.link.start();
    wait_until("dut listens", || dut.p2p.is_running()).await;
    let t: Arc<dyn Transport> = net.bind_raw(id, &[alpn::DESK]);
    let link = t.dial(&PeerAddr::new(dut.id), alpn::DESK).await.unwrap();
    let mut ctl = link.open_bi().await.unwrap();
    write_kind(&mut ctl.send, KIND_CONTROL).await.unwrap();
    write_frame(&mut ctl.send, &StreamMsg::Hello { version: PROTO_VERSION, device_id: id.to_hex(), name: "Raw".into(), os }).await.unwrap();
    drop(t);
    Raw { link, ctl }
}

#[tokio::test(start_paused = true)]
async fn the_hello_must_name_the_node_the_link_was_made_with() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let id = NodeId([0x11; 32]);
    dut.trust.add(id, [9; 32], "Raw", 1).unwrap();
    dut.trust.set_permission(&id, capability::DESK_INPUT, true).unwrap();
    dut.link.start();
    wait_until("listens", || dut.p2p.is_running()).await;
    let t = net.bind_raw(id, &[alpn::DESK]);
    let link = t.dial(&PeerAddr::new(dut.id), alpn::DESK).await.unwrap();
    let mut ctl = link.open_bi().await.unwrap();
    write_kind(&mut ctl.send, KIND_CONTROL).await.unwrap();
    // Claims to be someone else.
    write_frame(&mut ctl.send, &StreamMsg::Hello { version: PROTO_VERSION, device_id: NodeId([0x22; 32]).to_hex(), name: "Liar".into(), os: OsKind::Windows }).await.unwrap();
    assert_eq!(link.closed().await, CLOSE_PROTOCOL);
}

#[tokio::test(start_paused = true)]
async fn an_oversized_control_frame_cuts_the_link_before_anything_is_allocated() {
    // DevHop read control frames of up to 32 MiB.
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let mut raw = raw_peer(&net, &dut, OsKind::Windows, NodeId([0x33; 32])).await;
    let ack = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    assert!(matches!(ack, Some(StreamMsg::HelloAck { .. })));
    use tokio::io::AsyncWriteExt;
    raw.ctl.send.write_all(&(10_000_000u32).to_le_bytes()).await.unwrap();
    assert_eq!(raw.link.closed().await, CLOSE_PROTOCOL);
}

#[tokio::test(start_paused = true)]
async fn a_second_stream_is_dropped_not_parsed_as_something_else() {
    // DevHop treated every stream the peer opened as a file transfer.
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let mut raw = raw_peer(&net, &dut, OsKind::Windows, NodeId([0x44; 32])).await;
    let _ = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    let mut junk = raw.link.open_bi().await.unwrap();
    use tokio::io::AsyncWriteExt;
    junk.send.write_all(&[9, 1, 2, 3, 4, 5]).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Enter { monitor_id: "left".into(), nx_milli: 0, ny_milli: 500 }).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: 0x04, down: true }).await.unwrap();
    wait_until("the control stream still works", || dut.host.held_keys().contains(&0x04)).await;
}

#[tokio::test(start_paused = true)]
async fn input_without_an_enter_is_ignored() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let mut raw = raw_peer(&net, &dut, OsKind::Windows, NodeId([0x55; 32])).await;
    let _ = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: 0x04, down: true }).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Button { button: 0, down: true }).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    assert!(dut.host.with(|s| s.log.is_empty()), "only a computer that holds the mouse may type");
}

#[tokio::test(start_paused = true)]
async fn command_and_alt_swap_between_a_mac_and_a_pc() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let mut raw = raw_peer(&net, &dut, OsKind::Macos, NodeId([0x66; 32])).await;
    let _ = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Enter { monitor_id: "left".into(), nx_milli: 0, ny_milli: 500 }).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: 0xE3, down: true }).await.unwrap(); // Command
    write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: 0x06, down: true }).await.unwrap(); // C
    wait_until("mapped", || dut.host.held_keys().len() == 2).await;
    assert!(dut.host.held_keys().contains(&0xE2), "Command becomes Alt on Windows");
    // Ctrl is never remapped.
    write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: 0xE0, down: true }).await.unwrap();
    wait_until("ctrl", || dut.host.held_keys().contains(&0xE0)).await;
}

#[tokio::test(start_paused = true)]
async fn too_many_held_keys_are_not_remembered_without_bound() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let mut raw = raw_peer(&net, &dut, OsKind::Windows, NodeId([0x77; 32])).await;
    let _ = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Enter { monitor_id: "left".into(), nx_milli: 0, ny_milli: 500 }).await.unwrap();
    for hid in 0..2000u16 {
        write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: 4 + hid, down: true }).await.unwrap();
    }
    sleep(Duration::from_millis(200)).await;
    // The host's own release_injected is the backstop for what the hub stopped counting.
    drop(raw.ctl);
    raw.link.close(0, "done");
    wait_until("everything released", || dut.host.held_keys().is_empty()).await;
}

// ----- clipboard -----

#[tokio::test(start_paused = true)]
async fn a_copy_arrives_on_the_other_computer_when_the_pointer_crosses() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.set_clipboard(vec![ClipItem::Text("hello from the pc".into())]);
    cross(&a, &b).await;
    wait_until("copy arrives", || b.host.clipboard() == vec![ClipItem::Text("hello from the pc".into())]).await;
}

#[tokio::test(start_paused = true)]
async fn an_8_mb_image_arrives_whole_and_the_heartbeat_does_not_stall() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    let image = png(8 * 1024 * 1024, 3);
    a.host.set_clipboard(vec![ClipItem::Png(image.clone())]);
    cross(&a, &b).await;
    wait_until("image arrives", || b.host.clipboard().first().is_some_and(|i| matches!(i, ClipItem::Png(p) if *p == image))).await;
    let (_, gap) = b.link.beat_stats(&a.id).unwrap();
    assert!(gap < 150, "heartbeat gap {gap} ms while the image went across");
    assert_eq!(state_of(&b, &a), "online");
}

#[tokio::test(start_paused = true)]
async fn a_copy_that_came_from_the_other_computer_is_not_sent_back() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.set_clipboard(vec![ClipItem::Text("once".into())]);
    cross(&a, &b).await;
    wait_until("arrives", || !b.host.with(|s| s.clip_writes.is_empty())).await;
    // b pushes back through the edge; its clipboard is what a just sent it.
    for _ in 0..12 {
        a.host.person(Capture::Motion { dx: -40, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    wait_until("home", || !a.host.capturing()).await;
    sleep(Duration::from_millis(500)).await;
    assert!(a.host.with(|s| s.clip_writes.is_empty()), "the echo must not be written back");
}

#[tokio::test(start_paused = true)]
async fn a_new_copy_on_the_other_computer_comes_back_with_the_pointer() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    b.host.set_clipboard(vec![ClipItem::Text("copied on the laptop".into())]);
    for _ in 0..12 {
        a.host.person(Capture::Motion { dx: -40, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    wait_until("copy comes home", || a.host.clipboard() == vec![ClipItem::Text("copied on the laptop".into())]).await;
}

#[tokio::test(start_paused = true)]
async fn the_clipboard_switch_is_its_own_permission() {
    let (_net, a, b) = together();
    a.trust.set_permission(&b.id, capability::DESK_CLIPBOARD, false).unwrap();
    up(&a, &b).await;
    a.host.set_clipboard(vec![ClipItem::Text("secret".into())]);
    cross(&a, &b).await;
    sleep(Duration::from_secs(1)).await;
    assert!(b.host.with(|s| s.clip_writes.is_empty()));
    assert_eq!(a.host.with(|s| s.clip_reads), 0, "with the switch off the clipboard is not even read");
}

#[tokio::test(start_paused = true)]
async fn a_copy_over_32_mib_is_not_sent_and_the_person_is_told() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.set_clipboard(vec![ClipItem::Png(png(CAP + 1, 1))]);
    cross(&a, &b).await;
    wait_until("told", || a.link.view().clipboard_note.is_some()).await;
    assert!(a.link.view().clipboard_note.unwrap().contains("32 MB"));
    assert!(b.host.with(|s| s.clip_writes.is_empty()));
    assert_eq!(state_of(&b, &a), "online", "a refused copy does not cost the link");
}

// ----- clipboard rules (pure) -----

#[test]
fn a_copy_is_cut_into_64_kib_chunks_and_put_back_together() {
    assert_eq!(CHUNK, 64 * 1024);
    let data = png(CHUNK * 3 + 17, 5);
    let items = vec![ClipItem::Text("abc".into()), ClipItem::Png(data.clone())];
    let frames = clipboard::frames(7, &items);
    // announce + 1 text chunk + 4 image chunks
    assert_eq!(frames.len(), 1 + 1 + 4);
    let mut asm = Assembler::new();
    let mut out = None;
    for f in frames.clone() {
        if let StreamMsg::ClipboardChunk { bytes, .. } = &f {
            assert!(bytes.len() <= CHUNK);
        }
        out = asm.push(f).unwrap();
    }
    assert_eq!(out.unwrap(), items);
    assert_eq!(asm.held(), 0, "nothing is kept after a copy is complete");
}

#[test]
fn the_cap_is_32_mib_exactly() {
    assert_eq!(CAP, 32 * 1024 * 1024);
    assert!(clipboard::prepare(vec![ClipItem::Png(png(CAP, 0))]).is_ok());
    assert_eq!(clipboard::prepare(vec![ClipItem::Png(png(CAP + 1, 0))]), Err(ClipError::TooBig));
    assert_eq!(clipboard::prepare(vec![ClipItem::Text("a".repeat(CAP / 2 + 1)), ClipItem::Png(png(CAP / 2, 0))]), Err(ClipError::TooBig));
    assert_eq!(clipboard::prepare(vec![ClipItem::Text(String::new())]).unwrap(), vec![]);
}

#[test]
fn a_receiver_refuses_what_does_not_match_its_announce() {
    let ok = |n: usize| StreamMsg::ClipboardAnnounce { token: 1, items: vec![desk_proto::ClipAnnounce { kind: desk_proto::ClipKind::Text, bytes: n as u64 }] };
    let chunk = |seq: u32, last: bool, n: usize| StreamMsg::ClipboardChunk { token: 1, kind: desk_proto::ClipKind::Text, seq, last, bytes: vec![b'x'; n] };

    // Announces more than the cap.
    assert_eq!(Assembler::new().push(ok(CAP + 1)), Err(ClipError::TooBig));
    // Sends more than it announced (DevHop trusted the announce).
    let mut a = Assembler::new();
    a.push(ok(10)).unwrap();
    assert_eq!(a.push(chunk(0, false, 11)), Err(ClipError::TooBig));
    // Ends early.
    let mut a = Assembler::new();
    a.push(ok(10)).unwrap();
    assert!(matches!(a.push(chunk(0, true, 4)), Err(ClipError::Malformed(_))));
    // Skips a chunk.
    let mut a = Assembler::new();
    a.push(ok(CHUNK * 2)).unwrap();
    a.push(chunk(0, false, CHUNK)).unwrap();
    assert!(matches!(a.push(chunk(2, true, CHUNK)), Err(ClipError::Malformed(_))));
    // A chunk bigger than 64 KiB.
    let mut a = Assembler::new();
    a.push(ok(CHUNK * 2)).unwrap();
    assert!(matches!(a.push(chunk(0, false, CHUNK + 1)), Err(ClipError::Malformed(_))));
    // A chunk with no announce, or for another copy.
    assert!(matches!(Assembler::new().push(chunk(0, true, 1)), Err(ClipError::Malformed(_))));
    let mut a = Assembler::new();
    a.push(ok(10)).unwrap();
    let wrong = StreamMsg::ClipboardChunk { token: 2, kind: desk_proto::ClipKind::Text, seq: 0, last: true, bytes: vec![0; 10] };
    assert!(matches!(a.push(wrong), Err(ClipError::Malformed(_))));
    // Not text, not a picture.
    let mut a = Assembler::new();
    a.push(StreamMsg::ClipboardAnnounce { token: 1, items: vec![desk_proto::ClipAnnounce { kind: desk_proto::ClipKind::Png, bytes: 4 }] }).unwrap();
    let r = a.push(StreamMsg::ClipboardChunk { token: 1, kind: desk_proto::ClipKind::Png, seq: 0, last: true, bytes: vec![1, 2, 3, 4] });
    assert!(matches!(r, Err(ClipError::Malformed(_))));
    // Memory held is bounded by the announce.
    let mut a = Assembler::new();
    a.push(ok(CHUNK * 4)).unwrap();
    a.push(chunk(0, false, CHUNK)).unwrap();
    assert_eq!(a.held(), CHUNK);
}

#[test]
fn an_unchanged_copy_has_the_same_fingerprint_and_a_changed_one_does_not() {
    let a = vec![ClipItem::Text("same".into())];
    assert_eq!(clipboard::fingerprint(&a), clipboard::fingerprint(&[ClipItem::Text("same".into())]));
    assert_ne!(clipboard::fingerprint(&a), clipboard::fingerprint(&[ClipItem::Text("other".into())]));
    assert_ne!(clipboard::fingerprint(&a), clipboard::fingerprint(&[ClipItem::Png(b"same".to_vec())]));
}

// ----- the clipboard is never logged -----

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

#[tokio::test(start_paused = true)]
async fn a_copy_never_reaches_a_log_a_debug_print_or_an_error() {
    const SECRET: &str = "hunter2-correct-horse-battery-staple";
    let sink = Sink::default();
    let subscriber = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_writer(sink.clone()).with_ansi(false).finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    // Other tests run in parallel without a subscriber and may have cached "nobody listens" for these log lines.
    tracing::callsite::rebuild_interest_cache();

    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.set_clipboard(vec![ClipItem::Text(SECRET.into()), ClipItem::Png(png(2000, 9))]);
    cross(&a, &b).await;
    wait_until("copy arrives", || !b.host.with(|s| s.clip_writes.is_empty())).await;

    // The lines the clipboard wrote (captured where they are made, so a parallel test cannot hide them) ...
    let lines = super::clip_link::take_test_log();
    assert!(lines.iter().any(|l| l.contains("going out")) && lines.iter().any(|l| l.contains("arrived")), "{lines:?}");
    assert!(lines.iter().all(|l| !l.contains(SECRET)));
    // ... and whatever reached the tracing subscriber on this thread.
    let logged = String::from_utf8_lossy(&sink.0.lock().unwrap()).to_string();
    assert!(!logged.contains(SECRET));

    // Debug prints of the things that carry a copy show sizes only.
    let item = ClipItem::Text(SECRET.into());
    assert!(!format!("{item:?}").contains(SECRET) && format!("{item:?}").contains("bytes"));
    let mut asm = Assembler::new();
    for f in clipboard::frames(1, &[item]) {
        let _ = asm.push(f);
        assert!(!format!("{asm:?}").contains(SECRET));
    }
    assert!(!ClipError::Malformed("x").to_string().contains(SECRET));
    let png_item = ClipItem::Png(png(100, 1));
    assert!(format!("{png_item:?}").contains("100 bytes"));
}

// ----- the soak -----

/// DevHop's loopback soak (PLAN.md N0.3), on simulated time: both computers dial at once, plus duplicate dials,
/// 30 s of motion, a 4 MB copy every 5 s. Zero Offline, no heartbeat gap of 150 ms or more.
#[tokio::test(start_paused = true)]
async fn soak_30_seconds_of_motion_and_4_mb_copies_with_zero_offline() {
    let (_net, a, b) = together();
    a.link.start();
    b.link.start();
    wait_until("online", || state_of(&a, &b) == "online" && state_of(&b, &a) == "online").await;
    cross(&a, &b).await;

    let mut offline_seen = 0;
    let start = Instant::now();
    let mut next_copy = 0u64;
    let mut next_check = 0u64;
    let mut copies_sent = 0u8;
    let mut dup_done = false;
    let mut ms = 0u64;
    let mut moves = 0u64;
    while ms < 30_000 {
        // 1 kHz motion, back and forth so the pointer stays inside b.
        let d = if (moves / 50) % 2 == 0 { 1 } else { -1 };
        moves += 1;
        a.host.person(Capture::Motion { dx: d, dy: 0 });
        if ms >= next_copy {
            next_copy += 5_000;
            copies_sent += 1;
            let image = png(4 * 1024 * 1024, copies_sent);
            a.host.set_clipboard(vec![ClipItem::Png(image)]);
            let inner = a.link.inner.clone();
            tokio::spawn(super::clip_link::send_current(inner, b.id));
        }
        // After the first two seconds the session is settled: a stray duplicate must not touch it.
        if ms >= 3_000 && !dup_done {
            dup_done = true;
            for (x, y) in [(&a, &b), (&b, &a)] {
                let inner = x.link.inner.clone();
                let peer = y.id;
                let link = x.p2p.dial(&PeerAddr::new(peer), capability::DESK_INPUT).await.unwrap();
                tokio::spawn(super::io::run_link(inner, peer, link, true));
            }
        }
        if ms >= next_check {
            next_check += 100;
            if state_of(&a, &b) != "online" || state_of(&b, &a) != "online" {
                offline_seen += 1;
            }
        }
        sleep(Duration::from_millis(1)).await;
        ms = Instant::now().duration_since(start).as_millis() as u64;
    }
    wait_until("last copy lands", || b.host.clipboard().first().is_some_and(|c| c.len() == 4 * 1024 * 1024)).await;
    assert_eq!(offline_seen, 0, "the link must never report Offline");
    assert!(copies_sent >= 6);
    assert!(b.host.with(|s| s.clip_writes.len()) >= 6, "every 4 MB copy arrived");
    let (beats, gap_b) = b.link.beat_stats(&a.id).unwrap();
    let (_, gap_a) = a.link.beat_stats(&b.id).unwrap();
    assert!(beats > 250, "only {beats} heartbeats in 30 s");
    assert!(gap_a < 150 && gap_b < 150, "heartbeat gaps {gap_a} ms / {gap_b} ms");
    // The motion really arrived and nothing was left pressed.
    assert!(a.host.capturing() && b.host.held_keys().is_empty(), "a capturing {} b cursor {:?} a view {:?} a cursor {:?} b moves {}", a.host.capturing(), b.host.cursor(), a.link.view(), a.host.cursor(), b.host.with(|s| s.log.len()));
}

#[test]
fn the_sides_a_layout_can_hold_are_the_four_edges() {
    for (name, side) in [("left", Side::Left), ("right", Side::Right), ("top", Side::Top), ("bottom", Side::Bottom)] {
        assert_eq!(super::geometry::parse_side(name), Some(side));
    }
}

#[test]
fn the_mock_host_is_a_remote_host_and_the_real_default_is_not_built_in_tests() {
    let h: Arc<dyn RemoteHost> = super::remote::default_remote();
    assert!(!h.supported(), "a test build gets the stub, never the real PC");
    assert!(h.inject(Inject::Key { hid: 4, down: true }).is_err());
}

/// Swapping where the other computer sits, over and over, never leaves either computer unable to cross again.
#[tokio::test(start_paused = true)]
async fn crossing_there_and_back_after_every_swap_of_sides_never_sticks() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    let rounds = [("right", 0), ("left", 1), ("right", 0), ("left", 1), ("top", 2), ("bottom", 3), ("right", 0), ("left", 1), ("left", 1), ("right", 0)];
    for (i, (side, _)) in rounds.iter().enumerate() {
        a.link.set_side(&b.id, side).unwrap();
        // Where a's pointer rests to leave, and which way b pushes to bring it back.
        let (edge, push) = match *side {
            "right" => ((1919, 300), (-30, 0)),
            "left" => ((0, 300), (30, 0)),
            "top" => ((600, 0), (0, 30)),
            _ => ((600, 1079), (0, -30)),
        };
        a.host.put_cursor(edge.0, edge.1);
        wait_until(&format!("round {i} ({side}): crossed"), || a.host.capturing() && b.link.view().devices[0].controls_me).await;
        for _ in 0..8 {
            a.host.person(Capture::Motion { dx: push.0, dy: push.1 });
            sleep(Duration::from_millis(5)).await;
        }
        wait_until(&format!("round {i} ({side}): home"), || !a.host.capturing() && b.link.view().controlled_by.is_none()).await;
        sleep(Duration::from_millis(300)).await;
    }
}

// Hotkeys that reach the hub: jump to a computer, return home, lock.
mod hotkeys;
