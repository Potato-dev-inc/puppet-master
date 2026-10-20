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

// ---- the program's own error needs a real error row (SPEC 11.9) -----------------------------------

/// What an inline TUI prints first: a block-character logo, its name and version, and a status row that says it is
/// signed in (a row that holds a sign-in word without being a problem).
const BANNER_ROWS: &str = "\u{2584}\u{2580} Nimbus CLI 1.4.2.\nSigned in as dev@example.com\nv1.4.2\n";

fn with_banner(screen: &str) -> String {
    format!("{BANNER_ROWS}{screen}")
}

#[test]
fn a_prompt_regex_that_is_too_broad_is_the_drafts_fault_never_the_agents_own_error() {
    let f = fixture("vl-broad", None, quick_limits);
    let for_thread = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        // the profile reads the working screen as a question (its regex matched text that is not one)
        h.set_screen_of(pane, &with_banner(&screens::working(&word, 0)));
        h.set_card(Some(json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &with_banner(&screens::working(&word, 1)));
        });
        Delivered::Submitted
    });
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "done_reply"), "failed", "{}", v["checks"]);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("detect_regex"), "{text}");
    assert_eq!(v["agent_error"], json!(null), "the banner and a signed-in status row are not an error: {}", v["agent_error"]);
}

#[test]
fn a_real_provider_error_row_is_the_agents_own_error_and_is_shown_as_the_message_not_the_banner() {
    let f = fixture("vl-realerr", None, quick_limits);
    f.host.on_deliver(|h, pane, _| {
        h.set_screen_of(pane, &with_banner("\n> Reply\nError: No endpoints found that support tool use. Check your provider settings.\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n"));
        Delivered::Submitted
    });
    let run = f.run();
    f.pane();
    f.host.set_screen(screens::IDLE);
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    let ae = &v["agent_error"];
    assert_eq!(ae["external"], true, "{v}");
    assert_eq!(ae["message"], "Error: No endpoints found that support tool use. Check your provider settings.", "{ae}");
}

// ---- a row that was on the screen before the task is never the agent's error (founder run 2026-10-19) ----------------

/// The account row an inline TUI draws from startup on: the person's address and the plan in parentheses ("quota" in it
/// matched the external-error words).
const ACCOUNT: &str = "someone@example.com (Nimbus Starter Quota)";

fn with_account(screen: &str) -> String {
    format!("{screen}  {ACCOUNT}\n")
}

/// An agent that shows `ACCOUNT` on every screen, takes the task and then shows `after` (a reply screen body) instead of
/// the answer.
fn account_agent(f: &Fixture, after: &'static str) {
    let for_thread = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &with_account(&screens::working(&word, 0)));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &with_account(&screens::working(&word, 1)));
            std::thread::sleep(Duration::from_millis(120));
            host.set_screen_of(&pane, &with_account(&format!("NIMBUS 0.4\n\n> Reply with exactly the word: {word}\n{after}\n\ndone in 2s\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n")));
        });
        Delivered::Submitted
    });
}

#[test]
fn an_account_row_shown_since_startup_is_not_the_agents_error_and_the_draft_gets_the_failure() {
    let f = fixture("vl-account", None, quick_limits);
    account_agent(&f, "reply: something else entirely");
    let run = f.run();
    f.pane();
    f.host.set_screen(&with_account(screens::IDLE));
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "done_reply"), "failed", "{}", v["checks"]);
    assert_eq!(v["agent_error"], json!(null), "the account row is status: {}", v["agent_error"]);
    // the failure goes back to the drafter as an ordinary failed check, with the screen (the address masked)
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_DONE_REPLY_FAILED"), "{text}");
    assert!(!v.to_string().contains("someone@example.com"), "no address in what the wizard shows: {v}");
}

#[test]
fn a_row_that_read_like_an_error_but_was_on_the_screen_before_the_task_is_never_the_agents_error() {
    let f = fixture("vl-baseline-tip", None, quick_limits);
    const TIP: &str = "Tip: run /login to sign in with another provider.";
    fn shown(body: String) -> String {
        format!("{body}  {TIP}\n")
    }
    let for_thread = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        h.set_screen_of(pane, &shown(screens::working(&word, 0)));
        let (host, pane) = (for_thread.clone(), pane.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            host.set_screen_of(&pane, &shown(format!("NIMBUS 0.4\n\n> Reply with exactly the word: {word}\nreply: not it\n\ndone in 2s\n\n| > Type a task...             |\n")));
        });
        Delivered::Submitted
    });
    let run = f.run();
    f.pane();
    f.host.set_screen(&shown(screens::IDLE.to_string()));
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    assert_eq!(status_of(&v, "done_reply"), "failed", "{}", v["checks"]);
    assert_eq!(v["agent_error"], json!(null), "the tip was there before the task: {}", v["agent_error"]);
}

#[test]
fn a_new_error_row_after_the_task_still_stops_with_that_row_even_beside_an_account_row() {
    let f = fixture("vl-account-newerr", None, quick_limits);
    account_agent(&f, "Error: quota exceeded");
    let run = f.run();
    f.pane();
    f.host.set_screen(&with_account(screens::IDLE));
    let v = f.finish(run);
    assert_eq!(v["ok"], false);
    let ae = &v["agent_error"];
    assert_eq!(ae["message"], "Error: quota exceeded", "{v}");
    assert_eq!(ae["external"], true, "{ae}");
    assert!(!v.to_string().contains("someone@example.com"), "{v}");
}

#[test]
fn an_error_row_that_is_new_after_the_task_is_found_when_the_startup_screens_held_other_rows() {
    let f = fixture("vl-new-429", None, quick_limits);
    account_agent(&f, "429 Too Many Requests");
    let run = f.run();
    f.pane();
    f.host.set_screen(&with_account(screens::IDLE));
    let v = f.finish(run);
    assert_eq!(v["agent_error"]["message"], "429 Too Many Requests", "{v}");
}

#[test]
fn an_error_at_startup_before_any_task_is_still_the_agents_error() {
    // nothing was sent yet, so there is no "before": a sign-in wall that never reaches the input box is the program's own
    let f = fixture("vl-startup-wall", None, quick_limits);
    script_agent(&f.host, "");
    let run = f.run();
    f.pane();
    f.host.set_screen("NIMBUS 0.4\n\nError: No endpoints found that support tool use. Check your provider settings.\n");
    let v = f.finish(run);
    assert_eq!(status_of(&v, "ready"), "failed", "{}", v["checks"]);
    assert_eq!(v["agent_error"]["external"], true, "{v}");
    assert!(v["agent_error"]["message"].as_str().unwrap().starts_with("Error: No endpoints found"), "{v}");
}
