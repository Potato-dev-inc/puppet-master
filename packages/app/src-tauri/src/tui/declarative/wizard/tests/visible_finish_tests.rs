//! Finish for the visible path (SPEC 11.10): what is written, what must have passed first, and that
//! the written definition passes its own fixtures.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::tui::declarative::check::check_report;
use crate::tui::declarative::wizard::Wizard;

use super::fake_host::{run_visible_test, FakeHost};
use super::support::{call, hybrid_draft, live_body, ok, probed, provenance, visible_draft, with_captures, words_of, Tree};

fn verified(wizard: &Wizard, id: &str, toml: &str) -> Value {
    let v = ok(wizard, "verify", json!({"session": id, "toml": toml}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    v
}

fn visible_session(tree: &Tree) -> (Arc<Wizard>, Arc<FakeHost>, String) {
    let (wizard, host) = tree.visible_wizard();
    let wizard = Arc::new(wizard);
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    (wizard, host, id)
}

#[test]
fn a_visible_only_draft_goes_from_captures_to_an_unapproved_definition_with_screen_fixtures() {
    let tree = Tree::new("vf-visible");
    let (wizard, host, id) = visible_session(&tree);
    let words = words_of(&wizard, &id);
    let v = verified(&wizard, &id, &visible_draft("fake-agent"));
    // finish needs the visible test first
    let err = call(&wizard, "finish", json!({"session": id, "provenance": provenance()})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    let live = run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(live["ok"], true, "{}\n{}", live["text"], live["checks"]);
    assert_eq!(live["state"], "tested");
    // the headless live test does not apply to this draft
    let err = call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": "x"})).unwrap_err();
    assert_eq!(err.code, "WIZARD_NO_HEADLESS_TEST");

    let done = ok(&wizard, "finish", json!({"session": id, "provenance": provenance()}));
    assert_eq!(done["id"], "nimbus");
    assert_eq!(done["state"], "needs_approval");
    assert_eq!(done["fixtures"], json!({"passed": 5, "total": 5}));
    let agents = &tree.agents;
    let toml = agents.join("nimbus.toml");
    assert_eq!(std::fs::read_to_string(&toml).unwrap(), visible_draft("fake-agent"), "the draft is written exactly as tested");
    let folder = agents.join("nimbus");
    let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "tui_idle.expect.toml", "tui_idle.txt", "tui_permission.expect.toml", "tui_permission.txt", "tui_reply.expect.toml", "tui_reply.txt",
            "tui_trust.expect.toml", "tui_trust.txt", "tui_working.expect.toml", "tui_working.txt"
        ]
    );
    // the captures carry the header, the real words (the proof a reply was read) and the merged expectations
    let reply = std::fs::read_to_string(folder.join("tui_reply.txt")).unwrap();
    assert!(reply.starts_with("# capture: fake-agent "), "{reply}");
    assert!(reply.lines().next().unwrap().contains("pane 120x40"), "{reply}");
    assert!(reply.contains(&words.one), "fixtures keep the real word");
    let expect = std::fs::read_to_string(folder.join("tui_reply.expect.toml")).unwrap();
    assert!(expect.contains("ready = true") && expect.contains("not_chrome_lines") && expect.contains(&words.one), "{expect}");
    assert!(std::fs::read_to_string(folder.join("tui_working.expect.toml")).unwrap().contains("busy = true"));
    // provenance: results and the capture facts, no text, no paths, no words
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(agents.join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["results"]["visible"], json!({"passed": 7, "total": 7}));
    assert_eq!(prov["capture"]["grid"], json!({"cols": 120, "rows": 40}));
    assert_eq!(prov["capture"]["exit_method"], "slash_exit");
    let prov_text = prov.to_string();
    assert!(!prov_text.contains(&words.one) && !prov_text.contains(&words.sky));
    // the definition passes its own fixtures when checked from the file, like "Check with fixtures"
    let report = check_report(&toml, None, &["claude", "codex"]).unwrap();
    assert_eq!(report.status, "ran");
    assert_eq!(report.results.len(), 5);
    assert!(report.results.iter().all(|r| r.passed), "{:?}", report.results);
    assert_eq!(wizard.session_count(), 0, "the session ends with the finish");
}

#[test]
fn a_new_draft_must_be_tested_again() {
    let tree = Tree::new("vf-retest");
    let (wizard, host, id) = visible_session(&tree);
    let v = verified(&wizard, &id, &visible_draft("fake-agent"));
    run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(wizard.read(&id, |s| s.state).unwrap().as_str(), "tested");
    // a changed draft passes verify but has not been tested
    let changed = visible_draft("fake-agent").replace("idle_after_ms = 1500", "idle_after_ms = 1600");
    let v2 = verified(&wizard, &id, &changed);
    assert_eq!(v2["state"], "verified");
    let err = call(&wizard, "finish", json!({"session": id, "provenance": provenance()})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
    assert!(!tree.agents.exists() || std::fs::read_dir(&tree.agents).unwrap().count() == 0);
}

#[test]
fn a_draft_with_both_halves_needs_both_tests_and_writes_both_sets_of_fixtures() {
    let tree = Tree::new("vf-both");
    let (wizard, host, id) = visible_session(&tree);
    let draft = hybrid_draft();
    let v = verified(&wizard, &id, &draft);
    assert!(v["planned"]["fingerprint"].is_string() && v["planned_visible"]["fingerprint"].is_string());
    // the visible test alone is not enough
    run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(wizard.read(&id, |s| s.state).unwrap().as_str(), "verified");
    let err = call(&wizard, "finish", json!({"session": id, "provenance": provenance()})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
    // the one-task mode test completes it
    let live = ok(&wizard, "livetest", live_body(&wizard, &id));
    assert_eq!(live["ok"], true, "{}", live["text"]);
    assert_eq!(live["state"], "tested");
    let done = ok(&wizard, "finish", json!({"session": id, "provenance": provenance()}));
    assert_eq!(done["fixtures"], json!({"passed": 10, "total": 10}), "5 headless + 5 screens");
    let folder = tree.agents.join("fake_agent");
    for name in ["headless_success.jsonl", "headless_garbage.txt", "tui_idle.txt", "tui_idle.expect.toml"] {
        assert!(folder.join(name).is_file(), "{name}");
    }
    let report = check_report(&tree.agents.join("fake_agent.toml"), None, &["claude", "codex"]).unwrap();
    assert!(report.results.iter().all(|r| r.passed), "{:?}", report.results);
}

#[test]
fn a_visible_session_cannot_overwrite_an_existing_definition() {
    let tree = Tree::new("vf-overwrite");
    let (wizard, host, id) = visible_session(&tree);
    let v = verified(&wizard, &id, &visible_draft("fake-agent"));
    run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    std::fs::create_dir_all(&tree.agents).unwrap();
    std::fs::write(tree.agents.join("nimbus.toml"), "# somebody else\n").unwrap();
    let err = call(&wizard, "finish", json!({"session": id, "provenance": provenance()})).unwrap_err();
    assert_eq!(err.code, "AGENT_DEF_DUPLICATE_ID");
    assert_eq!(std::fs::read_to_string(tree.agents.join("nimbus.toml")).unwrap(), "# somebody else\n");
    assert!(!tree.agents.join("nimbus").exists(), "nothing else was left behind");
}
