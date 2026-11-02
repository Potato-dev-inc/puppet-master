// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! What the person chose on the My computers page. Not secret, so a plain JSON file in the app data folder.

use std::path::PathBuf;
use std::sync::Mutex;

use desk_core::HopMode;
use serde::{Deserialize, Serialize};

use super::hotkeys::{compile, HotkeyConfig};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// "Screen hops" switch on the page. Off until the person turns it on; the hooks follow it.
    pub hops_enabled: bool,
    pub hop_mode: HopMode,
    pub hotkeys: HotkeyConfig,
    /// Not saved: a restart never starts paused-and-forgotten, and never starts paused hooks.
    #[serde(skip)]
    pub paused: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { hops_enabled: false, hop_mode: HopMode::Relative, hotkeys: HotkeyConfig::default(), paused: false }
    }
}

/// A loaded config plus a note when something saved could not be used.
pub struct Loaded {
    pub config: Config,
    pub note: Option<String>,
}

pub trait ConfigStore: Send + Sync {
    fn load(&self) -> Loaded;
    fn save(&self, config: &Config) -> Result<(), String>;
}

/// Parse saved text. A file that does not parse, or whose hotkeys fail the checks, gives the defaults
/// for what is wrong and says so (the original swallowed it).
/// The lock-pointer key before Scroll Lock became its default (the pause key moved to Ctrl+Alt+P at the same time).
const LOCK_POINTER_BEFORE: &str = "ctrl+alt+l";

pub fn parse(text: &str) -> Loaded {
    let mut config: Config = match serde_json::from_str(text) {
        Ok(c) => c,
        Err(e) => {
            return Loaded { config: Config::default(), note: Some(format!("Saved settings could not be read ({e}); using the defaults")) }
        }
    };
    // A file that saved `pause` as Scroll Lock but has no `lock_pointer` would get the new lock default, which is the
    // same key. Saved values win, so the missing one takes the key it had before, and the saved keys stay.
    if compile(&config.hotkeys).is_err() && !saved_hotkey(text, "lock_pointer") {
        config.hotkeys.lock_pointer = LOCK_POINTER_BEFORE.to_string();
    }
    let mut note = None;
    if compile(&config.hotkeys).is_err() {
        config.hotkeys = HotkeyConfig::default();
        note = Some("Saved hotkeys were not valid; using the default keys".to_string());
    }
    Loaded { config, note }
}

/// Whether the saved file has this hotkey field. A file saved by an older build may lack some of them.
fn saved_hotkey(text: &str, field: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| v.get("hotkeys")?.get(field).cloned()).is_some()
}

pub struct FileStore {
    pub path: PathBuf,
}

impl ConfigStore for FileStore {
    fn load(&self) -> Loaded {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded { config: Config::default(), note: None },
            Err(e) => Loaded { config: Config::default(), note: Some(format!("Saved settings could not be opened ({e})")) },
        }
    }

    fn save(&self, config: &Config) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        // Write beside, then rename: a crash never leaves half a file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

/// In memory only: tests, and the build where nothing may touch the person's folder.
#[cfg_attr(not(test), allow(dead_code))]
pub struct MemStore {
    pub text: Mutex<Option<String>>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl MemStore {
    pub fn new(text: Option<&str>) -> Self {
        Self { text: Mutex::new(text.map(str::to_string)) }
    }
}

impl ConfigStore for MemStore {
    fn load(&self) -> Loaded {
        match self.text.lock().unwrap().as_deref() {
            Some(t) => parse(t),
            None => Loaded { config: Config::default(), note: None },
        }
    }

    fn save(&self, config: &Config) -> Result<(), String> {
        *self.text.lock().unwrap() = Some(serde_json::to_string(config).map_err(|e| e.to_string())?);
        Ok(())
    }
}
