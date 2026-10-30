//! When a Codex pane reads `idle` after its turn. The pane outbox types a Chat message only into an
//! `idle` pane, so this is the delay between "the answer is on screen" and "the message goes in".
//!
//! The frames are the rows of a live Codex 0.162.1 turn (answer drawn at 4.5 s here; after it the
//! footer's thread-title spinner repaints every ~100 ms for about a second, then the screen is
//! still), replayed on virtual time through the same status steps the PTY reader and the idle
//! watcher run.

use crate::pty::status::{self, PaneStatus};
use crate::replay::cast::{Cast, Event, Kind, Meta};
use crate::replay::pane::ReplayPane;
use crate::tui::{profile_for, TuiProfile};
use std::time::Duration;

fn secs(n: f64) -> Duration {
    Duration::from_secs_f64(n)
}

fn frame(rows: &[&str]) -> Vec<u8> {
    let mut bytes = b"\x1b[2J\x1b[H".to_vec();
    bytes.extend(rows.join("\r\n").as_bytes());
    bytes
}

fn footer(tail: &str) -> String {
    format!("  GPT-6-Luna medium \u{b7} ws{tail}")
}

fn idle_composer() -> Vec<u8> {
    frame(&["", "\u{203a} Ask Codex to do anything", "", &footer(""), "  ? for shortcuts"])
}

fn working(seconds: u32) -> Vec<u8> {
    let row = format!("\u{2022} Working ({seconds}s \u{2022} esc to interrupt)");
    frame(&["\u{203a} Reply with the single word ok.", "", &row, "", "\u{203a} Ask Codex to do anything", "", &footer(" \u{b7} \u{280f}")])
}

fn answered(footer_tail: &str) -> Vec<u8> {
    frame(&[
        "\u{203a} Reply with the single word ok.",
        "",
        "\u{2022} ok",
        "",
        "Worked for 2s \u{2022} 12:55 PM",
        "\u{203a} Ask Codex to do anything",
        "",
        &footer(footer_tail),
        "  ? for shortcuts",
    ])
}

/// Idle composer, a three-second turn, the answer at 4.5 s, the title spinner until 5.6 s, then still.
fn turn_cast() -> Cast {
    let mut events = vec![Event { at: secs(0.0), kind: Kind::Output(idle_composer()) }];
    for (at, seconds) in [(2.0, 0), (3.0, 1), (4.0, 2)] {
        events.push(Event { at: secs(at), kind: Kind::Output(working(seconds)) });
    }
    let spinner = ["\u{280f}", "\u{2826}", "\u{2807}", "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2807}", "\u{280f}"];
    for (i, glyph) in spinner.iter().enumerate() {
        events.push(Event { at: secs(4.5 + 0.1 * i as f64), kind: Kind::Output(answered(&format!(" \u{b7} {glyph}"))) });
    }
    events.push(Event { at: secs(5.7), kind: Kind::Output(answered(" \u{b7} Reply ok")) });
    Cast {
        name: "codex_turn_end".into(),
        cols: 100,
        rows: 30,
        meta: Meta { agent: "codex".into(), initial_status: "idle".into(), ..Meta::default() },
        events,
        reactions: Vec::new(),
    }
}

#[test]
fn the_fixture_frames_read_as_the_live_screens_do() {
    let busy = |bytes: Vec<u8>| status::busy_marker("codex", &String::from_utf8(bytes).unwrap().replace("\x1b[2J\x1b[H", ""));
    assert_eq!(busy(working(1)), Some(true), "the work row means a turn is running");
    assert_eq!(busy(answered(" \u{b7} \u{280f}")), Some(false), "no work row once the answer is drawn, spinner or not");
    assert_eq!(busy(idle_composer()), Some(false));
}

#[test]
fn the_quiet_window_is_shorter_than_the_default_and_not_below_a_spinner_frame() {
    let codex = profile_for("codex");
    assert_eq!(codex.quiet_idle_after(), Duration::from_millis(800));
    assert!(codex.quiet_idle_after() < codex.idle_after());
    assert!(crate::tui::conformance_rules::idle_windows_are_sane(codex.as_ref()).is_ok());
}

#[test]
fn a_finished_turn_reads_idle_one_quiet_window_after_the_last_repaint_not_five_seconds() {
    let mut pane = ReplayPane::new(turn_cast());
    pane.run_to(secs(1.9));
    assert_eq!(pane.status(), PaneStatus::Idle, "the empty composer is idle");
    pane.run_to(secs(4.4));
    assert_eq!(pane.status(), PaneStatus::Running, "the turn is in flight");
    // The title spinner keeps repainting after the answer: still not idle.
    pane.run_to(secs(5.6));
    assert_eq!(pane.status(), PaneStatus::Running, "{}", pane.timeline.render());
    // Last repaint at 5.7 s: not idle 0.7 s later, idle by 0.9 s later (the idle watcher looks every 100 ms).
    pane.run_to(secs(6.4));
    assert_eq!(pane.status(), PaneStatus::Running, "{}", pane.timeline.render());
    pane.run_to(secs(6.7));
    assert_eq!(pane.status(), PaneStatus::Idle, "{}", pane.timeline.render());
}

#[test]
fn a_turn_in_flight_never_reads_idle_between_its_once_a_second_repaints() {
    let mut pane = ReplayPane::new(turn_cast());
    pane.run_to_end(Duration::from_secs(10));
    let idle_during_turn = pane.transitions.iter().any(|(at, status)| *status == PaneStatus::Idle && *at > secs(2.0) && *at < secs(6.0));
    assert!(!idle_during_turn, "the work row keeps the long window: {}", pane.timeline.render());
}
