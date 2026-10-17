//! One answer per prompt occurrence, and the click behind a desktop answer.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const T0: u64 = 1_792_042_552_000; // 13:35:52 local on the hunt day, for flavour

fn pane(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

fn press(count: &AtomicUsize) -> impl FnOnce() -> Result<(), String> + '_ {
    move || {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn click(at_ms: u64) -> UserGesture {
    UserGesture { id: uuid::Uuid::new_v4().to_string(), trusted: true, at_ms, kind: "click".into() }
}

/// The live burst: five "trust" answers for one trust card within 400 ms. One key, four repeats.
#[test]
fn five_answers_to_one_trust_card_in_400_ms_press_one_key() {
    let (pane, keys) = (pane("burst"), AtomicUsize::new(0));
    note_prompt_event(&pane); // the card appeared
    let mut outcomes = Vec::new();
    for i in 0..5 {
        outcomes.push(answer_once(&pane, "prompt-c304183d1c6e71a8", "trust", T0 + i * 100, press(&keys)).unwrap());
    }
    assert_eq!(keys.load(Ordering::SeqCst), 1, "one key for one dialog");
    assert_eq!(outcomes[0], Slot::Sent(()));
    for repeat in &outcomes[1..] {
        let Slot::Duplicate(first) = repeat else { panic!("{repeat:?}") };
        assert_eq!((first.choice.as_str(), first.at_ms), ("trust", T0));
    }
}

/// A second, different answer to the same occurrence (Trust, then a quick Don't trust) is a repeat
/// too: one answer per occurrence, whatever it says.
#[test]
fn a_second_choice_for_the_same_occurrence_presses_nothing() {
    let (pane, keys) = (pane("flip"), AtomicUsize::new(0));
    answer_once(&pane, "p", "trust", T0, press(&keys)).unwrap();
    assert!(matches!(answer_once(&pane, "p", "deny", T0 + 50, press(&keys)).unwrap(), Slot::Duplicate(_)));
    assert_eq!(keys.load(Ordering::SeqCst), 1);
}

/// The same command asked again (the dialog left the screen and came back) is a new question.
#[test]
fn a_new_occurrence_of_the_same_prompt_is_answered_again() {
    let (pane, keys) = (pane("again"), AtomicUsize::new(0));
    note_prompt_event(&pane);
    answer_once(&pane, "p", "allow_once", T0, press(&keys)).unwrap();
    note_prompt_event(&pane); // cleared
    note_prompt_event(&pane); // shown again
    assert_eq!(answer_once(&pane, "p", "allow_once", T0 + 200, press(&keys)).unwrap(), Slot::Sent(()));
    assert_eq!(keys.load(Ordering::SeqCst), 2);
}

/// Past the window, a dialog still drawn may be answered again (the key may have been lost).
#[test]
fn after_the_window_the_same_occurrence_can_be_answered_again() {
    let (pane, keys) = (pane("late"), AtomicUsize::new(0));
    answer_once(&pane, "p", "trust", T0, press(&keys)).unwrap();
    assert!(matches!(answer_once(&pane, "p", "trust", T0 + DEDUPE_WINDOW_MS - 1, press(&keys)).unwrap(), Slot::Duplicate(_)));
    assert_eq!(answer_once(&pane, "p", "trust", T0 + DEDUPE_WINDOW_MS, press(&keys)).unwrap(), Slot::Sent(()));
    assert_eq!(keys.load(Ordering::SeqCst), 2);
}

#[test]
fn panes_and_prompts_do_not_share_a_slot() {
    let keys = AtomicUsize::new(0);
    let (a, b) = (pane("a"), pane("b"));
    answer_once(&a, "p", "trust", T0, press(&keys)).unwrap();
    answer_once(&b, "p", "trust", T0, press(&keys)).unwrap();
    answer_once(&a, "q", "allow_once", T0 + 10, press(&keys)).unwrap();
    assert_eq!(keys.load(Ordering::SeqCst), 3);
}

/// A send that failed records nothing: the next answer may press the key.
#[test]
fn a_failed_send_is_not_an_answer() {
    let (pane, keys) = (pane("fail"), AtomicUsize::new(0));
    let failed: Result<Slot<()>, String> = answer_once(&pane, "p", "trust", T0, || Err("pipe closed".to_string()));
    assert!(failed.is_err());
    assert_eq!(answer_once(&pane, "p", "trust", T0 + 10, press(&keys)).unwrap(), Slot::Sent(()));
}

/// Answers that arrive at the same moment from several threads (the desktop command is async: five
/// clicks become five concurrent calls) press the key once.
#[test]
fn concurrent_answers_press_the_key_once() {
    let pane = Arc::new(pane("conc"));
    let keys = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (pane, keys) = (pane.clone(), keys.clone());
            std::thread::spawn(move || {
                answer_once(&pane, "p", "trust", now_ms(), || {
                    keys.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    Ok::<(), String>(())
                })
                .unwrap()
            })
        })
        .collect();
    let sent = threads.into_iter().map(|t| t.join().unwrap()).filter(|slot| matches!(slot, Slot::Sent(()))).count();
    assert_eq!((sent, keys.load(Ordering::SeqCst)), (1, 1));
}

// ---- the click behind a desktop answer -----------------------------------------------------------

/// No code path may answer a trust gate without a person's click: no gesture, an untrusted event
/// (`element.click()` from code), a stale or a reused one are all refused, for trust AND deny.
#[test]
fn a_trust_gate_is_never_answered_without_a_fresh_trusted_click() {
    for choice in ["trust", "deny", "allow_once"] {
        let error = check_desktop_answer(None, Some("workspace_trust"), choice, T0).unwrap_err();
        assert_eq!(error.code, "ANSWER_NEEDS_A_PERSON", "{choice}");
        let mut synthetic = click(T0);
        synthetic.trusted = false;
        assert_eq!(check_desktop_answer(Some(&synthetic), Some("workspace_trust"), choice, T0).unwrap_err().code, "ANSWER_NEEDS_A_PERSON");
        let old = click(T0 - GESTURE_MAX_AGE_MS - 1);
        assert_eq!(check_desktop_answer(Some(&old), Some("workspace_trust"), choice, T0).unwrap_err().code, "ANSWER_NEEDS_A_PERSON");
        let future = click(T0 + 60_000);
        assert_eq!(check_desktop_answer(Some(&future), Some("workspace_trust"), choice, T0).unwrap_err().code, "ANSWER_NEEDS_A_PERSON");
    }
    let fresh = click(T0 - 30);
    assert!(check_desktop_answer(Some(&fresh), Some("workspace_trust"), "trust", T0).is_ok());
    // The same click cannot send a second answer.
    let error = check_desktop_answer(Some(&fresh), Some("workspace_trust"), "trust", T0 + 10).unwrap_err();
    assert_eq!(error.code, "ANSWER_NEEDS_A_PERSON");
    assert!(error.message.contains("already used"), "{}", error.message);
}

/// Every yes needs a click too; only a refusal of an ordinary prompt may go out without one
/// (Simple mode's own No to a high-risk command).
#[test]
fn without_a_click_only_a_refusal_of_an_ordinary_prompt_goes_out() {
    for yes in ["allow_once", "allow_all", "yes", "trust", "1"] {
        assert!(check_desktop_answer(None, Some("command_approval"), yes, T0).is_err(), "{yes}");
    }
    for no in ["deny", "reject", "no", "Deny"] {
        assert!(check_desktop_answer(None, Some("command_approval"), no, T0).is_ok(), "{no}");
    }
    assert!(check_desktop_answer(None, Some("startup_screen"), "deny", T0).is_err(), "a startup screen is the person's");
    assert!(check_desktop_answer(Some(&click(T0)), Some("command_approval"), "allow_once", T0).is_ok());
}

#[test]
fn refusals_are_named_answers_only() {
    assert!(is_refusal("deny") && is_refusal(" Reject ") && is_refusal("no"));
    assert!(!is_refusal("trust") && !is_refusal("allow_once") && !is_refusal("denying") && !is_refusal(""));
}

// ---- every path that answers -------------------------------------------------------------------

fn sources() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                if rel.contains("tests") || rel.ends_with("_tests.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let code: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
                out.push((rel, code));
            }
        }
    }
    out.sort();
    out
}

fn callers(needle: &str) -> Vec<String> {
    sources().into_iter().flat_map(|(rel, code)| std::iter::repeat(rel).take(code.matches(needle).count())).collect()
}

/// The audit of the caller paths (live v0.2.0 hunt, five trust answers in 400 ms): every keystroke
/// that answers a prompt is pressed inside `answer_once`, and the list of who may call the answer
/// functions is fixed. A new caller fails here until it is reviewed: does it come from a person's
/// click (desktop: `answer_pane_prompt` with its gesture), a phone tap, an agent through its
/// session (gated by `agent_authority`), or a run's own policy (allowed commands, never a trust gate)?
#[test]
fn every_answer_path_is_known_and_presses_its_key_through_answer_once() {
    // A key typed as an answer: `WriteOrigin::Answer` passed straight to `write_input`, or chosen in a
    // `match` next to a phone's `WriteOrigin::Decided(permit)` (integration v0.2.1: phone-approval's
    // decided answers go through the same two places).
    let mut presses = callers("WriteOrigin::Answer)");
    presses.extend(callers("WriteOrigin::Answer,"));
    presses.sort();
    assert_eq!(
        presses,
        ["agent_runs/headless/pane_turn.rs", "agent_runs/routes/answer.rs", "pane_prompt/mod.rs"],
        "a new place presses an answer key: route it through pane_prompt::answers::answer_once and add it here"
    );
    // The Agent Wizard's own answerer (SPEC 11.16) has a write origin of its own, so it can never be mistaken for
    // a card answer: the pane host writes with it (and refuses every pane the wizard did not start); the flight
    // recorder only names it.
    assert_eq!(callers("WriteOrigin::WizardAnswer"), ["flight/mod.rs", "tui/declarative/wizard/visible/registry_host.rs"]);
    for file in &presses {
        let code = &sources().into_iter().find(|(rel, _)| rel == file).unwrap().1;
        let keys = code.matches("WriteOrigin::Answer)").count() + code.matches("WriteOrigin::Answer,").count();
        assert!(code.matches("answer_once(").count() >= keys, "{file}: a key outside answer_once");
    }
    // The desktop's answer function: only the Tauri command (which carries the click) and the
    // dev-only fake-TUI harness (which builds a click like the UI does).
    assert_eq!(callers("answer_for_pane("), ["agent_runs/routes/answer.rs", "commands/cockpit.rs", "fake_tui/world.rs"]);
    // The bare-pane answer: the bridge route (agents and local programs, gated by agent_authority)
    // and a phone's lock-screen tap (a one-shot token minted for that device).
    // (The route calls `answer_with`, the same function with a phone's decided permit.)
    let mut bare = callers("pane_prompt::answer(");
    bare.extend(callers("pane_prompt::answer_with("));
    bare.sort();
    assert_eq!(bare, ["agent_runs/routes/answer.rs", "bridge/routes/push.rs"]);
}

/// The startup gates' keys are never declared as dismiss keys: a trust dialog is never closed by
/// Puppet Master itself (the gate-dismissal path is the only other place keys go into a gate).
#[test]
fn no_profile_dismisses_a_trust_gate_on_its_own() {
    let trust_screens = [
        include_str!("../tui/codex/fixtures/trust_folder_live.txt"),
        include_str!("../tui/claude/fixtures/trust_dialog_2_1_299.txt"),
        include_str!("../tui/cursor/fixtures/workspace_trust_screen.txt"),
    ];
    for agent in ["codex", "claude", "cursor_agent"] {
        let profile = crate::tui::profile_for(agent);
        for screen in trust_screens {
            if let Some(gate) = profile.startup_gate(screen) {
                assert!(gate.dismiss.is_none(), "{agent} would dismiss '{}' itself", gate.id);
            }
        }
    }
}
