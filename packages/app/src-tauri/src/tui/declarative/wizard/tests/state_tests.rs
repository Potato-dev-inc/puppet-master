//! The session store and the state machine (SPEC 2).

use std::time::{Duration, Instant};

use serde_json::json;

use crate::tui::declarative::wizard::session::{State, IDLE_LIMIT, MAX_SESSIONS};

use super::support::{call, ok, start, Tree, GOOD};

#[test]
fn steps_must_come_in_order() {
    let tree = Tree::new("order");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    // verify needs a probe first
    let err = call(&wizard, "verify", json!({"session": id, "toml": GOOD})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    assert!(err.message.contains("resolved"), "{}", err.message);
    // livetest needs a verified draft (confirm is checked first)
    let err = call(&wizard, "livetest", json!({"session": id, "confirm": true})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
    // finish needs a tested draft
    let err = call(&wizard, "finish", json!({"session": id, "provenance": super::support::provenance()})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
    assert_eq!(wizard.read(&id, |s| s.state).unwrap(), State::Resolved);
}

#[test]
fn states_move_forward_and_a_new_draft_drops_the_live_test() {
    let tree = Tree::new("forward");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    assert_eq!(ok(&wizard, "probe", json!({"session": id}))["state"], "probed");
    assert_eq!(ok(&wizard, "verify", json!({"session": id, "toml": GOOD}))["state"], "verified");
    let t = ok(&wizard, "livetest", json!({"session": id, "confirm": true}));
    assert_eq!(t["state"], "tested");
    // probing again does not change the state or the draft
    assert_eq!(ok(&wizard, "probe", json!({"session": id}))["state"], "tested");
    // a failing verify keeps the tested draft
    let bad = ok(&wizard, "verify", json!({"session": id, "toml": "schema_version = 1\nnonsense = 1\n"}));
    assert_eq!(bad["ok"], false);
    assert_eq!(bad["state"], "tested");
    // a passing verify of a new draft drops the live-test result
    let again = ok(&wizard, "verify", json!({"session": id, "toml": GOOD.replace("Fake Agent", "Fake Agent 2")}));
    assert_eq!(again["ok"], true);
    assert_eq!(again["state"], "verified");
    assert!(wizard.read(&id, |s| s.tested.is_none()).unwrap());
}

#[test]
fn unknown_and_missing_session_ids_are_reported() {
    let tree = Tree::new("unknown");
    let wizard = tree.wizard();
    let err = call(&wizard, "probe", json!({"session": "wz_nope"})).unwrap_err();
    assert_eq!((err.status, err.code), (404, "WIZARD_SESSION_NOT_FOUND"));
    let err = call(&wizard, "probe", json!({})).unwrap_err();
    assert_eq!((err.status, err.code), (400, "INVALID_ARGUMENT"));
}

#[test]
fn cancel_removes_the_work_folder_and_is_idempotent() {
    let tree = Tree::new("cancel");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 1);
    assert_eq!(ok(&wizard, "cancel", json!({"session": id}))["state"], "cancelled");
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 0);
    assert_eq!(ok(&wizard, "cancel", json!({"session": id}))["state"], "gone");
    assert_eq!(call(&wizard, "probe", json!({"session": id})).unwrap_err().code, "WIZARD_SESSION_NOT_FOUND");
    assert_eq!(ok(&wizard, "cancel", json!({"session": "wz_never"}))["state"], "gone");
}

#[test]
fn at_most_three_sessions_are_open() {
    let tree = Tree::new("limit");
    let wizard = tree.wizard();
    let ids: Vec<String> = (0..MAX_SESSIONS).map(|_| start(&wizard, "fake-agent")).collect();
    let err = call(&wizard, "start", json!({"command": "fake-agent"})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_TOO_MANY_SESSIONS"));
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), MAX_SESSIONS, "the refused start left no folder");
    ok(&wizard, "cancel", json!({"session": ids[0]}));
    start(&wizard, "fake-agent");
}

#[test]
fn idle_sessions_are_dropped_when_a_new_one_starts() {
    let tree = Tree::new("idle");
    let wizard = tree.wizard();
    let old = start(&wizard, "fake-agent");
    wizard.backdate(&old, IDLE_LIMIT + Duration::from_secs(60));
    let fresh = start(&wizard, "fake-agent");
    assert_eq!(call(&wizard, "probe", json!({"session": old})).unwrap_err().code, "WIZARD_SESSION_NOT_FOUND");
    assert!(wizard.read(&fresh, |_| ()).is_ok());
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 1, "the idle session's folder is gone");
}

#[test]
fn a_second_long_step_is_refused_while_one_runs_and_cancel_ends_it() {
    let tree = Tree::new("busy");
    let wizard = std::sync::Arc::new(tree.wizard_with(|env| {
        env.limits.version_timeout = Duration::from_millis(300);
        env.limits.probe_timeout = Duration::from_secs(20);
    }));
    let id = start(&wizard, "hang-agent");
    let runner = {
        let wizard = wizard.clone();
        let id = id.clone();
        std::thread::spawn(move || call(&wizard, "probe", json!({"session": id})))
    };
    let started = Instant::now();
    while wizard.read(&id, |s| s.busy).unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(10), "the probe never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let err = call(&wizard, "verify", json!({"session": id, "toml": GOOD})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BUSY"));
    let err = call(&wizard, "probe", json!({"session": id})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BUSY");
    // cancel works while busy and the running step ends fast
    assert_eq!(ok(&wizard, "cancel", json!({"session": id}))["state"], "cancelled");
    let result = runner.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(15), "cancel must not wait for the 20 s timeout");
    assert_eq!(result.unwrap_err().code, "WIZARD_CANCELLED");
    assert_eq!(wizard.session_count(), 0, "the cancelled session is dropped once its step ends");
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 0);
}

#[test]
fn capture_is_not_available_yet() {
    let tree = Tree::new("capture");
    let wizard = tree.wizard();
    let err = call(&wizard, "capture", json!({})).unwrap_err();
    assert_eq!((err.status, err.code), (501, "WIZARD_NOT_YET"));
    let err = call(&wizard, "bogus", json!({})).unwrap_err();
    assert_eq!(err.status, 404);
}
