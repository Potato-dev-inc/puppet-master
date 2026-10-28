//! The safety net for the night of 2026-10-28 (zero tokens, `pm-fake-tui` as the agent on a real ConPTY).
//!
//! 1. A message from the companion (a check-in) or the Chat tab never types into a startup screen only the person
//!    may pass: a trust dialog gets zero bytes, the card is up for the person, and the message goes in once after
//!    the person answered.
//! 2. Two panes in one folder keep two chats (read off the screen).
//! 3. Two helpers under one parent: an answer to a card that has moved on is refused as stale, and the other
//!    helper's question is untouched.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use puppet_master_app_lib::pane_outbox::{Outbox, OutboxMessage, State};
use serde_json::{json, Value};
use std::time::Duration;

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|event| event["ev"] == name).collect()
}

fn prompts(session: &Session) -> Vec<String> {
    events_named(session, "prompt").iter().filter_map(|event| event["text"].as_str().map(str::to_string)).collect()
}

fn find(outbox: &Outbox, pane: &str, id: &str) -> OutboxMessage {
    outbox.list(pane).into_iter().find(|message| message.id == id).unwrap_or_else(|| panic!("message {id} is not listed"))
}

// ---- 1. a message never types into the trust dialog ---------------------------------------------------------

const CHECKIN: &str = "[Check-in: Morning brief] What is on today?";

/// The companion's check-in goes through the pane outbox (`companion::live::send`), and so does a Chat tab message.
/// A folder-trust dialog is on screen: not one character, Enter or arrow reaches the pane (Enter on the dialog
/// would trust the folder for the person). The card for the person is up. After the person trusts, the
/// message goes in exactly once.
#[test]
fn a_check_in_for_a_pane_on_the_trust_dialog_writes_nothing_and_goes_in_once_after_the_person_trusts() {
    let session = World::get().session("safety-trust-outbox");
    session.script(
        r#"
agent = "claude"
boot_ms = 100
[[startup]]
gate = "trust"
after_ms = 100
[[turn]]
steps = [{ do = "think", ms = 300 }, { do = "say", text = "Good morning." }]
"#,
    );
    let pane = session.spawn_pane("fake_claude").expect("a pane");
    session.until(30, || (!events_named(&session, "gate_shown").is_empty()).then_some(())).expect("the trust dialog was drawn");
    let outbox = session.outbox();
    let message = outbox.send(&pane, CHECKIN).expect("accepted: the outbox queues, it does not type");

    // It waits, and for the whole time the dialog is up nothing is written to the pane.
    let waits = session.until(30, || {
        let now = find(&outbox, &pane, &message.id);
        (now.state == State::Queued && now.waiting_for.is_some()).then_some(now)
    });
    assert!(waits.is_some(), "the message never reported that it waits: {:?}\n{}", find(&outbox, &pane, &message.id), session.screen(&pane));
    std::thread::sleep(Duration::from_millis(2500));
    let waiting = find(&outbox, &pane, &message.id);
    assert_eq!((waiting.state, waiting.error.as_deref()), (State::Queued, None), "{waiting:?}");
    for ev in ["prompt", "key", "typed_at_gate", "gate_answer"] {
        assert!(events_named(&session, ev).is_empty(), "the pane saw `{ev}` while the trust dialog was up: {:#?}", session.events());
    }
    let screen = session.screen(&pane);
    assert!(!screen.contains("Check-in") && !screen.contains("Morning brief"), "the message is on the dialog:\n{screen}");

    // The card is raised for the person (the companion never answers it).
    let card = session.card(&pane).expect("the trust card for the pane");
    assert_eq!(card["kind"], "workspace_trust", "{card}");

    // The person trusts the folder; only then does the message go in, once.
    let prompt_id = card["prompt_id"].as_str().expect("a prompt id").to_string();
    session.answer_card(&pane, &prompt_id, "trust").unwrap_or_else(|e| panic!("answer refused: {e}"));
    session
        .until(60, || (find(&outbox, &pane, &message.id).state == State::Sent).then_some(()))
        .unwrap_or_else(|| panic!("never sent after trust: {:?}\n{}", find(&outbox, &pane, &message.id), session.screen(&pane)));
    session.until(60, || (!events_named(&session, "turn_done").is_empty()).then_some(())).expect("the turn ended");
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(prompts(&session), [CHECKIN], "exactly one copy reached the agent: {:#?}", session.events());
    assert_eq!(events_named(&session, "gate_answer").len(), 1, "only the person's answer reached the dialog");
    assert!(events_named(&session, "typed_at_gate").is_empty());
}

/// The probe the test above relies on: text typed at the dialog is logged by the fake (`typed_at_gate`), so
/// "no such event" really means "nothing was typed".
#[test]
fn the_fake_logs_text_typed_at_a_trust_dialog_so_a_silent_log_means_nothing_was_typed() {
    let session = World::get().session("safety-trust-probe");
    session.script(
        r#"
agent = "claude"
boot_ms = 100
[[startup]]
gate = "trust"
after_ms = 100
"#,
    );
    let pane = session.spawn_pane("fake_claude").expect("a pane");
    session.until(30, || (!events_named(&session, "gate_shown").is_empty()).then_some(())).expect("the trust dialog was drawn");
    session.type_as_user(&pane, "hello", false).expect("typed");
    session.until(15, || (!events_named(&session, "typed_at_gate").is_empty()).then_some(())).expect("the fake logged the typing at the dialog");
}

// ---- 2. two panes in one folder keep two chats ---------------------------------------------------------------

const CHAT_AGENT: &str = r##"
schema_version = 1

[agent]
id = "chat"
label = "Chat"
experimental = true

[binary]
names = ["chat"]

[tui]
argv = []
idle_after_ms = 1500
quiet_idle_after_ms = 800

[tui.busy]
working_regex = ['ctrl\+c to stop']
idle_regex = ['\|\s*>\s*type a task']

[tui.chrome]
line_regex = [
  '^nimbus\s+[\d.]+',
  '^[+|\-\s]+$',
  '^\|\s*>\s*type a task',
  '^/help for commands',
  '^model:\s',
  '^done in \d+s$',
  '^\W?\s*generating\.\.\.',
]

[tui.thinking]
line_regex = ['^[~*]\s*(thinking|generating)', '^thought for \d+s']

[tui.tool_activity]
line_regex = ['^tool:\s']

[tui.delivery]
input_echo_regex = '^\|\s*>\s*\S'
transcript_echo_regex = '^>\s+\S'
"##;

/// Two panes of the same screen-read agent in the same folder, each told something different: each Chat shows
/// only the words of its own pane (the screen of a pane is the pane's, whatever the folder holds).
#[test]
fn two_screen_read_panes_in_one_folder_each_show_only_their_own_messages() {
    let session = World::get().session("safety-two-screen-chats");
    session.screen_agent("fake_chat", CHAT_AGENT);
    session.script(
        r#"
agent = "nimbus"
boot_ms = 100
[[turn]]
steps = [
  { do = "think", ms = 500 },
  { do = "say", text = "Noted." },
]
"#,
    );
    let a = session.spawn_pane("fake_chat").expect("pane A");
    let b = session.spawn_pane("fake_chat").expect("pane B in the same folder");
    for pane in [&a, &b] {
        session.until(30, || session.screen(pane).contains("Type a task").then_some(())).expect("the agent drew its input box");
    }
    let (chat_a, chat_b) = (session.open_screen_chat(&a, "fake_chat"), session.open_screen_chat(&b, "fake_chat"));

    session.type_as_user(&a, "alpha topic only for pane A", true).expect("typed in A");
    session.until(60, || chat_a.ids().iter().any(|id| id.ends_with(":end")).then_some(())).expect("A's turn ended in its chat");
    session.type_as_user(&b, "bravo topic only for pane B", true).expect("typed in B");
    session.until(60, || chat_b.ids().iter().any(|id| id.ends_with(":end")).then_some(())).expect("B's turn ended in its chat");

    let (text_a, text_b) = (serde_json::to_string(&chat_a.items()).unwrap(), serde_json::to_string(&chat_b.items()).unwrap());
    assert!(text_a.contains("alpha topic only for pane A") && !text_a.contains("bravo"), "A's chat: {text_a}");
    assert!(text_b.contains("bravo topic only for pane B") && !text_b.contains("alpha"), "B's chat: {text_b}");
}

// ---- 3. a helper's answer never goes to another helper --------------------------------------------------------

/// Two helpers under one parent ask the very same question (the same command, so the same words on screen). The
/// person answers helper A's card; A moves on to a different question. Answering A's old card again, on the
/// phone and on the desktop, is refused or a repeat that presses nothing: it never reaches A's new question, and
/// B's question (which looks exactly like the old one) is untouched.
#[test]
fn an_answer_to_a_helpers_old_card_is_refused_and_the_other_helper_is_untouched() {
    let session = World::get().session("safety-two-helpers");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
on_prompt = "go"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "Tests ran." }], on_deny = [{ do = "say", text = "Stopped." }] },
  { do = "approve", kind = "bash", command = "npm run build", on_allow = [{ do = "say", text = "Built." }], on_deny = [{ do = "say", text = "Stopped." }] },
]
"#,
    );
    // Every wait below is a hang detector: with the machine starved, a fake agent took minutes to draw its box.
    let ready = |pane: &str| {
        session.until(150, || session.screen(pane).contains('\u{276f}').then_some(())).expect("the agent drew its input box");
    };
    let a = session.spawn_pane("fake_claude").expect("helper A");
    let b = session.spawn_pane("fake_claude").expect("helper B");
    ready(&a);
    ready(&b);
    session.type_as_user(&a, "go", true).expect("A starts");
    session.type_as_user(&b, "go", true).expect("B starts");
    let card_of = |pane: &str| session.until(200, || session.attention_card(pane)).unwrap_or_else(|| panic!("no card for {pane}\n{}", session.screen(pane)));
    let (card_a, card_b) = (card_of(&a), card_of(&b));
    let (item_a, item_b) = (card_a["id"].as_str().unwrap().to_string(), card_b["id"].as_str().unwrap().to_string());
    assert_ne!(item_a, item_b, "each helper has its own card");
    let b_question_before = session.card(&b).expect("B's question")["prompt_id"].as_str().unwrap().to_string();

    // The person answers A's card on the phone: exactly one key, into A.
    let prompt = &card_a["prompt"];
    let right = json!({
        "choice": "allow_once",
        "prompt_id": prompt["prompt_id"],
        "occurrence": prompt["occurrence"],
        "fingerprint": prompt["fingerprint"],
        "answer_key": "tap-a-1",
    });
    let (code, reply) = session.answer_from_phone(&item_a, right.clone());
    assert_eq!((code, reply["outcome"].as_str()), (200, Some("answered")), "{reply}");
    assert_eq!(events_named(&session, "approval_answer").len(), 1, "one key so far: {:#?}", session.events());

    // A moves on: its screen now asks the second question.
    session.until(200, || session.screen(&a).contains("npm run build").then_some(())).unwrap_or_else(|| panic!("A never moved on\n{}", session.screen(&a)));
    let a_new_question = session.until(100, || session.card(&a).filter(|card| card["prompt_id"] != prompt["prompt_id"])).expect("A's new question");
    assert_ne!(a_new_question["prompt_id"], prompt["prompt_id"], "A's second question has another id");

    // The old card answered again (a new tap on a stale phone screen, a second window): nothing is pressed.
    let mut again = right.clone();
    again["answer_key"] = json!("tap-a-2");
    let (code, reply) = session.answer_from_phone(&item_a, again);
    assert_ne!(reply["outcome"].as_str(), Some("answered"), "a stale card must not answer ({code}): {reply}");
    let old_id = prompt["prompt_id"].as_str().unwrap().to_string();
    match session.answer_card(&a, &old_id, "allow_once") {
        Ok(reply) => assert!(reply["already_applied"] == true || reply["answer"]["already_applied"] == true, "a desktop click on the old card must be a repeat: {reply}"),
        Err(error) => assert!(matches!(error["code"].as_str(), Some("STALE_PROMPT" | "PROMPT_NOT_PENDING")), "{error}"),
    }
    // The same old answer against helper B's pane carries A's item id: it is A's card and goes nowhere near B.
    let (code, reply) = session.answer_from_phone(&item_a, json!({ "choice": "allow_once", "prompt_id": b_question_before, "answer_key": "tap-a-3" }));
    assert_ne!(reply["outcome"].as_str(), Some("answered"), "A's card answered B's question ({code}): {reply}");

    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(events_named(&session, "approval_answer").len(), 1, "no second key reached any helper: {:#?}", session.events());
    assert!(session.screen(&a).contains("npm run build"), "A still waits on its new question:\n{}", session.screen(&a));
    assert!(session.screen(&b).contains("npm test"), "B still shows its question:\n{}", session.screen(&b));
    let b_after = session.card(&b).expect("B still asks");
    assert_eq!(b_after["prompt_id"].as_str(), Some(b_question_before.as_str()), "B's question is the same one");
    assert!(session.attention_card(&b).is_some_and(|card| card["id"] == item_b.as_str()), "B's card is still open and its own");

    // Each helper is then answered through its own, current card, once.
    let new_a = session.until(100, || session.attention_card(&a).filter(|card| card["prompt"]["prompt_id"] != prompt["prompt_id"])).expect("A's new card");
    for (pane, card, key) in [(&a, new_a, "tap-a-4"), (&b, session.attention_card(&b).unwrap(), "tap-b-1")] {
        let p = &card["prompt"];
        let body = json!({ "choice": "allow_once", "prompt_id": p["prompt_id"], "occurrence": p["occurrence"], "fingerprint": p["fingerprint"], "answer_key": key });
        let (code, reply) = session.answer_from_phone(card["id"].as_str().unwrap(), body);
        assert_eq!((code, reply["outcome"].as_str()), (200, Some("answered")), "{pane}: {reply}");
    }
    assert_eq!(events_named(&session, "approval_answer").len(), 3, "A's first, A's second, B's first: {:#?}", session.events());
}
