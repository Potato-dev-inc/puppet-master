//! Load and save of the one Package check setting (SPEC-n33 section 3.8). The `Settings` type is in
//! `model.rs`. The file is `settings.json` under the root given by the caller; the desktop passes
//! `live_root()`. A missing or bad file gives the default (lookups on).

use std::fs;
use std::path::{Path, PathBuf};

use super::model::Settings;
use crate::secure_file::write_private;

const FILE: &str = "settings.json";

fn file_in(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// The setting under `root`. A missing file, unreadable file or bad JSON gives the default (on).
pub fn load(root: &Path) -> Settings {
    fs::read_to_string(file_in(root))
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default()
}

/// Write the setting under `root` (private file; the folder is created if missing).
pub fn save(root: &Path, s: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|err| format!("encode settings: {err}"))?;
    write_private(&file_in(root), &text)
}

/// Where the desktop keeps Package check's files: `<app data>/package-check`.
pub fn live_root() -> PathBuf {
    crate::app_paths::app_data_dir().join("package-check")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pm-pc8-{name}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn missing_file_means_lookups_on() {
        let root = temp_root("missing");
        assert!(load(&root).lookups);
        assert_eq!(load(&root), Settings::default());
    }

    #[test]
    fn saved_setting_round_trips_both_ways() {
        let root = temp_root("round-trip");
        save(&root, &Settings { lookups: false }).unwrap();
        assert!(!load(&root).lookups);
        save(&root, &Settings { lookups: true }).unwrap();
        assert!(load(&root).lookups);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn save_creates_the_folder_and_writes_a_private_json_file() {
        let root = temp_root("nested").join("package-check");
        save(&root, &Settings { lookups: false }).unwrap();
        let text = fs::read_to_string(root.join(FILE)).unwrap();
        assert!(text.contains("\"lookups\": false"));
        assert!(!root.join("settings.json.tmp").exists());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn bad_json_gives_the_default() {
        let root = temp_root("bad");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(FILE), "{ not json").unwrap();
        assert!(load(&root).lookups);
        fs::write(root.join(FILE), "[1, 2]").unwrap();
        assert!(load(&root).lookups);
        fs::write(root.join(FILE), "{\"lookups\": \"no\"}").unwrap();
        assert!(load(&root).lookups);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_object_means_the_default() {
        let root = temp_root("empty");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(FILE), "{}").unwrap();
        assert!(load(&root).lookups);
        let _ = fs::remove_dir_all(root);
    }
}
