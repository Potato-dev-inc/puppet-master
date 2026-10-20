//! "Finish without me" (SPEC 11.19) end to end on real ConPTYs, with `pm-fake-tui` playing an UNKNOWN agent ("nimbus").
//! Zero tokens, and zero person input: the test never types into a pane. A stand-in drafter replays a recorded draft and
//! a stand-in classifier plays the drafter AI's one-word look at the screens the wizard cannot read; the TS loop's job
//! (poll the status, ask for a look, send the verdict, finish with the approval) is done by the small driver below, on the
//! same Rust operations the front end calls.
//!
//! 1. An agent whose ready screen the wizard cannot read (an input box next to the word "approve") and that asks the
//!    folder question: the safe answerer takes the folder question, the AI look says "ready" at each unsure screen, and
//!    the definition is written AND approved by the wizard with no click.
//! 2. An agent that shows a sign-in screen: the AI look says "sign in", the run STOPS with the plain message, nothing is
//!    typed and nothing is written or approved.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, WizardRig, World};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// The recorded draft a stand-in drafter answers with. `@@COMMAND@@` is the program the person typed.
const DRAFT: &str = include_str!("../src/tui/declarative/wizard/tests/drafts/visible_nimbus.toml");

/// The agent: a folder question, then an input box under a banner row that mentions "approve" (the wizard cannot tell
/// that screen is a ready one), a thinking-and-tool turn, a long reply and the two permission questions.
const SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200
inline = true
summary_row = "▸ Thought for {secs}s, 298 tokens"
banner_rows = ["Tip: ask me to approve a plan before I start."]

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
  { do = "tool", label = "ls", detail = "README.md" },
  { do = "think", ms = 3200 },
  { do = "say", text = "folder listed, code {last_word}." },
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

/// An agent that wants a sign-in before anything else (a code to enter on a web page).
const SIGN_IN_SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200
banner_rows = ["Visit https://example.test/device and sign in with the code ABCD-1234."]
"#;

/// The stand-in for the drafter AI's menu look: the plain yes the parser misses, and the keys that reach it.
const MENU_OPTION: &str = "Yes, run command";
const MENU_KEYS: [&str; 2] = ["Up", "Enter"];

const SIGN_IN_MESSAGE: &str = "Sign in to nimbus (or finish its setup), then press Retry.";

fn err(e: &Value) -> String {
    e["message"].as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
}

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == name).collect()
}

/// The stand-in for the drafter AI's one-word look: what a careful reader would say about the screen.
fn classify(screen: &str) -> &'static str {
    let s = screen.to_lowercase();
    if s.contains("sign in") || s.contains("log in") {
        "sign_in_or_payment"
    } else if s.contains("ctrl+c to stop") || s.contains("thinking") || s.contains("generating") {
        "working"
    } else if s.contains("| > type a task") {
        "ready_for_input"
    } else {
        "unknown"
    }
}

/// What the TS loop does while the capture runs, on the same operations: poll the status; at a "needs you" moment ask
/// for a look (once per picture) and send the verdict; ask the agent to close itself at the exit step. Returns the stop
/// message when the run was stopped. `looks` collects the screens the stand-in classifier was shown.
fn drive_capture(rig: &WizardRig, id: &str, looks: &mut Vec<String>, menu_looks: &mut usize, seconds: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut asked_exit = false;
    let mut classified: HashSet<String> = HashSet::new();
    let mut menu_asked: HashSet<String> = HashSet::new();
    loop {
        let s = rig.call("capture", json!({"session": id, "op": "status"})).map_err(|e| err(&e))?;
        if let Some(why) = s["halted"].as_str() {
            return Err(format!("halted: {why}"));
        }
        if s["settled"] == true {
            return Ok(());
        }
        if s["step"] == "exit" && s["needs_you"].is_null() && !asked_exit && s["can"].as_array().is_some_and(|c| c.iter().any(|a| a == "try_exit")) {
            asked_exit = true;
            rig.call("capture", json!({"session": id, "op": "act", "action": "try_exit", "step": "exit"})).map_err(|e| err(&e))?;
        }
        if s["needs_you"]["kind"] == "permission" && s["needs_you"]["unreadable"] == true {
            // a permission question the rules could not decide: the stand-in for the drafter AI's menu look (checked by Rust)
            let fp = s["question_fp"].as_str().unwrap_or_default().to_string();
            if menu_asked.insert(fp.clone()) {
                *menu_looks += 1;
                let reply = rig
                    .call("capture", json!({"session": id, "op": "menu_choice", "fingerprint": fp, "option": MENU_OPTION, "keys": MENU_KEYS}))
                    .map_err(|e| err(&e))?;
                assert_eq!(reply["accepted"], true, "Rust refused the menu look: {reply}");
            }
        } else if let Some(kind) = s["needs_you"]["kind"].as_str() {
            assert!(!matches!(kind, "trust" | "permission"), "the safe answerer must take the folder question; the capture asked: {s}");
            let key = format!("{}|{kind}|{}", s["step"], s["fingerprint"]);
            if classified.insert(key) {
                let screen = s["screen"].as_str().unwrap_or_default().to_string();
                let verdict = if kind == "login" { "sign_in_or_payment" } else { classify(&screen) };
                looks.push(screen);
                let reply = rig.call("capture", json!({"session": id, "op": "verdict", "verdict": verdict})).map_err(|e| err(&e))?;
                if reply["decision"] == "stop" {
                    return Err(reply["stop"]["message"].as_str().unwrap_or_default().to_string());
                }
                assert!(matches!(reply["decision"].as_str(), Some("pressed_ready" | "wait")), "unexpected decision for {verdict}: {reply}");
            }
        }
        assert!(Instant::now() < deadline, "the capture did not finish in {seconds} s: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn autonomous_rig(session: &Session, script: &str) -> WizardRig {
    // The same wizard as the product's, with shorter waits for an unsure screen (the fake is quick).
    session.wizard_rig_with("nimbus", script, |env| {
        env.visible.capture.unsure_ms = 1500;
        env.visible.capture.reply_unsure_ms = 1500;
    })
}

#[test]
fn finish_without_me_writes_and_approves_an_unknown_agent_with_zero_person_input() {
    finish_without_me_run("wizard-auto", SCRIPT, DRAFT);
}

/// The founder's run: the agent's permission questions mark the highlighted option only by inverse video (no `>`, no
/// `❯`), and the cursor starts on Deny. The wizard still finishes, APPROVED, with zero person input.
#[test]
fn finish_without_me_approves_an_agent_whose_permission_questions_use_an_inverse_highlight() {
    let script = SCRIPT.replace("inline = true
", "inline = true
approvals = \"inverse\"
approval_cursor = 2
");
    assert!(script.contains("approvals = \"inverse\""));
    // the questions have no `[y/n]` any more: the draft's own prompt table matches the question line
    let draft = DRAFT.replace(r"allow nimbus to .*\[y/n\]", "allow nimbus to (?:create|run)");
    assert_ne!(draft, DRAFT);
    finish_without_me_run("wizard-auto-inverse", &script, &draft);
}

/// The founder's second question: the "Thinking and a tool" step asks permission TWICE, the second time with other wording
/// (`Run command` / `Always run` / `Cancel`, numbered). The wizard finishes, APPROVED, with zero person input.
#[test]
fn finish_without_me_approves_an_agent_whose_work_step_asks_two_questions_worded_differently() {
    let draft = DRAFT.replace(r"allow nimbus to .*\[y/n\]", "allow nimbus to (?:create|run)");
    assert_ne!(draft, DRAFT);
    let script = two_work_questions("run_numbered");
    finish_without_me_run_with("wizard-auto-two-questions", &script, &draft, 5, false);
}

/// The same, where the second question's yes row is one the parser misses: the checked AI look answers it, in the capture
/// and in the test pane, and the run still finishes APPROVED with no person input.
#[test]
fn finish_without_me_approves_when_the_second_question_needs_the_checked_ai_look() {
    let draft = DRAFT.replace(r"allow nimbus to .*\[y/n\]", "allow nimbus to (?:create|run)");
    assert_ne!(draft, DRAFT);
    let script = two_work_questions("yes_missed");
    finish_without_me_run_with("wizard-auto-two-questions-ai", &script, &draft, 5, true);
}

/// The agent's script with an inverse-highlight menu whose cursor starts on the second row ("Always"), and two permission
/// questions in the work turn: the first drawn like every other, the second as `second`.
fn two_work_questions(second: &str) -> String {
    let script = SCRIPT.replace("inline = true
", "inline = true
approvals = \"inverse\"
approval_cursor = 1
");
    let from = "{ do = \"tool\", label = \"ls\", detail = \"README.md\" },
  { do = \"think\", ms = 3200 },";
    assert!(script.contains(from), "the work turn of the script");
    script.replace(
        from,
        &format!("{{ do = \"approve\", kind = \"bash\", command = \"ls\" }},
  {{ do = \"approve\", kind = \"bash\", command = \"cat README.md\", style = \"{second}\" }},
  {from}"),
    )
}

fn finish_without_me_run(name: &str, script: &str, recorded_draft: &str) {
    finish_without_me_run_with(name, script, recorded_draft, 3, false);
}

/// `answers`: how many questions the capture's safe answerer takes (the folder question and the permission ones).
/// `menu_looks`: the permission questions include one the rules cannot decide, so the stand-in AI look is asked in the
/// capture and in the test pane (and must be asked, at least once in each).
fn finish_without_me_run_with(name: &str, script: &str, recorded_draft: &str, answers: usize, menu_looks: bool) {
    let session = World::get().session(name);
    let rig = autonomous_rig(&session, script);

    // 1. The person typed the command; that is all they do.
    let started = rig.call("start", json!({"command": "nimbus", "autonomous": true})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().expect("session id").to_string();
    rig.call("probe", json!({"session": id})).unwrap_or_else(|e| panic!("probe: {}", err(&e)));
    let (one, two, sky) = rig.words(&id);

    // 2. The capture runs by itself. The folder question is answered by the wizard's safe answerer (never the test).
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap_or_else(|e| panic!("plan: {}", err(&e)));
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    rig.call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap_or_else(|e| panic!("capture start: {}", err(&e)));
    let mut looks: Vec<String> = Vec::new();
    let mut capture_menu_looks = 0usize;
    drive_capture(&rig, &id, &mut looks, &mut capture_menu_looks, 240).unwrap_or_else(|e| panic!("the capture stopped: {e}"));
    assert_eq!(capture_menu_looks > 0, menu_looks, "the menu look was asked in the capture exactly when a question needed it");
    assert!(!looks.is_empty() && looks.len() <= 12, "the AI look was needed (unsure screens) and stayed within 12: {}", looks.len());
    assert!(looks.iter().all(|s| s.contains("Tip: ask me to approve") || s.contains("Type a task")), "{looks:?}");
    assert!(looks.iter().all(|s| !s.contains(&one) && !s.contains(&two) && !s.contains(&sky)), "the classifier would see a test word");
    let status = rig.call("capture", json!({"session": id, "op": "status"})).unwrap();
    let flavours: Vec<&str> = status["answered"].as_array().unwrap().iter().filter_map(|a| a["flavour"].as_str()).collect();
    assert_eq!(flavours.len(), answers, "the safe answerer took the folder question and the one-time approvals: {status}");
    assert_eq!(flavours[0], "trust");
    assert!(flavours[1..].iter().all(|f| *f == "permission"), "{status}");
    let finished = rig.call("capture", json!({"session": id, "op": "finish"})).unwrap_or_else(|e| panic!("capture finish: {}", err(&e)));
    let names: Vec<String> = finished["captures"]["files"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
    for wanted in ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_permission"] {
        assert!(names.iter().any(|n| n == wanted), "{wanted} in {names:?}");
    }

    // 3. The stand-in drafter writes the recorded draft; verify; the visible test runs without anyone confirming it.
    let draft = recorded_draft.replace("@@COMMAND@@", "nimbus");
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
    let plan = verified["planned_visible"]["fingerprint"].as_str().expect("a visible plan").to_string();
    let mut test_menu_looks = 0usize;
    let test = std::thread::scope(|scope| {
        let handle = scope.spawn(|| rig.call("visibletest", json!({"session": id, "confirm": true, "plan": plan})));
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut classified: HashSet<String> = HashSet::new();
        let mut test_menu_asked: HashSet<String> = HashSet::new();
        while !handle.is_finished() {
            assert!(Instant::now() < deadline, "the visible test did not finish");
            let status = rig.call("capture", json!({"session": id, "op": "test_status"})).unwrap_or_else(|e| panic!("test_status: {}", err(&e)));
            if let Some(kind) = status["needs_you"]["kind"].as_str() {
                // a folder question is the safe answerer's; anything else would get a look
                if kind == "permission" && status["needs_you"]["unreadable"] == true {
                    // the stand-in for the drafter AI's menu look in the test pane; Rust checks it on its next look
                    if test_menu_asked.insert(status["screen"].to_string()) {
                        test_menu_looks += 1;
                        rig.call("capture", json!({"session": id, "op": "test_menu_choice", "option": MENU_OPTION, "keys": MENU_KEYS})).unwrap_or_else(|e| panic!("test_menu_choice: {}", err(&e)));
                    }
                } else if kind != "trust" && classified.insert(status["screen"].to_string()) {
                    let verdict = classify(status["screen"].as_str().unwrap_or_default());
                    rig.call("capture", json!({"session": id, "op": "test_verdict", "verdict": verdict})).unwrap_or_else(|e| panic!("test_verdict: {}", err(&e)));
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        handle.join().unwrap()
    })
    .unwrap_or_else(|e| panic!("visibletest: {}", err(&e)));
    assert_eq!(test["ok"], true, "the visible test said:\n{}\n{}", test["text"].as_str().unwrap_or_default(), test["checks"]);
    assert_eq!(test["state"], "tested");
    assert_eq!(test_menu_looks > 0, menu_looks, "the menu look was asked in the test pane exactly when a question needed it");
    assert_eq!(test["answered"].as_array().map(|a| a.len() >= 1), Some(true), "the wizard answered the folder question in the test pane too: {}", test["answered"]);

    // 4. Finish approves, because every check passed.
    let done = rig
        .call(
            "finish",
            json!({
                "session": id,
                "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "8", "rounds": 1},
                "autonomous": {"approve": true, "decisions": ["Answered the folder-trust question", "Decided the agent is ready (AI look)"]},
            }),
        )
        .unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    assert_eq!(done["id"], "nimbus");
    assert_eq!(done["approval"]["approved"], true, "the wizard did not approve: {}", done["approval"]);
    assert_eq!(done["approval"]["approved_by"], "wizard");
    let n = done["approval"]["count"].as_u64().unwrap();
    assert!(n >= 10, "{}", done["approval"]);
    let records = rig.approval_records();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!((records[0]["id"].as_str(), records[0]["approved_by"].as_str()), (Some("nimbus"), Some("wizard")));
    assert_eq!(records[0]["checks"].as_array().map(Vec::len), Some(n as usize));
    let listing = rig.listing();
    let row = listing["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "nimbus").expect("nimbus is listed");
    assert_eq!(row["state"], "active", "{row}");
    assert_eq!((row["approved_by"].as_str(), row["approval_checks"].as_u64()), (Some("wizard"), Some(n)));
    // the definition passes its own fixtures when checked from the file
    let report = rig.check_definition(&rig.agents_dir().join("nimbus.toml"));
    assert_eq!(report["status"], "ran", "{report}");
    assert!(report["results"].as_array().unwrap().iter().all(|r| r["passed"] == true), "{report}");
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(rig.agents_dir().join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["autonomous"]["approval"]["approved"], true);
    assert!(prov["autonomous"]["planned"].as_array().unwrap().iter().any(|p| p["test"] == "visible"), "{}", prov["autonomous"]);
    assert!(prov["autonomous"]["ai_looks"].as_array().is_some_and(|l| !l.is_empty()), "the looks Rust acted on are recorded");
    assert_eq!(rig.session_count(), 0);

    // 5. Zero person input: the test never typed into a pane, and the fake agent saw only the wizard's own tasks.
    let prompts: Vec<String> = events_named(&session, "prompt").iter().filter_map(|e| e["text"].as_str().map(str::to_string)).collect();
    assert!(prompts.iter().all(|p| p.starts_with("Reply with exactly the word")
        || p.starts_with("List the files")
        || p.starts_with("Create a file named hello.txt")
        || p.starts_with("Run this shell command")
        || p.starts_with("Write the whole numbers")
        || p == "/exit"), "{prompts:?}");
    assert!(!events_named(&session, "gate_answer").is_empty(), "the folder question was answered (by the wizard's safe answerer)");
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

#[test]
fn a_sign_in_screen_stops_finish_without_me_with_the_plain_message_and_nothing_is_written_or_approved() {
    let session = World::get().session("wizard-auto-signin");
    let rig = autonomous_rig(&session, SIGN_IN_SCRIPT);
    let started = rig.call("start", json!({"command": "nimbus", "autonomous": true})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().expect("session id").to_string();
    rig.call("probe", json!({"session": id})).unwrap();
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap();
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    rig.call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap();

    let mut looks = Vec::new();
    let stopped = drive_capture(&rig, &id, &mut looks, &mut 0usize, 120).expect_err("the run must stop at the sign-in screen");
    assert_eq!(stopped, SIGN_IN_MESSAGE);
    assert_eq!(looks.len(), 1, "one look was enough: {looks:?}");
    assert!(looks[0].contains("sign in with the code"), "{}", looks[0]);
    let status = rig.call("capture", json!({"session": id, "op": "status"})).unwrap();
    assert_eq!(status["halted"], "sign_in");

    // nothing is typed or pressed afterwards, however long it waits
    std::thread::sleep(Duration::from_secs(4));
    assert!(events_named(&session, "prompt").is_empty(), "the wizard typed into a sign-in screen: {:?}", events_named(&session, "prompt"));
    assert!(events_named(&session, "gate_answer").is_empty());
    // nothing was drafted, written or approved
    assert!(!rig.agents_dir().join("nimbus.toml").exists());
    assert!(rig.approval_records().is_empty());
    let verify = rig.call("verify", json!({"session": id, "toml": DRAFT.replace("@@COMMAND@@", "nimbus")})).unwrap();
    assert_eq!(verify["ok"], false, "a sign-in run has no screens to check a draft against; it never gets this far");

    // the pane stays open for the person to sign in; closing the wizard closes it
    rig.call("cancel", json!({"session": id})).unwrap();
    session.until(20, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("the pane is still open: {:?}", session.pane_ids()));
    assert_eq!(rig.session_count(), 0);
}
