//! The keeper and a startup notice (an update menu) on the companion's own pane: it presses only the keys the
//! profile declares for it, at most once per 20 s, a move is not held up by it, and a screen only the person
//! may pass (trust, sign-in) is never touched. On the fake host and a clock moved by hand.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::host::Blocking;
use super::keeper::{Pane, Status};
use super::manage::Patch;
use super::model::CompanionState;
use super::test_host::{draft, start_ms, temp_dir, FakeHost};
use super::Service;

const PANE: &str = "companion-mochi";
const HOME: &str = "claude";
const AWAY: &str = "other";

fn fresh() -> (Service, Arc<FakeHost>) {
    let root = temp_dir("pm-companion-gate");
    let host = FakeHost::new(start_ms());
    let service = Service::new(host.clone(), root.clone());
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    (service, host)
}

fn tick_after(service: &Service, host: &FakeHost, ms: i64) {
    host.advance_ms(ms);
    service.tick();
}

fn boot(service: &Service, host: &FakeHost) {
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    tick_after(service, host, 1_000);
}

/// The agent drew its update menu over the input box: the pane reads running.
fn menu_appears(host: &FakeHost) {
    host.notice.lock().push(PANE.to_string());
    host.set_pane(PANE, Pane::Alive(Status::Running));
}

fn move_to(agent: &str) -> Patch {
    Patch { agent_type: Some(agent.into()), ..Patch::default() }
}

fn agents_spawned(host: &FakeHost) -> Vec<String> {
    host.spawned.lock().iter().map(|s| s.agent_type.clone()).collect()
}

fn state(service: &Service) -> CompanionState {
    service.view("mochi", false).unwrap().state
}

#[test]
fn a_notice_on_a_quiet_companion_is_closed_once_with_its_own_keys() {
    let (service, host) = fresh();
    boot(&service, &host);
    menu_appears(&host);
    tick_after(&service, &host, 1_000);
    assert_eq!(host.notice_presses.lock().clone(), vec![PANE.to_string()]);
    assert!(host.notice.lock().is_empty(), "the menu is gone");
    tick_after(&service, &host, 1_000);
    assert_eq!(state(&service), CompanionState::Idle, "it reads free again");
    assert_eq!(host.notice_presses.lock().len(), 1, "nothing else was pressed");
}

#[test]
fn a_queued_move_happens_after_the_notice_is_closed() {
    let (service, host) = fresh();
    boot(&service, &host);
    // A turn is running when the person asks for the move, so it waits.
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    assert!(service.update("mochi", move_to(AWAY)).unwrap().queued_move.is_some());
    assert_eq!(host.spawn_count(), 1);

    // The turn ended on a menu that nobody closes.
    host.notice.lock().push(PANE.to_string());
    tick_after(&service, &host, 1_000);
    assert_eq!(host.notice_presses.lock().len(), 1);
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY], "and the move ran");
    assert!(service.view("mochi", false).unwrap().queued_move.is_none());
}

#[test]
fn a_notice_that_ignores_its_keys_is_not_pressed_again_within_20_seconds() {
    let (service, host) = fresh();
    host.notice_obeys.store(false, Ordering::SeqCst);
    boot(&service, &host);
    menu_appears(&host);
    tick_after(&service, &host, 1_000);
    assert_eq!(host.notice_presses.lock().len(), 1);
    for _ in 0..18 {
        tick_after(&service, &host, 1_000);
    }
    assert_eq!(host.notice_presses.lock().len(), 1, "19 s after the first press");
    tick_after(&service, &host, 2_000);
    assert_eq!(host.notice_presses.lock().len(), 2, "after 20 s it tries once more");
    tick_after(&service, &host, 1_000);
    assert_eq!(host.notice_presses.lock().len(), 2);
}

#[test]
fn a_move_is_not_held_up_by_a_notice_that_ignores_its_keys() {
    let (service, host) = fresh();
    host.notice_obeys.store(false, Ordering::SeqCst);
    boot(&service, &host);
    menu_appears(&host);
    tick_after(&service, &host, 1_000);
    assert_eq!(state(&service), CompanionState::Working, "the pane reads running behind the menu");

    let view = service.update("mochi", move_to(AWAY)).unwrap();
    assert!(view.queued_move.is_none(), "stopping the pane is fine: the move ran at once");
    assert_eq!(host.killed.lock().clone(), vec![PANE.to_string()]);
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY]);
    assert_eq!(host.notice_presses.lock().len(), 1);
}

#[test]
fn a_move_is_not_held_up_by_a_notice_while_the_agent_is_still_starting() {
    let (service, host) = fresh();
    host.notice_obeys.store(false, Ordering::SeqCst);
    // Never read idle: the pane is still "starting" behind its menu.
    menu_appears(&host);
    tick_after(&service, &host, 1_000);
    assert_eq!(state(&service), CompanionState::Starting);
    assert!(service.update("mochi", move_to(AWAY)).unwrap().queued_move.is_none());
    assert_eq!(agents_spawned(&host), vec![HOME, AWAY]);
}

#[test]
fn a_message_on_its_way_in_still_counts_as_busy_behind_a_notice() {
    let (service, host) = fresh();
    host.notice_obeys.store(false, Ordering::SeqCst);
    boot(&service, &host);
    menu_appears(&host);
    *host.waiting.lock() = vec!["hello".into()];
    tick_after(&service, &host, 1_000);
    assert!(service.update("mochi", move_to(AWAY)).unwrap().queued_move.is_some(), "the message is not cut off");
}

#[test]
fn a_screen_only_the_person_may_pass_is_never_touched() {
    let (service, host) = fresh();
    boot(&service, &host);
    // Trust or sign-in: no dismiss keys, so the host reports no notice; it is the normal card.
    host.set_pane(PANE, Pane::Alive(Status::WaitingInput));
    *host.blocking.lock() = Some(Blocking { item_id: "i1".into(), title: "Trust this folder?".into() });
    tick_after(&service, &host, 1_000);
    assert_eq!(state(&service), CompanionState::NeedsYou);

    let view = service.update("mochi", move_to(AWAY)).unwrap();
    assert!(view.queued_move.is_some(), "the move waits for the person");
    for _ in 0..30 {
        tick_after(&service, &host, 1_000);
    }
    assert!(host.notice_presses.lock().is_empty(), "no key went in");
    assert_eq!(host.spawn_count(), 1);
    assert_eq!(state(&service), CompanionState::NeedsYou);

    // The same for a sign-in screen that has no card of its own.
    *host.blocking.lock() = None;
    host.set_pane(PANE, Pane::Alive(Status::Idle));
    host.sign_in.store(true, Ordering::SeqCst);
    tick_after(&service, &host, 1_000);
    assert!(host.notice_presses.lock().is_empty());
}

#[test]
fn another_pane_is_none_of_the_keepers_business() {
    let (service, host) = fresh();
    boot(&service, &host);
    host.notice.lock().push("pane-7".to_string());
    host.set_pane("pane-7", Pane::Alive(Status::Running));
    for _ in 0..30 {
        tick_after(&service, &host, 1_000);
    }
    assert!(host.notice_presses.lock().is_empty());
    assert_eq!(state(&service), CompanionState::Idle);
}

#[test]
fn a_sleeping_companion_presses_nothing() {
    let (service, host) = fresh();
    boot(&service, &host);
    service.sleep("mochi").unwrap();
    host.notice.lock().push(PANE.to_string());
    host.set_pane(PANE, Pane::Alive(Status::Running));
    tick_after(&service, &host, 1_000);
    assert!(host.notice_presses.lock().is_empty());
}
