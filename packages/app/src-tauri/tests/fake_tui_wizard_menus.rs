//! The safe answerer on questions whose highlighted option is drawn, not marked (SPEC 11.20), end to end on real ConPTYs with
//! `pm-fake-tui` playing an UNKNOWN agent ("nimbus"). Zero tokens, zero person input: the test never types into a pane.
//!
//! The founder's run: "Finish without me" answered the folder question, then the agent asked permission to run a command
//! in the "Thinking and a tool" step and the wizard stopped with "no menu or yes/no question could be read from the
//! screen". These tests play the same step with the question drawn five ways (an inverse-video menu with no marker, a menu
//! in a box, a row of buttons moved with Tab, an inline `[Y]es / [N]o`, a menu whose only yes is "Always allow") and with a
//! menu no parser can read, where a stand-in for the drafter AI names the option and Rust checks it.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, WizardRig, World};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// The agent: a folder question, a thinking-and-tool turn that asks permission to run a command (the founder's step), and
/// the two permission turns. `@@STYLE@@` and `@@CURSOR@@` say how the questions are drawn.
const SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200
inline = true
approvals = "@@STYLE@@"
approval_cursor = @@CURSOR@@

[[startup]]
gate = "trust"
after_ms = 150

[[turn]]
on_prompt = "hello.txt"
steps = [
  { do = "think", ms = 1200 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [
      { do = "write_file", path = "hello.txt", content = "{last_word}\n" },
      { do = "say", text = "Created hello.txt." },
    ], on_deny = [{ do = "say", text = "Not created." }] },
]

[[turn]]
on_prompt = "shell command"
steps = [
  { do = "think", ms = 1000 },
  { do = "approve", kind = "bash", command = "echo ok", on_allow = [{ do = "say", text = "ok" }], on_deny = [{ do = "say", text = "Not run." }] },
]

[[turn]]
on_prompt = "list the files"
steps = [
  { do = "screen", text = "~ Thinking... (1s)" },
  { do = "approve", kind = "bash", command = "ls", on_allow = [
      { do = "tool", label = "ls", detail = "README.md" },
      { do = "say", text = "folder listed, code {last_word}." },
    ], on_deny = [{ do = "say", text = "Not run." }] },
]

[[turn]]
on_prompt = "whole numbers"
steps = [
  { do = "think", ms = 800 },
  { do = "lines", count = 70, template = "{n}" },
  { do = "say", text = "{last_word}" },
]

[[turn]]
on_prompt = "exactly the word"
steps = [
  { do = "think", ms = 2200 },
  { do = "say", text = "{last_word}" },
]
"#;

fn script(style: &str, cursor: usize) -> String {
    SCRIPT.replace("@@STYLE@@", style).replace("@@CURSOR@@", &cursor.to_string())
}

fn err(e: &Value) -> String {
    e["message"].as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
}

fn status(rig: &WizardRig, id: &str) -> Value {
    rig.call("capture", json!({"session": id, "op": "status"})).unwrap_or_else(|e| panic!("capture status: {}", err(&e)))
}

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == name).collect()
}

fn start_capture(rig: &WizardRig) -> String {
    // "Finish without me" on: the safe answerer is on and the AI look is allowed
    let started = rig.call("start", json!({"command": "nimbus", "autonomous": true})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().expect("session id").to_string();
    rig.call("probe", json!({"session": id})).unwrap_or_else(|e| panic!("probe: {}", err(&e)));
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap_or_else(|e| panic!("plan: {}", err(&e)));
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    rig.call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap_or_else(|e| panic!("capture start: {}", err(&e)));
    id
}

fn wizard(session: &Session, style: &str, cursor: usize) -> WizardRig {
    session.wizard_rig_with("nimbus", &script(style, cursor), |env| {
        env.visible.capture.unsure_ms = 1500;
        env.visible.capture.reply_unsure_ms = 1500;
    })
}

/// Watches a capture settle without a person: nothing but the optional `/exit` is ever pressed. A "needs you" moment the
/// wizard did not handle by itself fails the test, unless `ai` names an answer for a question the answerer could not read.
fn settle(rig: &WizardRig, id: &str, seconds: u64, mut ai: impl FnMut(&Value) -> Option<(String, Vec<String>)>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut tried_exit = false;
    let mut asked: Vec<String> = Vec::new();
    loop {
        let s = status(rig, id);
        if s["step"] == "exit" && !tried_exit && s["needs_you"].is_null() {
            tried_exit = true;
            rig.call("capture", json!({"session": id, "op": "act", "action": "try_exit"})).unwrap_or_else(|e| panic!("try_exit: {}", err(&e)));
        }
        if s["needs_you"]["unreadable"] == true {
            let fp = s["question_fp"].as_str().unwrap_or_default().to_string();
            if !asked.contains(&fp) {
                asked.push(fp.clone());
                if let Some((option, keys)) = ai(&s) {
                    let reply = rig.call("capture", json!({"session": id, "op": "menu_choice", "fingerprint": fp, "option": option, "keys": keys})).unwrap_or_else(|e| panic!("menu_choice: {}", err(&e)));
                    assert_eq!(reply["accepted"], true, "Rust refused the AI look's answer: {reply}");
                }
            }
        } else if s["needs_you"].is_object() {
            panic!("the wizard should have answered it, but it asked the person: {}\n{}", s["needs_you"], s["screen"]);
        }
        if s["settled"] == true {
            return s;
        }
        assert!(Instant::now() < deadline, "the capture did not settle in {seconds} s: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// `(flavour, label, keys, via)` of what the wizard answered.
fn answered(settled: &Value) -> Vec<(String, String, String, String)> {
    settled["answered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["flavour"].as_str().unwrap().into(),
                a["label"].as_str().unwrap().into(),
                a["keys"].as_str().unwrap().into(),
                a["via"].as_str().unwrap_or("").into(),
            )
        })
        .collect()
}

fn permission_keys(settled: &Value) -> Vec<String> {
    answered(settled).into_iter().filter(|a| a.0 == "permission").map(|a| a.2).collect()
}

fn assert_three_safe_answers(session: &Session, settled: &Value, label: &str, keys: &str) {
    let all = answered(settled);
    assert_eq!(all.len(), 4, "the folder question and three permission questions: {settled}");
    assert_eq!(all[0].0, "trust");
    for a in &all[1..] {
        assert_eq!((a.0.as_str(), a.1.as_str(), a.2.as_str()), ("permission", label, keys), "{settled}");
    }
    let approvals = events_named(session, "approval_answer");
    assert_eq!(approvals.len(), 3, "{approvals:?}");
    assert!(approvals.iter().all(|a| a["answer"] == "allow" && a["label"] == label), "the fake saw only the one-time yes: {approvals:?}");
}

#[test]
fn a_menu_whose_highlighted_row_is_inverse_video_with_no_marker_is_answered() {
    let session = World::get().session("wizard-menus-inverse");
    let rig = wizard(&session, "inverse", 2);
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 150, |_| None);
    // the cursor starts on Deny: two Up keys, then Enter, read back from the screen after each key
    assert_three_safe_answers(&session, &settled, "Allow once", "Up, Up, Enter");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_menu_drawn_in_a_box_is_answered() {
    let session = World::get().session("wizard-menus-boxed");
    let rig = wizard(&session, "boxed", 1);
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 150, |_| None);
    assert_three_safe_answers(&session, &settled, "Allow once", "Up, Enter");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_row_of_buttons_that_moves_with_tab_is_answered() {
    let session = World::get().session("wizard-menus-buttons");
    let rig = wizard(&session, "buttons", 2);
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 150, |_| None);
    // Left does nothing on this row (checked from the screen), so Tab is tried; it wraps from Deny to Allow once
    assert_three_safe_answers(&session, &settled, "Allow once", "Left, Tab, Enter");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_row_of_buttons_that_moves_with_the_arrows_is_answered() {
    let session = World::get().session("wizard-menus-buttons-arrows");
    let rig = wizard(&session, "buttons_arrows", 2);
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 150, |_| None);
    assert_three_safe_answers(&session, &settled, "Allow once", "Left, Left, Enter");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn an_inline_yes_no_question_with_bracketed_letters_is_answered_with_the_letter() {
    let session = World::get().session("wizard-menus-inline");
    let rig = wizard(&session, "inline", 0);
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 150, |_| None);
    assert_three_safe_answers(&session, &settled, "Yes", "y");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_menu_whose_only_yes_is_always_allow_is_never_pressed() {
    let session = World::get().session("wizard-menus-always-only");
    let rig = wizard(&session, "always_only", 0);
    let id = start_capture(&rig);
    let deadline = Instant::now() + Duration::from_secs(90);
    let needs = loop {
        let s = status(&rig, &id);
        if s["needs_you"]["kind"] == "permission" {
            break s["needs_you"].clone();
        }
        assert!(Instant::now() < deadline, "the person was never asked: {s}");
        std::thread::sleep(Duration::from_millis(150));
    };
    assert!(needs["message"].as_str().unwrap().contains("did not answer it for you: none of the options plainly means yes"), "{needs}");
    // the menu was READ and has no plain yes: the checked AI look may be asked (SPEC 11.22), and the options as read are listed
    assert_eq!(needs["unreadable"], true, "{needs}");
    let read = status(&rig, &id)["menu_read"].as_str().unwrap_or_default().to_string();
    assert!(read.contains("Always allow in this project\" (broad") && read.contains("Deny\" (no)"), "{read}");
    // ...but Rust refuses whatever it names: an always option, a refusal
    for option in ["Always allow in this project", "Deny"] {
        let refused = rig
            .call("capture", json!({"session": id, "op": "menu_choice", "option": option, "keys": ["Enter"]}))
            .unwrap_or_else(|e| panic!("menu_choice: {}", err(&e)));
        assert_eq!(refused["accepted"], false, "{refused}");
        assert!(refused["why"].as_str().unwrap().contains("never picks"), "{refused}");
    }
    std::thread::sleep(Duration::from_secs(4));
    assert!(events_named(&session, "approval_answer").is_empty(), "nothing pressed the always option: {:?}", events_named(&session, "approval_answer"));
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn an_unreadable_menu_is_answered_with_the_ai_looks_choice_after_rust_checks_it() {
    let session = World::get().session("wizard-menus-ai");
    let rig = wizard(&session, "blind", 0);
    let id = start_capture(&rig);
    let mut shown: Vec<String> = Vec::new();
    // the stand-in for the drafter AI: it reads the (masked) screen and names the option that means "allow once"
    let settled = settle(&rig, &id, 150, |s| {
        let screen = s["screen"].as_str().unwrap_or_default().to_string();
        assert!(screen.contains("Allow once") && screen.contains("Deny"), "{screen}");
        shown.push(screen);
        Some(("Allow once".to_string(), vec!["Enter".to_string()]))
    });
    assert_eq!(shown.len(), 3, "one look per question (the work, the file and the command questions)");
    let all = answered(&settled);
    assert_eq!(all.len(), 4, "{settled}");
    assert_eq!(all[0].3, "rules", "the folder question was read by the rules");
    for a in &all[1..] {
        assert_eq!((a.0.as_str(), a.1.as_str(), a.2.as_str(), a.3.as_str()), ("permission", "Allow once", "Enter", "ai_look"), "{settled}");
    }
    let approvals = events_named(&session, "approval_answer");
    assert_eq!(approvals.len(), 3);
    assert!(approvals.iter().all(|a| a["answer"] == "allow" && a["label"] == "Allow once"), "{approvals:?}");
    let record = rig.call("capture", json!({"session": id, "op": "status"})).unwrap();
    assert!(record["settled"] == true);
    rig.call("cancel", json!({"session": id})).unwrap();
}

/// Waits for a question the answerer could not read, and returns its status.
fn wait_unreadable(rig: &WizardRig, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let s = status(rig, id);
        if s["needs_you"]["unreadable"] == true {
            return s;
        }
        assert!(Instant::now() < deadline, "the answerer never reported an unreadable question: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[test]
fn the_ai_looks_choice_is_refused_when_it_names_a_broad_option_or_a_key_off_the_whitelist() {
    let session = World::get().session("wizard-menus-ai-refused");
    let rig = wizard(&session, "blind", 0);
    let id = start_capture(&rig);
    let s = wait_unreadable(&rig, &id);
    let fp = s["question_fp"].as_str().unwrap().to_string();
    let try_it = |option: &str, keys: &[&str]| -> Value {
        rig.call("capture", json!({"session": id, "op": "menu_choice", "fingerprint": fp, "option": option, "keys": keys})).unwrap_or_else(|e| panic!("menu_choice: {}", err(&e)))
    };
    for (option, keys, why) in [
        ("Always allow in this project", vec!["Enter"], "never picks"),
        ("Deny", vec!["Enter"], "never picks"),
        ("Allow once", vec!["Escape"], "not one the wizard may press"),
        ("Allow once", vec!["ctrl+c"], "not one the wizard may press"),
        ("Allow once", vec!["n"], "answers no"),
        ("Allow once", vec!["Down", "Down", "Down", "Down", "Down", "Down", "Down"], "at most"),
        ("Allow once and forever", vec!["Enter"], "not on the screen"),
        ("Allow once", vec!["Down"], "would not answer"),
    ] {
        let reply = try_it(option, &keys);
        assert_eq!(reply["accepted"], false, "{option} {keys:?}: {reply}");
        assert!(reply["why"].as_str().unwrap().contains(why), "{option} {keys:?}: {reply}");
    }
    std::thread::sleep(Duration::from_secs(3));
    assert!(events_named(&session, "approval_answer").is_empty(), "nothing was pressed");
    // a navigation the blind menu does not show is stopped before Enter: the keys are valid, the screen never changes
    let reply = try_it("Allow once", &["Down", "Enter"]);
    assert_eq!(reply["accepted"], true, "{reply}");
    let deadline = Instant::now() + Duration::from_secs(30);
    let declined = loop {
        let s = status(&rig, &id);
        if s["needs_you"]["kind"] == "permission" && s["needs_you"]["message"].as_str().unwrap().contains("changed nothing on the screen") {
            break s;
        }
        assert!(Instant::now() < deadline, "{s}");
        std::thread::sleep(Duration::from_millis(150));
    };
    assert_ne!(declined["needs_you"]["unreadable"], true, "no second look after a failed answer");
    assert!(events_named(&session, "approval_answer").is_empty(), "Enter was never pressed: {:?}", events_named(&session, "approval_answer"));
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn with_the_switch_off_an_unreadable_menu_gets_no_ai_look() {
    let session = World::get().session("wizard-menus-off");
    let rig = session.wizard_rig_with("nimbus", &script("blind", 0), |_| {});
    let started = rig.call("start", json!({"command": "nimbus"})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().unwrap().to_string();
    rig.call("probe", json!({"session": id})).unwrap();
    let refused = rig.call("capture", json!({"session": id, "op": "menu_choice", "option": "Allow once", "keys": ["Enter"]})).unwrap_err();
    assert_eq!(refused["code"], "WIZARD_NOT_AUTONOMOUS", "{refused}");
    rig.call("cancel", json!({"session": id})).unwrap();
}

// ---- a second permission question in the same step, worded differently (SPEC 11.22) -------------------------------------

/// The "Thinking and a tool" turn asks TWO permission questions one after the other; the second one is drawn as `second`.
fn two_questions(style: &str, cursor: usize, second: &str) -> String {
    let script = script(style, cursor);
    let from = "command = \"ls\", on_allow = [";
    assert!(script.contains(from), "the work turn of the script");
    script.replace(from, &format!("command = \"ls\" }},\n  {{ do = \"approve\", kind = \"bash\", command = \"cat README.md\", style = \"{second}\", on_allow = ["))
}

/// `(label, via, menu)` of what the wizard answered, in order.
fn answers(settled: &Value) -> Vec<(String, String, String)> {
    settled["answered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["label"].as_str().unwrap().into(), a["via"].as_str().unwrap_or("").into(), a["menu"].as_str().unwrap_or("").into()))
        .collect()
}

#[test]
fn a_second_question_with_other_wording_is_answered_by_the_rules_with_no_person_input() {
    let session = World::get().session("wizard-menus-two-run");
    // the first question is the inverse menu (cursor on Always allow: Up, Enter); the second is numbered: Run command / Always run / Cancel
    let rig = session.wizard_rig_with("nimbus", &two_questions("inverse", 0, "run_numbered"), |env| {
        env.visible.capture.unsure_ms = 1500;
        env.visible.capture.reply_unsure_ms = 1500;
    });
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 180, |_| None);
    let all = answers(&settled);
    // the folder question, the two work-step questions, then the file and the command questions
    assert_eq!(all.len(), 5, "{settled}");
    assert_eq!(all[1].0, "Allow once");
    assert_eq!((all[2].0.as_str(), all[2].1.as_str()), ("Run command", "rules"), "{settled}");
    assert!(all[2].2.contains("Run command\" (plain yes") && all[2].2.contains("Always run\" (broad)") && all[2].2.contains("Cancel\" (no)"), "the options as read: {}", all[2].2);
    let approvals = events_named(&session, "approval_answer");
    assert!(approvals.iter().all(|a| a["answer"] == "allow"), "only yeses: {approvals:?}");
    assert!(approvals.iter().any(|a| a["label"] == "Run command"), "{approvals:?}");
    assert!(approvals.iter().all(|a| a["label"] != "Always run" && a["label"] != "Cancel"), "{approvals:?}");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_second_question_whose_yes_row_the_parser_misses_is_answered_by_the_ai_look_and_verified() {
    let session = World::get().session("wizard-menus-two-ai");
    // cursor 1: the first question starts on "Always allow" (Up, Enter), the second on "Always run in this project"
    let rig = session.wizard_rig_with("nimbus", &two_questions("inverse", 1, "yes_missed"), |env| {
        env.visible.capture.unsure_ms = 1500;
        env.visible.capture.reply_unsure_ms = 1500;
    });
    let id = start_capture(&rig);
    let mut reads: Vec<String> = Vec::new();
    let settled = settle(&rig, &id, 180, |s| {
        let read = s["menu_read"].as_str().unwrap_or_default().to_string();
        let screen = s["screen"].as_str().unwrap_or_default().to_string();
        assert!(screen.contains("Yes, run command"), "{screen}");
        reads.push(read);
        Some(("Yes, run command".to_string(), vec!["Up".to_string(), "Enter".to_string()]))
    });
    assert_eq!(reads.len(), 1, "one look, for the one question the rules could not decide: {reads:?}");
    assert!(reads[0].contains("Always run in this project\" (broad, highlighted)") && !reads[0].contains("Yes, run command"), "the parser missed the yes row: {}", reads[0]);
    let all = answers(&settled);
    let by_ai: Vec<_> = all.iter().filter(|a| a.1 == "ai_look").collect();
    assert_eq!(by_ai.len(), 1, "{settled}");
    assert_eq!(by_ai[0].0, "Yes, run command");
    assert!(by_ai[0].2.contains("Always run in this project\" (broad"), "the answer keeps the menu as the parser read it: {}", by_ai[0].2);
    let approvals = events_named(&session, "approval_answer");
    assert!(approvals.iter().any(|a| a["label"] == "Yes, run command" && a["answer"] == "allow"), "{approvals:?}");
    assert!(approvals.iter().all(|a| a["answer"] == "allow"), "{approvals:?}");
    rig.call("cancel", json!({"session": id})).unwrap();
}

/// The wizard keeps the agent in its folder: the agent's "list the files" turn asks to run a command that inspects other
/// processes (the founder's run: `Get-Process`, the machine's process list, the home folder). The question is drawn as
/// that CLI draws it (a numbered menu, the broad rows long, a "No, cancel" last): the wizard presses the plain No row, the
/// fake sees a denial and never an allow for it, and the denial is in the answered list with its reason. The other
/// questions (a file write and a harmless command in the folder) are still answered "yes" once.
#[test]
fn a_command_that_inspects_other_processes_is_denied_with_the_plain_no_row_and_the_rest_is_still_allowed() {
    let script = script("bullets_numbered", 0).replace(r#"{ do = "approve", kind = "bash", command = "ls", on_allow = ["#, r#"{ do = "approve", kind = "bash", command = "Get-Process | Where-Object { $_.ProcessName -match 'node' }", on_allow = [
      { do = "say", text = "WENT OUTSIDE the folder" },"#);
    // after the denial the agent still answers the task (it does without the process list)
    let script = script.replace(
        r#"{ do = "say", text = "folder listed, code {last_word}." },
    ], on_deny = [{ do = "say", text = "Not run." }] },"#,
        r#"{ do = "say", text = "folder listed, code {last_word}." },
    ], on_deny = [{ do = "say", text = "folder listed, code {last_word}." }] },"#,
    );
    assert!(script.contains("WENT OUTSIDE"), "the script was changed");
    assert!(script.matches("folder listed").count() == 2, "the work turn answers after a denial too");
    let session = World::get().session("wizard-menus-isolation");
    let rig = session.wizard_rig_with("nimbus", &script, |env| {
        env.visible.capture.unsure_ms = 1500;
        env.visible.capture.reply_unsure_ms = 1500;
    });
    let id = start_capture(&rig);
    let settled = settle(&rig, &id, 150, |_| None);
    let approvals = events_named(&session, "approval_answer");
    assert!(approvals.iter().all(|a| a["answer"] != "allow" || a["label"] != "WENT OUTSIDE"), "{approvals:?}");
    let denies: Vec<&Value> = approvals.iter().filter(|a| a["answer"] == "deny").collect();
    assert_eq!(denies.len(), 1, "exactly one denial, the process listing: {approvals:?}");
    assert_eq!(denies[0]["label"], "No, cancel", "{approvals:?}");
    assert_eq!(approvals.iter().filter(|a| a["answer"] == "allow").count(), 2, "the file write and the harmless command: {approvals:?}");
    let all = settled["answered"].as_array().unwrap();
    let denied: Vec<&Value> = all.iter().filter(|a| a["denied"].is_string()).collect();
    assert_eq!(denied.len(), 1, "{settled}");
    assert_eq!((denied[0]["flavour"].as_str(), denied[0]["label"].as_str()), (Some("permission"), Some("No, cancel")), "{denied:?}");
    assert!(denied[0]["denied"].as_str().unwrap().contains("other processes or the machine"), "{denied:?}");
    rig.call("cancel", json!({"session": id})).unwrap();
}
