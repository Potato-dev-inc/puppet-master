//! Missions (Preview) end to end with `pm-fake-tui` as the agent: the real mission engine, real
//! `run_agent` / `followup_task` turns typed into real ConPTY panes, real checks (`node run.js`) in
//! the mission's real separate copy, real rollback checkpoints and the real test-change guard.
//! Included from `fake_tui_e2e.rs` (one test binary, one World).
//!
//! The fake picks its turn by a substring of the prompt it was given, so each script keys its turns
//! on the line the engine writes: "Mission:" (the first turn), "1 of 4 pass (was 0 of 4)" (a
//! follow-up), "made the checks worse" (after a rollback), "put the tests back" (after Undo).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::Value;

const RUNNER: &str = r#"const harness = require('./harness.js');
require('./cart.test.js');
let pass = 0, fail = 0;
for (const [name, fn] of harness.tests) {
  try { fn(); pass++; console.log('ok - ' + name); }
  catch (e) { fail++; console.log('not ok - ' + name + ': ' + e.message); }
}
console.log('# tests ' + (pass + fail + harness.skipped()));
console.log('# pass ' + pass);
console.log('# fail ' + fail);
console.log('# skipped ' + harness.skipped());
process.exit(fail ? 1 : 0);
"#;

const HARNESS: &str = r#"const tests = []; let skipped = 0;
function test(name, fn) { tests.push([name, fn]); }
test.skip = () => { skipped++; };
module.exports = { test, tests, skipped: () => skipped };
"#;

const TESTS: &str = r#"const { test } = require('./harness.js');
const assert = require('node:assert');
const cart = require('./cart.js');
test('adds', () => { assert.strictEqual(cart.add(2, 3), 5); });
test('subtracts', () => { assert.strictEqual(cart.sub(5, 3), 2); });
test('multiplies', () => { assert.strictEqual(cart.mul(4, 3), 12); });
test('rounds to cents', () => { assert.strictEqual(cart.cents(10.006), 10.01); });
"#;

const ALL: &[&str] = &["add", "sub", "mul", "cents"];

fn cart(fixed: &[&str]) -> String {
    let f = |name: &str, good: &str, bad: &str| format!("exports.{name} = {};\n", if fixed.contains(&name) { good } else { bad });
    let mut out = String::new();
    out.push_str(&f("add", "(a, b) => a + b", "(a, b) => a - b"));
    out.push_str(&f("sub", "(a, b) => a - b", "(a, b) => a + b"));
    out.push_str(&f("mul", "(a, b) => a * b", "(a, b) => a + b"));
    out.push_str(&f("cents", "(x) => Math.round(x * 100) / 100", "(x) => x"));
    out
}

/// A TOML basic string.
fn toml_str(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

fn writes(path: &str, content: &str) -> String {
    format!("{{ do = \"write_file\", path = \"{path}\", content = {} }}", toml_str(content))
}

/// One scripted turn: on a prompt containing `on`, think briefly, write files, say something.
fn turn(on: &str, files: &[(&str, String)], say: &str) -> String {
    let mut steps = vec!["{ do = \"think\", ms = 300 }".to_string()];
    for (path, content) in files {
        steps.push(writes(path, content));
    }
    steps.push(format!("{{ do = \"say\", text = {} }}", toml_str(say)));
    format!("[[turn]]\non_prompt = {}\nsteps = [{}]\n", toml_str(on), steps.join(", "))
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The cart project, committed, with every test failing.
fn seed(session: &Session) {
    let p = session.project();
    std::fs::write(p.join("cart.js"), cart(&[])).unwrap();
    std::fs::write(p.join("cart.test.js"), TESTS).unwrap();
    std::fs::write(p.join("run.js"), RUNNER).unwrap();
    std::fs::write(p.join("harness.js"), HARNESS).unwrap();
    git(p, &["add", "-A"]);
    git(p, &["-c", "user.name=pm", "-c", "user.email=pm@example.invalid", "commit", "-q", "-m", "cart"]);
}

fn start(session: &Session, agents: &[&str], turns: u32, may_change_tests: bool) -> String {
    let started = session
        .mission_start(agents, "Make the cart tests pass", &[&["node", "run.js"]], turns, may_change_tests)
        .unwrap_or_else(|e| panic!("the mission did not start: {e}"));
    assert_eq!(started["kind"], "mission");
    started["mission"]["id"].as_str().expect("an id").to_string()
}

fn explain(session: &Session, id: &str, view: &Value, what: &str) -> ! {
    panic!(
        "{what}\n-- mission: {}\n-- log: {:#?}\n-- fake events: {:#?}",
        serde_json::to_string_pretty(view).unwrap_or_default(),
        session.mission_log(id),
        session.events()
    );
}

fn lines(view: &Value) -> Vec<String> {
    view["mission"]["timeline"]
        .as_array()
        .map(|rows| rows.iter().filter_map(|r| r["text"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn copy_file(view: &Value, name: &str) -> String {
    let dir = view["mission"]["copy"]["path"].as_str().expect("a copy");
    std::fs::read_to_string(std::path::Path::new(dir).join(name)).unwrap_or_default().replace('\r', "")
}

// ---- pass after N turns ----------------------------------------------------------------------

/// Red to green in three turns of one conversation: the first prompt, then two follow-ups that tell
/// the agent what still fails. Nothing merges: the project's own files are untouched.
#[test]
fn a_mission_goes_red_to_green_over_three_turns() {
    let session = World::get().session("mission-green");
    seed(&session);
    session.script(&format!(
        "agent = \"claude\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n{}{}{}",
        turn("Mission:", &[("cart.js", cart(&["add"]))], "Fixed add."),
        turn("1 of 4 pass (was 0 of 4)", &[("cart.js", cart(&["add", "sub"]))], "Fixed sub."),
        turn("2 of 4 pass (was 1 of 4)", &[("cart.js", cart(ALL))], "All fixed."),
    ));
    let id = start(&session, &["fake_claude"], 10, false);
    let view = session.mission_wait(&id, &["green", "needs_you", "failed", "stopped"], 240);
    if view["stage"] != "green" {
        explain(&session, &id, &view, "the mission did not go green");
    }
    assert_eq!(view["status"], "completed");
    assert_eq!(view["mission"]["turn"], 3, "{view}");
    assert_eq!(view["mission"]["now"], "4 of 4");
    assert_eq!(view["mission"]["start"], "0 of 4");
    // One conversation: three prompts in one pane.
    let prompts: Vec<Value> = session.events().into_iter().filter(|e| e["ev"] == "prompt").collect();
    assert_eq!(prompts.len(), 3, "{prompts:#?}");
    assert!(prompts[1]["text"].as_str().unwrap_or_default().contains("Still failing"), "{prompts:#?}");
    // The work is in the separate copy; the project itself did not change and nothing merged.
    assert_eq!(copy_file(&view, "cart.js"), cart(ALL));
    assert_eq!(std::fs::read_to_string(session.project().join("cart.js")).unwrap().replace('\r', ""), cart(&[]));
    assert!(view["result"].as_str().unwrap_or_default().contains("Nothing was merged."));
    assert!(copy_file(&view, ".puppet-master/MISSION.md").contains("## Progress (Puppet Master)"));
}

// ---- regress -------------------------------------------------------------------------------------

/// The second turn makes the tests worse: the copy goes back to turn 1's files and the agent is told.
#[test]
fn a_turn_that_regresses_is_rolled_back() {
    let session = World::get().session("mission-regress");
    seed(&session);
    session.script(&format!(
        "agent = \"claude\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n{}{}{}",
        turn("Mission:", &[("cart.js", cart(&["add", "sub"]))], "Fixed two."),
        turn("2 of 4 pass (was 0 of 4)", &[("cart.js", cart(&["add"]))], "Refactored."),
        turn("made the checks worse", &[("cart.js", cart(ALL))], "Different approach; all fixed."),
    ));
    let id = start(&session, &["fake_claude"], 10, false);
    let view = session.mission_wait(&id, &["green", "needs_you", "failed", "stopped"], 240);
    if view["stage"] != "green" {
        explain(&session, &id, &view, "the mission did not go green after the rollback");
    }
    let text = lines(&view).join("\n");
    assert!(text.contains("went back to turn 1's files"), "{text}");
    assert!(session.mission_log(&id).iter().any(|e| e["event"] == "rollback"), "a rollback was recorded");
}

// ---- tamper --------------------------------------------------------------------------------------

/// Green by skipping a test: done is blocked with the card; Undo puts the test back and the agent
/// fixes the code for real.
#[test]
fn green_by_skipping_a_test_is_blocked_until_the_person_decides() {
    let session = World::get().session("mission-tamper");
    seed(&session);
    let skipping = TESTS.replace("test('rounds to cents'", "test.skip('rounds to cents'");
    session.script(&format!(
        "agent = \"claude\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n{}{}",
        turn("Mission:", &[("cart.js", cart(&["add", "sub", "mul"])), ("cart.test.js", skipping)], "All green."),
        turn("put the tests back", &[("cart.js", cart(ALL))], "Fixed the rounding."),
    ));
    let id = start(&session, &["fake_claude"], 10, false);
    let view = session.mission_wait(&id, &["green", "needs_you", "failed", "stopped"], 240);
    assert_eq!(view["stage"], "needs_you", "{view}");
    assert_eq!(view["status"], "needs_input");
    assert_eq!(view["mission"]["needs"], "tamper");
    let card = view["mission"]["test_change_lines"].to_string();
    assert!(card.contains(".skip"), "{card}");
    // The person undoes the test changes: the test is back, the agent keeps going.
    session.mission_decide(&id, "undo_tests").expect("undo");
    let view = session.mission_wait(&id, &["green", "failed", "stopped"], 240);
    if view["stage"] != "green" {
        explain(&session, &id, &view, "the mission did not go green after the tests were put back");
    }
    assert_eq!(copy_file(&view, "cart.test.js"), TESTS);
    assert!(lines(&view).iter().any(|l| l == "Put the tests back"));
}

// ---- stuck ---------------------------------------------------------------------------------------

/// An agent that never changes anything: after three turns without progress the list wraps once
/// (a fresh conversation), then the mission asks the person.
#[test]
fn an_agent_that_makes_no_progress_ends_in_the_stuck_card() {
    let session = World::get().session("mission-stuck");
    seed(&session);
    session.script("agent = \"claude\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n[[turn]]\nsteps = [{ do = \"think\", ms = 200 }, { do = \"say\", text = \"Looked around.\" }]\n");
    let id = start(&session, &["fake_claude"], 12, false);
    let view = session.mission_wait(&id, &["green", "needs_you", "failed", "stopped"], 420);
    assert_eq!(view["stage"], "needs_you", "{view}");
    assert_eq!(view["mission"]["needs"], "stuck");
    let text = lines(&view).join("\n");
    assert!(text.contains("made no progress"), "{text}");
    session.mission_decide(&id, "hand_to_me").expect("hand it to me");
    assert_eq!(session.mission(&id)["stage"], "handed_off");
}

// ---- crash / resume ------------------------------------------------------------------------------

/// The agent's program crashes mid-mission: the mission starts it again in a new conversation with a
/// brief, and goes on to green.
#[test]
fn an_agent_that_crashes_is_started_again_with_a_brief() {
    let session = World::get().session("mission-crash");
    seed(&session);
    // The brief also starts with "Mission:", so its turn is listed first (the fake takes the first match).
    session.script(&format!(
        "agent = \"claude\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n{}[[turn]]\non_prompt = \"Mission: Make\"\nsteps = [{{ do = \"think\", ms = 200 }}, {}, {{ do = \"crash\", message = \"thread main panicked\", code = 101 }}]\n",
        turn("You're taking over this mission", &[("cart.js", cart(ALL))], "Picked it up; all fixed."),
        writes("cart.js", &cart(&["add"])),
    ));
    let id = start(&session, &["fake_claude"], 10, false);
    let view = session.mission_wait(&id, &["green", "needs_you", "failed", "stopped"], 300);
    if view["stage"] != "green" {
        explain(&session, &id, &view, "the mission did not recover from the crash");
    }
    let text = lines(&view).join("\n");
    assert!(text.contains("closed unexpectedly"), "{text}");
}
