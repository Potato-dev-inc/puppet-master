//! `<app data>/chat_import.json`: what the person chose for "Bring all my chats". Written only by the desktop commands
//! (never the phone-patchable settings mirror), through `secure_file`, so other users of the PC cannot read it.
//!
//! A missing, unreadable or half-written file means the defaults. A field this build does not know is ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name inside the profile's app data folder.
pub const FILE_NAME: &str = "chat_import.json";
/// How far back chats are read unless the person chose otherwise (`None` in the file = all).
pub const DEFAULT_SINCE_DAYS: u32 = 365;

fn default_since_days() -> Option<u32> {
    Some(DEFAULT_SINCE_DAYS)
}

/// The saved choices. `agents` holds only the agents the person switched; the others follow `enabled_for`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportPrefs {
    /// The person answered the first-start card (yes or "Not now"). Nothing is read before they press "Find my chats".
    pub asked: bool,
    /// Only chats updated in the last N days; `None` = all.
    #[serde(default = "default_since_days")]
    pub since_days: Option<u32>,
    /// Agent id -> switched on or off.
    pub agents: BTreeMap<String, bool>,
    /// When the last import ended, in ms since the epoch (0 = never).
    pub last_run_ms: i64,
}

impl Default for ImportPrefs {
    fn default() -> Self {
        ImportPrefs { asked: false, since_days: default_since_days(), agents: BTreeMap::new(), last_run_ms: 0 }
    }
}

impl ImportPrefs {
    /// Whether `agent` is on: the person's switch when there is one; else built-in agents on, agents from a TOML
    /// definition off (the person turns them on after seeing the folder they read).
    pub fn enabled_for(&self, agent: &str, from_definition: bool) -> bool {
        self.agents.get(agent).copied().unwrap_or(!from_definition)
    }

    /// Switches one agent on or off.
    pub fn set_agent(&mut self, agent: &str, on: bool) {
        self.agents.insert(agent.to_string(), on);
    }

    /// The file of this profile.
    pub fn default_path() -> PathBuf {
        crate::app_paths::app_data_dir().join(FILE_NAME)
    }

    /// Reads the file at `path`; the defaults when it is missing or damaged.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Writes the file at `path` (atomic, private).
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|error| format!("INTERNAL: chat import prefs: {error}"))?;
        crate::secure_file::write_private(path, &text).map_err(|_| "INTERNAL: chat import prefs: could not save".to_string())
    }
}
