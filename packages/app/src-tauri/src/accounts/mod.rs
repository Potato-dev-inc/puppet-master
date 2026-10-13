//! User accounts per agent: the same agent CLI run under different logins, one chosen per pane.
//!
//! An account is `{ id, agent_id, label, home }`. `home` is a directory the user controls (by
//! default one folder per account under `<app data>/accounts/`). Puppet Master only points the
//! agent CLI at it through the env var the agent's profile declares (`TuiProfile::account_scheme`);
//! it never reads, copies or pools the credentials inside. Generic code here never matches agent
//! names: whether an agent supports accounts is the profile's answer.

use crate::tui::{AccountScheme, LaunchEnv};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const STORE_VERSION: u32 = 1;
const MAX_LABEL: usize = 40;

/// Serialises read-modify-write of the store file across commands and bridge routes.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub agent_id: String,
    pub label: String,
    /// Directory the agent is pointed at (its config and login live here).
    pub home: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: Vec<Account>,
}

/// Where the accounts live; tests point it at a temp dir.
#[derive(Debug, Clone)]
pub struct Store {
    file: PathBuf,
    homes_root: PathBuf,
}

impl Store {
    pub fn at(root: &Path) -> Self {
        Self { file: root.join("accounts.json"), homes_root: root.join("accounts") }
    }

    /// `<app data>/accounts.json`, homes under `<app data>/accounts/<id>`.
    pub fn default_location() -> Self {
        Self::at(&crate::app_paths::app_data_dir())
    }

    fn read(&self) -> StoreFile {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write(&self, file: &StoreFile) -> Result<(), String> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(&StoreFile { version: STORE_VERSION, accounts: file.accounts.clone() })
            .map_err(|e| e.to_string())?;
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.file).map_err(|e| format!("could not save {}: {e}", self.file.display()))
    }

    pub fn list(&self) -> Vec<Account> {
        self.read().accounts
    }

    pub fn get(&self, id: &str) -> Option<Account> {
        self.list().into_iter().find(|a| a.id == id)
    }

    /// Add an account for an agent whose profile supports accounts. `home` defaults to a new
    /// folder under the app data directory; a custom `home` must be an absolute path.
    pub fn add(&self, agent_id: &str, label: &str, home: Option<&str>) -> Result<Account, String> {
        let agent_id = agent_id.trim().to_ascii_lowercase();
        scheme_for(&agent_id)?;
        let label = clean_label(label)?;
        let _guard = WRITE_LOCK.lock();
        let mut file = self.read();
        if file.accounts.iter().any(|a| a.agent_id == agent_id && a.label.eq_ignore_ascii_case(&label)) {
            return Err(format!("{agent_id} already has an account named \"{label}\""));
        }
        let id = format!("acc_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let home = match home.map(str::trim).filter(|h| !h.is_empty()) {
            Some(custom) => {
                if !Path::new(custom).is_absolute() {
                    return Err("the account folder must be an absolute path".into());
                }
                custom.to_string()
            }
            None => self.homes_root.join(&id).to_string_lossy().into_owned(),
        };
        std::fs::create_dir_all(&home).map_err(|e| format!("could not create {home}: {e}"))?;
        let account = Account { id, agent_id, label, home };
        file.accounts.push(account.clone());
        self.write(&file)?;
        Ok(account)
    }

    pub fn rename(&self, id: &str, label: &str) -> Result<Account, String> {
        let label = clean_label(label)?;
        let _guard = WRITE_LOCK.lock();
        let mut file = self.read();
        let agent_id = file
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.agent_id.clone())
            .ok_or_else(|| unknown(id))?;
        if file
            .accounts
            .iter()
            .any(|a| a.id != id && a.agent_id == agent_id && a.label.eq_ignore_ascii_case(&label))
        {
            return Err(format!("{agent_id} already has an account named \"{label}\""));
        }
        let account = file.accounts.iter_mut().find(|a| a.id == id).ok_or_else(|| unknown(id))?;
        account.label = label;
        let renamed = account.clone();
        self.write(&file)?;
        Ok(renamed)
    }

    /// Forget an account. The home folder is left alone unless `delete_home` is set, and even then
    /// only a folder this store created (inside its own `accounts/` folder) is ever deleted.
    /// Returns whether a folder was deleted.
    pub fn remove(&self, id: &str, delete_home: bool) -> Result<bool, String> {
        let _guard = WRITE_LOCK.lock();
        let mut file = self.read();
        let index = file.accounts.iter().position(|a| a.id == id).ok_or_else(|| unknown(id))?;
        let account = file.accounts.remove(index);
        self.write(&file)?;
        if !delete_home {
            return Ok(false);
        }
        let home = Path::new(&account.home);
        if home.starts_with(&self.homes_root) && home != self.homes_root {
            std::fs::remove_dir_all(home).map_err(|e| format!("removed the account but not its folder: {e}"))?;
            return Ok(true);
        }
        Ok(false)
    }

    /// The account `id`, which must belong to `agent_id`.
    pub fn resolve(&self, agent_id: &str, id: &str) -> Result<Account, String> {
        let account = self.get(id).ok_or_else(|| unknown(id))?;
        if !account.agent_id.eq_ignore_ascii_case(agent_id) {
            return Err(format!("account {id} belongs to {}, not {agent_id}", account.agent_id));
        }
        Ok(account)
    }

    /// The environment that points `agent_id` at account `id`.
    pub fn launch_env(&self, agent_id: &str, id: &str) -> Result<LaunchEnv, String> {
        let scheme = scheme_for(agent_id)?;
        let account = self.resolve(agent_id, id)?;
        Ok(env_for(&scheme, &account))
    }
}

/// `home_env=<home>` plus the scheme's extra variables.
pub fn env_for(scheme: &AccountScheme, account: &Account) -> LaunchEnv {
    let mut set = vec![(scheme.home_env.clone(), account.home.clone())];
    set.extend(scheme.extra_env.iter().cloned());
    LaunchEnv { set, remove: Vec::new() }
}

fn scheme_for(agent_id: &str) -> Result<AccountScheme, String> {
    if !crate::tui::is_registered(agent_id) {
        return Err(format!("unknown agent type: {agent_id}"));
    }
    crate::tui::profile_for(agent_id)
        .account_scheme()
        .ok_or_else(|| format!("{agent_id} does not support accounts"))
}

fn unknown(id: &str) -> String {
    format!("unknown account: {id}")
}

fn clean_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("an account needs a name".into());
    }
    if label.chars().count() > MAX_LABEL {
        return Err(format!("an account name is limited to {MAX_LABEL} characters"));
    }
    Ok(label.to_string())
}

/// What the Settings screen needs about one agent that supports accounts.
#[derive(Debug, Clone, Serialize)]
pub struct AccountAgent {
    pub agent_id: String,
    pub login_hint: String,
    pub home_env: String,
}

/// Registered agents whose profile declares an account scheme.
pub fn supporting_agents() -> Vec<AccountAgent> {
    crate::tui::registry::snapshot()
        .iter()
        .filter_map(|profile| {
            profile.account_scheme().map(|scheme| AccountAgent {
                agent_id: profile.id().to_string(),
                login_hint: scheme.login_hint,
                home_env: scheme.home_env,
            })
        })
        .collect()
}

/// The env a pane or run of `agent_id` under account `id` gets (default store).
pub fn launch_env(agent_id: &str, id: &str) -> Result<LaunchEnv, String> {
    Store::default_location().launch_env(agent_id, id)
}

/// The env and the label for a pane or run of `agent_id` under account `id` (default store).
pub fn launch_for(agent_id: &str, id: &str) -> Result<(LaunchEnv, String), String> {
    let store = Store::default_location();
    let env = store.launch_env(agent_id, id)?;
    let label = store.get(id).map(|a| a.label).unwrap_or_default();
    Ok((env, label))
}

/// The account's label for display, when it still exists.
pub fn label_of(id: &str) -> Option<String> {
    Store::default_location().get(id).map(|a| a.label)
}

/// Validate a run's `account` argument before anything spawns: the agent supports accounts and
/// the account exists for it. Returns the account id to store on the run.
pub fn validate_for_run(agent_id: &str, id: &str) -> Result<String, String> {
    let id = id.trim();
    Store::default_location().launch_env(agent_id, id).map(|_| id.to_string())
}

/// Everything the Settings screen shows: agents that support accounts, and the accounts.
pub fn overview(store: &Store) -> serde_json::Value {
    serde_json::json!({ "agents": supporting_agents(), "accounts": store.list() })
}

/// Shared by the Tauri commands and the bridge routes.
pub fn overview_default() -> serde_json::Value {
    overview(&Store::default_location())
}

#[cfg(test)]
mod tests;
