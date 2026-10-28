//! `today.md` over the service, on a fake host and a clock moved by hand: off by default, the line format,
//! the own pane left out, redaction, the brief rule and the one-time note, and the midnight rollover.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::brief;
use super::host::FinishedTurn;
use super::manage::Patch;
use super::test_host::{service_with_one, start_ms, FakeHost};
use super::Service;

const HOUR: i64 = 60 * 60_000;

fn home(root: &Path) -> PathBuf {
    root.join("mochi")
}

fn today_text(root: &Path) -> Option<String> {
    std::fs::read_to_string(home(root).join("today.md")).ok()
}

fn turn(pane: &str, n: u32, ended_at_ms: i64, sentence: &str) -> FinishedTurn {
    FinishedTurn {
        pane_id: pane.into(),
        turn: n,
        ended_at_ms,
        end: "idle".into(),
        agent: "Codex".into(),
        folder_name: "my-app".into(),
        parent_label: None,
        sentence: sentence.into(),
    }
}

fn tick(service: &Service, host: &FakeHost, ms: i64) {
    host.advance_ms(ms);
    service.tick();
}

fn turn_on(service: &Service) {
    service.update("mochi", Patch { share_today: Some(true), ..Patch::default() }).unwrap();
}

fn setup() -> (Service, Arc<FakeHost>, PathBuf) {
    service_with_one()
}

/// A key-shaped value made at run time, so the secret scanners of this repository see no literal.
fn fake_key() -> String {
    format!("sk-ant-api03-{}", "Zq9Xv3LmN7pRt2YhB5cWd8KfJ4sG6aUe1oIxTn0V")
}

#[test]
fn it_is_off_by_default_and_writes_nothing() {
    let (service, host, root) = setup();
    assert!(!service.view("mochi", false).unwrap().share_today);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Changed 2 files."));
    for _ in 0..4 {
        tick(&service, &host, 3_000);
    }
    assert!(today_text(&root).is_none(), "no today.md while the toggle is off");
    assert!(!home(&root).join("days").exists());
}

/// The archive folders of the companion (`companions/mochi/archive/<stamp>/`), one per time it was turned off.
fn archive_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join("companions").join("mochi").join("archive")) else { return Vec::new() };
    entries.flatten().map(|e| e.path()).collect()
}

#[test]
fn the_toggle_off_moves_the_list_out_of_the_folder_and_writes_nothing_more() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Changed 2 files."));
    tick(&service, &host, 3_000);
    let before = today_text(&root).unwrap();
    assert!(before.contains("Changed 2 files."));

    service.update("mochi", Patch { share_today: Some(false), ..Patch::default() }).unwrap();
    assert!(today_text(&root).is_none(), "the agent's folder no longer holds today.md");
    let archived = archive_dirs(&root);
    assert_eq!(archived.len(), 1, "one archive folder for this turn-off");
    assert_eq!(std::fs::read_to_string(archived[0].join("today.md")).unwrap(), before, "kept for the person, not deleted");

    host.turns.lock().push(turn("pane-2", 1, start_ms() + 1_000, "Ran the tests."));
    for _ in 0..3 {
        tick(&service, &host, 3_000);
    }
    assert!(today_text(&root).is_none(), "nothing written while off");
    assert!(!home(&root).join("days").exists());
}

#[test]
fn the_toggle_off_moves_old_days_into_the_archive_too() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Evening work."));
    tick(&service, &host, 3_000);
    host.set_now(start_ms() + 16 * HOUR + 5 * 60_000);
    tick(&service, &host, 3_000);
    assert!(home(&root).join("days").join("2026-10-28.md").exists());

    service.update("mochi", Patch { share_today: Some(false), ..Patch::default() }).unwrap();
    assert!(!home(&root).join("days").exists(), "no day file left in the agent's folder");
    let moved = archive_dirs(&root).remove(0).join("days").join("2026-10-28.md");
    assert!(std::fs::read_to_string(moved).unwrap().contains("Evening work."));
}

#[test]
fn a_running_agent_is_told_once_that_the_list_is_gone() {
    let (service, host, _root) = setup();
    turn_on(&service);
    tick(&service, &host, 1_000);
    host.set_pane("companion-mochi", super::keeper::Pane::Alive(super::keeper::Status::Idle));
    tick(&service, &host, 1_000);
    service.update("mochi", Patch { share_today: Some(false), ..Patch::default() }).unwrap();
    tick(&service, &host, 1_000);
    tick(&service, &host, 1_000);
    let texts = host.sent_texts();
    assert_eq!(texts.iter().filter(|t| *t == brief::TODAY_GONE_NOTICE).count(), 1, "{texts:?}");
}

#[test]
fn turning_it_off_with_no_list_yet_moves_nothing_and_tells_nobody() {
    let (service, host, root) = setup();
    service.update("mochi", Patch { share_today: Some(true), ..Patch::default() }).unwrap();
    service.update("mochi", Patch { share_today: Some(false), ..Patch::default() }).unwrap();
    assert!(archive_dirs(&root).is_empty());
    host.set_pane("companion-mochi", super::keeper::Pane::Alive(super::keeper::Status::Idle));
    tick(&service, &host, 1_000);
    assert!(!host.sent_texts().iter().any(|t| t == brief::TODAY_GONE_NOTICE));
}

#[test]
fn a_finished_turn_becomes_one_line_with_time_agent_folder_and_sentence() {
    let (service, host, root) = setup();
    turn_on(&service);
    // 07:05 UTC (the fake zone is UTC): an hour before the clock.
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR - 55 * 60_000, "Changed 3 files and the checks passed."));
    tick(&service, &host, 3_000);
    let text = today_text(&root).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "# What my agents did today · 2026-10-28");
    assert_eq!(lines[1], "06:05 · Codex · my-app · Changed 3 files and the checks passed.");
    assert_eq!(lines.len(), 2);
}

#[test]
fn a_turn_is_written_once_however_often_it_is_listed() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Did a thing."));
    for _ in 0..5 {
        tick(&service, &host, 3_000);
    }
    host.turns.lock().push(turn("pane-1", 2, start_ms() + 10_000, "Did another."));
    for _ in 0..3 {
        tick(&service, &host, 3_000);
    }
    let text = today_text(&root).unwrap();
    assert_eq!(text.matches("Did a thing.").count(), 1);
    assert_eq!(text.matches("Did another.").count(), 1);
    assert!(text.find("Did a thing.").unwrap() < text.find("Did another.").unwrap());
}

#[test]
fn a_restart_does_not_write_the_lines_again() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Did a thing."));
    tick(&service, &host, 3_000);
    // A new service over the same files (the app was restarted): it reads the file first.
    let again = Service::new(host.clone(), root.clone());
    for _ in 0..3 {
        tick(&again, &host, 3_000);
    }
    assert_eq!(today_text(&root).unwrap().matches("Did a thing.").count(), 1);
}

#[test]
fn the_companions_own_pane_and_unfinished_turns_are_left_out() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("companion-mochi", 1, start_ms() - HOUR, "Answered the person."));
    host.turns.lock().push(turn("companion-bit", 1, start_ms() - HOUR, "Answered someone else."));
    let mut waiting = turn("pane-1", 1, start_ms() - HOUR, "Waiting for approval.");
    waiting.end = "waiting_input".into();
    host.turns.lock().push(waiting);
    host.turns.lock().push(turn("pane-2", 1, start_ms() - HOUR, "Fixed the bug."));
    tick(&service, &host, 3_000);
    let text = today_text(&root).unwrap();
    assert!(text.contains("Fixed the bug."));
    assert!(!text.contains("Answered"));
    assert!(!text.contains("approval"));
}

#[test]
fn a_helper_line_starts_with_its_parent() {
    let (service, host, root) = setup();
    turn_on(&service);
    let mut helper = turn("pane-3", 1, start_ms() - HOUR, "Read the logs.");
    helper.parent_label = Some("Claude Code".into());
    host.turns.lock().push(helper);
    tick(&service, &host, 3_000);
    assert!(today_text(&root).unwrap().contains(" · ↳ Claude Code › Codex · my-app · Read the logs."));
}

#[test]
fn a_key_in_a_sentence_never_reaches_the_file() {
    let (service, host, root) = setup();
    turn_on(&service);
    let key = fake_key();
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, &format!("Put {key} into the env file.")));
    tick(&service, &host, 3_000);
    let text = today_text(&root).unwrap();
    assert!(text.contains("env file"));
    assert!(!text.contains(&key) && !text.contains("Zq9Xv3LmN7pRt2"));
}

#[test]
fn only_the_folder_name_is_written_never_a_path() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Done."));
    tick(&service, &host, 3_000);
    let text = today_text(&root).unwrap();
    assert!(text.contains(" · my-app · "));
    assert!(!text.contains('\\') && !text.contains("C:/"));
}

#[test]
fn turning_it_on_later_picks_up_the_turns_since_midnight_but_not_yesterday() {
    let (service, host, root) = setup();
    host.turns.lock().push(turn("pane-1", 1, start_ms() - 9 * HOUR, "Before midnight."));
    host.turns.lock().push(turn("pane-1", 2, start_ms() - 2 * HOUR, "Early today."));
    tick(&service, &host, 3_000);
    assert!(today_text(&root).is_none());
    turn_on(&service);
    tick(&service, &host, 3_000);
    let text = today_text(&root).unwrap();
    assert!(text.contains("Early today."));
    assert!(!text.contains("Before midnight."));
}

#[test]
fn the_brief_mentions_today_md_only_while_the_toggle_is_on() {
    let (service, host, root) = setup();
    let brief_file = home(&root).join("AGENTS.md");
    assert!(!std::fs::read_to_string(&brief_file).unwrap().contains("today.md"));

    turn_on(&service);
    assert!(std::fs::read_to_string(&brief_file).unwrap().contains("today.md"), "the next start reads the rule");
    // The running agent is told once, through the outbox.
    tick(&service, &host, 1_000);
    host.set_pane("companion-mochi", super::keeper::Pane::Alive(super::keeper::Status::Idle));
    tick(&service, &host, 1_000);
    assert!(host.sent_texts().iter().any(|t| t == brief::TODAY_NOTICE), "{:?}", host.sent_texts());

    service.update("mochi", Patch { share_today: Some(false), ..Patch::default() }).unwrap();
    assert!(!std::fs::read_to_string(&brief_file).unwrap().contains("today.md"));
}

#[test]
fn at_local_midnight_yesterday_moves_to_days_and_today_starts_again() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Evening work."));
    tick(&service, &host, 3_000);
    assert!(today_text(&root).unwrap().contains("Evening work."));

    // 2026-10-29 00:05 UTC.
    host.set_now(start_ms() + 16 * HOUR + 5 * 60_000);
    host.turns.lock().push(turn("pane-1", 2, start_ms() + 16 * HOUR + 2 * 60_000, "After midnight."));
    tick(&service, &host, 3_000);

    let yesterday = std::fs::read_to_string(home(&root).join("days").join("2026-10-28.md")).unwrap();
    assert!(yesterday.contains("Evening work.") && !yesterday.contains("After midnight."));
    let today = today_text(&root).unwrap();
    assert!(today.starts_with("# What my agents did today · 2026-10-29"));
    assert!(today.contains("After midnight.") && !today.contains("Evening work."));
}

#[test]
fn midnight_follows_the_local_zone_not_utc() {
    let (service, host, root) = setup();
    // UTC+8: 08:00 UTC is 16:00 local on the 28th.
    host.offset.store(8 * 3600, std::sync::atomic::Ordering::SeqCst);
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Afternoon work."));
    tick(&service, &host, 3_000);
    assert!(today_text(&root).unwrap().starts_with("# What my agents did today · 2026-10-28"));
    assert!(today_text(&root).unwrap().contains("15:00 · Codex"));

    // 15:59 UTC is 23:59 local: still the 28th. 16:01 UTC is 00:01 local on the 29th.
    host.set_now(start_ms() + 7 * HOUR + 59 * 60_000);
    tick(&service, &host, 3_000);
    assert!(today_text(&root).unwrap().starts_with("# What my agents did today · 2026-10-28"));
    host.set_now(start_ms() + 8 * HOUR + 60_000);
    tick(&service, &host, 3_000);
    assert!(today_text(&root).unwrap().starts_with("# What my agents did today · 2026-10-29"));
    assert!(home(&root).join("days").join("2026-10-28.md").exists());
}

#[test]
fn an_empty_new_day_still_clears_yesterdays_lines_from_today_md() {
    let (service, host, root) = setup();
    turn_on(&service);
    host.turns.lock().push(turn("pane-1", 1, start_ms() - HOUR, "Evening work."));
    tick(&service, &host, 3_000);
    host.set_now(start_ms() + 17 * HOUR);
    tick(&service, &host, 3_000);
    assert_eq!(today_text(&root).unwrap(), "# What my agents did today · 2026-10-29\n");
}

#[test]
fn the_phone_views_say_whether_it_is_on_and_nothing_else_about_it() {
    let (service, _host, _root) = setup();
    turn_on(&service);
    let phone = serde_json::to_string(&service.view("mochi", true).unwrap()).unwrap();
    assert!(phone.contains("\"shareToday\":true"));
    assert!(!phone.contains("today.md"));
}
