//! What `apply` changed, kept in `<app data>/agent-pack.json`, so `undo` can put every file back.
//!
//! An entry is written the first time Puppet Master touches a path and keeps the ORIGINAL state
//! (created, or backed up) across later updates of the pack, so undo always returns to what the
//! user had before.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    /// A pack file (`<skills dir>/<skill>/SKILL.md`).
    File {
        path: String,
        /// The file did not exist before Puppet Master wrote it.
        created: bool,
        /// Copy of the file Puppet Master replaced (when `created` is false and it was not ours).
        backup: Option<String>,
        /// Directories apply created for this file, shallowest first.
        created_dirs: Vec<String>,
        /// Hash of what apply last wrote; undo leaves the file alone if it changed since.
        written_sha: String,
    },
    /// Puppet Master's own entry in an agent's MCP config.
    Mcp {
        path: String,
        format: String,
        /// Dotted key path, e.g. `mcpServers.puppet-master`.
        key_path: String,
        file_created: bool,
        created_dirs: Vec<String>,
        /// The `.bak` copy apply made of the file (None when the file was created, or a `.bak` already existed).
        #[serde(default)]
        backup: Option<String>,
        /// The value written under the key.
        written: Value,
        /// Hash of the whole file as apply left it (used to delete a file apply created).
        written_file_sha: String,
    },
}

impl Entry {
    fn same_target(&self, other: &Entry) -> bool {
        match (self, other) {
            (Entry::File { path: a, .. }, Entry::File { path: b, .. }) => a == b,
            (Entry::Mcp { path: a, key_path: ka, .. }, Entry::Mcp { path: b, key_path: kb, .. }) => a == b && ka == kb,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

pub fn sha_hex(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn record_path(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-pack.json")
}

pub fn backups_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-pack-backups")
}

impl Record {
    pub fn load(state_dir: &Path) -> Result<Self, String> {
        match fs::read_to_string(record_path(state_dir)) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("The install record is not valid, so Undo can't run: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("could not read the install record: {e}")),
        }
    }

    pub fn save(&self, state_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(state_dir).map_err(|e| format!("could not create {}: {e}", state_dir.display()))?;
        let path = record_path(state_dir);
        if self.entries.is_empty() {
            let _ = fs::remove_file(&path);
            return Ok(());
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n";
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &path).map_err(|e| format!("could not write {}: {e}", path.display()))
    }

    pub fn find(&self, probe: &Entry) -> Option<&Entry> {
        self.entries.iter().find(|e| e.same_target(probe))
    }

    /// Add an entry, or refresh the "last written" part of an existing one while keeping the
    /// original state (`created`, `backup`, `created_dirs`, `file_created`).
    pub fn upsert(&mut self, entry: Entry) {
        match self.entries.iter_mut().find(|e| e.same_target(&entry)) {
            Some(old) => match (old, entry) {
                (Entry::File { written_sha, .. }, Entry::File { written_sha: new, .. }) => *written_sha = new,
                (Entry::Mcp { written, written_file_sha, .. }, Entry::Mcp { written: w, written_file_sha: s, .. }) => {
                    *written = w;
                    *written_file_sha = s;
                }
                _ => {}
            },
            None => self.entries.push(entry),
        }
    }

    pub fn remove(&mut self, probe: &Entry) {
        self.entries.retain(|e| !e.same_target(probe));
    }
}
