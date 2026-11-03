//! The desk link on the in-memory net with a pretend PC. No socket, no hook, no real pointer, no real clipboard, no
//! real key store. Time is simulated (`start_paused`): 30 seconds of motion take no wall-clock time.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use desk_core::Side;
use desk_proto::{Datagram, OsKind, StreamMsg, PROTO_VERSION};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Instant};

use super::clipboard::{self, Assembler, ClipError, CAP, CHUNK};
use super::hub::Msg;
use super::io::{run_tasks, LinkShared};
use super::mock::{display, MockRemote};
use super::remote::{Capture, ClipItem, Inject, RemoteHost};
use super::wire::{read_frame, write_frame, write_kind, CONTROL_FRAME_MAX, KIND_CONTROL};
use super::{DeskLink, MemLinkStore, CLOSE_PROTOCOL};
use crate::p2p::alpn;
use crate::p2p::capability::{self, Registry};
use crate::p2p::endpoint::{BiStream, Link, NodeId, P2pError, PathKind, PeerAddr, RefusalKind, Transport};
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
    pc_with(net, name, MockRemote::standard())
}

fn pc_with(net: &MemoryNet, name: &str, host: MockRemote) -> Pc {
    let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
    let keys = KeyStore::with_backend(backend, name);
    let ident = DeviceIdentity::load(&keys).unwrap();
    let trust = TrustList::open(MemoryTrustStorage::new(), Arc::new(KeyStoreSigner::new(keys.clone())));
    let p2p = Arc::new(P2p::new(keys, net.factory(), Arc::new(Registry::new()), trust.clone()));
    p2p.apply_settings(&json!({ "desk_enabled": true }));
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
    together_with(MockRemote::standard())
}

/// The same, with a chosen screen setup for the second computer.
fn together_with(host_b: MockRemote) -> (MemoryNet, Pc, Pc) {
    let net = MemoryNet::new();
    // The lower node id has the other computer on its right until someone moves it, so `a` is the lower one.
    let a = pc(&net, "Ren-PC");
    let mut b = pc_with(&net, "Laptop", host_b.clone());
    while b.id.to_hex() < a.id.to_hex() {
        b = pc_with(&net, "Laptop", host_b.clone());
    }
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

/// Both people push at their edges in the same moment: each computer holds the other's pointer. Exactly one of them
/// keeps its crossing and the other is controlled by it. Never both controlling each other, and the holder can come
/// home and cross again.
#[tokio::test(start_paused = true)]
async fn two_pointers_crossing_at_once_settle_on_one_holder() {
    let (_net, a, b) = together();
    b.link.set_side(&a.id, "left").unwrap();
    up(&a, &b).await;
    a.host.put_cursor(1919, 540);
    b.host.put_cursor(0, 540);
    sleep(Duration::from_millis(800)).await;
    assert!(a.host.capturing() != b.host.capturing(), "exactly one computer holds the mouse");
    let (holder, other, edge) = if a.host.capturing() { (&a, &b, (1919, 540)) } else { (&b, &a, (0, 540)) };
    assert_eq!(other.link.view().controlled_by.as_deref(), Some(holder.name.as_str()));
    assert_eq!(holder.link.view().mouse_on.as_deref(), Some(other.name.as_str()));
    assert!(a.host.held_keys().is_empty() && b.host.held_keys().is_empty());
    // The holder comes home, and a normal crossing works afterwards.
    holder.link.panic();
    wait_until("the holder is home", || !holder.host.capturing() && other.link.view().controlled_by.is_none()).await;
    holder.host.put_cursor(edge.0, edge.1);
    wait_until("crosses again", || holder.host.capturing() && other.link.view().controlled_by.is_some()).await;
}

/// The other computer goes quiet while it is using this one. This computer lets go and says so (`Leave`), so the
/// other computer comes home too instead of staying on `remote` and answering every crossing with "busy".
#[tokio::test(start_paused = true)]
async fn a_quiet_computer_using_this_one_is_told_to_come_home() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let mut raw = raw_peer(&net, &dut, OsKind::Windows, NodeId([0x33; 32])).await;
    let ack = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    assert!(matches!(ack, Some(StreamMsg::HelloAck { .. })));
    // The hand-played peer puts its pointer on this computer's left edge, then sends nothing more.
    write_frame(&mut raw.ctl.send, &StreamMsg::Enter { monitor_id: "left".into(), nx_milli: 500, ny_milli: 500 }).await.unwrap();
    wait_until("this computer is being used", || dut.link.view().controlled_by.is_some()).await;
    let left = timeout(Duration::from_secs(3), async {
        loop {
            match read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await {
                Ok(Some(StreamMsg::Leave)) => return true,
                Ok(Some(_)) => continue,
                _ => return false,
            }
        }
    })
    .await;
    assert_eq!(left, Ok(true), "no Leave came: the other computer would stay on remote");
    assert_eq!(dut.link.view().controlled_by, None);
}

/// Motion from the other computer is the pointer's own movement: a jump that comes with it does not take the mouse back.
#[tokio::test(start_paused = true)]
async fn remote_motion_keeps_the_pointer_here_even_when_it_jumps() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    b.host.put_cursor(500, 500);
    for _ in 0..15 {
        a.host.person(Capture::Motion { dx: 1, dy: 0 });
        sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(b.link.view().controlled_by.as_deref(), Some("Ren-PC"));
    assert!(a.host.capturing());
}

/// A button the other computer pressed is held on this computer too: that one does not count as a person here.
#[tokio::test(start_paused = true)]
async fn a_button_pressed_by_the_other_computer_does_not_take_the_mouse_back() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Button { button: 0, down: true });
    wait_until("button held there", || b.host.with(|s| s.held_buttons.contains(&0))).await;
    b.host.with(|s| s.buttons_down = true);
    sleep(Duration::from_millis(300)).await;
    assert_eq!(b.link.view().controlled_by.as_deref(), Some("Ren-PC"));
    assert!(a.host.capturing());
}

/// While a pointer rests on an edge that faces a connected computer, the log says why it does not cross, at most once a
/// second for each reason. Away from the edges it says nothing.
#[tokio::test(start_paused = true)]
async fn the_log_says_why_the_pointer_is_not_crossing_at_most_once_a_second() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(300)).await;
    assert!(!log_text(&log).contains("not crossing"), "{}", log_text(&log));
    a.host.with(|s| s.buttons_down = true);
    a.host.put_cursor(1919, 540);
    sleep(Duration::from_millis(1500)).await;
    assert!(!a.host.capturing());
    let lines = log_text(&log).lines().filter(|l| l.contains("desk: not crossing reason=button held")).count();
    assert!((1..=2).contains(&lines), "{lines} lines in 1.5 s: {}", log_text(&log));
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
    // The pointer arrives on the edge pixel of b (no inset).
    assert!(bx == 0 && (by - 540).abs() <= 2, "{bx},{by}");
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

// ----- the hand-back: one outward push, no counter, no pause, no step-off -----

/// One outward push through the edge that faces the holder sends the pointer back, at once, to the matching spot.
#[tokio::test(start_paused = true)]
async fn one_outward_push_returns() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    // The pointer arrived on b's left edge pixel: a single push through it is enough.
    assert_eq!(b.host.cursor(), (0, 300));
    a.host.person(Capture::Motion { dx: -30, dy: 0 });
    wait_until("pointer home", || !a.host.capturing()).await;
    assert_eq!(a.host.cursor(), (1919, 300), "back on the PC's edge pixel, same row");
    assert_eq!(b.link.view().controlled_by, None);
}

/// After the hand-back the pointer rests on the edge it came back to. Resting there does not cross again; an outward
/// move does.
#[tokio::test(start_paused = true)]
async fn a_returned_pointer_resting_on_the_edge_does_not_cross_until_it_moves_outward() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    a.host.person(Capture::Motion { dx: -30, dy: 0 });
    wait_until("a is home", || !a.host.capturing()).await;
    let sent = b.host.with(|s| s.log.len());
    sleep(Duration::from_secs(3)).await;
    assert!(!a.host.capturing(), "resting on the edge crossed");
    assert_eq!(b.link.view().controlled_by, None);
    assert_eq!(b.host.with(|s| s.log.len()), sent, "nothing was sent to the other computer while it rested");
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(50)).await;
    a.host.put_cursor(1919, 300);
    wait_until("crosses on the outward move", || a.host.capturing()).await;
}

/// The hand-back does not step the pointer off the edge: it stays where the push left it.
#[tokio::test(start_paused = true)]
async fn the_hand_back_does_not_step_the_pointer_off_the_edge() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    a.host.person(Capture::Motion { dx: -30, dy: 0 });
    wait_until("a is home", || !a.host.capturing()).await;
    let moves = b.host.with(|s| s.moves.len());
    sleep(Duration::from_millis(300)).await;
    assert_eq!(b.host.with(|s| s.moves.len()), moves, "something moved the pointer after the hand-back");
    assert_eq!(b.host.cursor(), (0, 300));
}

/// A person moves the mouse of the computer that is being controlled: that is not a hand-back. The other computer
/// keeps its mouse and nothing is let go.
#[tokio::test(start_paused = true)]
async fn a_person_using_the_controlled_computer_does_not_take_the_mouse_back() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    b.host.put_cursor(500, 500);
    sleep(Duration::from_millis(300)).await;
    assert_eq!(b.link.view().controlled_by.as_deref(), Some("Ren-PC"), "b let go of a pointer a person moved");
    assert!(a.host.capturing(), "a is still holding the mouse");
    assert!(!log_text(&log).contains("taking the mouse back"));
}

/// A button held on the controlled computer, which the other computer did not press, does not take the mouse back.
#[tokio::test(start_paused = true)]
async fn a_button_held_here_does_not_take_the_mouse_back() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    b.host.with(|s| s.buttons_down = true);
    sleep(Duration::from_millis(300)).await;
    assert_eq!(b.link.view().controlled_by.as_deref(), Some("Ren-PC"));
    assert!(a.host.capturing());
}

/// Clicks on the other computer change nothing: a click, then one push, returns the pointer.
#[tokio::test(start_paused = true)]
async fn a_click_then_one_push_returns_the_pointer() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    a.host.person(Capture::Button { button: 0, down: true });
    a.host.person(Capture::Button { button: 0, down: false });
    wait_until("click arrives", || b.host.with(|s| s.log.iter().filter(|e| matches!(e, Inject::Button { .. })).count() == 2)).await;
    sleep(Duration::from_millis(100)).await;
    assert!(a.host.capturing(), "a click sent the pointer home");
    a.host.person(Capture::Motion { dx: -30, dy: 0 });
    wait_until("pointer home", || !a.host.capturing()).await;
    assert_eq!(b.link.view().controlled_by, None);
}

/// A key and a button held on the other computer are let go when the pointer comes home.
#[tokio::test(start_paused = true)]
async fn a_held_button_and_key_are_released_when_the_pointer_comes_home() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    a.host.person(Capture::Button { button: 0, down: true });
    a.host.person(Capture::Key { hid: 0x04, down: true });
    wait_until("held on b", || b.host.with(|s| s.held_buttons.contains(&0)) && !b.host.held_keys().is_empty()).await;
    let releases = b.host.with(|s| s.releases);
    a.host.person(Capture::Motion { dx: -30, dy: 0 });
    wait_until("pointer home", || !a.host.capturing()).await;
    wait_until("released on b", || b.host.held_keys().is_empty() && b.host.with(|s| s.held_buttons.is_empty())).await;
    assert!(b.host.with(|s| s.releases) > releases, "release_injected was not called");
    assert!(log_text(&log).contains("desk: held at end keys=1 buttons=1"), "{}", log_text(&log));
}

/// A push through an edge with no computer beyond it does nothing: the pointer stays on the other computer.
#[tokio::test(start_paused = true)]
async fn a_push_at_an_edge_with_no_neighbour_does_nothing() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    // Across b's screen to its top edge, where nothing of this layout lies.
    a.host.person(Capture::Motion { dx: 500, dy: -400 });
    wait_until("b's pointer on its top edge", || b.host.cursor().1 == 0).await;
    a.host.person(Capture::Motion { dx: 0, dy: -30 });
    sleep(Duration::from_millis(200)).await;
    assert!(a.host.capturing(), "a push at a wall returned the pointer");
    assert_eq!(b.host.cursor(), (500, 0));
}

/// A long session on the other computer: 500 jitter moves that never reach an edge, 50 clicks, 40 keys, wheel events,
/// then 2 s of quiet on this computer while the other computer's own pointer is moved (trackpad) and a button is
/// reported held there with none injected. Nothing comes home by itself, nothing is let go, the other computer stays
/// controlled.
#[tokio::test(start_paused = true)]
async fn a_long_session_of_jitter_clicks_and_local_use_never_returns_by_itself() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    let released_at_start = b.host.with(|s| s.releases);
    // Well inside b's screen first: the arrival is on its left edge pixel.
    a.host.person(Capture::Motion { dx: 500, dy: 0 });
    wait_until("b's pointer is inside", || b.host.cursor().0 == 500).await;
    // A fixed pseudo-random sequence in -3..=3: the pointer wanders a few units around (500, 300).
    let mut seed: u32 = 12_345;
    let mut next = move || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((seed >> 16) % 7) as i32 - 3
    };
    for _ in 0..500 {
        let (dx, dy) = (next(), next());
        a.host.person(Capture::Motion { dx, dy });
    }
    sleep(Duration::from_millis(500)).await;
    for _ in 0..50 {
        a.host.person(Capture::Button { button: 0, down: true });
        a.host.person(Capture::Button { button: 0, down: false });
    }
    for _ in 0..40 {
        a.host.person(Capture::Key { hid: 0x04, down: true });
        a.host.person(Capture::Key { hid: 0x04, down: false });
    }
    for _ in 0..20 {
        a.host.person(Capture::Wheel { dx: 0, dy: -120 });
    }
    sleep(Duration::from_millis(500)).await;
    // The other computer's own pointer and button: local use, which is not a hand-back.
    let (x, y) = b.host.cursor();
    b.host.put_cursor(x + 100, y);
    b.host.with(|s| s.buttons_down = true);
    sleep(Duration::from_secs(2)).await;
    assert!(a.host.capturing(), "the pointer came home without a push");
    assert_eq!(b.link.view().controlled_by.as_deref(), Some("Ren-PC"), "the other computer let go on its own");
    // (`together` drops the first pretend Mac it may have built, and that drop releases on the shared host: count from here.)
    assert_eq!(b.host.with(|s| s.releases), released_at_start, "something was let go: {}", log_text(&log));
}

/// Twenty enter-and-return round trips leave nothing behind: nothing held, no pointer kept, and each crossing works.
#[tokio::test(start_paused = true)]
async fn twenty_round_trips_leave_no_state_behind() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    for round in 0..20 {
        a.host.put_cursor(960, 540);
        sleep(Duration::from_millis(30)).await;
        a.host.put_cursor(1919, 300);
        wait_until(&format!("round {round}: crossed"), || a.host.capturing() && b.link.view().controlled_by.is_some()).await;
        a.host.person(Capture::Key { hid: 0x04, down: true });
        a.host.person(Capture::Key { hid: 0x04, down: false });
        a.host.person(Capture::Motion { dx: 300, dy: 0 });
        sleep(Duration::from_millis(30)).await;
        a.host.person(Capture::Motion { dx: -400, dy: 0 });
        wait_until(&format!("round {round}: home"), || !a.host.capturing() && b.link.view().controlled_by.is_none()).await;
        assert!(b.host.held_keys().is_empty(), "round {round}: a key was left held");
    }
    assert_eq!(a.link.view().error, None);
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
    // The pointer was already on the edge: it must move outward again before it crosses (resting does not cross).
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(30)).await;
    a.host.put_cursor(1919, 540);
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
    // And the person at a is told why, in plain words.
    let err = a.link.view().error.unwrap_or_default();
    assert!(err.contains("Laptop did not take the mouse") && err.contains("paused on Laptop"), "{err}");
}

/// A Mac without Accessibility cannot send input. It says so at the edge: the sender keeps its mouse, both pages say
/// why, and nothing on the Mac was moved.
#[tokio::test(start_paused = true)]
async fn a_computer_that_cannot_be_controlled_says_why_at_the_edge() {
    const WHY: &str = "Puppet Master needs Accessibility access to control this Mac.";
    let (_net, a, b) = together();
    b.host.with(|s| s.refuse_control = Some(WHY.into()));
    up(&a, &b).await;
    a.host.put_cursor(1919, 540);
    wait_until("a is told", || a.link.view().error.is_some()).await;
    wait_until("a is home", || !a.host.capturing()).await;
    let err = a.link.view().error.unwrap();
    assert_eq!(err, format!("Laptop did not take the mouse: {WHY}"));
    assert_eq!(b.link.view().error.as_deref(), Some(WHY), "the Mac's own page says it too");
    assert!(b.host.with(|s| s.moves.is_empty() && s.log.is_empty()), "nothing on the Mac was touched");
    assert!(!b.link.view().devices[0].controls_me);
    assert_eq!(state_of(&a, &b), "online", "a refusal does not cost the link");
}

/// After a no, the pointer is not thrown at that computer again every 20 ms while the person keeps pushing.
#[tokio::test(start_paused = true)]
async fn a_refused_crossing_is_not_retried_at_once() {
    let (_net, a, b) = together();
    b.host.with(|s| s.refuse_control = Some("no Accessibility".into()));
    up(&a, &b).await;
    a.host.put_cursor(1919, 540);
    wait_until("refused", || a.link.view().error.is_some() && !a.host.capturing()).await;
    assert_eq!(a.host.with(|s| s.captures_started), 1);
    // The person keeps pushing at the edge: away from it, then outward again, every 100 ms.
    for _ in 0..15 {
        a.host.put_cursor(960, 540);
        sleep(Duration::from_millis(50)).await;
        a.host.put_cursor(1919, 540);
        sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(a.host.with(|s| s.captures_started), 1, "tried again inside the pause");
    // Fixed on the other computer: the next push goes across.
    b.host.with(|s| s.refuse_control = None);
    sleep(Duration::from_millis(600)).await;
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(30)).await;
    a.host.put_cursor(1919, 540);
    wait_until("crosses once it is allowed", || a.host.capturing() && b.link.view().devices[0].controls_me).await;
    assert_eq!(a.link.view().error, None, "a crossing that works clears the old reason");
}

/// Input that stops working while the pointer is over there (Accessibility switched off mid-way): the other computer
/// gets its mouse back with the reason, instead of swallowing every move for nothing.
#[tokio::test(start_paused = true)]
async fn input_that_stops_working_gives_the_mouse_back_with_the_reason() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    b.host.with(|s| s.fail_inject = Some("macOS said no".into()));
    a.host.person(Capture::Motion { dx: 5, dy: 0 });
    wait_until("a is home", || !a.host.capturing()).await;
    let err = a.link.view().error.unwrap_or_default();
    assert!(err.contains("Laptop could not move the pointer: macOS said no"), "{err}");
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
async fn a_pulled_cable_releases_held_keys_within_500_ms() {
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
        assert!(at.duration_since(t0) <= Duration::from_millis(500), "key {hid:#x} released after {:?}", at.duration_since(t0));
    }
    // The sender's pointer comes home on its own too, with nobody to ask.
    wait_until("sender home", || !a.host.capturing()).await;
    assert!(Instant::now().duration_since(t0) <= Duration::from_millis(500));
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
        // 1 kHz motion, back and forth so the pointer stays inside b: first 200 units in from its edge, then 50 units
        // either way. (A push on b's edge would hand the pointer back, so it must not reach the edge.)
        let d = if moves < 200 || (moves / 50) % 2 == 0 { 1 } else { -1 };
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
        // The pointer comes back inside the edge; it must leave the edge before it may cross again.
        a.host.put_cursor(960, 540);
        sleep(Duration::from_millis(30)).await;
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
mod hotkey_tests;


// Per-computer keys and scrolling.
mod prefs_tests;

// ----- the layout -----

fn laptop() -> MockRemote {
    MockRemote::new(vec![display("main", 0, 0, 1280, 800)], (640, 400))
}

fn spot_of_in(p: &Pc, other: &Pc) -> (i32, i32) {
    let v = p.link.layout();
    let c = v.computers.iter().find(|c| c.node == other.id.to_hex()).unwrap();
    (c.x, c.y)
}

/// Both computers have told each other their screens.
async fn knows_screens(a: &Pc, b: &Pc) {
    wait_until("a knows b's screens", || a.link.layout().computers.iter().any(|c| c.node == b.id.to_hex() && c.known)).await;
    wait_until("b knows a's screens", || b.link.layout().computers.iter().any(|c| c.node == a.id.to_hex() && c.known)).await;
}

#[tokio::test(start_paused = true)]
async fn the_layout_view_has_every_computer_in_one_space() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    let v = a.link.layout();
    assert_eq!(v.computers.len(), 2);
    let me = &v.computers[0];
    assert!(me.me && me.node.is_empty() && (me.x, me.y, me.width, me.height) == (0, 0, 1920, 1080));
    let other = &v.computers[1];
    assert_eq!((other.x, other.y, other.width, other.height), (1920, 0, 1280, 800), "the default: on the right, tops aligned");
    assert_eq!((other.screens[0].x, other.screens[0].width), (1920, 1280));
    assert!(other.known && other.online);
    // The other computer sees the mirror.
    assert_eq!(spot_of_in(&b, &a), (-1920, 0));
}

#[tokio::test(start_paused = true)]
async fn the_pointer_crosses_only_where_the_screens_touch_and_lands_at_the_matching_spot() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    // The laptop sits on the right, 200 px lower than the top of the big screen.
    a.link.set_position(&b.id, 1920, 200).unwrap();
    wait_until("b agrees", || spot_of_in(&b, &a) == (-1920, -200)).await;
    // Above the laptop and below it, the edge is a wall.
    for y in [100, 199, 1000, 1070] {
        a.host.put_cursor(1919, y);
        sleep(Duration::from_millis(200)).await;
        assert!(!a.host.capturing(), "y={y} is beside the laptop");
        a.host.put_cursor(960, 540);
        sleep(Duration::from_millis(30)).await;
    }
    a.host.put_cursor(1919, 500);
    wait_until("crossed", || a.host.capturing() && b.link.view().devices[0].controls_me).await;
    let (bx, by) = b.host.cursor();
    assert_eq!((bx, by), (0, 300), "500 down the big screen is 300 down the laptop, on the laptop's edge pixel");
}

#[tokio::test(start_paused = true)]
async fn the_way_back_lands_at_the_matching_spot_of_the_big_screen() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    a.link.set_position(&b.id, 1920, 200).unwrap();
    wait_until("b agrees", || spot_of_in(&b, &a) == (-1920, -200)).await;
    a.host.put_cursor(1919, 500);
    wait_until("crossed", || a.host.capturing()).await;
    for _ in 0..8 {
        a.host.person(Capture::Motion { dx: -30, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    wait_until("pointer home", || !a.host.capturing()).await;
    let (x, y) = a.host.cursor();
    assert_eq!((x, y), (1919, 500), "came back to the same height, on the PC's edge pixel");
    assert!(b.link.view().controlled_by.is_none());
    sleep(Duration::from_millis(300)).await;
    assert!(!a.host.capturing(), "it does not cross again by itself");
}

#[tokio::test(start_paused = true)]
async fn a_gap_between_the_computers_is_a_wall() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    a.link.set_position(&b.id, 2300, 0).unwrap();
    wait_until("b agrees", || spot_of_in(&b, &a) == (-2300, 0)).await;
    a.host.put_cursor(1919, 540);
    sleep(Duration::from_millis(300)).await;
    assert!(!a.host.capturing());
    // Closing the gap makes it cross again: the pointer must move outward again (resting on the edge does not cross).
    a.link.set_position(&b.id, 1950, 0).unwrap();
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(30)).await;
    a.host.put_cursor(1919, 540);
    wait_until("crosses", || a.host.capturing()).await;
}

#[tokio::test(start_paused = true)]
async fn moving_a_computer_on_either_side_moves_it_on_the_other() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    a.link.set_position(&b.id, 1920, 300).unwrap();
    wait_until("b sees a left of it and 300 higher", || spot_of_in(&b, &a) == (-1920, -300)).await;
    // And the other way: the person moves it on the laptop (the big screen at x -1920, y 0 from the laptop).
    sleep(Duration::from_millis(5)).await;
    b.link.set_position(&a.id, -1920, 0).unwrap();
    wait_until("a sees the laptop at 1920, 0", || spot_of_in(&a, &b) == (1920, 0)).await;
    // The newer decision wins everywhere; neither side keeps its old answer.
    sleep(Duration::from_millis(500)).await;
    assert_eq!(spot_of_in(&a, &b), (1920, 0));
    assert_eq!(spot_of_in(&b, &a), (-1920, 0));
}

#[tokio::test(start_paused = true)]
async fn set_side_is_a_snapped_placement_both_agree_on() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    a.link.set_side(&b.id, "bottom").unwrap();
    wait_until("b has a above it", || spot_of_in(&b, &a) == (0, -1080)).await;
    assert_eq!(spot_of_in(&a, &b), (0, 1080));
    assert_eq!(a.link.view().devices[0].side, "bottom");
    assert_eq!(b.link.view().devices[0].side, "top");
    b.link.set_side(&a.id, "right").unwrap();
    wait_until("a has b on its left", || a.link.view().devices[0].side == "left").await;
    assert_eq!(spot_of_in(&a, &b), (-1280, 0));
}

#[tokio::test(start_paused = true)]
async fn screens_cannot_be_put_on_top_of_each_other() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    assert!(a.link.set_position(&b.id, 100, 100).is_err());
    assert!(a.link.set_position(&b.id, 10_000_000, 0).is_err());
    assert_eq!(spot_of_in(&a, &b), (1920, 0), "a refused placement changes nothing");
    assert!(a.link.set_position(&b.id, 1919, 0).is_err(), "one pixel over is over");
    assert!(a.link.set_position(&b.id, 1920, 0).is_ok(), "touching is fine");
}

#[test]
fn the_old_side_words_are_still_read_from_the_store() {
    let raw = std::collections::BTreeMap::from([("aa".to_string(), "left".to_string()), ("bb".to_string(), "sideways".to_string())]);
    let peers = super::decode_store(raw);
    assert_eq!(peers["aa"].spot, Some(super::arrange::Spot::Side(Side::Left)));
    assert!(peers["aa"].rev > 0, "an old choice beats the default");
    assert_eq!(peers["bb"].spot, None);
    // And what is written now reads back.
    let mut m = std::collections::BTreeMap::new();
    m.insert("cc".to_string(), super::PeerRec { spot: Some(super::arrange::Spot::At { x: -5, y: 9 }), rev: 77, screens: vec![] });
    assert_eq!(super::decode_store(super::encode_store(&m)), m);
}

#[tokio::test(start_paused = true)]
async fn an_older_decision_does_not_win() {
    let (_net, a, b) = together_with(laptop());
    up(&a, &b).await;
    knows_screens(&a, &b).await;
    a.link.set_position(&b.id, 1920, 100).unwrap();
    wait_until("b agrees", || spot_of_in(&b, &a) == (-1920, -100)).await;
    // A message with an older revision is answered with the newer decision, not adopted.
    let old = super::arrange::LayoutMsg {
        v: super::arrange::LAYOUT_V,
        screens: vec![super::arrange::Shape { id: "main".into(), name: "main".into(), primary: true, rect: desk_core::PhysRect::new(0, 0, 1920, 1080) }],
        spot: super::arrange::Spot::At { x: -1920, y: -900 },
        rev: 2,
    };
    let reply = b.link.inner.apply_layout(&a.id, old);
    assert!(reply, "the newer decision is said back");
    assert_eq!(spot_of_in(&b, &a), (-1920, -100));
}

// ----- why the pointer came home -----

/// What the hub logs on this thread goes into the buffer (the test runs on one thread, hub included).
fn log_capture() -> (Sink, tracing::subscriber::DefaultGuard) {
    let sink = Sink::default();
    let subscriber = tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).with_writer(sink.clone()).with_ansi(false).finish();
    let guard = tracing::subscriber::set_default(subscriber);
    // Other tests run in parallel without a subscriber and may have cached "nobody listens" for these log lines.
    tracing::callsite::rebuild_interest_cache();
    (sink, guard)
}

fn log_text(log: &Sink) -> String {
    String::from_utf8_lossy(&log.0.lock().unwrap()).to_string()
}

/// The reason part of every "pointer home" line, e.g. "edge peer=..." or "stall gap_ms=300 peer=...".
fn home_reasons(log: &Sink) -> Vec<String> {
    log_text(log).lines().filter_map(|l| l.split("desk: pointer home reason=").nth(1)).map(str::to_string).collect()
}

#[tokio::test(start_paused = true)]
async fn the_log_says_the_pointer_came_home_on_an_edge() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    a.host.put_cursor(1919, 300);
    wait_until("crossed", || a.host.capturing()).await;
    for _ in 0..3 {
        a.host.person(Capture::Motion { dx: 30, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    for _ in 0..8 {
        a.host.person(Capture::Motion { dx: -30, dy: 0 });
        sleep(Duration::from_millis(5)).await;
    }
    wait_until("pointer home", || !a.host.capturing()).await;
    let reasons = home_reasons(&log);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("edge"), "{reasons:?}");
    // The controlled side says where it handed the pointer back, and that it let go of nothing.
    assert!(log_text(&log).contains("desk: returning at="), "{}", log_text(&log));
    assert!(log_text(&log).contains("desk: held at end keys=0 buttons=0"));
}

#[tokio::test(start_paused = true)]
async fn the_log_says_the_pointer_came_home_when_the_link_went_quiet() {
    let (log, _guard) = log_capture();
    let (net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    net.blackhole(a.id, b.id, true);
    wait_until("sender home", || !a.host.capturing()).await;
    let reasons = home_reasons(&log);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    let gap: Option<u64> = reasons[0].strip_prefix("stall gap_ms=").and_then(|r| r.split_whitespace().next()).and_then(|g| g.parse().ok());
    assert!(gap.is_some_and(|g| g >= crate::p2p::session::STALL_MS), "{reasons:?}");
}

#[tokio::test(start_paused = true)]
async fn the_log_says_the_pointer_came_home_when_the_other_computer_leaves() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    // The other computer's panic: it lets go and sends `Leave`, so this computer's pointer comes home.
    b.link.panic();
    wait_until("a is home", || !a.host.capturing()).await;
    let reasons = home_reasons(&log);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("leave"), "{reasons:?}");
}

#[tokio::test(start_paused = true)]
async fn the_log_says_the_pointer_came_home_when_the_link_ended() {
    let (log, _guard) = log_capture();
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.link.stop();
    // The pointer is home at once; the hub writes its line when it takes the shutdown.
    wait_until("the log says why", || !home_reasons(&log).is_empty()).await;
    assert!(!a.host.capturing());
    let reasons = home_reasons(&log);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("ended"), "{reasons:?}");
}

// ----- a link that can no longer carry motion -----

/// The real link, except that no datagram gets out (the path went bad under it).
struct NoDatagrams(Arc<dyn Link>);

#[async_trait]
impl Link for NoDatagrams {
    fn peer(&self) -> NodeId {
        self.0.peer()
    }
    fn alpn(&self) -> Vec<u8> {
        self.0.alpn()
    }
    fn path(&self) -> PathKind {
        self.0.path()
    }
    async fn wait_path_change(&self, from: PathKind) -> Option<PathKind> {
        self.0.wait_path_change(from).await
    }
    async fn open_bi(&self) -> Result<BiStream, P2pError> {
        self.0.open_bi().await
    }
    async fn accept_bi(&self) -> Result<BiStream, P2pError> {
        self.0.accept_bi().await
    }
    async fn send_datagram(&self, _data: Vec<u8>) -> Result<(), P2pError> {
        Err(P2pError::Closed)
    }
    async fn recv_datagram(&self) -> Result<Vec<u8>, P2pError> {
        self.0.recv_datagram().await
    }
    fn close(&self, code: u32, reason: &str) {
        self.0.close(code, reason)
    }
    async fn closed(&self) -> u32 {
        self.0.closed().await
    }
}

#[tokio::test(start_paused = true)]
async fn a_motion_send_error_ends_the_link_so_it_is_dialed_again() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let far_id = NodeId([0x11; 32]);
    dut.trust.add(far_id, [9; 32], "Far", 1).unwrap();
    dut.trust.set_permission(&far_id, capability::DESK_INPUT, true).unwrap();
    // A plain pair of endpoints stands in for the other computer; the link under test is the near end.
    let _far = net.bind_raw(far_id, &[alpn::DESK]);
    let near = net.bind_raw(NodeId([0x22; 32]), &[alpn::DESK]);
    let real = near.dial(&PeerAddr::new(far_id), alpn::DESK).await.unwrap();
    let link: Arc<dyn Link> = Arc::new(NoDatagrams(real));

    let (hub_tx, mut hub_rx) = mpsc::unbounded_channel::<Msg>();
    let (dg_tx, dg_rx) = mpsc::channel::<Datagram>(8);
    let shared = Arc::new(LinkShared::default());
    let stream = link.open_bi().await.unwrap();
    let (_ctl_tx, ctl_rx) = mpsc::unbounded_channel::<StreamMsg>();
    tokio::spawn(run_tasks(dut.link.inner.clone(), hub_tx, link.clone(), far_id, 7, shared, stream, ctl_rx, dg_rx));

    dg_tx.send(Datagram::Motion { seq: 1, dx: 4, dy: 0, sent_us: 0 }).await.unwrap();
    let ended = timeout(Duration::from_secs(5), async {
        loop {
            match hub_rx.recv().await {
                Some(Msg::Ended { peer, link_id }) => return Some((peer, link_id)),
                Some(_) => continue,
                None => return None,
            }
        }
    })
    .await;
    assert_eq!(ended, Ok(Some((far_id, 7))), "the hub hears that the link ended");
    assert_eq!(link.closed().await, CLOSE_PROTOCOL, "closed the way the other fatal io errors close");
}

// ----- less work per motion event -----

#[tokio::test(start_paused = true)]
async fn a_thousand_motions_do_not_make_the_page_publish() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    let (bx, _) = b.host.cursor();
    let before = a.link.inner.publishes.load(std::sync::atomic::Ordering::SeqCst);
    for _ in 0..1000 {
        a.host.person(Capture::Motion { dx: 1, dy: 0 });
        tokio::task::yield_now().await;
    }
    wait_until("all the motion arrives", || b.host.cursor().0 >= bx + 1000).await;
    let after = a.link.inner.publishes.load(std::sync::atomic::Ordering::SeqCst);
    assert!(after - before <= 3, "{} page publishes for 1000 motions", after - before);
}

/// A computer that is already holding the mouse (or controlled by another) answers a crossing with "busy" and Leave,
/// and the sender stops pushing at it for a while.
#[tokio::test(start_paused = true)]
async fn a_busy_computer_says_busy_and_the_sender_backs_off() {
    let (net, a, b) = together();
    let c = pc(&net, "Tablet");
    pair(&a, &c);
    allow(&a, &c, &ALL);
    allow(&c, &a, &ALL);
    up(&a, &b).await;
    c.link.start();
    wait_until("the tablet is online", || state_of(&a, &c) == "online" && state_of(&c, &a) == "online").await;
    knows_screens(&a, &b).await;
    knows_screens(&a, &c).await;
    // The tablet sits on this computer's left: its right edge leads here.
    a.link.set_side(&c.id, "left").unwrap();
    c.link.set_side(&a.id, "right").unwrap();
    wait_until("the tablet knows this computer is on its right", || c.link.view().devices.iter().any(|d| d.node == a.id.to_hex() && d.side == "right")).await;
    // This computer holds the mouse toward the laptop.
    cross(&a, &b).await;
    // The tablet pushes into this computer, which is busy.
    c.host.put_cursor(1919, 540);
    wait_until("the tablet is refused", || c.link.view().error.as_deref().is_some_and(|e| e.contains("busy"))).await;
    assert!(!c.host.capturing(), "the tablet's pointer came home");
    assert!(a.host.capturing() && b.link.view().devices[0].controls_me, "the laptop keeps the mouse");
    // No new try while the pause lasts.
    let tries = c.host.with(|s| s.captures_started);
    for _ in 0..10 {
        c.host.put_cursor(1919, 540);
        sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(c.host.with(|s| s.captures_started), tries, "tried again inside the pause");
}
