//! "Finish without me" (SPEC 11.19), the wizard session's side: the bypass deny list, the wizard's own approval
//! (only when every check passed, marked as the wizard's, revocable like any other), no approval on any failure,
//! and the stop on a sign-in screen. Scripted panes, zero tokens.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::tui::declarative::loader::LoadInputs;
use crate::tui::declarative::service;
use crate::tui::declarative::wizard::Wizard;

use super::fake_host::{run_visible_test, wait_for, FakeHost};
use super::support::{call, ok, probed, provenance, visible_draft, with_captures, Tree, GOOD};

fn autonomous_session(tree: &Tree) -> (Arc<Wizard>, Arc<FakeHost>, String) {
    let (wizard, host) = tree.visible_wizard();
    let wizard = Arc::new(wizard);
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "autonomous": true}));
    let id = started["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    (wizard, host, id)
}

fn plain_session(tree: &Tree) -> (Arc<Wizard>, Arc<FakeHost>, String) {
    let (wizard, host) = tree.visible_wizard();
    let wizard = Arc::new(wizard);
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    (wizard, host, id)
}

fn with_flag(flag: &str) -> String {
    visible_draft("fake-agent").replace("argv = []", &format!("argv = [\"{flag}\"]"))
}

fn codes(v: &Value) -> Vec<String> {
    v["errors"].as_array().unwrap().iter().map(|e| e["code"].as_str().unwrap_or("").to_string()).collect()
}

/// verify, run the visible test and return the finish body for it.
fn tested(wizard: &Arc<Wizard>, host: &Arc<FakeHost>, id: &str) {
    let v = ok(wizard, "verify", json!({"session": id, "toml": visible_draft("fake-agent")}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    let live = run_visible_test(wizard, host, id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(live["ok"], true, "{}\n{}", live["text"], live["checks"]);
}

fn approvals_file(tree: &Tree) -> std::path::PathBuf {
    tree.root.join("agent-approvals.json")
}

fn approval_records(tree: &Tree) -> Vec<Value> {
    match std::fs::read_to_string(approvals_file(tree)) {
        Ok(text) => serde_json::from_str::<Value>(&text).unwrap()["approvals"].as_array().cloned().unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn listing(tree: &Tree) -> Value {
    let inputs = LoadInputs { bundled: &[], user_dir: Some(tree.agents.as_path()), code_ids: &["claude", "codex"], projects: &[], extra: &[] };
    service::listing(&inputs, &approvals_file(tree))
}

fn finish_body(id: &str, approve: bool) -> Value {
    json!({"session": id, "provenance": provenance(), "autonomous": {"approve": approve, "decisions": ["Answered the folder-trust question", "Decided the agent is ready (AI look)"]}})
}

// ---- the deny list ------------------------------------------------------------------------------------

#[test]
fn a_bypass_flag_in_a_draft_is_refused_and_goes_back_to_the_drafter_with_what_to_remove() {
    let tree = Tree::new("auto-deny");
    let (wizard, host, id) = autonomous_session(&tree);
    for flag in ["--yolo", "--dangerously-skip-permissions", "--no-sandbox", "--trust-all-tools", "--auto-approve", "--force"] {
        let v = ok(&wizard, "verify", json!({"session": id, "toml": with_flag(flag)}));
        assert_eq!(v["ok"], false, "{flag}");
        assert!(codes(&v).contains(&"WIZARD_AUTO_BYPASS_FLAG".to_string()), "{flag}: {:?}", codes(&v));
        let text = v["text"].as_str().unwrap();
        assert!(text.contains(&format!("remove {flag}; the wizard never runs agents with permission bypasses")), "{flag}: {text}");
        assert!(v.get("planned_visible").is_none() && v.get("planned").is_none(), "a refused draft is never planned, so never tested: {flag}");
    }
    assert!(host.spawned().is_empty(), "nothing was started");
    // and the refused draft is not kept as a draft to test
    let err = call(&wizard, "visibletest", json!({"session": id, "confirm": true, "plan": "x"})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
}

#[test]
fn a_bypass_flag_in_the_one_task_part_is_refused_too() {
    let tree = Tree::new("auto-deny-headless");
    let (wizard, _host, id) = autonomous_session(&tree);
    let draft = GOOD.replace("argv = [\"run\", \"--format\", \"json\"]", "argv = [\"run\", \"--format\", \"json\", \"--dangerously-skip-permissions\"]");
    assert_ne!(draft, GOOD, "the test draft changed");
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    assert!(codes(&v).contains(&"WIZARD_AUTO_BYPASS_FLAG".to_string()), "{:?}", codes(&v));
    assert!(v.get("planned").is_none());
}

#[test]
fn an_environment_variable_that_turns_asking_off_is_refused() {
    let tree = Tree::new("auto-deny-env");
    let (wizard, _host, id) = autonomous_session(&tree);
    let draft = format!("{}\n[env]\nset = {{ YOLO_MODE = \"1\" }}\n", visible_draft("fake-agent").trim_end());
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    assert!(codes(&v).contains(&"WIZARD_AUTO_BYPASS_FLAG".to_string()), "{}", v["text"]);
}

#[test]
fn without_the_switch_the_same_draft_is_checked_as_before() {
    let tree = Tree::new("auto-deny-off");
    let (wizard, _host, id) = plain_session(&tree);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": with_flag("--yolo")}));
    assert!(!codes(&v).contains(&"WIZARD_AUTO_BYPASS_FLAG".to_string()), "{:?}", codes(&v));
    assert!(v["planned_visible"]["fingerprint"].is_string(), "the person confirms it as today");
}

#[test]
fn the_test_start_refuses_a_bypass_plan_even_if_a_draft_got_past_verify() {
    let tree = Tree::new("auto-deny-start");
    let (wizard, host, id) = autonomous_session(&tree);
    // a draft that never went through the autonomous verify (stored directly), with a confirmed fingerprint
    let text = with_flag("--yolo");
    let plan = wizard
        .read(&id, |s| {
            let def = crate::tui::declarative::schema::parse_definition(&text).unwrap();
            crate::tui::declarative::wizard::visible::livetest::plan(s, &def).unwrap().fingerprint
        })
        .unwrap();
    wizard
        .end(&id, |s| {
            s.draft = Some(crate::tui::declarative::wizard::session::Draft {
                text: text.clone(),
                sha256: crate::tui::declarative::wizard::session::sha256_hex(text.as_bytes()),
                expects: Default::default(),
            })
        })
        .unwrap();
    let err = call(&wizard, "visibletest", json!({"session": id, "confirm": true, "plan": plan})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_AUTO_BYPASS_REFUSED"));
    assert!(err.message.contains("remove --yolo"), "{}", err.message);
    assert!(host.spawned().is_empty(), "no pane was started");
}

#[test]
fn the_capture_start_refuses_a_bypass_argument_in_an_autonomous_session() {
    let tree = Tree::new("auto-deny-capture");
    let (wizard, host) = tree.visible_wizard();
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "autonomous": true}));
    let id = started["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    let args = json!(["--dangerously-skip-permissions"]);
    let plan = ok(&wizard, "capture", json!({"session": id, "op": "plan", "args": args}))["plan"]["fingerprint"].as_str().unwrap().to_string();
    let err = call(&wizard, "capture", json!({"session": id, "op": "start", "args": args, "confirm": true, "plan": plan})).unwrap_err();
    assert_eq!(err.code, "WIZARD_AUTO_BYPASS_REFUSED");
    assert!(host.spawned().is_empty());
}

// ---- the wizard's own approval -------------------------------------------------------------------------

#[test]
fn when_every_check_passed_the_wizard_approves_through_the_normal_record_and_says_so() {
    let tree = Tree::new("auto-approve");
    let (wizard, host, id) = autonomous_session(&tree);
    tested(&wizard, &host, &id);
    let done = ok(&wizard, "finish", finish_body(&id, true));
    let approval = &done["approval"];
    assert_eq!(approval["approved"], true, "{approval}");
    assert_eq!(approval["approved_by"], "wizard");
    let n = approval["count"].as_u64().unwrap() as usize;
    assert!(n >= 9, "{approval}");
    assert_eq!(approval["checks"].as_array().unwrap().len(), n);
    assert!(approval["checks"].as_array().unwrap().iter().any(|c| c.as_str().unwrap().starts_with("Visible pane: ")));

    // the same record a person's approval makes, plus who and what
    let records = approval_records(&tree);
    assert_eq!(records.len(), 1, "{records:?}");
    let r = &records[0];
    assert_eq!((r["id"].as_str(), r["approved_by"].as_str()), (Some("nimbus"), Some("wizard")));
    assert_eq!(r["checks"].as_array().unwrap().len(), n);
    assert!(r["fingerprint"].as_str().is_some_and(|f| !f.is_empty()) && r["binary"].as_str().is_some_and(|b| !b.is_empty()) && r["approved_at"].is_string());
    assert_eq!(r["path"].as_str().unwrap().replace('\\', "/").to_lowercase(), done["file"].as_str().unwrap().replace('\\', "/").to_lowercase());

    // Settings reads it from the listing: active, and "approved by the wizard after N checks"
    let list = listing(&tree);
    let row = list["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "nimbus").unwrap();
    assert_eq!(row["state"], "active", "{row}");
    assert_eq!(row["approved_by"], "wizard");
    assert_eq!(row["approval_checks"].as_u64().unwrap() as usize, n);

    // <id>.wizard.json keeps the decisions, the exact command and the outcome
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(tree.agents.join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["autonomous"]["enabled"], true);
    assert_eq!(prov["autonomous"]["decisions"], json!(["Answered the folder-trust question", "Decided the agent is ready (AI look)"]));
    assert_eq!(prov["autonomous"]["approval"]["approved"], true);
    let planned = prov["autonomous"]["planned"].as_array().unwrap();
    assert!(planned.iter().any(|p| p["test"] == "visible" && p["args"] == json!([])), "{planned:?}");

    // revoking works as for any approval: the agent goes back to "needs approval"
    service::revoke(&approvals_file(&tree), "nimbus").unwrap();
    assert!(approval_records(&tree).is_empty());
    let list = listing(&tree);
    let row = list["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "nimbus").unwrap();
    assert_eq!(row["state"], "needs_approval");
    assert!(row["approved_by"].is_null());
}

#[test]
fn nothing_is_approved_when_the_tests_did_not_pass() {
    let tree = Tree::new("auto-no-tests");
    let (wizard, _host, id) = autonomous_session(&tree);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": visible_draft("fake-agent")}));
    assert_eq!(v["ok"], true);
    // verified but never tested: finish itself refuses, and no approval exists
    let err = call(&wizard, "finish", finish_body(&id, true)).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
    assert!(approval_records(&tree).is_empty());
    assert!(!tree.agents.join("nimbus.toml").exists(), "nothing was written either");
}

#[test]
fn nothing_is_approved_when_the_checker_still_had_a_problem_with_the_draft() {
    let tree = Tree::new("auto-no-verify");
    let (wizard, host, id) = autonomous_session(&tree);
    tested(&wizard, &host, &id);
    wizard.end(&id, |s| s.verified_sha256 = None).unwrap();
    let done = ok(&wizard, "finish", finish_body(&id, true));
    assert_eq!(done["approval"]["approved"], false, "{}", done["approval"]);
    assert!(done["approval"]["reason"].as_str().unwrap().contains("checker"), "{}", done["approval"]);
    assert!(approval_records(&tree).is_empty(), "no record");
    assert!(tree.agents.join("nimbus.toml").is_file(), "the definition is still written, unapproved");
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(tree.agents.join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["autonomous"]["approval"]["approved"], false);
}

#[test]
fn nothing_is_approved_when_a_visible_check_did_not_pass() {
    let tree = Tree::new("auto-no-check");
    let (wizard, host, id) = autonomous_session(&tree);
    tested(&wizard, &host, &id);
    wizard.end(&id, |s| s.visible_tested.as_mut().unwrap().passed -= 1).unwrap();
    let done = ok(&wizard, "finish", finish_body(&id, true));
    assert_eq!(done["approval"]["approved"], false, "{}", done["approval"]);
    assert!(done["approval"]["reason"].as_str().unwrap().contains("visible checks"), "{}", done["approval"]);
    assert!(approval_records(&tree).is_empty());
}

#[test]
fn nothing_is_approved_when_the_caller_did_not_ask_for_it() {
    let tree = Tree::new("auto-not-asked");
    let (wizard, host, id) = autonomous_session(&tree);
    tested(&wizard, &host, &id);
    let done = ok(&wizard, "finish", finish_body(&id, false));
    assert_eq!(done["approval"]["approved"], false);
    assert!(approval_records(&tree).is_empty());
}

#[test]
fn a_session_without_the_switch_is_never_approved_by_the_wizard_whatever_the_request_says() {
    let tree = Tree::new("auto-off");
    let (wizard, host) = tree.visible_wizard();
    let wizard = Arc::new(wizard);
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    tested(&wizard, &host, &id);
    let done = ok(&wizard, "finish", finish_body(&id, true));
    assert!(done.get("approval").is_none(), "{done}");
    assert!(approval_records(&tree).is_empty());
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(tree.agents.join("nimbus.wizard.json")).unwrap()).unwrap();
    assert!(prov.get("autonomous").is_none(), "today's record is unchanged");
}

// ---- the AI look on a capture ---------------------------------------------------------------------------

const SIGN_IN_SCREEN: &str = "NIMBUS\n\nVisit https://example.test/device and enter the code ABCD-1234.\nWaiting for you to finish...\n";

fn start_capture(wizard: &Wizard, id: &str) {
    let plan = ok(wizard, "capture", json!({"session": id, "op": "plan"}))["plan"]["fingerprint"].as_str().unwrap().to_string();
    ok(wizard, "capture", json!({"session": id, "op": "start", "confirm": true, "plan": plan}));
}

#[test]
fn a_sign_in_look_stops_the_capture_with_the_plain_message_and_nothing_is_typed() {
    let tree = Tree::new("auto-signin");
    let (wizard, host) = tree.visible_wizard();
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "autonomous": true}));
    let id = started["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    start_capture(&wizard, &id);
    host.set_screen(SIGN_IN_SCREEN);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let out = ok(&wizard, "capture", json!({"session": id, "op": "verdict", "verdict": "sign_in_or_payment"}));
    assert_eq!(out["decision"], "stop");
    assert_eq!(out["stop"]["code"], "WIZARD_SIGN_IN_NEEDED");
    assert_eq!(out["stop"]["message"], "Sign in to fake-agent (or finish its setup), then press Retry.");
    assert_eq!(out["status"]["halted"], "sign_in");
    // a ready-looking screen afterwards still gets nothing: no message typed, no key pressed
    host.set_screen(super::fake_host::screens::IDLE);
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(host.delivered().is_empty(), "{:?}", host.delivered());
    assert!(host.pressed().is_empty());
    // the verdicts Rust acted on are kept for the record
    let looks = wizard.read(&id, |s| s.visible.auto.verdicts.lock().clone()).unwrap();
    assert_eq!(looks, vec![json!({"target": "capture", "verdict": "sign_in_or_payment", "decision": "stop"})]);
    ok(&wizard, "cancel", json!({"session": id}));
}

#[test]
fn the_ai_look_is_only_taken_in_a_session_started_with_the_switch() {
    let tree = Tree::new("auto-look-off");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let err = call(&wizard, "capture", json!({"session": id, "op": "verdict", "verdict": "ready_for_input"})).unwrap_err();
    assert_eq!(err.code, "WIZARD_NOT_AUTONOMOUS");
    let err = call(&wizard, "capture", json!({"session": id, "op": "test_verdict", "verdict": "working"})).unwrap_err();
    assert_eq!(err.code, "WIZARD_NOT_AUTONOMOUS");
}

#[test]
fn an_unknown_verdict_word_is_refused() {
    let tree = Tree::new("auto-look-bad");
    let (wizard, _host, id) = autonomous_session(&tree);
    let err = call(&wizard, "capture", json!({"session": id, "op": "verdict", "verdict": "press y"})).unwrap_err();
    assert_eq!(err.status, 400);
}

#[test]
fn a_ready_look_on_an_unsure_screen_lets_the_capture_type_the_test_message() {
    let tree = Tree::new("auto-look-ready");
    let (wizard, host) = tree.visible_wizard();
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "autonomous": true}));
    let id = started["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    start_capture(&wizard, &id);
    host.set_screen("NIMBUS 0.4\n\nTip: ask me to approve a plan before I start.\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n");
    let unsure = wait_for(20, || {
        let s = ok(&wizard, "capture", json!({"session": id, "op": "status"}));
        (s["needs_you"]["kind"] == "unsure").then_some(s)
    })
    .expect("the capture asks: unsure");
    assert!(unsure["can"].as_array().unwrap().iter().any(|c| c == "ready"));
    assert!(host.delivered().is_empty(), "nothing typed while unsure");
    let out = ok(&wizard, "capture", json!({"session": id, "op": "verdict", "verdict": "ready_for_input"}));
    assert_eq!(out["decision"], "pressed_ready");
    assert!(wait_for(10, || (!host.delivered().is_empty()).then_some(())).is_some(), "the test message was typed after the look");
    ok(&wizard, "cancel", json!({"session": id}));
}

// ---- the AI look in the visible test -------------------------------------------------------------------

use crate::tui::declarative::wizard::visible::answer::AnswerKey;
use crate::tui::declarative::wizard::WizardError;

use super::fake_host::{screens, script_agent};

const FILE_QUESTION: &str = "NIMBUS\n\nLet nimbus read and edit the files in this folder?\n\n\u{25CF} Yes\n  No\n";
const UPDATE_PROMPT: &str = "NIMBUS\n\nA new version is available. Update now?\n\n\u{25CF} Yes\n  No\n";

struct Test {
    wizard: Arc<Wizard>,
    host: Arc<FakeHost>,
    id: String,
    run: Option<std::thread::JoinHandle<Result<Value, WizardError>>>,
    _tree: Tree,
}

/// A visible test running in a pane that shows `first`. `autonomous` is the first-screen switch.
fn running_test(name: &str, autonomous: bool, first: &str) -> Test {
    let tree = Tree::new(name);
    let (wizard, host) = tree.visible_wizard_with(|env| {
        env.visible.window = true; // the product opens pop-out windows; "Finish without me" must not
        env.visible.capture.question_ms = 300;
        env.visible.test.ready = std::time::Duration::from_secs(6);
    });
    let wizard = Arc::new(wizard);
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "autonomous": autonomous}));
    let id = started["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    // the capture saw no startup question here, so the test does not look for the startup card
    wizard.read(&id, |s| s.visible.captures.lock().as_mut().unwrap().frames.retain(|f| f.name != "tui_trust")).unwrap();
    // the draft knows the file question (as a drafter that saw it would): the startup card check passes
    let draft = visible_draft("fake-agent").replace("do you trust the files in this folder", "read and edit the files in this folder");
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    let plan = v["planned_visible"]["fingerprint"].as_str().unwrap().to_string();
    script_agent(&host, "");
    let (w, i) = (wizard.clone(), id.clone());
    let run = std::thread::spawn(move || call(&w, "visibletest", json!({"session": i, "confirm": true, "plan": plan})));
    wait_for(10, || (!host.spawned().is_empty()).then_some(())).expect("the test pane is started");
    host.set_screen(first);
    Test { wizard, host, id, run: Some(run), _tree: tree }
}

impl Test {
    fn status(&self) -> Value {
        ok(&self.wizard, "capture", json!({"session": self.id, "op": "test_status"}))
    }

    fn needs(&self) -> Option<Value> {
        wait_for(10, || {
            let s = self.status();
            s["needs_you"].is_object().then(|| s["needs_you"].clone())
        })
    }

    fn look(&self, verdict: &str) -> Value {
        ok(&self.wizard, "capture", json!({"session": self.id, "op": "test_verdict", "verdict": verdict}))
    }

    fn finish(&mut self) -> Value {
        self.run.take().unwrap().join().unwrap().unwrap_or_else(|e| panic!("{e}"))
    }
}

#[test]
fn an_unrecognised_screen_in_the_test_pane_is_a_needs_you_moment_only_with_the_switch_on() {
    let mut off = running_test("auto-test-off", false, UPDATE_PROMPT);
    std::thread::sleep(std::time::Duration::from_millis(1200));
    assert!(off.status()["needs_you"].is_null(), "today's behaviour: the test just waits");
    off.host.set_screen(screens::IDLE);
    off.finish();

    let mut on = running_test("auto-test-on", true, UPDATE_PROMPT);
    let needs = on.needs().expect("a needs-you moment");
    assert_eq!(needs["kind"], "unsure");
    on.host.set_screen(screens::IDLE);
    on.finish();
}

#[test]
fn the_ai_look_hands_a_file_question_in_the_test_pane_to_the_safe_answerer() {
    let mut t = running_test("auto-test-trust", true, FILE_QUESTION);
    t.host.on_answer(|h, pane, keys| {
        if keys.contains(&AnswerKey::Enter) {
            h.set_screen_of(pane, screens::IDLE);
        }
    });
    t.needs().expect("a needs-you moment");
    let out = t.look("question_trust_or_once");
    assert_eq!(out["decision"], "safe_answerer");
    assert!(wait_for(10, || (t.host.pressed() == vec![AnswerKey::Enter]).then_some(())).is_some(), "{:?}", t.host.pressed());
    let done = t.finish();
    assert_eq!(done["ok"], true, "{}\n{}", done["text"], done["checks"]);
    assert_eq!(done["answered"].as_array().unwrap().len(), 1);
}

#[test]
fn the_ai_look_cannot_make_the_wizard_answer_an_update_prompt() {
    let mut t = running_test("auto-test-update", true, UPDATE_PROMPT);
    t.needs().expect("a needs-you moment");
    assert_eq!(t.look("question_trust_or_once")["decision"], "safe_answerer");
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(t.host.pressed().is_empty(), "the screen does not talk about trust or a permission: {:?}", t.host.pressed());
    // an unreadable screen: wait, then the round fails with the reason, never a key
    assert_eq!(t.look("question_other")["decision"], "wait_then_skip");
    ok(&t.wizard, "capture", json!({"session": t.id, "op": "test_act", "action": "stop", "reason": "the screen is a question the wizard may not answer"}));
    let done = t.finish();
    assert_eq!(done["ok"], false);
    assert!(done["text"].as_str().unwrap().contains("The wizard stopped the test: the screen is a question the wizard may not answer"), "{}", done["text"]);
    assert!(t.host.pressed().is_empty());
}

#[test]
fn a_sign_in_look_in_the_test_pane_stops_the_test_with_the_reason() {
    let mut t = running_test("auto-test-signin", true, "NIMBUS\n\nSign in at https://example.test/device with code ABCD-1234 to go on.\n");
    t.needs().expect("a needs-you moment");
    let out = t.look("sign_in_or_payment");
    assert_eq!((out["decision"].as_str(), out["stop"]["code"].as_str()), (Some("stop"), Some("WIZARD_SIGN_IN_NEEDED")));
    assert_eq!(out["stop"]["message"], "Sign in to fake-agent (or finish its setup), then press Retry.");
    let done = t.finish();
    assert_eq!(done["ok"], false);
    assert!(done["text"].as_str().unwrap().contains("sign-in or a payment step"), "{}", done["text"]);
    assert!(t.host.pressed().is_empty() && t.host.delivered().is_empty());
}

#[test]
fn an_error_look_in_the_test_pane_fails_the_round_and_the_reason_goes_to_the_drafter() {
    let mut t = running_test("auto-test-error", true, "NIMBUS 0.4\n\nError: the model is overloaded, try again later\n");
    t.needs().expect("a needs-you moment");
    assert_eq!(t.look("error")["decision"], "stopped_test");
    let done = t.finish();
    assert_eq!(done["ok"], false);
    assert!(done["text"].as_str().unwrap().contains("The wizard stopped the test: the agent shows an error on its screen"), "{}", done["text"]);
}

// ---- no windows in autonomous mode ---------------------------------------------------------------------

#[test]
fn the_test_pane_of_an_autonomous_session_asks_for_no_window_and_off_still_does() {
    let mut on = running_test("auto-window-on", true, screens::IDLE);
    let pane = &on.host.spawned()[0];
    assert!(!pane.window, "no pop-out window is requested");
    assert_eq!((pane.cols, pane.rows), (120, 40), "the pane is still a real pane at the recorded grid");
    assert!(on.host.shown().is_empty(), "nothing was shown or focused");
    // "Show the pane" opens it on demand
    ok(&on.wizard, "capture", json!({"session": on.id, "op": "show"}));
    assert_eq!(on.host.shown().len(), 1, "show opens the test pane");
    on.finish();
    assert!(on.host.killed().len() >= 1, "the hidden pane is closed at the end");

    let mut off = running_test("auto-window-off", false, screens::IDLE);
    assert!(off.host.spawned()[0].window, "today: the pane opens as a window");
    assert!(!off.host.shown().is_empty(), "today: and is shown");
    off.finish();
}

#[test]
fn the_capture_pane_of_an_autonomous_session_asks_for_no_window_and_show_opens_it() {
    for (autonomous, expect_window) in [(true, false), (false, true)] {
        let tree = Tree::new(if autonomous { "auto-cap-window-on" } else { "auto-cap-window-off" });
        let (wizard, host) = tree.visible_wizard_with(|env| env.visible.window = true);
        let started = ok(&wizard, "start", json!({"command": "fake-agent", "autonomous": autonomous}));
        let id = started["session"].as_str().unwrap().to_string();
        ok(&wizard, "probe", json!({"session": id}));
        start_capture(&wizard, &id);
        let pane = host.spawned()[0].clone();
        assert_eq!(pane.window, expect_window, "autonomous={autonomous}");
        assert_eq!((pane.cols, pane.rows), (120, 40));
        if !autonomous {
            continue;
        }
        assert!(host.shown().is_empty());
        ok(&wizard, "capture", json!({"session": id, "op": "show"}));
        assert_eq!(host.shown().len(), 1, "Show the pane opens it");
        ok(&wizard, "cancel", json!({"session": id}));
        assert!(host.killed().iter().any(|p| *p == host.pane_id()), "cancel closes the hidden pane");
    }
}
