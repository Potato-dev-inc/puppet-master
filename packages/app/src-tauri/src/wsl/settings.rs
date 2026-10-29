//! The per-agent place setting in `<app data>/wsl.json` (spec section 3.2): which agents run in
//! which distro. A missing or broken file means every agent runs on Windows. Reads are tolerant: an
//! invalid distro name is ignored, and unknown fields are ignored. A rewrite keeps the unknown
//! fields (a newer build may have written them). Only Tauri commands call `set`.

use crate::secure_file;
use crate::wsl::model::DistroName;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FILE_NAME: &str = "wsl.json";
pub const VERSION: u64 = 1;

/// Serialises the read-modify-write in `set`, so two quick Settings picks never lose each other.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    /// The store whose file is `dir/wsl.json`.
    pub fn at(dir: &Path) -> Self {
        Self { path: dir.join(FILE_NAME) }
    }

    /// The store in the app data folder of this profile.
    #[cfg(not(test))]
    pub fn live() -> Self {
        Self::at(&crate::app_paths::app_data_dir())
    }

    /// The distro this agent runs in, or None when it runs on Windows.
    pub fn get(&self, agent: &str) -> Option<String> {
        self.all().remove(&agent.to_ascii_lowercase())
    }

    /// Every agent that runs in Linux, with its distro. Agent ids are lower case.
    pub fn all(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let Some(root) = read_root(&self.path) else { return out };
        let Some(agents) = root.get("agents").and_then(Value::as_object) else { return out };
        for (agent, entry) in agents {
            let distro = entry.get("distro").and_then(Value::as_str).and_then(DistroName::parse);
            if let Some(distro) = distro {
                out.insert(agent.to_ascii_lowercase(), distro.as_str().to_string());
            }
        }
        out
    }

    /// Set the distro for `agent` (None = Windows). Writes the whole file atomically.
    pub fn set(&self, agent: &str, distro: Option<&DistroName>) -> Result<(), String> {
        let key = agent.to_ascii_lowercase();
        if key.is_empty() {
            return Err("agent id is empty".to_string());
        }
        let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut root = read_root(&self.path).unwrap_or_default();
        root.insert("version".to_string(), json!(VERSION));
        let mut agents = match root.remove("agents") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        // Merge every spelling of this agent into one entry (keeping its unknown fields), then
        // write the lower-case key back.
        let spellings: Vec<String> = agents.keys().filter(|k| k.to_ascii_lowercase() == key).cloned().collect();
        let mut entry = Map::new();
        for spelling in spellings {
            if let Some(Value::Object(old)) = agents.remove(&spelling) {
                entry.extend(old);
            }
        }
        match distro {
            Some(name) => {
                entry.insert("distro".to_string(), json!(name.as_str()));
                agents.insert(key, Value::Object(entry));
            }
            None => {
                entry.remove("distro");
                if !entry.is_empty() {
                    agents.insert(key, Value::Object(entry));
                }
            }
        }
        root.insert("agents".to_string(), Value::Object(agents));
        let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|err| err.to_string())?;
        secure_file::write_private(&self.path, &format!("{text}\n"))
    }
}

/// The top-level JSON object of the file, or None when the file is missing, unreadable, not JSON,
/// or not an object.
fn read_root(path: &Path) -> Option<Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test (never the app data folder).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-wsl-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn distro(name: &str) -> DistroName {
        DistroName::parse(name).unwrap()
    }

    fn read_json(store: &Store) -> Value {
        serde_json::from_str(&fs::read_to_string(&store.path).unwrap()).unwrap()
    }

    #[test]
    fn missing_file_means_every_agent_on_windows() {
        let dir = scratch("missing");
        let store = Store::at(&dir);
        assert_eq!(store.path, dir.join("wsl.json"));
        assert_eq!(store.get("claude-code"), None);
        assert!(store.all().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_file_means_every_agent_on_windows_and_a_write_replaces_it() {
        let dir = scratch("broken");
        let store = Store::at(&dir);
        for text in ["{ not json", "[1, 2]", "", r#"{"agents": []}"#] {
            fs::write(&store.path, text).unwrap();
            assert!(store.all().is_empty(), "{text:?}");
            assert_eq!(store.get("codex"), None, "{text:?}");
        }
        fs::write(&store.path, "{ not json").unwrap();
        store.set("claude-code", Some(&distro("Ubuntu"))).unwrap();
        assert_eq!(store.get("claude-code").as_deref(), Some("Ubuntu"));
        assert_eq!(read_json(&store)["version"], 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_keys_are_lower_cased() {
        let dir = scratch("case");
        let store = Store::at(&dir);
        store.set("Claude-Code", Some(&distro("Ubuntu"))).unwrap();
        assert_eq!(store.get("claude-code").as_deref(), Some("Ubuntu"));
        assert_eq!(store.get("CLAUDE-CODE").as_deref(), Some("Ubuntu"));
        let text = fs::read_to_string(&store.path).unwrap();
        assert!(text.contains("\"claude-code\"") && !text.contains("Claude-Code"));
        // A hand-written mixed-case key is read the same way.
        fs::write(&store.path, r#"{"version":1,"agents":{"Codex":{"distro":"Debian"}}}"#).unwrap();
        assert_eq!(store.get("codex").as_deref(), Some("Debian"));
        assert_eq!(store.all().keys().collect::<Vec<_>>(), vec!["codex"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_fields_are_ignored_and_kept_on_write() {
        let dir = scratch("unknown");
        let store = Store::at(&dir);
        fs::write(
            &store.path,
            r#"{"version":1,"theme":"dark","agents":{"codex":{"distro":"Ubuntu","note":"kept"}}}"#,
        )
        .unwrap();
        assert_eq!(store.get("codex").as_deref(), Some("Ubuntu"));
        store.set("opencode", Some(&distro("Debian"))).unwrap();
        let root = read_json(&store);
        assert_eq!(root["theme"], "dark");
        assert_eq!(root["agents"]["codex"]["note"], "kept");
        assert_eq!(store.get("opencode").as_deref(), Some("Debian"));
        // Back to Windows drops only the distro; the agent's other fields stay.
        store.set("codex", None).unwrap();
        assert_eq!(store.get("codex"), None);
        let root = read_json(&store);
        assert_eq!(root["agents"]["codex"]["note"], "kept");
        assert!(root["agents"]["codex"].get("distro").is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_distro_text_is_ignored_on_read() {
        let dir = scratch("invalid");
        let store = Store::at(&dir);
        fs::write(
            &store.path,
            r#"{"version":1,"agents":{
                "claude-code":{"distro":"a b"},
                "codex":{"distro":"-x"},
                "opencode":{"distro":"../evil"},
                "cursor":{"distro":5},
                "gemini":{"distro":null},
                "pi":{"distro":"Ubuntu"}
            }}"#,
        )
        .unwrap();
        assert_eq!(store.all(), BTreeMap::from([("pi".to_string(), "Ubuntu".to_string())]));
        assert_eq!(store.get("claude-code"), None);
        assert_eq!(store.get("opencode"), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn set_lists_linux_agents_and_none_returns_to_windows() {
        let dir = scratch("roundtrip");
        let store = Store::at(&dir);
        store.set("claude-code", Some(&distro("Ubuntu"))).unwrap();
        store.set("codex", Some(&distro("Ubuntu-24.04"))).unwrap();
        assert_eq!(store.all().len(), 2);
        store.set("claude-code", None).unwrap();
        assert_eq!(store.get("claude-code"), None);
        assert_eq!(
            store.all(),
            BTreeMap::from([("codex".to_string(), "Ubuntu-24.04".to_string())])
        );
        // Clearing an agent that was never set is fine too.
        store.set("opencode", None).unwrap();
        store.set("codex", None).unwrap();
        assert!(store.all().is_empty());
        assert!(store.set("", Some(&distro("Ubuntu"))).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn write_is_atomic_and_creates_the_folder() {
        let dir = scratch("atomic");
        let store = Store::at(&dir.join("not-yet-there"));
        store.set("codex", Some(&distro("Ubuntu"))).unwrap();
        store.set("codex", Some(&distro("Debian"))).unwrap();
        assert_eq!(store.get("codex").as_deref(), Some("Debian"));
        assert!(!store.path.with_extension("json.tmp").exists());
        assert_eq!(read_json(&store)["version"], 1);
        let _ = fs::remove_dir_all(dir);
    }
}
