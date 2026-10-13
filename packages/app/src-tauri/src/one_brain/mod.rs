//! One brain: one project memory (`<project>/.puppet-master/memory.md`) rendered into the file each
//! agent reads its instructions from, inside a managed marker block (see `block`).
//!
//! Which files exist is declared per agent (`TuiProfile::instruction_files`, TOML `[instructions]`);
//! nothing here knows an agent by name. A sync runs before a pane or run spawns in a project and
//! when the memory is saved from the UI. Content outside the block is never modified.
//!
//! The feature is off for a project when `.puppet-master/memory.off` exists, and off everywhere
//! when the app-wide switch (`one_brain.json` in the app data dir) is off. Turning it off removes
//! the blocks the next time a sync runs in that project.

mod block;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::tui::TuiProfile;
pub use block::{block_state, render, BlockError, BlockState, Outcome, END, START};

pub const MEMORY_REL: &str = ".puppet-master/memory.md";
const OFF_REL: &str = ".puppet-master/memory.off";

/// One file to keep in sync, with the agents that read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    pub path: String,
    #[serde(skip)]
    pub header: Option<String>,
    pub agents: Vec<String>,
}

/// A path that stays inside the project: relative, no `..`, no drive, no leading slash.
pub fn relative_path_ok(path: &str) -> bool {
    let p = path.trim();
    if p.is_empty() || p.starts_with('/') || p.starts_with('\\') || p.contains(':') {
        return false;
    }
    p.split(['/', '\\']).all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Every declared file across `profiles`; profiles declaring the same path share one target.
pub fn targets_from(profiles: &[Arc<dyn TuiProfile>]) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    for profile in profiles {
        for file in profile.instruction_files() {
            if !relative_path_ok(&file.path) {
                continue;
            }
            let path = file.path.replace('\\', "/");
            match out.iter_mut().find(|t| t.path.eq_ignore_ascii_case(&path)) {
                Some(t) => {
                    if !t.agents.iter().any(|a| a == profile.id()) {
                        t.agents.push(profile.id().to_string());
                    }
                    if t.header.is_none() {
                        t.header = file.header;
                    }
                }
                None => out.push(Target { path, header: file.header, agents: vec![profile.id().to_string()] }),
            }
        }
    }
    out
}

/// Targets of the profiles registered right now (code profiles and approved definitions).
pub fn targets() -> Vec<Target> {
    targets_from(&crate::tui::registry::snapshot())
}

// ---- memory + switches -------------------------------------------------------------------

pub fn memory_path(project: &Path) -> PathBuf {
    project.join(MEMORY_REL)
}

pub fn read_memory(project: &Path) -> String {
    std::fs::read_to_string(memory_path(project)).unwrap_or_default()
}

pub fn write_memory(project: &Path, text: &str) -> std::io::Result<()> {
    let path = memory_path(project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

pub fn project_enabled(project: &Path) -> bool {
    !project.join(OFF_REL).exists()
}

pub fn set_project_enabled(project: &Path, enabled: bool) -> std::io::Result<()> {
    let marker = project.join(OFF_REL);
    if enabled {
        match std::fs::remove_file(&marker) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        if let Some(dir) = marker.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(marker, "Shared project memory is off for this project.\n")
    }
}

fn global_file() -> PathBuf {
    crate::app_paths::app_data_dir().join("one_brain.json")
}

/// The app-wide "Shared project memory" switch; on unless explicitly turned off.
pub fn global_enabled() -> bool {
    std::fs::read_to_string(global_file())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

pub fn set_global_enabled(enabled: bool) -> std::io::Result<()> {
    let path = global_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::json!({ "enabled": enabled }).to_string())
}

/// The memory an agent file should carry: empty when the feature is off here.
fn effective_memory(project: &Path, global: bool) -> String {
    if global && project_enabled(project) {
        read_memory(project)
    } else {
        String::new()
    }
}

// ---- status ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetStatus {
    pub path: String,
    pub agents: Vec<String>,
    /// `in_sync` | `will_create` | `will_add` | `will_update` | `will_remove` | `nothing` | `blocked`.
    pub status: &'static str,
    /// The file has text of its own outside the managed block (it is never touched).
    pub user_content: bool,
}

fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// True when the file holds non-blank text outside the managed block.
fn has_user_content(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    match (text.find(START), text.find(END)) {
        (Some(s), Some(e)) if e > s => {
            !text[..s].trim().is_empty() || !text[e + END.len()..].trim().is_empty()
        }
        _ => !text.trim().is_empty(),
    }
}

pub fn status(project: &Path, targets: &[Target], global: bool) -> Vec<TargetStatus> {
    let memory = effective_memory(project, global);
    targets
        .iter()
        .map(|t| {
            let existing = read_existing(&project.join(&t.path)).unwrap_or(None);
            let user_content = existing.as_deref().is_some_and(has_user_content);
            let status = match render(existing.as_deref(), t.header.as_deref(), &memory) {
                Err(_) => "blocked",
                Ok(Outcome::NoChange) if memory.trim().is_empty() => "nothing",
                Ok(Outcome::NoChange) => "in_sync",
                Ok(Outcome::Delete) => "will_remove",
                Ok(Outcome::Write(_)) => match existing.as_deref().map(block_state) {
                    None => "will_create",
                    Some(BlockState::Present) if memory.trim().is_empty() => "will_remove",
                    Some(BlockState::Present) => "will_update",
                    Some(_) => "will_add",
                },
            };
            TargetStatus { path: t.path.clone(), agents: t.agents.clone(), status, user_content }
        })
        .collect()
}

// ---- sync --------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub written: Vec<String>,
    pub removed: Vec<String>,
    pub errors: Vec<String>,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.pm-tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Sync one target. Backs an existing user file up to `<file>.bak` once, before the first block is
/// inserted into it.
fn sync_target(project: &Path, t: &Target, memory: &str, report: &mut SyncReport) {
    let path = project.join(&t.path);
    let existing = match read_existing(&path) {
        Ok(v) => v,
        Err(e) => return report.errors.push(format!("{}: {e}", t.path)),
    };
    let outcome = match render(existing.as_deref(), t.header.as_deref(), memory) {
        Ok(o) => o,
        Err(_) => {
            return report.errors.push(format!("{}: the memory markers are damaged; fix or delete them", t.path))
        }
    };
    match outcome {
        Outcome::NoChange => {}
        Outcome::Delete => match std::fs::remove_file(&path) {
            Ok(()) => report.removed.push(t.path.clone()),
            Err(e) => report.errors.push(format!("{}: {e}", t.path)),
        },
        Outcome::Write(bytes) => {
            if let Some(old) = &existing {
                if block_state(old) == BlockState::Absent {
                    let bak = PathBuf::from(format!("{}.bak", path.display()));
                    if !bak.exists() {
                        if let Err(e) = std::fs::write(&bak, old) {
                            return report.errors.push(format!("{}: backup failed: {e}", t.path));
                        }
                    }
                }
            }
            let removing = memory.trim().is_empty();
            match write_atomic(&path, &bytes) {
                Ok(()) if removing => report.removed.push(t.path.clone()),
                Ok(()) => report.written.push(t.path.clone()),
                Err(e) => report.errors.push(format!("{}: {e}", t.path)),
            }
        }
    }
}

static SYNC_LOCK: Mutex<()> = Mutex::new(());

/// Render the project's memory into every target (or remove the blocks when the feature is off or
/// the memory is empty). Idempotent: unchanged files are not written.
pub fn sync(project: &Path, targets: &[Target], global: bool) -> SyncReport {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let memory = effective_memory(project, global);
    let mut report = SyncReport::default();
    for t in targets {
        sync_target(project, t, &memory, &mut report);
    }
    report
}

/// The spawn hook's body: sync every registered agent's files in `cwd`.
pub fn sync_for_spawn(cwd: &str, global: bool) -> SyncReport {
    let path = Path::new(cwd);
    if cwd.trim().is_empty() || !path.is_dir() {
        return SyncReport::default();
    }
    sync(path, &targets(), global)
}

/// Called before a pane or run spawns in `cwd`. Never fails the spawn.
pub fn sync_on_spawn(cwd: &str) {
    let report = sync_for_spawn(cwd, global_enabled());
    if !report.written.is_empty() || !report.removed.is_empty() {
        tracing::info!(cwd, written = ?report.written, removed = ?report.removed, "project memory synced");
    }
    for error in &report.errors {
        tracing::warn!(cwd, %error, "project memory sync");
    }
}

// ---- what the UI shows -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MemoryView {
    pub project: String,
    pub memory: String,
    pub global_enabled: bool,
    pub project_enabled: bool,
    pub targets: Vec<TargetStatus>,
    pub report: Option<SyncReport>,
}

pub fn view(project: &Path, report: Option<SyncReport>) -> MemoryView {
    let global = global_enabled();
    MemoryView {
        project: project.display().to_string(),
        memory: read_memory(project),
        global_enabled: global,
        project_enabled: project_enabled(project),
        targets: status(project, &targets(), global),
        report,
    }
}
