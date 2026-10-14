//! The two settings, kept in their own file.
//!
//! They are deliberately NOT in `settings.json` or the public-settings mirror: that mirror can be
//! patched over the bridge by a paired phone or a local program (`settings_store::PATCHABLE_KEYS`),
//! and these are the switches that decide what an agent may do without asking. Only the Settings
//! window, through a Tauri command, writes this file.

use super::{DEFAULT_AGENTS_MAY_MERGE, DEFAULT_ASK_BEFORE_AGENT_CONTROL};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FILE: &str = "agent_authority.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// A: ask a person before an agent controls a pane it did not start.
    #[serde(default = "default_ask")]
    pub ask_before_agent_control: bool,
    /// B: an agent may merge a separate copy with `review_worktree`.
    #[serde(default = "default_merge")]
    pub agents_may_merge: bool,
}

fn default_ask() -> bool {
    DEFAULT_ASK_BEFORE_AGENT_CONTROL
}
fn default_merge() -> bool {
    DEFAULT_AGENTS_MAY_MERGE
}

impl Default for Config {
    fn default() -> Self {
        Self { ask_before_agent_control: default_ask(), agents_may_merge: default_merge() }
    }
}

fn path() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE)
}

/// What a missing, unreadable or half-written file means: the defaults.
pub fn parse(text: &str) -> Config {
    serde_json::from_str(text).unwrap_or_default()
}

static CACHE: Lazy<RwLock<Option<Config>>> = Lazy::new(|| RwLock::new(None));

#[cfg(test)]
thread_local! {
    /// A test sets the config for its own thread only, so tests never see each other's switches.
    static TEST_CONFIG: std::cell::Cell<Option<Config>> = const { std::cell::Cell::new(None) };
}

/// The settings now in force.
pub fn current() -> Config {
    #[cfg(test)]
    {
        TEST_CONFIG.with(|slot| slot.get()).unwrap_or_default()
    }
    #[cfg(not(test))]
    {
        if let Some(config) = *CACHE.read() {
            return config;
        }
        let loaded = std::fs::read_to_string(path()).map(|text| parse(&text)).unwrap_or_default();
        *CACHE.write() = Some(loaded);
        loaded
    }
}

/// Save new settings. Returns what was in force before, so the caller can log the change.
pub fn set(next: Config) -> Result<Config, String> {
    let before = current();
    #[cfg(test)]
    {
        TEST_CONFIG.with(|slot| slot.set(Some(next)));
        Ok(before)
    }
    #[cfg(not(test))]
    {
        let target = path();
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
        crate::settings_patch::write_atomic(&target, &text).map_err(|e| format!("could not save: {e}"))?;
        *CACHE.write() = Some(next);
        Ok(before)
    }
}

pub fn ask_before_agent_control() -> bool {
    current().ask_before_agent_control
}

pub fn agents_may_merge() -> bool {
    current().agents_may_merge
}

/// Run `body` with these settings in force on this thread (tests only).
#[cfg(test)]
pub fn with_config<T>(config: Config, body: impl FnOnce() -> T) -> T {
    let before = TEST_CONFIG.with(|slot| slot.replace(Some(config)));
    let out = body();
    TEST_CONFIG.with(|slot| slot.set(before));
    out
}
