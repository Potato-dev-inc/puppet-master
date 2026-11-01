//! A terminal chat answering the agent's question (`terminal_chat::prompt`) on a real ConPTY with `pm-fake-tui` as
//! the agent. Zero tokens. The card is read off the real screen through the real profile, a stale or unclicked or
//! unlisted answer presses nothing (the fake logs every key it acts on), and only a clicked, current pick answers.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::Value;

fn logged(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == name).collect()
}

fn ids(card: &Value) -> Vec<String> {
    card["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_string()).collect()
}

fn code(error: &Value) -> &str {
    error["code"].as_str().unwrap_or_default()
}

fn composer_ready(session: &Session, pane: &str) {
    session.until(30, || session.screen(pane).contains('\u{276f}').then_some(())).expect("the agent drew its composer");
}

fn card_for(session: &Session, pane: &str) -> Value {
    session
        .until(30, || session.terminal_chat_prompt(pane))
        .unwrap_or_else(|| panic!("no card appeared\n-- screen:\n{}", session.screen(pane)))
}

const TWO_COMMANDS: &str = r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "first allowed" }], on_deny = [{ do = "say", text = "first denied" }] },
  { do = "approve", kind = "bash", command = "npm run build", on_allow = [{ do = "say", text = "second allowed" }], on_deny = [{ do = "say", text = "second denied" }] },
]
"#;

#[test]
fn a_permission_question_is_answered_only_by_a_current_clicked_listed_pick() {
    let session = World::get().session("tchat-permission");
    session.script(TWO_COMMANDS);
    let pane = session.spawn_pane("fake_claude").expect("a pane");
    composer_ready(&session, &pane);
    assert!(session.terminal_chat_prompt(&pane).is_none(), "no question yet, no card");
    session.type_as_user(&pane, "please run the tests", true).expect("typed");

    let first = card_for(&session, &pane);
    assert_eq!(first["kind"], "permission", "{first}");
    assert_eq!(ids(&first), ["allow_once", "deny"], "broad approvals are not offered: {first}");
    assert!(first["text"].as_str().unwrap_or_default().contains("npm test") || first["summary"].as_str().unwrap_or_default().contains("npm test"), "{first}");
    let id = first["promptId"].as_str().unwrap().to_string();
    assert!(logged(&session, "approval_answer").is_empty(), "nothing answered for the person");

    // Refusals, each of which must press nothing.
    let stale = session.terminal_chat_answer(&pane, "tcp-0000000000000000", "allow_once", true).unwrap_err();
    assert_eq!(code(&stale), "TERMINAL_CHAT_PROMPT_STALE", "{stale}");
    let unknown = session.terminal_chat_answer(&pane, &id, "allow_all", true).unwrap_err();
    assert_eq!(code(&unknown), "TERMINAL_CHAT_UNKNOWN_OPTION", "{unknown}");
    let unclicked = session.terminal_chat_answer(&pane, &id, "allow_once", false).unwrap_err();
    assert_eq!(code(&unclicked), "ANSWER_NEEDS_A_PERSON", "{unclicked}");
    let unclicked_no = session.terminal_chat_answer(&pane, &id, "deny", false).unwrap_err();
    assert_eq!(code(&unclicked_no), "ANSWER_NEEDS_A_PERSON", "even a refusal needs the person: {unclicked_no}");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(logged(&session, "approval_answer").is_empty(), "a refused answer pressed a key: {:#?}", session.events());
    assert_eq!(session.terminal_chat_prompt(&pane).unwrap()["promptId"], first["promptId"], "the question is still the same one");

    // The person's click answers.
    session.terminal_chat_answer(&pane, &id, "allow_once", true).unwrap_or_else(|e| panic!("answer refused: {e}"));
    let answers = logged(&session, "approval_answer");
    assert_eq!(answers.len(), 1, "{:#?}", session.events());
    assert_eq!(answers[0]["answer"], "allow");

    // The next question is a new card; the old card's id is stale and presses nothing.
    let second = session
        .until(30, || session.terminal_chat_prompt(&pane).filter(|card| card["promptId"] != first["promptId"]))
        .unwrap_or_else(|| panic!("no second card\n-- screen:\n{}", session.screen(&pane)));
    assert!(second["text"].as_str().unwrap_or_default().contains("npm run build") || second["summary"].as_str().unwrap_or_default().contains("npm run build"), "{second}");
    let stale = session.terminal_chat_answer(&pane, &id, "allow_once", true).unwrap_err();
    assert_eq!(code(&stale), "TERMINAL_CHAT_PROMPT_STALE", "{stale}");
    assert_eq!(logged(&session, "approval_answer").len(), 1, "the stale answer pressed a key");

    // Deny the second one with its own id.
    let second_id = second["promptId"].as_str().unwrap().to_string();
    session.terminal_chat_answer(&pane, &second_id, "deny", true).unwrap_or_else(|e| panic!("answer refused: {e}"));
    let answers = logged(&session, "approval_answer");
    assert_eq!(answers.len(), 2, "{:#?}", session.events());
    assert_eq!(answers[1]["answer"], "deny");
    session.until(20, || session.terminal_chat_prompt(&pane).is_none().then_some(())).expect("the card goes away after the answer");
    // And once it is gone, its id answers nothing.
    let gone = session.terminal_chat_answer(&pane, &second_id, "deny", true).unwrap_err();
    assert_eq!(code(&gone), "TERMINAL_CHAT_PROMPT_STALE", "{gone}");
}

#[test]
fn a_trust_question_is_a_trust_card_and_needs_the_click() {
    let session = World::get().session("tchat-trust");
    session.script(
        r#"
agent = "claude"
boot_ms = 100
[[startup]]
gate = "trust"
[[turn]]
steps = [{ do = "say", text = "hello after trust" }]
"#,
    );
    let pane = session.spawn_pane("fake_claude").expect("a pane");
    let card = card_for(&session, &pane);
    assert_eq!(card["kind"], "trust", "{card}");
    assert_eq!(ids(&card), ["trust", "deny"], "{card}");
    assert_eq!(card["options"][0]["name"], "Trust this folder", "{card}");
    let id = card["promptId"].as_str().unwrap().to_string();

    let unclicked = session.terminal_chat_answer(&pane, &id, "trust", false).unwrap_err();
    assert_eq!(code(&unclicked), "ANSWER_NEEDS_A_PERSON", "{unclicked}");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(logged(&session, "gate_answer").is_empty(), "nothing trusted the folder without a click");

    session.terminal_chat_answer(&pane, &id, "trust", true).unwrap_or_else(|e| panic!("answer refused: {e}"));
    let answers = session.until(20, || Some(logged(&session, "gate_answer")).filter(|l| !l.is_empty())).expect("the dialog was answered");
    assert_eq!(answers[0]["answer"], "trust");
    session.until(20, || session.terminal_chat_prompt(&pane).is_none().then_some(())).expect("the trust card goes away");
}
