//! Snapshots on disk: one JSON file per snapshot under `<project>/.puppet-master/tamper/`.
//!
//! The checkpoints shadow repository is keyed by pane and pruned as turns go by, and its git needs
//! the shadow repo to exist. A tamper snapshot has to outlive both and carry its own bytes, so it
//! is a plain file. The one thing reused from checkpoints is the project-storage git exclusion.

use std::fs;
use std::path::{Path, PathBuf};

use super::model::Snapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TamperError {
    /// No snapshot with that id in this project.
    NotFound(String),
    /// The id is not one of ours (letters, digits, `-` and `_`).
    BadId(String),
    /// The project folder is missing or not a folder.
    BadProject(String),
    Io(String),
    Corrupt(String),
}

impl std::fmt::Display for TamperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TamperError::NotFound(id) => write!(f, "There is no saved test snapshot called {id} for this project."),
            TamperError::BadId(id) => write!(f, "{id:?} is not a valid snapshot name."),
            TamperError::BadProject(p) => write!(f, "{p} is not a project folder."),
            TamperError::Io(e) => write!(f, "Couldn't read or write the test snapshot: {e}"),
            TamperError::Corrupt(e) => write!(f, "The saved test snapshot is damaged: {e}"),
        }
    }
}

impl std::error::Error for TamperError {}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn dir(root: &Path) -> PathBuf {
    root.join(".puppet-master").join("tamper")
}

fn file(root: &Path, id: &str) -> Result<PathBuf, TamperError> {
    if !valid_id(id) {
        return Err(TamperError::BadId(id.to_string()));
    }
    Ok(dir(root).join(format!("{id}.json")))
}

pub fn save(root: &Path, snapshot: &Snapshot) -> Result<(), TamperError> {
    let path = file(root, &snapshot.id)?;
    crate::git_exclude::ensure_project_storage_excluded(root);
    fs::create_dir_all(dir(root)).map_err(|e| TamperError::Io(e.to_string()))?;
    let json = serde_json::to_vec(snapshot).map_err(|e| TamperError::Io(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| TamperError::Io(e.to_string()))?;
    fs::rename(&tmp, &path).map_err(|e| TamperError::Io(e.to_string()))
}

pub fn load(root: &Path, id: &str) -> Result<Snapshot, TamperError> {
    let path = file(root, id)?;
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(TamperError::NotFound(id.to_string())),
        Err(e) => return Err(TamperError::Io(e.to_string())),
    };
    let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(|e| TamperError::Corrupt(e.to_string()))?;
    if snapshot.version > super::model::SNAPSHOT_VERSION {
        return Err(TamperError::Corrupt(format!("it was saved by a newer version (format {})", snapshot.version)));
    }
    Ok(snapshot)
}

/// Ids of the saved snapshots, newest first.
pub fn list(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir(root)) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".json")?.to_string();
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            valid_id(&id).then_some((modified, id))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, id)| id).collect()
}

pub fn delete(root: &Path, id: &str) -> Result<(), TamperError> {
    let path = file(root, id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(TamperError::Io(e.to_string())),
    }
}
