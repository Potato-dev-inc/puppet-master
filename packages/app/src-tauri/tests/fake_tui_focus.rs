//! Focus end to end (SPEC n33 section 5, FT-12): `pm-fake-tui` as the agent on real ConPTYs, the real turn tracker, the
//! real attention list and the real Focus engine (`fake_tui::FocusRig`, the engine with recorded sinks). Zero tokens.
//!
//! 1. A turn ends while Focus is on: nothing is sent (no digest, no note), the turn is in the tracker, and the end
//!    summary counts it once (`1 turn done`). A second end changes nothing. Focus types nothing.
//! 2. A medium-risk approval raised during Focus is open at once, is counted as a quiet waiter, and its push gate is
//!    `quiet`; a high-risk one is `send` (it goes through). A budget push is `send`. The run waits for the person, and
//!    the end summary counts both cards (`1 asked for you, 1 urgent`). The only answers are the two the test gives as
//!    the person.
//!
//! Push and chat sinks are recorded by the rig; nothing is sent. The scan of the Focus source for agent names and for
//! write paths (SPEC S4, S9) is a unit test in FT-4, not here.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World per process).

use puppet_master_app_lib::fake_tui::{FocusRig, Session, World};
use serde_json::{json, Value};

const STOPPED: &[&str] = &["completed", "failed", "cancelled", "needs_input"];

/// The events that mean a key or an answer reached the agent. The fake logs each one it receives. A submit is a `key`
/// too, so the tests compare the count across a window in which only Focus acts, never over a whole run.
const TYPED: &[&str] = &["key", "typed_at_gate", "gate_answer", "approval_answer", "limit_wait_cancelled"];

fn handle_of(view: &Value) -> String {
    view["handle"].as_str().unwrap_or_else(|| panic!("no handle in {view}")).to_string()
}

fn status(view: &Value) -> &str {
    view["status"].as_str().unwrap_or("")
}

fn events_named(events: &[Value], name: &str) -> Vec<Value> {
    events.iter().filter(|event| event["ev"] == name).cloned().collect()
}

/// The typed-kind events in one fake's log. Each run has its own log (`script_with_log`): `script` would delete the
/// log of the run before.
fn typed_so_far(session: &Session, log: &str) -> usize {
    let events = session.events_of(log);
    TYPED.iter().map(|name| events_named(&events, name).len()).sum()
}

/// Panics with what is needed to see what went wrong: the view, the pane's screen, the fake's log, the rig's sinks.
fn explain(session: &Session, focus: &FocusRig, view: &Value, what: &str) -> ! {
    let pane = view["pane_id"].as_str().unwrap_or_default();
    panic!(
        "{what}\n-- view: {}\n-- screen:\n{}\n-- fake events: {:#?}\n-- sinks: {}\n-- digests: {:#?}\n-- notes: {:#?}",
        serde_json::to_string_pretty(view).unwrap_or_default(),
        session.screen(pane),
        session.events(),
        focus.sink_counts(),
        focus.digests(),
        focus.notes(),
    );
}

// ---- 1. a turn during Focus: held back, counted once at the end --------------------------------------------------

#[test]
fn a_turn_during_focus_is_counted_once_at_the_end_and_nothing_is_sent_before_it() {
    let session = World::get().session("focus_turn");
    let focus = FocusRig::new(session.aux());
    focus.start(25).unwrap_or_else(|e| panic!("Focus would not start: {e}"));
    assert!(focus.is_on(), "Focus is on after start");

    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 500 },
  { do = "say", text = "The first draft is ready." },
]
"#,
    );
    let started = session.run("fake_claude", "Write the first draft", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let done = session.wait(&handle_of(&started), STOPPED, 240_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    if status(&done) != "completed" {
        explain(&session, &focus, &done, "the turn should complete during Focus");
    }
    let pane = done["pane_id"].as_str().expect("the run had a pane").to_string();

    // The turn is in the tracker once its summary is built. Until Focus ends, nothing went to the desktop card, the
    // phone or the chat: a finished turn is not a ping.
    let turns = session
        .until(320, || {
            let turns = focus.finished_turns();
            turns.iter().any(|t| t["pane_id"] == pane.as_str() && t["end"] == "idle").then_some(turns)
        })
        .unwrap_or_else(|| explain(&session, &focus, &done, "the finished turn never reached the tracker"));
    assert_eq!(turns.iter().filter(|t| t["pane_id"] == pane.as_str()).count(), 1, "{turns:?}");
    let before = focus.sink_counts();
    assert_eq!((before["digests"].as_u64(), before["notes"].as_u64()), (Some(0), Some(0)), "a turn in Focus sent something: {before}");
    assert_eq!(events_named(&session.events(), "prompt").len(), 1, "the task is the only prompt the agent got");

    // The end: one summary, one note, counts only. Focus types nothing when it ends.
    let typed_before_end = typed_so_far(&session, "events.jsonl");
    let ended = focus.end();
    assert!(!focus.is_on(), "Focus is off after the end: {ended}");
    let digests = focus.digests();
    assert_eq!(digests.len(), 1, "one end, one desktop summary: {digests:?}");
    assert_eq!(digests[0]["counts"]["turns_done"], 1, "{digests:?}");
    assert_eq!(digests[0]["ended_by"], "early", "{digests:?}");
    let line = digests[0]["line"].as_str().unwrap_or_default();
    assert!(line.contains("1 turn done"), "the summary says the turn: {line}");
    let notes = focus.notes();
    assert_eq!(notes.len(), 1, "one end, one note to the phone and the chat: {notes:?}");
    assert_eq!(notes[0]["kind"], "summary", "{notes:?}");

    // A second end changes nothing (SPEC S8).
    focus.end();
    assert_eq!(focus.digests().len(), 1, "a second end adds no summary");
    assert_eq!(focus.notes().len(), 1, "a second end adds no note");
    assert_eq!(typed_so_far(&session, "events.jsonl"), typed_before_end, "Focus typed into the agent when it ended");

    // The summary and the note name no pane.
    let text = format!("{}{}", serde_json::to_string(&focus.digests()).unwrap(), serde_json::to_string(&focus.notes()).unwrap());
    assert!(!text.contains(&pane), "the summary names the pane: {text}");
}

// ---- 2. approvals during Focus: the gate, the card, the answer -------------------------------------------------------

#[test]
fn a_medium_card_waits_quietly_a_high_risk_one_goes_through_and_nothing_is_typed() {
    let session = World::get().session("focus_approval");
    let focus = FocusRig::new(session.aux());
    focus.start(25).unwrap_or_else(|e| panic!("Focus would not start: {e}"));

    // A medium-risk command (`git rm`: files tracked by git) parks the run on its approval card.
    session.script_with_log(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "git rm old.txt", on_allow = [{ do = "say", text = "Removed old.txt." }], on_deny = [{ do = "say", text = "Kept old.txt." }] },
]
"#,
        "medium.jsonl",
    );
    let first = session.run("fake_claude", "Tidy the repo", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let first_handle = handle_of(&first);
    let parked = session.wait(&first_handle, STOPPED, 240_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    if status(&parked) != "needs_input" {
        explain(&session, &focus, &parked, "the run should park on its medium approval");
    }
    let pane = parked["pane_id"].as_str().expect("the run had a pane").to_string();
    let card = session.until(30, || session.card(&pane)).unwrap_or_else(|| explain(&session, &focus, &parked, "no approval card on the pane"));
    let card_id = card["id"].as_str().or_else(|| parked["prompt"]["id"].as_str()).expect("a prompt id").to_string();

    // Raised during Focus: open at once, a medium risk, and a quiet push (the gate the push hook reads). Focus acts in
    // this window and must not type: the count of typed events does not move.
    let typed_before = typed_so_far(&session, "medium.jsonl");
    focus.tick();
    let open = focus.open_cards();
    assert!(open.contains(&json!({ "kind": "approval", "risk": "medium" })), "the medium card is open: {open:?}");
    assert_eq!(focus.gate("waiting_input", Some("medium")), "quiet", "a medium card goes out quietly");
    assert_eq!(focus.gate("budget", None), "send", "a budget stop goes out at full strength");
    assert_eq!(focus.sink_counts()["notes"], 0, "no note before the end");
    assert_eq!(typed_so_far(&session, "medium.jsonl"), typed_before, "Focus typed while the medium card waited");

    // The person answers the card (the test plays the person): deny.
    session.answer_card(&pane, &card_id, "deny").unwrap_or_else(|e| panic!("answer refused: {e}"));
    let done = session.wait(&first_handle, STOPPED, 60_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    if status(&done) != "completed" {
        explain(&session, &focus, &done, "the run should complete after the answer");
    }

    // A high-risk command: `rm -r` on a folder outside the project. Its gate is `send`, the normal push.
    session.script_with_log(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "rm -r ../shared-cache", on_allow = [{ do = "say", text = "Deleted the cache." }], on_deny = [{ do = "say", text = "Kept the cache." }] },
]
"#,
        "high.jsonl",
    );
    let second = session.run("fake_claude", "Clear the shared cache", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let second_handle = handle_of(&second);
    let risky = session.wait(&second_handle, STOPPED, 240_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    if status(&risky) != "needs_input" {
        explain(&session, &focus, &risky, "the run should park on its high-risk approval");
    }
    let risky_pane = risky["pane_id"].as_str().expect("the run had a pane").to_string();
    let risky_card = session
        .until(30, || session.card(&risky_pane))
        .unwrap_or_else(|| explain(&session, &focus, &risky, "no approval card on the second pane"));
    let risky_card_id = risky_card["id"].as_str().or_else(|| risky["prompt"]["id"].as_str()).expect("a prompt id").to_string();

    let typed_before = typed_so_far(&session, "high.jsonl");
    focus.tick();
    let open = focus.open_cards();
    assert!(open.contains(&json!({ "kind": "approval", "risk": "high" })), "the high-risk card is open: {open:?}");
    assert_eq!(focus.gate("waiting_input", Some("high")), "send", "a high-risk card goes through at full strength");
    assert_eq!(focus.gate("waiting_input", Some("medium")), "quiet", "the medium gate is unchanged");
    assert_eq!(typed_so_far(&session, "high.jsonl"), typed_before, "Focus typed while the high-risk card waited");

    session.answer_card(&risky_pane, &risky_card_id, "deny").unwrap_or_else(|e| panic!("answer refused: {e}"));
    let risky_done = session.wait(&second_handle, STOPPED, 60_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    if status(&risky_done) != "completed" {
        explain(&session, &focus, &risky_done, "the second run should complete after the answer");
    }

    // The end: both cards were asked about during Focus and both are counted.
    focus.end();
    let digests = focus.digests();
    assert_eq!(digests.len(), 1, "one end, one desktop summary: {digests:?}");
    assert_eq!(digests[0]["counts"]["asked"], 1, "the quiet card is counted as asked: {digests:?}");
    assert_eq!(digests[0]["counts"]["urgent"], 1, "the high-risk card is counted as urgent: {digests:?}");
    let line = digests[0]["line"].as_str().unwrap_or_default();
    assert!(line.contains("1 asked for you") && line.contains("1 urgent"), "the summary counts both: {line}");

    // Each task was typed once, and each card was answered once, by the test as the person.
    for log in ["medium.jsonl", "high.jsonl"] {
        let events = session.events_of(log);
        assert_eq!(events_named(&events, "prompt").len(), 1, "{log}: {events:#?}");
        assert_eq!(events_named(&events, "approval_answer").len(), 1, "{log}: {events:#?}");
    }
    let text = serde_json::to_string(&focus.digests()).unwrap();
    assert!(!text.contains("../shared-cache") && !text.contains("git rm"), "the summary names no command: {text}");
}
