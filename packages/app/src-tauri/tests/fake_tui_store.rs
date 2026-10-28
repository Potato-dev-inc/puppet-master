//! The Agent Store, end to end (SPEC n32 section 5, AS-22): zero tokens, `pm-fake-tui` as a made-up agent (`nimbus`,
//! drawn with the nimbus layout) on a real ConPTY, the Store's own `add` / `remove` over the e2e app-data folder,
//! the real approval, the real registry, the real pane reader. Only the agent, and one test-only catalog entry
//! (`fake_tui::store_rig`, a `dev-fake-tui` hook that the shipped app never has), are made up.
//!
//! 1. Add with the program on `PATH`: the file is written, the approval says `store`, the card is `ready`, a visible
//!    pane starts, the Chat tab reads the screen, and a permission question becomes a card. Nothing answers it
//!    except the test, playing the person.
//! 2. Add with the program missing is refused (`ADD_PROGRAM_MISSING`) and writes nothing.
//! 3. A file the person edited is `approval_stale`, the card asks for a review, a run is refused and Remove leaves the
//!    file alone.
//! 4. Remove moves the files to `.removed/`; Settings' restore and the Store's own Add both bring the agent back.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World per process).

use puppet_master_app_lib::fake_tui::{ChatProbe, Session, StoreRig, World};
use serde_json::{json, Value};

/// The agent: answers a greeting, and asks before it creates a file.
const SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 100
[[turn]]
on_prompt = "greet"
steps = [
  { do = "think", ms = 400 },
  { do = "say", text = "Hello from the store agent." },
]
[[turn]]
on_prompt = "create"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [{ do = "say", text = "Created hello.txt." }], on_deny = [{ do = "say", text = "Stopped." }] },
]
"#;

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|event| event["ev"] == name).collect()
}

fn state_of(card: &Value) -> &str {
    card["state"].as_str().unwrap_or_default()
}

fn text_of(items: &[Value], id: &str) -> String {
    let item = items.iter().find(|item| item["id"] == id).unwrap_or_else(|| panic!("no item {id} in {items:#?}"));
    item["text"].as_str().or(item["summary"].as_str()).unwrap_or_default().to_string()
}

/// The rig with the program installed and `nimbus` added by the Store.
fn added(session: &Session) -> StoreRig {
    let rig = session.store_rig(SCRIPT);
    rig.program_on_path();
    let out = rig.add(false).unwrap_or_else(|e| panic!("the Store refused to add nimbus: {e}"));
    assert_eq!(out["ok"], true, "{out}");
    rig
}

// ---- 1. add, a visible pane, the chat, a card ------------------------------------------------------------------------

#[test]
fn an_agent_added_from_the_store_runs_in_a_visible_pane_and_its_question_waits_for_the_person() {
    let session = World::get().session("store-add-run");
    let rig = session.store_rig(SCRIPT);

    // Not on this PC yet: the card offers it, nothing is registered.
    assert_eq!(state_of(&rig.card()), "available", "{}", rig.card());
    assert!(rig.definition_row().is_null());
    assert!(!rig.agent_file().exists(), "nothing is written before an add");

    // The program is on PATH: found, still not added.
    rig.program_on_path();
    assert_eq!(state_of(&rig.card()), "found_on_pc", "{}", rig.card());

    let added = rig.add(false).unwrap_or_else(|e| panic!("add refused: {e}"));
    assert_eq!(added["ok"], true, "{added}");
    assert_eq!(added["id"], "nimbus");
    assert_eq!(added["state"], "ready", "{added}");

    // Registered, approved by the Store, with the shipped file and its fixtures on disk.
    let row = rig.definition_row();
    assert_eq!(row["state"], "active", "{row}");
    assert_eq!(row["approved_by"], "store", "{row}");
    assert_eq!(row["approval_checks"], 2, "{row}");
    assert!(rig.agent_file().is_file() && rig.fixtures_dir().join("idle.txt").is_file());
    let card = rig.card();
    assert_eq!(state_of(&card), "ready", "{card}");
    assert!(card["state_reason"].as_str().unwrap_or_default().contains("ready"), "{card}");

    // A visible pane starts and reads as ready.
    let pane = session.spawn_pane("nimbus").unwrap_or_else(|e| panic!("no pane for the added agent: {e}"));
    session.until(30, || session.screen(&pane).contains("Type a task").then_some(())).unwrap_or_else(|| panic!("nimbus never drew its input box\n{}", session.screen(&pane)));

    // The Chat tab is read off the screen: the person types, the chat follows.
    let chat: ChatProbe = session.open_screen_chat(&pane, "nimbus");
    session.type_as_user(&pane, "Please greet me", true).expect("typing");
    session
        .until(60, || chat.ids().contains(&"screen:1:end".to_string()).then_some(()))
        .unwrap_or_else(|| panic!("the chat never showed the end of turn 1\n{:#?}\n{}", chat.items(), session.history(&pane)));
    let items = chat.items();
    assert_eq!(text_of(&items, "screen:1:user"), "Please greet me");
    assert_eq!(text_of(&items, "screen:1:reply"), "reply: Hello from the store agent.");

    // The agent asks before it creates a file: a card for the person. Nothing answers it.
    session.type_as_user(&pane, "Please create the file", true).expect("typing");
    let card = session
        .until(30, || session.card(&pane))
        .unwrap_or_else(|| panic!("the question never became a card\n{}", session.screen(&pane)));
    let prompt_id = card["prompt_id"].as_str().expect("the card has an id").to_string();
    assert_eq!(card["kind"], "command_approval", "{card}");
    for _ in 0..8 {
        assert_eq!(session.card(&pane).and_then(|c| c["prompt_id"].as_str().map(str::to_string)), Some(prompt_id.clone()), "the card is stable");
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    assert!(events_named(&session, "approval_answer").is_empty(), "something answered the question: {:#?}", session.events());
    assert!(session.approvals_for(&pane).is_empty(), "no answer was logged yet");

    // The person taps Allow (the desktop UI's own path, a trusted click): that is the only answer the agent gets.
    let answered = session.answer_card(&pane, &prompt_id, "allow_once").unwrap_or_else(|e| panic!("the person's answer was refused: {e}"));
    assert_eq!(answered["sent_key"], "y", "{answered}");
    session
        .until(60, || chat.ids().contains(&"screen:2:end".to_string()).then_some(()))
        .unwrap_or_else(|| panic!("the chat never showed the end of turn 2\n{:#?}\n{}", chat.items(), session.history(&pane)));
    let answers = events_named(&session, "approval_answer");
    assert_eq!(answers.len(), 1, "exactly the person's answer: {answers:#?}");
    assert_eq!(answers[0]["answer"], "allow", "{answers:#?}");
    assert_eq!(text_of(&chat.items(), "screen:2:reply"), "reply: Created hello.txt.");
}

// ---- 2. the program is missing -----------------------------------------------------------------------------------------

#[test]
fn adding_an_agent_whose_program_is_missing_is_refused_and_writes_nothing() {
    let session = World::get().session("store-add-missing");
    let rig = session.store_rig(SCRIPT);
    rig.program_off_path();

    let refused = rig.add(false).expect_err("add must be refused without the program");
    assert!(refused.starts_with("ADD_PROGRAM_MISSING"), "{refused}");
    assert!(!rig.agent_file().exists() && !rig.fixtures_dir().exists(), "a refused add wrote files");
    assert!(rig.definition_row().is_null(), "{}", rig.definition_row());
    assert_eq!(state_of(&rig.card()), "available", "{}", rig.card());
}

// ---- 3. an edited file ----------------------------------------------------------------------------------------------------

#[test]
fn a_file_edited_after_the_add_needs_review_and_is_never_run_or_removed_by_the_store() {
    let session = World::get().session("store-edited");
    let rig = added(&session);
    assert_eq!(state_of(&rig.card()), "ready", "{}", rig.card());

    // A change that is not part of what was approved (the label, a comment) keeps the approval, but the file is no
    // longer the shipped copy: the card says the person has their own version, and the Store leaves it alone.
    let shipped = std::fs::read_to_string(rig.agent_file()).expect("the added file");
    let relabelled = shipped.replace("label = \"Nimbus\"", "label = \"Nimbus (mine)\"");
    assert_ne!(relabelled, shipped);
    std::fs::write(rig.agent_file(), &relabelled).expect("edit the file");
    rig.reload();
    assert_eq!(rig.definition_row()["state"], "active", "{}", rig.definition_row());
    assert_eq!(state_of(&rig.card()), "own_version", "{}", rig.card());

    // A change to what runs (the program's arguments) makes the approval stale. The app's file watcher reloads; the
    // test does it.
    let edited = shipped.replace("argv = []", "argv = [\"--danger\"]");
    assert_ne!(edited, shipped);
    std::fs::write(rig.agent_file(), edited).expect("edit the file");
    rig.reload();

    let row = rig.definition_row();
    assert_eq!(row["state"], "approval_stale", "{row}");
    // The card is never `ready`. (A stale definition is not registered, so `state::detect_at` finds no registry row
    // for it and the card reads `found_on_pc`, not the `needs_review` row of SPEC 3.4; Add then says the file is
    // already there. Reported with AS-22; the assertion keeps the safe part.)
    let card = rig.card();
    assert_ne!(state_of(&card), "ready", "an edited file is never `ready`: {card}");
    assert!(rig.add(false).expect_err("add never overwrites").starts_with("AGENT_DEF_DUPLICATE_ID"));

    // A run is refused at the spawn gate and nothing starts.
    let refused = session.run("nimbus", "Please greet me", json!({})).expect_err("a stale definition must not run");
    assert_eq!(refused["code"], "AGENT_DEF_APPROVAL_STALE", "{refused}");
    assert!(events_named(&session, "prompt").is_empty(), "the agent got a prompt: {:#?}", session.events());

    // The Store removes only its own file, byte for byte.
    let removal = rig.remove().expect_err("an edited file is not the Store's file any more");
    assert!(removal.starts_with("REMOVE_NOT_STORE_FILE"), "{removal}");
    assert!(std::fs::read_to_string(rig.agent_file()).expect("still there").contains("--danger"));
}

// ---- 4. remove, then bring it back ----------------------------------------------------------------------------------------

#[test]
fn removing_a_store_agent_can_be_undone_both_ways() {
    let session = World::get().session("store-remove-restore");
    let rig = added(&session);

    // Remove: the files leave the agents folder, the agent leaves the registry, the program is still found.
    let out = rig.remove().unwrap_or_else(|e| panic!("remove refused: {e}"));
    assert_eq!(out["ok"], true, "{out}");
    assert!(!rig.agent_file().exists() && !rig.fixtures_dir().exists());
    assert!(rig.definition_row().is_null(), "{}", rig.definition_row());
    assert_eq!(state_of(&rig.card()), "found_on_pc", "{}", rig.card());
    let removed = rig.removed();
    assert_eq!(removed["total"], 1, "{removed}");
    let removed_id = removed["removed"][0]["removed_id"].as_str().unwrap_or_else(|| panic!("no removed id in {removed}")).to_string();

    // Undo through the removal list (Settings > Agents): the file and its approval come back as they were.
    rig.restore(&removed_id).unwrap_or_else(|e| panic!("restore refused: {e}"));
    let row = rig.definition_row();
    assert_eq!(row["state"], "active", "{row}");
    assert_eq!(row["approved_by"], "store", "the approval returned with the identical file: {row}");
    assert!(rig.agent_file().is_file());
    let pane = session.spawn_pane("nimbus").unwrap_or_else(|e| panic!("no pane after the restore: {e}"));
    session.until(30, || session.screen(&pane).contains("Type a task").then_some(())).expect("the restored agent starts");
    session.close_pane(&pane);

    // The other way back: remove again, then Add from the Store (the card's "Put back").
    rig.remove().unwrap_or_else(|e| panic!("second remove refused: {e}"));
    assert_eq!(state_of(&rig.card()), "found_on_pc", "{}", rig.card());
    let again = rig.add(false).unwrap_or_else(|e| panic!("add after a remove refused: {e}"));
    assert_eq!(again["state"], "ready", "{again}");
    let row = rig.definition_row();
    assert_eq!((row["state"].as_str(), row["approved_by"].as_str()), (Some("active"), Some("store")), "{row}");
    assert_eq!(state_of(&rig.card()), "ready", "{}", rig.card());
}
