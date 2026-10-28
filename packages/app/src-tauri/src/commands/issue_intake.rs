//! Tauri commands of "Assign an issue" (SPEC n32 section 3.7). Desktop only: no bridge route and no MCP
//! tool. The commands are thin: the runs and the engine live in `issue_intake::live`, the saved settings
//! in `issue_intake::store`. What this file adds: the save-time checks that need the outside world (the
//! folder's GitHub origin, the agent's readiness), the baseline time of a new repo, and the checks of the
//! arguments of `issue_intake_act` before the engine sees them.
//!
//! Every command runs its work off the UI thread (git and `gh` calls can take a moment) and returns an
//! `OperationError` (a code and one plain sentence) when it refuses. The names and argument names match
//! `packages/app/src/lib/issue-intake.ts`.

use super::AppState;
use crate::issue_intake::live::{self, Io, LiveIo};
use crate::issue_intake::model::{self, Config, RepoWatch, ERR_AGENT_NOT_READY, ERR_REPO_MISMATCH};
use crate::issue_intake::store::Store;
use crate::issue_intake::view::{IntakeView, RunView};
use crate::operations::OperationError;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, State};

/// The buttons a run row can take (`IntakeAction` in the TS file). The engine checks them again.
const ACTIONS: [&str; 5] = ["start", "ask_again", "push", "throw_away", "unignore"];

/// What `issue_intake_check_folder` says about a folder the person picked (`FolderCheck` in the TS file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderCheck {
    /// The `owner/repo` the folder's `origin` points at.
    pub owner_repo: Option<String>,
    pub default_branch: Option<String>,
    /// `ready`, `missing` or `signed_out`.
    pub gh: String,
    pub why_not: Option<String>,
}

/// What saving and checking a folder need from the outside world. The app answers from [`LiveIo`]
/// (below); the tests answer from a table, so no test touches git, `gh`, an agent or the audit log.
trait Outside {
    /// The `owner/repo` of the folder's `origin`, for a folder that is the top of a clone with one.
    fn origin_of(&self, folder: &str) -> Option<String>;
    /// The default branch GitHub reports for the folder's repository (asked only when `gh` is ready).
    fn default_branch_of(&self, folder: &str) -> Option<String>;
    fn agent_ready(&self, agent_type: &str) -> bool;
    fn gh_state(&self) -> String;
    /// One audit line for a settings change. `detail` never carries a token or a folder path.
    fn record_settings(&self, summary: &str, detail: Value);
}

impl Outside for LiveIo {
    fn origin_of(&self, folder: &str) -> Option<String> {
        let folder = folder.trim();
        if folder.is_empty() || !Path::new(folder).is_dir() {
            return None;
        }
        // The folder must be the top of its clone. A folder inside someone else's clone would
        // otherwise borrow that clone's origin.
        if !is_toplevel(Path::new(folder)) {
            return None;
        }
        Io::folder_owner_repo(self, folder)
    }

    fn default_branch_of(&self, folder: &str) -> Option<String> {
        Io::repo_facts(self, Path::new(folder.trim())).ok().and_then(|facts| facts.default_branch)
    }

    fn agent_ready(&self, agent_type: &str) -> bool {
        Io::agent_ready(self, agent_type)
    }

    fn gh_state(&self) -> String {
        Io::gh_state(self)
    }

    fn record_settings(&self, summary: &str, detail: Value) {
        Io::audit(self, "settings_changed", "desktop", summary, detail);
    }
}

/// True when `folder` is the top level of the git repository it sits in.
fn is_toplevel(folder: &Path) -> bool {
    let Some(top) = crate::team::git::toplevel(folder) else { return false };
    let (Ok(top), Ok(here)) = (std::fs::canonicalize(top), std::fs::canonicalize(folder)) else {
        return false;
    };
    if cfg!(windows) {
        // Windows paths are case-insensitive; git and the file system may spell the drive differently.
        top.to_string_lossy().to_lowercase() == here.to_string_lossy().to_lowercase()
    } else {
        top == here
    }
}

fn invalid(message: impl Into<String>) -> OperationError {
    OperationError::new("INVALID_ARGUMENT", message, true)
}

/// Runs one blocking job on Tauri's pool. A panic or a cancelled job becomes an `INTERNAL` error.
async fn off_ui<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T, OperationError> {
    tauri::async_runtime::spawn_blocking(job).await.map_err(|error| OperationError::new("INTERNAL", error.to_string(), false))
}

/// The app's world for the save and the folder check.
fn live_world(state: &State<'_, AppState>, app: AppHandle) -> LiveIo {
    LiveIo::new(live::store(), state.registry.clone(), app)
}

// ------------------------------------------------------------------------------- save

/// Give each new repo the baseline time now (labels older than that are never started by themselves).
/// A repo that is still the same watch (same id, same repository) keeps its saved baseline. The
/// client's `added_at_ms` is never trusted: the form sends `0` for a new repo.
fn stamp_baselines(incoming: &Config, saved: &Config, now_ms: u64) -> Config {
    let mut stamped = incoming.clone();
    for repo in &mut stamped.repos {
        let kept = saved.repos.iter().find(|old| old.id == repo.id && old.owner_repo == repo.owner_repo);
        repo.added_at_ms = kept.map_or(now_ms, |old| old.added_at_ms);
    }
    stamped
}

/// The checks that need the outside world, run before anything is written. Each watched repo needs
/// (S3) a folder that is the top of a clone whose `origin` is that repo, and an agent that is ready.
/// Two rows with one id are refused here; two rows with one repo are refused by the store.
fn check_repos(cfg: &Config, world: &dyn Outside) -> Result<(), OperationError> {
    for (index, repo) in cfg.repos.iter().enumerate() {
        if cfg.repos[..index].iter().any(|other| other.id == repo.id) {
            return Err(invalid("Two watched repositories share one id. Remove one of them."));
        }
        check_origin(repo, world)?;
        if !world.agent_ready(&repo.agent.agent_type) {
            return Err(OperationError::new(
                ERR_AGENT_NOT_READY,
                format!("The agent for {} is not ready. Pick another agent, or set this one up first.", repo.owner_repo),
                true,
            ));
        }
    }
    Ok(())
}

fn check_origin(repo: &RepoWatch, world: &dyn Outside) -> Result<(), OperationError> {
    let origin = world.origin_of(&repo.folder);
    // GitHub names are not case-sensitive.
    let same = origin.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(&repo.owner_repo));
    if same {
        return Ok(());
    }
    let message = match origin {
        Some(other) => format!(
            "The folder for {} is a clone of {other}. Pick the folder of {} itself.",
            repo.owner_repo, repo.owner_repo
        ),
        None => format!(
            "The folder for {} is not the top of a clone with a GitHub origin for it. Pick that clone's folder.",
            repo.owner_repo
        ),
    };
    Err(OperationError::new(ERR_REPO_MISMATCH, message, true))
}

/// The save: stamp the baselines, check each repo against the outside world, then let the store check
/// the values and write. A refused save writes nothing and audits nothing.
fn save_with(store: &Store, incoming: Config, now_ms: u64, world: &dyn Outside) -> Result<Config, OperationError> {
    let before = store.config();
    let stamped = stamp_baselines(&incoming, &before, now_ms);
    check_repos(&stamped, world)?;
    let saved = store.save_config(&stamped).map_err(invalid)?;
    world.record_settings("settings changed", settings_diff(&before, &saved));
    Ok(saved)
}

/// What changed, for the audit line: repos added, removed and changed (by id), and the Labs switch.
/// Counts and names of the person's own repositories only.
fn settings_diff(before: &Config, after: &Config) -> Value {
    let names = |repos: Vec<&RepoWatch>| repos.iter().map(|repo| repo.owner_repo.clone()).collect::<Vec<String>>();
    let added = names(after.repos.iter().filter(|repo| !before.repos.iter().any(|old| old.id == repo.id)).collect());
    let removed = names(before.repos.iter().filter(|repo| !after.repos.iter().any(|new| new.id == repo.id)).collect());
    let changed = names(
        after
            .repos
            .iter()
            .filter(|repo| before.repos.iter().any(|old| old.id == repo.id && old != *repo))
            .collect(),
    );
    json!({
        "enabled": after.enabled,
        "was_enabled": before.enabled,
        "repos_added": added,
        "repos_removed": removed,
        "repos_changed": changed,
        "allowed_count": after.allowed.len(),
        "parallel": after.parallel,
    })
}

#[tauri::command]
pub async fn issue_intake_save(config: Config, state: State<'_, AppState>, app: AppHandle) -> Result<IntakeView, OperationError> {
    let store = live::store();
    let world = live_world(&state, app);
    off_ui(move || -> Result<IntakeView, OperationError> {
        save_with(&store, config, crate::questions::now_ms(), &world)?;
        live::notify_changed();
        Ok(live::view())
    })
    .await?
}

// ------------------------------------------------------------------------------- folder

/// What the folder picker shows: the folder's repo, its default branch and why it cannot be used.
fn check_folder_with(folder: &str, world: &dyn Outside) -> FolderCheck {
    let gh = world.gh_state();
    let owner_repo = world.origin_of(folder);
    let default_branch = match (&owner_repo, gh.as_str()) {
        (Some(_), "ready") => world.default_branch_of(folder),
        _ => None,
    };
    let why_not = if owner_repo.is_none() {
        Some("This folder is not the top of a clone with a GitHub origin.".to_string())
    } else if gh == "missing" {
        Some("The GitHub command line tool (gh) is not installed.".to_string())
    } else if gh == "signed_out" {
        Some("Sign in to GitHub with gh first.".to_string())
    } else {
        None
    };
    FolderCheck { owner_repo, default_branch, gh, why_not }
}

#[tauri::command]
pub async fn issue_intake_check_folder(folder: String, state: State<'_, AppState>, app: AppHandle) -> Result<FolderCheck, OperationError> {
    let world = live_world(&state, app);
    off_ui(move || check_folder_with(&folder, &world)).await
}

// ------------------------------------------------------------------------------- view, act, poll

#[tauri::command]
pub async fn issue_intake_view() -> Result<IntakeView, OperationError> {
    off_ui(live::view).await
}

/// The arguments of `issue_intake_act`: a valid run id and one of the five buttons. Anything else is
/// refused before the engine is asked.
fn check_act_args(run_id: &str, action: &str) -> Result<(), OperationError> {
    if !model::valid_run_id(run_id) {
        return Err(invalid("That is not an issue run id."));
    }
    if !ACTIONS.contains(&action) {
        return Err(invalid("That is not a button a run can take."));
    }
    Ok(())
}

#[tauri::command]
pub async fn issue_intake_act(run_id: String, action: String) -> Result<RunView, OperationError> {
    check_act_args(&run_id, &action)?;
    off_ui(move || live::act(&run_id, &action, "desktop")).await?
}

#[tauri::command]
pub async fn issue_intake_poll_now() -> Result<(), OperationError> {
    off_ui(live::poll_now).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const NOW: u64 = 1_700_000_000_000;

    /// A fake world: each folder has an origin (or none), some agents are ready, and audit lines are kept.
    struct FakeWorld {
        origins: Vec<(&'static str, Option<&'static str>)>,
        ready: Vec<&'static str>,
        gh: &'static str,
        branch: Option<&'static str>,
        audits: RefCell<Vec<String>>,
    }

    impl FakeWorld {
        fn with(origins: Vec<(&'static str, Option<&'static str>)>) -> Self {
            Self { origins, ready: vec!["codex"], gh: "ready", branch: Some("main"), audits: RefCell::new(Vec::new()) }
        }
    }

    impl Outside for FakeWorld {
        fn origin_of(&self, folder: &str) -> Option<String> {
            self.origins.iter().find(|(path, _)| *path == folder).and_then(|(_, origin)| origin.map(str::to_string))
        }
        fn default_branch_of(&self, _folder: &str) -> Option<String> {
            self.branch.map(str::to_string)
        }
        fn agent_ready(&self, agent_type: &str) -> bool {
            self.ready.contains(&agent_type)
        }
        fn gh_state(&self) -> String {
            self.gh.to_string()
        }
        fn record_settings(&self, summary: &str, _detail: Value) {
            self.audits.borrow_mut().push(summary.to_string());
        }
    }

    /// A store in a fresh folder under the temp dir (tests never touch the app's data folder).
    fn fresh_store() -> Store {
        let root = std::env::temp_dir().join(format!("pm-issue-cmd-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).expect("temp folder");
        Store::at(root)
    }

    fn repo(id: &str, owner_repo: &str, folder: &str, agent: &str) -> RepoWatch {
        RepoWatch {
            id: id.to_string(),
            owner_repo: owner_repo.to_string(),
            folder: folder.to_string(),
            label: "puppet".to_string(),
            agent: model::AgentPick { agent_type: agent.to_string(), account: None },
            added_at_ms: 0,
            ..RepoWatch::default()
        }
    }

    fn config_with(repos: Vec<RepoWatch>) -> Config {
        Config { enabled: true, repos, ..Config::default() }
    }

    const FOLDER: &str = "C:/work/billing";
    const ID: &str = "rw-0123abcd";

    fn code_of(result: Result<Config, OperationError>) -> String {
        result.expect_err("the save must be refused").code
    }

    #[test]
    fn save_stamps_the_baseline_of_a_new_repo_and_ignores_the_client_value() {
        let store = fresh_store();
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/billing"))]);
        let incoming = config_with(vec![repo(ID, "acme/billing", FOLDER, "codex")]);
        let saved = save_with(&store, incoming, NOW, &world).expect("a good save");
        assert_eq!(saved.repos[0].added_at_ms, NOW);
        assert_eq!(store.config().repos[0].added_at_ms, NOW);
        assert_eq!(world.audits.borrow().as_slice(), ["settings changed"]);
    }

    #[test]
    fn save_keeps_the_saved_baseline_of_a_repo_that_is_still_the_same_watch() {
        let store = fresh_store();
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/billing"))]);
        let mut old = repo(ID, "acme/billing", FOLDER, "codex");
        old.added_at_ms = 42;
        store.save_config(&config_with(vec![old])).expect("seed");
        let incoming = config_with(vec![repo(ID, "acme/billing", FOLDER, "codex")]);
        let saved = save_with(&store, incoming, NOW, &world).expect("a good save");
        assert_eq!(saved.repos[0].added_at_ms, 42);
    }

    #[test]
    fn save_stamps_a_new_baseline_when_an_id_is_reused_for_another_repo() {
        let stamped = stamp_baselines(
            &config_with(vec![repo(ID, "acme/other", FOLDER, "codex")]),
            &config_with(vec![RepoWatch { added_at_ms: 42, ..repo(ID, "acme/billing", FOLDER, "codex") }]),
            NOW,
        );
        assert_eq!(stamped.repos[0].added_at_ms, NOW);
    }

    #[test]
    fn save_refuses_a_bad_label_and_writes_nothing() {
        let store = fresh_store();
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/billing"))]);
        let mut bad = repo(ID, "acme/billing", FOLDER, "codex");
        bad.label = "a,b".to_string();
        let result = save_with(&store, config_with(vec![bad]), NOW, &world);
        assert_eq!(result.expect_err("bad label").code, "INVALID_ARGUMENT");
        assert!(store.config().repos.is_empty(), "a refused save writes nothing");
        assert!(world.audits.borrow().is_empty(), "a refused save audits nothing");
    }

    #[test]
    fn save_refuses_a_folder_whose_origin_is_another_repo() {
        let store = fresh_store();
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/other"))]);
        let result = save_with(&store, config_with(vec![repo(ID, "acme/billing", FOLDER, "codex")]), NOW, &world);
        assert_eq!(code_of(result), ERR_REPO_MISMATCH);
        assert!(store.config().repos.is_empty());
    }

    #[test]
    fn save_refuses_a_folder_with_no_origin_or_no_folder() {
        let world = FakeWorld::with(vec![(FOLDER, None)]);
        let no_origin = check_repos(&config_with(vec![repo(ID, "acme/billing", FOLDER, "codex")]), &world);
        assert_eq!(no_origin.expect_err("no origin").code, ERR_REPO_MISMATCH);
        let no_folder = check_repos(&config_with(vec![repo(ID, "acme/billing", "C:/nowhere", "codex")]), &world);
        assert_eq!(no_folder.expect_err("no folder").code, ERR_REPO_MISMATCH);
    }

    #[test]
    fn origin_match_ignores_case_because_github_names_do_not() {
        let world = FakeWorld::with(vec![(FOLDER, Some("Acme/Billing"))]);
        check_repos(&config_with(vec![repo(ID, "acme/billing", FOLDER, "codex")]), &world).expect("same repo");
    }

    #[test]
    fn save_refuses_an_unknown_or_not_ready_agent() {
        let store = fresh_store();
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/billing"))]);
        let result = save_with(&store, config_with(vec![repo(ID, "acme/billing", FOLDER, "no-such-agent")]), NOW, &world);
        assert_eq!(code_of(result), ERR_AGENT_NOT_READY);
        assert!(store.config().repos.is_empty());
    }

    #[test]
    fn save_refuses_two_rows_with_one_id() {
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/billing")), ("C:/work/other", Some("acme/other"))]);
        let twins = config_with(vec![
            repo(ID, "acme/billing", FOLDER, "codex"),
            repo(ID, "acme/other", "C:/work/other", "codex"),
        ]);
        let refused = check_repos(&twins, &world).expect_err("same id twice");
        assert_eq!(refused.code, "INVALID_ARGUMENT");
    }

    #[test]
    fn settings_diff_names_only_the_repos_that_changed() {
        let before = config_with(vec![repo(ID, "acme/billing", FOLDER, "codex")]);
        let mut after = config_with(vec![
            RepoWatch { turns: 9, ..repo(ID, "acme/billing", FOLDER, "codex") },
            repo("rw-89abcdef", "acme/new", "C:/work/new", "codex"),
        ]);
        after.enabled = false;
        let diff = settings_diff(&before, &after);
        assert_eq!(diff["repos_added"], json!(["acme/new"]));
        assert_eq!(diff["repos_changed"], json!(["acme/billing"]));
        assert_eq!(diff["repos_removed"], json!([]));
        assert_eq!(diff["enabled"], json!(false));
        assert_eq!(diff["was_enabled"], json!(true));
    }

    // `live::act` is not called from a test: it reaches the AppHandle drop glue, the test exe then
    // imports `TaskDialogIndirect` and does not start (docs/REPO_MAP.md). The engine tests cover it.
    #[test]
    fn act_refuses_a_bad_run_id() {
        for bad in ["", "ir-0123456789AB", "ir-../../x", "rw-0123456789ab", "ir-0123"] {
            let refused = check_act_args(bad, "start").expect_err(bad);
            assert_eq!(refused.code, "INVALID_ARGUMENT", "{bad:?}");
        }
    }

    #[test]
    fn act_refuses_a_bad_action() {
        for bad in ["", "merge", "START", "fix_it", "push "] {
            let refused = check_act_args("ir-0123456789ab", bad).expect_err(bad);
            assert_eq!(refused.code, "INVALID_ARGUMENT", "{bad:?}");
        }
    }

    #[test]
    fn act_accepts_the_five_buttons_with_a_good_id() {
        for action in ["start", "ask_again", "push", "throw_away", "unignore"] {
            check_act_args("ir-0123456789ab", action).expect(action);
        }
    }

    #[test]
    fn check_folder_reports_the_origin_and_the_default_branch() {
        let world = FakeWorld::with(vec![(FOLDER, Some("acme/billing"))]);
        let check = check_folder_with(FOLDER, &world);
        assert_eq!(check.owner_repo.as_deref(), Some("acme/billing"));
        assert_eq!(check.default_branch.as_deref(), Some("main"));
        assert_eq!(check.gh, "ready");
        assert!(check.why_not.is_none());
    }

    #[test]
    fn check_folder_says_why_it_cannot_be_used() {
        let mut world = FakeWorld::with(vec![(FOLDER, Some("acme/billing"))]);
        world.gh = "missing";
        let check = check_folder_with(FOLDER, &world);
        assert!(check.default_branch.is_none(), "no branch lookup without gh");
        assert!(check.why_not.expect("a reason").contains("gh"));

        let none = check_folder_with("C:/nowhere", &FakeWorld::with(vec![]));
        assert!(none.owner_repo.is_none());
        assert!(none.why_not.is_some());
    }
}
