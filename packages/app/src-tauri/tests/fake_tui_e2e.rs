//! End-to-end tests with `pm-fake-tui` as the agent: zero tokens, a real ConPTY, the real `/agents/*`
//! routes, the real pane observers, the real `TuiProfile`s.
//!
//! Run (Windows, needs ConPTY and a free cargo target dir for `tools/pm-fake-tui`):
//!
//! ```text
//! cargo test --features dev-fake-tui --test fake_tui_e2e -- --test-threads=1
//! cargo test --features dev-fake-tui --test fake_tui_e2e -- --test-threads=1 --ignored   # the TODO ones
//! ```
//!
//! Each test plays one script from `tools/pm-fake-tui` through `run_agent` -> pane -> delivery ->
//! readiness -> status -> turn summary -> receipt. `src/fake_tui/mod.rs` says what is real (everything
//! except the agent) and what is not (no HTTP, no windows, no bridge port).

#![cfg(windows)]

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::{json, Value};

const TERMINAL: &[&str] = &["completed", "failed", "cancelled"];
const STOPPED: &[&str] = &["completed", "failed", "cancelled", "needs_input"];

fn handle_of(view: &Value) -> String {
    view["handle"].as_str().unwrap_or_else(|| panic!("no handle in {view}")).to_string()
}

fn status(view: &Value) -> &str {
    view["status"].as_str().unwrap_or("")
}

fn error_code(view: &Value) -> &str {
    view["error"]["code"].as_str().unwrap_or("")
}

/// Panics with everything needed to see what went wrong: the view, the pane's screen, the fake's log.
fn explain(session: &Session, view: &Value, what: &str) -> ! {
    let pane = view["pane_id"].as_str().unwrap_or_default();
    panic!(
        "{what}\n-- view: {}\n-- screen:\n{}\n-- fake events: {:#?}",
        serde_json::to_string_pretty(view).unwrap_or_default(),
        session.screen(pane),
        session.events()
    );
}

/// Starts a run (its pane kept open, like a pane a person watches) and returns the started view.
fn start(session: &Session, agent: &str, task: &str, extra: Value) -> Value {
    let mut body = json!({ "keep_pane": true });
    if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        map.extend(extra.clone());
    }
    session.run(agent, task, body).unwrap_or_else(|e| panic!("run_agent refused: {e}"))
}

fn wait_for(session: &Session, handle: &str, until: &[&str], secs: u64) -> Value {
    session.wait(handle, until, secs * 1000).unwrap_or_else(|e| panic!("wait failed: {e}"))
}

/// Starts a run and waits until it is finished or parked on something.
fn run_until_stopped(session: &Session, agent: &str, task: &str, extra: Value) -> Value {
    let started = start(session, agent, task, extra);
    wait_for(session, &handle_of(&started), STOPPED, 90)
}

fn event_names(session: &Session) -> Vec<String> {
    session.events().iter().filter_map(|e| e["ev"].as_str().map(str::to_string)).collect()
}

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == name).collect()
}

// ---- 1. a simple task ----------------------------------------------------------------------------

/// The whole chain on the happy path: the pane is spawned, the readiness recogniser sees Claude's
/// empty composer, the task is typed and submitted, the status goes running -> idle, the reply is read
/// off the screen, the file the agent wrote is on disk, and the run ends with a receipt.
#[test]
fn a_simple_task_runs_through_the_real_path() {
    let session = World::get().session("simple");
    session.script(
        r#"
agent = "claude"
boot_ms = 300
[[turn]]
steps = [
  { do = "think", ms = 800 },
  { do = "write_file", path = "hello.txt", content = "hi\n" },
  { do = "say", text = "Created hello.txt containing hi." },
]
"#,
    );
    let done = run_until_stopped(&session, "fake_claude", "Create hello.txt containing hi", json!({}));
    if status(&done) != "completed" {
        explain(&session, &done, "the run did not complete");
    }
    assert!(done["result"].as_str().unwrap_or_default().contains("Created hello.txt containing hi."), "{done}");
    assert_eq!(done["result_capture"], "inferred", "the reply was read off the terminal screen");
    assert_eq!(done["receipt"]["backend"], "fake_claude");
    assert_eq!(done["receipt"]["outcome"], "completed_unchecked");

    // The agent's own file write really happened, in the run's project.
    assert_eq!(std::fs::read_to_string(session.project().join("hello.txt")).unwrap(), "hi\n");
    // The task reached the agent exactly once, typed into its composer.
    let prompts = events_named(&session, "prompt");
    assert_eq!(prompts.len(), 1, "{:#?}", session.events());
    assert!(prompts[0]["text"].as_str().unwrap().contains("Create hello.txt containing hi"), "{prompts:?}");
    assert_eq!(events_named(&session, "turn_done").len(), 1);

    // The turn summary the cockpit shows ("Claude Code ... changed 1 file") is built from the pane's status
    // transitions and the checkpoint taken after the turn.
    let pane = done["pane_id"].as_str().expect("the run had a pane").to_string();
    let summary = session
        .until(40, || {
            let all = session.turn_summaries(&pane);
            all.as_array().filter(|list| !list.is_empty()).map(|list| list[0].clone())
        })
        .unwrap_or_else(|| explain(&session, &done, "no turn summary was recorded"));
    assert_eq!(summary["turn"], 1, "{summary}");
    assert_eq!(summary["files"]["paths"], json!(["hello.txt"]), "{summary}");
    assert_eq!(summary["end"]["kind"], "idle", "{summary}");
    assert_eq!(summary["approvals"], json!([]), "{summary}");
    assert!(summary["sentence"].as_str().unwrap_or_default().contains("edited 1 file"), "{summary}");
}

// ---- 2. an approval answered from the card ---------------------------------------------------------

/// The run parks on Claude's Bash approval, the card the desktop UI shows is read off the real screen, the
/// answer goes through `answer_pane_prompt`'s own function, the agent continues, and the turn summary
/// remembers that one command was approved. Nothing answers for the user.
#[test]
fn an_approval_is_answered_from_the_card_path() {
    let session = World::get().session("approval");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 500 },
  { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "tool", label = "Bash(npm test)", detail = "all tests passed" }, { do = "say", text = "The tests pass." }], on_deny = [{ do = "say", text = "Stopped: not allowed." }] },
]
"#,
    );
    let parked = run_until_stopped(&session, "fake_claude", "Run the tests", json!({}));
    if status(&parked) != "needs_input" {
        explain(&session, &parked, "the run should have parked on the approval");
    }
    let pane = parked["pane_id"].as_str().expect("pane").to_string();
    let card = &parked["prompt"];
    assert_eq!(card["kind"], "command_approval", "{card}");
    assert_eq!(card["command"], "npm test", "{card}");
    let offered: Vec<&str> = card["options"].as_array().map(|o| o.iter().filter_map(|x| x["id"].as_str()).collect()).unwrap_or_default();
    assert_eq!(offered, ["allow_once", "deny"], "broad approvals are not offered: {card}");
    // The agent is still waiting: nothing was answered by anyone.
    assert!(events_named(&session, "approval_answer").is_empty());
    let shown = session.card(&pane).expect("the pane shows the card");
    let prompt_id = shown["id"].as_str().or_else(|| card["id"].as_str()).expect("a prompt id").to_string();

    session.answer_card(&pane, &prompt_id, "allow_once").unwrap_or_else(|e| panic!("answer refused: {e}"));
    let done = wait_for(&session, &handle_of(&parked), TERMINAL, 60);
    if status(&done) != "completed" {
        explain(&session, &done, "the run should complete after the answer");
    }
    assert!(done["result"].as_str().unwrap_or_default().contains("The tests pass."), "{done}");
    let answers = events_named(&session, "approval_answer");
    assert_eq!(answers.len(), 1, "{:#?}", session.events());
    assert_eq!((answers[0]["answer"].as_str(), answers[0]["option"].as_u64()), (Some("allow"), Some(1)), "{answers:?}");

    let summary = session
        .until(40, || session.turn_summaries(&pane).as_array().and_then(|l| l.last().cloned()))
        .unwrap_or_else(|| explain(&session, &done, "no turn summary"));
    assert_eq!(summary["approvals"].as_array().map(Vec::len), Some(1), "{summary}");
    assert!(summary["approvals"][0]["prompt"].as_str().unwrap_or_default().contains("npm test"), "{summary}");
}

// ---- 3. the folder-trust dialog ---------------------------------------------------------------------

const TRUST_SCRIPT: &str = r#"
agent = "claude"
boot_ms = 200
[[startup]]
gate = "trust"
after_ms = 100
[[turn]]
steps = [{ do = "think", ms = 300 }, { do = "say", text = "Hello from a trusted folder." }]
"#;

/// Folder trust is the user's call, never Puppet Master's. The run parks with the trust card (the task is
/// not typed); declining it from the card makes the agent exit to its host shell, and the run fails with
/// `WORKSPACE_TRUST_DECLINED` instead of typing the task into PowerShell or waiting out its timeout.
#[test]
fn declining_the_trust_dialog_fails_the_run_with_workspace_trust_declined() {
    let session = World::get().session("trust-decline");
    session.script(TRUST_SCRIPT);
    let parked = run_until_stopped(&session, "fake_claude", "Say hello", json!({}));
    if status(&parked) != "needs_input" {
        explain(&session, &parked, "the run should park on the trust dialog");
    }
    let pane = parked["pane_id"].as_str().expect("pane").to_string();
    assert_eq!(parked["prompt"]["kind"], "workspace_trust", "{}", parked["prompt"]);
    let offered: Vec<&str> = parked["prompt"]["options"].as_array().map(|o| o.iter().filter_map(|x| x["id"].as_str()).collect()).unwrap_or_default();
    assert_eq!(offered, ["trust", "deny"], "{}", parked["prompt"]);
    assert!(events_named(&session, "gate_answer").is_empty(), "nothing answered the dialog for the user: {:#?}", session.events());
    assert!(events_named(&session, "prompt").is_empty(), "the task must not be typed over the dialog");

    let prompt_id = parked["prompt"]["id"].as_str().expect("prompt id").to_string();
    session.answer_card(&pane, &prompt_id, "deny").unwrap_or_else(|e| panic!("answer refused: {e}"));
    let done = wait_for(&session, &handle_of(&parked), TERMINAL, 60);
    assert_eq!(status(&done), "failed", "{done}");
    assert_eq!(error_code(&done), "WORKSPACE_TRUST_DECLINED", "{}", done["error"]);
    assert_eq!(events_named(&session, "gate_answer")[0]["answer"], "decline");
    assert_eq!(events_named(&session, "exit")[0]["code"], 1);
    assert!(events_named(&session, "prompt").is_empty(), "the task never reached the agent");
}

/// The other answer: trusting the folder from the card lets the same run continue and finish.
#[test]
fn trusting_the_folder_from_the_card_lets_the_run_continue() {
    let session = World::get().session("trust-accept");
    session.script(TRUST_SCRIPT);
    let parked = run_until_stopped(&session, "fake_claude", "Say hello", json!({}));
    assert_eq!(status(&parked), "needs_input", "{parked}");
    let pane = parked["pane_id"].as_str().expect("pane").to_string();
    let prompt_id = parked["prompt"]["id"].as_str().expect("prompt id").to_string();
    session.answer_card(&pane, &prompt_id, "trust").unwrap_or_else(|e| panic!("answer refused: {e}"));
    let done = wait_for(&session, &handle_of(&parked), TERMINAL, 60);
    if status(&done) != "completed" {
        explain(&session, &done, "the run should finish after trust was given");
    }
    assert!(done["result"].as_str().unwrap_or_default().contains("Hello from a trusted folder."), "{done}");
    assert_eq!(events_named(&session, "gate_answer")[0]["answer"], "trust");
    assert_eq!(events_named(&session, "prompt").len(), 1);
}

// ---- 4. the update menu -------------------------------------------------------------------------------

/// Codex's "Update available" menu opens 600 ms after start, OVER a composer that already looked ready (the
/// live bug of 0.159.3). The run must close it with Esc (skip this launch; never "Update now"), then type
/// the task exactly once into the real composer, and finish.
#[test]
fn the_codex_update_menu_is_dismissed_then_the_task_is_delivered() {
    let session = World::get().session("update-menu");
    session.script(
        r#"
agent = "codex"
boot_ms = 100
[[startup]]
gate = "update_menu"
after_ms = 600
composer_first = true
[[turn]]
on_prompt = "say hi"
steps = [{ do = "think", ms = 400 }, { do = "say", text = "hi from codex" }]
"#,
    );
    let done = run_until_stopped(&session, "fake_codex", "say hi", json!({}));
    if status(&done) != "completed" {
        explain(&session, &done, "the run should complete once the menu is closed");
    }
    assert!(done["result"].as_str().unwrap_or_default().contains("hi from codex"), "{done}");
    let names = event_names(&session);
    let shown = names.iter().position(|n| n == "gate_shown").expect("the menu was shown");
    let answered = names.iter().position(|n| n == "gate_answer").expect("the menu was answered");
    let typed = names.iter().position(|n| n == "prompt").expect("the task was typed");
    assert!(shown < answered && answered < typed, "menu, then its dismissal, then the task: {names:?}");
    assert_eq!(events_named(&session, "gate_answer")[0]["answer"], "skip", "Esc is `skip`; never `update_now`");
    assert_eq!(events_named(&session, "prompt").len(), 1, "the task is typed once");
    assert!(events_named(&session, "exit").is_empty(), "Codex was not sent to update itself");
    assert!(events_named(&session, "enter_in_paste_burst").is_empty(), "Enter waited out Codex's 120 ms paste-burst window");
}

// ---- 5. the agent dies mid-run -------------------------------------------------------------------------

/// The agent crashes after the task was typed. The pane falls back to its host shell's prompt; the run must end
/// at once with `AGENT_EXITED` and the last screen, not wait for its 15-minute timeout.
#[test]
fn an_agent_crash_mid_run_fails_the_run_with_agent_exited() {
    let session = World::get().session("crash");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [{ do = "think", ms = 600 }, { do = "say", text = "Working on it..." }, { do = "think", ms = 600 }, { do = "crash", message = "thread 'main' panicked at agent.rs:42: fake crash", code = 101 }]
"#,
    );
    let done = run_until_stopped(&session, "fake_claude", "Do something long", json!({}));
    assert_eq!(status(&done), "failed", "{done}");
    assert_eq!(error_code(&done), "AGENT_EXITED", "{}", done["error"]);
    assert!(done["error"]["context"]["last_screen"].as_str().unwrap_or_default().contains("fake crash"), "the last screen is in the error: {}", done["error"]);
    assert_eq!(events_named(&session, "prompt").len(), 1);
    assert_eq!(events_named(&session, "exit")[0]["code"], 101);
}

// ---- 6. a follow-up over a dimmed suggestion ---------------------------------------------------------------

/// After a turn Claude draws a dimmed suggestion in its composer (`commit this`). It is the agent's text, not
/// the user's: the follow-up must be typed over it, not parked on "the input box already holds text"
/// (live bug, Claude 2.1.300). Checked on a real ConPTY: the emulator sees the words, readiness does not.
#[test]
fn a_follow_up_is_delivered_over_a_dimmed_suggestion() {
    let session = World::get().session("suggestion");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
on_prompt = "create"
steps = [{ do = "write_file", path = "hello.txt", content = "hi\n" }, { do = "say", text = "Created hello.txt." }, { do = "suggest", text = "commit this" }]
[[turn]]
on_prompt = "commit"
steps = [{ do = "think", ms = 300 }, { do = "say", text = "Committed." }]
"#,
    );
    let first = run_until_stopped(&session, "fake_claude", "create hello.txt", json!({}));
    if status(&first) != "completed" {
        explain(&session, &first, "the first turn should complete");
    }
    let pane = first["pane_id"].as_str().expect("pane").to_string();
    let handle = handle_of(&first);
    // The suggestion is on the pane (the emulator reads it as text) but not in what readiness reads.
    let on_screen = session.until(10, || Some(session.screen(&pane)).filter(|s| s.contains("commit this"))).expect("the suggestion is drawn");
    assert!(on_screen.contains("commit this"));
    assert!(!session.input_screen(&pane).contains("commit this"), "readiness must not read the agent's suggestion as typed text");

    let second = session
        .agents(
            "POST",
            "/agents/followup",
            json!({ "project_path": session.project().to_string_lossy(), "handle": handle, "task": "commit it", "wait_ms": 1, "background": true }),
        )
        .unwrap_or_else(|e| panic!("followup refused: {e}"));
    let done = wait_for(&session, &handle_of(&second), STOPPED, 90);
    if status(&done) != "completed" {
        explain(&session, &done, "the follow-up should be delivered and complete");
    }
    assert!(done["result"].as_str().unwrap_or_default().contains("Committed."), "{done}");
    let prompts = events_named(&session, "prompt");
    assert_eq!(prompts.len(), 2, "{:#?}", session.events());
    let second_text = prompts[1]["text"].as_str().unwrap_or_default();
    assert!(second_text.contains("commit it") && !second_text.contains("commit this"), "typed over the ghost, not appended to it: {second_text:?}");
}

// ---- 7. the usage limit (detector not merged on this base) ---------------------------------------------------

/// The agent hits its usage limit mid-task. Today the run reads as an ordinary finished turn.
///
/// TODO(feat/handoff-detect): that branch adds the usage-limit detector (`handoff/`, DESIGN-RELAY.md sections 1-2).
/// It is not merged on this base (fix/live-r12), so the assertion about what Puppet Master DOES with the screen
/// cannot be written yet. When it lands: remove `#[ignore]`, and replace the final TODO block with the
/// detector's verdict (kind `usage`, reset `9:45 PM`), then assert the run is parked or handed off rather
/// than reported as a normal completion. The fake already draws the vendor texts for both agents
/// (`{ do = "limit" }`; `auto_continue = true` for Claude's "continuing automatically ... esc to cancel").
/// The part that works today (the fake reaches the limit on a real pane and the screen carries the vendor
/// wording the detector will read) runs with `--ignored`.
#[test]
#[ignore = "TODO(feat/handoff-detect): needs the usage-limit detector, not merged on this base"]
fn a_usage_limit_is_detected() {
    let session = World::get().session("limit");
    session.script(
        r#"
agent = "codex"
boot_ms = 100
[[turn]]
steps = [{ do = "think", ms = 400 }, { do = "limit", reset = "9:45 PM" }]
"#,
    );
    let view = run_until_stopped(&session, "fake_codex", "Refactor the parser", json!({}));
    let pane = view["pane_id"].as_str().expect("pane").to_string();
    let screen = session.screen(&pane);
    assert!(screen.contains("\u{25a0} You\u{2019}ve hit your usage limit."), "{screen}");
    assert!(screen.contains("try again at 9:45 PM."), "{screen}");
    // TODO(feat/handoff-detect): let verdict = <the detector>(codex, &screen); assert kind == usage and
    // reset == "9:45 PM"; assert status(&view) is not "completed" (or the run was handed to the next agent).
}

// ---- conformance: every screen the fake draws is read by the REAL profile ---------------------------------------

/// Waits for `needle` on the pane's screen; panics with the screen when it never shows.
fn wait_screen(session: &Session, pane: &str, needle: &str) -> String {
    session
        .until(30, || Some(session.screen(pane)).filter(|screen| screen.contains(needle)))
        .unwrap_or_else(|| panic!("`{needle}` never appeared on:\n{}\nfake events: {:#?}", session.screen(pane), session.events()))
}

fn field<'a>(read: &'a Value, key: &str) -> &'a str {
    read[key].as_str().unwrap_or("")
}

/// Claude Code: trust dialog, empty composer (with the `Try` hint), a running turn, the Bash approval, the
/// edit approval, the dimmed suggestion. The fake draws each on a real ConPTY; the real `claude` profile
/// reads it back the way delivery, the status classifier and the approval card do. A screen the profile
/// stops recognising fails HERE, with the screen in the message.
#[test]
fn every_claude_screen_the_fake_draws_is_recognised_by_the_real_profile() {
    let session = World::get().session("tour-claude");
    session.script(
        r#"
agent = "claude"
boot_ms = 300
[[startup]]
gate = "trust"
after_ms = 100
[[turn]]
steps = [{ do = "think", ms = 2500 }, { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "tests ok" }] }, { do = "approve", kind = "edit", path = "src/sum.js", diff = ["1 -// Adds up a list of numbers.", "1 +// Sums numbers."], on_allow = [{ do = "say", text = "edited" }] }, { do = "suggest", text = "commit this" }]
"#,
    );
    let pane = session.spawn_pane("fake_claude").expect("spawn the pane");

    // The folder-trust dialog is a startup gate only the user may pass, and a card with trust / deny.
    let screen = wait_screen(&session, &pane, "Quick safety check");
    let read = session.read_screen("claude", &screen);
    assert_eq!(field(&read, "gate"), "workspace_trust", "{read}");
    assert!(read["gate_dismiss"].is_null(), "trust is never dismissed for the user: {read}");
    assert_eq!(read["prompt"]["kind"], "workspace_trust", "{read}");
    assert_ne!(field(&read, "ready"), "Empty", "a dialog is never a composer: {read}");
    assert_eq!(field(&read, "status"), "WaitingInput", "{read}");
    session.type_as_user(&pane, "\u{1b}[B", true).expect("Down, Enter: trust");

    // The composer with its dimmed `Try "..."` hint is an empty box.
    let screen = wait_screen(&session, &pane, "Try \"edit");
    let read = session.read_screen("claude", &session.input_screen(&pane));
    assert_eq!(field(&read, "ready"), "Empty", "{screen}\n{read}");
    assert!(read["gate"].is_null() && read["prompt"].is_null() && read["busy"] == false, "{read}");

    // A turn: the activity row and the interrupt hint read as work, and the composer is Working.
    session.type_as_user(&pane, "go", false).expect("type");
    std::thread::sleep(std::time::Duration::from_millis(600));
    session.type_as_user(&pane, "", true).expect("Enter");
    let screen = wait_screen(&session, &pane, "esc to interrupt");
    let read = session.read_screen("claude", &session.input_screen(&pane));
    assert_eq!(field(&read, "ready"), "Working", "{screen}\n{read}");
    assert_eq!(read["busy"], true, "{read}");

    // The Bash approval: a command card with allow / deny, and the pane reads as waiting for the user.
    let screen = wait_screen(&session, &pane, "Do you want to proceed?");
    let read = session.read_screen("claude", &screen);
    assert_eq!(read["prompt"]["kind"], "command_approval", "{screen}\n{read}");
    assert_eq!(read["prompt"]["command"], "npm test", "{read}");
    assert_eq!(field(&read, "status"), "WaitingInput", "{read}");
    assert_ne!(field(&read, "ready"), "Empty", "{read}");
    session.type_as_user(&pane, "", true).expect("Enter: yes");

    // The edit approval: a file card for the path.
    let screen = wait_screen(&session, &pane, "make this edit to src/sum.js");
    let read = session.read_screen("claude", &screen);
    assert_eq!(read["prompt"]["kind"], "file_edit", "{screen}\n{read}");
    assert_eq!(read["prompt"]["path"], "src/sum.js", "{read}");
    session.type_as_user(&pane, "", true).expect("Enter: yes");

    // After the turn: the agent's suggestion is text to the emulator and an empty box to readiness.
    let screen = wait_screen(&session, &pane, "commit this");
    assert!(screen.contains("Churned for") || screen.contains("Brewed for") || screen.contains("Cogitated for") || screen.contains("Crunched for"), "{screen}");
    let seen_by_readiness = session.input_screen(&pane);
    assert!(!seen_by_readiness.contains("commit this"), "{seen_by_readiness}");
    assert_eq!(field(&session.read_screen("claude", &seen_by_readiness), "ready"), "Empty");
    assert_eq!(field(&session.read_screen("claude", &screen), "ready"), "HasText", "control: without the faint-cell rule the suggestion reads as typed text");
}

/// Codex: the loading splash, the update menu, the trust question, the empty composer, a running turn
/// (`Working (Ns • esc to interrupt)`), a failed tool row, and the `■` usage-limit line.
#[test]
fn every_codex_screen_the_fake_draws_is_recognised_by_the_real_profile() {
    let session = World::get().session("tour-codex");
    session.script(
        r#"
agent = "codex"
boot_ms = 1500
[[startup]]
gate = "update_menu"
after_ms = 200
[[startup]]
gate = "trust"
after_ms = 100
[[turn]]
steps = [{ do = "think", ms = 2500 }, { do = "tool", label = "Get-Content README.md -TotalCount 1", detail = "Failed to create unified exec process", failed = -1 }, { do = "say", text = "I could not read README.md." }, { do = "limit", reset = "9:45 PM" }]
"#,
    );
    let pane = session.spawn_pane("fake_codex").expect("spawn the pane");

    // Still loading: Codex draws a composer before it takes keys; the profile must not call that ready.
    let screen = wait_screen(&session, &pane, "loading");
    let read = session.read_screen("codex", &session.input_screen(&pane));
    assert_ne!(field(&read, "ready"), "Empty", "{screen}\n{read}");
    assert_eq!(field(&read, "ready"), "Unknown", "{read}");

    // The update menu: a dismissible gate whose key is Esc (skip this launch, never `Update now`).
    let screen = wait_screen(&session, &pane, "Skip until next version");
    let read = session.read_screen("codex", &screen);
    assert_eq!(field(&read, "gate"), "update_available", "{screen}\n{read}");
    assert_eq!(read["gate_dismiss"], "\u{1b}", "{read}");
    assert_ne!(field(&read, "ready"), "Empty", "{read}");
    session.type_as_user(&pane, "\u{1b}", false).expect("Esc");

    // The trust question: a gate only the user passes.
    let screen = wait_screen(&session, &pane, "Do you trust the contents of this directory");
    let read = session.read_screen("codex", &screen);
    assert_eq!(field(&read, "gate"), "workspace_trust", "{screen}\n{read}");
    assert!(read["gate_dismiss"].is_null(), "{read}");
    session.type_as_user(&pane, "", true).expect("Enter: Yes, continue");

    // The empty composer with its dimmed placeholder.
    let screen = wait_screen(&session, &pane, "Ask Codex to do anything");
    let read = session.read_screen("codex", &session.input_screen(&pane));
    assert_eq!(field(&read, "ready"), "Empty", "{screen}\n{read}");
    assert!(read["gate"].is_null() && read["busy"] == false, "{read}");

    // A turn: `Working (Ns • esc to interrupt)` is work.
    session.type_as_user(&pane, "go", false).expect("type");
    std::thread::sleep(std::time::Duration::from_millis(700));
    session.type_as_user(&pane, "", true).expect("Enter");
    let screen = wait_screen(&session, &pane, "esc to interrupt");
    let read = session.read_screen("codex", &session.input_screen(&pane));
    assert_eq!(field(&read, "ready"), "Working", "{screen}\n{read}");
    assert_eq!(read["busy"], true, "{read}");

    // The failed tool row, then the usage-limit line the relay design reads.
    let screen = wait_screen(&session, &pane, "hit your usage limit");
    assert!(screen.contains("\u{2022} Failed (exit -1) Get-Content README.md -TotalCount 1"), "{screen}");
    assert!(screen.contains("\u{25a0} You\u{2019}ve hit your usage limit.") && screen.contains("try again at 9:45 PM."), "{screen}");
    let read = session.read_screen("codex", &session.input_screen(&pane));
    assert_eq!(field(&read, "ready"), "Empty", "the composer is back after the limit: {read}");
}

// ---- resize ----------------------------------------------------------------------------------------------------

/// A window resize makes a TUI repaint its whole screen. An idle pane must stay idle through the repaint (the
/// live Codex bug: a resize read as "Working 0s" for 5 s), for both layouts, on a real ConPTY.
#[test]
fn a_resize_repaint_does_not_make_an_idle_pane_busy() {
    let session = World::get().session("resize");
    for (agent, fake, composer) in [("codex", "fake_codex", "Ask Codex to do anything"), ("claude", "fake_claude", "Try \"edit")] {
        session.script(&format!("agent = \"{agent}\"\nboot_ms = 200\n"));
        let pane = session.spawn_pane(fake).expect("spawn the pane");
        wait_screen(&session, &pane, composer);
        session.until(30, || (session.pane_status(&pane).as_deref() == Some("idle")).then_some(())).unwrap_or_else(|| panic!("{agent}: the pane never went idle: {:?}", session.pane_status(&pane)));
        let mut seen = Vec::new();
        for (cols, rows) in [(100, 40), (80, 24), (120, 30)] {
            assert_eq!(session.resize(&pane, cols, rows), Ok(true));
            for _ in 0..25 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                seen.push(session.pane_status(&pane).unwrap_or_default());
            }
            let screen = wait_screen(&session, &pane, composer);
            assert!(screen.lines().any(|row| row.contains(composer)), "{agent} repainted at {cols}x{rows}:\n{screen}");
        }
        assert!(seen.iter().all(|status| status == "idle"), "{agent}: an idle pane read {:?} through a resize", seen.iter().collect::<std::collections::BTreeSet<_>>());
        session.close_pane(&pane);
    }
}

// ---- a run whose pane auto-closes still gets its turn summary ------------------------------------------------

/// With the default `keep_pane = false` the run's pane is closed as soon as the run completes
/// (`agent_runs::pane_close::maybe_dispose_after_terminal`). That used to happen before the turn summary had
/// waited out its "after" checkpoint and read the pane, so no summary was recorded and the away digest never
/// listed the turn. Closing now waits (bounded) for the summary, and the summary no longer needs the pane.
#[test]
fn a_run_whose_pane_auto_closes_still_gets_its_turn_summary() {
    let session = World::get().session("autoclose");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [{ do = "write_file", path = "hello.txt", content = "hi\n" }, { do = "say", text = "Created hello.txt." }]
"#,
    );
    let done = run_until_stopped(&session, "fake_claude", "create hello.txt", json!({ "keep_pane": false }));
    assert_eq!(status(&done), "completed", "{done}");
    let pane = done["pane_id"].as_str().expect("pane").to_string();
    let recorded = session.until(40, || session.turn_summaries(&pane).as_array().filter(|l| !l.is_empty()).map(|l| l.len()));
    assert_eq!(recorded, Some(1), "no turn summary was recorded for the auto-closed pane");
}
