//! Routines: the next-run math, the coalesced catch-up, one-time runs, pause, validation, the file, and the tick
//! into a bot's brain. The clock is passed in, so these need no real time (the local zone is only used to turn
//! the instant the store keeps into wall-clock time and back, on dates away from the DST change).

use std::path::PathBuf;

use chrono::{Local, NaiveDateTime, TimeZone};
use serde_json::Value;

use super::routines::*;
use super::service_tests::Rig;
use super::store;

fn naive(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn ms(text: &str) -> i64 {
    Local.from_local_datetime(&naive(text)).earliest().unwrap().timestamp_millis()
}

fn iso(text: &str) -> String {
    naive(text).format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn input(schedule: Schedule) -> RoutineInput {
    RoutineInput { title: "Morning sweep".into(), prompt: "Sweep the inbox.".into(), schedule }
}

fn routine(schedule: Schedule, next: &str) -> Routine {
    Routine {
        id: "r-test".into(),
        bot: "nimbus".into(),
        title: "Morning sweep".into(),
        prompt: "Sweep the inbox.".into(),
        schedule,
        paused: false,
        done: false,
        last_run_ms: None,
        next_run: Some(iso(next)),
        created_ms: 0,
    }
}

fn daily(time: &str) -> Schedule {
    Schedule::Daily { time: time.into() }
}

fn weekdays(time: &str) -> Schedule {
    Schedule::Weekdays { time: time.into() }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-routines-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    store::add_bot(&dir, "nimbus", "fish", 1).unwrap();
    dir
}

// ---- next-run math ------------------------------------------------------------------------------

#[test]
fn hourly_runs_at_the_next_full_hour_strictly_after_now() {
    let s = Schedule::Hourly;
    assert_eq!(next_after(&s, naive("2026-11-02 10:30:00")), Some(naive("2026-11-02 11:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 10:00:00")), Some(naive("2026-11-02 11:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 23:59:59")), Some(naive("2026-11-03 00:00:00")));
}

#[test]
fn daily_runs_today_if_the_time_is_still_ahead_and_tomorrow_if_not() {
    let s = daily("09:00");
    assert_eq!(next_after(&s, naive("2026-11-02 08:59:00")), Some(naive("2026-11-02 09:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 09:00:00")), Some(naive("2026-11-03 09:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 18:00:00")), Some(naive("2026-11-03 09:00:00")));
}

#[test]
fn weekdays_skip_the_weekend_and_wait_for_monday() {
    let s = weekdays("06:00");
    // 2026-11-02 is a Monday.
    assert_eq!(next_after(&s, naive("2026-11-02 05:00:00")), Some(naive("2026-11-02 06:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 06:00:00")), Some(naive("2026-11-03 06:00:00")));
    // Friday after the run goes to Monday, not Saturday.
    assert_eq!(next_after(&s, naive("2026-11-06 07:00:00")), Some(naive("2026-11-09 06:00:00")));
    // Saturday and Sunday both go to Monday.
    assert_eq!(next_after(&s, naive("2026-11-07 12:00:00")), Some(naive("2026-11-09 06:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-08 05:00:00")), Some(naive("2026-11-09 06:00:00")));
}

#[test]
fn every_n_minutes_lands_on_the_grid_counted_from_midnight() {
    let s = Schedule::EveryMinutes { minutes: 15 };
    assert_eq!(next_after(&s, naive("2026-11-02 10:00:00")), Some(naive("2026-11-02 10:15:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 10:14:59")), Some(naive("2026-11-02 10:15:00")));
    // The last slot of the day rolls over to midnight of the next day.
    assert_eq!(next_after(&s, naive("2026-11-02 23:50:00")), Some(naive("2026-11-03 00:00:00")));
    let thirty = Schedule::EveryMinutes { minutes: 30 };
    assert_eq!(next_after(&thirty, naive("2026-11-02 10:00:30")), Some(naive("2026-11-02 10:30:00")));
}

#[test]
fn an_interval_under_fifteen_minutes_has_no_next_run() {
    assert_eq!(next_after(&Schedule::EveryMinutes { minutes: 14 }, naive("2026-11-02 10:00:00")), None);
    assert_eq!(next_after(&Schedule::EveryMinutes { minutes: 1441 }, naive("2026-11-02 10:00:00")), None);
}

#[test]
fn a_one_time_run_has_a_next_run_only_while_it_is_in_the_future() {
    let s = Schedule::Once { at: "2026-11-02T09:00".into() };
    assert_eq!(next_after(&s, naive("2026-11-02 08:00:00")), Some(naive("2026-11-02 09:00:00")));
    assert_eq!(next_after(&s, naive("2026-11-02 09:00:00")), None);
}

#[test]
fn the_summary_reads_in_plain_words() {
    assert_eq!(summary(&weekdays("06:00")), "Weekdays 6am");
    assert_eq!(summary(&daily("18:30")), "Every day 6:30pm");
    assert_eq!(summary(&daily("00:00")), "Every day 12am");
    assert_eq!(summary(&Schedule::Hourly), "Every hour");
    assert_eq!(summary(&Schedule::EveryMinutes { minutes: 15 }), "Every 15 minutes");
    assert_eq!(summary(&Schedule::Once { at: "2026-11-02T09:00".into() }), "Once on Nov 2 at 9am");
}

// ---- validation ---------------------------------------------------------------------------------

#[test]
fn a_form_is_checked_before_it_is_saved() {
    let now = naive("2026-11-02 08:00:00");
    assert!(validate(&input(daily("09:00")), now).is_ok());
    let blank_title = RoutineInput { title: "  ".into(), ..input(daily("09:00")) };
    assert_eq!(validate(&blank_title, now).unwrap_err().message, "Give the routine a name.");
    let blank_prompt = RoutineInput { prompt: "".into(), ..input(daily("09:00")) };
    assert_eq!(validate(&blank_prompt, now).unwrap_err().message, "Write what the bot should do.");
    let long_title = RoutineInput { title: "x".repeat(81), ..input(daily("09:00")) };
    assert!(validate(&long_title, now).is_err());
    let long_prompt = RoutineInput { prompt: "x".repeat(2001), ..input(daily("09:00")) };
    assert!(validate(&long_prompt, now).is_err());
    assert!(validate(&input(daily("25:00")), now).is_err());
    assert!(validate(&input(weekdays("nine")), now).is_err());
    assert!(validate(&input(Schedule::EveryMinutes { minutes: 14 }), now).is_err());
    assert!(validate(&input(Schedule::EveryMinutes { minutes: 15 }), now).is_ok());
}

#[test]
fn a_one_time_run_must_be_in_the_future() {
    let now = naive("2026-11-02 08:00:00");
    assert!(validate(&input(Schedule::Once { at: "2026-11-02T09:00".into() }), now).is_ok());
    let past = validate(&input(Schedule::Once { at: "2026-11-02T07:00".into() }), now).unwrap_err();
    assert_eq!(past.message, "Pick a time in the future.");
    assert!(validate(&input(Schedule::Once { at: "tomorrow".into() }), now).is_err());
}

// ---- the scheduler step -------------------------------------------------------------------------

#[test]
fn a_routine_on_time_runs_without_a_late_note() {
    let mut rs = vec![routine(daily("09:00"), "2026-11-02 09:00:00")];
    let dues = take_due(&mut rs, naive("2026-11-02 09:00:20"), ms("2026-11-02 09:00:20"));
    assert_eq!(dues.len(), 1);
    assert_eq!(dues[0].context, None);
    assert_eq!(dues[0].prompt, "Sweep the inbox.");
    assert_eq!(rs[0].next_run.as_deref(), Some("2026-11-03T09:00:00"));
    assert_eq!(rs[0].last_run_ms, Some(ms("2026-11-02 09:00:20")));
    assert!(take_due(&mut rs, naive("2026-11-02 09:00:30"), 0).is_empty(), "one run, not two");
}

#[test]
fn ten_missed_hours_make_one_catch_up_run_that_says_it_is_late() {
    let mut rs = vec![routine(Schedule::Hourly, "2026-11-02 10:00:00")];
    let now = naive("2026-11-02 15:20:00");
    let dues = take_due(&mut rs, now, ms("2026-11-02 15:20:00"));
    assert_eq!(dues.len(), 1, "the missed hours are coalesced into one run");
    let note = dues[0].context.clone().expect("a late note");
    assert!(note.starts_with("This run is late."), "{note}");
    assert!(note.contains("Nov 2, 10:00 AM") && note.contains("Nov 2, 3:20 PM"), "{note}");
    assert_eq!(rs[0].next_run.as_deref(), Some("2026-11-02T16:00:00"), "the next run is the first one after now");
    assert!(take_due(&mut rs, now, 0).is_empty());
}

#[test]
fn a_run_fourteen_minutes_late_is_not_called_late_and_fifteen_is() {
    let mut on_edge = vec![routine(daily("09:00"), "2026-11-02 09:00:00")];
    assert_eq!(take_due(&mut on_edge, naive("2026-11-02 09:14:59"), 0)[0].context, None);
    let mut late = vec![routine(daily("09:00"), "2026-11-02 09:00:00")];
    assert!(take_due(&mut late, naive("2026-11-02 09:15:00"), 0)[0].context.is_some());
}

#[test]
fn a_one_time_routine_runs_once_then_is_done() {
    let at = "2026-11-02T09:00";
    let mut rs = vec![routine(Schedule::Once { at: at.into() }, "2026-11-02 09:00:00")];
    let dues = take_due(&mut rs, naive("2026-11-02 09:00:05"), 0);
    assert_eq!(dues.len(), 1);
    assert_eq!(dues[0].context, None);
    assert!(rs[0].done);
    assert_eq!(rs[0].next_run, None);
    assert!(take_due(&mut rs, naive("2026-11-03 09:00:00"), 0).is_empty(), "never again");
    assert_eq!(view(&rs[0]).status, "done");
}

#[test]
fn a_one_time_routine_found_after_its_time_runs_once_and_says_it_is_late() {
    let mut rs = vec![routine(Schedule::Once { at: "2026-11-02T09:00".into() }, "2026-11-02 09:00:00")];
    let dues = take_due(&mut rs, naive("2026-11-02 12:00:00"), 0);
    assert_eq!(dues.len(), 1);
    assert!(dues[0].context.as_deref().unwrap().contains("late"));
    assert!(rs[0].done);
}

#[test]
fn a_paused_routine_never_runs_and_resuming_does_not_run_the_paused_time() {
    let root = temp("pause");
    let made = create(&root, "nimbus", &input(Schedule::Hourly), ms("2026-11-02 08:10:00")).unwrap();
    assert_eq!(made.next_run_ms, Some(ms("2026-11-02 09:00:00")));
    let paused = set_paused(&root, &made.id, true, ms("2026-11-02 08:20:00")).unwrap();
    assert_eq!(paused.status, "paused");
    assert_eq!(paused.next_run_ms, None);
    // Eight hours later the paused routine is still quiet: its runs are not owed.
    let mut stored = store::read_json::<RoutinesFile>(&routines_path(&root), "routines.json").unwrap().unwrap();
    assert!(take_due(&mut stored.routines, naive("2026-11-02 17:00:00"), 0).is_empty());
    let resumed = set_paused(&root, &made.id, false, ms("2026-11-02 17:00:00")).unwrap();
    assert_eq!(resumed.status, "active");
    assert_eq!(resumed.next_run_ms, Some(ms("2026-11-02 18:00:00")), "resume looks forward, no backlog");
}

// ---- the file -----------------------------------------------------------------------------------

#[test]
fn routines_are_created_listed_updated_and_deleted_per_bot() {
    let root = temp("crud");
    let now = ms("2026-11-02 08:00:00");
    let made = create(&root, "nimbus", &input(weekdays("06:00")), now).unwrap();
    assert!(made.id.starts_with("r-"));
    assert_eq!(made.summary, "Weekdays 6am");
    assert_eq!(made.status, "active");
    // Created on Monday at 08:00, so today's 06:00 has passed: the next run is Tuesday 06:00.
    assert_eq!(made.next_run_ms, Some(ms("2026-11-03 06:00:00")));
    assert_eq!(list(&root, Some("nimbus")).unwrap().len(), 1);
    assert_eq!(list(&root, Some("other")).unwrap().len(), 0);

    let renamed = update(&root, &made.id, &RoutineInput { title: "Inbox".into(), ..input(daily("07:30")) }, now).unwrap();
    assert_eq!(renamed.title, "Inbox");
    assert_eq!(renamed.summary, "Every day 7:30am");

    delete(&root, &made.id).unwrap();
    assert!(list(&root, None).unwrap().is_empty());
    assert_eq!(delete(&root, &made.id).unwrap_err().code, "POTATO_ROUTINE_NOT_FOUND");
}

#[test]
fn a_bot_has_at_most_twenty_routines_and_an_unknown_bot_is_refused() {
    let root = temp("cap");
    let now = ms("2026-11-02 08:00:00");
    for _ in 0..20 {
        create(&root, "nimbus", &input(Schedule::Hourly), now).unwrap();
    }
    assert!(create(&root, "nimbus", &input(Schedule::Hourly), now).is_err());
    assert_eq!(create(&root, "nobody", &input(Schedule::Hourly), now).unwrap_err().code, "POTATO_BOT_NOT_FOUND");
}

#[test]
fn a_one_time_routine_that_ran_can_be_set_to_a_new_future_time() {
    let root = temp("rearm");
    let now = ms("2026-11-02 08:00:00");
    let made = create(&root, "nimbus", &input(Schedule::Once { at: "2026-11-02T09:00".into() }), now).unwrap();
    let mut stored = store::read_json::<RoutinesFile>(&routines_path(&root), "routines.json").unwrap().unwrap();
    take_due(&mut stored.routines, naive("2026-11-02 09:00:01"), 0);
    store::write_json(&routines_path(&root), &stored, "routines.json").unwrap();
    assert_eq!(list(&root, None).unwrap()[0].status, "done");
    let again = update(&root, &made.id, &input(Schedule::Once { at: "2026-11-03T09:00".into() }), ms("2026-11-02 10:00:00")).unwrap();
    assert_eq!(again.status, "active");
    assert_eq!(again.next_run_ms, Some(ms("2026-11-03 09:00:00")));
}

#[test]
fn a_damaged_routines_file_is_an_error_and_is_never_overwritten() {
    let root = temp("damaged");
    std::fs::write(routines_path(&root), "{ damaged").unwrap();
    assert_eq!(list(&root, None).unwrap_err().code, "POTATO_STORE_INVALID");
    assert!(create(&root, "nimbus", &input(Schedule::Hourly), ms("2026-11-02 08:00:00")).is_err());
    assert_eq!(std::fs::read_to_string(routines_path(&root)).unwrap(), "{ damaged");
}

// ---- the tick into the brain ---------------------------------------------------------------------

#[test]
fn a_due_routine_reaches_the_brain_as_a_message_from_routine_with_the_late_note() {
    let rig = Rig::new("routine-tick");
    let made = create(&rig.root, "nimbus", &input(daily("09:00")), ms("2026-11-02 08:00:00")).unwrap();
    assert_eq!(run_due(&rig.svc, ms("2026-11-02 08:59:59")), 0, "not yet due");
    // The app was closed until 10:05: one run, late.
    assert_eq!(run_due(&rig.svc, ms("2026-11-02 10:05:00")), 1);
    assert_eq!(run_due(&rig.svc, ms("2026-11-02 10:05:30")), 0);

    let (items, _) = rig.inbox(0);
    let send: &Value = items.iter().find(|i| i["type"] == "send").expect("a send item");
    assert_eq!(send["from"]["id"], "routine");
    assert_eq!(send["from"]["trust"], "owner_pc", "trusted like the person at this computer");
    assert_eq!(send["text"], "Sweep the inbox.");
    assert!(send["context"].as_str().unwrap().contains("late"));
    let saved = rig.thread();
    assert!(saved.iter().any(|m| m.text == "Routine: Morning sweep\n\nSweep the inbox."), "the thread shows the routine");
    let stored = list(&rig.root, None).unwrap().into_iter().find(|r| r.id == made.id).unwrap();
    assert_eq!(stored.last_run_ms, Some(ms("2026-11-02 10:05:00")));
}

#[test]
fn run_now_sends_the_prompt_and_leaves_the_schedule_alone() {
    let rig = Rig::new("routine-now");
    let made = create(&rig.root, "nimbus", &input(daily("09:00")), ms("2026-11-02 08:00:00")).unwrap();
    run_now(&rig.svc, &made.id, ms("2026-11-02 08:30:00")).unwrap();
    let (items, _) = rig.inbox(0);
    let send = items.iter().find(|i| i["type"] == "send").expect("a send item");
    assert_eq!(send["from"]["id"], "routine");
    assert!(send.get("context").is_none(), "a manual run is not late");
    let after = list(&rig.root, None).unwrap().into_iter().find(|r| r.id == made.id).unwrap();
    assert_eq!(after.next_run_ms, made.next_run_ms, "the next run did not move");
    assert_eq!(after.last_run_ms, Some(ms("2026-11-02 08:30:00")));
}

#[test]
fn nothing_runs_while_potato_bot_is_switched_off() {
    let rig = Rig::new("routine-off");
    create(&rig.root, "nimbus", &input(daily("09:00")), ms("2026-11-02 08:00:00")).unwrap();
    rig.brains.set_enabled(false);
    assert_eq!(run_due(&rig.svc, ms("2026-11-02 09:30:00")), 0);
    rig.brains.set_enabled(true);
    assert_eq!(run_due(&rig.svc, ms("2026-11-02 09:30:00")), 1, "the missed run is still owed, once");
}
