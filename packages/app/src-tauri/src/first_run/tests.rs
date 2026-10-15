//! The first-run welcome: shown once, and the first task cannot change anything.

use super::*;
use guard::KnownDirs;

const CASES: &str = include_str!("../../../src/components/simple/folder-cases.json");

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmfirst-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A computer whose profile is nowhere near the temp folder, so temp folders are plain folders.
fn elsewhere() -> KnownDirs {
    KnownDirs { home: "Q:/nobody".into(), onedrive: vec![], projects_root: "Q:/nobody/Documents/PM".into(), system_drive: "Q:".into() }
}

fn repo(_: &Path) -> &'static str {
    "repo"
}

#[test]
fn a_profile_that_never_decided_shows_the_welcome_on_its_own() {
    let s = state(&json!({ "theme": "dark" }));
    assert_eq!(s["show"], true);
    assert_eq!(s["auto"], true, "the UI may veto an automatic show, never a requested one");
    assert!(s["step"].is_null());
}

#[test]
fn done_is_never_shown_again_and_false_asks_for_it_again() {
    let done = state(&json!({ DONE_KEY: true }));
    assert_eq!(done["show"], false);
    assert_eq!(done["auto"], false);
    let again = state(&json!({ DONE_KEY: false, STEP_KEY: "project" }));
    assert_eq!(again["show"], true);
    assert_eq!(again["auto"], false, "a person who asked for it is not vetoed by the first-run checks");
    assert_eq!(again["step"], "project");
}

#[test]
fn a_settings_value_that_is_not_a_bool_counts_as_never_decided() {
    assert_eq!(state(&json!({ DONE_KEY: "yes" }))["auto"], true);
    assert_eq!(state(&Value::Null)["show"], true);
}

#[test]
fn the_first_task_request_is_a_separate_copy_and_read_only_when_it_can_be() {
    let req = task_request(Path::new("C:/code/shop"), "claude", true);
    assert_eq!(req["isolation"], "worktree");
    assert_eq!(req["read_only"], true);
    assert_eq!(req["headless"], true, "read-only is only enforced on a headless run");
    assert_eq!(req["task"], TASK_PROMPT);
    assert_eq!(req["project_path"], "C:/code/shop");
    for flag in ["allow_shell", "trust_workspace", "allow_broad"] {
        assert_eq!(req[flag], false, "{flag} stays off");
    }
    // An agent that cannot enforce read-only still works in a copy, in a pane the person can see.
    let visible = task_request(Path::new("C:/code/shop"), "kilo", false);
    assert_eq!(visible["isolation"], "worktree");
    assert_eq!(visible["read_only"], false);
    assert_eq!(visible["headless"], false);
    assert_eq!(visible["keep_pane"], true);
}

#[test]
fn the_suggested_prompt_asks_for_words_and_says_to_change_nothing() {
    assert!(TASK_PROMPT.contains("Change nothing"));
    assert!(TASK_PROMPT.starts_with("Read this project"));
}

#[test]
fn the_request_is_a_valid_run_request() {
    let req = task_request(Path::new("C:/code/shop"), "claude", true);
    let parsed: crate::agent_runs::AgentRunRequest = serde_json::from_value(req).expect("the run request parses");
    assert!(parsed.read_only);
    assert!(parsed.headless);
    assert_eq!(parsed.isolation.as_deref(), Some("worktree"));
}

#[test]
fn an_agent_with_enforced_read_only_is_found_through_its_features_not_its_name() {
    // The command asks the profile; this pins that the shipped Claude/Codex profiles say yes.
    assert!(crate::tui::profile_for("claude").features().read_only_headless);
    assert!(crate::tui::profile_for("codex").features().read_only_headless);
}

#[test]
fn a_broad_folder_is_refused_before_anything_starts() {
    let value: Value = serde_json::from_str(CASES).unwrap();
    let k = &value["known"];
    let known = KnownDirs {
        home: k["home"].as_str().unwrap().into(),
        onedrive: k["onedrive"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect(),
        projects_root: k["projectsRoot"].as_str().unwrap().into(),
        system_drive: k["systemDrive"].as_str().unwrap().into(),
    };
    let mut refused = 0;
    for case in value["cases"].as_array().unwrap() {
        let path = case["path"].as_str().unwrap();
        if case["verdict"] == "ok" {
            continue;
        }
        let err = plan_task(path, "claude", true, &known, repo).expect_err(path);
        assert!(err.code == "FOLDER_TOO_BROAD" || err.code == "FOLDER_MISSING", "{path}: {}", err.code);
        refused += 1;
    }
    assert!(refused > 15, "the shared list must stay long enough to mean something");
}

#[test]
fn a_folder_without_git_is_sent_to_set_up_git_first() {
    let dir = temp_dir("nogit");
    let err = plan_task(&dir.to_string_lossy(), "claude", true, &elsewhere(), |_| "folder").unwrap_err();
    assert_eq!(err.code, "FOLDER_NEEDS_GIT");
    let err = plan_task(&dir.to_string_lossy(), "claude", true, &elsewhere(), |_| "no_commits").unwrap_err();
    assert_eq!(err.code, "FOLDER_NEEDS_GIT");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_folder_and_a_missing_agent_are_refused() {
    let gone = std::env::temp_dir().join("pmfirst-does-not-exist-xyz");
    assert_eq!(plan_task(&gone.to_string_lossy(), "claude", true, &elsewhere(), repo).unwrap_err().code, "FOLDER_MISSING");
    let dir = temp_dir("noagent");
    assert_eq!(plan_task(&dir.to_string_lossy(), "  ", true, &elsewhere(), repo).unwrap_err().code, "AGENT_MISSING");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_small_repo_gets_a_plan_with_the_copy_request() {
    let dir = temp_dir("ok");
    std::fs::write(dir.join("a.txt"), "hello").unwrap();
    let plan = plan_task(&dir.to_string_lossy(), "claude", true, &elsewhere(), repo).unwrap();
    assert!(plan.enforced);
    assert_eq!(plan.request["isolation"], "worktree");
    assert_eq!(plan.request["read_only"], true);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_summary_carries_only_what_the_welcome_shows() {
    let view = json!({
        "status": "completed", "result": "It sells shoes.", "error": null, "pane_id": null,
        "receipt": { "outcome": "completed_unchecked", "read_only": "enforced" },
        "handle": "h", "operation_id": "secret-ish", "prompt": { "a": 1 },
    });
    let s = summarize(&view);
    assert_eq!(s["status"], "completed");
    assert_eq!(s["result"], "It sells shoes.");
    assert_eq!(s["receipt"]["read_only"], "enforced");
    assert!(s.get("operation_id").is_none());
    assert!(s.get("handle").is_none());
    assert!(is_final("completed") && is_final("failed") && is_final("cancelled"));
    assert!(!is_final("running") && !is_final("waiting_input"));
}
