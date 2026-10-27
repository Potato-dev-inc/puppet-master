//! The Chat tab's message box (`pane_outbox`) on a real ConPTY with `pm-fake-tui` as the agent. Zero tokens.
//!
//! A message sent while the agent works waits and goes in after the turn, exactly once; text the person typed in
//! the terminal box is never typed over or cleared; a question the agent asks is not answered by a message; a
//! two-line message reaches the agent as one message.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use puppet_master_app_lib::pane_outbox::{Outbox, OutboxMessage, State};
use serde_json::Value;
use std::time::Duration;

const WAIT_BUSY: &str = "The agent is working. Sends when the turn ends.";
const WAIT_GATE: &str = "The agent asks something. Answer it first.";
const WAIT_DRAFT: &str = "The terminal box has text. Send or clear it there first.";

fn prompts(session: &Session) -> Vec<String> {
    session
        .events()
        .iter()
        .filter(|event| event["ev"] == "prompt")
        .filter_map(|event| event["text"].as_str().map(str::to_string))
        .collect()
}

fn find(outbox: &Outbox, pane: &str, id: &str) -> OutboxMessage {
    outbox.list(pane).into_iter().find(|message| message.id == id).unwrap_or_else(|| panic!("message {id} is not listed"))
}

fn until_state(session: &Session, outbox: &Outbox, pane: &str, id: &str, want: State) -> OutboxMessage {
    session
        .until(60, || Some(find(outbox, pane, id)).filter(|message| message.state == want))
        .unwrap_or_else(|| panic!("{id} never became {want:?}: {:?}\n-- screen:\n{}", find(outbox, pane, id), session.screen(pane)))
}

fn until_waiting(session: &Session, outbox: &Outbox, pane: &str, id: &str, why: &str) {
    let found = session.until(30, || {
        let message = find(outbox, pane, id);
        (message.state == State::Queued && message.waiting_for.as_deref() == Some(why)).then_some(())
    });
    assert!(found.is_some(), "{id} never waited for {why:?}: {:?}\n-- screen:\n{}", find(outbox, pane, id), session.screen(pane));
}

fn ready_pane(session: &Session) -> String {
    let pane = session.spawn_pane("fake_claude").expect("a pane");
    session.until(30, || session.screen(&pane).contains('\u{276f}').then_some(())).expect("the agent drew its input box");
    pane
}

fn turn_ends(session: &Session, n: usize) -> Option<()> {
    (session.events().iter().filter(|event| event["ev"] == "turn_done").count() >= n).then_some(())
}

#[test]
fn a_message_sent_while_the_agent_works_arrives_after_the_turn_exactly_once() {
    let session = World::get().session("chat-reply-busy");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
on_prompt = "first"
steps = [
  { do = "think", ms = 3000 },
  { do = "say", text = "First done." },
]
[[turn]]
on_prompt = "second"
steps = [
  { do = "say", text = "Second done." },
]
"#,
    );
    let pane = ready_pane(&session);
    let outbox = session.outbox();

    // The agent is free: the first message goes in at once.
    let first = outbox.send(&pane, "first message please").expect("accepted");
    until_state(&session, &outbox, &pane, &first.id, State::Sent);
    session.until(30, || (!prompts(&session).is_empty()).then_some(())).expect("the agent got the first message");

    // The turn is running: the second message waits, and nothing is typed while it does.
    let second = outbox.send(&pane, "second message please").expect("accepted");
    until_waiting(&session, &outbox, &pane, &second.id, WAIT_BUSY);
    std::thread::sleep(Duration::from_millis(1200));
    let waiting = find(&outbox, &pane, &second.id);
    assert_eq!(waiting.state, State::Queued, "{waiting:?}");
    assert_eq!(prompts(&session).len(), 1, "nothing was typed while the turn ran: {:?}", prompts(&session));

    // The turn ends, and the second message follows, once.
    until_state(&session, &outbox, &pane, &second.id, State::Sent);
    session.until(60, || turn_ends(&session, 2)).unwrap_or_else(|| panic!("the second turn never ended\n{}", session.screen(&pane)));
    std::thread::sleep(Duration::from_millis(1500));
    let seen = prompts(&session);
    assert_eq!(seen.len(), 2, "each message reached the agent exactly once: {seen:?}");
    assert!(seen[0].contains("first message please") && seen[1].contains("second message please"), "{seen:?}");
    assert_eq!(seen.iter().filter(|text| text.contains("second message please")).count(), 1);
    // The screen shows the agent answering the second message.
    assert!(session.history(&pane).contains("Second done."), "{}", session.history(&pane));
}

#[test]
fn a_two_line_message_reaches_the_agent_as_one_message_with_both_lines() {
    let session = World::get().session("chat-reply-two-lines");
    // A real Claude reads text and Enters that arrive in one burst as a paste: the Enters are line breaks.
    session.script(
        r#"
agent = "claude"
boot_ms = 200
paste_burst_ms = 40
paste_burst_effect = "newline"
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "say", text = "Got both lines." },
]
"#,
    );
    let pane = ready_pane(&session);
    let outbox = session.outbox();
    let message = outbox.send(&pane, "alpha line\nbeta line").expect("accepted");
    until_state(&session, &outbox, &pane, &message.id, State::Sent);
    session.until(60, || turn_ends(&session, 1)).unwrap_or_else(|| panic!("the turn never ended\n{}", session.screen(&pane)));
    let seen = prompts(&session);
    assert_eq!(seen, ["alpha line\nbeta line"], "one message, both lines, in order: {seen:?}");
}

#[test]
fn text_the_person_typed_in_the_terminal_box_is_not_typed_over_or_cleared() {
    let session = World::get().session("chat-reply-draft");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "say", text = "Done." },
]
"#,
    );
    let pane = ready_pane(&session);
    let outbox = session.outbox();
    // The person starts a sentence in the terminal box and does not send it.
    session.type_as_user(&pane, "my own half sentence", false).expect("typed");
    session.until(15, || session.screen(&pane).contains("my own half sentence").then_some(())).expect("the draft shows");

    let message = outbox.send(&pane, "message from the chat").expect("accepted");
    until_waiting(&session, &outbox, &pane, &message.id, WAIT_DRAFT);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(find(&outbox, &pane, &message.id).state, State::Queued);
    assert!(prompts(&session).is_empty(), "nothing was submitted: {:?}", prompts(&session));
    assert!(session.screen(&pane).contains("my own half sentence"), "the draft is still in the box:\n{}", session.screen(&pane));

    // The person clears their draft (Ctrl+U); the message goes in on its own, without the draft.
    session.type_as_user(&pane, "\u{15}", false).expect("cleared");
    until_state(&session, &outbox, &pane, &message.id, State::Sent);
    session.until(60, || turn_ends(&session, 1)).unwrap_or_else(|| panic!("the turn never ended\n{}", session.screen(&pane)));
    let seen = prompts(&session);
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].contains("message from the chat") && !seen[0].contains("half sentence"), "{seen:?}");
}

#[test]
fn a_question_the_agent_asks_is_not_answered_by_a_message() {
    let session = World::get().session("chat-reply-gate");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
on_prompt = "run"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "The tests pass." }], on_deny = [{ do = "say", text = "Stopped." }] },
]
[[turn]]
on_prompt = "after"
steps = [
  { do = "say", text = "Got the later message." },
]
"#,
    );
    let pane = ready_pane(&session);
    let outbox = session.outbox();
    session.type_as_user(&pane, "run the tests", true).expect("typed");
    let card = session.until(40, || session.card(&pane)).unwrap_or_else(|| panic!("no question appeared\n{}", session.screen(&pane)));
    let prompt_id = card["prompt_id"].as_str().expect("a prompt id").to_string();

    let message = outbox.send(&pane, "after you are done").expect("accepted");
    until_waiting(&session, &outbox, &pane, &message.id, WAIT_GATE);
    std::thread::sleep(Duration::from_millis(1500));
    let events: Vec<Value> = session.events();
    assert!(events.iter().all(|event| event["ev"] != "approval_answer"), "a message answered the question: {events:?}");
    assert_eq!(find(&outbox, &pane, &message.id).state, State::Queued);
    assert_eq!(prompts(&session).len(), 1, "only the person's own line was typed: {:?}", prompts(&session));
    assert!(session.card(&pane).is_some(), "the question is still up");

    // The person answers it; the turn ends and the message follows.
    session.answer_card(&pane, &prompt_id, "allow_once").unwrap_or_else(|e| panic!("answer refused: {e}"));
    until_state(&session, &outbox, &pane, &message.id, State::Sent);
    session.until(60, || turn_ends(&session, 2)).unwrap_or_else(|| panic!("the second turn never ended\n{}", session.screen(&pane)));
    let seen = prompts(&session);
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen[1].contains("after you are done"), "{seen:?}");
}

#[test]
fn a_pane_that_is_closed_fails_its_waiting_message() {
    let session = World::get().session("chat-reply-closed");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 20000 },
  { do = "say", text = "Never." },
]
"#,
    );
    let pane = ready_pane(&session);
    let outbox = session.outbox();
    session.type_as_user(&pane, "take your time", true).expect("typed");
    session.until(30, || (session.pane_status(&pane).as_deref() == Some("running")).then_some(())).expect("the agent is working");
    let message = outbox.send(&pane, "too late").expect("accepted");
    until_waiting(&session, &outbox, &pane, &message.id, WAIT_BUSY);
    session.close_pane(&pane);
    let failed = until_state(&session, &outbox, &pane, &message.id, State::Failed);
    assert_eq!(failed.error.as_deref(), Some("The pane was closed."), "{failed:?}");
}
