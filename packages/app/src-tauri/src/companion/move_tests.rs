//! Moving a companion to another agent and back, on the fake host and a clock moved by hand: a move while
//! it is idle, a move that waits while it is busy or has a card open, a timed move that goes back by
//! itself (or stays while the old agent is out of usage), an agent the registry refuses, and the promise
//! that the memory files are not touched.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::host::Blocking;
use super::keeper::{Pane, Status};
use super::manage::Patch;
use super::model::{CompanionState, CompanionView, FeedKind};
use super::store;
use super::test_host::{draft, start_ms, temp_dir, FakeHost};
use super::Service;
use crate::tui::InstructionFile;

const PANE: &str = "companion-mochi";
const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
/// The agent it starts on (the draft's) and the one it moves to. The fake registry accepts any id.
const HOME: &str = "claude";
const AWAY: &str = "other";

fn fresh() -> (Service, Arc<FakeHost>, PathBuf) {
    let root = temp_dir("pm-companion-move");
    let host = FakeHost::new(start_ms());
    let service = Service::new(host.clone(), root.clone());
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    (service, host, root)
}

fn tick_after(service: &Service, host: &FakeHost, ms: i64) {
    host.advance_ms(ms);
    service.tick();
}

/// The agent drew its input box and reads idle.
fn boot(service: &Service, host: &FakeHost) {
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(service, host, 1_000);
}

fn mochi(service: &Service) -> CompanionView {
    service.view("mochi", false).unwrap()
}

fn move_to(agent: &str) -> Patch {
    Patch { agent_type: Some(agent.into()), ..Patch::default() }
}

fn move_until(agent: &str, at_ms: i64) -> Patch {
    Patch { agent_type: Some(agent.into()), until_ms: Some(Some(at_ms)), ..Patch::default() }
}

fn agents_spawned(host: &FakeHost) -> Vec<String> {
    host.spawned.lock().iter().map(|s| s.agent_type.clone()).collect()
}

fn moved_lines(root: &Path) -> Vec<String> {
    store::feed_read(root, "mochi").unwrap().into_iter().filter(|i| i.kind == FeedKind::Moved).map(|i| i.text).collect()
}

fn read(path: PathBuf) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn a_move_while_idle_restarts_it_on_the_new_agent_in_the_same_home() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    assert_eq!(mochi(&service).state, CompanionState::Idle);

    let view = service.update("mochi", move_to(AWAY)).unwrap();

    assert_eq!(host.killed.lock().clone(), vec![PANE.to_string()], "the old pane was closed");
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY], "and started again on the new agent");
    let spawned = host.spawned.lock().clone();
    assert_eq!(spawned[1].pane_id, PANE, "same pane id");
    assert_eq!(spawned[1].cwd, spawned[0].cwd, "same home folder");
    assert_eq!(view.agent_type, AWAY);
    assert_eq!(view.agent_label, "Agent other");
    assert_eq!(view.previous_agent.as_deref(), Some(HOME));
    assert_eq!(view.previous_agent_label.as_deref(), Some("Agent claude"));
    assert!(view.queued_move.is_none());
    assert_eq!((view.name.as_str(), view.avatar.as_str()), ("Mochi", "mochi"), "same name and face");
    assert_eq!(moved_lines(&root), vec!["Mochi moved to Agent other"]);
    // The pane was closed by the keeper itself: it is not read as "the person closed it".
    assert_ne!(mochi(&service).state, CompanionState::Sleeping);
    let saved = store::load(&root).unwrap();
    assert_eq!(saved[0].agent_type, AWAY, "the move is saved");
}

#[test]
fn the_brief_is_rewritten_for_the_new_agent_and_the_memory_files_are_not_touched() {
    let root = temp_dir("pm-companion-move");
    let host = FakeHost::new(start_ms());
    host.files_of.lock().insert(HOME.into(), vec![InstructionFile::new("CLAUDE.md")]);
    host.files_of.lock().insert(AWAY.into(), vec![InstructionFile::new("OTHER.md")]);
    let service = Service::new(host.clone(), root.clone());
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    let home = root.join("mochi");
    // Texts the person (or the agent) wrote: odd bytes on purpose (CRLF, no final newline, non-ASCII).
    std::fs::write(home.join("SOUL.md"), "Soft voice.\r\nCurious.").unwrap();
    std::fs::write(home.join("USER.md"), "Ren works at night \u{2014} UTC+8.\n").unwrap();
    std::fs::write(home.join("MEMORY.md"), "- likes short answers\n- uses tabs\t\n").unwrap();
    std::fs::write(home.join("memory").join("pending.md"), "- note: likes tea\n- forget: uses tabs\n").unwrap();
    std::fs::write(home.join("today.md"), "10:00 Fixed a bug.\n").unwrap();
    let names = ["SOUL.md", "USER.md", "MEMORY.md", "today.md"];
    let before: Vec<Vec<u8>> = names.iter().map(|n| read(home.join(n))).collect();
    let pending_before = read(home.join("memory").join("pending.md"));

    boot(&service, &host);
    service.update("mochi", move_to(AWAY)).unwrap();
    boot(&service, &host);

    let after: Vec<Vec<u8>> = names.iter().map(|n| read(home.join(n))).collect();
    assert_eq!(before, after, "SOUL, USER, MEMORY and today are byte for byte the same");
    assert_eq!(pending_before, read(home.join("memory").join("pending.md")), "the pending list too");
    assert!(home.join("CLAUDE.md").exists(), "the first agent's file was written before the move");
    let new_brief = std::fs::read_to_string(home.join("OTHER.md")).unwrap();
    assert!(new_brief.contains("puppet-master:companion:start"), "{new_brief}");
    // The files were edited outside Puppet Master, so the brief keeps using the approved copies (as at any start).
    assert!(new_brief.contains("# You are Mochi") && new_brief.contains("calm, steady helper"), "{new_brief}");
    assert!(new_brief.contains("memory/pending.md"), "the house rules are there for the new agent");
}

#[test]
fn a_move_while_it_is_working_waits_and_runs_when_it_is_free() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    assert_eq!(mochi(&service).state, CompanionState::Working);

    let view = service.update("mochi", move_to(AWAY)).unwrap();
    assert_eq!(view.agent_type, HOME, "it stays on its agent while it works");
    let queued = view.queued_move.expect("the move is queued");
    assert_eq!((queued.agent_type.as_str(), queued.agent_label.as_str()), (AWAY, "Agent other"));
    assert!(host.killed.lock().is_empty(), "nothing was closed");
    assert_eq!(host.spawn_count(), 1);

    tick_after(&service, &host, 5_000);
    assert_eq!(host.spawn_count(), 1, "still busy: still waiting");
    assert!(mochi(&service).queued_move.is_some());

    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(&service, &host, 1_000);
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY], "it ran once the turn ended");
    let view = mochi(&service);
    assert_eq!((view.agent_type.as_str(), view.queued_move.is_none()), (AWAY, true));
    assert_eq!(moved_lines(&root).len(), 1);
}

#[test]
fn a_move_waits_while_a_question_card_is_open() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::WaitingInput));
    *host.blocking.lock() = Some(Blocking { item_id: "i1".into(), title: "Trust this folder?".into() });
    tick_after(&service, &host, 1_000);
    assert_eq!(mochi(&service).state, CompanionState::NeedsYou);

    let view = service.update("mochi", move_to(AWAY)).unwrap();
    assert!(view.queued_move.is_some(), "queued, not run");
    assert_eq!(view.agent_type, HOME);
    for _ in 0..3 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(host.spawn_count(), 1, "the card is the person's: nothing happens around it");

    *host.blocking.lock() = None;
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(&service, &host, 1_000);
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY]);
}

#[test]
fn a_message_still_waiting_in_the_outbox_counts_as_busy() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    *host.waiting.lock() = vec!["hello".into()];
    tick_after(&service, &host, 1_000);
    assert!(service.update("mochi", move_to(AWAY)).unwrap().queued_move.is_some());
    *host.waiting.lock() = Vec::new();
    tick_after(&service, &host, 1_000);
    assert_eq!(mochi(&service).agent_type, AWAY);
}

#[test]
fn the_same_agent_again_cancels_a_move_that_was_waiting() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    service.update("mochi", move_to(AWAY)).unwrap();
    let view = service.update("mochi", move_to(HOME)).unwrap();
    assert!(view.queued_move.is_none());
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(&service, &host, 1_000);
    assert_eq!(host.spawn_count(), 1, "nothing moved");
}

#[test]
fn a_timed_move_goes_back_by_itself() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    let until = host.now() + 9 * HOUR;
    let view = service.update("mochi", move_until(AWAY, until)).unwrap();
    assert_eq!(view.until_ms, Some(until));
    assert_eq!(moved_lines(&root), vec!["Mochi moved to Agent other until 17:00"]);
    boot(&service, &host);

    tick_after(&service, &host, HOUR);
    assert_eq!(mochi(&service).agent_type, AWAY, "not yet");
    host.set_now(until);
    service.tick();

    assert_eq!(agents_spawned(&host), vec![HOME, AWAY, HOME], "it went back to the first agent");
    let view = mochi(&service);
    assert_eq!(view.agent_type, HOME);
    assert_eq!((view.previous_agent, view.until_ms, view.queued_move), (None, None, None));
    assert_eq!(moved_lines(&root).last().unwrap(), "Mochi moved back to Agent claude");
    let saved = store::load(&root).unwrap();
    assert!(saved[0].previous_agent.is_none() && saved[0].until_ms.is_none());
}

#[test]
fn the_way_back_waits_too_when_it_is_busy_at_that_time() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    let until = host.now() + 2 * HOUR;
    service.update("mochi", move_until(AWAY, until)).unwrap();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    host.set_now(until + MIN);
    service.tick();
    assert_eq!(host.spawn_count(), 2, "busy: it does not go back in the middle of a turn");
    assert!(mochi(&service).queued_move.is_some_and(|m| m.agent_type == HOME));
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(&service, &host, 1_000);
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY, HOME]);
}

#[test]
fn it_stays_when_the_old_agent_is_still_out_of_usage_and_says_so() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    let until = host.now() + HOUR;
    service.update("mochi", move_until(AWAY, until)).unwrap();
    boot(&service, &host);
    let reset = until + 40 * MIN;
    host.limits.lock().insert((HOME.into(), None), Some(reset));

    host.set_now(until + MIN);
    service.tick();
    assert_eq!(mochi(&service).agent_type, AWAY, "it stays");
    assert_eq!(host.spawn_count(), 2);
    let lines = moved_lines(&root);
    assert_eq!(lines.last().unwrap(), "Agent claude is still out of usage. Mochi stays on Agent other.");
    assert_eq!(mochi(&service).until_ms, Some(reset + MIN), "it looks again after the reset");

    // The limit is over: at the new time it goes back.
    host.limits.lock().clear();
    host.set_now(reset + 2 * MIN);
    service.tick();
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY, HOME]);
}

#[test]
fn a_limit_without_a_reset_time_is_asked_again_in_half_an_hour() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    let until = host.now() + HOUR;
    service.update("mochi", move_until(AWAY, until)).unwrap();
    boot(&service, &host);
    host.limits.lock().insert((HOME.into(), None), None);
    host.set_now(until + MIN);
    service.tick();
    assert_eq!(mochi(&service).until_ms, Some(until + MIN + super::moving::STILL_LIMITED_RETRY_MS));
}

#[test]
fn a_limit_on_another_plan_of_the_old_agent_does_not_keep_it_away() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    // It runs on plan A of the home agent; plan B of that agent is the one at its limit.
    service.update("mochi", Patch { account: Some(Some("acct-a".into())), ..Patch::default() }).unwrap();
    boot(&service, &host);
    let until = host.now() + HOUR;
    service.update("mochi", move_until(AWAY, until)).unwrap();
    boot(&service, &host);
    host.limits.lock().insert((HOME.into(), Some("acct-b".into())), Some(until + 5 * HOUR));

    host.set_now(until + MIN);
    service.tick();
    tick_after(&service, &host, 1_000);
    // (The first start, the restart for the account change, the timed move, then the move back.)
    assert_eq!(agents_spawned(&host).last().map(String::as_str), Some(HOME), "plan A is free: it goes back");
    assert_eq!(mochi(&service).agent_type, HOME);
    assert!(moved_lines(&root).iter().all(|l| !l.contains("still out of usage")), "{:?}", moved_lines(&root));
}

#[test]
fn an_agent_the_registry_refuses_is_not_moved_to_and_nothing_changes() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    host.refused.lock().push("nope".into());

    let err = service.update("mochi", move_to("nope")).unwrap_err();
    assert_eq!(err.code, "COMPANION_AGENT_UNAVAILABLE");
    assert!(err.message.contains("not installed"), "{}", err.message);
    let view = mochi(&service);
    assert_eq!((view.agent_type.as_str(), view.queued_move.is_none(), view.previous_agent.is_none()), (HOME, true, true));
    assert!(host.killed.lock().is_empty() && host.spawn_count() == 1, "the pane was not touched");
    assert!(moved_lines(&root).is_empty());

    // Refused even when it is busy (a typo must not sit in the queue).
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    assert!(service.update("mochi", move_to("nope")).is_err());
    assert!(mochi(&service).queued_move.is_none());
    // An empty id is refused too.
    assert_eq!(service.update("mochi", move_to("  ")).unwrap_err().code, "COMPANION_AGENT_UNAVAILABLE");
}

#[test]
fn a_time_that_has_passed_is_refused() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    let err = service.update("mochi", move_until(AWAY, host.now() - MIN)).unwrap_err();
    assert_eq!(err.code, "COMPANION_MOVE_TIME_PAST");
    assert_eq!(mochi(&service).agent_type, HOME);
}

#[test]
fn a_move_that_cannot_run_any_more_is_dropped_with_a_feed_line() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    service.update("mochi", move_to(AWAY)).unwrap();
    // The agent was removed while it waited.
    host.refused.lock().push(AWAY.into());
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(&service, &host, 1_000);
    let view = mochi(&service);
    assert_eq!((view.agent_type.as_str(), view.queued_move.is_none()), (HOME, true));
    assert!(moved_lines(&root).last().unwrap().starts_with("Mochi could not move to Agent other:"));
}

#[test]
fn a_sleeping_companion_changes_agent_without_starting_and_starts_there_when_woken() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    service.sleep("mochi").unwrap();
    assert_eq!(mochi(&service).state, CompanionState::Sleeping);
    let spawns = host.spawn_count();

    let view = service.update("mochi", move_to(AWAY)).unwrap();
    assert_eq!(view.agent_type, AWAY);
    assert_eq!(view.state, CompanionState::Sleeping, "a move does not wake it");
    assert_eq!(host.spawn_count(), spawns);

    service.wake("mochi").unwrap();
    assert_eq!(agents_spawned(&host).last().map(String::as_str), Some(AWAY));
}

#[test]
fn a_lasting_move_remembers_where_it_came_from_and_a_timed_stay_keeps_its_first_home() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    // Lasting: HOME -> AWAY. "previous" is HOME.
    service.update("mochi", move_to(AWAY)).unwrap();
    boot(&service, &host);
    assert_eq!(mochi(&service).previous_agent.as_deref(), Some(HOME));

    // A second lasting move: the one it left is the new "previous".
    service.update("mochi", move_to("third")).unwrap();
    boot(&service, &host);
    assert_eq!(mochi(&service).previous_agent.as_deref(), Some(AWAY));

    // A timed stay from there, then another timed move: the home to return to stays AWAY.
    let until = host.now() + 3 * HOUR;
    service.update("mochi", move_until("fourth", until)).unwrap();
    boot(&service, &host);
    assert_eq!(mochi(&service).previous_agent.as_deref(), Some("third"));
    service.update("mochi", move_until("fifth", until + HOUR)).unwrap();
    boot(&service, &host);
    let view = mochi(&service);
    assert_eq!((view.agent_type.as_str(), view.previous_agent.as_deref()), ("fifth", Some("third")));

    // "Move back" is just a move to the previous agent.
    service.update("mochi", move_to("third")).unwrap();
    let view = mochi(&service);
    assert_eq!((view.agent_type.as_str(), view.previous_agent, view.until_ms), ("third", None, None));
}

#[test]
fn launch_args_and_account_stay_with_the_agent_they_were_set_for() {
    let (service, host, root) = fresh();
    service.update("mochi", Patch { launch_args: Some(vec!["--model".into(), "small".into()]), ..Patch::default() }).unwrap();
    boot(&service, &host);
    service.update("mochi", move_to(AWAY)).unwrap();
    boot(&service, &host);
    assert!(host.spawned.lock().last().unwrap().extra_args.is_empty(), "the old agent's flags are not given to the new one");
    assert_eq!(store::load(&root).unwrap()[0].launch_args, Vec::<String>::new());

    service.update("mochi", move_to(HOME)).unwrap();
    let last = host.spawned.lock().last().unwrap().clone();
    assert_eq!((last.agent_type.as_str(), last.extra_args), (HOME, vec!["--model".to_string(), "small".to_string()]));
}

#[test]
fn a_move_is_told_to_the_push_layer_with_the_companions_own_setting() {
    let (service, host, _root) = fresh();
    boot(&service, &host);
    service.update("mochi", Patch { push_replies: Some(false), ..Patch::default() }).unwrap();
    service.update("mochi", move_to(AWAY)).unwrap();
    let pushed = host.pushed.lock().clone();
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0].2, "Mochi moved to Agent other");
    assert!(!pushed[0].3, "the push setting is passed on: the push layer sends nothing when it is off");
}

#[test]
fn a_waiting_move_survives_a_restart_of_the_app() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    service.update("mochi", move_to(AWAY)).unwrap();
    drop(service);

    let again = Service::new(host.clone(), root.clone());
    let view = again.view("mochi", false).unwrap();
    assert_eq!(view.queued_move.map(|m| m.agent_type), Some(AWAY.to_string()));
}

#[test]
fn the_phone_copy_shows_the_agent_and_never_the_folder() {
    let (service, host, root) = fresh();
    boot(&service, &host);
    let until = host.now() + HOUR;
    service.update("mochi", move_until(AWAY, until)).unwrap();
    let phone = service.view("mochi", true).unwrap();
    assert_eq!(phone.agent_label, "Agent other");
    let json = serde_json::to_string(&phone).unwrap();
    assert!(!json.contains(&root.display().to_string()), "{json}");
}
