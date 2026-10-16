//! Finish: the files, never overwriting, unapproved, rollback, provenance.

use serde_json::{json, Value};

use crate::tui::declarative::approvals::Approvals;
use crate::tui::declarative::check::check_report;
use crate::tui::declarative::loader::{build, LoadInputs};
use crate::tui::declarative::wizard::finish::{parse_provenance, Provenance};

use super::support::{call, ok, provenance, tested, verified, Tree, GOOD};

fn code_ids() -> Vec<&'static str> {
    vec!["claude", "codex", "opencode", "opencode_native", "cursor", "cursor_agent", "cmd", "powershell", "bash"]
}

fn finish(wizard: &crate::tui::declarative::wizard::Wizard, id: &str) -> Result<Value, crate::tui::declarative::wizard::WizardError> {
    call(wizard, "finish", json!({"session": id, "provenance": provenance()}))
}

#[test]
fn it_writes_the_definition_its_fixtures_and_the_provenance() {
    let tree = Tree::new("f-files");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    let out = finish(&wizard, &id).unwrap();
    assert_eq!(out["ok"], true);
    assert_eq!(out["id"], "fake_agent");
    assert_eq!(out["state"], "needs_approval");
    assert_eq!(out["fixtures"], json!({"passed": 5, "total": 5}));

    let toml_path = tree.agents.join("fake_agent.toml");
    assert_eq!(std::fs::read_to_string(&toml_path).unwrap(), GOOD, "the verified text, byte for byte");
    let mut names: Vec<String> = std::fs::read_dir(tree.agents.join("fake_agent"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "headless_error.expect.toml",
            "headless_error.jsonl",
            "headless_garbage.expect.toml",
            "headless_garbage.txt",
            "headless_resume.expect.toml",
            "headless_resume.jsonl",
            "headless_sentence.expect.toml",
            "headless_sentence.jsonl",
            "headless_success.expect.toml",
            "headless_success.jsonl",
        ]
    );
    // the fixtures are the real output, with expectations from the stages
    let success = std::fs::read_to_string(tree.agents.join("fake_agent/headless_success.expect.toml")).unwrap();
    assert!(success.contains(&format!("result = \"{}\"", words.one)) && success.contains("session = \"fk-0001\""), "{success}");
    let sentence = std::fs::read_to_string(tree.agents.join("fake_agent/headless_sentence.expect.toml")).unwrap();
    assert!(sentence.contains(&format!("result = \"the sky is {}.\"", words.sky)), "{sentence}");
    assert!(success.contains("model = \"fake/fast\"") && success.contains("done = true"), "{success}");
    let error = std::fs::read_to_string(tree.agents.join("fake_agent/headless_error.expect.toml")).unwrap();
    assert!(error.contains("result = \"\"") && error.contains("exit = 1") && error.contains("Model not found"), "{error}");
    assert!(std::fs::read_to_string(tree.agents.join("fake_agent/headless_resume.jsonl")).unwrap().contains("\"resumed\":true"));
}

#[test]
fn the_written_definition_validates_and_its_fixtures_pass_the_normal_check() {
    let tree = Tree::new("f-check");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    finish(&wizard, &id).unwrap();
    let report = check_report(&tree.agents.join("fake_agent.toml"), None, &code_ids()).unwrap();
    assert_eq!(report.status, "ran");
    assert_eq!(report.results.len(), 5);
    for r in &report.results {
        assert!(r.passed, "{}: {:?}", r.fixture, r.failures);
    }
}

#[test]
fn the_definition_is_written_unapproved() {
    let tree = Tree::new("f-unapproved");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    finish(&wizard, &id).unwrap();
    // no approvals file was created or touched
    let approvals_path = tree.root.join("agent-approvals.json");
    assert!(!approvals_path.exists());
    let ids = code_ids();
    let inputs = LoadInputs { bundled: &[], user_dir: Some(&tree.agents), code_ids: &ids, projects: &[], extra: &[] };
    let built = build(&inputs, &Approvals::load(&approvals_path));
    assert_eq!(built.entries.len(), 1);
    assert_eq!(built.entries[0].id.as_deref(), Some("fake_agent"));
    assert_eq!(built.entries[0].state.as_str(), "needs_approval");
    assert!(built.profiles.is_empty(), "nothing is registered until the person approves");
}

#[test]
fn provenance_records_who_wrote_it_and_what_was_tested_without_paths() {
    let tree = Tree::new("f-prov");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    finish(&wizard, &id).unwrap();
    let text = std::fs::read_to_string(tree.agents.join("fake_agent.wizard.json")).unwrap();
    let prov: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(prov["wizard_schema"], 1);
    assert_eq!(prov["id"], "fake_agent");
    assert_eq!(prov["command"], "fake-agent");
    assert_eq!(prov["drafter"], json!({"name": "Test drafter", "kind": "other", "model": null}));
    assert_eq!(prov["prompt_version"], "1");
    assert_eq!(prov["rounds"], 1);
    assert_eq!(prov["tested_version"], "1.4.2");
    assert_eq!(prov["app_build"], "test-build");
    assert_eq!(prov["definition_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(prov["results"]["fixtures"], json!({"passed": 5, "total": 5}));
    assert_eq!(prov["results"]["stages"][0], json!({"name": "pong", "status": "passed"}));
    assert!(prov["created_at"].as_str().unwrap().contains('T'));
    let root = tree.root.to_string_lossy().replace('\\', "/").to_lowercase();
    let lowered = text.replace("\\\\", "/").to_lowercase();
    assert!(!lowered.contains(&root), "no absolute paths in the provenance: {text}");
    assert!(!lowered.contains("users/"), "{text}");
}

#[test]
fn an_existing_definition_is_never_overwritten() {
    let tree = Tree::new("f-nooverwrite");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    // someone adds the file between the check and the finish
    std::fs::create_dir_all(&tree.agents).unwrap();
    let theirs = tree.agents.join("fake_agent.toml");
    std::fs::write(&theirs, "# theirs\n").unwrap();
    let err = finish(&wizard, &id).unwrap_err();
    assert_eq!((err.status, err.code), (409, "AGENT_DEF_DUPLICATE_ID"));
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "# theirs\n");
    assert!(!tree.agents.join("fake_agent").exists(), "no fixtures folder was created");
    assert!(!tree.agents.join("fake_agent.wizard.json").exists());
    // the session is still tested and can finish once the name is free
    std::fs::remove_file(&theirs).unwrap();
    assert_eq!(finish(&wizard, &id).unwrap()["ok"], true);
}

#[test]
fn an_existing_fixtures_folder_or_provenance_file_is_left_alone() {
    let tree = Tree::new("f-existing");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    std::fs::create_dir_all(tree.agents.join("fake_agent")).unwrap();
    std::fs::write(tree.agents.join("fake_agent/keep.txt"), "mine").unwrap();
    let err = finish(&wizard, &id).unwrap_err();
    assert_eq!(err.code, "WIZARD_FIXTURES_DIR_EXISTS");
    assert_eq!(std::fs::read_to_string(tree.agents.join("fake_agent/keep.txt")).unwrap(), "mine");
    assert!(!tree.agents.join("fake_agent.toml").exists());
    std::fs::remove_dir_all(tree.agents.join("fake_agent")).unwrap();

    std::fs::write(tree.agents.join("fake_agent.wizard.json"), "{}").unwrap();
    let err = finish(&wizard, &id).unwrap_err();
    assert_eq!(err.code, "WIZARD_PROVENANCE_EXISTS");
    assert_eq!(std::fs::read_to_string(tree.agents.join("fake_agent.wizard.json")).unwrap(), "{}");
    assert!(!tree.agents.join("fake_agent").exists() && !tree.agents.join("fake_agent.toml").exists());
}

#[test]
fn a_draft_that_was_not_live_tested_cannot_be_finished() {
    let tree = Tree::new("f-untested");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let err = finish(&wizard, &id).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    assert!(!tree.agents.exists());
}

#[test]
fn a_finished_session_is_gone_and_its_work_folder_with_it() {
    let tree = Tree::new("f-gone");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    finish(&wizard, &id).unwrap();
    assert_eq!(wizard.session_count(), 0);
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 0);
    assert_eq!(finish(&wizard, &id).unwrap_err().code, "WIZARD_SESSION_NOT_FOUND");
}

#[test]
fn provenance_input_is_checked() {
    let good = json!({"provenance": {"drafter": "Claude Code (local)", "drafter_kind": "local_agent", "model": null, "prompt_version": "1", "rounds": 2}});
    assert_eq!(
        parse_provenance(&good).unwrap(),
        Provenance {
            drafter: "Claude Code (local)".into(),
            drafter_kind: "local_agent".into(),
            model: None,
            prompt_version: "1".into(),
            rounds: 2
        }
    );
    let bad = |p: Value| parse_provenance(&json!({"provenance": p})).unwrap_err();
    assert_eq!(parse_provenance(&json!({})).unwrap_err().code, "INVALID_ARGUMENT");
    assert!(bad(json!({"drafter": "x\\y", "prompt_version": "1", "rounds": 1})).message.contains("drafter"));
    assert!(bad(json!({"drafter": "a\nb", "prompt_version": "1", "rounds": 1})).message.contains("drafter"));
    assert!(bad(json!({"drafter": "a".repeat(81), "prompt_version": "1", "rounds": 1})).message.contains("80"));
    assert!(bad(json!({"drafter": "ok", "drafter_kind": "robot", "prompt_version": "1", "rounds": 1})).message.contains("drafter_kind"));
    assert!(bad(json!({"drafter": "ok", "rounds": 1})).message.contains("prompt_version"));
    assert!(bad(json!({"drafter": "ok", "prompt_version": "1", "rounds": 0})).message.contains("rounds"));
    assert!(bad(json!({"drafter": "ok", "prompt_version": "1", "rounds": 99})).message.contains("rounds"));
}

#[test]
fn bad_provenance_is_refused_before_anything_is_written() {
    let tree = Tree::new("f-badprov");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    let err = call(&wizard, "finish", json!({"session": id, "provenance": {"drafter": "x/../../y\\z"}})).unwrap_err();
    assert_eq!(err.status, 400);
    assert!(!tree.agents.exists());
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "tested");
}

#[test]
fn drop_removes_nothing_when_finish_succeeds() {
    let tree = Tree::new("f-keep");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    let out = finish(&wizard, &id).unwrap();
    assert!(std::path::Path::new(out["file"].as_str().unwrap()).is_file());
    assert!(std::path::Path::new(out["fixtures_dir"].as_str().unwrap()).is_dir());
    assert!(std::path::Path::new(out["provenance_file"].as_str().unwrap()).is_file());
    let _ = ok;
}
