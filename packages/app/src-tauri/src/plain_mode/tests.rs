//! Guards of Simple mode: a broad folder is refused, and a copy is always used.

use super::guard::{self, KnownDirs, Verdict};
use super::*;
use std::process::Command;

const CASES: &str = include_str!("../../../src/components/simple/folder-cases.json");

fn known_from(value: &Value) -> KnownDirs {
    let k = &value["known"];
    KnownDirs {
        home: k["home"].as_str().unwrap().into(),
        onedrive: k["onedrive"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect(),
        projects_root: k["projectsRoot"].as_str().unwrap().into(),
        system_drive: k["systemDrive"].as_str().unwrap().into(),
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmplain-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A computer whose profile is nowhere near the temp folder, so temp folders are plain folders.
fn elsewhere() -> KnownDirs {
    KnownDirs { home: "Q:/nobody".into(), onedrive: vec![], projects_root: "Q:/nobody/Documents/PM".into(), system_drive: "Q:".into() }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").output().expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn input(name: &str, existing: Option<&str>) -> StartInput {
    StartInput { name: name.into(), prompt: "Make a quiz.".into(), agent: "claude".into(), existing_folder: existing.map(str::to_string) }
}

#[test]
fn every_case_in_the_shared_list_gets_its_verdict() {
    let value: Value = serde_json::from_str(CASES).unwrap();
    let known = known_from(&value);
    let cases = value["cases"].as_array().unwrap();
    assert!(cases.len() > 20, "the list must stay long enough to mean something");
    for case in cases {
        let path = case["path"].as_str().unwrap();
        let want = case["verdict"].as_str().unwrap();
        assert_eq!(guard::verdict(path, &known).as_str(), want, "folder {path:?}");
    }
}

#[test]
fn a_broad_folder_is_refused_before_anything_is_made() {
    let value: Value = serde_json::from_str(CASES).unwrap();
    let known = known_from(&value);
    let root = PathBuf::from(&known.projects_root);
    for broad in [
        "C:\\Users\\Ana",
        "C:\\Users\\Ana\\Desktop",
        "C:\\Users\\Ana\\Documents",
        "C:\\Users\\Ana\\Downloads",
        "C:\\Users\\Ana\\OneDrive",
        "C:\\",
        "C:\\Windows",
        "C:\\Program Files",
    ] {
        let refusal = plan(&input("Quiz", Some(broad)), &known, &root).expect_err(broad);
        assert_eq!(refusal.code, "FOLDER_TOO_BROAD", "{broad}");
    }
    assert!(!root.exists(), "a refusal must not create the projects folder");
}

#[test]
fn a_new_project_always_lands_inside_the_projects_folder() {
    let known = elsewhere();
    let root = temp_dir("root");
    for name in ["Plants quiz", "..\\..\\Windows", "C:\\Users\\Ana", "a/b/c", "CON", "", "   ", "x".repeat(200).as_str()] {
        let plan = plan(&input(name, None), &known, &root).unwrap();
        assert!(plan.is_new);
        assert_eq!(plan.origin.parent().unwrap(), root, "{name:?} -> {:?}", plan.origin);
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_taken_name_gets_a_number() {
    let known = elsewhere();
    let root = temp_dir("taken");
    std::fs::create_dir_all(root.join("Quiz")).unwrap();
    std::fs::create_dir_all(root.join("Quiz 2")).unwrap();
    let plan = plan(&input("Quiz", None), &known, &root).unwrap();
    assert_eq!(plan.origin, root.join("Quiz 3"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_copy_is_always_used() {
    let root = temp_dir("copy-req");
    let known = elsewhere();
    // New project, a folder she has: the request never lets the agent work in her folder itself.
    let existing = temp_dir("existing");
    for existing_folder in [None, Some(existing.to_string_lossy().into_owned())] {
        let plan = plan(&input("Quiz", existing_folder.as_deref()), &known, &root).unwrap();
        assert_eq!(plan.request["isolation"], "worktree");
        assert_eq!(plan.request["project_path"], plan.origin.to_string_lossy().as_ref());
        assert_eq!(plan.request["allow_shell"], false);
        assert_eq!(plan.request["trust_workspace"], false);
        // The request parses as a real run request, with the copy switch on.
        let run: crate::agent_runs::AgentRunRequest = serde_json::from_value(plan.request.clone()).unwrap();
        assert_eq!(run.isolation.as_deref(), Some("worktree"));
        assert!(run.keep_pane && !run.headless && !run.read_only);
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&existing);
}

#[test]
fn the_agent_works_in_a_copy_and_her_folder_is_untouched() {
    let known = elsewhere();
    let root = temp_dir("copy-real");
    let plan = plan(&input("Plants quiz", None), &known, &root).unwrap();
    prepare(&plan).unwrap();
    assert!(plan.origin.join(".git").exists(), "the new project can save versions");
    let before = git(&plan.origin, &["rev-parse", "HEAD"]);

    let reference = crate::worktrees::create(&plan.origin.to_string_lossy(), "plain-test").unwrap();
    let cwd = PathBuf::from(&reference.cwd);
    assert_ne!(cwd, plan.origin, "the agent must not run in her folder");
    assert!(cwd.starts_with(&plan.origin) || cwd.exists());

    // The agent writes in the copy: her folder shows nothing until Keep.
    std::fs::write(cwd.join("tool.html"), "<h1>quiz</h1>").unwrap();
    assert!(!plan.origin.join("tool.html").exists());
    assert_eq!(git(&plan.origin, &["rev-parse", "HEAD"]), before);

    crate::worktrees::discard(&reference).unwrap();
    assert!(!plan.origin.join("tool.html").exists(), "Throw away leaves her folder as it was");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_folder_without_saved_versions_is_not_copied_yet() {
    let known = elsewhere();
    let root = temp_dir("root2");
    let plain = temp_dir("plainfolder");
    std::fs::write(plain.join("orders.csv"), "a,b").unwrap();
    let plan = plan(&input("Orders", Some(&plain.to_string_lossy())), &known, &root).unwrap();
    let refusal = prepare(&plan).unwrap_err();
    assert_eq!(refusal.code, "FOLDER_NEEDS_SETUP");
    assert!(!plain.join(".git").exists(), "PM must not set up Git inside a folder she owns");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&plain);
}

#[test]
fn a_folder_that_is_already_a_repository_is_accepted() {
    let known = elsewhere();
    let root = temp_dir("root3");
    let repo = temp_dir("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let plan = plan(&input("Mine", Some(&repo.to_string_lossy())), &known, &root).unwrap();
    assert!(!plan.is_new);
    prepare(&plan).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn a_folder_with_too_many_files_is_too_big() {
    let dir = temp_dir("big");
    assert!(guard::small_enough(&dir));
    let many = dir.join("many");
    std::fs::create_dir_all(&many).unwrap();
    for i in 0..(guard::MAX_FILES + 1) {
        std::fs::write(many.join(format!("{i}.txt")), "").unwrap();
    }
    assert!(!guard::small_enough(&dir));
    // node_modules does not count (it is never copied).
    let dir2 = temp_dir("big2");
    let nm = dir2.join("node_modules");
    std::fs::create_dir_all(&nm).unwrap();
    for i in 0..(guard::MAX_FILES + 1) {
        std::fs::write(nm.join(format!("{i}.txt")), "").unwrap();
    }
    assert!(guard::small_enough(&dir2));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn an_empty_prompt_or_agent_is_refused() {
    let known = elsewhere();
    let root = temp_dir("root4");
    let mut empty = input("Quiz", None);
    empty.prompt = "  ".into();
    assert_eq!(plan(&empty, &known, &root).unwrap_err().code, "PROMPT_EMPTY");
    let mut no_agent = input("Quiz", None);
    no_agent.agent = String::new();
    assert_eq!(plan(&no_agent, &known, &root).unwrap_err().code, "AGENT_MISSING");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verdicts_have_stable_names() {
    assert_eq!(Verdict::AboveHome.as_str(), "above_home");
    assert!(Verdict::Ok.is_ok() && !Verdict::Home.is_ok());
}
