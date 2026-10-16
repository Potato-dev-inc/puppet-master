//! The Agent Wizard's visible path end to end, with `pm-fake-tui` playing an UNKNOWN agent ("nimbus": no
//! profile in the app knows its screens) on a real ConPTY. Zero tokens: capture -> a stand-in drafter that
//! replays a recorded draft -> verify -> visible test -> finish, and the written definition must pass
//! `check_report` including its `tui_*` fixtures.
//!
//! The test plays the PERSON: it answers the folder question and the permission question by typing into the
//! pane as a user. The wizard itself never presses a key into either (its capture has no way to), and this
//! test checks that too, from the fake agent's own event log.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// The recorded draft a stand-in drafter answers with. `@@COMMAND@@` is the program the person typed.
const DRAFT: &str = include_str!("../src/tui/declarative/wizard/tests/drafts/visible_nimbus.toml");

const SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200

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
on_prompt = "exactly the word"
steps = [
  { do = "think", ms = 2200 },
  { do = "say", text = "{last_word}" },
]
"#;

fn err(e: &Value) -> String {
    e["message"].as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
}

fn status(rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str) -> Value {
    rig.call("capture", json!({"session": id, "op": "status"})).unwrap_or_else(|e| panic!("capture status: {}", err(&e)))
}

/// The person at the pane: answers what the pane asks, once each, and asks for `/exit` at the exit step.
/// Returns what it answered (so the test can say what the WIZARD did not do).
fn play_the_person(session: &Session, rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str, seconds: u64) -> Vec<String> {
    let mut answered: HashSet<String> = HashSet::new();
    let mut log = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let s = status(rig, id);
        let pane = s["pane"].as_str().unwrap_or_default().to_string();
        if let Some(kind) = s["needs_you"]["kind"].as_str() {
            let once = format!("{}-{kind}", s["step"].as_str().unwrap_or(""));
            if !answered.contains(&once) {
                let (keys, what) = match kind {
                    "trust" => ("1", "trusted the folder"),
                    "permission" => ("y", "allowed the file"),
                    other => panic!("an unexpected question ({other}): {s}"),
                };
                // The person presses the key in the pane: the same function a click or a keystroke in the window uses.
                session.type_as_user(&pane, keys, false).expect("the person types");
                answered.insert(once);
                log.push(what.to_string());
            }
        }
        if s["step"] == "exit" && !answered.contains("exit") {
            answered.insert("exit".into());
            rig.call("capture", json!({"session": id, "op": "act", "action": "try_exit"})).unwrap_or_else(|e| panic!("try_exit: {}", err(&e)));
            log.push("asked the wizard to try /exit".into());
        }
        if s["settled"] == true {
            return log;
        }
        assert!(Instant::now() < deadline, "the capture did not settle in {seconds} s: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == name).collect()
}

#[test]
fn the_wizard_learns_an_unknown_visible_agent_from_its_screens_and_writes_a_definition_that_passes() {
    let session = World::get().session("wizard-visible");
    let rig = session.wizard_rig("nimbus", SCRIPT);

    // 1. Resolve and probe: the typed command is found on PATH.
    let started = rig.call("start", json!({"command": "nimbus"})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().expect("session id").to_string();
    assert_eq!(started["id_suggestion"], "nimbus");
    rig.call("probe", json!({"session": id})).unwrap_or_else(|e| panic!("probe: {}", err(&e)));
    let (one, two, sky) = rig.words(&id);

    // 2. The person sees the exact command and confirms it; the pane starts at the fixed grid in an isolated folder.
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap_or_else(|e| panic!("plan: {}", err(&e)));
    assert_eq!(plan["plan"]["grid"], json!({"cols": 120, "rows": 40}));
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    let first = rig
        .call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint}))
        .unwrap_or_else(|e| panic!("capture start: {}", err(&e)));
    let pane = first["pane"].as_str().expect("a pane").to_string();
    assert_eq!(first["grid"], json!({"cols": 120, "rows": 40}));
    let grid = session.pane_ids();
    assert!(grid.contains(&pane));

    // 3. The guided capture, with the test as the person.
    let did = play_the_person(&session, &rig, &id, 120);
    assert_eq!(did, ["trusted the folder", "allowed the file", "asked the wizard to try /exit"], "{did:?}");
    // The fake's own log is the proof of what the WIZARD typed: exactly the two tasks and `/exit`, as prompts.
    let prompts: Vec<String> = events_named(&session, "prompt").iter().filter_map(|e| e["text"].as_str().map(str::to_string)).collect();
    assert_eq!(prompts.len(), 3, "{prompts:?}");
    assert_eq!(prompts[0], format!("Reply with exactly the word: {one}"));
    assert!(prompts[1].starts_with("Create a file named hello.txt") && prompts[1].contains(&sky), "{}", prompts[1]);
    assert_eq!(prompts[2], "/exit");
    // It answered no question: the only answers the fake saw were the person's (one gate answer, one approval).
    assert_eq!(events_named(&session, "gate_answer").len(), 1);
    assert_eq!(events_named(&session, "approval_answer").len(), 1);
    assert_eq!(events_named(&session, "approval_answer")[0]["answer"], "allow");
    assert_eq!(std::fs::read_to_string(session.project().join("hello.txt")).ok(), None, "the agent ran in an isolated folder, not the project");

    let finished = rig.call("capture", json!({"session": id, "op": "finish"})).unwrap_or_else(|e| panic!("capture finish: {}", err(&e)));
    let files = finished["captures"]["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().filter_map(|f| f["name"].as_str()).collect();
    assert_eq!(names, ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_permission"], "{finished}");
    let shown = finished["captures"].to_string();
    assert!(!shown.contains(&one) && !shown.contains(&two) && !shown.contains(&sky), "the AI would see a test word: {shown}");
    assert_eq!(finished["captures"]["facts"]["exit_method"], "slash_exit");
    assert!(!rig.is_registered(&format!("wzc_{}", id.trim_start_matches("wz_"))), "the capture's transient profile is gone");

    // 4. The stand-in drafter answers with the recorded draft; verify checks it against the captures.
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus");
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
    assert_eq!(verified["checked"]["fixtures"], 5);

    // 5. The person confirms the visible test; it starts a NEW pane from the drafted profile. The folder question comes
    //    again (a new isolated folder): the person answers it, the wizard does not.
    let prompts_before = events_named(&session, "prompt").len();
    let plan = verified["planned_visible"]["fingerprint"].as_str().expect("a visible plan").to_string();
    let test = std::thread::scope(|scope| {
        let handle = scope.spawn(|| rig.call("visibletest", json!({"session": id, "confirm": true, "plan": plan})));
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut trusted = false;
        while !handle.is_finished() {
            assert!(Instant::now() < deadline, "the visible test did not finish");
            if !trusted {
                let status = rig
                    .call("capture", json!({"session": id, "op": "test_status"}))
                    .unwrap_or_else(|e| panic!("test_status: {}", err(&e)));
                if status["needs_you"]["message"].as_str().is_some_and(|m| m.contains("trust")) {
                    let pane = session.pane_ids().into_iter().find(|p| !grid.contains(p)).expect("the test pane");
                    session.type_as_user(&pane, "1", false).expect("the person types");
                    trusted = true;
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        handle.join().unwrap()
    })
    .unwrap_or_else(|e| panic!("visibletest: {}", err(&e)));
    assert_eq!(test["ok"], true, "the visible test said:\n{}\n{}", test["text"].as_str().unwrap_or_default(), test["checks"]);
    assert_eq!(test["state"], "tested");
    assert_eq!((test["passed"].as_u64(), test["total"].as_u64()), (Some(7), Some(7)), "{}", test["checks"]);
    let prompts_after: Vec<String> = events_named(&session, "prompt").iter().skip(prompts_before).filter_map(|e| e["text"].as_str().map(str::to_string)).collect();
    assert_eq!(prompts_after.len(), 2, "{prompts_after:?}");
    assert_eq!(prompts_after[0], format!("Reply with exactly the word: {two}"));
    assert!(prompts_after[1].contains("hello.txt"));
    assert_eq!(events_named(&session, "approval_answer").len(), 1, "the permission check answered nothing: only the person's earlier answer exists");
    assert!(session.pane_ids().iter().all(|p| !p.is_empty()));

    // 6. Finish writes an unapproved definition with its screen fixtures, and the fixtures pass.
    let done = rig
        .call("finish", json!({"session": id, "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "5", "rounds": 1}}))
        .unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    assert_eq!(done["id"], "nimbus");
    assert_eq!(done["state"], "needs_approval");
    assert_eq!(done["fixtures"], json!({"passed": 5, "total": 5}));
    let file = rig.agents_dir().join("nimbus.toml");
    let report = rig.check_definition(&file);
    assert_eq!(report["status"], "ran", "{report}");
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 5, "{report}");
    assert!(results.iter().all(|r| r["passed"] == true), "{report}");
    let reply = std::fs::read_to_string(rig.agents_dir().join("nimbus").join("tui_reply.txt")).unwrap();
    assert!(reply.contains(&one) && reply.starts_with("# capture: nimbus"), "{reply}");
    assert_eq!(rig.session_count(), 0);
    // nothing is left running or registered by the wizard
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

#[test]
fn a_capture_that_is_cancelled_closes_its_pane() {
    let session = World::get().session("wizard-visible-cancel");
    let rig = session.wizard_rig("nimbus", SCRIPT);
    let started = rig.call("start", json!({"command": "nimbus"})).unwrap();
    let id = started["session"].as_str().unwrap().to_string();
    rig.call("probe", json!({"session": id})).unwrap();
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap();
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    rig.call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap();
    session.until(20, || (!session.pane_ids().is_empty()).then_some(())).expect("the pane exists");
    // the folder question is on screen, nobody answers it, the person gives up
    session.until(20, || (status(&rig, &id)["needs_you"]["kind"] == "trust").then_some(())).expect("the question is shown");
    assert!(events_named(&session, "gate_answer").is_empty());
    rig.call("cancel", json!({"session": id})).unwrap();
    session.until(20, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("the pane is still open: {:?}", session.pane_ids()));
    assert!(events_named(&session, "gate_answer").is_empty(), "cancel answered nothing");
    assert_eq!(rig.session_count(), 0);
}
