//! Hot reload (P5-1): watch the enabled definition directories and keep the registry in step.
//!
//! `notify` events are noisy (editors write a temp file, rename it, touch it twice), so a small
//! debounce thread waits for the directory to go quiet and then runs one `refresh`. A refresh
//! reads only the enabled directories (it is the loader's own scan). If a definition that is
//! registered right now no longer parses (an editor is mid-write), the previous snapshot is kept
//! and the refresh is retried a few times; the registry is never left half-updated.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{json, Value};

use super::approvals::Approvals;
use super::loader::{build, DefState, Entry, LoadInputs};
use crate::tui::registry::Registry;

/// Quiet time after the last file event before a refresh runs.
pub const DEBOUNCE: Duration = Duration::from_millis(150);
/// A busy directory is refreshed at least this often while events keep arriving.
const MAX_WAIT: Duration = Duration::from_millis(600);
/// Retries after a refresh that kept the previous snapshot (half-written file, file lock).
const RETRIES: usize = 4;
const RETRY_EVERY: Duration = Duration::from_millis(350);

/// What changed between two registry builds, by agent id (active definitions only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// Ids whose warm standby workers must go: their definition changed or disappeared.
    pub fn invalidated(&self) -> impl Iterator<Item = &String> {
        self.changed.iter().chain(self.removed.iter())
    }

    pub fn to_json(&self) -> Value {
        json!({"added": self.added, "removed": self.removed, "changed": self.changed})
    }
}

fn active(entries: &[Entry]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter(|e| e.state == DefState::Active)
        .filter_map(|e| Some((e.id.clone()?, e.fingerprint.clone().unwrap_or_default())))
        .collect()
}

/// Added / removed / changed (fingerprint) ids between two row sets.
pub fn diff_entries(before: &[Entry], after: &[Entry]) -> Diff {
    let (before, after) = (active(before), active(after));
    let mut diff = Diff::default();
    for (id, fingerprint) in &after {
        match before.iter().find(|(b, _)| b == id) {
            None => diff.added.push(id.clone()),
            Some((_, old)) if old != fingerprint => diff.changed.push(id.clone()),
            _ => {}
        }
    }
    diff.removed = before
        .iter()
        .filter(|(id, _)| !after.iter().any(|(a, _)| a == id))
        .map(|(id, _)| id.clone())
        .collect();
    diff
}

#[derive(Debug)]
pub enum Refresh {
    /// The new build was swapped into the registry.
    Applied { entries: Vec<Entry>, diff: Diff },
    /// A registered definition could not be read cleanly; nothing changed.
    KeptPrevious,
}

/// Rebuild from `inputs` and swap into `registry`, unless a definition that is `Active` in
/// `previous` now fails to parse or validate (a half-written save): then keep everything as is.
pub fn refresh(
    registry: &Registry,
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    previous: &[Entry],
) -> Refresh {
    let built = build(inputs, &Approvals::load(approvals_path));
    let broke = previous
        .iter()
        .filter(|p| p.state == DefState::Active && p.source != "bundled")
        .any(|p| built.entries.iter().any(|e| e.path == p.path && e.state == DefState::Invalid));
    if broke {
        return Refresh::KeptPrevious;
    }
    let diff = diff_entries(previous, &built.entries);
    registry.set_declarative(built.profiles);
    Refresh::Applied { entries: built.entries, diff }
}

/// Keeps the watcher and its debounce thread alive; dropping it stops both.
pub struct WatchHandle {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Watch `dirs` (non-recursive; missing ones are skipped) and call `on_change` after each quiet
/// period. `on_change` returns true when it kept the previous snapshot and wants a retry.
pub fn watch_dirs(
    dirs: Vec<PathBuf>,
    debounce: Duration,
    on_change: impl Fn() -> bool + Send + 'static,
) -> Result<WatchHandle, String> {
    let (tx, rx) = channel::<()>();
    let canon = |d: &Path| std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf());
    let roots: Vec<PathBuf> = dirs.iter().map(|d| canon(d)).collect();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        // Only events directly inside a watched directory count.
        let inside = event.paths.iter().any(|p| {
            p.parent().map(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf())).is_some_and(|parent| roots.contains(&parent))
        });
        if inside {
            let _ = tx.send(());
        }
    })
    .map_err(|e| e.to_string())?;
    let mut watched = 0;
    for dir in &dirs {
        if watcher.watch(dir, RecursiveMode::NonRecursive).is_ok() {
            watched += 1;
        }
    }
    if watched == 0 && !dirs.is_empty() {
        return Err("none of the definition directories could be watched".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("agent-defs-watch".into())
        .spawn(move || loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => {
                    if flag.load(Ordering::SeqCst) {
                        return;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let first = Instant::now();
            while first.elapsed() < MAX_WAIT {
                if rx.recv_timeout(debounce).is_err() {
                    break;
                }
            }
            if flag.load(Ordering::SeqCst) {
                return;
            }
            let mut retry = on_change();
            for _ in 0..RETRIES {
                if !retry || flag.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(RETRY_EVERY);
                while rx.try_recv().is_ok() {}
                retry = on_change();
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(WatchHandle { _watcher: watcher, stop })
}

// ---- the app's own watcher -------------------------------------------------------------------

static HANDLE: std::sync::Mutex<Option<WatchHandle>> = std::sync::Mutex::new(None);

/// Watch the enabled source dirs of this app and keep the process-wide registry current. Call
/// once at startup after the first `loader::reload()`. `notify_change` receives each applied
/// non-empty diff (the app broadcasts it as `agent_definitions_changed`).
pub fn start(notify_change: impl Fn(&Diff) + Send + 'static) -> Result<(), String> {
    let dir = crate::app_paths::agents_dir();
    let _ = std::fs::create_dir_all(&dir);
    let handle = watch_dirs(vec![dir], DEBOUNCE, move || {
        let registry = crate::tui::registry::registry();
        let previous = super::loader::last_entries();
        let code_ids = registry.code_ids();
        let user_dir = crate::app_paths::agents_dir();
        let inputs = LoadInputs { bundled: &[], user_dir: Some(&user_dir), code_ids: &code_ids };
        match refresh(registry, &inputs, &super::approvals::default_path(), &previous) {
            Refresh::KeptPrevious => {
                tracing::warn!("agent definition unreadable (half-written?); keeping the previous registry");
                true
            }
            Refresh::Applied { entries, diff } => {
                super::loader::store_last(entries);
                if !diff.is_empty() {
                    tracing::info!(?diff, "agent definitions reloaded");
                    notify_change(&diff);
                }
                false
            }
        }
    })?;
    *HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    Ok(())
}

#[cfg(test)]
#[path = "tests/watch.rs"]
mod tests;
