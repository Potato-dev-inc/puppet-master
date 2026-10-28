//! Store tests: settings, runs, caps, id guards and the private folders. Each test uses its own
//! temp folder (a fresh uuid name), so nothing is shared between tests.

use super::model::{
    valid_run_id, AgentPick, CleanNotes, Config, IssueRun, IssueSnapshot, RepoWatch, RunState, RECORDS_CAP,
};
use super::store::{new_repo_id, new_run_id, Store};
use std::path::PathBuf;

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("pm-issue-intake-{}", uuid::Uuid::new_v4().simple()))
}

fn repo(owner_repo: &str) -> RepoWatch {
    RepoWatch {
        owner_repo: owner_repo.to_string(),
        folder: "C:/Users/you/work/billing".to_string(),
        agent: AgentPick { agent_type: "fake-agent".to_string(), account: None },
        ..RepoWatch::default()
    }
}

fn snapshot() -> IssueSnapshot {
    IssueSnapshot {
        owner_repo: "acme/billing".to_string(),
        number: 42,
        url: "https://github.com/acme/billing/issues/42".to_string(),
        title: "Login button does nothing".to_string(),
        body: "Steps: click it.".to_string(),
        author: "someone".to_string(),
        author_association: "NONE".to_string(),
        labeled_by: "you".to_string(),
        labeled_by_bot: false,
        label_event_id: 9001,
        labeled_at_ms: 1_000,
        notes: CleanNotes::default(),
        sha8: "0a1b2c3d".to_string(),
    }
}

fn run(id: &str, state: RunState) -> IssueRun {
    IssueRun {
        id: id.to_string(),
        repo_id: "rw-01234567".to_string(),
        issue: snapshot(),
        agent: AgentPick { agent_type: "fake-agent".to_string(), account: None },
        agent_label: "Fake agent".to_string(),
        at_ms: 5,
        started_at_ms: None,
        forced_ask: Vec::new(),
        approved_by: None,
        handle: None,
        branch: None,
        base: None,
        git_fingerprint: None,
        state,
    }
}

fn hex_id(prefix: &str, n: u64) -> String {
    format!("{prefix}{n:012x}")
}

#[test]
fn missing_files_read_as_defaults() {
    let store = Store::at(temp_root());
    assert_eq!(store.config(), Config::default());
    assert!(store.runs().is_empty());
    assert!(store.by_id("ir-000000000001").is_none());
}

#[test]
fn config_round_trips_and_tidies_logins_and_label() {
    let store = Store::at(temp_root());
    let mut input = Config { enabled: true, ..Config::default() };
    input.allowed = vec!["Some-One ".to_string(), "some-one".to_string(), "helper[bot]".to_string()];
    let mut watched = repo("acme/billing");
    watched.label = "  puppet  ".to_string();
    input.repos = vec![watched];
    input.ignored = vec![("acme/billing".to_string(), 7), ("acme/billing".to_string(), 7)];

    let saved = store.save_config(&input).unwrap();
    assert_eq!(saved.allowed, vec!["some-one".to_string(), "helper[bot]".to_string()]);
    assert_eq!(saved.repos[0].label, "puppet");
    assert!(saved.repos[0].id.starts_with("rw-") && saved.repos[0].id.len() == 11);
    assert_eq!(saved.ignored, vec![("acme/billing".to_string(), 7)]);
    assert_eq!(store.config(), saved);
}

#[test]
fn twenty_first_repo_is_refused_and_nothing_is_written() {
    let store = Store::at(temp_root());
    let mut twenty: Vec<RepoWatch> = (0..20).map(|i| repo(&format!("acme/repo{i}"))).collect();
    let mut config = Config { repos: twenty.clone(), ..Config::default() };
    assert_eq!(store.save_config(&config).unwrap().repos.len(), 20);
    twenty.push(repo("acme/extra"));
    config.repos = twenty;
    assert!(store.save_config(&config).is_err());
    assert_eq!(store.config().repos.len(), 20);
}

#[test]
fn fifty_first_login_is_refused() {
    let store = Store::at(temp_root());
    let fifty: Vec<String> = (0..50).map(|i| format!("user{i}")).collect();
    let mut config = Config { allowed: fifty.clone(), ..Config::default() };
    assert_eq!(store.save_config(&config).unwrap().allowed.len(), 50);
    let mut fifty_one = fifty;
    fifty_one.push("user50".to_string());
    config.allowed = fifty_one;
    assert!(store.save_config(&config).is_err());
}

#[test]
fn bad_settings_are_refused() {
    let store = Store::at(temp_root());
    let good = repo("acme/billing");
    let cases: Vec<Config> = vec![
        Config { repos: vec![RepoWatch { owner_repo: "../x".into(), ..good.clone() }], ..Config::default() },
        Config { repos: vec![good.clone(), good.clone()], ..Config::default() },
        Config { repos: vec![RepoWatch { label: "a,b".into(), ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { turns: 0, ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { turns: 201, ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { minutes: 4, ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { minutes: 241, ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { runs_per_day: 0, ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { folder: "  ".into(), ..good.clone() }], ..Config::default() },
        Config { repos: vec![RepoWatch { id: "rw-../../x".into(), ..good.clone() }], ..Config::default() },
        Config { allowed: vec!["bad_name".into()], ..Config::default() },
        Config { parallel: 0, ..Config::default() },
        Config { parallel: 4, ..Config::default() },
        Config { ignored: vec![("acme/billing".into(), 0)], ..Config::default() },
    ];
    for config in &cases {
        assert!(store.save_config(config).is_err(), "{config:?} must be refused");
    }
    assert!(!store.root().join("config.json").exists(), "a refused config must not write a file");
}

#[test]
fn runs_are_newest_first_and_capped() {
    let store = Store::at(temp_root());
    for n in 1..=(RECORDS_CAP as u64 + 1) {
        store.put(&run(&hex_id("ir-", n), RunState::Starting)).unwrap();
    }
    let runs = store.runs();
    assert_eq!(runs.len(), RECORDS_CAP);
    assert_eq!(runs[0].id, hex_id("ir-", RECORDS_CAP as u64 + 1));
    assert_eq!(runs.last().unwrap().id, hex_id("ir-", 2), "the oldest run is dropped");
    assert_eq!(store.by_id(&hex_id("ir-", 2)).unwrap().state, RunState::Starting);
}

#[test]
fn update_keeps_the_place_of_a_run() {
    let store = Store::at(temp_root());
    let first = hex_id("ir-", 1);
    let second = hex_id("ir-", 2);
    store.put(&run(&first, RunState::Starting)).unwrap();
    store.put(&run(&second, RunState::Starting)).unwrap();
    store.put(&run(&first, RunState::Declined)).unwrap();
    let runs = store.runs();
    assert_eq!(runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), [second.as_str(), first.as_str()]);
    assert_eq!(runs[1].state, RunState::Declined);
}

#[test]
fn run_round_trips_through_the_file() {
    let store = Store::at(temp_root());
    let mut original = run(&new_run_id(), RunState::Running {
        handle: "issue-42-a1b2".into(),
        operation_id: "op-1".into(),
        pane_id: Some("pane-1".into()),
    });
    original.forced_ask = vec!["Written by someone who is not on your list".into()];
    store.put(&original).unwrap();
    assert_eq!(store.by_id(&original.id).unwrap(), original);
}

#[test]
fn bad_run_id_never_makes_a_file() {
    let root = temp_root();
    let store = Store::at(&root);
    for bad in ["ir-../../x", "ir-0123456789AB", "xx-000000000001", ""] {
        assert!(store.put(&run(bad, RunState::Starting)).is_err(), "{bad:?} must be refused");
        assert!(store.by_id(bad).is_none());
    }
    assert!(!root.exists(), "no folder may be created for a bad id");
}

#[test]
fn by_question_finds_only_stored_question_ids() {
    let store = Store::at(temp_root());
    let asking = hex_id("ir-", 1);
    let pushing = hex_id("ir-", 2);
    store.put(&run(&asking, RunState::Asking { question_id: "q-start".into() })).unwrap();
    store.put(&run(&pushing, RunState::AskPush { question_id: "q-push".into(), facts: facts() })).unwrap();
    assert_eq!(store.by_question("q-start").unwrap().id, asking);
    assert_eq!(store.by_question("q-push").unwrap().id, pushing);
    assert!(store.by_question("q-other").is_none());
    assert!(store.by_question("").is_none());
}

fn facts() -> super::model::PushFacts {
    super::model::PushFacts {
        files: 0,
        additions: 0,
        deletions: 0,
        ci_files: vec![],
        secret_files: vec![],
        secret_lines: vec![],
        home_path_lines: vec![],
        internal_files: vec![],
        git_changed: false,
        remote_branch_exists: false,
        issue_changed: false,
        run_status: "completed".into(),
        turns: 3,
        minutes: 4,
    }
}

#[test]
fn private_folders_are_created_and_empty() {
    let store = Store::at(temp_root());
    for dir in [store.empty_hooks_dir().unwrap(), store.gh_empty_dir().unwrap()] {
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
}

#[test]
fn the_push_block_folder_holds_only_the_refusing_hook() {
    let store = Store::at(temp_root());
    let dir = store.push_block_hooks_dir().unwrap();
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["pre-push"]);
    // Called again after an edit: the hook is right again.
    std::fs::write(dir.join("pre-push"), "#!/bin/sh\nexit 0\n").unwrap();
    store.push_block_hooks_dir().unwrap();
    assert!(std::fs::read_to_string(dir.join("pre-push")).unwrap().contains("exit 1"));
}

#[test]
fn generated_ids_have_the_right_shape() {
    assert!(valid_run_id(&new_run_id()));
    let repo_id = new_repo_id();
    assert!(repo_id.starts_with("rw-") && repo_id.len() == 11);
    assert_ne!(new_run_id(), new_run_id());
}
