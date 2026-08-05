//! OpenCode API key profiles for rotation (desktop-only storage, never exposed via MCP).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const STORE_VERSION: u32 = 2;
const DEFAULT_PROVIDER: &str = "opencode-go";
pub const PROFILE_A: &str = "a";
pub const PROFILE_B: &str = "b";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAutomationMode {
    AutoIfBackup,
    NotifyOnly,
}

impl KeyAutomationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoIfBackup => "auto_if_backup",
            Self::NotifyOnly => "notify_only",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "auto_if_backup" | "auto" => Some(Self::AutoIfBackup),
            "notify_only" | "notify" => Some(Self::NotifyOnly),
            _ => None,
        }
    }
}

fn default_automation_mode() -> KeyAutomationMode {
    KeyAutomationMode::AutoIfBackup
}

fn default_restart_pane_on_rotate() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyProfilesStore {
    version: u32,
    active: String,
    profiles: BTreeMap<String, KeyProfile>,
    #[serde(default = "default_automation_mode")]
    key_automation_mode: KeyAutomationMode,
    #[serde(default = "default_restart_pane_on_rotate")]
    restart_pane_on_rotate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyProfile {
    #[serde(default)]
    label: String,
    auth: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenCodeKeyProfileInfo {
    pub id: String,
    pub label: String,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenCodeKeyStatus {
    pub active_profile: String,
    pub profiles: Vec<OpenCodeKeyProfileInfo>,
    pub key_automation_mode: String,
    pub restart_pane_on_rotate: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeKeySettingsPatch {
    pub key_automation_mode: Option<String>,
    pub restart_pane_on_rotate: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateTarget {
    Next,
    ProfileA,
    ProfileB,
}

impl RotateTarget {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "next" | "toggle" | "other" | "rotate" => Some(Self::Next),
            "a" | "primary" => Some(Self::ProfileA),
            "b" | "secondary" => Some(Self::ProfileB),
            _ => None,
        }
    }
}

pub fn store_path() -> PathBuf {
    test_paths::store_path_override()
        .unwrap_or_else(|| crate::app_paths::app_data_dir().join("opencode-key-profiles.json"))
}

pub fn auth_json_path() -> PathBuf {
    test_paths::auth_json_path_override().unwrap_or_else(|| {
        let home = crate::project_path::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json")
    })
}

pub fn status() -> Result<OpenCodeKeyStatus, String> {
    let store = load_store()?;
    Ok(public_status(&store))
}

pub fn automation_mode() -> Result<KeyAutomationMode, String> {
    Ok(load_store()?.key_automation_mode)
}

pub fn restart_pane_on_rotate() -> Result<bool, String> {
    Ok(load_store()?.restart_pane_on_rotate)
}

pub fn set_automation_settings(
    mode: Option<KeyAutomationMode>,
    restart_pane_on_rotate: Option<bool>,
) -> Result<OpenCodeKeyStatus, String> {
    let mut store = load_store()?;
    ensure_default_profiles(&mut store);
    if let Some(mode) = mode {
        store.key_automation_mode = mode;
    }
    if let Some(restart) = restart_pane_on_rotate {
        store.restart_pane_on_rotate = restart;
    }
    store.version = STORE_VERSION;
    save_store(&store)?;
    status()
}

pub fn patch_settings(patch: &OpenCodeKeySettingsPatch) -> Result<OpenCodeKeyStatus, String> {
    let mode = patch
        .key_automation_mode
        .as_deref()
        .map(|raw| {
            KeyAutomationMode::parse(raw)
                .ok_or_else(|| format!("unsupported key_automation_mode '{raw}'"))
        })
        .transpose()?;
    set_automation_settings(mode, patch.restart_pane_on_rotate)
}

pub fn toggle_profile_id(current: &str) -> String {
    toggle_active(current)
}

pub fn set_profile_api_key(profile_id: &str, api_key: &str, label: Option<&str>) -> Result<(), String> {
    let id = normalize_profile_id(profile_id)?;
    let key = api_key.trim();
    if key.is_empty() {
        return Err("api_key must not be empty".into());
    }
    let mut store = load_store()?;
    ensure_default_profiles(&mut store);
    let entry = store.profiles.entry(id.clone()).or_insert_with(|| KeyProfile {
        label: default_label_for_id(&id),
        auth: json!({}),
    });
    if let Some(label) = label.map(str::trim).filter(|value| !value.is_empty()) {
        entry.label = label.to_string();
    }
    entry.auth = auth_json_for_key(key);
    save_store(&store)
}

pub fn capture_profile_from_disk(profile_id: &str, label: Option<&str>) -> Result<(), String> {
    let id = normalize_profile_id(profile_id)?;
    let auth = read_auth_json()?;
    let mut store = load_store()?;
    ensure_default_profiles(&mut store);
    let entry = store.profiles.entry(id.clone()).or_insert_with(|| KeyProfile {
        label: default_label_for_id(&id),
        auth: json!({}),
    });
    entry.auth = auth;
    if let Some(label) = label.map(str::trim).filter(|value| !value.is_empty()) {
        entry.label = label.to_string();
    }
    save_store(&store)
}

pub fn rotate(target: RotateTarget) -> Result<OpenCodeKeyStatus, String> {
    let mut store = load_store()?;
    ensure_default_profiles(&mut store);
    let next = match target {
        RotateTarget::Next => toggle_active(&store.active),
        RotateTarget::ProfileA => PROFILE_A.to_string(),
        RotateTarget::ProfileB => PROFILE_B.to_string(),
    };
    if !profile_configured(&store, &next) {
        return Err(format!(
            "OpenCode key profile '{next}' is not configured — set it in Puppet Master settings (desktop only)"
        ));
    }
    store.active = next;
    save_store(&store)?;
    apply_active_auth()?;
    status()
}

pub fn apply_active_auth() -> Result<(), String> {
    let store = load_store()?;
    let profile = store
        .profiles
        .get(&store.active)
        .filter(|profile| profile_configured_value(&profile.auth))
        .ok_or_else(|| {
            format!(
                "active OpenCode key profile '{}' is not configured",
                store.active
            )
        })?;
    write_auth_json(&profile.auth)
}

fn load_store() -> Result<KeyProfilesStore, String> {
    let path = store_path();
    if !path.is_file() {
        return Ok(default_store());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("read {}: {err}", path.display()))?;
    let mut store: KeyProfilesStore = serde_json::from_str(&raw)
        .map_err(|err| format!("parse {}: {err}", path.display()))?;
    if store.version < STORE_VERSION {
        store.version = STORE_VERSION;
    }
    ensure_default_profiles(&mut store);
    Ok(store)
}

fn save_store(store: &KeyProfilesStore) -> Result<(), String> {
    crate::app_paths::ensure_app_data_dir()?;
    let path = store_path();
    let json = serde_json::to_string_pretty(store)
        .map_err(|err| format!("serialize opencode key profiles: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write {}: {err}", path.display()))
}

fn default_store() -> KeyProfilesStore {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        PROFILE_A.to_string(),
        KeyProfile {
            label: "Primary".into(),
            auth: json!({}),
        },
    );
    profiles.insert(
        PROFILE_B.to_string(),
        KeyProfile {
            label: "Secondary".into(),
            auth: json!({}),
        },
    );
    KeyProfilesStore {
        version: STORE_VERSION,
        active: PROFILE_A.to_string(),
        profiles,
        key_automation_mode: default_automation_mode(),
        restart_pane_on_rotate: default_restart_pane_on_rotate(),
    }
}

fn ensure_default_profiles(store: &mut KeyProfilesStore) {
    store.profiles.entry(PROFILE_A.to_string()).or_insert(KeyProfile {
        label: "Primary".into(),
        auth: json!({}),
    });
    store.profiles.entry(PROFILE_B.to_string()).or_insert(KeyProfile {
        label: "Secondary".into(),
        auth: json!({}),
    });
    if store.active != PROFILE_A && store.active != PROFILE_B {
        store.active = PROFILE_A.to_string();
    }
}

fn public_status(store: &KeyProfilesStore) -> OpenCodeKeyStatus {
    OpenCodeKeyStatus {
        active_profile: store.active.clone(),
        profiles: store
            .profiles
            .iter()
            .map(|(id, profile)| OpenCodeKeyProfileInfo {
                id: id.clone(),
                label: profile.label.clone(),
                configured: profile_configured_value(&profile.auth),
            })
            .collect(),
        key_automation_mode: store.key_automation_mode.as_str().to_string(),
        restart_pane_on_rotate: store.restart_pane_on_rotate,
    }
}

fn profile_configured(store: &KeyProfilesStore, id: &str) -> bool {
    store
        .profiles
        .get(id)
        .is_some_and(|profile| profile_configured_value(&profile.auth))
}

fn profile_configured_value(auth: &Value) -> bool {
    auth.as_object().is_some_and(|map| !map.is_empty())
}

fn toggle_active(current: &str) -> String {
    if current == PROFILE_A {
        PROFILE_B.to_string()
    } else {
        PROFILE_A.to_string()
    }
}

fn normalize_profile_id(profile_id: &str) -> Result<String, String> {
    match profile_id.trim().to_lowercase().as_str() {
        "a" | "primary" => Ok(PROFILE_A.to_string()),
        "b" | "secondary" => Ok(PROFILE_B.to_string()),
        other => Err(format!("unsupported OpenCode key profile '{other}' (use a or b)")),
    }
}

fn default_label_for_id(id: &str) -> String {
    if id == PROFILE_A {
        "Primary".into()
    } else {
        "Secondary".into()
    }
}

fn auth_json_for_key(api_key: &str) -> Value {
    json!({
        DEFAULT_PROVIDER: {
            "type": "api",
            "key": api_key
        }
    })
}

fn read_auth_json() -> Result<Value, String> {
    let path = auth_json_path();
    if !path.is_file() {
        return Err(format!(
            "OpenCode auth file not found at {} — run `opencode auth login` first",
            path.display()
        ));
    }
    let raw = fs::read_to_string(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_str(&raw).map_err(|err| format!("parse {}: {err}", path.display()))
}

fn write_auth_json(auth: &Value) -> Result<(), String> {
    let path = auth_json_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(auth)
        .map_err(|err| format!("serialize opencode auth.json: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write {}: {err}", path.display()))
}

#[cfg(test)]
mod test_paths {
    use std::path::PathBuf;

    pub fn store_path_override() -> Option<PathBuf> {
        std::env::var_os("PUPPET_MASTER_TEST_OPENCODE_KEYS_STORE").map(PathBuf::from)
    }

    pub fn auth_json_path_override() -> Option<PathBuf> {
        std::env::var_os("PUPPET_MASTER_TEST_OPENCODE_AUTH").map(PathBuf::from)
    }
}

#[cfg(test)]
pub(crate) fn test_keys_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(not(test))]
mod test_paths {
    use std::path::PathBuf;

    pub fn store_path_override() -> Option<PathBuf> {
        None
    }

    pub fn auth_json_path_override() -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        test_keys_lock()
    }

    #[test]
    fn rotate_toggles_between_profiles() {
        let _guard = lock();
        let unique = format!(
            "pm-opencode-keys-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        let store_file = dir.join("opencode-key-profiles.json");
        let auth_file = dir.join("auth.json");
        fs::create_dir_all(&dir).expect("tempdir");

        std::env::set_var("PUPPET_MASTER_TEST_OPENCODE_KEYS_STORE", &store_file);
        std::env::set_var("PUPPET_MASTER_TEST_OPENCODE_AUTH", &auth_file);

        let mut store = default_store();
        store.profiles.get_mut(PROFILE_A).unwrap().auth = auth_json_for_key("key-a");
        store.profiles.get_mut(PROFILE_B).unwrap().auth = auth_json_for_key("key-b");
        save_store(&store).expect("save");

        let status = rotate(RotateTarget::Next).expect("rotate");
        assert_eq!(status.active_profile, PROFILE_B);
        let written: Value = serde_json::from_str(&fs::read_to_string(&auth_file).unwrap()).unwrap();
        assert_eq!(
            written[DEFAULT_PROVIDER]["key"].as_str(),
            Some("key-b")
        );

        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("PUPPET_MASTER_TEST_OPENCODE_KEYS_STORE");
        std::env::remove_var("PUPPET_MASTER_TEST_OPENCODE_AUTH");
    }

    #[test]
    fn public_status_never_includes_key_material() {
        let mut store = default_store();
        store.profiles.get_mut(PROFILE_A).unwrap().auth = auth_json_for_key("secret");
        let status = public_status(&store);
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("sk-"));
    }
}
