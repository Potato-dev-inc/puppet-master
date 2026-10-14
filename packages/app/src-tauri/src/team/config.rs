//! `team.json` in the app data folder: the v0 team, written by hand or from Settings › General ›
//! Labs › Team. No invites and no keys in v0 (those are v1, plan §4).
//!
//! ```json
//! { "enabled": true, "team": "acme", "folder": "D:\\pm-team-demo\\wire",
//!   "me": { "id": "…", "name": "Maya" }, "members": [{ "name": "Leo" }], "projects": ["D:\\work\\billing"] }
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "team.json";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    /// Stable id. v0: a random id made on first save; v1: the member key's fingerprint.
    #[serde(default)]
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamConfig {
    /// Labs switch; off by default.
    #[serde(default)]
    pub enabled: bool,
    /// Team id carried on every event; events of another team are ignored.
    #[serde(default = "default_team")]
    pub team: String,
    /// The shared folder events travel through (`FolderTransport`).
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub me: Member,
    /// This install's device id (one member can have several).
    #[serde(default)]
    pub device: String,
    /// Teammates, by name.
    #[serde(default)]
    pub members: Vec<Member>,
    /// Extra project folders to watch for pushes; folders of open panes are watched anyway.
    #[serde(default)]
    pub projects: Vec<String>,
}

fn default_team() -> String {
    "team".into()
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            team: default_team(),
            folder: String::new(),
            me: Member::default(),
            device: String::new(),
            members: Vec::new(),
            projects: Vec::new(),
        }
    }
}

impl TeamConfig {
    /// Enabled and complete enough to run: a folder and a name.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.folder.trim().is_empty() && !self.me.name.trim().is_empty()
    }

    /// Teammates' names, for "Tell Leo and Sam".
    pub fn member_names(&self) -> Vec<String> {
        self.members.iter().map(|m| m.name.clone()).filter(|n| !n.trim().is_empty()).collect()
    }

    /// Clean what the person typed and fill the ids on first save.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.team = super::event::clean_text(&self.team, 64);
        if self.team.is_empty() {
            self.team = default_team();
        }
        self.folder = self.folder.trim().to_string();
        self.me.name = super::event::clean_text(&self.me.name, 40);
        if self.me.id.trim().is_empty() {
            self.me.id = uuid::Uuid::new_v4().to_string();
        }
        if self.device.trim().is_empty() {
            self.device = uuid::Uuid::new_v4().to_string();
        }
        let mut names = std::collections::BTreeSet::new();
        self.members = self
            .members
            .into_iter()
            .map(|m| Member { id: m.id.trim().to_string(), name: super::event::clean_text(&m.name, 40) })
            .filter(|m| !m.name.is_empty() && !m.name.eq_ignore_ascii_case(&self.me.name))
            .filter(|m| names.insert(m.name.to_ascii_lowercase()))
            .collect();
        self.projects = self
            .projects
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if self.enabled {
            if self.folder.is_empty() {
                return Err("Pick the shared folder first.".into());
            }
            if !Path::new(&self.folder).is_absolute() {
                return Err("The shared folder must be a full path.".into());
            }
            if self.me.name.is_empty() {
                return Err("Type your name first; teammates see it on their cards.".into());
            }
        }
        Ok(self)
    }
}

pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

pub fn load(path: &Path) -> TeamConfig {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, config: &TeamConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
