//! The `capture` operations and the runner thread (SPEC 11.4), against a scripted pane host: plan and
//! confirm, one pane in an isolated folder at the fixed grid, the guided steps, finish with masked
//! screens, cancel closing the pane.

use serde_json::{json, Value};

use crate::tui::declarative::wizard::visible::host::Delivered;

use super::fake_host::{screens, wait_for};
use super::support::{call, ok, probed, start, words_of, Tree};

fn plan_for(wizard: &crate::tui::declarative::wizard::Wizard, id: &str, args: Value) -> String {
    let plan = ok(wizard, "capture", json!({"session": id, "op": "plan", "args": args}));
    plan["plan"]["fingerprint"].as_str().unwrap().to_string()
}

fn start_capture(wizard: &crate::tui::declarative::wizard::Wizard, id: &str, args: Value) -> Value {
    let fingerprint = plan_for(wizard, id, args.clone());
    ok(wizard, "capture", json!({"session": id, "op": "start", "args": args, "confirm": true, "plan": fingerprint}))
}

fn status(wizard: &crate::tui::declarative::wizard::Wizard, id: &str) -> Value {
    ok(wizard, "capture", json!({"session": id, "op": "status"}))
}

fn step_status(status: &Value, step: &str) -> String {
    status["steps"].as_array().unwrap().iter().find(|s| s["id"] == step).map(|s| s["status"].as_str().unwrap().to_string()).unwrap()
}

fn wait_status(wizard: &crate::tui::declarative::wizard::Wizard, id: &str, what: &str, check: impl Fn(&Value) -> bool) -> Value {
    wait_for(20, || {
        let s = status(wizard, id);
        check(&s).then_some(s)
    })
    .unwrap_or_else(|| panic!("never reached: {what}; status: {}", status(wizard, id)))
}

#[test]
fn a_capture_needs_the_probe_first() {
    let tree = Tree::new("cap-state");
    let (wizard, _host) = tree.visible_wizard();
    let id = start(&wizard, "fake-agent");
    let err = call(&wizard, "capture", json!({"session": id, "op": "plan"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    let err = call(&wizard, "capture", json!({"session": id, "op": "dance"})).unwrap_err();
    assert_eq!(err.status, 400, "an unknown op is refused");
}

#[test]
fn the_plan_shows_the_program_the_arguments_the_folder_and_the_grid() {
    let tree = Tree::new("cap-plan");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let plan = ok(&wizard, "capture", json!({"session": id, "op": "plan", "args": ["--model", "small"]}));
    let plan = &plan["plan"];
    assert!(plan["program"]["path"].as_str().unwrap().to_lowercase().contains("fake-agent"));
    assert_eq!(plan["args"], json!(["--model", "small"]));
    assert_eq!(plan["cwd"], "<isolated folder>");
    assert_eq!(plan["grid"], json!({"cols": 120, "rows": 40}));
    let first = plan["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(plan_for(&wizard, &id, json!(["--model", "small"])), first, "the same command has the same fingerprint");
    assert_ne!(plan_for(&wizard, &id, json!(["--model", "big"])), first);
    assert_ne!(plan_for(&wizard, &id, json!([])), first);
}

#[test]
fn arguments_are_limited_and_checked() {
    let tree = Tree::new("cap-args");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    for bad in [json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"]), json!(["x".repeat(121)]), json!(["new\nline"]), json!("not a list"), json!([1])] {
        let err = call(&wizard, "capture", json!({"session": id, "op": "plan", "args": bad})).unwrap_err();
        assert_eq!((err.status, err.code), (400, "INVALID_ARGUMENT"), "{bad}");
    }
}

#[test]
fn starting_needs_the_persons_confirmation_and_the_fingerprint_of_what_they_saw() {
    let tree = Tree::new("cap-confirm");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let fingerprint = plan_for(&wizard, &id, json!([]));
    let err = call(&wizard, "capture", json!({"session": id, "op": "start"})).unwrap_err();
    assert_eq!(err.code, "WIZARD_CONFIRM_REQUIRED");
    let err = call(&wizard, "capture", json!({"session": id, "op": "start", "confirm": true})).unwrap_err();
    assert_eq!(err.code, "WIZARD_PLAN_REQUIRED");
    let err = call(&wizard, "capture", json!({"session": id, "op": "start", "confirm": true, "plan": "not it"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_PLAN_MISMATCH"));
    // arguments the person did not see change the fingerprint
    let err = call(&wizard, "capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint, "args": ["--yolo"]})).unwrap_err();
    assert_eq!(err.code, "WIZARD_PLAN_MISMATCH");
    assert!(host.spawned().is_empty(), "nothing started before the person confirmed the exact command");
}

#[test]
fn one_pane_starts_in_an_isolated_folder_at_the_fixed_grid() {
    let tree = Tree::new("cap-start");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let started = start_capture(&wizard, &id, json!([]));
    assert_eq!(started["state"], "running");
    assert_eq!(started["grid"], json!({"cols": 120, "rows": 40}));
    let panes = host.spawned();
    assert_eq!(panes.len(), 1);
    assert_eq!((panes[0].cols, panes[0].rows), (120, 40));
    assert!(panes[0].profile_id.starts_with("wzc_"), "{}", panes[0].profile_id);
    let work = wizard.read(&id, |s| s.work_dir.clone()).unwrap();
    assert_eq!(panes[0].cwd, work.join("capture-project"), "the pane runs in the session's own folder, never a project");
    assert!(panes[0].cwd.join("README.md").is_file());
    // the profile is findable by the pane registry while the wizard owns the pane, and never listed
    let registry = crate::tui::registry::registry();
    assert!(registry.find(&panes[0].profile_id).is_some());
    assert!(registry.snapshot().iter().all(|p| p.id() != panes[0].profile_id), "never in the listing");
    // a second capture while one runs is refused
    let fingerprint = plan_for(&wizard, &id, json!([]));
    let err = call(&wizard, "capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_CAPTURE_RUNNING"));
    ok(&wizard, "cancel", json!({"session": id}));
}

#[test]
fn a_pane_that_cannot_start_says_so_and_leaves_nothing_registered() {
    let tree = Tree::new("cap-spawn-fail");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    host.fail_spawn("no console here");
    let fingerprint = plan_for(&wizard, &id, json!([]));
    let err = call(&wizard, "capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap_err();
    assert_eq!((err.status, err.code), (500, "WIZARD_PANE_FAILED"));
    assert!(err.message.contains("no console here"));
    assert_eq!(status(&wizard, &id)["state"], "none");
}

#[test]
fn status_and_act_without_a_capture() {
    let tree = Tree::new("cap-none");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let s = status(&wizard, &id);
    assert_eq!((s["state"].as_str(), s["captured"].as_bool()), (Some("none"), Some(false)));
    let err = call(&wizard, "capture", json!({"session": id, "op": "act", "action": "skip"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_NO_CAPTURE"));
    assert_eq!(ok(&wizard, "capture", json!({"session": id, "op": "captures"}))["captures"], Value::Null);
}

#[test]
fn the_guided_capture_runs_end_to_end_and_finish_returns_masked_screens() {
    let tree = Tree::new("cap-flow");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let words = words_of(&wizard, &id);
    // The agent: typing a task makes it work; the question about hello.txt shows its permission question.
    {
        let one = words.one.clone();
        host.on_deliver(move |host, pane, text| {
            if text.contains("hello.txt") {
                host.set_screen_of(pane, screens::PERMISSION);
            } else {
                host.set_screen_of(pane, &screens::working(&one, 0));
            }
            Delivered::Submitted
        });
    }
    start_capture(&wizard, &id, json!([]));
    host.set_screen(screens::TRUST);
    let s = wait_status(&wizard, &id, "the folder question", |s| s["needs_you"]["kind"] == "trust");
    assert_eq!(step_status(&s, "start"), "needs_you");
    assert!(s["screen"].as_str().unwrap().contains("Do you trust"));
    assert!(host.delivered().is_empty(), "nothing is typed into the question");
    // the person answers it in the pane
    host.set_screen(screens::IDLE);
    wait_for(20, || (!host.delivered().is_empty()).then_some(())).expect("the test message is typed");
    assert_eq!(host.delivered()[0], format!("Reply with exactly the word: {}", words.one));
    host.set_screen(&screens::working(&words.one, 1));
    std::thread::sleep(std::time::Duration::from_millis(150));
    host.set_screen(&screens::working(&words.one, 2));
    std::thread::sleep(std::time::Duration::from_millis(150));
    host.set_screen(&screens::reply(&words.one));
    wait_for(20, || (host.delivered().len() >= 2).then_some(())).expect("the permission task is typed");
    assert!(host.delivered()[1].contains("hello.txt"));
    let s = wait_status(&wizard, &id, "the permission question", |s| s["needs_you"]["kind"] == "permission");
    assert!(s["needs_you"]["message"].as_str().unwrap().contains("answers nothing"));
    assert_eq!(host.delivered().len(), 2, "nothing is typed into the permission question");
    host.set_screen(screens::IDLE);
    wait_status(&wizard, &id, "the exit step", |s| s["step"] == "exit");
    ok(&wizard, "capture", json!({"session": id, "op": "act", "action": "closed"}));
    let finished = ok(&wizard, "capture", json!({"session": id, "op": "finish"}));
    assert!(host.killed().contains(&host.pane_id()), "finish closes the pane");
    let files = finished["captures"]["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().filter_map(|f| f["name"].as_str()).collect();
    assert_eq!(names, ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_permission"]);
    // masked: the AI never sees the random words
    let shown = finished["captures"].to_string();
    assert!(!shown.contains(&words.one) && !shown.contains(&words.sky), "{shown}");
    assert!(shown.contains("<word-1>"), "{shown}");
    assert_eq!(finished["captures"]["grid"], json!({"cols": 120, "rows": 40}));
    assert_eq!(finished["captures"]["facts"]["exit_method"], "person");
    // the session keeps the real words for the fixtures
    let real = wizard.read(&id, |s| s.visible.captures.lock().clone()).unwrap().unwrap();
    assert!(real.get("tui_reply").unwrap().text.contains(&words.one));
    assert_eq!(status(&wizard, &id)["state"], "none");
    assert!(crate::tui::registry::registry().find(&host.spawned()[0].profile_id).is_none(), "the transient profile is gone");
    let again = ok(&wizard, "capture", json!({"session": id, "op": "captures"}));
    assert_eq!(again["captures"], finished["captures"]);
}

#[test]
fn the_buttons_work_through_the_route() {
    let tree = Tree::new("cap-act");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    start_capture(&wizard, &id, json!([]));
    host.set_screen(screens::TRUST);
    wait_status(&wizard, &id, "the question", |s| s["needs_you"]["kind"] == "trust");
    // "It is ready" is refused while a question is on screen
    let err = call(&wizard, "capture", json!({"session": id, "op": "act", "action": "ready"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_CAPTURE_REFUSED"));
    assert!(err.message.contains("Answer it in the pane"));
    for bad in [json!({"action": "explode"}), json!({"action": "skip", "step": "nowhere"})] {
        let mut body = json!({"session": id, "op": "act"});
        body.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert_eq!(call(&wizard, "capture", body).unwrap_err().status, 400);
    }
    let after = ok(&wizard, "capture", json!({"session": id, "op": "act", "step": "start", "action": "skip"}));
    assert_eq!(step_status(&after, "start"), "skipped");
    assert!(host.delivered().is_empty());
    ok(&wizard, "capture", json!({"session": id, "op": "show"}));
    assert_eq!(host.shown().len(), 1);
    ok(&wizard, "cancel", json!({"session": id}));
}

#[test]
fn cancel_closes_the_pane_forgets_the_profile_and_removes_the_folder() {
    let tree = Tree::new("cap-cancel");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    start_capture(&wizard, &id, json!([]));
    let profile = host.spawned()[0].profile_id.clone();
    let work = wizard.read(&id, |s| s.work_dir.clone()).unwrap();
    assert!(work.join("capture-project").is_dir());
    ok(&wizard, "cancel", json!({"session": id}));
    assert!(host.killed().contains(&host.pane_id()));
    assert!(crate::tui::registry::registry().find(&profile).is_none());
    assert!(!work.exists());
}

#[test]
fn discard_drops_the_pane_and_the_captures() {
    let tree = Tree::new("cap-discard");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    start_capture(&wizard, &id, json!([]));
    super::support::with_captures(&wizard, &id);
    assert_eq!(status(&wizard, &id)["captured"], true);
    ok(&wizard, "capture", json!({"session": id, "op": "discard"}));
    assert!(host.killed().contains(&host.pane_id()));
    assert_eq!(status(&wizard, &id)["captured"], false);
    assert_eq!(status(&wizard, &id)["state"], "none");
}

#[test]
fn a_capture_pane_that_dies_fails_the_step_and_the_rest_is_skipped() {
    let tree = Tree::new("cap-death");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    start_capture(&wizard, &id, json!([]));
    host.set_screen(screens::IDLE);
    wait_for(20, || (!host.delivered().is_empty()).then_some(())).expect("typed");
    host.die();
    let s = wait_status(&wizard, &id, "the agent quit", |s| s["alive"] == false && s["steps"][2]["status"] == "failed");
    assert_eq!(step_status(&s, "permission"), "skipped");
    ok(&wizard, "cancel", json!({"session": id}));
}

#[test]
fn the_last_looks_stay_readable_while_running_after_it_looks_stuck_and_after_finish() {
    let tree = Tree::new("cap-debug");
    let (wizard, host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    let word = words_of(&wizard, &id).one;
    start_capture(&wizard, &id, json!([]));
    host.set_screen(&format!("NIMBUS\n\nreply: {word}\n\n{}", screens::TRUST));
    wait_status(&wizard, &id, "the question", |s| s["needs_you"]["kind"] == "trust");
    let live = ok(&wizard, "capture", json!({"session": id, "op": "debug"}));
    let frames = live["debug"]["frames"].as_array().expect("frames");
    let last = frames.last().unwrap();
    assert_eq!((last["classifier"].as_str(), last["step"].as_str()), (Some("dialog"), Some("start")));
    let shown = last["screen"].as_str().unwrap();
    assert!(shown.contains("Do you trust") && !shown.contains(&word) && shown.contains("<word-1>"), "{shown}");
    assert!(last.get("raw").is_none(), "the route never carries the real words");
    // "It looks stuck" keeps the looks in the session
    ok(&wizard, "capture", json!({"session": id, "op": "act", "action": "stuck"}));
    let kept = wizard.read(&id, |s| s.visible.debug.lock().clone()).unwrap();
    assert!(kept.is_some_and(|d| !d["frames"].as_array().unwrap().is_empty()));
    // and after finish the run is gone but the looks are not
    ok(&wizard, "capture", json!({"session": id, "op": "finish"}));
    let after = ok(&wizard, "capture", json!({"session": id, "op": "debug"}));
    assert!(!after["debug"]["frames"].as_array().unwrap().is_empty());
    ok(&wizard, "cancel", json!({"session": id}));
}
