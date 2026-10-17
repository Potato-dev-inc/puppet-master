//! The visible test pane with the setup-question switch on and off (SPEC 11.16): the startup question must
//! become a card (checked BEFORE anything answers it), the wizard answers a trust or one-time approval
//! question only in its own pane and only with the safe option, and says plainly when a question is the
//! person's. Scripted panes; the whole flow runs with zero person input where the question is answerable.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::answer::AnswerKey;
use crate::tui::declarative::wizard::{Wizard, WizardError};

use super::fake_host::{screens, script_agent, wait_for, FakeHost};
use super::support::{call, ok, visible_draft, with_captures, words_of, Tree};

struct Fixture {
    wizard: Arc<Wizard>,
    host: Arc<FakeHost>,
    id: String,
    plan: String,
    _words: TestWords,
    _tree: Tree,
}

fn fixture(name: &str, draft: Option<String>, auto: bool, change: impl FnOnce(&mut crate::tui::declarative::wizard::WizardEnv)) -> Fixture {
    let tree = Tree::new(name);
    let (wizard, host) = tree.visible_wizard_with(change);
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "auto_answer": auto}));
    let id = started["session"].as_str().expect("session").to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft.unwrap_or_else(|| visible_draft("fake-agent"))}));
    let plan = v["planned_visible"]["fingerprint"].as_str().expect("a planned visible test").to_string();
    let words = words_of(&wizard, &id);
    Fixture { wizard: Arc::new(wizard), host, id, plan, _words: words, _tree: tree }
}

impl Fixture {
    fn run(&self) -> std::thread::JoinHandle<Result<Value, WizardError>> {
        let (wizard, id, plan) = (self.wizard.clone(), self.id.clone(), self.plan.clone());
        std::thread::spawn(move || call(&wizard, "visibletest", json!({"session": id, "confirm": true, "plan": plan})))
    }

    fn pane(&self) -> String {
        wait_for(10, || self.host.spawned().first().map(|_| self.host.pane_id())).expect("the test pane is started")
    }

    fn finish(&self, handle: std::thread::JoinHandle<Result<Value, WizardError>>) -> Value {
        handle.join().unwrap().unwrap_or_else(|e| panic!("{e}"))
    }

    fn status(&self) -> Value {
        ok(&self.wizard, "capture", json!({"session": self.id, "op": "test_status"}))
    }

    fn needs(&self) -> Option<Value> {
        wait_for(10, || {
            let s = self.status();
            s["needs_you"].is_object().then(|| s["needs_you"].clone())
        })
    }
}

fn status_of(v: &Value, name: &str) -> String {
    v["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["status"].as_str().unwrap().to_string()).unwrap()
}

fn quick(env: &mut crate::tui::declarative::wizard::WizardEnv) {
    env.visible.test.ready = Duration::from_secs(8);
    env.visible.test.busy = Duration::from_millis(600);
    env.visible.test.done = Duration::from_secs(3);
    env.visible.test.permission = Duration::from_millis(500);
    env.visible.test.person = Duration::from_secs(4);
    env.visible.test.step = Duration::from_secs(1);
}

const ARROW: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n> Yes, I trust this folder\n  No, exit\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const NO_FIRST: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n> No, exit\n  Yes, I trust this folder\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const NO_FIRST_MOVED: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n  No, exit\n> Yes, I trust this folder\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";
const NUMBERED: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n  1. Yes, trust this folder\n  2. No, exit\n";
const ALWAYS_ONLY: &str = "NIMBUS\n\nDo you trust the files in this folder?\n\n> Yes, always trust this folder\n  No, exit\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";

/// The pane goes to the empty input box when `done` is pressed.
fn closes_on(f: &Fixture, done: AnswerKey) {
    f.host.on_answer(move |h, pane, keys| {
        if keys.contains(&done) {
            h.set_screen_of(pane, screens::IDLE);
        }
    });
}

fn all_checks_passed(v: &Value) -> bool {
    v["ok"] == true && v["checks"].as_array().unwrap().iter().all(|c| c["status"] != "failed")
}

#[test]
fn an_arrow_menu_with_the_safe_option_highlighted_is_answered_and_the_whole_test_needs_no_person() {
    let f = fixture("aa-arrow", None, true, quick);
    script_agent(&f.host, "");
    closes_on(&f, AnswerKey::Enter);
    let run = f.run();
    f.pane();
    f.host.set_screen(ARROW);
    let v = f.finish(run);
    assert!(all_checks_passed(&v), "{}\n{}", v["text"], v["checks"]);
    assert_eq!(status_of(&v, "startup"), "passed", "the draft's own prompt saw the question first: {}", v["checks"]);
    assert_eq!(f.host.pressed(), vec![AnswerKey::Enter]);
    assert_eq!(v["answered"][0]["label"], "Yes, I trust this folder");
    assert_eq!(v["answered"][0]["flavour"], "trust");
    assert_eq!(v["answered"][0]["keys"], "Enter");
    assert!(v["answered"][0]["screen"].as_str().unwrap().contains("Do you trust the files in this folder?"));
    assert_eq!(f.host.delivered().len(), 2, "the two test tasks went in after the question was answered");
    assert_eq!(v["state"], "tested");
}

#[test]
fn a_safe_option_on_the_second_row_is_walked_to_with_the_highlight_checked() {
    let f = fixture("aa-second", None, true, quick);
    script_agent(&f.host, "");
    f.host.on_answer(|h, pane, keys| match keys {
        [AnswerKey::Down] => h.set_screen_of(pane, NO_FIRST_MOVED),
        [AnswerKey::Enter] => h.set_screen_of(pane, screens::IDLE),
        _ => {}
    });
    let run = f.run();
    f.pane();
    f.host.set_screen(NO_FIRST);
    let v = f.finish(run);
    assert!(all_checks_passed(&v), "{}\n{}", v["text"], v["checks"]);
    assert_eq!(f.host.pressed(), vec![AnswerKey::Down, AnswerKey::Enter]);
    assert_eq!(v["answered"][0]["keys"], "Down, Enter");
}

#[test]
fn a_numbered_menu_gets_its_number() {
    let f = fixture("aa-numbered", None, true, quick);
    script_agent(&f.host, "");
    closes_on(&f, AnswerKey::Text("1".into()));
    let run = f.run();
    f.pane();
    f.host.set_screen(NUMBERED);
    let v = f.finish(run);
    assert!(all_checks_passed(&v), "{}\n{}", v["text"], v["checks"]);
    assert_eq!(f.host.pressed(), vec![AnswerKey::Text("1".into())]);
}

#[test]
fn a_yes_no_question_gets_y() {
    let f = fixture("aa-yn", None, true, quick);
    script_agent(&f.host, "");
    closes_on(&f, AnswerKey::Text("y".into()));
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::PERMISSION);
    let v = f.finish(run);
    assert!(all_checks_passed(&v), "{}\n{}", v["text"], v["checks"]);
    assert_eq!(f.host.pressed(), vec![AnswerKey::Text("y".into())]);
    assert_eq!(v["answered"][0]["flavour"], "permission");
}

#[test]
fn a_menu_whose_only_yes_is_always_allow_is_never_answered_and_the_person_is_told() {
    let f = fixture("aa-always", None, true, quick);
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen(ALWAYS_ONLY);
    let needs = f.needs().expect("the person is told");
    let text = needs["message"].as_str().unwrap();
    assert!(text.contains("asking a question in the test pane") && text.contains("did not answer it for you"), "{text}");
    std::thread::sleep(Duration::from_millis(400));
    assert!(f.host.pressed().is_empty(), "nothing was pressed");
    // the person answers it in the pane
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], true, "{}\n{}", v["text"], v["checks"]);
    assert!(f.host.pressed().is_empty());
    assert_eq!(v["answered"].as_array().unwrap().len(), 0);
}

#[test]
fn with_the_switch_off_the_same_question_waits_for_the_person_and_nothing_is_pressed() {
    let f = fixture("aa-off", None, false, quick);
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen(ARROW);
    let needs = f.needs().expect("the person is told");
    assert_eq!(needs["kind"], "trust");
    assert!(needs["message"].as_str().unwrap().contains("Answer it there"), "{needs}");
    assert!(!needs["message"].as_str().unwrap().contains("did not answer it for you"), "it never tried: {needs}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(f.host.pressed().is_empty());
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], true);
}

#[test]
fn a_sign_in_question_is_the_persons_even_with_the_switch_on() {
    let f = fixture("aa-login", None, true, quick);
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen("NIMBUS\n\nPlease sign in to continue.\n\n  1. Yes, open the browser\n  2. No, quit\n");
    let needs = f.needs().expect("the person is told");
    assert_eq!(needs["kind"], "login");
    std::thread::sleep(Duration::from_millis(500));
    assert!(f.host.pressed().is_empty());
    f.host.set_screen(screens::IDLE);
    f.finish(run);
}

#[test]
fn a_draft_with_no_prompt_for_the_startup_question_fails_the_startup_check_before_anything_answers_it() {
    // the trust prompt block removed: only the generic heuristics would recognise the question
    let draft = visible_draft("fake-agent");
    let start = draft.find("[[tui.prompt]]\nkind = \"trust_folder\"").or_else(|| draft.find("[[tui.prompt]]\r\nkind = \"trust_folder\"")).expect("the trust prompt");
    let end = draft[start + 10..].find("[[tui.prompt]]").map(|i| i + start + 10).expect("the next prompt");
    let weak = format!("{}{}", &draft[..start], &draft[end..]);
    let f = fixture("aa-nocard", Some(weak), true, quick);
    script_agent(&f.host, "");
    closes_on(&f, AnswerKey::Enter);
    let run = f.run();
    f.pane();
    f.host.set_screen(ARROW);
    let v = f.finish(run);
    assert_eq!(status_of(&v, "startup"), "failed", "{}", v["checks"]);
    assert_eq!(v["ok"], false);
    assert!(v["text"].as_str().unwrap().contains("[[tui.prompt]]") && v["text"].as_str().unwrap().contains("workspace_trust"), "{}", v["text"]);
    // the failure was recorded first; the rest of the test still ran so one round tells the drafter everything
    assert_eq!(status_of(&v, "ready"), "passed");
}

#[test]
fn stop_the_test_ends_it_while_a_question_is_up_and_a_long_wait_says_so() {
    let f = fixture("aa-stop", None, false, quick);
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen(ARROW);
    let needs = f.needs().expect("told");
    assert_eq!(needs["kind"], "trust");
    // after a while on one question: "this is taking long", kind stuck
    let stuck = wait_for(10, || {
        let n = f.status()["needs_you"].clone();
        (n["kind"] == "stuck").then_some(n)
    })
    .expect("stuck after the step limit");
    assert!(stuck["message"].as_str().unwrap().contains("You can stop the test"), "{stuck}");
    ok(&f.wizard, "capture", json!({"session": f.id, "op": "test_act", "action": "stop"}));
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "ready"), "failed");
    assert!(v["text"].as_str().unwrap().contains("You stopped the test"), "{}", v["text"]);
    assert!(f.host.pressed().is_empty());
    // a bad action is refused
    assert_eq!(call(&f.wizard, "capture", json!({"session": f.id, "op": "test_act", "action": "explode"})).unwrap_err().status, 400);
}

#[test]
fn a_pane_the_wizard_did_not_start_is_never_pressed() {
    use crate::tui::declarative::wizard::visible::host::PaneHost;
    let host = FakeHost::new();
    let err = host.answer("some-other-pane", &[AnswerKey::Enter]).unwrap_err();
    assert!(err.contains("not a pane the wizard started"), "{err}");
    assert!(host.pressed().is_empty());
}

#[test]
fn what_the_wizard_answered_is_recorded_in_the_provenance_file() {
    let f = fixture("aa-prov", None, true, quick);
    script_agent(&f.host, "");
    closes_on(&f, AnswerKey::Enter);
    let run = f.run();
    f.pane();
    f.host.set_screen(ARROW);
    let v = f.finish(run);
    assert!(all_checks_passed(&v), "{}\n{}", v["text"], v["checks"]);
    let done = ok(&f.wizard, "finish", json!({"session": f.id, "provenance": super::support::provenance()}));
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(done["provenance_file"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(prov["auto_answer"]["enabled"], true);
    let answered = prov["auto_answer"]["answered"].as_array().unwrap();
    assert_eq!(answered.len(), 1);
    assert_eq!((answered[0]["place"].as_str(), answered[0]["label"].as_str(), answered[0]["keys"].as_str()), (Some("visibletest"), Some("Yes, I trust this folder"), Some("Enter")));
}
