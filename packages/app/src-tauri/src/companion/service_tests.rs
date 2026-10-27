//! The whole life of a companion on a fake host and a clock moved by hand: start, crash and restart,
//! a pane the person closed, check-ins, the nap, needs-you states, replies and memory. No terminal.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::host::{Blocking, Reply};
use super::keeper::{Pane, Status};
use super::manage::Patch;
use super::model::{CheckIn, CompanionState, CompanionView, FeedKind, HhMm, When};
use super::store::{self, SentToday};
use super::test_host::{draft, service_with_one, start_ms, temp_dir, FakeHost};
use super::Service;
use crate::tui::InstructionFile;

const PANE: &str = "companion-mochi";
const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;

fn fresh() -> (Service, Arc<FakeHost>, PathBuf) {
    let root = temp_dir("pm-companion-service");
    let host = FakeHost::new(start_ms());
    (Service::new(host.clone(), root.clone()), host, root)
}

fn tick_after(service: &Service, host: &FakeHost, ms: i64) {
    host.advance_ms(ms);
    service.tick();
}

fn mochi(service: &Service) -> CompanionView {
    service.view("mochi", false).unwrap()
}

fn kinds(root: &Path) -> Vec<FeedKind> {
    store::feed_read(root, "mochi").unwrap().into_iter().map(|i| i.kind).collect()
}

fn texts(root: &Path, kind: FeedKind) -> Vec<String> {
    store::feed_read(root, "mochi").unwrap().into_iter().filter(|i| i.kind == kind).map(|i| i.text).collect()
}

/// Boots the pane: the agent drew its input box and reads idle.
fn boot(service: &Service, host: &FakeHost) {
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(service, host, 1_000);
}

fn checkin(id: &str, label: &str, when: When) -> CheckIn {
    CheckIn { id: id.into(), label: label.into(), text: "What is on today?".into(), when, enabled: true }
}

fn at(h: u8, m: u8) -> HhMm {
    HhMm { h, m }
}

// ---- start -------------------------------------------------------------------------------------

#[test]
fn creating_starts_one_pane_with_the_brief_and_keeps_only_that_one() {
    let (service, host, root) = service_with_one();
    let spawned = host.spawned.lock().clone();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].pane_id, PANE);
    assert_eq!(spawned[0].agent_type, "claude");
    assert_eq!(spawned[0].cwd, root.join("mochi").display().to_string());
    assert!(spawned[0].extra_args.is_empty() && spawned[0].account.is_none());

    // The brief is in AGENTS.md (this fake agent declares no instruction file), between the markers.
    let brief = std::fs::read_to_string(root.join("mochi").join("AGENTS.md")).unwrap();
    assert!(brief.contains("puppet-master:companion:start") && brief.contains("# You are Mochi"));
    assert!(brief.contains("memory/pending.md"));
    // ...and the generic agent gets the primer, through the outbox.
    let primer = host.sent.lock().clone();
    assert_eq!(primer.len(), 1);
    assert_eq!(primer[0].0, PANE);
    assert!(primer[0].1.starts_with("Please read SOUL.md"), "{}", primer[0].1);

    assert_eq!(mochi(&service).state, CompanionState::Starting);
    for _ in 0..5 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(host.spawn_count(), 1, "one pane per companion, never a second");

    boot(&service, &host);
    let view = mochi(&service);
    assert_eq!((view.state, view.state_line.as_str()), (CompanionState::Idle, "Here. Ask me anything."));
    assert_eq!(view.pane_id.as_deref(), Some(PANE));
    assert_eq!(texts(&root, FeedKind::Woke), vec!["Woke up."]);
}

#[test]
fn an_agent_with_its_own_instruction_file_gets_the_brief_there_and_no_primer() {
    let (service, host, root) = fresh();
    *host.files.lock() = vec![InstructionFile::new("CLAUDE.md")];
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    assert!(root.join("mochi").join("CLAUDE.md").exists());
    assert!(!root.join("mochi").join("AGENTS.md").exists());
    assert!(host.sent.lock().is_empty(), "no primer");
}

#[test]
fn a_companion_with_auto_start_off_waits_asleep_until_woken() {
    let (service, host, root) = fresh();
    let mut d = draft("Mochi", &root.join("mochi"));
    d.auto_start = Some(false);
    service.create(d).unwrap();
    for _ in 0..3 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(host.spawn_count(), 0);
    assert_eq!(mochi(&service).state, CompanionState::Sleeping);
    service.wake("mochi").unwrap();
    assert_eq!(host.spawn_count(), 1);
}

#[test]
fn a_start_that_fails_waits_five_seconds_and_tries_again() {
    let (service, host, root) = fresh();
    *host.spawn_error.lock() = Some("could not open the terminal".into());
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    assert_eq!(host.spawn_count(), 0);
    assert_eq!(mochi(&service).state, CompanionState::Starting);
    *host.spawn_error.lock() = None;
    tick_after(&service, &host, 4_000);
    assert_eq!(host.spawn_count(), 0, "still waiting");
    tick_after(&service, &host, 1_000);
    assert_eq!(host.spawn_count(), 1);
}

// ---- crash, restart, stop -----------------------------------------------------------------------

#[test]
fn a_crash_restarts_with_5s_30s_2min_and_the_fourth_stops_it_with_one_card() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);

    // Crash 1 -> restart after 5 s.
    host.set_pane(PANE, Pane::Dead);
    service.tick();
    assert!(host.killed.lock().contains(&PANE.to_string()), "the dead pane is closed first");
    assert_eq!(mochi(&service).state, CompanionState::Starting);
    tick_after(&service, &host, 4_000);
    assert_eq!(host.spawn_count(), 1);
    tick_after(&service, &host, 1_000);
    assert_eq!(host.spawn_count(), 2);
    assert_eq!(texts(&root, FeedKind::Restarted), vec!["Restarted after it stopped."]);

    // Crash 2 -> 30 s.
    host.set_pane(PANE, Pane::Dead);
    service.tick();
    tick_after(&service, &host, 29_000);
    assert_eq!(host.spawn_count(), 2);
    tick_after(&service, &host, 1_000);
    assert_eq!(host.spawn_count(), 3);

    // Crash 3 -> 2 min.
    host.set_pane(PANE, Pane::Dead);
    service.tick();
    tick_after(&service, &host, 119_000);
    assert_eq!(host.spawn_count(), 3);
    tick_after(&service, &host, 1_000);
    assert_eq!(host.spawn_count(), 4);
    assert!(host.raised.lock().is_empty());

    // Crash 4 inside 15 minutes -> stopped, one card, never started again by itself.
    host.set_pane(PANE, Pane::Dead);
    service.tick();
    let view = mochi(&service);
    assert_eq!(view.state, CompanionState::Stopped);
    assert_eq!(view.state_line, "Mochi keeps stopping. Open the terminal to see why.");
    assert_eq!(*host.raised.lock(), vec!["mochi".to_string()]);
    for _ in 0..10 {
        tick_after(&service, &host, 30_000);
    }
    assert_eq!(host.spawn_count(), 4);
    assert_eq!(host.raised.lock().len(), 1, "one card, not one per tick");

    // `Try again` (companion_restart) clears the card and starts it.
    let view = service.restart("mochi").unwrap();
    assert!(host.cleared.lock().contains(&"mochi".to_string()));
    assert_eq!(host.spawn_count(), 5);
    assert_ne!(view.state, CompanionState::Stopped);
}

#[test]
fn a_pane_the_person_closed_means_sleeping_and_is_never_restarted() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    host.remove_pane(PANE); // the person closed the terminal
    service.tick();
    let view = mochi(&service);
    assert_eq!((view.state, view.state_line.as_str()), (CompanionState::Sleeping, "Sleeping. Wake me when you need me."));
    assert!(store::runtime_load(&root, "mochi").unwrap().sleeping, "it stays asleep across a restart of the app");
    assert_eq!(kinds(&root).last(), Some(&FeedKind::Slept));
    for _ in 0..12 {
        tick_after(&service, &host, 60_000);
    }
    assert_eq!(host.spawn_count(), 1, "never restarted");
    assert!(host.raised.lock().is_empty(), "closing it is not a crash");
    let woke = service.wake("mochi").unwrap();
    assert_eq!(host.spawn_count(), 2);
    assert_ne!(woke.state, CompanionState::Sleeping);
    assert!(!store::runtime_load(&root, "mochi").unwrap().sleeping);
}

#[test]
fn sleep_closes_the_pane_and_the_app_starts_it_asleep_next_time() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    let view = service.sleep("mochi").unwrap();
    assert_eq!(view.state, CompanionState::Sleeping);
    assert!(host.killed.lock().contains(&PANE.to_string()));
    assert_eq!(texts(&root, FeedKind::Slept), vec!["Went to sleep."]);
    // A new service over the same folder (the app restarted) keeps it asleep.
    let host2 = FakeHost::new(start_ms() + HOUR);
    let again = Service::new(host2.clone(), root.clone());
    again.tick();
    assert_eq!(host2.spawn_count(), 0);
    assert_eq!(again.view("mochi", false).unwrap().state, CompanionState::Sleeping);
}

#[test]
fn an_agent_that_exited_in_its_host_shell_counts_as_a_crash() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Dead); // `LiveHost::pane` maps "agent process exited" to Dead
    service.tick();
    tick_after(&service, &host, 5_000);
    assert_eq!(host.spawn_count(), 2);
}

// ---- needs you ----------------------------------------------------------------------------------

#[test]
fn a_card_for_the_pane_makes_it_needs_you_and_nothing_is_typed_until_it_is_answered() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    *host.blocking.lock() = Some(Blocking { item_id: "item1".into(), title: "Trust this folder?".into() });
    host.set_pane(PANE, Pane::Alive(Status::WaitingInput));
    tick_after(&service, &host, 1_000);
    let view = mochi(&service);
    assert_eq!((view.state, view.state_line.as_str()), (CompanionState::NeedsYou, "Needs you: Trust this folder?"));
    assert_eq!(view.needs.as_ref().map(|n| n.item_id.as_str()), Some("item1"));
    assert_eq!(texts(&root, FeedKind::NeedsYou).len(), 1);
    tick_after(&service, &host, 1_000);
    assert_eq!(texts(&root, FeedKind::NeedsYou).len(), 1, "one line per time it needs you");

    // A tidy-up request waits behind the card: nothing reaches the outbox while it is open.
    let before = host.sent.lock().len();
    service.tidy_up("mochi").unwrap();
    tick_after(&service, &host, 1_000);
    assert_eq!(host.sent.lock().len(), before);

    // The person answers on the card; the pane goes back to idle; the message goes in.
    *host.blocking.lock() = None;
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(&service, &host, 1_000);
    assert_eq!(mochi(&service).state, CompanionState::Idle);
    assert!(host.sent_texts().last().unwrap().starts_with("Your notes are full ("));
}

#[test]
fn a_sign_in_screen_and_a_bare_waiting_pane_each_have_their_own_line() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    host.sign_in.store(true, std::sync::atomic::Ordering::SeqCst);
    tick_after(&service, &host, 1_000);
    let view = mochi(&service);
    assert_eq!((view.state, view.state_line.as_str()), (CompanionState::NeedsYou, "Sign in to Agent claude on the computer"));
    assert!(view.needs.is_none());
    host.sign_in.store(false, std::sync::atomic::Ordering::SeqCst);
    host.set_pane(PANE, Pane::Alive(Status::WaitingInput));
    tick_after(&service, &host, 1_000);
    assert_eq!(mochi(&service).state_line, "Waiting on the computer screen");
}

#[test]
fn working_shows_the_activity_phrase_and_a_waiting_message_says_so_when_idle() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    *host.activity.lock() = Some("Reading notes.md".into());
    tick_after(&service, &host, 1_000);
    assert_eq!((mochi(&service).state, mochi(&service).state_line), (CompanionState::Working, "Reading notes.md".to_string()));
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    *host.waiting.lock() = vec!["hello".into()];
    tick_after(&service, &host, 1_000);
    assert_eq!(mochi(&service).state_line, "Will read your message next");
}

// ---- check-ins ----------------------------------------------------------------------------------

#[test]
fn a_daily_check_in_goes_through_the_outbox_once_a_day() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Morning brief", When::Daily { at: at(8, 30) })]).unwrap();
    let view = mochi(&service);
    assert_eq!(view.next_checkin.as_ref().map(|n| n.at_ms), Some(start_ms() + 30 * MIN));
    assert_eq!(view.checkins.as_ref().map(Vec::len), Some(1), "the desktop view carries the list");

    tick_after(&service, &host, 29 * MIN);
    assert!(!host.sent_texts().iter().any(|t| t.starts_with("[Check-in:")));
    tick_after(&service, &host, MIN);
    let sent: Vec<String> = host.sent_texts().into_iter().filter(|t| t.starts_with("[Check-in:")).collect();
    assert_eq!(sent, vec!["[Check-in: Morning brief] What is on today?"]);
    assert_eq!(texts(&root, FeedKind::Checkin), vec!["Asked \"Morning brief\""]);
    for _ in 0..5 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1, "once, not every tick");

    // Next morning it goes again.
    tick_after(&service, &host, 24 * HOUR);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 2);
}

#[test]
fn a_check_in_that_is_still_waiting_blocks_the_next_one_with_a_missed_line() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Hourly", When::Every { minutes: 30 })]).unwrap();
    // The first one is still queued in the outbox (the agent is busy) when the next is due.
    tick_after(&service, &host, 30 * MIN);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1);
    *host.waiting.lock() = vec!["[Check-in: Hourly] What is on today?".into()];
    tick_after(&service, &host, 30 * MIN);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1, "nothing new went in");
    assert_eq!(texts(&root, FeedKind::Missed), vec!["Skipped \"Hourly\": the last check-in is still waiting."]);
    // The outbox drained: the next one goes.
    host.waiting.lock().clear();
    tick_after(&service, &host, 30 * MIN);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 2);
}

#[test]
fn the_daily_cap_of_24_stops_scheduled_messages_for_that_day() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Often", When::Every { minutes: 30 })]).unwrap();
    service.inner.lock().slots.get_mut("mochi").unwrap().runtime.sent_today = SentToday { date: "2026-10-28".into(), count: 24 };
    tick_after(&service, &host, 30 * MIN);
    assert!(!host.sent_texts().iter().any(|t| t.starts_with("[Check-in:")));
    assert_eq!(texts(&root, FeedKind::Missed), vec!["Skipped \"Often\": the daily limit was reached."]);
    // Tomorrow the count starts over.
    tick_after(&service, &host, 24 * HOUR);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1);
    assert_eq!(store::runtime_load(&root, "mochi").unwrap().sent_today.count, 1);
}

#[test]
fn a_once_check_in_sends_and_then_turns_itself_off() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Reminder", When::Once { at_ms: start_ms() + 10 * MIN })]).unwrap();
    tick_after(&service, &host, 10 * MIN);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1);
    let list = mochi(&service).checkins.unwrap();
    assert!(!list[0].enabled, "disabled after it ran");
    assert!(mochi(&service).next_checkin.is_none());
    tick_after(&service, &host, HOUR);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1);
    // The change was saved: a new service reads it disabled.
    let reread = store::load(&_root).unwrap();
    assert!(!reread[0].checkins[0].enabled);
}

#[test]
fn a_check_in_wakes_a_sleeping_companion_and_goes_in_after_the_primer() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Morning brief", When::Daily { at: at(8, 30) })]).unwrap();
    service.sleep("mochi").unwrap();
    assert_eq!(host.spawn_count(), 1);
    host.sent.lock().clear();
    tick_after(&service, &host, 30 * MIN);
    assert_eq!(host.spawn_count(), 2, "woken for the check-in");
    let sent = host.sent_texts();
    assert!(sent[0].starts_with("Please read SOUL.md"), "the primer first: {sent:?}");
    assert_eq!(sent[1], "[Check-in: Morning brief] What is on today?");
}

#[test]
fn closed_for_less_than_two_hours_runs_once_and_longer_is_reported_missed() {
    // Puppet Master was closed at 08:30; it opens again at 09:30.
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Morning brief", When::Daily { at: at(8, 30) })]).unwrap();
    tick_after(&service, &host, 90 * MIN);
    assert_eq!(host.sent_texts().iter().filter(|t| t.starts_with("[Check-in:")).count(), 1, "ran once, late");
    assert!(texts(&root, FeedKind::Missed).is_empty());

    // Another one: opens five hours late.
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    service.checkins_set("mochi", vec![checkin("0a0a0a0a", "Morning brief", When::Daily { at: at(8, 30) })]).unwrap();
    tick_after(&service, &host, 5 * HOUR);
    assert!(!host.sent_texts().iter().any(|t| t.starts_with("[Check-in:")));
    assert_eq!(texts(&root, FeedKind::Missed), vec!["Missed \"Morning brief\" at 8:30. Puppet Master was closed."]);
}

// ---- nap ----------------------------------------------------------------------------------------

#[test]
fn the_nap_restarts_an_idle_companion_with_fresh_notes_at_night() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    // 04:00 the next day, idle since the evening before.
    host.set_now(start_ms() + 20 * HOUR);
    service.tick();
    assert!(host.killed.lock().contains(&PANE.to_string()));
    assert_eq!(texts(&root, FeedKind::Napped), vec!["Took a nap. Woke up with fresh notes."]);
    tick_after(&service, &host, 1_000);
    assert_eq!(host.spawn_count(), 2, "started again with a new brief");
    assert_eq!(texts(&root, FeedKind::Woke).len(), 1, "a nap is not a second 'woke up'");
    assert_eq!(store::runtime_load(&root, "mochi").unwrap().last_nap_ms, Some(start_ms() + 20 * HOUR));
    // Not again the same night.
    boot(&service, &host);
    host.set_now(start_ms() + 20 * HOUR + 45 * MIN);
    service.tick();
    assert_eq!(host.spawn_count(), 2);
}

#[test]
fn the_nap_waits_for_thirty_idle_minutes_and_an_empty_outbox() {
    let (service, host, _root) = service_with_one();
    host.set_now(start_ms() + 19 * HOUR + 45 * MIN); // 03:45
    boot(&service, &host); // idle since 03:45
    host.set_now(start_ms() + 20 * HOUR + 10 * MIN); // 04:10: only 25 minutes idle
    service.tick();
    assert!(host.killed.lock().is_empty(), "idle for only 25 minutes");
    *host.waiting.lock() = vec!["hello".into()];
    host.set_now(start_ms() + 20 * HOUR + 20 * MIN); // 04:20: idle 35 minutes, but a message waits
    service.tick();
    assert!(host.killed.lock().is_empty(), "a message is waiting");
    host.waiting.lock().clear();
    host.set_now(start_ms() + 20 * HOUR + 30 * MIN);
    service.tick();
    assert!(host.killed.lock().contains(&PANE.to_string()));
}

// ---- replies and memory ------------------------------------------------------------------------

#[test]
fn a_finished_turn_puts_the_agents_last_words_in_the_feed_once() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    let turn = |reply: Option<Reply>| {
        host.set_pane(PANE, Pane::Alive(Status::Running));
        tick_after(&service, &host, 1_000);
        *host.reply.lock() = reply;
        host.set_pane(PANE, Pane::Alive(Status::Idle));
        tick_after(&service, &host, 1_000);
        tick_after(&service, &host, 2_000);
    };
    // The answer to the primer ("ready") is not worth a line.
    turn(Some(Reply { id: "1:0".into(), text: "ready".into() }));
    assert!(texts(&root, FeedKind::Reply).is_empty());
    turn(Some(Reply { id: "2:0".into(), text: "Hello\nthere, friend".into() }));
    assert_eq!(texts(&root, FeedKind::Reply), vec!["Hello there, friend"]);
    for _ in 0..4 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(texts(&root, FeedKind::Reply).len(), 1, "no repeat");
}

#[test]
fn without_a_session_file_the_turn_summary_sentence_is_the_fallback() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    *host.sentence.lock() = Some(Reply { id: "turn-1-5".into(), text: "Edited 2 files.".into() });
    tick_after(&service, &host, 1_000);
    tick_after(&service, &host, 3_000);
    assert!(texts(&root, FeedKind::Reply).is_empty(), "the reader is given time first");
    tick_after(&service, &host, 5_000);
    // The primer's reply is skipped once; the sentence is that first turn here, so skip it, then a real one.
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    *host.sentence.lock() = Some(Reply { id: "turn-2-9".into(), text: "Wrote the notes.".into() });
    tick_after(&service, &host, 1_000);
    tick_after(&service, &host, 9_000);
    assert_eq!(texts(&root, FeedKind::Reply), vec!["Wrote the notes."]);
}

#[test]
fn new_proposals_are_counted_and_kept_automatically_only_when_review_is_off() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    tick_after(&service, &host, 3_000);
    let home = root.join("mochi");
    std::fs::write(home.join("memory").join("pending.md"), "- note: Likes tea.\n- note: read https://x.example/y\n").unwrap();
    tick_after(&service, &host, 3_000);
    let view = mochi(&service);
    assert_eq!(view.pending_memory, 2);
    assert_eq!(texts(&root, FeedKind::Memory), vec!["Mochi wants to remember something."]);
    assert!(std::fs::read_to_string(home.join("MEMORY.md")).unwrap().is_empty(), "nothing is kept without the person");

    service.update("mochi", Patch { memory_review: Some(false), ..Patch::default() }).unwrap();
    tick_after(&service, &host, 3_000);
    let kept = std::fs::read_to_string(home.join("MEMORY.md")).unwrap();
    assert!(kept.contains("Likes tea."), "{kept}");
    assert!(!kept.contains("https://"), "a flagged line still waits for the person");
    assert_eq!(mochi(&service).pending_memory, 1);
}

// ---- making, changing, archiving ----------------------------------------------------------------

#[test]
fn a_fourth_companion_a_bypass_flag_and_an_unusable_agent_are_refused() {
    let (service, host, root) = service_with_one();
    service.create(draft("Bit", &root.join("bit"))).unwrap();
    service.create(draft("Nib", &root.join("nib"))).unwrap();
    let err = service.create(draft("Tofu", &root.join("tofu"))).unwrap_err();
    assert_eq!(err.code, "COMPANION_LIMIT");

    let (service, host2, root) = fresh();
    let mut bad = draft("Mochi", &root.join("mochi"));
    bad.launch_args = vec!["--dangerously-skip-permissions".into()];
    assert_eq!(service.create(bad).unwrap_err().code, "COMPANION_BYPASS_FLAG");
    *host2.agent_error.lock() = Some("Chat can't send messages to this agent yet.".into());
    let err = service.create(draft("Mochi", &root.join("mochi"))).unwrap_err();
    assert_eq!((err.code, err.message.as_str()), ("COMPANION_AGENT_UNAVAILABLE", "Chat can't send messages to this agent yet."));
    assert!(service.views(false).is_empty() && host2.spawn_count() == 0, "nothing was saved or started");
    assert_eq!(service.create(draft(" x", &root.join("x"))).unwrap_err().code, "COMPANION_NAME_INVALID");
    let _ = host;
}

#[test]
fn two_companions_with_one_name_get_different_ids_and_panes() {
    let (service, host, root) = service_with_one();
    let second = service.create(draft("Mochi", &root.join("mochi-two"))).unwrap();
    assert_eq!(second.id, "mochi-2");
    assert_eq!(second.pane_id.as_deref(), Some("companion-mochi-2"));
    assert_eq!(host.spawn_count(), 2);
}

#[test]
fn archiving_closes_the_pane_hides_it_and_keeps_the_folder() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    service.archive("mochi").unwrap();
    assert!(host.killed.lock().contains(&PANE.to_string()));
    assert!(host.cleared.lock().contains(&"mochi".to_string()));
    assert!(service.views(false).is_empty());
    assert!(root.join("mochi").join("SOUL.md").exists(), "the folder is never deleted");
    assert_eq!(service.archive("mochi").unwrap_err().code, "COMPANION_NOT_FOUND");
    for _ in 0..3 {
        tick_after(&service, &host, 10_000);
    }
    assert_eq!(host.spawn_count(), 1, "an archived companion is not kept alive");
    assert!(store::load(&root).unwrap()[0].archived);
}

#[test]
fn changing_the_agent_or_folder_restarts_the_pane_and_refuses_a_bypass_flag() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    let bypass = Patch { launch_args: Some(vec!["--yolo".into(), "--dangerously-skip-permissions".into()]), ..Patch::default() };
    assert_eq!(service.update("mochi", bypass).unwrap_err().code, "COMPANION_BYPASS_FLAG");
    assert_eq!(host.spawn_count(), 1);
    let rename = Patch { name: Some("Moo".into()), ..Patch::default() };
    assert_eq!(service.update("mochi", rename).unwrap().name, "Moo");
    assert_eq!(host.spawn_count(), 1, "a new name needs no restart");
    let folder = Patch { folder: Some(root.join("elsewhere").display().to_string()), ..Patch::default() };
    service.update("mochi", folder).unwrap();
    assert_eq!(host.spawn_count(), 2);
    assert!(host.spawned.lock()[1].cwd.ends_with("elsewhere"));
}

#[test]
fn the_host_hears_about_a_change_once_and_not_on_a_quiet_tick() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    let published = host.published.lock().len();
    assert!(published >= 1);
    for _ in 0..4 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(host.published.lock().len(), published, "nothing changed, nothing published");
    service.sleep("mochi").unwrap();
    assert_eq!(host.published.lock().len(), published + 1);
    assert_eq!(host.published.lock().last().unwrap()[0].state, CompanionState::Sleeping);
}

#[test]
fn memory_edits_keep_the_caps_and_the_phone_view_hides_outside_changes() {
    let (service, _host, root) = service_with_one();
    let long = "x".repeat(2_201);
    let err = service.memory_save("mochi", super::model::MemoryFile::Memory, &long).unwrap_err();
    assert_eq!(err.code, "COMPANION_FILE_TOO_LONG");
    service.memory_save("mochi", super::model::MemoryFile::User, "Works at night.").unwrap();
    // An edit made outside Puppet Master shows on the desktop only.
    std::fs::write(root.join("mochi").join("SOUL.md"), "I am someone else.").unwrap();
    assert_eq!(service.memory_view("mochi", false).unwrap().outside_changes.len(), 1);
    assert!(service.memory_view("mochi", true).unwrap().outside_changes.is_empty());
    let kept = service.outside_decide("mochi", super::model::MemoryFile::Soul, false).unwrap();
    assert!(kept.outside_changes.is_empty());
    assert!(!std::fs::read_to_string(root.join("mochi").join("SOUL.md")).unwrap().contains("someone else"), "Undo wrote the approved copy back");
}

// ---- replies and push ----------------------------------------------------------------------------

/// One reply, the way the live host hands it over: the keeper's turn ends and the agent's words are read.
fn finish_turn(service: &Service, host: &FakeHost, id: &str, text: &str) {
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(service, host, 1_000);
    *host.reply.lock() = Some(Reply { id: id.into(), text: text.into() });
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(service, host, 1_000);
    tick_after(service, host, 2_000);
}

#[test]
fn a_reply_is_handed_to_the_push_layer_with_the_companions_toggle() {
    let (service, host, _root) = service_with_one();
    boot(&service, &host);
    // The primer's answer is not a reply: it is never pushed or kept.
    finish_turn(&service, &host, "1:0", "ready");
    assert!(host.pushed.lock().is_empty(), "the primer is not pushed");
    finish_turn(&service, &host, "2:0", "Hello there, friend");
    {
        let pushed = host.pushed.lock();
        assert_eq!(pushed.len(), 1, "one reply, one hand-over");
        assert_eq!(pushed[0].0, "mochi");
        assert_eq!(pushed[0].1, "Mochi");
        assert_eq!(pushed[0].2, "Hello there, friend");
        assert!(pushed[0].3, "on by default");
    }
    service.update("mochi", Patch { push_replies: Some(false), ..Patch::default() }).unwrap();
    assert!(!mochi(&service).push_replies);
    finish_turn(&service, &host, "3:0", "Second, quieter one.");
    let pushed = host.pushed.lock();
    assert_eq!(pushed.len(), 2);
    assert!(!pushed[1].3, "the toggle is off: the push layer is told not to send");
}

#[test]
fn a_reply_handed_over_has_the_home_folder_taken_out() {
    let (service, host, root) = service_with_one();
    boot(&service, &host);
    let folder = root.join("mochi").display().to_string();
    finish_turn(&service, &host, "1:0", "ready");
    finish_turn(&service, &host, "2:0", &format!("Saved notes in {folder}/notes.md"));
    let pushed = host.pushed.lock();
    assert!(!pushed[0].2.contains(&folder), "{}", pushed[0].2);
    assert!(pushed[0].2.contains("its folder"), "{}", pushed[0].2);
}
