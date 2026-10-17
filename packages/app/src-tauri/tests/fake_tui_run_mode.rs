//! "How agent runs work" end to end with `pm-fake-tui` as the agent: the real `/agents/run` and
//! `/agents/followup` routes, real `run_agent`, a real pane for the visible runs. The fake agents
//! have no wizard test, so the harness decides whether their headless mode counts as verified
//! (`Session::run_mode`); the fake has no headless mode either, so a run that was sent down the
//! headless road fails with "has no headless mode" and never opens a pane: that is how these tests
//! see the choice. Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::{json, Value};

const STOPPED: &[&str] = &["completed", "failed", "cancelled", "needs_input"];

const SAY: &str = r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [{ do = "think", ms = 300 }, { do = "say", text = "Hello from the fake." }]
"#;

fn run(session: &Session, extra: Value) -> Value {
    // No "headless" key unless the test passes one: the caller did not choose.
    let started = session
        .run("fake_claude", "say hello", extra)
        .unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let handle = started["handle"].as_str().expect("handle").to_string();
    session.wait(&handle, STOPPED, 90_000).unwrap_or_else(|e| panic!("wait failed: {e}"))
}

fn ran_in_a_pane(done: &Value) -> bool {
    done["pane_id"].as_str().is_some_and(|p| !p.is_empty())
}

fn failed_for_lack_of_headless(done: &Value) -> bool {
    done["status"] == "failed" && done.to_string().contains("no headless mode")
}

#[test]
fn the_default_is_a_visible_pane_and_the_receipt_says_nothing_about_the_setting() {
    let session = World::get().session("runmode-default");
    session.script(SAY);
    let done = run(&session, json!({}));
    assert_eq!(done["status"], "completed", "{done}");
    assert!(ran_in_a_pane(&done));
    assert!(done.get("run_mode").is_none() && done["receipt"].get("run_mode").is_none(), "{done}");
    assert!(done["receipt"]["fallback"].is_null());
}

#[test]
fn a_run_without_headless_uses_one_task_mode_when_the_agent_is_set_to_it_and_verified() {
    let session = World::get().session("runmode-pick");
    session.script(SAY);
    session.run_mode("fake_claude", true, true);
    let done = run(&session, json!({}));
    assert!(failed_for_lack_of_headless(&done), "the run must take the headless road: {done}");
    assert!(!ran_in_a_pane(&done), "no pane in one-task mode: {done}");
    assert_eq!(done["receipt"]["run_mode"], json!({"mode": "one_task", "source": "agent_setting"}));
    assert!(session.pane_ids().is_empty(), "no pane was opened");
}

#[test]
fn an_explicit_headless_choice_from_the_caller_wins_over_the_setting() {
    let session = World::get().session("runmode-explicit");
    session.script(SAY);
    session.run_mode("fake_claude", true, true);
    // headless: false -> a visible pane although the agent is set to One-task mode.
    let pane_run = run(&session, json!({ "headless": false }));
    assert_eq!(pane_run["status"], "completed", "{pane_run}");
    assert!(ran_in_a_pane(&pane_run));
    assert!(pane_run["receipt"].get("run_mode").is_none(), "the caller chose, not the setting: {pane_run}");
    // headless: true -> headless although the agent is set to the visible pane.
    session.run_mode("fake_claude", false, true);
    let headless_run = run(&session, json!({ "headless": true }));
    assert!(failed_for_lack_of_headless(&headless_run), "{headless_run}");
    assert!(!ran_in_a_pane(&headless_run));
}

#[test]
fn one_task_mode_that_is_not_verified_falls_back_to_the_visible_pane_and_says_so() {
    let session = World::get().session("runmode-fallback");
    session.script(SAY);
    session.run_mode("fake_claude", true, false);
    let done = run(&session, json!({}));
    assert_eq!(done["status"], "completed", "a visible pane ran: {done}");
    assert!(ran_in_a_pane(&done));
    assert_eq!(done["receipt"]["fallback"], json!({"from": "one_task", "to": "visible", "reason": "not_verified"}));
    assert_eq!(done["receipt"]["run_mode"], json!({"mode": "visible", "source": "agent_setting"}));
    let not_verified = done["receipt"]["not_verified"].to_string();
    assert!(not_verified.contains("one-task mode") && !not_verified.contains("ran headless"), "{not_verified}");
}

#[test]
fn a_follow_up_continues_in_the_mode_of_its_run_whatever_the_setting_became() {
    let session = World::get().session("runmode-followup");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
on_prompt = "say hello"
steps = [{ do = "say", text = "Hello from the fake." }]
[[turn]]
on_prompt = "again"
steps = [{ do = "say", text = "Hello again." }]
"#,
    );
    let first = run(&session, json!({}));
    assert_eq!(first["status"], "completed", "{first}");
    let handle = first["handle"].as_str().expect("handle").to_string();
    // The person switches the agent to One-task mode between the turns.
    session.run_mode("fake_claude", true, true);
    let second = session
        .agents(
            "POST",
            "/agents/followup",
            json!({ "project_path": session.project().to_string_lossy(), "handle": handle, "task": "again please", "wait_ms": 1, "background": true }),
        )
        .unwrap_or_else(|e| panic!("followup refused: {e}"));
    let done = session
        .wait(second["handle"].as_str().expect("handle"), STOPPED, 90_000)
        .unwrap_or_else(|e| panic!("wait failed: {e}"));
    assert_eq!(done["status"], "completed", "the conversation stays in its pane: {done}");
    assert!(ran_in_a_pane(&done));
    assert!(done["receipt"].get("run_mode").is_none(), "{done}");
}
