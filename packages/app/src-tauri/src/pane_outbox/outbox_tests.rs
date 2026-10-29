//! The outbox state machine against a fake host (no terminal), and the delivery guard against a scripted pane.

use super::host::{Attempt, Host, Look, PaneKind};
use super::live::attempt_of;
use super::model::*;
use super::worker::{decide, target_block, Step};
use super::{Outbox, Timing};
use crate::agent_runs::delivery::{self, Delivery};
use crate::agent_runs::readiness::{HostShell, Readiness, PROMPT_GATE};
use crate::tui::{ComposerState, StartupGate};
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---- a fake host -----------------------------------------------------------------------------------------------

struct Fake {
    kind: Mutex<Option<PaneKind>>,
    run: Mutex<bool>,
    look: Mutex<Option<Look>>,
    attempt: Mutex<Attempt>,
    typed: Mutex<Vec<String>>,
    /// How often `type_message` was called (a gate that keeps refusing must not be retried every poll).
    calls: Mutex<usize>,
    /// When each `type_message` call began and ended: the waits the worker keeps between tries are checked
    /// against these (a gap can only grow on a busy machine, so a lower bound on it never flakes).
    attempts: Mutex<Vec<(Instant, Instant)>>,
    published: Mutex<Vec<Vec<OutboxMessage>>>,
    /// While true `type_message` waits (a message stays `sending`).
    hold: Mutex<bool>,
    released: Condvar,
}

fn look(readiness: Readiness, composer: ComposerState, idle: bool) -> Look {
    Look { readiness, composer, idle }
}

fn ready() -> Look {
    look(Readiness::Ready, ComposerState::Empty, true)
}

fn busy() -> Look {
    look(Readiness::Busy, ComposerState::Working, false)
}

impl Fake {
    fn new(first: Look) -> Arc<Self> {
        Arc::new(Self {
            kind: Mutex::new(Some(PaneKind::Agent)),
            run: Mutex::new(false),
            look: Mutex::new(Some(first)),
            attempt: Mutex::new(Attempt::Sent),
            typed: Mutex::new(Vec::new()),
            calls: Mutex::new(0),
            attempts: Mutex::new(Vec::new()),
            published: Mutex::new(Vec::new()),
            hold: Mutex::new(false),
            released: Condvar::new(),
        })
    }

    fn set(&self, next: Look) {
        *self.look.lock() = Some(next);
    }

    fn typed(&self) -> Vec<String> {
        self.typed.lock().clone()
    }

    /// The shortest time between the end of one `type_message` call and the start of the next.
    fn shortest_gap(&self) -> Option<Duration> {
        let attempts = self.attempts.lock();
        attempts.windows(2).map(|pair| pair[1].0.saturating_duration_since(pair[0].1)).min()
    }

    /// The states one message went through, as published (repeats dropped).
    fn states_of(&self, id: &str) -> Vec<State> {
        let mut seen: Vec<State> = Vec::new();
        for list in self.published.lock().iter() {
            if let Some(message) = list.iter().find(|m| m.id == id) {
                if seen.last() != Some(&message.state) {
                    seen.push(message.state);
                }
            }
        }
        seen
    }
}

impl Host for Fake {
    fn pane_kind(&self, _pane_id: &str) -> Option<PaneKind> {
        *self.kind.lock()
    }

    fn run_controls(&self, _pane_id: &str) -> bool {
        *self.run.lock()
    }

    fn look(&self, _pane_id: &str) -> Option<Look> {
        self.look.lock().clone()
    }

    fn type_message(&self, _pane_id: &str, text: &str) -> Attempt {
        *self.calls.lock() += 1;
        let began = Instant::now();
        let mut hold = self.hold.lock();
        while *hold {
            self.released.wait(&mut hold);
        }
        drop(hold);
        let attempt = self.attempt.lock().clone();
        if attempt == Attempt::Sent {
            self.typed.lock().push(text.to_string());
        }
        self.attempts.lock().push((began, Instant::now()));
        attempt
    }

    fn publish(&self, _pane_id: &str, messages: &[OutboxMessage]) {
        self.published.lock().push(messages.to_vec());
    }
}

fn fast() -> Timing {
    Timing {
        poll: Duration::from_millis(5),
        give_up: Duration::from_secs(60),
        keep: Duration::from_secs(60),
        keep_failed: Duration::from_secs(60),
        after_send: Duration::ZERO,
        gate_retry: Duration::from_millis(100),
    }
}

fn outbox(host: &Arc<Fake>) -> Outbox {
    Outbox::with_timing(host.clone(), fast())
}

/// A hang detector, not a time limit: a machine running the whole suite stretches every poll of the worker.
fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn state(outbox: &Outbox, id: &str) -> Option<OutboxMessage> {
    outbox.list("p").into_iter().find(|m| m.id == id)
}

fn until_state(outbox: &Outbox, id: &str, want: State) {
    until(&format!("{id} to be {want:?}"), || state(outbox, id).is_some_and(|m| m.state == want));
}

fn waiting_for(outbox: &Outbox, id: &str, want: &str) {
    until(&format!("{id} to wait for {want:?}"), || {
        state(outbox, id).is_some_and(|m| m.state == State::Queued && m.waiting_for.as_deref() == Some(want))
    });
}

// ---- the decision ----------------------------------------------------------------------------------------------

#[test]
fn only_a_ready_idle_agent_with_an_empty_box_gets_the_message() {
    use ComposerState::*;
    assert_eq!(decide(&look(Readiness::Ready, Empty, true)), Step::Deliver);
    assert_eq!(decide(&look(Readiness::Ready, Unknown, true)), Step::Deliver, "a box the profile cannot read");
    assert_eq!(decide(&look(Readiness::Ready, Empty, false)), Step::Wait(WAIT_BUSY), "status is not idle");
    assert_eq!(decide(&look(Readiness::Ready, HasText, true)), Step::Wait(WAIT_DRAFT));
    assert_eq!(decide(&look(Readiness::Unknown, HasText, true)), Step::Wait(WAIT_DRAFT));
    assert_eq!(decide(&look(Readiness::Unknown, Unknown, true)), Step::Wait(WAIT_BUSY));
    assert_eq!(decide(&busy()), Step::Wait(WAIT_BUSY));
    assert_eq!(decide(&look(Readiness::Gate(PROMPT_GATE), Unknown, true)), Step::Wait(WAIT_GATE));
    let startup = StartupGate { id: "notice", summary: "x", dismiss: Some("\u{1b}") };
    // A notice with its own dismiss keys goes to delivery, which closes it before typing (whatever the status says).
    assert_eq!(decide(&look(Readiness::Gate(startup), Unknown, true)), Step::Deliver);
    assert_eq!(decide(&look(Readiness::Gate(startup), Unknown, false)), Step::Deliver, "status is not read over a notice");
    let only_user = StartupGate { id: "trust", summary: "x", dismiss: None };
    assert_eq!(decide(&look(Readiness::Gate(only_user), Unknown, true)), Step::Wait(WAIT_GATE), "a gate only the user may pass");
    assert_eq!(decide(&look(Readiness::HostShell(HostShell::AgentExited), Unknown, true)), Step::Fail(FAIL_NOT_RUNNING));
    assert_eq!(decide(&look(Readiness::HostShell(HostShell::NotInstalled("x".into())), Unknown, true)), Step::Fail(FAIL_NOT_RUNNING));
}

// ---- the state machine -----------------------------------------------------------------------------------------

#[test]
fn a_message_for_a_ready_agent_is_sent_once() {
    let host = Fake::new(ready());
    let outbox = outbox(&host);
    let message = outbox.send("p", "  hello there \n").unwrap();
    assert_eq!(message.text, "hello there", "the text is trimmed");
    assert_eq!(message.state, State::Queued);
    until_state(&outbox, &message.id, State::Sent);
    assert_eq!(host.typed(), ["hello there"]);
    assert_eq!(host.states_of(&message.id), [State::Queued, State::Sending, State::Sent]);
    let done = state(&outbox, &message.id).unwrap();
    assert_eq!((done.waiting_for, done.error), (None, None));
    // Looking again later does not send it again.
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(host.typed().len(), 1);
}

#[test]
fn a_message_for_a_busy_agent_waits_for_the_turn_to_end() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let message = outbox.send("p", "next please").unwrap();
    waiting_for(&outbox, &message.id, WAIT_BUSY);
    std::thread::sleep(Duration::from_millis(60));
    assert!(host.typed().is_empty(), "nothing is typed while a turn runs");
    // The turn ends: ready, but the pane status has not caught up yet.
    host.set(look(Readiness::Ready, ComposerState::Empty, false));
    std::thread::sleep(Duration::from_millis(40));
    assert!(host.typed().is_empty(), "status must read idle too");
    host.set(ready());
    until_state(&outbox, &message.id, State::Sent);
    assert_eq!(host.typed(), ["next please"]);
}

#[test]
fn a_question_or_dialog_is_waited_for_and_no_key_is_pressed() {
    // The agent's question and a startup screen only the person may pass (folder trust, sign-in).
    for gate in [PROMPT_GATE, StartupGate { id: "workspace_trust", summary: "x", dismiss: None }] {
        let host = Fake::new(look(Readiness::Gate(gate), ComposerState::Unknown, true));
        let outbox = outbox(&host);
        let message = outbox.send("p", "hello").unwrap();
        waiting_for(&outbox, &message.id, WAIT_GATE);
        std::thread::sleep(Duration::from_millis(60));
        assert!(host.typed().is_empty(), "a gate gets no key and no text");
        assert_eq!(*host.calls.lock(), 0, "delivery is not even started: it would look for keys to press");
        host.set(ready());
        until_state(&outbox, &message.id, State::Sent);
        assert_eq!(host.typed(), ["hello"]);
    }
}

#[test]
fn a_startup_notice_with_its_own_keys_is_handed_to_delivery_instead_of_waiting_for_ever() {
    // Codex's "Update available" menu on a companion pane: nobody closed it, so every message sat behind it.
    let notice = StartupGate { id: "update_available", summary: "x", dismiss: Some("\u{1b}") };
    let host = Fake::new(look(Readiness::Gate(notice), ComposerState::Unknown, false));
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    until_state(&outbox, &message.id, State::Sent);
    assert_eq!(host.typed(), ["hello"], "delivery closed the notice and typed once");
}

#[test]
fn a_notice_that_ignores_its_keys_keeps_the_message_waiting_and_is_not_pressed_every_poll() {
    let notice = StartupGate { id: "update_available", summary: "x", dismiss: Some("\u{1b}") };
    let host = Fake::new(look(Readiness::Gate(notice), ComposerState::Unknown, true));
    *host.attempt.lock() = Attempt::Gate;
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    waiting_for(&outbox, &message.id, WAIT_GATE);
    // Polls are 5 ms and the retry gap is 100 ms: once the keys were tried three times, none of the tries came
    // sooner than the gap after the one before (every poll would be 5 ms). Checked on the gaps, not on how many
    // tries fit in a sleep, which depends on how fast the machine runs the worker.
    until("three tries to close the notice", || *host.calls.lock() >= 3);
    let gap = host.shortest_gap().expect("at least two tries");
    assert!(gap >= fast().gate_retry, "the keys were pressed again after {gap:?}");
    assert!(host.typed().is_empty());
    // The person closes it themselves: the next look is Ready and the message goes at once (no retry gap).
    *host.attempt.lock() = Attempt::Sent;
    host.set(ready());
    until_state(&outbox, &message.id, State::Sent);
}

#[test]
fn text_the_person_typed_in_the_box_is_waited_for_and_never_cleared() {
    for readiness in [Readiness::Ready, Readiness::Unknown] {
        let host = Fake::new(look(readiness, ComposerState::HasText, true));
        let outbox = outbox(&host);
        let message = outbox.send("p", "hello").unwrap();
        waiting_for(&outbox, &message.id, WAIT_DRAFT);
        std::thread::sleep(Duration::from_millis(60));
        assert!(host.typed().is_empty(), "nothing is typed over a draft");
        host.set(ready());
        until_state(&outbox, &message.id, State::Sent);
    }
}

#[test]
fn a_draft_found_at_the_last_moment_puts_the_message_back_to_waiting() {
    let host = Fake::new(ready());
    *host.attempt.lock() = Attempt::Draft;
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    waiting_for(&outbox, &message.id, WAIT_DRAFT);
    assert!(host.states_of(&message.id).contains(&State::Sending));
    *host.attempt.lock() = Attempt::Sent;
    until_state(&outbox, &message.id, State::Sent);
}

#[test]
fn a_gate_found_while_typing_puts_the_message_back_to_waiting_for_the_answer() {
    let host = Fake::new(ready());
    *host.attempt.lock() = Attempt::Gate;
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    waiting_for(&outbox, &message.id, WAIT_GATE);
    *host.attempt.lock() = Attempt::Sent;
    until_state(&outbox, &message.id, State::Sent);
}

#[test]
fn an_agent_that_is_not_running_fails_every_waiting_message() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let first = outbox.send("p", "one").unwrap();
    let second = outbox.send("p", "two").unwrap();
    host.set(look(Readiness::HostShell(HostShell::AgentExited), ComposerState::Unknown, true));
    for id in [&first.id, &second.id] {
        until_state(&outbox, id, State::Failed);
        assert_eq!(state(&outbox, id).unwrap().error.as_deref(), Some("The agent is not running in this pane."));
    }
    assert!(host.typed().is_empty());
}

#[test]
fn a_failed_attempt_says_why_in_plain_words() {
    let host = Fake::new(ready());
    *host.attempt.lock() = Attempt::Failed(FAIL_NOT_TAKEN.into());
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    until_state(&outbox, &message.id, State::Failed);
    assert_eq!(state(&outbox, &message.id).unwrap().error.as_deref(), Some(FAIL_NOT_TAKEN));
}

#[test]
fn a_waiting_message_can_be_cancelled_and_is_then_never_sent() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let message = outbox.send("p", "never mind").unwrap();
    waiting_for(&outbox, &message.id, WAIT_BUSY);
    let cancelled = outbox.cancel("p", &message.id).unwrap();
    assert_eq!(cancelled.state, State::Cancelled);
    host.set(ready());
    std::thread::sleep(Duration::from_millis(60));
    assert!(host.typed().is_empty());
    assert_eq!(state(&outbox, &message.id).unwrap().state, State::Cancelled, "it stays listed for a while");
    assert_eq!(outbox.cancel("p", &message.id), Err(NO_SUCH_MESSAGE.to_string()), "twice");
    assert_eq!(outbox.cancel("p", "nope"), Err(NO_SUCH_MESSAGE.to_string()));
    assert_eq!(outbox.cancel("other", &message.id), Err(NO_SUCH_MESSAGE.to_string()));
}

#[test]
fn a_message_being_typed_cannot_be_cancelled() {
    let host = Fake::new(ready());
    *host.hold.lock() = true;
    let outbox = outbox(&host);
    let message = outbox.send("p", "in flight").unwrap();
    until_state(&outbox, &message.id, State::Sending);
    assert_eq!(outbox.cancel("p", &message.id), Err(ALREADY_SENDING.to_string()));
    *host.hold.lock() = false;
    host.released.notify_all();
    until_state(&outbox, &message.id, State::Sent);
    assert_eq!(host.typed(), ["in flight"]);
}

#[test]
fn messages_go_in_the_order_they_were_written_one_per_turn() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let ids: Vec<String> = ["a", "b", "c"].iter().map(|text| outbox.send("p", text).unwrap().id).collect();
    waiting_for(&outbox, &ids[0], WAIT_BUSY);
    waiting_for(&outbox, &ids[1], WAIT_TURN);
    waiting_for(&outbox, &ids[2], WAIT_TURN);
    host.set(ready());
    until_state(&outbox, &ids[2], State::Sent);
    assert_eq!(host.typed(), ["a", "b", "c"]);
}

#[test]
fn the_next_message_waits_a_moment_after_one_went_in() {
    let host = Fake::new(ready());
    let outbox = Outbox::with_timing(host.clone(), Timing { after_send: Duration::from_millis(150), ..fast() });
    let first = outbox.send("p", "a").unwrap();
    let second = outbox.send("p", "b").unwrap();
    until_state(&outbox, &first.id, State::Sent);
    until_state(&outbox, &second.id, State::Sent);
    assert_eq!(host.typed(), ["a", "b"]);
    // The pane read ready the whole time, but a turn was just starting: the second went in no sooner than
    // `after_send` after the first one was in (measured on the host's own clock, so a slow worker cannot fake it).
    let gap = host.shortest_gap().expect("two messages were typed");
    assert!(gap >= Duration::from_millis(150), "the second message followed the first after {gap:?}");
}

#[test]
fn five_messages_may_wait_in_a_pane_and_the_sixth_is_refused() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let ids: Vec<String> = (0..5).map(|n| outbox.send("p", &format!("m{n}")).unwrap().id).collect();
    assert_eq!(outbox.send("p", "one too many"), Err("Too many waiting messages.".to_string()));
    // A cancelled one frees its place; another pane has its own five.
    outbox.cancel("p", &ids[0]).unwrap();
    outbox.send("p", "fits now").unwrap();
    assert!(outbox.send("q", "other pane").is_ok());
}

#[test]
fn text_must_be_there_and_at_most_8000_characters() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    assert_eq!(outbox.send("p", ""), Err(EMPTY.to_string()));
    assert_eq!(outbox.send("p", "  \n\t "), Err(EMPTY.to_string()));
    assert!(outbox.send("p", &"x".repeat(8000)).is_ok());
    assert_eq!(outbox.send("p", &"x".repeat(8001)), Err(TOO_LONG.to_string()));
    // Characters, not bytes.
    assert!(outbox.send("p", &"\u{e9}".repeat(8000)).is_ok());
}

#[test]
fn a_shell_a_protocol_agent_a_missing_pane_and_a_run_are_refused_when_sending() {
    let host = Fake::new(ready());
    let outbox = outbox(&host);
    *host.kind.lock() = Some(PaneKind::Shell);
    assert_eq!(outbox.send("p", "ls"), Err("Chat replies need an agent pane.".to_string()));
    *host.kind.lock() = Some(PaneKind::Protocol);
    assert_eq!(outbox.send("p", "hi"), Err(NOT_SUPPORTED.to_string()));
    *host.kind.lock() = None;
    assert_eq!(outbox.send("p", "hi"), Err(NO_SUCH_PANE.to_string()));
    *host.kind.lock() = Some(PaneKind::Agent);
    *host.run.lock() = true;
    assert_eq!(
        outbox.send("p", "hi"),
        Err("An agent run controls this pane. Use Terminal, or wait until the run ends.".to_string())
    );
    assert!(host.typed().is_empty() && outbox.list("p").is_empty());
}

#[test]
fn a_run_that_starts_while_a_message_waits_is_not_typed_over() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    waiting_for(&outbox, &message.id, WAIT_BUSY);
    *host.run.lock() = true;
    host.set(ready());
    waiting_for(&outbox, &message.id, WAIT_RUN);
    assert!(host.typed().is_empty());
    *host.run.lock() = false;
    until_state(&outbox, &message.id, State::Sent);
}

#[test]
fn a_pane_that_is_closed_drops_its_waiting_messages() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let first = outbox.send("p", "one").unwrap();
    let second = outbox.send("p", "two").unwrap();
    waiting_for(&outbox, &first.id, WAIT_BUSY);
    *host.look.lock() = None;
    *host.kind.lock() = None;
    for id in [&first.id, &second.id] {
        until_state(&outbox, id, State::Failed);
        assert_eq!(state(&outbox, id).unwrap().error.as_deref(), Some("The pane was closed."));
    }
    assert!(host.typed().is_empty());
}

#[test]
fn a_message_that_waited_too_long_fails() {
    let host = Fake::new(busy());
    let outbox = Outbox::with_timing(host.clone(), Timing { give_up: Duration::from_millis(80), ..fast() });
    let message = outbox.send("p", "late").unwrap();
    until_state(&outbox, &message.id, State::Failed);
    assert_eq!(state(&outbox, &message.id).unwrap().error.as_deref(), Some("Waited too long. Not sent."));
}

#[test]
fn finished_messages_stay_listed_for_a_while_then_go_and_the_worker_stops() {
    let host = Fake::new(ready());
    let outbox = Outbox::with_timing(host.clone(), Timing { keep: Duration::from_millis(120), ..fast() });
    let message = outbox.send("p", "hello").unwrap();
    until_state(&outbox, &message.id, State::Sent);
    assert!(state(&outbox, &message.id).is_some(), "still listed");
    until("the sent message to leave the list", || outbox.list("p").is_empty());
    // The pane's queue is gone, so its worker has ended; a new message starts a new one.
    until("the worker to end", || !outbox.inner.panes.lock().contains_key("p"));
    let again = outbox.send("p", "again").unwrap();
    until_state(&outbox, &again.id, State::Sent);
    assert_eq!(host.typed(), ["hello", "again"]);
    // The last list the windows were told about is empty once everything is gone.
    until("the empty list to be published", || host.published.lock().last().is_some_and(|list| list.is_empty()));
}

#[test]
fn every_change_is_published_with_the_whole_list() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    waiting_for(&outbox, &message.id, WAIT_BUSY);
    let lists = host.published.lock().clone();
    assert!(lists.first().is_some_and(|list| list.len() == 1 && list[0].state == State::Queued));
    assert!(lists.iter().any(|list| list[0].waiting_for.as_deref() == Some(WAIT_BUSY)));
}

#[test]
fn the_json_has_the_names_the_phone_and_the_window_read() {
    let host = Fake::new(busy());
    let outbox = outbox(&host);
    let message = outbox.send("p", "hello").unwrap();
    waiting_for(&outbox, &message.id, WAIT_BUSY);
    let json = serde_json::to_value(state(&outbox, &message.id).unwrap()).unwrap();
    let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["createdAt", "error", "id", "state", "text", "waitingFor"]);
    assert_eq!(json["state"], "queued");
    assert_eq!(json["waitingFor"], WAIT_BUSY);
    assert!(json["error"].is_null());
    assert!(json["createdAt"].is_u64());
}

// ---- failures stay listed; the "To:" list refuses panes that are not at their input box ------------------------

#[test]
fn a_failed_message_stays_listed_longer_than_a_sent_one() {
    let host = Fake::new(ready());
    *host.attempt.lock() = Attempt::Failed(FAIL_STUCK.into());
    let timing = Timing { keep: Duration::from_millis(60), keep_failed: Duration::from_secs(30), ..fast() };
    let outbox = Outbox::with_timing(host.clone(), timing);
    let failed = outbox.send("p", "half typed").unwrap();
    until_state(&outbox, &failed.id, State::Failed);
    assert_eq!(state(&outbox, &failed.id).unwrap().error.as_deref(), Some(FAIL_STUCK));
    std::thread::sleep(Duration::from_millis(300));
    assert!(state(&outbox, &failed.id).is_some(), "the failure is still there after the time a sent message would have left");
}

#[test]
fn a_pane_on_a_dialog_or_without_its_input_box_is_not_a_target() {
    let ask = look(Readiness::Gate(StartupGate { id: "update_available", summary: "offers an update", dismiss: Some("\u{1b}") }), ComposerState::Unknown, false);
    assert_eq!(target_block(&ask), Some(WAIT_GATE), "a notice with dismiss keys too: it is not Point's to close");
    let prompt = look(Readiness::Gate(PROMPT_GATE), ComposerState::Unknown, false);
    assert_eq!(target_block(&prompt), Some(WAIT_GATE));
    let no_box = look(Readiness::Unknown, ComposerState::Unknown, false);
    assert_eq!(target_block(&no_box), Some(NOT_READY_BOX));
    let gone = look(Readiness::HostShell(HostShell::AgentExited), ComposerState::Unknown, false);
    assert_eq!(target_block(&gone), Some(FAIL_NOT_RUNNING));
}

#[test]
fn a_working_agent_and_a_draft_are_still_targets_because_the_outbox_waits_for_them() {
    assert_eq!(target_block(&ready()), None);
    assert_eq!(target_block(&busy()), None);
    let draft = look(Readiness::Unknown, ComposerState::HasText, true);
    assert_eq!(target_block(&draft), None);
    let draft_ready = look(Readiness::Ready, ComposerState::HasText, true);
    assert_eq!(target_block(&draft_ready), None);
}

// ---- how a delivery's outcome reads ----------------------------------------------------------------------------

#[test]
fn a_delivery_outcome_becomes_one_of_six_attempts() {
    let gate = PROMPT_GATE;
    assert_eq!(attempt_of(Delivery::Started, true), Attempt::Sent);
    assert_eq!(attempt_of(Delivery::SubmittedUnconfirmed, true), Attempt::Sent);
    assert_eq!(attempt_of(Delivery::Blocked(gate), false), Attempt::Gate);
    assert_eq!(attempt_of(Delivery::Blocked(gate), true), Attempt::Failed(FAIL_IN_BOX.into()), "text may be in the box");
    assert_eq!(attempt_of(Delivery::NotReady { why: String::new(), screen: String::new() }, false), Attempt::NotTyped);
    assert_eq!(attempt_of(Delivery::NotReady { why: String::new(), screen: String::new() }, true), Attempt::Failed(FAIL_IN_BOX.into()));
    assert_eq!(attempt_of(Delivery::AgentGone(HostShell::AgentExited), false), Attempt::Gone);
    assert_eq!(attempt_of(Delivery::NotVisible { cleared: true }, true), Attempt::Failed(FAIL_NOT_TAKEN.into()));
    assert_eq!(attempt_of(Delivery::NotVisible { cleared: false }, true), Attempt::Failed(FAIL_IN_BOX.into()));
    assert_eq!(attempt_of(Delivery::WriteFailed("PERMIT_STALE: x".into()), false), Attempt::Failed(FAIL_WRITE.into()));
}

// ---- the real delivery code against a scripted pane ------------------------------------------------------------

mod scripted {
    use super::*;
    use crate::agent_runs::delivery::DeliveryIo;
    use crate::agent_runs::readiness::AgentProcess;
    use crate::pane_outbox::draft_guard::DraftGuard;
    use crate::pty::WriteOrigin;
    use crate::tui::TuiProfile;

    /// A made-up agent: its box is the row `> ...`; `clearable` says whether it has a key that empties the box.
    struct Boxed {
        verifies: bool,
        clearable: bool,
    }

    impl TuiProfile for Boxed {
        fn id(&self) -> &'static str {
            "boxed"
        }
        fn verifies_prompt_delivery(&self) -> bool {
            self.verifies
        }
        fn clear_input_sequence(&self) -> Option<&'static str> {
            self.clearable.then_some("\u{15}")
        }
        fn ready_recogniser(&self, screen: &str) -> Option<ComposerState> {
            if screen.contains("ctrl+c to stop") {
                return Some(ComposerState::Working);
            }
            let row = screen.lines().find(|line| line.starts_with('>'))?;
            Some(if row.trim() == ">" { ComposerState::Empty } else { ComposerState::HasText })
        }
        fn composer_state(&self, screen: &str) -> ComposerState {
            self.ready_recogniser(screen).unwrap_or(ComposerState::Unknown)
        }
    }

    struct Pane {
        profile: Arc<Boxed>,
        screen: String,
        writes: Vec<(String, bool)>,
    }

    impl Pane {
        fn new(screen: &str, verifies: bool, clearable: bool) -> Self {
            Self { profile: Arc::new(Boxed { verifies, clearable }), screen: screen.into(), writes: Vec::new() }
        }
    }

    impl DeliveryIo for Pane {
        fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
            assert!(matches!(origin, WriteOrigin::Task(_)), "the outbox types only under a typing permit");
            origin.admit_terminal_write("", text, enter, None, || AgentProcess::Unknown)?;
            self.writes.push((text.to_string(), enter));
            if text == "\u{15}" {
                self.screen = "agent\n>".into();
            } else if !text.is_empty() {
                self.screen = format!("agent\n> {}", text.replace('\n', "\n  "));
            }
            if enter {
                self.screen = "agent\n  thinking... ctrl+c to stop\n>".into();
            }
            Ok(())
        }
        fn screen(&mut self) -> String {
            self.screen.clone()
        }
        fn busy(&mut self) -> bool {
            self.writes.iter().any(|(_, enter)| *enter)
        }
        fn pause(&mut self, _duration: Duration) {}
        fn profile(&mut self) -> Arc<dyn TuiProfile> {
            self.profile.clone()
        }
    }

    fn run(pane: &mut Pane, text: &str) -> (Attempt, Vec<String>) {
        let profile = pane.profile.clone();
        let mut trace = Vec::new();
        let outcome = {
            let mut guard = DraftGuard::new(pane);
            delivery::deliver_text(&mut guard, profile.as_ref(), None, text, &mut trace)
        };
        let typed = trace.iter().any(|line| line.contains(delivery::TYPING_TRACE));
        (attempt_of(outcome, typed), trace)
    }

    // ---- the real Codex profile behind its update menu (live captures) ----

    const MENU: &str = include_str!("../tui/codex/fixtures/update_prompt_0_164_live.txt");
    const CODEX_READY: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");
    const CODEX_UNSENT: &str = include_str!("../tui/codex/fixtures/composer_unsent_glued_live.txt");
    const CODEX_SUBMITTED: &str = include_str!("../tui/codex/fixtures/submitted_live.txt");
    /// The text those captures show in the box and the transcript.
    const CODEX_TEXT: &str = "Reply with the single word FIVE. Do not use any tools.";

    /// A Codex pane that starts at its update menu. `esc_closes`: whether Esc closes it (it does on the real one).
    struct CodexMenu {
        screen: String,
        esc_closes: bool,
        /// (what, enter) for every write, whatever its permit.
        writes: Vec<(String, bool)>,
        /// Writes made while the menu was still up that were not Esc: the bug (a message typed into the menu).
        into_menu: Vec<String>,
    }

    impl CodexMenu {
        fn new(esc_closes: bool) -> Self {
            Self { screen: MENU.into(), esc_closes, writes: Vec::new(), into_menu: Vec::new() }
        }
        fn menu_up(&self) -> bool {
            self.screen == MENU
        }
    }

    impl DeliveryIo for CodexMenu {
        fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
            origin.admit_terminal_write("", text, enter, None, || AgentProcess::Unknown)?;
            self.writes.push((text.to_string(), enter));
            if self.menu_up() {
                if text == "\u{1b}" && !enter {
                    if self.esc_closes {
                        self.screen = CODEX_READY.into();
                    }
                } else {
                    self.into_menu.push(format!("{text:?} enter={enter}"));
                }
            } else if enter {
                self.screen = CODEX_SUBMITTED.into();
            } else if !text.is_empty() {
                self.screen = CODEX_UNSENT.into();
            }
            Ok(())
        }
        fn screen(&mut self) -> String {
            self.screen.clone()
        }
        fn busy(&mut self) -> bool {
            self.writes.iter().any(|(_, enter)| *enter)
        }
        fn pause(&mut self, _duration: Duration) {}
        fn profile(&mut self) -> Arc<dyn TuiProfile> {
            crate::tui::profile_for("codex")
        }
    }

    fn run_codex(pane: &mut CodexMenu) -> Attempt {
        let profile = crate::tui::profile_for("codex");
        let mut trace = Vec::new();
        let outcome = {
            let mut guard = DraftGuard::new(pane);
            delivery::deliver_text(&mut guard, profile.as_ref(), None, CODEX_TEXT, &mut trace)
        };
        let typed = trace.iter().any(|line| line.contains(delivery::TYPING_TRACE));
        attempt_of(outcome, typed)
    }

    /// The pane outbox (a companion's check-in, a phone message) and a run's task go through one delivery step:
    /// the update menu is closed with its own Esc, then the message is typed. Never Enter, never into the menu.
    #[test]
    fn the_update_menu_is_closed_with_esc_before_a_message_is_typed() {
        let mut pane = CodexMenu::new(true);
        assert_eq!(run_codex(&mut pane), Attempt::Sent);
        assert!(pane.into_menu.is_empty(), "typed into the menu: {:?}", pane.into_menu);
        assert_eq!(pane.writes.first(), Some(&("\u{1b}".to_string(), false)), "Esc first: {:?}", pane.writes);
        assert_eq!(pane.writes.iter().filter(|(text, _)| text == "\u{1b}").count(), 1);
        let text_at = pane.writes.iter().position(|(text, _)| text == CODEX_TEXT).expect("the message was typed");
        assert!(text_at > 0, "the message came after the Esc: {:?}", pane.writes);
    }

    #[test]
    fn a_menu_that_ignores_esc_gets_nothing_typed_and_the_message_waits() {
        let mut pane = CodexMenu::new(false);
        assert_eq!(run_codex(&mut pane), Attempt::Gate);
        assert!(pane.into_menu.is_empty(), "typed into the menu: {:?}", pane.into_menu);
        assert!(pane.writes.iter().all(|(text, enter)| text == "\u{1b}" && !enter), "only Esc went in: {:?}", pane.writes);
        assert!(pane.writes.len() <= 1, "one Esc, not a storm: {:?}", pane.writes);
    }

    #[test]
    fn text_in_the_box_is_never_typed_over_and_the_clear_key_is_never_sent() {
        for verifies in [false, true] {
            // This agent could clear its box (a run would); the outbox must not.
            let mut pane = Pane::new("agent\n> half a sentence I was typing", verifies, true);
            let (attempt, _) = run(&mut pane, "my message");
            assert_eq!(attempt, Attempt::Draft, "verifies={verifies}");
            assert!(pane.writes.is_empty(), "verifies={verifies}: nothing was written, not even a clear: {:?}", pane.writes);
            assert!(pane.screen.contains("half a sentence I was typing"));
        }
    }

    #[test]
    fn a_box_that_cannot_be_cleared_is_waited_for_without_a_key() {
        let mut pane = Pane::new("agent\n> half a sentence I was typing", false, false);
        let (attempt, _) = run(&mut pane, "my message");
        assert_eq!(attempt, Attempt::NotTyped);
        assert!(pane.writes.is_empty(), "{:?}", pane.writes);
    }

    #[test]
    fn an_empty_box_gets_the_message_and_the_enter_but_no_clear() {
        let mut pane = Pane::new("agent\n>", false, true);
        let (attempt, _) = run(&mut pane, "my message");
        assert_eq!(attempt, Attempt::Sent);
        assert_eq!(pane.writes, [("my message".to_string(), true)], "text with its Enter, no clear key");
    }

    #[test]
    fn a_two_line_message_is_typed_exactly_like_a_runs_task_text() {
        let text = "first line\nsecond line";
        let mut pane = Pane::new("agent\n>", true, true);
        let (attempt, _) = run(&mut pane, text);
        assert_eq!(attempt, Attempt::Sent);
        // The text is written as it is, line break included (the same as `run_agent`'s task text), then Enter.
        assert_eq!(pane.writes.first(), Some(&(text.to_string(), false)), "{:?}", pane.writes);
        assert!(pane.writes.iter().all(|(written, _)| written != "\u{15}"), "{:?}", pane.writes);
        assert_eq!(pane.writes.iter().filter(|(_, enter)| *enter).count(), 1, "one Enter");
    }

    #[test]
    fn a_draft_that_appears_after_the_look_is_caught_at_the_keystroke() {
        // The person starts typing between delivery's look and the write: the guard looks again.
        struct Racing(Pane, bool);
        impl DeliveryIo for Racing {
            fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
                self.0.write(origin, text, enter)
            }
            fn screen(&mut self) -> String {
                self.0.screen()
            }
            fn observe(&mut self) -> crate::agent_runs::readiness::Observation {
                // After the first look the person has typed a letter.
                if self.1 {
                    self.0.screen = "agent\n> x".into();
                }
                self.1 = true;
                crate::agent_runs::readiness::Observation::screen_only(self.0.screen.clone())
            }
            fn busy(&mut self) -> bool {
                self.0.busy()
            }
            fn pause(&mut self, _duration: Duration) {}
            fn profile(&mut self) -> Arc<dyn TuiProfile> {
                self.0.profile()
            }
        }
        let mut racing = Racing(Pane::new("agent\n>", false, true), false);
        let profile = racing.0.profile.clone();
        let outcome = {
            let mut guard = DraftGuard::new(&mut racing);
            delivery::deliver_text(&mut guard, profile.as_ref(), None, "my message", &mut Vec::new())
        };
        assert_eq!(attempt_of(outcome, false), Attempt::Draft);
        assert!(racing.0.writes.is_empty(), "{:?}", racing.0.writes);
    }
}
