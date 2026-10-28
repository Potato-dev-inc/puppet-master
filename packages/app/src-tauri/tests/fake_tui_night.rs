//! Night Shift end to end (SPEC n32 NS-21): zero tokens, `pm-fake-tui` as both agents on real ConPTYs, the real
//! Start checks, the real runner, the real mission engine, the real checks (`node run.js`) in real separate copies,
//! the real Keep and Throw away. Only the agents are fake, and the night's clock is the test's to move
//! (`NightRig::advance_min`), so a stop time or a limit's grace never costs real minutes.
//!
//! The project is a small git folder with a declared test command (`node run.js`): four tests, all failing.
//!
//! 1. Four tasks, two plans (`fake_claude`, `fake_codex`; two companies): two finish green on the first plan; the third
//!    hits a usage limit that is not armed (the CLI will not continue by itself), is ended after the three-minute grace
//!    and finishes green on the other plan; the fourth is pinned to the plan that shows its folder-trust screen, so it
//!    ends as `Needs a look` and nothing is ever typed at that screen (no `gate_answer` in the flight log).
//! 2. A stop time six minutes away cuts a long task off and keeps its copy; the task queued behind it never starts
//!    and says why.
//! 3. The morning: Keep merges a copy into the project, Throw away removes a copy, a second Keep of a task that edits
//!    the same lines conflicts, says so in plain words, and leaves the project and its copy as they were.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World per process).

use puppet_master_app_lib::fake_tui::{NightRig, Session, World};
use serde_json::Value;
use std::time::Duration;

const CLAUDE: &str = "fake_claude";
const CODEX: &str = "fake_codex";

// ---- the project: a cart whose four tests all fail ------------------------------------------------------------

const RUNNER: &str = r#"const harness = require('./harness.js');
require('./cart.test.js');
let pass = 0, fail = 0;
for (const [name, fn] of harness.tests) {
  try { fn(); pass++; console.log('ok - ' + name); }
  catch (e) { fail++; console.log('not ok - ' + name + ': ' + e.message); }
}
console.log('# tests ' + (pass + fail));
console.log('# pass ' + pass);
console.log('# fail ' + fail);
process.exit(fail ? 1 : 0);
"#;

const HARNESS: &str = r#"const tests = [];
function test(name, fn) { tests.push([name, fn]); }
module.exports = { test, tests };
"#;

const TESTS: &str = r#"const { test } = require('./harness.js');
const assert = require('node:assert');
const cart = require('./cart.js');
test('adds', () => { assert.strictEqual(cart.add(2, 3), 5); });
test('subtracts', () => { assert.strictEqual(cart.sub(5, 3), 2); });
test('multiplies', () => { assert.strictEqual(cart.mul(4, 3), 12); });
test('rounds to cents', () => { assert.strictEqual(cart.cents(10.006), 10.01); });
"#;

/// The cart as the project has it: every function wrong.
const BROKEN: [&str; 4] = ["(a, b) => a - b", "(a, b) => a + b", "(a, b) => a + b", "(x) => x"];
/// One way to make all four right.
const FIXED: [&str; 4] = ["(a, b) => a + b", "(a, b) => a - b", "(a, b) => a * b", "(x) => Math.round(x * 100) / 100"];

fn cart(add: &str, sub: &str, mul: &str, cents: &str) -> String {
    format!("exports.add = {add};\nexports.sub = {sub};\nexports.mul = {mul};\nexports.cents = {cents};\n")
}

fn cart_broken() -> String {
    cart(BROKEN[0], BROKEN[1], BROKEN[2], BROKEN[3])
}

/// The first fix: what a Keep merges.
fn cart_fixed() -> String {
    cart(FIXED[0], FIXED[1], FIXED[2], FIXED[3])
}

/// Also right, but `add` is written the other way round: it edits the same line as `cart_fixed`.
fn cart_rival() -> String {
    cart("(a, b) => b + a", FIXED[1], FIXED[2], FIXED[3])
}

/// Also right, with a different `cents`: no other task touches that line.
fn cart_other() -> String {
    cart(FIXED[0], FIXED[1], FIXED[2], "(x) => +(Math.round(x * 100) / 100).toFixed(2)")
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The cart project, committed, with every test failing and `node run.js` declared as its tests.
fn seed(session: &Session, rig: &NightRig) {
    let p = session.project();
    std::fs::write(p.join("cart.js"), cart_broken()).unwrap();
    std::fs::write(p.join("cart.test.js"), TESTS).unwrap();
    std::fs::write(p.join("run.js"), RUNNER).unwrap();
    std::fs::write(p.join("harness.js"), HARNESS).unwrap();
    git(p, &["add", "-A"]);
    git(p, &["-c", "user.name=pm", "-c", "user.email=pm@example.invalid", "commit", "-q", "-m", "cart"]);
    rig.declare_tests("Cart tests", &["node", "run.js"]).expect("declare the tests");
}

// ---- the fake agents' scripts -----------------------------------------------------------------------------------

/// A TOML basic string.
fn toml_str(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

/// One scripted turn: on a prompt holding `on` (the tag in the task's words), think, maybe write the cart, then end.
fn turn(on: &str, cart_text: Option<&str>, last: &str) -> String {
    let mut steps = vec!["{ do = \"think\", ms = 300 }".to_string()];
    if let Some(text) = cart_text {
        steps.push(format!("{{ do = \"write_file\", path = \"cart.js\", content = {} }}", toml_str(text)));
    }
    steps.push(last.to_string());
    format!("[[turn]]\non_prompt = {}\nsteps = [{}]\n", toml_str(on), steps.join(", "))
}

fn say(text: &str) -> String {
    format!("{{ do = \"say\", text = {} }}", toml_str(text))
}

/// Not armed: the agent prints its limit and stops. It never says it will continue by itself. The reset is days
/// away, so the plan stays out for the whole test.
fn limit(reset: &str) -> String {
    format!("{{ do = \"limit\", text = {}, reset = {} }}", toml_str(&format!("⎿  You've hit your session limit · resets {reset}")), toml_str(reset))
}

fn claude(turns: &[String]) -> String {
    format!("agent = \"claude\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n{}", turns.concat())
}

fn codex(gate: bool, turns: &[String]) -> String {
    let startup = if gate { "[[startup]]\ngate = \"trust\"\nafter_ms = 200\n" } else { "" };
    format!("agent = \"codex\"\nboot_ms = 200\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"\n{startup}{}", turns.concat())
}

/// A reset time written the way the vendor writes it, three days from now (it parses as a date, not "today at ...").
fn reset_in_days() -> String {
    let at = chrono::Local::now() + chrono::Duration::days(3);
    format!("{}, 5pm", at.format("%b %-d"))
}

// ---- helpers ------------------------------------------------------------------------------------------------------

fn finished(run: &Value) -> bool {
    matches!(run["state"].as_str(), Some("done") | Some("stopped"))
}

fn task<'a>(run: &'a Value, index: usize) -> &'a Value {
    &run["tasks"][index]
}

fn class(run: &Value, index: usize) -> &str {
    task(run, index)["outcome"]["class"].as_str().unwrap_or("")
}

fn headline(run: &Value, index: usize) -> &str {
    task(run, index)["outcome"]["headline"].as_str().unwrap_or("")
}

/// How the tries of a task ended, oldest first: `completed`, `out_of_usage`, `gate`, `cut_off`...
fn ends(run: &Value, index: usize) -> Vec<String> {
    task(run, index)["attempts"]
        .as_array()
        .map(|attempts| attempts.iter().filter_map(|a| a["end"]["end"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The plan each try of a task ran on, oldest first (the agent type).
fn plans_of(run: &Value, index: usize) -> Vec<String> {
    task(run, index)["attempts"]
        .as_array()
        .map(|attempts| attempts.iter().filter_map(|a| a["plan"]["agent_type"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn event_texts(run: &Value) -> Vec<String> {
    run["events"].as_array().map(|events| events.iter().filter_map(|e| e["text"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn explain(session: &Session, rig: &NightRig, what: &str) -> ! {
    panic!(
        "{what}\n-- run: {}\n-- fake_claude events: {:#?}\n-- fake_codex events: {:#?}",
        serde_json::to_string_pretty(&rig.run()).unwrap_or_default(),
        session.events_for(CLAUDE),
        session.events_for(CODEX)
    );
}

fn count(events: &[Value], name: &str) -> usize {
    events.iter().filter(|e| e["ev"] == name).count()
}

fn prompts(events: &[Value]) -> Vec<String> {
    events.iter().filter(|e| e["ev"] == "prompt").filter_map(|e| e["text"].as_str().map(str::to_string)).collect()
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default().replace('\r', "")
}

// ---- 1. four tasks: green, green, a limit that moves to the other plan, a trust gate -----------------------------

#[test]
fn a_night_goes_green_moves_past_a_limit_and_never_answers_a_trust_screen() {
    let session = World::get().session("night-four");
    let rig = session.night();
    seed(&session, &rig);
    let reset = reset_in_days();
    session.script_for(
        CLAUDE,
        &claude(&[
            turn("[alpha]", Some(&cart_fixed()), &say("Fixed the cart.")),
            turn("[bravo]", Some(&cart_other()), &say("Fixed the cart another way.")),
            turn("[charlie]", None, &limit(&reset)),
        ]),
    );
    // The other plan's turn is slow enough that the script can be changed (below) before the next task starts.
    let charlie_on_codex = format!(
        "[[turn]]\non_prompt = \"[charlie]\"\nsteps = [{{ do = \"think\", ms = 3000 }}, {{ do = \"write_file\", path = \"cart.js\", content = {} }}, {}]\n",
        toml_str(&cart_rival()),
        say("Fixed the cart on the other plan.")
    );
    session.script_for(CODEX, &codex(false, &[charlie_on_codex.clone()]));

    let started = rig
        .start(
            &[
                ("Make the cart tests pass. [alpha]", None),
                ("Make the cart tests pass. [bravo]", None),
                ("Make the cart tests pass. [charlie]", None),
                ("Make the cart tests pass. [delta]", Some(CODEX)),
            ],
            &[CLAUDE, CODEX],
            90,
            1,
        )
        .unwrap_or_else(|e| panic!("Start was refused: {e}"));
    assert_eq!(started["tasks"].as_array().map(Vec::len), Some(4), "{started}");
    assert_eq!(task(&started, 0)["checks"], serde_json::json!([["node", "run.js"]]), "the declared test command is every task's check");

    // (a) Two done and green: one try each, on the first plan, and the copies are kept for the morning.
    let run = rig.wait_for(240, |run| class(run, 0) != "" && class(run, 1) != "");
    if class(&run, 0) != "ready" || class(&run, 1) != "ready" {
        explain(&session, &rig, "tasks 1 and 2 did not finish green");
    }
    for index in [0, 1] {
        assert_eq!(headline(&run, index), "Tests pass 4 of 4", "task {}", index + 1);
        assert_eq!(ends(&run, index), ["completed"]);
        assert_eq!(plans_of(&run, index), [CLAUDE]);
        assert_eq!(rig.copy_on_disk(index), Some(true), "the copy of task {} is kept", index + 1);
    }
    // Nothing merged: the project itself still has the broken cart.
    assert_eq!(read(&session.project().join("cart.js")), cart_broken());

    // (b) Task 3's first plan hits a limit that is not armed. The runner waits in place for the grace (three minutes of
    // the night's clock), then ends the try and runs the task again on the other plan.
    let run = rig.wait_for(120, |run| event_texts(run).iter().any(|t| t.contains("usage limit. Waiting in place")));
    if !event_texts(&run).iter().any(|t| t.contains("usage limit. Waiting in place")) {
        explain(&session, &rig, "task 3 never showed its limit");
    }
    assert_eq!(class(&run, 2), "", "still working on the first plan until the grace has passed: {}", task(&run, 2)["state"]);
    rig.advance_min(4);
    let run = rig.wait_for(120, |run| plans_of(run, 2).len() == 2);
    if plans_of(&run, 2) != [CLAUDE, CODEX] {
        explain(&session, &rig, "task 3 did not move to the other plan");
    }
    // The first plan is out; the new try is the other company's.
    assert_eq!(run["plan_out"][0]["plan_key"], CLAUDE, "{}", run["plan_out"]);
    assert!(event_texts(&run).iter().any(|t| t.contains("ran out of usage at")), "{:#?}", event_texts(&run));
    // The other plan's pane has its prompt: it has started its screen. Its next pane will show the trust screen.
    let codex_started = session.until(60, || (count(&session.events_for(CODEX), "prompt") >= 1).then_some(()));
    assert!(codex_started.is_some(), "the other plan never got the task");
    session.script_for(CODEX, &codex(true, &[charlie_on_codex, turn("[delta]", Some(&cart_other()), &say("Fixed the cart."))]));

    let run = rig.wait_for(240, |run| class(run, 2) != "");
    if class(&run, 2) != "ready" {
        explain(&session, &rig, "task 3 did not finish green on the other plan");
    }
    assert_eq!(headline(&run, 2), "Tests pass 4 of 4");
    assert_eq!(ends(&run, 2), ["out_of_usage", "completed"]);
    assert!(event_texts(&run).iter().any(|t| t.starts_with("Moved task 3 to ")), "{:#?}", event_texts(&run));
    assert_eq!(rig.copy_on_disk(2), Some(true));
    assert_eq!(read(&rig.copy_path(2).expect("a copy").join("cart.js")), cart_rival(), "the result is in the copy the second plan worked in");

    // (c) Task 4 is pinned to the plan that shows its folder-trust screen. Nothing answers it: the try ends as a gate,
    // the task says what it needs, and the run goes on to its end.
    let run = rig.wait_finished(240);
    if !finished(&run) || class(&run, 3) != "needs_look" {
        explain(&session, &rig, "task 4 did not end as `needs a look` at the trust screen");
    }
    assert!(headline(&run, 3).ends_with("asked to trust the folder."), "{}", headline(&run, 3));
    assert_eq!(task(&run, 3)["outcome"]["reason"], "Open its terminal to answer, then run the task again.");
    assert_eq!(ends(&run, 3), ["gate"]);
    assert_eq!(plans_of(&run, 3), [CODEX]);
    assert_eq!(run["blocked"][0]["plan_key"], CODEX, "the plan is not tried again in this folder tonight: {}", run["blocked"]);
    assert!(event_texts(&run).iter().any(|t| t.ends_with("It was not answered.")), "{:#?}", event_texts(&run));
    // The flight logs of both fakes: nothing was typed at a gate, no screen was answered, the fourth task's words were never typed.
    std::thread::sleep(Duration::from_secs(2));
    for agent in [CLAUDE, CODEX] {
        let events = session.events_for(agent);
        assert_eq!(count(&events, "gate_answer"), 0, "{agent} was answered at a gate: {events:#?}");
        assert_eq!(count(&events, "typed_at_gate"), 0, "{agent} was typed at while it showed a gate: {events:#?}");
        assert!(!prompts(&events).iter().any(|p| p.contains("[delta]")), "{agent} was given task 4's words: {events:#?}");
    }
    assert_eq!(count(&session.events_for(CLAUDE), "limit"), 1, "claude drew its limit once");
    assert_eq!(count(&session.events_for(CODEX), "gate_shown"), 1, "codex showed its trust screen to task 4's pane");
    assert_eq!(count(&session.events_for(CLAUDE), "prompt"), 3, "claude: tasks 1, 2 and 3, and nothing else");
    assert_eq!(count(&session.events_for(CODEX), "prompt"), 1, "codex: task 3's second try only");

    // The morning's numbers: three done and green, one that needs you.
    assert_eq!(rig.summary_line(), "3 of 4 done, tests green. 1 needs you.");
    assert_eq!(read(&session.project().join("cart.js")), cart_broken(), "nothing was merged");
}

// ---- 2. a stop time six minutes away ----------------------------------------------------------------------------

#[test]
fn a_stop_time_cuts_a_long_task_off_and_keeps_its_copy() {
    let session = World::get().session("night-cutoff");
    let rig = session.night();
    seed(&session, &rig);
    // The long task fixes one function and then keeps working for ten minutes.
    let part = cart(FIXED[0], BROKEN[1], BROKEN[2], BROKEN[3]);
    session.script_for(CLAUDE, &claude(&[turn("[long]", Some(&part), "{ do = \"wait\", ms = 600000 }")]));
    rig.start(&[("Make the cart tests pass. [long]", None), ("Make the cart tests pass. [later]", None)], &[CLAUDE], 40, 1)
        .unwrap_or_else(|e| panic!("Start was refused: {e}"));

    // The long task is working: its words were typed and its first file is written in its copy.
    let typed = session.until(120, || (count(&session.events_for(CLAUDE), "write_file") >= 1).then_some(()));
    assert!(typed.is_some(), "the long task never got going: {}", rig.run());
    // 34 of the 40 minutes pass: six are left. A task cannot start with less than 25 minutes left, so the one
    // behind the long task is told it will not run; the long task is still going.
    rig.advance_min(34);
    let run = rig.wait_for(60, |run| class(run, 1) != "");
    if class(&run, 1) != "didnt_run" {
        explain(&session, &rig, "the task behind the long one was not told it will not run");
    }
    assert!(headline(&run, 1).starts_with("Didn't run: no time left before "), "{}", headline(&run, 1));
    assert_eq!(run["state"], "running", "the long task is still going");
    assert_eq!(task(&run, 0)["state"], "working");
    assert!(task(&run, 1)["attempts"].as_array().is_some_and(Vec::is_empty), "it never started");

    // The stop time passes: everything still running is stopped, and the copy stays.
    rig.advance_min(7);
    let run = rig.wait_finished(60);
    if !finished(&run) {
        explain(&session, &rig, "the night did not end at the stop time");
    }
    assert_eq!(class(&run, 0), "needs_look");
    assert!(headline(&run, 0).starts_with("Cut off at "), "{}", headline(&run, 0));
    assert!(headline(&run, 0).contains(" while working"), "{}", headline(&run, 0));
    assert_eq!(task(&run, 0)["outcome"]["reason"], "Run the task again to finish it.");
    assert_eq!(ends(&run, 0), ["cut_off"]);
    assert!(event_texts(&run).iter().any(|t| t.starts_with("Stop time ") && t.contains("reached")), "{:#?}", event_texts(&run));
    // The copy is kept, with what the agent had written so far, and the mission is stopped (it gave its slot back).
    assert_eq!(rig.copy_on_disk(0), Some(true), "the copy is kept");
    assert_eq!(read(&rig.copy_path(0).expect("a copy").join("cart.js")), part);
    let mission_id = task(&run, 0)["attempts"][0]["mission_id"].as_str().expect("a mission").to_string();
    assert_eq!(session.mission(&mission_id)["stage"], "stopped");
    assert_eq!(read(&session.project().join("cart.js")), cart_broken(), "nothing was merged");
    assert_eq!(count(&session.events_for(CLAUDE), "prompt"), 1, "the second task was never started");
    assert_eq!(rig.summary_line(), "0 of 2 done. 1 needs you. 1 didn't run.");
}

// ---- 3. the morning: Keep, Throw away, a second Keep that conflicts -------------------------------------------------

#[test]
fn keep_merges_a_copy_throw_away_removes_one_and_a_second_keep_conflicts() {
    let session = World::get().session("night-morning");
    let rig = session.night();
    seed(&session, &rig);
    session.script_for(
        CLAUDE,
        &claude(&[
            turn("[alpha]", Some(&cart_fixed()), &say("Fixed the cart.")),
            turn("[bravo]", Some(&cart_other()), &say("Fixed the cart another way.")),
            turn("[charlie]", Some(&cart_rival()), &say("Fixed the cart a third way.")),
        ]),
    );
    rig.start(
        &[
            ("Make the cart tests pass. [alpha]", None),
            ("Make the cart tests pass. [bravo]", None),
            ("Make the cart tests pass. [charlie]", None),
        ],
        &[CLAUDE],
        90,
        1,
    )
    .unwrap_or_else(|e| panic!("Start was refused: {e}"));
    let run = rig.wait_finished(300);
    if !finished(&run) || (0..3).any(|i| class(&run, i) != "ready") {
        explain(&session, &rig, "the three tasks did not all finish green");
    }
    let project = session.project();
    let commits_before = git(project, &["rev-list", "--count", "HEAD"]).trim().to_string();
    let copies: Vec<_> = (0..3).map(|i| rig.copy_path(i).expect("a copy")).collect();
    assert!(copies.iter().all(|c| c.is_dir()));
    assert_eq!(read(&project.join("cart.js")), cart_broken(), "nothing is merged before the person says so");

    // Keep: task 1's copy is merged into the project's branch, and the project has the fix.
    let kept = rig.keep(0).unwrap_or_else(|e| panic!("Keep was refused: {e}"));
    assert_eq!(kept["merged"], true, "{kept}");
    assert_eq!(kept["into"], git(project, &["symbolic-ref", "--short", "HEAD"]).trim(), "{kept}");
    assert_eq!(read(&project.join("cart.js")), cart_fixed());
    assert_ne!(git(project, &["rev-list", "--count", "HEAD"]).trim(), commits_before, "the merge is in the project's history");
    assert_eq!(rig.run()["tasks"][0]["review"]["review"], "kept");

    // Throw away: task 2's copy is removed, nothing is merged.
    rig.throw_away(1).unwrap_or_else(|e| panic!("Throw away was refused: {e}"));
    assert!(!copies[1].exists(), "the thrown-away copy is gone");
    assert_eq!(rig.copy_on_disk(1), Some(false));
    assert_eq!(rig.run()["tasks"][1]["review"]["review"], "discarded");
    assert_eq!(read(&project.join("cart.js")), cart_fixed(), "throwing away merges nothing");

    // A second Keep that edits the same line conflicts: the project is as it was, the copy is still there, and the
    // row says why in plain words.
    let head = git(project, &["rev-parse", "HEAD"]);
    let second = rig.keep(2).unwrap_or_else(|e| panic!("Keep was refused outright: {e}"));
    assert_eq!(second["merged"], false, "{second}");
    let reason = second["reason"].as_str().unwrap_or_default().to_string();
    assert!(reason.starts_with("This conflicts with a task you already kept (cart.js)."), "{reason}");
    assert_eq!(rig.run()["tasks"][2]["review"]["review"], "keep_failed");
    assert_eq!(rig.run()["tasks"][2]["review"]["sentence"], reason.as_str());
    assert_eq!(git(project, &["rev-parse", "HEAD"]), head, "a conflict leaves the project's history alone");
    assert_eq!(read(&project.join("cart.js")), cart_fixed(), "a conflict leaves the project's files alone");
    assert!(git(project, &["status", "--porcelain"]).trim().is_empty(), "no merge is left half done");
    assert!(copies[2].is_dir(), "the copy stays so the person can look at it");

    // Throw away ends it: the copy goes.
    rig.throw_away(2).unwrap_or_else(|e| panic!("Throw away was refused: {e}"));
    assert!(!copies[2].exists());
}
