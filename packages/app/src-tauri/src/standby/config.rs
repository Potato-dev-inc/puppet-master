//! The standby pool's settings and the small persisted history it warms from.
//!
//! Off by default. Stored in `standby-pool.json` in the app data dir (not in the UI's settings
//! store), so the bridge honours it even while no window is open.

use super::key::PoolKey;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which standby kinds one backend may have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackendToggle {
    pub visible: bool,
    pub headless: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StandbyConfig {
    /// Master switch. Off: nothing is spawned, claimed or kept.
    pub enabled: bool,
    /// Per `agent_type`. A backend missing here is off.
    pub backends: BTreeMap<String, BackendToggle>,
    /// Hard cap on standbys alive at once, across every key.
    pub max_total: usize,
    /// Warm only the most recently used N projects.
    pub recent_projects: usize,
    /// ... and only those with a run in the last X hours.
    pub recent_hours: u64,
    /// A standby unused this long is shut down and its key stays cold until the next run.
    pub idle_timeout_secs: u64,
}

impl Default for StandbyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backends: BTreeMap::new(),
            max_total: 3,
            recent_projects: 2,
            recent_hours: 6,
            idle_timeout_secs: 20 * 60,
        }
    }
}

impl StandbyConfig {
    /// Pull every field into its supported range (a hand-edited file or an old UI must not be
    /// able to start 50 CLIs) and drop toggles a backend cannot honour.
    pub fn clamped(mut self) -> Self {
        self.max_total = self.max_total.min(8);
        self.recent_projects = self.recent_projects.clamp(1, 5);
        self.recent_hours = self.recent_hours.clamp(1, 24 * 14);
        self.idle_timeout_secs = self.idle_timeout_secs.clamp(5, 24 * 3600);
        self.backends.retain(|agent, _| {
            let support = crate::tui::profile_for(agent).standby_support();
            support.visible || support.headless
        });
        for (agent, toggle) in &mut self.backends {
            let support = crate::tui::profile_for(agent).standby_support();
            toggle.visible &= support.visible;
            toggle.headless &= support.headless;
        }
        self
    }

    /// Whether this config allows a standby for `key`.
    pub fn allows(&self, key: &PoolKey) -> bool {
        if !self.enabled || self.max_total == 0 {
            return false;
        }
        let Some(toggle) = self.backends.get(&key.agent_type) else {
            return false;
        };
        let support = crate::tui::profile_for(&key.agent_type).standby_support();
        if key.headless {
            toggle.headless && support.headless
        } else {
            toggle.visible && support.visible
        }
    }
}

/// A run seen recently: the key it would have claimed and the path to spawn in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentUse {
    pub key: PoolKey,
    /// The project path as the run named it (what a standby is spawned in).
    pub cwd: String,
    pub at_ms: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoreFile {
    config: StandbyConfig,
    recent: Vec<RecentUse>,
}

pub const MAX_RECENT: usize = 32;

pub fn default_path() -> PathBuf {
    crate::app_paths::app_data_dir().join("standby-pool.json")
}

pub fn load(path: &Path) -> (StandbyConfig, Vec<RecentUse>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (StandbyConfig::default(), Vec::new());
    };
    match serde_json::from_str::<StoreFile>(&text) {
        Ok(file) => (file.config.clamped(), file.recent),
        Err(error) => {
            tracing::warn!(%error, "standby pool config unreadable; using defaults (pool off)");
            (StandbyConfig::default(), Vec::new())
        }
    }
}

pub fn save(path: &Path, config: &StandbyConfig, recent: &[RecentUse]) -> Result<(), String> {
    let file = StoreFile {
        config: config.clone(),
        recent: recent.to_vec(),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|error| error.to_string())?;
    std::fs::rename(&tmp, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(agent: &str, visible: bool, headless: bool) -> StandbyConfig {
        let mut config = StandbyConfig {
            enabled: true,
            ..Default::default()
        };
        config
            .backends
            .insert(agent.into(), BackendToggle { visible, headless });
        config
    }

    #[test]
    fn off_by_default() {
        let config = StandbyConfig::default();
        assert!(!config.enabled && config.backends.is_empty());
        let key = PoolKey::new("cursor_agent", "/p", None, false, false, false);
        assert!(!config.allows(&key));
    }

    #[test]
    fn a_backend_needs_its_own_toggle_per_kind() {
        let config = enabled("cursor_agent", true, false).clamped();
        assert!(config.allows(&PoolKey::new("cursor_agent", "/p", None, false, false, false)));
        assert!(!config.allows(&PoolKey::new("cursor_agent", "/p", None, false, false, true)));
        assert!(!config.allows(&PoolKey::new("claude", "/p", None, false, false, false)));
        let mut off = config.clone();
        off.enabled = false;
        assert!(!off.allows(&PoolKey::new("cursor_agent", "/p", None, false, false, false)));
    }

    #[test]
    fn clamping_drops_unsupported_toggles_and_bounds_numbers() {
        let mut config = enabled("cursor_agent", true, true);
        config.backends.insert(
            "opencode_native".into(),
            BackendToggle {
                visible: true,
                headless: true,
            },
        );
        config.backends.insert(
            "nonsense".into(),
            BackendToggle {
                visible: true,
                headless: true,
            },
        );
        config.max_total = 99;
        config.recent_projects = 0;
        config.idle_timeout_secs = 0;
        let config = config.clamped();
        assert_eq!(config.max_total, 8);
        assert_eq!(config.recent_projects, 1);
        assert_eq!(config.idle_timeout_secs, 5);
        assert!(!config.backends.contains_key("nonsense"));
        assert!(!config.backends.contains_key("opencode_native"), "no standby support");
        assert!(
            !config.backends["cursor_agent"].headless,
            "headless cursor cannot be pre-launched"
        );
    }

    #[test]
    fn save_and_load_round_trip_and_a_bad_file_means_off() {
        let dir = std::env::temp_dir().join(format!("pm-standby-config-{}", uuid::Uuid::new_v4()));
        let path = dir.join("standby-pool.json");
        let config = enabled("claude", true, true).clamped();
        let recent = vec![RecentUse {
            key: PoolKey::new("claude", "/p", None, false, false, false),
            cwd: "/p".into(),
            at_ms: 5,
        }];
        save(&path, &config, &recent).unwrap();
        assert_eq!(load(&path), (config, recent));
        std::fs::write(&path, "{ not json").unwrap();
        let (loaded, history) = load(&path);
        assert!(!loaded.enabled && history.is_empty());
        assert!(!load(&dir.join("missing.json")).0.enabled);
    }
}
