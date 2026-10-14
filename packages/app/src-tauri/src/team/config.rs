//! `team.json` in the app data folder: which team this app is on and how it reaches it. Not a
//! secret: keys are in the credential store (`keystore`) and the member list is the signed roster
//! (`roster.json`, checked on every load).
//!
//! ```json
//! { "enabled": true, "team": "<team id>", "name": "Acme", "relay": "https://relay.example.com",
//!   "me": { "id": "<member key>", "name": "Maya" }, "device": "<device key>",
//!   "root": "<admin key>", "salt": "…", "projects": ["D:\\work\\billing"] }
//! ```
//!
//! A file that exists but can't be read or parsed is reported (Settings says it couldn't be read)
//! and Team stays off; it is never silently replaced by an empty team.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "team.json";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    /// My member key (base64url), filled when I create or join a team.
    #[serde(default)]
    pub id: String,
    pub name: String,
}

/// Waiting for the admin to let me in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJoin {
    pub link: super::invite::InviteLink,
    pub sent_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamConfig {
    /// Labs switch; off by default.
    #[serde(default)]
    pub enabled: bool,
    /// Team id (a hash of the admin key); empty until I create or join a team.
    #[serde(default)]
    pub team: String,
    /// The team's name, for display.
    #[serde(default)]
    pub name: String,
    /// The relay address (`RelayTransport`), or empty to use `folder`.
    #[serde(default)]
    pub relay: String,
    /// A shared folder (`FolderTransport`).
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub me: Member,
    /// This install's device key (one member can have several).
    #[serde(default)]
    pub device: String,
    /// The team's admin key and id salt, pinned when I created or joined.
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub salt: String,
    /// My GitHub login, as I claimed it when joining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    /// Extra project folders to watch for pushes; folders of open panes are watched anyway.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Set while a join request waits for the admin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<PendingJoin>,
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            team: String::new(),
            name: String::new(),
            relay: String::new(),
            folder: String::new(),
            me: Member::default(),
            device: String::new(),
            root: String::new(),
            salt: String::new(),
            github: None,
            projects: Vec::new(),
            join: None,
        }
    }
}

impl TeamConfig {
    /// On a team (created or joined), with a way to reach it.
    pub fn has_team(&self) -> bool {
        !self.team.is_empty() && !self.root.is_empty() && (!self.relay.trim().is_empty() || !self.folder.trim().is_empty())
    }

    /// Enabled and on a team: the transport runs.
    pub fn is_active(&self) -> bool {
        self.enabled && self.has_team() && !self.me.name.trim().is_empty()
    }

    /// Clean what the person typed.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.name = super::event::clean_text(&self.name, 60);
        self.relay = self.relay.trim().to_string();
        self.folder = self.folder.trim().to_string();
        self.me.name = super::event::clean_text(&self.me.name, 40);
        self.projects = self
            .projects
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if !self.relay.is_empty() {
            self.relay = super::transport::relay::normalize_relay_url(&self.relay)?;
        }
        if !self.folder.is_empty() && !Path::new(&self.folder).is_absolute() {
            return Err("The shared folder must be a full path.".into());
        }
        Ok(self)
    }
}

pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// The config, or why the file that is there could not be used. A missing file is a fresh start.
pub fn load_checked(path: &Path) -> Result<TeamConfig, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TeamConfig::default()),
        Err(e) => return Err(format!("Your team settings couldn't be read ({e}).")),
    };
    serde_json::from_slice(&bytes).map_err(|e| format!("Your team settings couldn't be read: the file is damaged ({e})."))
}

/// Write the config. If the file on disk could not be read, it is kept next to it first
/// (`team.json.unreadable`), never silently lost.
pub fn save(path: &Path, config: &TeamConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.exists() && load_checked(path).is_err() {
        let _ = std::fs::rename(path, path.with_extension("json.unreadable"));
    }
    let json = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
