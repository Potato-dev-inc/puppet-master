//! Agents Puppet Master drives by READING THEIR SCREEN (a declarative `[tui]` definition, no native protocol),
//! on a real ConPTY with `pm-fake-tui` as a made-up agent ("zeta", drawn with the nimbus layout). Zero tokens.
//!
//! Three generic problems found when a lead drove a wizard-made screen-read agent over MCP and compared it with
//! an agent that has a native protocol:
//!
//! 1. The id of the trust card changed while the screen redrew under the question (a spinner, a counter, a
//!    blinking cursor), so the answer to the id the lead had just read was refused as stale.
//! 2. The reply `run_agent` read from the screen carried the banner, thinking rows and the previous turn.
//! 3. `resolved_model` stayed null although the model name had been on screen.
//!
//! Nothing here is tuned to one CLI: the agent, its words and its regexes are made up.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::World;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// A screen-read agent as a wizard would write it for the made-up layout. `@@THINKING@@` is where the table for
/// thinking / status rows goes (a definition from before `[tui.thinking]` puts them in `[tui.chrome]`).
const ZETA: &str = r##"
schema_version = 1

[agent]
id = "zeta"
label = "Zeta"
experimental = true

[binary]
names = ["zeta"]

[tui]
argv = []
idle_after_ms = 1500
quiet_idle_after_ms = 800

[tui.ready]
regex = '\|\s*>\s*type a task'
not_regex = ['ctrl\+c to stop', 'allow nimbus', 'do you trust']

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
@@CHROME@@
]

@@THINKING@@

[tui.tool_activity]
line_regex = ['^tool:\s', '^->\s']

[tui.footer_model]
regex = 'model:\s*(?P<model>[\w.-]+)'

[[tui.prompt]]
kind = "workspace_trust"
detect_regex = 'do you trust the files in this folder'
choices = [
  { id = "allow_once", text = "1" },
  { id = "deny", text = "2" },
]
"##;

const THINKING_ROWS: &str = r"'^[~*]\s*(thinking|generating)', '^thought for \d+s'";

/// With the thinking rows in the thinking table.
fn zeta() -> String {
    ZETA.replace("@@CHROME@@", "").replace("@@THINKING@@", &format!("[tui.thinking]\nline_regex = [{THINKING_ROWS}]"))
}

/// The same agent written before `[tui.thinking]` existed: the thinking rows are chrome.
fn zeta_chrome_only() -> String {
    ZETA.replace("@@CHROME@@", &format!("  {THINKING_ROWS},")).replace("@@THINKING@@", "")
}

fn handle_of(view: &Value) -> String {
    view["handle"].as_str().unwrap_or_else(|| panic!("no handle in {view}")).to_string()
}

fn status(view: &Value) -> &str {
    view["status"].as_str().unwrap_or("")
}

const TERMINAL: &[&str] = &["completed", "failed", "cancelled"];
const STOPPED: &[&str] = &["completed", "failed", "cancelled", "needs_input"];

fn run(session: &puppet_master_app_lib::fake_tui::Session, task: &str) -> Value {
    session
        .run("fake_zeta", task, json!({ "keep_pane": true }))
        .unwrap_or_else(|e| panic!("run_agent refused: {e}"))
}

fn wait(session: &puppet_master_app_lib::fake_tui::Session, handle: &str, until: &[&str], secs: u64) -> Value {
    session.wait(handle, until, secs * 1000).unwrap_or_else(|e| panic!("wait failed: {e}"))
}

fn followup(session: &puppet_master_app_lib::fake_tui::Session, handle: &str, task: &str) -> Value {
    session
        .agents(
            "POST",
            "/agents/followup",
            json!({ "project_path": session.project().to_string_lossy(), "handle": handle, "task": task, "wait_ms": 1, "background": true }),
        )
        .unwrap_or_else(|e| panic!("followup refused: {e}"))
}

// ---- 1. the id of a card does not change when the screen redraws under the question -----------------------------

const ANIMATED_TRUST: &str = r#"
agent = "nimbus"
boot_ms = 100
[[startup]]
gate = "trust"
after_ms = 100
animate = true
[[turn]]
steps = [{ do = "think", ms = 300 }, { do = "say", text = "ok" }]
"#;

#[test]
fn a_trust_card_keeps_its_id_while_the_screen_animates_under_the_question() {
    let session = World::get().session("screen-card-id");
    session.screen_agent("fake_zeta", &zeta());
    session.script(ANIMATED_TRUST);
    let started = run(&session, "Reply with exactly the word: ok");
    let parked = wait(&session, &handle_of(&started), STOPPED, 60);
    assert_eq!(status(&parked), "needs_input", "{parked}");
    let pane = parked["pane_id"].as_str().expect("pane").to_string();
    let first = parked["prompt"]["id"].as_str().expect("the card has an id").to_string();

    // The question is on screen for a few seconds, redrawing every 200 ms (spinner, counter, cursor block). The id the
    // pane hands out for it must stay the same through all of that.
    let mut seen = BTreeSet::new();
    let screens = std::cell::RefCell::new(BTreeSet::new());
    let until = Instant::now() + Duration::from_secs(4);
    while Instant::now() < until {
        screens.borrow_mut().insert(session.screen(&pane));
        if let Some(card) = session.card(&pane) {
            seen.insert(card["prompt_id"].as_str().unwrap_or_default().to_string());
        }
        std::thread::sleep(Duration::from_millis(90));
    }
    assert!(screens.borrow().len() >= 5, "the screen did redraw under the question ({} pictures)", screens.borrow().len());
    assert_eq!(seen, BTreeSet::from([first.clone()]), "the card id changed while the same question stayed on screen: {seen:?}");

    // The lead answers with the id it read first, through the run's own route (answer_prompt).
    let answered = session
        .agents(
            "POST",
            "/agents/answer",
            json!({ "project_path": session.project().to_string_lossy(), "handle": handle_of(&started), "prompt_id": first, "choice": "allow_once" }),
        )
        .unwrap_or_else(|e| panic!("answer_prompt with the id just read was refused: {e}"));
    assert_eq!(answered["answer"]["applied"], true, "{answered}");
    let done = wait(&session, &handle_of(&started), TERMINAL, 60);
    assert_eq!(status(&done), "completed", "{done}");
    assert_eq!(session.events().iter().filter(|e| e["ev"] == "gate_answer").count(), 1, "one key, one answer");
}

/// The safety rule stays: an id that is not the question's types nothing.
#[test]
fn a_wrong_card_id_types_nothing() {
    let session = World::get().session("screen-card-wrong-id");
    session.screen_agent("fake_zeta", &zeta());
    session.script(ANIMATED_TRUST);
    let started = run(&session, "Reply with exactly the word: ok");
    let parked = wait(&session, &handle_of(&started), STOPPED, 60);
    assert_eq!(status(&parked), "needs_input", "{parked}");
    let refused = session
        .agents(
            "POST",
            "/agents/answer",
            json!({ "project_path": session.project().to_string_lossy(), "handle": handle_of(&started), "prompt_id": "not-the-question", "choice": "allow_once" }),
        )
        .expect_err("a stale id must be refused");
    assert_eq!(refused["code"], "STALE_PROMPT", "{refused}");
    assert!(session.events().iter().all(|e| e["ev"] != "gate_answer"), "nothing was typed: {:#?}", session.events());
}

// ---- 2. the reply is what follows THIS turn's task echo ------------------------------------------------------------

const TWO_TURNS: &str = r#"
agent = "nimbus"
version = "1.0"
boot_ms = 100
backlog_rows = 10
[[turn]]
on_prompt = "first"
steps = [
  { do = "screen", text = "~ Thinking... (2s)" },
  { do = "think", ms = 500 },
  { do = "screen", text = "Thought for 2s" },
  { do = "say", text = "Hello there, friend." },
]
[[turn]]
on_prompt = "second"
steps = [
  { do = "screen", text = "~ Thinking... (4s)" },
  { do = "think", ms = 700 },
  { do = "screen", text = "~ Generating... (1s)" },
  { do = "screen", text = "Thought for 4s" },
  { do = "say", text = "The quick brown fox." },
]
"#;

/// The result of a run whose reply was read off the screen carries the notice that it was (`inferred` capture).
fn reply_of(view: &Value) -> String {
    let result = view["result"].as_str().unwrap_or_default();
    result.strip_prefix("[unverified: read from terminal screen] ").unwrap_or(result).to_string()
}

fn second_turn_result(definition: &str, name: &str) -> (String, Value) {
    let session = World::get().session(name);
    session.screen_agent("fake_zeta", definition);
    session.script(TWO_TURNS);
    let first = run(&session, "Say the first thing please");
    let first = wait(&session, &handle_of(&first), STOPPED, 90);
    assert_eq!(status(&first), "completed", "{first}");
    assert_eq!(reply_of(&first), "reply: Hello there, friend.", "{first}");
    let second = followup(&session, &handle_of(&first), "Say the second thing please");
    let done = wait(&session, &handle_of(&second), STOPPED, 90);
    assert_eq!(status(&done), "completed", "{done}");
    (reply_of(&done), done)
}

/// The first turn is still on screen above this one, and so are the banner and the rows of both turns' thinking. The
/// result is what the agent said to THIS task, and nothing else.
#[test]
fn the_reply_has_no_banner_no_thinking_rows_and_nothing_of_the_previous_turn() {
    let (result, done) = second_turn_result(&zeta(), "screen-reply-clean");
    assert_eq!(result, "reply: The quick brown fox.", "{done}");
    assert_eq!(done["result_capture"], "inferred");
}

/// The same with a definition written before `[tui.thinking]` existed (thinking rows listed as chrome): the banner and
/// the previous turn were the junk then.
#[test]
fn the_reply_is_cut_after_the_task_echo_even_when_thinking_rows_are_chrome() {
    let (result, done) = second_turn_result(&zeta_chrome_only(), "screen-reply-chrome");
    assert_eq!(result, "reply: The quick brown fox.", "{done}");
}

// ---- 3. the model name that was on screen is read ---------------------------------------------------------------------

/// The banner scrolled off the top (a long conversation) and the model is named only on the working row; the footer
/// of the finished screen does not carry it. It was on screen during the turn, so the run knows it.
#[test]
fn the_model_that_was_on_screen_during_the_turn_is_resolved() {
    let session = World::get().session("screen-model");
    session.screen_agent("fake_zeta", &zeta());
    session.script(
        r#"
agent = "nimbus"
model = "zeta-banner"
backlog_rows = 60
status_model = "zeta-live"
status_model_when = "busy"
boot_ms = 100
[[turn]]
steps = [{ do = "think", ms = 1500 }, { do = "say", text = "done" }]
"#,
    );
    let started = run(&session, "Say done");
    let done = wait(&session, &handle_of(&started), STOPPED, 90);
    assert_eq!(status(&done), "completed", "{done}");
    assert_eq!(done["resolved_model"], "zeta-live", "{done}");
    let codes: Vec<&str> = done["warnings"].as_array().into_iter().flatten().filter_map(|w| w["code"].as_str()).collect();
    assert!(!codes.contains(&"MODEL_UNRESOLVED"), "{codes:?}");
}

/// And when the footer shows it all the time, the plain case works too.
#[test]
fn a_model_in_the_footer_is_resolved() {
    let session = World::get().session("screen-model-footer");
    session.screen_agent("fake_zeta", &zeta());
    session.script(
        r#"
agent = "nimbus"
status_model = "zeta-footer"
boot_ms = 100
[[turn]]
steps = [{ do = "think", ms = 300 }, { do = "say", text = "done" }]
"#,
    );
    let started = run(&session, "Say done");
    let done = wait(&session, &handle_of(&started), STOPPED, 90);
    assert_eq!(status(&done), "completed", "{done}");
    assert_eq!(done["resolved_model"], "zeta-footer", "{done}");
}

// ---- 5. a window resize: the last column, and the conversation printed twice ----------------------------------------

/// The pane's viewer sized it (the registry's resize is what the `resize_pane` command calls): the agent is told its
/// new grid and redraws. A footer drawn flush right ends in the NEW last column, for a screen-read agent exactly as for
/// a built-in (nothing in the resize path looks at which agent it is).
#[test]
fn a_resized_screen_read_pane_redraws_its_right_aligned_footer_to_the_new_last_column() {
    let session = World::get().session("screen-resize-footer");
    session.screen_agent("fake_zeta", &zeta());
    session.script("agent = \"nimbus\"\nboot_ms = 100\nfooter_right = \"effort: high\"\n");
    let pane = session.spawn_pane("fake_zeta").expect("a pane");
    let footer_width = |pane: &str| {
        session
            .screen(pane)
            .lines()
            .find(|row| row.trim_end().ends_with("effort: high"))
            .map(|row| row.trim_end().chars().count())
    };
    assert_eq!(session.until(30, || footer_width(&pane).filter(|w| *w == 120)), Some(120), "{}", session.screen(&pane));
    for cols in [100u16, 90, 140, 120] {
        assert_eq!(session.resize(&pane, cols, 30), Ok(true));
        let seen = session.until(20, || footer_width(&pane).filter(|w| *w == cols as usize));
        assert_eq!(seen, Some(cols as usize), "after a resize to {cols} columns the footer must end in the last one:\n{}", session.screen(&pane));
    }
}

const INLINE: &str = r#"
agent = "nimbus"
boot_ms = 100
inline = true
backlog_rows = 70
"#;

/// An inline agent whose conversation is taller than the pane and which prints it ALL again on a resize leaves a second
/// copy of its first rows in the scrollback, unless the profile says it reprints (`reprints_on_resize`): then the
/// viewer drops the history at the resize point and the reprint fills it once.
fn copies_of_the_first_note_after_a_resize(definition: &str, script: &str, name: &str) -> usize {
    let session = World::get().session(name);
    session.screen_agent("fake_zeta", definition);
    session.script(script);
    let pane = session.spawn_pane("fake_zeta").expect("a pane");
    let count = |pane: &str| session.history(pane).lines().filter(|row| row.trim() == "earlier session note 1").count();
    assert_eq!(session.until(30, || Some(count(&pane)).filter(|n| *n >= 1)), Some(1), "the first note is in the history once:\n{}", session.history(&pane));
    assert_eq!(session.resize(&pane, 100, 30), Ok(true));
    // wait for the agent to react to its new size (its footer is redrawn at the new width), then let it finish
    session.until(20, || session.screen(&pane).contains("ctrl+c twice to quit").then_some(()));
    std::thread::sleep(Duration::from_millis(2500));
    count(&pane)
}

#[test]
fn an_inline_agent_that_reprints_leaves_a_second_copy_in_the_history_unless_the_profile_says_so() {
    let reprinting = format!("{INLINE}reprint_on_resize = true\n");
    let flagged = zeta().replace("[tui]\n", "[features]\nreprints_on_resize = true\n\n[tui]\n");
    assert_eq!(copies_of_the_first_note_after_a_resize(&zeta(), &reprinting, "screen-reprint-unflagged"), 2, "the conversation was printed twice");
    assert_eq!(copies_of_the_first_note_after_a_resize(&flagged, &reprinting, "screen-reprint-flagged"), 1, "the viewer dropped the old copy at the resize");
}

#[test]
fn an_inline_agent_that_redraws_only_what_is_visible_never_duplicates() {
    assert_eq!(copies_of_the_first_note_after_a_resize(&zeta(), INLINE, "screen-redraw-plain"), 1);
}

// ---- 6b. a menu nobody's profile describes becomes a card ----------------------------------------------------------------

const MENUS: &str = r#"
agent = "nimbus"
boot_ms = 100
approvals = "menus"
[[turn]]
steps = [
  { do = "think", ms = 800 },
  { do = "approve", kind = "bash", command = "python --version", on_allow = [{ do = "say", text = "ran it" }], on_deny = [{ do = "say", text = "did not run it" }] },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [{ do = "write_file", path = "hello.txt", content = "hi\n" }, { do = "say", text = "wrote it" }], on_deny = [{ do = "say", text = "did not write it" }] },
]
"#;

fn approval_answers(session: &puppet_master_app_lib::fake_tui::Session) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == "approval_answer").collect()
}

fn answer(session: &puppet_master_app_lib::fake_tui::Session, handle: &str, prompt_id: &str, choice: &str, extra: Value) -> Result<Value, Value> {
    let mut body = json!({ "project_path": session.project().to_string_lossy(), "handle": handle, "prompt_id": prompt_id, "choice": choice });
    if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        map.extend(extra.clone());
    }
    session.agents("POST", "/agents/answer", body)
}

/// The definition knows the folder question only. The command and the file questions are menus no table matches: each
/// becomes a card of kind `unknown_question`, marked as recognised by shape, with the options as the screen shows them.
/// `answer_prompt` picks one; nothing is typed until it does, and never into a pane that is working.
#[test]
fn menus_no_table_describes_become_cards_that_answer_prompt_can_pick_from() {
    let session = World::get().session("screen-unknown-menus");
    session.screen_agent("fake_zeta", &zeta());
    session.script(MENUS);
    let started = run(&session, "Check the version and write hello.txt");
    let handle = handle_of(&started);
    let parked = wait(&session, &handle, STOPPED, 60);
    assert_eq!(status(&parked), "needs_input", "{parked}");
    let pane = parked["pane_id"].as_str().expect("pane").to_string();
    let card = &parked["prompt"];
    assert_eq!(card["kind"], "unknown_question", "{parked}");
    let id = card["id"].as_str().expect("a prompt id, not manual_input_required").to_string();
    assert_eq!(session.card(&pane).expect("the pane's card")["recognised_by"], "shape");
    assert!(card["summary"].as_str().unwrap_or_default().contains("recognised by shape, not by profile"), "{card}");
    let labels: Vec<&str> = card["options"].as_array().unwrap().iter().filter_map(|o| o["label"].as_str()).collect();
    assert_eq!(labels, ["Allow once", "Always allow in this project", "Deny"], "{card}");
    // the card is stable while it is shown, and nothing was typed yet
    for _ in 0..5 {
        assert_eq!(session.card(&pane).and_then(|c| c["prompt_id"].as_str().map(str::to_string)), Some(id.clone()));
        std::thread::sleep(Duration::from_millis(150));
    }
    assert!(approval_answers(&session).is_empty());

    // a stale id types nothing
    let stale = answer(&session, &handle, "not-this-question", "allow_once", json!({})).expect_err("stale");
    assert_eq!(stale["code"], "STALE_PROMPT", "{stale}");
    // a broad option needs the explicit opt-in
    let broad = answer(&session, &handle, &id, "Always allow in this project", json!({})).expect_err("broad");
    assert!(broad["message"].as_str().unwrap_or_default().contains("allow_broad") || broad["code"] == "INVALID_PROMPT_CHOICE", "{broad}");
    assert!(approval_answers(&session).is_empty(), "nothing was typed for a refused answer");

    // the lead picks the plain option by label
    let sent = answer(&session, &handle, &id, "Allow once", json!({})).unwrap_or_else(|e| panic!("answer refused: {e}"));
    assert_eq!(sent["answer"]["applied"], true, "{sent}");
    // the next question is the file menu: a different card with a different id
    let second = session
        .until(40, || {
            let v = wait(&session, &handle, STOPPED, 1);
            (status(&v) == "needs_input" && v["prompt"]["id"].as_str().is_some_and(|i| i != id)).then_some(v)
        })
        .unwrap_or_else(|| {
            panic!(
                "the file question parks the run
-- view: {}
-- status: {:?}
-- card: {:?}
-- screen:
{}",
                wait(&session, &handle, &[], 0),
                session.pane_status(&pane),
                session.card(&pane),
                session.screen(&pane)
            )
        });
    assert_eq!(second["prompt"]["kind"], "unknown_question");
    let labels: Vec<&str> = second["prompt"]["options"].as_array().unwrap().iter().filter_map(|o| o["label"].as_str()).collect();
    assert_eq!(labels, ["Yes, write it", "Yes, and allow all edits this session", "No, keep it as it is"], "{second}");
    let second_id = second["prompt"]["id"].as_str().unwrap().to_string();
    // "deny" is the one plain no; the arrow menu is walked to it (Down, Down) and confirmed
    let denied = answer(&session, &handle, &second_id, "deny", json!({})).unwrap_or_else(|e| panic!("answer refused: {e}"));
    assert_eq!(denied["answer"]["applied"], true);
    let done = wait(&session, &handle, TERMINAL, 60);
    assert_eq!(status(&done), "completed", "{done}");
    let answers = approval_answers(&session);
    assert_eq!(answers.len(), 2, "{answers:#?}");
    assert_eq!(answers[0]["answer"], "allow");
    assert_eq!(answers[1]["answer"], "deny", "the cursor was moved to the last row before Enter");
    assert!(reply_of(&done).contains("did not write it"), "{done}");
    assert!(!session.project().join("hello.txt").exists());
}

/// With the opt-in, the broad option can be chosen.
#[test]
fn a_broad_option_is_chosen_only_with_the_opt_in() {
    let session = World::get().session("screen-unknown-menus-broad");
    session.screen_agent("fake_zeta", &zeta());
    session.script(MENUS);
    let started = run(&session, "Check the version");
    let handle = handle_of(&started);
    let parked = wait(&session, &handle, STOPPED, 60);
    assert_eq!(status(&parked), "needs_input", "{parked}");
    let id = parked["prompt"]["id"].as_str().unwrap().to_string();
    let sent = answer(&session, &handle, &id, "option_2", json!({ "allow_broad": true })).unwrap_or_else(|e| panic!("refused: {e}"));
    assert_eq!(sent["answer"]["applied"], true);
    let answers = approval_answers(&session);
    assert_eq!(answers.len(), 1, "{answers:#?}");
    assert_eq!(answers[0]["option"], 2, "the second row was chosen: {answers:#?}");
}
