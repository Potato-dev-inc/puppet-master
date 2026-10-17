//! `<app data>/agent_run_mode.json`: the run-mode choice per agent id.
//!
//! A missing, unreadable or half-written file, or a value this build does not know, means the
//! default: a visible pane. Only agents that were changed are listed, so the file stays tiny and a
//! newer build's extra values are simply ignored by an older one.

use super::RunMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const FILE: &str = "agent_run_mode.json";
const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct StoreFile {
    version: u32,
    /// Agent id (lower case) -> `visible` | `one_task`. Values are kept as text so one unknown
    /// value never discards the others.
    agents: BTreeMap<String, String>,
}

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::app_paths::app_data_dir().join(FILE)
}

fn key(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

fn parse(text: &str) -> StoreFile {
    serde_json::from_str(text).unwrap_or_default()
}

#[cfg(not(test))]
static CACHE: once_cell::sync::Lazy<parking_lot::RwLock<Option<StoreFile>>> =
    once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(None));

#[cfg(test)]
thread_local! {
    /// A test sets the choices (and what is verified) for its own thread only.
    static TEST_ENV: std::cell::RefCell<Option<TestEnv>> = const { std::cell::RefCell::new(None) };
}

/// What a test fakes: the saved choices and, per agent id, whether its headless mode is verified.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct TestEnv {
    pub modes: BTreeMap<String, RunMode>,
    pub verified: BTreeMap<String, bool>,
}

#[cfg(test)]
pub fn with_test_env<T>(env: TestEnv, body: impl FnOnce() -> T) -> T {
    let before = TEST_ENV.with(|slot| slot.replace(Some(env)));
    let out = body();
    TEST_ENV.with(|slot| *slot.borrow_mut() = before);
    out
}

/// The faked verification of `id` when a test environment is active.
#[cfg(test)]
pub(super) fn test_verified(id: &str) -> Option<bool> {
    TEST_ENV.with(|slot| slot.borrow().as_ref().map(|env| env.verified.get(&key(id)).copied().unwrap_or(false)))
}

pub(super) fn read_file(path: &Path) -> StoreFile {
    std::fs::read_to_string(path).map(|text| parse(&text)).unwrap_or_default()
}

#[cfg(not(test))]
fn loaded() -> StoreFile {
    if let Some(file) = CACHE.read().as_ref() {
        return file.clone();
    }
    let file = read_file(&path());
    *CACHE.write() = Some(file.clone());
    file
}

pub(super) fn mode_in(file: &StoreFile, id: &str) -> RunMode {
    file.agents.get(&key(id)).and_then(|value| RunMode::parse(value)).unwrap_or_default()
}

/// The choice for `id`; a visible pane unless the person changed it.
pub fn setting_for(id: &str) -> RunMode {
    #[cfg(test)]
    {
        // Tests never read the real app data folder: no fake environment means the default.
        TEST_ENV
            .with(|slot| slot.borrow().as_ref().map(|env| env.modes.get(&key(id)).copied()))
            .flatten()
            .unwrap_or_default()
    }
    #[cfg(not(test))]
    {
        mode_in(&loaded(), id)
    }
}

/// Writes one agent's choice into the file at `target`. Choosing the default removes the entry.
pub(super) fn save_at(target: &Path, id: &str, mode: RunMode) -> Result<StoreFile, String> {
    let id = key(id);
    if id.is_empty() {
        return Err("INVALID_ARGUMENT: agent id is required".into());
    }
    let mut file = read_file(target);
    file.version = FILE_VERSION;
    if mode == RunMode::Visible {
        file.agents.remove(&id);
    } else {
        file.agents.insert(id, mode.as_str().to_string());
    }
    let text = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    crate::settings_patch::write_atomic(target, &text).map_err(|e| format!("could not save: {e}"))?;
    Ok(file)
}

/// Saves the choice for one agent.
pub fn set_for_agent(id: &str, mode: RunMode) -> Result<(), String> {
    #[cfg(test)]
    {
        let id = key(id);
        TEST_ENV.with(|slot| {
            if let Some(env) = slot.borrow_mut().as_mut() {
                env.modes.insert(id, mode);
            }
        });
        Ok(())
    }
    #[cfg(not(test))]
    {
        let file = save_at(&path(), id, mode)?;
        *CACHE.write() = Some(file);
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn file_name() -> &'static str {
    FILE
}

#[cfg(test)]
pub(super) fn parse_text(text: &str) -> StoreFile {
    parse(text)
}

