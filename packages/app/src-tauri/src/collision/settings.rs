//! Collision Alert settings: `<app data>/collision.json`, `{"enabled": true}` by default.
//!
//! A missing, unreadable or half-written file means the default (on). Unknown fields are ignored,
//! so a newer build's extra keys never break an older one. Only the on/off switch is stored. Alerts
//! live in memory and are never written to disk.

use super::model::Settings;
use std::path::{Path, PathBuf};

const FILE: &str = "collision.json";

/// Where the settings file lives. Tests point it at a temp file with `Store::at`.
#[derive(Debug, Clone)]
pub struct Store {
    file: PathBuf,
}

impl Store {
    /// The settings file at `file` (the whole path, `collision.json` included).
    pub fn at(file: &Path) -> Self {
        Self { file: file.to_path_buf() }
    }

    /// The saved settings, or the default when the file is missing or cannot be read as settings.
    pub fn load(&self) -> Settings {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the settings with private permissions (atomic temp file, then rename).
    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        let text = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
        crate::secure_file::write_private(&self.file, &format!("{text}\n"))
    }
}

/// Serialises the tests that write the one per-process test file that `default_file` names under `cfg(test)`.
#[cfg(test)]
pub(crate) static TEST_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `<app data>/collision.json`.
#[cfg(not(test))]
pub(crate) fn default_file() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE)
}

/// Tests never touch the real app data folder: a per-process temp file instead.
#[cfg(test)]
pub(crate) fn default_file() -> PathBuf {
    std::env::temp_dir().join(format!("pm-collision-unused-{}", std::process::id())).join(FILE)
}

/// The saved switch, or the default (on).
pub fn load() -> Settings {
    Store::at(&default_file()).load()
}

/// Saves the switch to `<app data>/collision.json`.
pub fn save(settings: &Settings) -> Result<(), String> {
    Store::at(&default_file()).save(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A unique folder per test (the process id keeps parallel test runs apart).
    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-collision-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join(FILE)
    }

    #[test]
    fn default_is_on() {
        assert!(Settings::default().enabled);
        let store = Store::at(&temp_file("default"));
        assert_eq!(store.load(), Settings { enabled: true });
    }

    #[test]
    fn round_trip_keeps_the_switch() {
        let file = temp_file("round-trip");
        let store = Store::at(&file);
        store.save(&Settings { enabled: false }).unwrap();
        assert!(!store.load().enabled);
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\n  \"enabled\": false\n}\n");
        store.save(&Settings { enabled: true }).unwrap();
        assert!(store.load().enabled);
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\n  \"enabled\": true\n}\n");
    }

    #[test]
    fn garbage_file_is_the_default() {
        let file = temp_file("garbage");
        let store = Store::at(&file);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        for text in ["", "not json", "[1, 2]", "{\"enabled\": \"no\"}", "{\"enabled\": false", "{}", "null"] {
            fs::write(&file, text).unwrap();
            assert!(store.load().enabled, "garbage {text:?} must load as the default (on)");
        }
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let file = temp_file("unknown");
        let store = Store::at(&file);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{\"enabled\": false, \"future_flag\": {\"deep\": [1, 2]}, \"other\": \"x\"}").unwrap();
        assert!(!store.load().enabled);
        fs::write(&file, "{\"future\": 1, \"enabled\": true}").unwrap();
        assert!(store.load().enabled);
    }

    #[test]
    fn save_reports_a_failure_when_the_folder_is_a_file() {
        let blocker = temp_file("blocked");
        fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        fs::write(&blocker, "not a folder").unwrap();
        let store = Store::at(&blocker.join(FILE));
        assert!(store.save(&Settings { enabled: false }).is_err());
        assert!(store.load().enabled);
    }

    #[test]
    fn load_and_save_use_the_default_file_in_tests() {
        let _guard = TEST_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        save(&Settings { enabled: false }).unwrap();
        assert!(!load().enabled);
        save(&Settings { enabled: true }).unwrap();
        assert!(load().enabled);
    }
}
