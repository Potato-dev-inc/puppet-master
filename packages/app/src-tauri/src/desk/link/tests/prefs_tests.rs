//! Keys and scrolling chosen per computer: they change what crosses, only for that computer, are checked, kept, and
//! shown on the page's view.

use super::super::prefs::{FilePrefsStore, MemPrefsStore, PeerPrefs, PrefsStore};
use super::*;

const MAC: NodeId = NodeId([0x66; 32]);
const OTHER: NodeId = NodeId([0x67; 32]);
const HID_ALT: u16 = 0xE2;
const HID_GUI: u16 = 0xE3;

/// A Mac that sends these keys to `dut`, which then holds what arrived.
async fn mac_sends(net: &MemoryNet, dut: &Pc, hids: &[u16]) -> Raw {
    let mut raw = raw_peer(net, dut, OsKind::Macos, MAC).await;
    let _ = read_frame(&mut raw.ctl.recv, CONTROL_FRAME_MAX).await.unwrap();
    write_frame(&mut raw.ctl.send, &StreamMsg::Enter { monitor_id: "left".into(), nx_milli: 0, ny_milli: 500 }).await.unwrap();
    for hid in hids {
        write_frame(&mut raw.ctl.send, &StreamMsg::Key { hid: *hid, down: true }).await.unwrap();
    }
    raw
}

fn prefs_of(p: &Pc, peer: &NodeId) -> PeerPrefs {
    p.link.inner.peer_prefs(peer)
}

// ----- keys -----

#[tokio::test(start_paused = true)]
async fn without_a_choice_command_still_becomes_alt_and_option_becomes_the_windows_key() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    let _raw = mac_sends(&net, &dut, &[HID_GUI, HID_ALT]).await;
    wait_until("mapped", || dut.host.held_keys().len() == 2).await;
    assert_eq!(dut.host.held_keys(), [HID_ALT, HID_GUI].into());
    assert_eq!(prefs_of(&dut, &MAC), PeerPrefs::default());
}

#[tokio::test(start_paused = true)]
async fn turning_the_command_swap_off_for_a_computer_lets_command_through_as_the_windows_key() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    dut.trust.add(MAC, [9; 32], "Raw", 1).unwrap();
    dut.link.update_peer_prefs(&MAC, |p| p.alt_cmd = false).unwrap();
    let _raw = mac_sends(&net, &dut, &[HID_GUI]).await;
    wait_until("arrived", || !dut.host.held_keys().is_empty()).await;
    // Command stays Command (the Windows key's usage), it is not turned into Alt.
    assert_eq!(dut.host.held_keys(), [HID_GUI].into());
}

#[tokio::test(start_paused = true)]
async fn turning_the_option_swap_off_leaves_option_as_alt() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    dut.trust.add(MAC, [9; 32], "Raw", 1).unwrap();
    dut.link.update_peer_prefs(&MAC, |p| p.win_option = false).unwrap();
    let _raw = mac_sends(&net, &dut, &[HID_ALT]).await;
    wait_until("arrived", || !dut.host.held_keys().is_empty()).await;
    // Option stays Alt (with the swap on it would have become the Windows key).
    assert_eq!(dut.host.held_keys(), [HID_ALT].into());
}

#[tokio::test(start_paused = true)]
async fn a_choice_for_one_computer_does_not_touch_another() {
    let net = MemoryNet::new();
    let dut = pc(&net, "Ren-PC");
    dut.trust.add(OTHER, [8; 32], "Other", 1).unwrap();
    dut.link.update_peer_prefs(&OTHER, |p| {
        p.alt_cmd = false;
        p.natural = true;
    })
    .unwrap();
    assert_eq!(prefs_of(&dut, &MAC), PeerPrefs::default());
    assert!(!prefs_of(&dut, &OTHER).alt_cmd);
    let _raw = mac_sends(&net, &dut, &[HID_GUI]).await;
    wait_until("mapped for the Mac", || dut.host.held_keys().contains(&HID_ALT)).await;
}

// ----- scrolling -----

#[tokio::test(start_paused = true)]
async fn scrolling_out_is_flipped_and_scaled_for_that_computer_only() {
    let (_net, a, b) = together();
    a.link
        .update_peer_prefs(&b.id, |p| {
            p.natural = true;
            p.speed = 2.0;
        })
        .unwrap();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Wheel { dx: 0, dy: -120 });
    wait_until("wheel arrives", || b.host.with(|s| s.log.contains(&Inject::Wheel { dx: 0, dy: 240 }))).await;
}

#[tokio::test(start_paused = true)]
async fn scrolling_in_is_flipped_when_that_computer_uses_this_one() {
    let (_net, a, b) = together();
    b.link.update_peer_prefs(&a.id, |p| p.reverse_incoming = true).unwrap();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Wheel { dx: 5, dy: -120 });
    wait_until("wheel arrives flipped", || b.host.with(|s| s.log.contains(&Inject::Wheel { dx: -5, dy: 120 }))).await;
}

#[tokio::test(start_paused = true)]
async fn the_wheel_is_untouched_until_the_person_chooses() {
    let (_net, a, b) = together();
    up(&a, &b).await;
    cross(&a, &b).await;
    a.host.person(Capture::Wheel { dx: 0, dy: -120 });
    wait_until("wheel arrives as is", || b.host.with(|s| s.log.contains(&Inject::Wheel { dx: 0, dy: -120 }))).await;
}

// ----- checks, saving, the view -----

#[tokio::test(start_paused = true)]
async fn a_scroll_speed_that_cannot_work_is_refused_and_nothing_changes() {
    let (_net, a, b) = together();
    for bad in [0.0, -1.0, 0.1, 4.5, f64::NAN, f64::INFINITY] {
        let err = a.link.update_peer_prefs(&b.id, |p| p.speed = bad).unwrap_err();
        assert!(err.contains("between 0.25 and 4"), "{bad}: {err}");
    }
    assert_eq!(prefs_of(&a, &b.id), PeerPrefs::default());
    a.link.update_peer_prefs(&b.id, |p| p.speed = 0.25).unwrap();
    a.link.update_peer_prefs(&b.id, |p| p.speed = 4.0).unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_view_carries_each_computers_choices_and_changes_when_they_do() {
    let (_net, a, b) = together();
    assert_eq!(a.link.view().devices[0].prefs, PeerPrefs::default());
    let seq = a.link.view().seq;
    a.link.update_peer_prefs(&b.id, |p| p.win_option = false).unwrap();
    let v = a.link.view();
    assert!(!v.devices[0].prefs.win_option && v.devices[0].prefs.alt_cmd);
    assert!(v.seq > seq, "the page is told");
}

#[tokio::test(start_paused = true)]
async fn choices_are_kept_and_a_computer_back_at_the_defaults_leaves_no_trace() {
    let (_net, a, b) = together();
    let store = Arc::new(MemPrefsStore::default());
    a.link.use_prefs_store(store.clone());
    a.link.update_peer_prefs(&b.id, |p| p.speed = 1.5).unwrap();
    assert_eq!(store.load().get(&b.id.to_hex()).map(|p| p.speed), Some(1.5));
    // A new run of the app reads them back.
    let (_net2, a2, _b2) = together();
    a2.link.use_prefs_store(store.clone());
    assert_eq!(prefs_of(&a2, &b.id).speed, 1.5);
    a.link.update_peer_prefs(&b.id, |p| *p = PeerPrefs::default()).unwrap();
    assert!(store.load().is_empty());
}

#[test]
fn the_file_is_plain_json_next_to_the_layout_and_a_bad_speed_in_it_means_normal_speed() {
    let dir = std::env::temp_dir().join(format!("pm-desk-prefs-{}", std::process::id()));
    let path = dir.join("desk").join("peers.json");
    let store = FilePrefsStore { path: path.clone() };
    assert!(store.load().is_empty(), "no file yet");
    let mut map = std::collections::BTreeMap::new();
    map.insert("ab".repeat(32), PeerPrefs { alt_cmd: false, speed: 2.0, ..PeerPrefs::default() });
    store.save(&map).unwrap();
    assert_eq!(store.load(), map);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"alt_cmd\": false") && !text.contains("secret"));
    // Hand-edited into nonsense: the wheel must not freeze.
    std::fs::write(&path, text.replace("2.0", "0.0")).unwrap();
    assert_eq!(store.load().values().next().unwrap().speed, 1.0);
    // Missing fields (an older file) take their defaults.
    std::fs::write(&path, format!("{{\"{}\":{{\"natural\":true}}}}", "cd".repeat(32))).unwrap();
    let p = *store.load().values().next().unwrap();
    assert!(p.natural && p.alt_cmd && p.win_option && p.speed == 1.0);
    let _ = std::fs::remove_dir_all(&dir);
}
