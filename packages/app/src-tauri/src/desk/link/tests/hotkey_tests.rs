//! What the hotkeys do to the link (the keys themselves are tested in `desk::tests::new_hotkeys`): jump to the Nth
//! computer, bring the pointer home, and the pointer lock that keeps it from crossing.

use desk_core::parse_chord;

use super::*;
use crate::desk::port::{LinkAction, LinkPort};

fn with_port(p: &Pc) -> Arc<LinkPort> {
    let port = LinkPort::new();
    p.link.attach_port(port.clone());
    port
}

#[tokio::test(start_paused = true)]
async fn jump_crosses_to_the_numbered_computer_without_the_pointer_touching_an_edge() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    up(&a, &b).await;
    a.host.put_cursor(960, 540);
    sleep(Duration::from_millis(200)).await;
    assert!(!a.host.capturing());
    port.request(LinkAction::Jump(1)).unwrap();
    wait_until("crossed", || a.host.capturing() && b.link.view().devices[0].controls_me).await;
    // It enters b in the middle of the edge facing a.
    let (bx, by) = b.host.cursor();
    assert!(bx <= 6 && (by - 540).abs() <= 3, "{bx},{by}");
    assert_eq!(a.link.view().mouse_on.as_deref(), Some("Laptop"));
    assert_eq!(a.link.view().error, None);
    // And what is typed now goes to b.
    a.host.person(Capture::Key { hid: 0x04, down: true });
    wait_until("typed there", || b.host.held_keys().contains(&0x04)).await;
}

#[tokio::test(start_paused = true)]
async fn jump_to_a_number_nobody_has_says_so_and_stays_put() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    up(&a, &b).await;
    port.request(LinkAction::Jump(2)).unwrap();
    wait_until("told", || a.link.view().error.is_some()).await;
    assert!(a.link.view().error.unwrap().contains("There is no computer 2"));
    assert!(!a.host.capturing());
}

#[tokio::test(start_paused = true)]
async fn jump_to_a_computer_that_is_not_connected_says_which() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    // Only a is started: b never comes up.
    a.link.start();
    sleep(Duration::from_millis(300)).await;
    port.request(LinkAction::Jump(1)).unwrap();
    wait_until("told", || a.link.view().error.is_some()).await;
    assert_eq!(a.link.view().error.as_deref(), Some("Laptop is not connected"));
    drop(b);
}

#[tokio::test(start_paused = true)]
async fn jump_is_refused_while_paused_or_locked() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    up(&a, &b).await;
    port.set_locked(true);
    port.request(LinkAction::Jump(1)).unwrap();
    wait_until("told", || a.link.view().error.is_some()).await;
    assert!(a.link.view().error.unwrap().contains("locked"));
    assert!(!a.host.capturing());
    port.set_locked(false);
    a.link.pause(true);
    sleep(Duration::from_millis(50)).await;
    port.request(LinkAction::Jump(1)).unwrap();
    wait_until("told paused", || a.link.view().error.as_deref() == Some("Mouse sharing is paused")).await;
    assert!(!a.host.capturing());
}

#[tokio::test(start_paused = true)]
async fn return_home_brings_the_pointer_back_from_the_other_computer() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Key { hid: 0x04, down: true });
    wait_until("held on b", || b.host.held_keys().contains(&0x04)).await;
    port.request(LinkAction::ReturnHome).unwrap();
    wait_until("home", || !a.host.capturing()).await;
    wait_until("released on b", || b.host.held_keys().is_empty()).await;
    assert!(!b.link.view().devices[0].controls_me);
    assert_eq!(a.link.view().mouse_on, None);
    // Not paused: it is a way back, not a stop.
    assert!(!a.link.view().paused);
}

#[tokio::test(start_paused = true)]
async fn return_home_from_the_computer_being_controlled_takes_the_pointer_back_too() {
    let (_net, a, b) = together();
    let port_b = with_port(&b);
    up(&a, &b).await;
    cross(&a, &b).await;
    port_b.request(LinkAction::ReturnHome).unwrap();
    wait_until("a is home", || !a.host.capturing()).await;
    wait_until("b is free", || !b.link.view().devices[0].controls_me).await;
}

#[tokio::test(start_paused = true)]
async fn the_return_chord_is_matched_on_the_keys_captured_while_the_pointer_is_away() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    port.set_return_chord(parse_chord("ctrl+alt+shift+h"));
    up(&a, &b).await;
    cross(&a, &b).await;
    // Only Ctrl+Alt+H: not the chord, it goes to b like any other typing.
    a.host.person(Capture::Key { hid: 0xE0, down: true });
    a.host.person(Capture::Key { hid: 0xE2, down: true });
    a.host.person(Capture::Key { hid: 0x0B, down: true });
    wait_until("typed on b", || b.host.held_keys().contains(&0x0B)).await;
    assert!(a.host.capturing());
    a.host.person(Capture::Key { hid: 0x0B, down: false });
    // Add Shift and press H again: home, and that H is not sent.
    a.host.person(Capture::Key { hid: 0xE1, down: true });
    a.host.person(Capture::Key { hid: 0x0B, down: true });
    wait_until("home", || !a.host.capturing()).await;
    wait_until("released on b", || b.host.held_keys().is_empty()).await;
    assert!(!b.host.with(|s| s.log.iter().filter(|e| matches!(e, Inject::Key { hid: 0x0B, down: true })).count() > 1));
}

#[tokio::test(start_paused = true)]
async fn a_locked_pointer_does_not_cross_and_crosses_again_once_free() {
    let (_net, a, b) = together();
    let port = with_port(&a);
    up(&a, &b).await;
    port.set_locked(true);
    a.host.put_cursor(1919, 540);
    sleep(Duration::from_millis(300)).await;
    assert!(!a.host.capturing(), "locked: the edge is only an edge");
    port.set_locked(false);
    wait_until("crosses", || a.host.capturing()).await;
}
