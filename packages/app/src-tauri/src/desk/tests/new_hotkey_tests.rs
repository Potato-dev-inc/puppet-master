//! Center, lock the pointer to a screen, jump to a computer, return home: the keys, their checks, and what the
//! engine does with them. Mock host only.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::*;
use crate::desk::port::LinkAction;

const SHIFT: u16 = 0xE1;
const KEY_C: u16 = 0x06;
const SCROLL_LOCK: u16 = 0x47;
const KEY_H: u16 = 0x0B;
const KEY_2: u16 = 0x1F;

fn main_screen_host(cursor: (i32, i32)) -> MockHost {
    MockHost::new(three(), cursor)
}

fn near(a: (i32, i32), b: (i32, i32)) -> bool {
    (a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1
}

// ---- the checks ----------------------------------------------------------------------------------------------

#[test]
fn a_file_saved_before_these_keys_existed_still_loads_with_its_own_keys_kept() {
    let old = r#"{"hops_enabled":true,"hop_mode":"relative","hotkeys":{"next":"ctrl+alt+up","prev":"ctrl+alt+down","screen_n":"ctrl+alt+{n}","find_pointer":"ctrl+alt+f","pause":"scrolllock"}}"#;
    let loaded = config::parse(old);
    assert_eq!(loaded.note, None, "an older file is not 'not valid'");
    let h = loaded.config.hotkeys;
    assert_eq!(h.next, "ctrl+alt+up");
    assert_eq!(h.center, HotkeyConfig::default().center);
    assert_eq!(h.jump_n, HotkeyConfig::default().jump_n);
    // Saved values win: Scroll Lock stays the pause key. The lock key was not saved, so it keeps the key it had
    // before (the new default would be the same key and two actions cannot share one).
    assert_eq!(h.pause, "scrolllock");
    assert_eq!(h.lock_pointer, "ctrl+alt+l");
    assert!(loaded.config.hops_enabled);
}

#[test]
fn the_defaults_are_scroll_lock_to_lock_the_pointer_and_ctrl_alt_p_to_pause() {
    let d = HotkeyConfig::default();
    assert_eq!(d.lock_pointer, "scrolllock");
    assert_eq!(d.pause, "ctrl+alt+p");
    assert!(compile(&d).is_ok());
}

#[test]
fn a_file_that_saved_both_keys_as_scroll_lock_is_refused_with_a_note_and_its_other_settings_stay() {
    let text = r#"{"hops_enabled":true,"hotkeys":{"next":"ctrl+alt+up","pause":"scrolllock","lock_pointer":"scrolllock"}}"#;
    let loaded = config::parse(text);
    assert_eq!(loaded.config.hotkeys, HotkeyConfig::default(), "the hotkeys fall back as before, with a note");
    assert!(loaded.note.is_some());
    assert!(loaded.config.hops_enabled, "the rest of the file is kept");
}

#[test]
fn the_new_keys_compile_to_their_actions_and_none_shares_keys_with_another() {
    let b = compile(&HotkeyConfig::default()).unwrap();
    for action in [
        DeskAction::Center,
        DeskAction::LockPointer,
        DeskAction::ReturnHome,
        DeskAction::Jump(1),
        DeskAction::Jump(9),
    ] {
        assert!(b.iter().any(|x| x.action == action), "{action:?}");
    }
    for (i, x) in b.iter().enumerate() {
        assert!(b.iter().skip(i + 1).all(|y| y.chord != x.chord), "{:?} shares keys", x.action);
    }
}

#[test]
fn jump_needs_the_number_placeholder_like_screens_do() {
    let mut cfg = HotkeyConfig::default();
    cfg.jump_n = "ctrl+alt+shift+j".into();
    let problems = compile(&cfg).unwrap_err();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].field, "jump_n");
}

#[test]
fn the_new_keys_are_checked_like_the_old_ones() {
    let mut cfg = HotkeyConfig::default();
    cfg.center = cfg.next.clone();
    let p = compile(&cfg).unwrap_err();
    assert_eq!(p[0].field, "center");
    assert!(p[0].message.contains("Same keys"));

    let mut cfg = HotkeyConfig::default();
    cfg.lock_pointer = "l".into();
    assert_eq!(compile(&cfg).unwrap_err()[0].field, "lock_pointer");

    let mut cfg = HotkeyConfig::default();
    cfg.return_home = "ctrl+alt+banana".into();
    assert_eq!(compile(&cfg).unwrap_err()[0].field, "return_home");
}

#[test]
fn the_matcher_fires_the_new_actions_and_keeps_jump_apart_from_screen() {
    let mut m = matcher();
    m.on_key(CTRL, true, false);
    m.on_key(ALT, true, false);
    assert_eq!(m.on_key(KEY_C, true, false), Verdict::Fire(DeskAction::Center));
    m.on_key(KEY_C, false, false);
    m.on_key(CTRL, false, false);
    m.on_key(ALT, false, false);
    assert_eq!(m.on_key(SCROLL_LOCK, true, false), Verdict::Fire(DeskAction::LockPointer));
    m.on_key(SCROLL_LOCK, false, false);
    m.on_key(CTRL, true, false);
    m.on_key(ALT, true, false);
    assert_eq!(m.on_key(KEY_2, true, false), Verdict::Fire(DeskAction::Screen(2)));
    m.on_key(KEY_2, false, false);
    m.on_key(SHIFT, true, false);
    assert_eq!(m.on_key(KEY_2, true, false), Verdict::Fire(DeskAction::Jump(2)));
    m.on_key(KEY_2, false, false);
    assert_eq!(m.on_key(KEY_H, true, false), Verdict::Fire(DeskAction::ReturnHome));
}

#[test]
fn the_snapshot_lists_the_new_bindings_by_name() {
    let host = main_screen_host((100, 100));
    let desk = desk_on(&host);
    let names: Vec<String> = desk.snapshot().unwrap().hotkeys.bindings.iter().map(|b| b.action.clone()).collect();
    for n in ["center", "lock", "return_home", "jump:1", "jump:9"] {
        assert!(names.iter().any(|x| x == n), "{n}");
    }
}

// ---- center --------------------------------------------------------------------------------------------------

#[test]
fn the_center_key_puts_the_pointer_in_the_middle_of_the_screen_it_is_on() {
    let host = main_screen_host((100, 100));
    let _desk = desk_hops(&host);
    press_chord(&host, &[CTRL, ALT], KEY_C);
    wait_until("centered", || near(host.cursor(), (960, 540)));
    // On the right-hand screen (raised by 200 px) it stays on that screen.
    host.with(|s| s.cursor = (2000, 100));
    press_chord(&host, &[CTRL, ALT], KEY_C);
    wait_until("centered on the right screen", || near(host.cursor(), (3200, 520)));
}

#[test]
fn the_center_command_does_the_same_without_a_key() {
    let host = main_screen_host((100, 100));
    let desk = desk_on(&host);
    desk.command(Cmd::Center).unwrap();
    assert!(near(host.cursor(), (960, 540)), "{:?}", host.cursor());
}

// ---- lock ----------------------------------------------------------------------------------------------------

#[test]
fn locking_holds_the_pointer_on_its_screen_and_says_so_everywhere() {
    let host = main_screen_host((100, 100));
    let desk = desk_on(&host);
    let port = desk.port();
    let snap = desk.command(Cmd::SetPointerLock { locked: true }).unwrap();
    assert!(snap.status.pointer_locked);
    assert!(port.is_locked(), "the link reads the same flag");
    // Dragged off to the next screen, it is put back on this one.
    host.with(|s| s.cursor = (2500, 600));
    wait_until("pulled back", || host.cursor().0 < 1920);
    assert_eq!(host.cursor(), (1919, 600));
    // A hop to another screen is refused with a reason; the middle of this screen is not a hop away.
    let err = desk.command(Cmd::HopNext).unwrap_err();
    assert!(err.contains("locked"), "{err}");
    desk.command(Cmd::Center).unwrap();
    let snap = desk.command(Cmd::SetPointerLock { locked: false }).unwrap();
    assert!(!snap.status.pointer_locked && !port.is_locked());
    desk.command(Cmd::HopNext).unwrap();
}

#[test]
fn the_lock_key_toggles() {
    let host = main_screen_host((100, 100));
    let desk = desk_hops(&host);
    press_chord(&host, &[], SCROLL_LOCK);
    wait_until("locked", || desk.port().is_locked());
    assert!(desk.port().is_locked());
    press_chord(&host, &[], SCROLL_LOCK);
    wait_until("free", || !desk.port().is_locked());
    assert!(!desk.port().is_locked());
}

#[test]
fn pause_and_labs_off_free_a_locked_pointer() {
    let host = main_screen_host((100, 100));
    let desk = desk_on(&host);
    desk.command(Cmd::SetPointerLock { locked: true }).unwrap();
    desk.command(Cmd::Pause { paused: true }).unwrap();
    assert!(!desk.port().is_locked());
    assert!(!desk.snapshot().unwrap().status.pointer_locked);
    host.with(|s| s.cursor = (2500, 600));
    thread::sleep(Duration::from_millis(150));
    assert_eq!(host.cursor(), (2500, 600), "paused: the pointer is nobody's to hold");

    desk.command(Cmd::Pause { paused: false }).unwrap();
    desk.command(Cmd::SetPointerLock { locked: true }).unwrap();
    desk.apply_settings(&labs(false), true);
    assert!(!desk.port().is_locked());
}

// ---- jump and return home ------------------------------------------------------------------------------------

fn record(desk: &Desk) -> Arc<Mutex<Vec<LinkAction>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    desk.port().set_handler(Some(Arc::new(move |a| {
        s.lock().unwrap().push(a);
        Ok(())
    })));
    seen
}

#[test]
fn the_jump_and_return_keys_ask_the_link() {
    let host = main_screen_host((100, 100));
    let desk = desk_hops(&host);
    let seen = record(&desk);
    press_chord(&host, &[CTRL, ALT, SHIFT], KEY_2);
    press_chord(&host, &[CTRL, ALT, SHIFT], KEY_H);
    wait_until("both asked", || seen.lock().unwrap().len() == 2);
    assert_eq!(*seen.lock().unwrap(), vec![LinkAction::Jump(2), LinkAction::ReturnHome]);
    assert!(host.with(|s| s.moves.is_empty()), "the pointer is not moved by this computer");
}

#[test]
fn a_jump_with_no_link_says_so_instead_of_doing_nothing() {
    let host = main_screen_host((100, 100));
    let desk = desk_hops(&host);
    press_chord(&host, &[CTRL, ALT, SHIFT], KEY_2);
    wait_until("told", || desk.snapshot().unwrap().status.error.is_some());
    assert!(desk.snapshot().unwrap().status.error.unwrap().contains("No other computer"));
}

#[test]
fn the_link_is_told_the_return_chord_so_it_can_match_it_while_the_pointer_is_away() {
    let host = main_screen_host((100, 100));
    let desk = desk_on(&host);
    let held: BTreeSet<u16> = [CTRL, ALT, SHIFT].into();
    assert!(desk.port().is_return_home(&held, KEY_H));
    let mut cfg = HotkeyConfig::default();
    cfg.return_home = "ctrl+alt+shift+r".into();
    desk.command(Cmd::SetHotkeys { hotkeys: cfg }).unwrap();
    assert!(!desk.port().is_return_home(&held, KEY_H));
    assert!(desk.port().is_return_home(&held, 0x15));
    desk.command(Cmd::ResetHotkeys).unwrap();
    assert!(desk.port().is_return_home(&held, KEY_H));
}
