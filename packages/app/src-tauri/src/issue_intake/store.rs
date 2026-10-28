//! The private store under `<app data>/issue-intake/` (SPEC n32 section 3.2).
//!
//! - `config.json`: the Labs settings, the watched repos, the allowed logins, the ignored issues.
//! - `runs.json`: issue runs, newest first, at most `RECORDS_CAP`.
//! - `empty-hooks/`: an empty folder for `core.hooksPath` on PM's own git calls (3.5).
//! - `gh-empty/`: an empty `GH_CONFIG_DIR` for panes that must not see the person's gh login (3.4).
//! - `push-blocked-hooks/`: `core.hooksPath` for those panes; its one `pre-push` hook refuses every push.
//!
//! Every write goes through `secure_file::write_private` (atomic, owner-only on unix). A bad id is
//! refused before any file is touched, so a hostile id never makes a file (S15). A missing or
//! unreadable file reads as empty; nothing panics.

use super::model::{
    valid_label, valid_login, valid_owner_repo, valid_repo_id, valid_run_id, Config, IssueRun, RepoWatch, RunState,
    ALLOWED_CAP, MINUTES_MAX, MINUTES_MIN, PARALLEL_MAX, RECORDS_CAP, REPOS_CAP, RUNS_PER_DAY_MAX, TURNS_MAX,
};
use crate::secure_file::{create_private_dir, write_private};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One writer at a time in this process (read-modify-write of the JSON files).
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// The app's own store: `<app data>/issue-intake`.
    #[cfg(not(test))]
    pub fn live() -> Store {
        Store::at(crate::app_paths::app_data_dir().join("issue-intake"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The saved settings. Missing or unreadable file = the defaults (feature off).
    pub fn config(&self) -> Config {
        read_json::<Config>(&self.config_path()).unwrap_or_default()
    }

    /// Validate, tidy and save the settings. Returns what was saved. A refused config writes nothing.
    pub fn save_config(&self, config: &Config) -> Result<Config, String> {
        let clean = clean_config(config)?;
        let _guard = lock();
        write_json(&self.config_path(), &clean)?;
        Ok(clean)
    }

    /// All runs, newest first. Missing or unreadable file = none.
    pub fn runs(&self) -> Vec<IssueRun> {
        read_json::<Vec<IssueRun>>(&self.runs_path()).unwrap_or_default()
    }

    /// One run by its id. A bad id finds nothing.
    pub fn by_id(&self, id: &str) -> Option<IssueRun> {
        if !valid_run_id(id) {
            return None;
        }
        self.runs().into_iter().find(|run| run.id == id)
    }

    /// The run waiting on this card (`Asking` or `AskPush`). Only ids stored in a record match.
    pub fn by_question(&self, question_id: &str) -> Option<IssueRun> {
        if question_id.is_empty() {
            return None;
        }
        self.runs().into_iter().find(|run| match &run.state {
            RunState::Asking { question_id: stored } | RunState::AskPush { question_id: stored, .. } => {
                stored == question_id
            }
            _ => false,
        })
    }

    /// Insert or update one run. An update keeps its place. A new run goes first, and the oldest
    /// ones are dropped past `RECORDS_CAP`.
    pub fn put(&self, run: &IssueRun) -> Result<(), String> {
        if !valid_run_id(&run.id) {
            return Err("bad issue run id".to_string());
        }
        let _guard = lock();
        let mut runs = self.runs();
        match runs.iter().position(|existing| existing.id == run.id) {
            Some(index) => runs[index] = run.clone(),
            None => runs.insert(0, run.clone()),
        }
        runs.truncate(RECORDS_CAP);
        write_json(&self.runs_path(), &runs)
    }

    /// The empty folder for `core.hooksPath`. Created private; PM never puts a file in it.
    pub fn empty_hooks_dir(&self) -> Result<PathBuf, String> {
        self.empty_dir("empty-hooks")
    }

    /// The empty `GH_CONFIG_DIR` for guarded panes. Created private; PM never puts a file in it.
    pub fn gh_empty_dir(&self) -> Result<PathBuf, String> {
        self.empty_dir("gh-empty")
    }

    /// The folder `core.hooksPath` points at for guarded panes: one `pre-push` hook that refuses every
    /// push. Created private; the hook is written again at every call, so an edit does not last.
    pub fn push_block_hooks_dir(&self) -> Result<PathBuf, String> {
        let dir = self.empty_dir("push-blocked-hooks")?;
        crate::pty::env_guard::write_push_block_hook(&dir)?;
        Ok(dir)
    }

    fn empty_dir(&self, name: &str) -> Result<PathBuf, String> {
        let dir = self.root.join(name);
        create_private_dir(&dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
        Ok(dir)
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    fn runs_path(&self) -> PathBuf {
        self.root.join("runs.json")
    }
}

/// A new run id: `ir-` + 12 lowercase hex characters.
pub fn new_run_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("ir-{}", &hex[..12])
}

/// A new repo id: `rw-` + 8 lowercase hex characters.
pub fn new_repo_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("rw-{}", &hex[..8])
}

/// Check and tidy the settings. Logins are trimmed, lowercased and de-duplicated (first wins).
/// A repo with no id gets a new one. Anything out of range or malformed is an error with a plain
/// English reason, so nothing is silently changed except the tidying above.
fn clean_config(config: &Config) -> Result<Config, String> {
    if !(1..=PARALLEL_MAX).contains(&config.parallel) {
        return Err(format!("Runs at once must be 1 to {PARALLEL_MAX}."));
    }
    let mut allowed: Vec<String> = Vec::new();
    for login in &config.allowed {
        let login = login.trim().to_lowercase();
        if !valid_login(&login) {
            return Err(format!("{login:?} is not a GitHub name."));
        }
        if !allowed.contains(&login) {
            allowed.push(login);
        }
    }
    if allowed.len() > ALLOWED_CAP {
        return Err(format!("You can add at most {ALLOWED_CAP} GitHub names."));
    }
    if config.repos.len() > REPOS_CAP {
        return Err(format!("You can watch at most {REPOS_CAP} repositories."));
    }
    let mut repos: Vec<RepoWatch> = Vec::new();
    for repo in &config.repos {
        let clean = clean_repo(repo)?;
        if repos.iter().any(|kept| kept.owner_repo == clean.owner_repo) {
            return Err(format!("{} is in the list twice.", clean.owner_repo));
        }
        repos.push(clean);
    }
    let mut ignored: Vec<(String, u64)> = Vec::new();
    for (owner_repo, number) in &config.ignored {
        if !valid_owner_repo(owner_repo) || *number == 0 {
            return Err("An ignored issue must be a repository and an issue number.".to_string());
        }
        if !ignored.iter().any(|(repo, num)| repo == owner_repo && num == number) {
            ignored.push((owner_repo.clone(), *number));
        }
    }
    Ok(Config {
        enabled: config.enabled,
        allowed,
        write_access_counts: config.write_access_counts,
        parallel: config.parallel,
        repos,
        ignored,
    })
}

fn clean_repo(repo: &RepoWatch) -> Result<RepoWatch, String> {
    if !valid_owner_repo(&repo.owner_repo) {
        return Err(format!("{:?} is not a repository name like owner/name.", repo.owner_repo));
    }
    let id = if repo.id.is_empty() { new_repo_id() } else { repo.id.clone() };
    if !valid_repo_id(&id) {
        return Err("bad repository id".to_string());
    }
    let label = repo.label.trim().to_string();
    if !valid_label(&label) {
        return Err(format!("The label for {} is not valid.", repo.owner_repo));
    }
    if repo.folder.trim().is_empty() {
        return Err(format!("Choose the folder for {}.", repo.owner_repo));
    }
    if !(1..=TURNS_MAX).contains(&repo.turns) {
        return Err(format!("Turns must be 1 to {TURNS_MAX}."));
    }
    if !(MINUTES_MIN..=MINUTES_MAX).contains(&repo.minutes) {
        return Err(format!("Minutes must be {MINUTES_MIN} to {MINUTES_MAX}."));
    }
    if !(1..=RUNS_PER_DAY_MAX).contains(&repo.runs_per_day) {
        return Err(format!("Runs a day must be 1 to {RUNS_PER_DAY_MAX}."));
    }
    Ok(RepoWatch { id, label, folder: repo.folder.trim().to_string(), ..repo.clone() })
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    write_private(path, &text)
}
