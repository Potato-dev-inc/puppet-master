//! The standby pool's settings and the small persisted history it warms from.
//!
//! Off by default. Stored in `standby-pool.json` in the app data dir (not in the UI's settings
//! store), so the bridge honours it even while no window is open.
//!
//! Two layers share the file: **Preload** (`preload`, what the Settings window shows: a master switch
//! and a number of warm visible workers per provider, always in the default project folder) and the
//! legacy **headless** toggles (`backends[..].headless`, hand-edited only). Visible standbys come from
//! Preload alone; old `backends[..].visible` toggles are migrated into it on load.

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

/// Most warm workers one provider may keep.
pub const MAX_WARM_PER_PROVIDER: u8 = 2;
/// Most warm workers across every provider.
pub const MAX_PRELOAD_TOTAL: usize = 8;

/// Settings > Preload: keep `providers[agent]` (0-2) warm visible workers for each provider, in the
/// default project folder, so opening a pane for that provider is instant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreloadConfig {
    /// Master switch. Off: no provider is warmed whatever its count.
    pub enabled: bool,
    /// Warm workers wanted per `agent_type`. Missing or 0: that provider is not warmed.
    pub providers: BTreeMap<String, u8>,
}

impl PreloadConfig {
    pub fn count_for(&self, agent_type: &str) -> usize {
        if !self.enabled {
            return 0;
        }
        self.providers
            .get(&agent_type.to_ascii_lowercase())
            .copied()
            .unwrap_or(0) as usize
    }

    /// Workers wanted in total (what the pool must have room for).
    pub fn total_target(&self) -> usize {
        if !self.enabled {
            return 0;
        }
        self.providers.values().map(|n| *n as usize).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StandbyConfig {
    /// Preload (visible warm workers). Shown in Settings.
    pub preload: PreloadConfig,
    /// Legacy master switch for the headless toggles in `backends`.
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
            preload: PreloadConfig::default(),
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
        // Visible standbys belong to Preload; only its supported providers survive, counts 0-2, total capped.
        let mut budget = MAX_PRELOAD_TOTAL;
        let providers = std::mem::take(&mut self.preload.providers);
        for (agent, count) in providers {
            let agent = agent.to_ascii_lowercase();
            let count = count.min(MAX_WARM_PER_PROVIDER).min(budget as u8);
            if count == 0 || !crate::tui::profile_for(&agent).standby_support().visible {
                continue;
            }
            budget -= count as usize;
            self.preload.providers.insert(agent, count);
        }
        for toggle in self.backends.values_mut() {
            toggle.visible = false;
        }
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

    /// Pool-wide cap on live standbys: the legacy cap, widened to hold every wanted preload worker.
    pub fn cap(&self) -> usize {
        self.max_total.max(self.preload.total_target()).min(MAX_PRELOAD_TOTAL)
    }

    /// Whether this config allows a standby for `key`.
    pub fn allows(&self, key: &PoolKey) -> bool {
        if !key.headless {
            return self.preload.count_for(&key.agent_type) > 0
                && crate::tui::profile_for(&key.agent_type).standby_support().visible;
        }
        if !self.enabled || self.max_total == 0 {
            return false;
        }
        let Some(toggle) = self.backends.get(&key.agent_type) else {
            return false;
        };
        let support = crate::tui::profile_for(&key.agent_type).standby_support();
        toggle.headless && support.headless
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
    let parsed = serde_json::from_str::<serde_json::Value>(&text).and_then(|value| {
        let has_preload = value.pointer("/config/preload").is_some();
        serde_json::from_value::<StoreFile>(value).map(|file| (file, has_preload))
    });
    match parsed {
        Ok((mut file, has_preload)) => {
            if !has_preload {
                migrate_legacy_visible(&mut file.config);
            }
            (file.config.clamped(), file.recent)
        }
        Err(error) => {
            tracing::warn!(%error, "standby pool config unreadable; using defaults (pool off)");
            (StandbyConfig::default(), Vec::new())
        }
    }
}

/// A file written before Preload: its visible toggles become one warm worker per provider, and the
/// old master switch becomes the Preload master switch. Runs once (the next save writes `preload`).
fn migrate_legacy_visible(config: &mut StandbyConfig) {
    for (agent, toggle) in &config.backends {
        if toggle.visible {
            config.preload.providers.insert(agent.to_ascii_lowercase(), 1);
        }
    }
    config.preload.enabled = config.enabled && !config.preload.providers.is_empty();
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
        assert!(!config.enabled && config.backends.is_empty() && !config.preload.enabled);
        assert!(config.preload.providers.is_empty());
        let key = PoolKey::new("cursor_agent", "/p", None, false, false, false);
        assert!(!config.allows(&key));
    }

    fn preload(agents: &[(&str, u8)]) -> StandbyConfig {
        let mut config = StandbyConfig::default();
        config.preload.enabled = true;
        for (agent, n) in agents {
            config.preload.providers.insert((*agent).into(), *n);
        }
        config.clamped()
    }

    #[test]
    fn visible_standbys_need_the_preload_master_and_a_provider_count() {
        let config = preload(&[("cursor_agent", 1)]);
        let visible = |a: &str| PoolKey::new(a, "/p", None, false, false, false);
        assert!(config.allows(&visible("cursor_agent")));
        assert!(!config.allows(&visible("claude")), "provider not ticked");
        assert!(
            !config.allows(&PoolKey::new("cursor_agent", "/p", None, false, false, true)),
            "headless is a separate legacy toggle"
        );
        let mut off = config.clone();
        off.preload.enabled = false;
        assert!(!off.allows(&visible("cursor_agent")));
        assert_eq!(off.preload.count_for("cursor_agent"), 0);
    }

    #[test]
    fn headless_keeps_its_legacy_toggle() {
        let config = enabled("claude", false, true).clamped();
        assert!(config.allows(&PoolKey::new("claude", "/p", None, false, false, true)));
        assert!(!config.allows(&PoolKey::new("claude", "/p", None, false, false, false)));
    }

    #[test]
    fn clamping_bounds_counts_and_drops_unsupported_providers() {
        let config = preload(&[("claude", 9), ("nonsense", 1), ("opencode_native", 1), ("cursor_agent", 0)]);
        assert_eq!(config.preload.providers.get("claude"), Some(&MAX_WARM_PER_PROVIDER));
        assert!(!config.preload.providers.contains_key("nonsense"));
        assert!(!config.preload.providers.contains_key("opencode_native"), "no standby support");
        assert!(!config.preload.providers.contains_key("cursor_agent"), "0 means not warmed");
        let mut config = enabled("cursor_agent", true, true);
        config.backends.insert("nonsense".into(), BackendToggle { visible: true, headless: true });
        config.max_total = 99;
        config.recent_projects = 0;
        config.idle_timeout_secs = 0;
        let config = config.clamped();
        assert_eq!(config.max_total, 8);
        assert_eq!(config.recent_projects, 1);
        assert_eq!(config.idle_timeout_secs, 5);
        assert!(!config.backends.contains_key("nonsense"));
        assert!(!config.backends["cursor_agent"].headless, "headless cursor cannot be pre-launched");
        assert!(!config.backends["cursor_agent"].visible, "visible lives in preload");
    }

    #[test]
    fn cap_widens_to_hold_every_preload_worker() {
        let config = preload(&[("claude", 2), ("cursor_agent", 2)]);
        assert_eq!(config.max_total, 3);
        assert_eq!(config.cap(), 4);
        assert_eq!(StandbyConfig::default().cap(), 3);
    }

    #[test]
    fn an_old_file_migrates_its_visible_toggles_into_preload() {
        let dir = std::env::temp_dir().join(format!("pm-standby-migrate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("standby-pool.json");
        let old = serde_json::json!({"config": {"enabled": true, "backends": {
            "claude": {"visible": true, "headless": true},
            "cursor_agent": {"visible": false, "headless": false}}}, "recent": []});
        std::fs::write(&path, old.to_string()).unwrap();
        let (config, _) = load(&path);
        assert!(config.preload.enabled);
        assert_eq!(config.preload.providers.get("claude"), Some(&1));
        assert!(!config.preload.providers.contains_key("cursor_agent"));
        assert!(config.enabled && config.backends["claude"].headless, "headless toggle kept");
        assert!(!config.backends["claude"].visible);
        // A master-off old file keeps the ticks but stays off.
        let off = serde_json::json!({"config": {"enabled": false, "backends": {"claude": {"visible": true}}}});
        std::fs::write(&path, off.to_string()).unwrap();
        let (config, _) = load(&path);
        assert!(!config.preload.enabled);
        assert_eq!(config.preload.providers.get("claude"), Some(&1));
        // A file that already has `preload` is never migrated again.
        let fresh = serde_json::json!({"config": {"enabled": true, "preload": {"enabled": false},
            "backends": {"claude": {"visible": true}}}});
        std::fs::write(&path, fresh.to_string()).unwrap();
        let (config, _) = load(&path);
        assert!(!config.preload.enabled && config.preload.providers.is_empty());
    }

    #[test]
    fn save_and_load_round_trip_and_a_bad_file_means_off() {
        let dir = std::env::temp_dir().join(format!("pm-standby-config-{}", uuid::Uuid::new_v4()));
        let path = dir.join("standby-pool.json");
        let mut config = enabled("claude", false, true);
        config.preload = PreloadConfig { enabled: true, providers: [("claude".to_string(), 2u8)].into() };
        let config = config.clamped();
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
