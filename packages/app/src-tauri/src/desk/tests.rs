//! Tests for My computers. Every test uses `MockHost`: no real hook, no real cursor, no real key.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use desk_core::{HopAction, HopMode, PositionMemory};
use serde_json::json;

use super::engine::config::{self, ConfigStore, MemStore};
use super::engine::hops::{self, HopError};
use super::engine::hotkeys::{
    compile, new_table, Binding, DeskAction, HotkeyConfig, HotkeyMatcher, Verdict,
};
use super::engine::{Cmd, Snapshot};
use super::host::mock::{display, MockHost};
use super::host::{hook_looks_dropped, DeskHost, HookHealth};
use super::{labs_on, mem_store, Desk, FEATURE_OFF};

// ---- helpers -------------------------------------------------------------------------------------------------

const TICK: Duration = Duration::from_millis(20);

/// Three screens: a 125% one left of the main screen (negative x), the main 100% one, a 150% one right of it,
/// raised by 200 px (negative y).
fn three() -> Vec<super::host::DisplayInfo> {
    vec![
        display("D1", 0, 0, 1920, 1080, 1.0, true),
        display("D2", -1280, 0, 1280, 1024, 1.25, false),
        display("D3", 1920, -200, 2560, 1440, 1.5, false),
    ]
}

fn labs(on: bool) -> serde_json::Value {
    json!({ "desk_enabled": on })
}

fn desk_for(host: &MockHost, store: Arc<dyn ConfigStore>) -> Desk {
    Desk::new(Arc::new(host.clone()), store, TICK)
}

/// A desk with Labs on and "Screen hops" off (the default).
fn desk_on(host: &MockHost) -> Desk {
    let d = desk_for(host, mem_store(None));
    d.apply_settings(&labs(true), true);
    d
}

/// A desk with Labs on and "Screen hops" on, hooks installed.
fn desk_hops(host: &MockHost) -> Desk {
    let d = desk_on(host);
    d.command(Cmd::SetHopsEnabled { enabled: true }).unwrap();
    assert!(host.hook_installed());
    d
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        if f() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for: {what}");
}

fn press_chord(host: &MockHost, mods: &[u16], key: u16) {
    for m in mods {
        host.key(*m, true, false);
    }
    host.key(key, true, false);
    host.key(key, false, false);
    for m in mods.iter().rev() {
        host.key(*m, false, false);
    }
}

const CTRL: u16 = 0xE0;
const ALT: u16 = 0xE2;
const RIGHT: u16 = 0x4F;
const LEFT: u16 = 0x50;

// ---- hop geometry (pure) -------------------------------------------------------------------------------------

#[test]
fn screens_are_numbered_left_to_right_even_with_negative_origins() {
    let layout = hops::layout_of(&three());
    let ids: Vec<String> = hops::order(&layout).iter().map(|m| m.0.clone()).collect();
    assert_eq!(ids, ["D2", "D1", "D3"]);
}

#[test]
fn next_and_prev_keep_the_same_place_across_different_dpi_and_negative_coordinates() {
    let layout = hops::layout_of(&three());
    let mut memory = PositionMemory::default();
    // Middle of the main screen to the next screen on the right (150%, raised by 200 px).
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (960, 540), HopAction::Next).unwrap();
    assert_eq!((t.screen_id.as_str(), t.screen_index), ("D3", 3));
    assert_eq!((t.x, t.y), (3200, 520));
    // Next from the last one wraps to the first, whose x is negative.
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (3200, 520), HopAction::Next).unwrap();
    assert_eq!((t.screen_id.as_str(), t.screen_index), ("D2", 1));
    assert_eq!((t.x, t.y), (-640, 512));
    // Prev from the first wraps to the last.
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (-640, 512), HopAction::Prev).unwrap();
    assert_eq!(t.screen_id, "D3");
    // Every destination lies inside its screen.
    for n in 1..=3u8 {
        let t = hops::plan(&layout, HopMode::Relative, &mut memory, (0, 0), HopAction::Number(n)).unwrap();
        let m = layout.monitor(&desk_core::MonitorId(t.screen_id.clone())).unwrap();
        assert!(m.bounds.contains(t.x, t.y), "{t:?}");
        assert_eq!(t.screen_index, usize::from(n));
    }
}

#[test]
fn one_screen_hops_stay_where_they_are() {
    let layout = hops::layout_of(&[display("D1", 0, 0, 1920, 1080, 1.0, true)]);
    let mut memory = PositionMemory::default();
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (100, 200), HopAction::Next).unwrap();
    assert_eq!((t.x, t.y), (100, 200));
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (100, 200), HopAction::Prev).unwrap();
    assert_eq!((t.x, t.y), (100, 200));
}

#[test]
fn two_screens_hop_both_ways_and_to_a_number() {
    let layout = hops::layout_of(&[
        display("A", 0, 0, 1000, 800, 1.0, true),
        display("B", 1000, 0, 2000, 1600, 2.0, false),
    ]);
    let mut memory = PositionMemory::default();
    let to_b = hops::plan(&layout, HopMode::Relative, &mut memory, (0, 0), HopAction::Next).unwrap();
    assert_eq!((to_b.x, to_b.y), (1000, 0));
    let to_a = hops::plan(&layout, HopMode::Relative, &mut memory, (2999, 1599), HopAction::Prev).unwrap();
    assert_eq!((to_a.x, to_a.y), (999, 799));
    let two = hops::plan(&layout, HopMode::Relative, &mut memory, (10, 10), HopAction::Number(2)).unwrap();
    assert_eq!(two.screen_id, "B");
}

// Known DevHop bug: the pointer in a gap between screens (or on a screen that just went away) made every hop
// do nothing, because the current screen could not be found.
#[test]
fn bug_pointer_in_a_gap_counts_as_on_the_nearest_screen() {
    let layout = hops::layout_of(&[
        display("A", 0, 0, 1000, 1000, 1.0, true),
        display("B", 1200, 0, 1000, 1000, 1.0, false),
    ]);
    let mut memory = PositionMemory::default();
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (1050, 500), HopAction::Next).unwrap();
    assert_eq!(t.screen_id, "B");
    assert_eq!((t.x, t.y), (2199, 500));
}

// Known DevHop bug: mirrored displays were two screens, so "next" moved to a copy of the same picture.
#[test]
fn bug_mirrored_displays_are_one_screen() {
    let displays = vec![
        display("D1", 0, 0, 1920, 1080, 1.0, true),
        display("D2", 0, 0, 1920, 1080, 1.0, false),
        display("D3", 1920, 0, 1920, 1080, 1.0, false),
    ];
    let layout = hops::layout_of(&displays);
    assert_eq!(layout.monitors.len(), 2);
    let mut memory = PositionMemory::default();
    let t = hops::plan(&layout, HopMode::Relative, &mut memory, (10, 10), HopAction::Next).unwrap();
    assert_eq!(t.screen_id, "D3");
}

#[test]
fn zero_size_screens_are_ignored() {
    let layout = hops::layout_of(&[display("D1", 0, 0, 1920, 1080, 1.0, true), display("Z", 5000, 0, 0, 0, 1.0, false)]);
    assert_eq!(layout.monitors.len(), 1);
}

// Known DevHop bug: "screen 5" with three screens did nothing and said nothing.
#[test]
fn bug_screen_number_past_the_count_is_an_error_with_the_count() {
    let layout = hops::layout_of(&three());
    let mut memory = PositionMemory::default();
    let err = hops::plan(&layout, HopMode::Relative, &mut memory, (0, 0), HopAction::Number(5)).unwrap_err();
    assert_eq!(err, HopError::NoSuchScreen { wanted: 5, have: 3 });
    assert!(err.to_string().contains("3"), "{err}");
    assert_eq!(hops::plan(&layout, HopMode::Relative, &mut memory, (0, 0), HopAction::Number(0)).unwrap_err(), HopError::NoSuchScreen { wanted: 0, have: 3 });
}

#[test]
fn memory_mode_returns_to_the_spot_last_used_on_a_screen() {
    let layout = hops::layout_of(&[
        display("A", 0, 0, 1000, 1000, 1.0, true),
        display("B", 1000, 0, 1000, 1000, 1.0, false),
    ]);
    let mut memory = PositionMemory::default();
    // Leave A from (100, 200), come back from somewhere else on B.
    hops::plan(&layout, HopMode::Memory, &mut memory, (100, 200), HopAction::Next).unwrap();
    let back = hops::plan(&layout, HopMode::Memory, &mut memory, (1900, 900), HopAction::Prev).unwrap();
    assert_eq!((back.x, back.y), (100, 200));
}

#[test]
fn no_screens_is_an_error() {
    let layout = hops::layout_of(&[]);
    let mut memory = PositionMemory::default();
    assert_eq!(hops::plan(&layout, HopMode::Relative, &mut memory, (0, 0), HopAction::Next).unwrap_err(), HopError::NoScreens);
}

// ---- commands through the engine -----------------------------------------------------------------------------

#[test]
fn hop_commands_move_the_mock_cursor_and_report_the_screen() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_on(&host);
    let snap = desk.command(Cmd::HopNext).unwrap();
    assert_eq!(host.cursor(), (3200, 520));
    let p = snap.pointer.unwrap();
    assert_eq!((p.x, p.y, p.screen_index), (3200, 520, Some(3)));
    desk.command(Cmd::HopPrev).unwrap();
    assert_eq!(host.cursor().0 > 0 && host.cursor().0 < 1920, true);
    let snap = desk.command(Cmd::HopTo { screen: 1 }).unwrap();
    assert_eq!(snap.pointer.unwrap().screen_id.as_deref(), Some("D2"));
    assert_eq!(snap.screens.iter().map(|s| s.index).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(snap.screens[0].id, "D2");
    assert_eq!(snap.screens[0].x, -1280);
}

#[test]
fn hop_to_a_missing_screen_fails_without_moving() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_on(&host);
    let err = desk.command(Cmd::HopTo { screen: 7 }).unwrap_err();
    assert!(err.contains("no screen 7"), "{err}");
    assert!(host.with(|s| s.moves.is_empty()));
}

// Known DevHop bug: SetCursorPos failure was ignored, so a hop that did nothing looked like it worked.
// Known DevHop bug: the one `error` field was cleared by the 1 s refresh, so messages vanished.
#[test]
fn bug_a_failed_move_is_reported_and_the_error_survives_polling() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_on(&host);
    host.with(|s| s.fail_move = true);
    let err = desk.command(Cmd::HopNext).unwrap_err();
    assert!(err.contains("SetCursorPos"), "{err}");
    for _ in 0..3 {
        let snap = desk.snapshot().unwrap();
        assert!(snap.status.error.as_deref().unwrap().contains("SetCursorPos"), "polling must not clear it");
    }
    host.with(|s| s.fail_move = false);
    let snap = desk.command(Cmd::HopNext).unwrap();
    assert_eq!(snap.status.error, None, "the next command that works clears it");
}

#[test]
fn find_pointer_reports_where_it_is_and_does_not_move_it() {
    let host = MockHost::new(three(), (2000, 100));
    let desk = desk_on(&host);
    let a = desk.command(Cmd::FindPointer).unwrap();
    let b = desk.command(Cmd::FindPointer).unwrap();
    let (fa, fb) = (a.find.unwrap(), b.find.unwrap());
    assert_eq!((fa.x, fa.y, fa.screen_index), (2000, 100, Some(3)));
    assert_eq!(fb.seq, fa.seq + 1, "the page draws the ring when seq changes");
    assert!(fa.announce.contains("screen 3"), "{}", fa.announce);
    assert!(host.with(|s| s.moves.is_empty()), "finding the pointer never moves it");
}

#[test]
fn hop_while_paused_is_refused_and_find_still_works() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_on(&host);
    desk.command(Cmd::Pause { paused: true }).unwrap();
    let err = desk.command(Cmd::HopNext).unwrap_err();
    assert!(err.contains("paused"), "{err}");
    assert!(desk.command(Cmd::FindPointer).is_ok());
    assert!(host.with(|s| s.moves.is_empty()));
}

#[test]
fn unsupported_host_says_so() {
    let host = MockHost::new(vec![], (0, 0));
    host.with(|s| s.unsupported = true);
    let desk = desk_on(&host);
    let snap = desk.snapshot().unwrap();
    assert_eq!(snap.status.state, "unsupported");
    assert!(snap.screens.is_empty());
}

#[test]
fn snapshot_seq_only_goes_up_when_something_changed() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_on(&host);
    let a = desk.snapshot().unwrap();
    let b = desk.snapshot().unwrap();
    assert_eq!(a.seq, b.seq);
    let c = desk.command(Cmd::HopNext).unwrap();
    assert!(c.seq > b.seq);
}

// ---- hotkey settings -----------------------------------------------------------------------------------------

#[test]
fn default_hotkeys_compile_to_next_prev_nine_screens_find_and_pause() {
    let b = compile(&HotkeyConfig::default()).unwrap();
    assert_eq!(b.len(), 4 + 9 + 3 + 9, "next, prev, find, pause, 9 screens, center, lock, return home, 9 computers");
    assert!(b.iter().any(|x| x.action == DeskAction::Screen(9)));
}

// Known DevHop bug: any text was accepted and unparsable chords were dropped silently.
#[test]
fn bug_unparsable_hotkey_text_is_refused_with_the_field() {
    let mut cfg = HotkeyConfig::default();
    cfg.next = "ctrl+alt+banana".into();
    let problems = compile(&cfg).unwrap_err();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].field, "next");
}

// Known DevHop bug: two actions could share a chord; the first in the table won and the other never fired.
#[test]
fn bug_two_actions_on_the_same_keys_are_refused() {
    let mut cfg = HotkeyConfig::default();
    cfg.prev = cfg.next.clone();
    let problems = compile(&cfg).unwrap_err();
    assert!(problems.iter().any(|p| p.field == "prev" && p.message.contains("next")), "{problems:?}");
    // Also a screen number that lands on another action.
    let mut cfg = HotkeyConfig::default();
    cfg.find_pointer = "ctrl+alt+2".into();
    assert!(compile(&cfg).is_err());
}

#[test]
fn keys_that_would_eat_normal_typing_or_system_shortcuts_are_refused() {
    for bad in ["a", "ctrl+c", "shift+x", "ctrl+shift+v", "alt+f4", "ctrl+alt"] {
        let mut cfg = HotkeyConfig::default();
        cfg.find_pointer = bad.into();
        assert!(compile(&cfg).is_err(), "{bad} should be refused");
    }
    for good in ["scrolllock", "ctrl+alt+f", "win+shift+left", "ctrl+f9"] {
        let mut cfg = HotkeyConfig::default();
        cfg.pause = "ctrl+alt+p".into();
        cfg.lock_pointer = "ctrl+alt+l".into();
        cfg.find_pointer = good.into();
        assert!(compile(&cfg).is_ok(), "{good} should be allowed");
    }
}

#[test]
fn screen_pattern_needs_the_number_placeholder() {
    let mut cfg = HotkeyConfig::default();
    cfg.screen_n = "ctrl+alt+7".into();
    let problems = compile(&cfg).unwrap_err();
    assert_eq!(problems[0].field, "screen_n");
    assert!(problems[0].message.contains("{n}"));
}

#[test]
fn a_refused_hotkey_change_leaves_the_old_keys_in_force() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_hops(&host);
    let before = host.with(|s| s.tables_set.len());
    let mut cfg = HotkeyConfig::default();
    cfg.prev = cfg.next.clone();
    let err = desk.command(Cmd::SetHotkeys { hotkeys: cfg }).unwrap_err();
    assert!(err.contains("prev"), "{err}");
    let snap = desk.snapshot().unwrap();
    assert_eq!(snap.hotkeys.config, HotkeyConfig::default());
    assert_eq!(host.with(|s| s.tables_set.len()), before, "the hook table was not touched");
    assert!(snap.status.error.is_some(), "the message stays for the page to show");
}

#[test]
fn a_valid_hotkey_change_reaches_the_running_hook_at_once() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    let mut cfg = HotkeyConfig::default();
    cfg.next = "ctrl+alt+n".into();
    let snap = desk.command(Cmd::SetHotkeys { hotkeys: cfg }).unwrap();
    assert!(snap.hotkeys.bindings.iter().any(|b| b.action == "next" && b.keys == "ctrl+alt+n"));
    // The old keys no longer hop, the new ones do.
    press_chord(&host, &[CTRL, ALT], RIGHT);
    assert!(host.with(|s| s.moves.is_empty()));
    press_chord(&host, &[CTRL, ALT], 0x11);
    wait_until("hop by new keys", || !host.with(|s| s.moves.is_empty()));
    let snap = desk.command(Cmd::ResetHotkeys).unwrap();
    assert_eq!(snap.hotkeys.config, HotkeyConfig::default());
}

// ---- the matcher ---------------------------------------------------------------------------------------------

fn matcher() -> HotkeyMatcher {
    let table = new_table();
    table.store(Arc::new(compile(&HotkeyConfig::default()).unwrap()));
    HotkeyMatcher::new(table)
}

#[test]
fn chord_fires_once_and_eats_press_repeat_and_release() {
    let mut m = matcher();
    assert_eq!(m.on_key(CTRL, true, false), Verdict::Pass);
    assert_eq!(m.on_key(ALT, true, false), Verdict::Pass);
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Fire(DeskAction::Next));
    // Key repeat: eaten, no second hop.
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Swallow);
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Swallow);
    // Known DevHop bug: the release of a swallowed key went through as a stray key-up.
    assert_eq!(m.on_key(RIGHT, false, false), Verdict::Swallow, "bug: stray key-up");
    assert_eq!(m.on_key(RIGHT, false, false), Verdict::Pass, "only one release is owed");
    // After the release it fires again.
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Fire(DeskAction::Next));
}

#[test]
fn other_keys_and_wrong_modifiers_pass_through() {
    let mut m = matcher();
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Pass, "no modifiers held");
    m.on_key(RIGHT, false, false);
    m.on_key(CTRL, true, false);
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Pass, "only Ctrl held");
    assert_eq!(m.on_key(0x04, true, false), Verdict::Pass, "plain letter");
}

#[test]
fn injected_keys_are_never_hotkeys_and_never_eaten() {
    let mut m = matcher();
    m.on_key(CTRL, true, false);
    m.on_key(ALT, true, false);
    assert_eq!(m.on_key(RIGHT, true, true), Verdict::Pass);
    assert_eq!(m.on_key(RIGHT, false, true), Verdict::Pass);
}

// Known DevHop bug: modifiers were read with GetAsyncKeyState inside the hook, which can disagree with the
// event being decided. The matcher follows the hook's own events, including the right-hand modifier keys.
#[test]
fn bug_modifiers_come_from_the_hooks_own_events() {
    let mut m = matcher();
    m.on_key(0xE4, true, false); // right Ctrl
    m.on_key(0xE6, true, false); // right Alt
    assert_eq!(m.on_key(LEFT, true, false), Verdict::Fire(DeskAction::Prev));
    m.on_key(LEFT, false, false);
    m.on_key(0xE6, false, false);
    assert_eq!(m.on_key(LEFT, true, false), Verdict::Pass, "Alt was released");
}

#[test]
fn scroll_lock_locks_the_pointer_ctrl_alt_p_pauses_and_screen_numbers_use_the_digit_row() {
    let mut m = matcher();
    assert_eq!(m.on_key(0x47, true, false), Verdict::Fire(DeskAction::LockPointer), "Scroll Lock alone locks");
    m.on_key(0x47, false, false);
    m.on_key(CTRL, true, false);
    m.on_key(ALT, true, false);
    assert_eq!(m.on_key(0x13, true, false), Verdict::Fire(DeskAction::Pause));
    m.on_key(0x13, false, false);
    assert_eq!(m.on_key(0x20, true, false), Verdict::Fire(DeskAction::Screen(3)));
}

#[test]
fn reset_forgets_held_modifiers_and_owed_releases() {
    let mut m = matcher();
    m.on_key(CTRL, true, false);
    m.on_key(ALT, true, false);
    m.on_key(RIGHT, true, false);
    m.reset();
    assert_eq!(m.on_key(RIGHT, false, false), Verdict::Pass);
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Pass, "modifiers forgotten");
}

#[test]
fn an_empty_table_matches_nothing() {
    let mut m = HotkeyMatcher::new(new_table());
    m.on_key(CTRL, true, false);
    m.on_key(ALT, true, false);
    assert_eq!(m.on_key(RIGHT, true, false), Verdict::Pass);
    let _unused: Vec<Binding> = Vec::new();
}

// ---- hooks: when they exist, where they run ------------------------------------------------------------------

#[test]
fn by_default_nothing_is_installed() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_on(&host);
    let snap = desk.snapshot().unwrap();
    assert_eq!(snap.status.state, "off");
    assert!(!snap.status.hops_enabled);
    assert_eq!(host.with(|s| s.installs), 0, "Labs on is not enough: the person has to turn Screen hops on");
}

#[test]
fn turning_screen_hops_on_installs_the_hook_and_off_removes_it() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_on(&host);
    let on = desk.command(Cmd::SetHopsEnabled { enabled: true }).unwrap();
    assert_eq!((on.status.state, on.status.hooks_installed), ("on", true));
    assert_eq!(host.with(|s| s.installs), 1);
    let off = desk.command(Cmd::SetHopsEnabled { enabled: false }).unwrap();
    assert_eq!((off.status.state, off.status.hooks_installed), ("off", false));
    assert!(!host.hook_installed());
    assert!(host.with(|s| s.releases) >= 1, "removal also releases");
}

#[test]
fn the_hook_thread_is_not_the_main_the_test_or_the_engine_thread() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    desk.command(Cmd::HopNext).unwrap();
    let hook = host.hook_thread_id().expect("a hook thread");
    let engine = host.with(|s| s.move_thread).expect("the engine moved the cursor");
    assert_ne!(hook, thread::current().id(), "not the calling (test / app) thread");
    assert_ne!(hook, engine, "not the engine thread");
    assert_ne!(engine, thread::current().id());
}

#[test]
fn a_hotkey_on_the_hook_thread_hops_through_the_engine() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    press_chord(&host, &[CTRL, ALT], RIGHT);
    wait_until("hop by hotkey", || host.cursor() == (3200, 520));
    // The Find chord does not move the pointer but publishes a find.
    press_chord(&host, &[CTRL, ALT], 0x09);
    wait_until("find by hotkey", || desk.snapshot().unwrap().find.is_some());
    assert_eq!(host.cursor(), (3200, 520));
}

// Known DevHop bug: nothing noticed when Windows silently dropped the low-level hook (a slow callback), so
// the hotkeys just stopped working until the app was restarted.
#[test]
fn bug_the_watchdog_installs_a_dropped_hook_again() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    assert_eq!(host.hook_health(), HookHealth::Healthy);
    host.simulate_dropped_hook();
    assert_eq!(host.hook_health(), HookHealth::Dropped);
    wait_until("hook reinstalled", || host.hook_installed());
    assert_eq!(host.with(|s| s.installs), 2);
    wait_until("count shown", || desk.snapshot().unwrap().status.hook_reinstalls == 1);
    assert_eq!(desk.snapshot().unwrap().status.state, "on");
    // Known DevHop bug: the first receiver was kept forever. The new hook must deliver to the live engine.
    press_chord(&host, &[CTRL, ALT], RIGHT);
    wait_until("hotkey after reinstall", || host.cursor() == (3200, 520));
}

#[test]
fn a_healthy_hook_is_left_alone() {
    let host = MockHost::new(three(), (960, 540));
    let _desk = desk_hops(&host);
    thread::sleep(TICK * 8);
    assert_eq!(host.with(|s| s.installs), 1);
    assert_eq!(host.with(|s| s.removes), 0);
}

#[test]
fn a_failed_install_is_shown_and_retried() {
    let host = MockHost::new(three(), (960, 540));
    host.with(|s| s.fail_install = true);
    let desk = desk_on(&host);
    let snap = desk.command(Cmd::SetHopsEnabled { enabled: true }).unwrap();
    assert_eq!(snap.status.state, "error");
    assert!(snap.status.error.unwrap().contains("hook refused"));
    host.with(|s| s.fail_install = false);
    wait_until("retry installs", || host.hook_installed());
    wait_until("error cleared", || desk.snapshot().unwrap().status.error.is_none());
}

#[test]
fn the_dropped_hook_rule_needs_a_held_normal_key_and_a_quiet_hook() {
    assert!(!hook_looks_dropped(60_000, false), "idle keyboard says nothing");
    assert!(!hook_looks_dropped(100, true), "a recent callback means it is alive");
    assert!(hook_looks_dropped(5_000, true));
}

// ---- pause, quit, Labs ---------------------------------------------------------------------------------------

#[test]
fn pause_removes_the_hook_and_releases_and_resume_installs_it_again() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    let releases = host.with(|s| s.releases);
    let snap = desk.command(Cmd::Pause { paused: true }).unwrap();
    assert_eq!((snap.status.state, snap.status.paused, snap.status.hooks_installed), ("paused", true, false));
    assert!(!host.hook_installed());
    assert!(host.with(|s| s.releases) > releases, "pause releases everything");
    // Keys no longer reach any hook.
    assert_eq!(host.key(CTRL, true, false), Verdict::Pass);
    let snap = desk.command(Cmd::Pause { paused: false }).unwrap();
    assert_eq!(snap.status.state, "on");
    assert_eq!(host.with(|s| s.installs), 2);
}

#[test]
fn pause_releases_even_when_no_hook_was_up() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_on(&host);
    desk.command(Cmd::Pause { paused: true }).unwrap();
    assert!(host.with(|s| s.releases) >= 1);
    assert_eq!(host.with(|s| s.installs), 0);
}

#[test]
fn the_pause_hotkey_pauses() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    press_chord(&host, &[CTRL, ALT], 0x13);
    wait_until("paused by hotkey", || desk.snapshot().unwrap().status.paused);
    wait_until("hook gone", || !host.hook_installed());
}

#[test]
fn shutdown_removes_the_hook_releases_and_ends_the_engine() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    let releases = host.with(|s| s.releases);
    desk.shutdown();
    assert!(!host.hook_installed());
    assert!(host.with(|s| s.releases) > releases);
    assert!(!desk.engine_running());
    desk.shutdown(); // twice is fine
}

#[test]
fn labs_off_means_feature_off_and_no_engine() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_for(&host, mem_store(None));
    assert_eq!(desk.snapshot().unwrap_err(), FEATURE_OFF);
    assert!(desk.command(Cmd::HopNext).unwrap_err().starts_with("FEATURE_OFF"));
    assert!(desk.command(Cmd::SetHopsEnabled { enabled: true }).unwrap_err().starts_with("FEATURE_OFF"));
    desk.apply_settings(&labs(false), true);
    assert!(!desk.engine_running());
    assert_eq!(host.with(|s| s.installs), 0);
    assert!(host.with(|s| s.moves.is_empty()));
}

#[test]
fn labs_switch_must_be_exactly_true() {
    assert!(labs_on(&json!({"desk_enabled": true})));
    assert!(!labs_on(&json!({"desk_enabled": "yes"})));
    assert!(!labs_on(&json!({"desk_enabled": 1})));
    assert!(!labs_on(&json!({})));
}

#[test]
fn a_saved_screen_hops_choice_does_not_install_a_hook_while_labs_is_off() {
    let store = mem_store(Some(r#"{"hops_enabled": true}"#));
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_for(&host, store);
    desk.apply_settings(&labs(false), true);
    assert_eq!(host.with(|s| s.installs), 0);
    // Labs on at start: the saved choice takes effect.
    desk.apply_settings(&labs(true), true);
    wait_until("saved choice installs", || host.hook_installed());
    assert_eq!(host.with(|s| s.installs), 1);
}

#[test]
fn turning_labs_off_while_running_removes_the_hook_and_releases() {
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    let releases = host.with(|s| s.releases);
    desk.apply_settings(&labs(false), true);
    assert!(!host.hook_installed());
    assert!(host.with(|s| s.releases) > releases);
    assert!(!desk.engine_running());
    assert_eq!(desk.snapshot().unwrap_err(), FEATURE_OFF);
}

#[test]
fn labs_off_in_settings_stops_the_hotkeys() {
    // The Labs flag is shared with the engine; if it flips before the engine thread reacts, a late key does nothing.
    let host = MockHost::new(three(), (960, 540));
    let desk = desk_hops(&host);
    desk.apply_settings(&labs(false), false);
    wait_until("watchdog removes hook", || !host.hook_installed());
    press_chord(&host, &[CTRL, ALT], RIGHT);
    thread::sleep(TICK * 3);
    assert!(host.with(|s| s.moves.is_empty()));
}

// ---- saved settings ------------------------------------------------------------------------------------------

#[test]
fn settings_are_saved_and_come_back() {
    let store = Arc::new(MemStore::new(None));
    let host = MockHost::new(three(), (10, 10));
    let desk = Desk::new(Arc::new(host.clone()), store.clone(), TICK);
    desk.apply_settings(&labs(true), true);
    let mut cfg = HotkeyConfig::default();
    cfg.next = "ctrl+alt+n".into();
    desk.command(Cmd::SetHotkeys { hotkeys: cfg }).unwrap();
    desk.command(Cmd::SetHopMode { mode: HopMode::Memory }).unwrap();
    desk.command(Cmd::SetHopsEnabled { enabled: true }).unwrap();
    desk.command(Cmd::Pause { paused: true }).unwrap();
    let loaded = store.load();
    assert_eq!(loaded.config.hotkeys.next, "ctrl+alt+n");
    assert_eq!(loaded.config.hop_mode, HopMode::Memory);
    assert!(loaded.config.hops_enabled);
    assert!(!loaded.config.paused, "pause is never saved: a restart does not start paused");
}

// Known DevHop bug: a settings file with bad hotkeys was loaded as it was and the bad keys silently dropped.
#[test]
fn bug_saved_invalid_hotkeys_fall_back_to_defaults_and_say_so() {
    let loaded = config::parse(r#"{"hotkeys":{"next":"ctrl+alt+right","prev":"ctrl+alt+right","screen_n":"ctrl+alt+{n}","find_pointer":"ctrl+alt+f","pause":"scrolllock"}}"#);
    assert_eq!(loaded.config.hotkeys, HotkeyConfig::default());
    assert!(loaded.note.is_some());
    let loaded = config::parse("{not json");
    assert_eq!(loaded.config, config::Config::default());
    assert!(loaded.note.is_some());
    assert!(!loaded.config.hops_enabled, "default is off");
}

#[test]
fn a_load_note_reaches_the_page() {
    let host = MockHost::new(three(), (10, 10));
    let desk = desk_for(&host, mem_store(Some("{broken")));
    desk.apply_settings(&labs(true), true);
    let snap: Snapshot = desk.snapshot().unwrap();
    assert!(snap.status.error.unwrap().contains("could not be read"));
}

#[test]
fn display_list_failure_is_shown() {
    let host = MockHost::new(three(), (10, 10));
    host.with(|s| s.fail_displays = true);
    let desk = desk_on(&host);
    let snap = desk.snapshot().unwrap();
    assert!(snap.status.error.unwrap().contains("Could not list screens"));
}

// The settings added after the first version: center, lock, jump to a computer, return home.
mod new_hotkey_tests;
