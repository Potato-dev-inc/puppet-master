//! The "Stop everything" hold: it survives a restart (read back from the file), a routine that comes due while
//! held is written as "Skipped while paused" (never silently lost), a panic pauses the running bots, and
//! resume clears it.

use std::time::Duration;

use chrono::{Local, NaiveDateTime, TimeZone};

use super::hold::{self, Hold};
use super::sandbox_host;
use super::routines::{create, run_due, Schedule, RoutineInput};
use super::service_tests::Rig;

fn ms(text: &str) -> i64 {
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap();
    Local.from_local_datetime(&naive).earliest().unwrap().timestamp_millis()
}

fn morning() -> RoutineInput {
    RoutineInput { title: "Morning sweep".into(), prompt: "Sweep the inbox.".into(), schedule: Schedule::Daily { time: "09:00".into() } }
}

fn held() -> Hold {
    Hold { held: true, since_ms: Some(5), bots_paused: 2, routines_held: 4, computer_asleep: true }
}

#[test]
fn the_computer_counts_as_asleep_only_when_it_was_up() {
    let mut stops = 0;
    assert!(!sandbox_host::sleep_if_up(Some("stopped"), || { stops += 1; true }), "stopped: not asleep");
    assert!(!sandbox_host::sleep_if_up(Some("not_installed"), || { stops += 1; true }), "not installed: not asleep");
    assert!(!sandbox_host::sleep_if_up(None, || { stops += 1; true }), "unknown: not asleep");
    assert_eq!(stops, 0, "nothing was stopped");
    assert!(sandbox_host::sleep_if_up(Some("running"), || { stops += 1; true }), "running: asleep");
    assert!(sandbox_host::sleep_if_up(Some("starting"), || { stops += 1; true }), "starting: asleep");
    assert_eq!(stops, 2);
}

#[test]
fn a_hold_survives_a_restart_and_resume_clears_it() {
    let rig = Rig::new("hold-restart");
    hold::save(&rig.root, &held()).unwrap();
    // A restart is a fresh read from the file: nothing is kept in memory.
    assert_eq!(hold::load(&rig.root), held());
    assert!(hold::is_held(&rig.root));
    hold::resume(&rig.root).unwrap();
    assert!(!hold::is_held(&rig.root));
    assert!(!hold::load(&rig.root).held);
}

#[test]
fn a_damaged_hold_file_keeps_everything_paused_until_start_again() {
    let rig = Rig::new("hold-damaged");
    std::fs::write(hold::path(&rig.root), "{not json").unwrap();
    assert!(hold::is_held(&rig.root));
    hold::resume(&rig.root).unwrap();
    assert!(!hold::is_held(&rig.root));
}

#[test]
fn a_routine_that_comes_due_while_held_is_recorded_as_skipped_and_runs_after_resume() {
    let rig = Rig::new("hold-routine");
    create(&rig.root, "nimbus", &morning(), ms("2026-11-02 08:00:00")).unwrap();
    hold::save(&rig.root, &Hold { held: true, ..Hold::default() }).unwrap();

    assert_eq!(run_due(&rig.svc, ms("2026-11-02 09:30:00")), 1, "it came due and was handled");
    let (items, _) = rig.inbox(0);
    assert!(items.iter().all(|i| i["type"] != "send"), "nothing was sent to the brain");
    assert!(rig.notes().iter().any(|n| n == "Skipped while paused: Morning sweep"), "{:?}", rig.notes());

    hold::resume(&rig.root).unwrap();
    assert_eq!(run_due(&rig.svc, ms("2026-11-03 09:00:30")), 1, "the next run is not lost");
}

#[test]
fn a_panic_pauses_the_running_bot_and_a_message_sent_while_held_is_refused() {
    let rig = Rig::ready("hold-panic");
    assert_eq!(rig.svc.running_bots(), 1);

    let held_now = hold::pause(&rig.root, Some(&rig.svc), true, Duration::ZERO, 1_000);
    assert!(held_now.held && held_now.computer_asleep);
    assert_eq!(held_now.bots_paused, 1);
    assert_eq!(rig.svc.running_bots(), 0, "the brain was stopped");

    let refused = rig.svc.send("nimbus", "hello again").unwrap_err();
    assert_eq!(refused.code, "POTATO_PAUSED");
    hold::resume(&rig.root).unwrap();
    assert!(rig.svc.send("nimbus", "hello again").is_ok(), "a message works again after Start bots again");
}
