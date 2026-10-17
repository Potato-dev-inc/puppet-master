//! The visible test (SPEC 11.9) against a scripted pane: the checks, what is typed and what is not,
//! the wait for a question on the way, masking, cancel.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::host::Delivered;
use crate::tui::declarative::wizard::{Wizard, WizardError};

use super::fake_host::{screens, script_agent, wait_for, FakeHost};
use super::support::{call, ok, probed, visible_draft, with_captures, words_of, Tree};

struct Fixture {
    wizard: Arc<Wizard>,
    host: Arc<FakeHost>,
    id: String,
    plan: String,
    words: TestWords,
    _tree: Tree,
}

fn fixture(name: &str, draft: Option<String>, change: impl FnOnce(&mut crate::tui::declarative::wizard::WizardEnv)) -> Fixture {
    fixture_with(name, draft, false, change)
}

/// `auto`: the person left "answer setup questions for me" on.
fn fixture_with(name: &str, draft: Option<String>, auto: bool, change: impl FnOnce(&mut crate::tui::declarative::wizard::WizardEnv)) -> Fixture {
    let tree = Tree::new(name);
    let (wizard, host) = tree.visible_wizard_with(change);
    let started = ok(&wizard, "start", json!({"command": "fake-agent", "auto_answer": auto}));
    let id = started["session"].as_str().expect("session").to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft.unwrap_or_else(|| visible_draft("fake-agent"))}));
    let plan = v["planned_visible"]["fingerprint"].as_str().expect("a planned visible test").to_string();
    let words = words_of(&wizard, &id);
    Fixture { wizard: Arc::new(wizard), host, id, plan, words, _tree: tree }
}

impl Fixture {
    /// Run the visible test on a thread; the screens are the test's to set once the pane exists.
    fn run(&self) -> JoinHandle<Result<Value, WizardError>> {
        let (wizard, id, plan) = (self.wizard.clone(), self.id.clone(), self.plan.clone());
        std::thread::spawn(move || call(&wizard, "visibletest", json!({"session": id, "confirm": true, "plan": plan})))
    }

    fn pane(&self) -> String {
        wait_for(10, || self.host.spawned().first().map(|_| self.host.pane_id())).expect("the test pane is started")
    }

    fn finish(&self, handle: JoinHandle<Result<Value, WizardError>>) -> Value {
        handle.join().unwrap().unwrap_or_else(|e| panic!("{e}"))
    }
}

fn checks(v: &Value) -> Vec<(String, String)> {
    v["checks"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_string(), c["status"].as_str().unwrap().to_string())).collect()
}

fn status_of(v: &Value, name: &str) -> String {
    checks(v).into_iter().find(|(n, _)| n == name).map(|(_, s)| s).unwrap()
}

fn quick_limits(env: &mut crate::tui::declarative::wizard::WizardEnv) {
    env.visible.test.ready = Duration::from_millis(400);
    env.visible.test.busy = Duration::from_millis(400);
    env.visible.test.done = Duration::from_millis(600);
    env.visible.test.permission = Duration::from_millis(400);
    env.visible.test.person = Duration::from_secs(3);
}

#[test]
fn a_good_draft_passes_every_check_and_the_session_is_tested() {
    let f = fixture("vl-good", None, |_| ());
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], true, "{}\n{}", v["text"], v["checks"]);
    assert_eq!(v["state"], "tested");
    assert_eq!(
        checks(&v).iter().map(|(n, s)| format!("{n}:{s}")).collect::<Vec<_>>(),
        ["starts:passed", "startup:skipped", "ready:passed", "delivery:passed", "busy:passed", "done_reply:passed", "model:skipped", "resize:skipped", "work:skipped", "long_reply:skipped", "permission:passed", "permission_command:skipped", "exit:passed"]
    );
    assert_eq!((v["passed"].as_u64(), v["total"].as_u64()), (Some(7), Some(7)));
    // two tasks were typed: a plain message with the SECOND random word, and the hello.txt request
    let typed = f.host.delivered();
    assert_eq!(typed.len(), 2, "{typed:?}");
    assert_eq!(typed[0], format!("Reply with exactly the word: {}", f.words.two));
    assert!(typed[1].contains("hello.txt"));
    // nothing is answered: the pane is simply closed
    assert!(f.host.killed().contains(&f.host.pane_id()));
    // the transient profile is gone
    assert!(crate::tui::registry::registry().find(&f.host.spawned()[0].profile_id).is_none());
    // the pane started from the draft's profile under a wizard id, in a folder of its own at the fixed grid
    let pane = &f.host.spawned()[0];
    assert!(pane.profile_id.starts_with("wzt_"), "{}", pane.profile_id);
    assert_eq!((pane.cols, pane.rows), (120, 40));
    assert!(pane.cwd.ends_with("test-project"));
    // the notes carry the masked reply, never the word
    let shown = v.to_string();
    assert!(!shown.contains(&f.words.two) && !shown.contains(&f.words.one) && !shown.contains(&f.words.sky), "{shown}");
    assert!(shown.contains("<word-2>"), "{shown}");
}

#[test]
fn nothing_is_typed_when_the_profile_does_not_read_the_screen_as_ready() {
    let f = fixture("vl-notready", None, quick_limits);
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen("mystery agent\n\nloading models\nthings are ready\n");
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(v["state"], "verified", "a failed test changes nothing");
    assert_eq!(status_of(&v, "starts"), "passed");
    assert_eq!(status_of(&v, "ready"), "failed");
    for later in ["delivery", "busy", "done_reply", "permission"] {
        assert_eq!(status_of(&v, later), "skipped", "{later}");
    }
    assert!(f.host.delivered().is_empty(), "no task is typed into a screen the profile did not read as ready");
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("The visible test started your profile in a real pane and found 1 problem"), "{text}");
    assert!(text.contains("VISIBLETEST_READY_FAILED") && text.contains("mystery agent"), "{text}");
    assert!(text.contains("[tui.ready]"), "{text}");
    assert!(f.host.killed().contains(&f.host.pane_id()), "the pane is closed even though the test failed");
}

#[test]
fn a_question_on_the_way_waits_for_the_person_and_nothing_is_pressed() {
    let f = fixture("vl-trust", None, |_| ());
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::TRUST);
    let seen = wait_for(10, || {
        let s = f.wizard.read(&f.id, |s| s.visible.test_status.lock().clone()).unwrap();
        s["needs_you"]["message"].as_str().map(str::to_string)
    })
    .expect("the test says it is waiting for the person");
    assert!(seen.contains("asking a question in the test pane") && seen.contains("isolated temp folder, not your project"), "{seen}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(f.host.delivered().is_empty(), "nothing is typed while the question is up");
    // the person answers it in the pane
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], true, "{}\n{}", v["text"], v["checks"]);
}

#[test]
fn a_question_nobody_answers_ends_the_test_without_typing() {
    let f = fixture("vl-noanswer", None, |env| env.visible.test.person = Duration::from_millis(300));
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::TRUST);
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "ready"), "failed");
    assert!(v["text"].as_str().unwrap().contains("nobody answered"), "{}", v["text"]);
    assert!(f.host.delivered().is_empty());
}

#[test]
fn a_reply_that_does_not_contain_the_answer_fails_the_reply_check() {
    let f = fixture("vl-badreply", None, quick_limits);
    script_agent(&f.host, "something else entirely");
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "delivery"), "passed");
    assert_eq!(status_of(&v, "done_reply"), "failed");
    assert_eq!(status_of(&v, "permission"), "skipped");
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_DONE_REPLY_FAILED") && text.contains("something else entirely"), "{text}");
}

#[test]
fn an_agent_never_seen_working_fails_the_busy_check() {
    let f = fixture("vl-notbusy", None, quick_limits);
    // it answers at once, with no working screen
    f.host.on_deliver(|h, pane, text| {
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &screens::reply(&word));
        Delivered::Submitted
    });
    let run = f.run();
    f.pane();
    f.host.set_status("idle"); // the pane never reports running either: only the profile's own pattern could tell
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(status_of(&v, "busy"), "failed");
    assert!(v["text"].as_str().unwrap().contains("working_regex"), "{}", v["text"]);
}

#[test]
fn no_permission_question_skips_that_check_and_the_rest_still_counts() {
    let f = fixture("vl-noperm", None, quick_limits);
    let for_thread = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            // the agent just does it
            h.set_screen_of(pane, &screens::reply("done"));
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &screens::working(&word, 0));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &screens::reply(&word));
        });
        Delivered::Submitted
    });
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], true, "{}\n{}", v["text"], v["checks"]);
    assert_eq!(status_of(&v, "permission"), "skipped");
    assert_eq!((v["passed"].as_u64(), v["total"].as_u64()), (Some(6), Some(6)));
    assert_eq!(v["state"], "tested");
}

#[test]
fn a_question_the_profile_does_not_make_a_card_for_fails_the_permission_check() {
    // the draft fails its screen fixture (so verify is not ok) but it may still be tested
    let draft = visible_draft("fake-agent").replace("detect_regex = 'allow nimbus to .*\\[y/n\\]'", "detect_regex = 'nothing like it'");
    let f = fixture("vl-nocard", Some(draft), quick_limits);
    let for_thread = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            h.set_screen_of(pane, screens::PERMISSION);
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &screens::working(&word, 0));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &screens::reply(&word));
        });
        Delivered::Submitted
    });
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "permission"), "failed");
    assert!(v["text"].as_str().unwrap().contains("[[tui.prompt]]"), "{}", v["text"]);
    assert_eq!(v["state"], "probed", "a failed test is not a tested draft");
}

#[test]
fn the_test_needs_the_confirmation_and_the_fingerprint_of_what_the_person_saw() {
    let f = fixture("vl-confirm", None, |_| ());
    let err = call(&f.wizard, "visibletest", json!({"session": f.id})).unwrap_err();
    assert_eq!(err.code, "WIZARD_CONFIRM_REQUIRED");
    let err = call(&f.wizard, "visibletest", json!({"session": f.id, "confirm": true})).unwrap_err();
    assert_eq!(err.code, "WIZARD_PLAN_REQUIRED");
    let err = call(&f.wizard, "visibletest", json!({"session": f.id, "confirm": true, "plan": "stale"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_PLAN_MISMATCH"));
    assert!(f.host.spawned().is_empty(), "no pane before the person confirmed what starts");
}

#[test]
fn a_draft_without_a_tui_table_has_no_visible_test() {
    let tree = Tree::new("vl-nodraft");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let err = call(&wizard, "visibletest", json!({"session": id, "confirm": true, "plan": "x"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    assert!(host.spawned().is_empty());
}

#[test]
fn a_pane_that_cannot_start_is_reported_as_a_spawn_failure() {
    let f = fixture("vl-spawn", None, |_| ());
    f.host.fail_spawn("no console");
    let v = ok(&f.wizard, "visibletest", json!({"session": f.id, "confirm": true, "plan": f.plan}));
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"][0]["code"], "VISIBLETEST_SPAWN_FAILED");
    assert!(v["text"].as_str().unwrap().contains("no console"));
}

// ---- the permission check must end on its own (SPEC 11.15) ----------------------------------------------------

fn permission_limits(env: &mut crate::tui::declarative::wizard::WizardEnv) {
    quick_limits(env);
    env.visible.capture.frozen_ms = 150;
    env.visible.capture.idle_done_ms = 250;
    env.visible.capture.question_ms = 300;
    env.visible.test.permission = Duration::from_secs(6);
}

/// An agent whose plain turns work as in `script_agent` and whose hello.txt task does `on_task`.
fn agent_that(f: &Fixture, on_task: impl Fn(&FakeHost, &str) + Send + Sync + 'static) {
    let for_thread = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            on_task(h, pane);
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &screens::working(&word, 0));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &screens::working(&word, 1));
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &screens::reply(&word));
        });
        Delivered::Submitted
    });
}

#[test]
fn an_agent_back_at_its_input_box_ends_the_permission_check_without_waiting_for_the_limit() {
    let f = fixture("vl-backready", None, permission_limits);
    agent_that(&f, |h, pane| h.set_screen_of(pane, &screens::reply("alldone")));
    let began = std::time::Instant::now();
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(status_of(&v, "permission"), "skipped", "{}\n{}", v["text"], v["checks"]);
    assert_eq!(v["ok"], true, "{}", v["text"]);
    assert!(began.elapsed() < Duration::from_secs(5), "ended by the idle shape, not by the six second limit: {:?}", began.elapsed());
}

#[test]
fn a_file_written_behind_a_working_looking_screen_that_never_moves_ends_the_check() {
    let f = fixture("vl-frozenfile", None, permission_limits);
    agent_that(&f, |h, pane| {
        let cwd = h.pane(pane).expect("the pane").cwd;
        std::fs::write(cwd.join("hello.txt"), "x").expect("the agent writes the file");
        h.set_screen_of(pane, &screens::working("later", 0));
    });
    let began = std::time::Instant::now();
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(status_of(&v, "permission"), "skipped", "{}\n{}", v["text"], v["checks"]);
    assert!(began.elapsed() < Duration::from_secs(5), "{:?}", began.elapsed());
}

#[test]
fn a_picture_nothing_recognises_is_shown_to_the_person_and_nothing_is_pressed() {
    let f = fixture("vl-oddquestion", None, |env| {
        permission_limits(env);
        env.visible.test.permission = Duration::from_millis(2500);
    });
    agent_that(&f, |h, pane| h.set_screen_of(pane, "NIMBUS 0.4\n\nWhere should hello.txt go?\n\n   project folder      scratch folder\n\nuse the arrow keys\n"));
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let mut asked = false;
    for _ in 0..80 {
        let status = ok(&f.wizard, "capture", json!({"session": f.id, "op": "test_status"}));
        if status["needs_you"]["kind"] == "question" {
            asked = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let delivered_before_end = f.host.delivered();
    let v = f.finish(run);
    assert!(asked, "the person was never asked: {}", v["checks"]);
    assert_eq!(status_of(&v, "permission"), "skipped");
    assert_eq!(delivered_before_end.len(), 2, "only the two tasks were typed: {delivered_before_end:?}");
}

#[test]
fn cancelling_the_session_closes_the_test_pane() {
    let f = fixture("vl-cancel", None, |_| ());
    let run = f.run();
    f.pane();
    f.host.set_screen("still loading things\n");
    std::thread::sleep(Duration::from_millis(100));
    ok(&f.wizard, "cancel", json!({"session": f.id}));
    let result = run.join().unwrap();
    assert!(result.is_err() || result.as_ref().is_ok_and(|v| v["ok"] == false));
    assert!(f.host.killed().contains(&f.host.pane_id()));
    assert!(crate::tui::registry::registry().find(&f.host.spawned()[0].profile_id).is_none());
}
