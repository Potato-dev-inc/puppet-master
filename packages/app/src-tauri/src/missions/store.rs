//! Missions on disk (DESIGN-MISSIONS 2.6).
//!
//! ```text
//! <origin>/.puppet-master/missions/<id>/mission.json   the Mission (atomic write, rev bumped)
//! <origin>/.puppet-master/missions/<id>/rounds.jsonl   one line per event; append-only
//! <origin>/.puppet-master/missions/<id>/baseline.json  the test snapshot taken at the start
//! <app data>/missions-index.json                       id -> project, for startup recovery
//! ```
//!
//! The store lives under the origin project, not the copy, so discarding the copy keeps the
//! mission's history. `.puppet-master/` is git-excluded, so none of it is ever merged.

use super::model::{Mission, MissionState};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn root(project: &str) -> PathBuf {
    Path::new(project).join(".puppet-master").join("missions")
}

pub fn dir(project: &str, id: &str) -> PathBuf {
    root(project).join(id)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    NotFound,
    /// The record exists but can't be read: the mission ends as `failed`.
    Corrupt(String),
}

/// One lock per mission: every read-modify-write of `mission.json` goes through `update`.
static LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn lock_for(id: &str) -> Arc<Mutex<()>> {
    LOCKS.lock().entry(id.to_string()).or_default().clone()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    // Windows: rename over an open file can fail for a moment (antivirus, a reader).
    let mut last = String::new();
    for _ in 0..20 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(format!("replace {}: {last}", path.display()))
}

/// Writes the mission (bumping `rev` and `updated_at_ms`) and keeps the index current.
pub fn save(mission: &mut Mission) -> Result<(), String> {
    mission.rev += 1;
    mission.updated_at_ms = now_ms();
    if mission.state.terminal() && mission.ended_at_ms.is_none() {
        mission.ended_at_ms = Some(mission.updated_at_ms);
    }
    crate::git_exclude::ensure_project_storage_excluded(Path::new(&mission.project));
    let bytes = serde_json::to_vec_pretty(mission).map_err(|e| e.to_string())?;
    atomic_write(&dir(&mission.project, &mission.id).join("mission.json"), &bytes)?;
    index_put(&mission.id, &mission.project, mission.state);
    super::notify_changed(mission);
    Ok(())
}

pub fn load(project: &str, id: &str) -> Result<Mission, LoadError> {
    if !valid_id(id) {
        return Err(LoadError::NotFound);
    }
    let path = dir(project, id).join("mission.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LoadError::NotFound),
        Err(e) => return Err(LoadError::Corrupt(e.to_string())),
    };
    serde_json::from_slice(&bytes).map_err(|e| LoadError::Corrupt(e.to_string()))
}

/// Load, change and save under the mission's lock. `f` returns `false` to skip the save.
pub fn update<R>(project: &str, id: &str, f: impl FnOnce(&mut Mission) -> (R, bool)) -> Result<R, LoadError> {
    let lock = lock_for(id);
    let _held = lock.lock();
    let mut mission = load(project, id)?;
    let (out, changed) = f(&mut mission);
    if changed {
        save(&mut mission).map_err(LoadError::Corrupt)?;
    }
    Ok(out)
}

/// Every mission of a project, newest first. Unreadable records are skipped here.
pub fn list(project: &str) -> Vec<Mission> {
    let Ok(entries) = std::fs::read_dir(root(project)) else { return Vec::new() };
    let mut found: Vec<Mission> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter_map(|id| load(project, &id).ok())
        .collect();
    found.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    found
}

// ---- the round log ---------------------------------------------------------------------------

/// Appends one event line to `rounds.jsonl` (start, turn created, verdict, decision...).
pub fn log(project: &str, id: &str, event: &Value) {
    let path = dir(project, id).join("rounds.jsonl");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut line = serde_json::to_string(event).unwrap_or_default();
    line.push('\n');
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// The log's events. A damaged line (a crash in the middle of a write) is skipped, never fatal.
#[cfg_attr(not(test), allow(dead_code))]
pub fn read_log(project: &str, id: &str) -> Vec<Value> {
    std::fs::read_to_string(dir(project, id).join("rounds.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

// ---- the test snapshot at the start ------------------------------------------------------------

pub fn save_baseline(project: &str, id: &str, snap: &crate::tamper::Snapshot) -> Result<(), String> {
    let bytes = serde_json::to_vec(snap).map_err(|e| e.to_string())?;
    atomic_write(&dir(project, id).join("baseline.json"), &bytes)
}

pub fn load_baseline(project: &str, id: &str) -> Option<crate::tamper::Snapshot> {
    std::fs::read(dir(project, id).join("baseline.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

// ---- the index ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub project: String,
    pub state: MissionState,
}

static INDEX_LOCK: Mutex<()> = Mutex::new(());

fn index_path() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!("pm-missions-test-index-{}", std::process::id())).join("missions-index.json")
    }
    #[cfg(not(test))]
    {
        crate::app_paths::app_data_dir().join("missions-index.json")
    }
}

/// The index as stored. `None` when the file exists but can't be read right now (a replace in
/// flight, antivirus): the caller must not write an index built from nothing then.
fn read_index() -> Option<Vec<IndexEntry>> {
    let path = index_path();
    for attempt in 0..5 {
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(entries) => return Some(entries),
                Err(_) if attempt < 4 => {}
                Err(_) => return None,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
            Err(_) => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    None
}

pub fn index() -> Vec<IndexEntry> {
    let _held = INDEX_LOCK.lock();
    read_index().unwrap_or_default()
}

fn index_put(id: &str, project: &str, state: MissionState) {
    let _held = INDEX_LOCK.lock();
    let Some(mut entries) = read_index() else {
        tracing::warn!(mission = id, "the missions index can't be read now; not rewriting it");
        return;
    };
    match entries.iter_mut().find(|e| e.id == id) {
        Some(entry) if entry.state == state && entry.project == project => return,
        Some(entry) => {
            entry.state = state;
            entry.project = project.to_string();
        }
        None => entries.push(IndexEntry { id: id.to_string(), project: project.to_string(), state }),
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&entries) {
        let _ = atomic_write(&index_path(), &bytes);
    }
}

/// The project a mission id belongs to (the MCP handle carries only the id).
pub fn project_of(id: &str) -> Option<String> {
    index().into_iter().find(|e| e.id == id).map(|e| e.project)
}

/// `read_log` for the fake-TUI harness (a separate name so the app build sees no unused fn).
#[cfg(feature = "dev-fake-tui")]
pub fn read_log_all(project: &str, id: &str) -> Vec<Value> {
    read_log(project, id)
}
