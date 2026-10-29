//! The private store under `<app data>/ci-fix/` (SPEC n33 section 3.2).
//!
//! - `settings.json`: the Labs settings (the switch and four numbers, always clamped).
//! - `watches.json`: watched pull requests, newest first. At most `WATCH_RECORDS_CAP` records and
//!   `WATCHES_CAP` that are `Watching`.
//! - `fixes.json`: fix tries, newest first, at most `FIX_RECORDS_CAP`. A finished fix keeps its
//!   `sha8` but its log lines are dropped.
//!
//! Every write goes through `secure_file::write_private` (atomic, owner-only on unix). A bad id or
//! sha is refused before any file is touched, so a hostile id never makes a file. A missing or
//! unreadable file reads as the defaults or as empty; nothing panics.

use super::model::{
    fix_active, valid_fix_id, valid_sha, valid_watch_id, FixRun, FixState, Settings, Watch, WatchState,
    FIX_RECORDS_CAP, WATCHES_CAP, WATCH_RECORDS_CAP,
};
use crate::issue_intake::model::valid_owner_repo;
use crate::secure_file::write_private;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The stop reason written when the active cap pushes an idle watch out.
pub const STOP_TOO_MANY: &str = "Too many pull requests watched";

/// One writer at a time in this process (read-modify-write of the JSON files).
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// The app's own store: `<app data>/ci-fix`.
    #[cfg(not(test))]
    pub fn live() -> Store {
        Store::at(crate::app_paths::app_data_dir().join("ci-fix"))
    }

    // Reason: only the engine tests read the root. Nothing in the app does yet.
    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The saved settings. Missing or unreadable file = the defaults (feature off).
    pub fn settings(&self) -> Settings {
        read_json::<Settings>(&self.settings_path()).unwrap_or_default().clamped()
    }

    /// Clamp and save the settings. Returns what was saved.
    pub fn save_settings(&self, settings: &Settings) -> Result<Settings, String> {
        let clean = settings.clamped();
        let _guard = lock();
        write_json(&self.settings_path(), &clean)?;
        Ok(clean)
    }

    /// All watches, newest first. Missing or unreadable file = none.
    pub fn watches(&self) -> Vec<Watch> {
        read_json::<Vec<Watch>>(&self.watches_path()).unwrap_or_default()
    }

    /// One watch by its id. A bad id finds nothing.
    pub fn watch_by_id(&self, id: &str) -> Option<Watch> {
        if !valid_watch_id(id) {
            return None;
        }
        self.watches().into_iter().find(|watch| watch.id == id)
    }

    /// The watch of one copy and its handle. A live watch (watching or paused) wins over an old one.
    pub fn watch_for(&self, project: &str, handle: &str) -> Option<Watch> {
        let found: Vec<Watch> =
            self.watches().into_iter().filter(|watch| watch.project == project && watch.handle == handle).collect();
        found.iter().find(|watch| is_live(&watch.state)).or(found.first()).cloned()
    }

    /// Insert or update one watch. An update keeps its place; a new watch goes first.
    ///
    /// Active cap: when a new `Watching` would make more than `WATCHES_CAP`, the oldest idle one
    /// (no fix in progress) becomes `Stopped`. If every active watch is busy the save is refused
    /// and nothing is written. Records past `WATCH_RECORDS_CAP` drop the oldest non-watching ones.
    pub fn put_watch(&self, watch: &Watch) -> Result<(), String> {
        if !valid_watch_id(&watch.id) {
            return Err("bad watch id".to_string());
        }
        if !valid_owner_repo(&watch.owner_repo) {
            return Err("bad repository name".to_string());
        }
        if !watch.pushed_shas.iter().all(|sha| valid_sha(sha)) {
            return Err("bad commit id".to_string());
        }
        let _guard = lock();
        let mut watches = self.watches();
        match watches.iter().position(|existing| existing.id == watch.id) {
            Some(index) => watches[index] = watch.clone(),
            None => watches.insert(0, watch.clone()),
        }
        if watches.iter().filter(|item| is_watching(&item.state)).count() > WATCHES_CAP {
            let fixes = self.fixes();
            let idle = |item: &Watch| {
                item.id != watch.id
                    && is_watching(&item.state)
                    && !fixes.iter().any(|fix| fix.watch_id == item.id && fix_active(&fix.state))
            };
            let oldest_idle = watches
                .iter()
                .filter(|item| idle(item))
                .min_by_key(|item| item.added_at_ms)
                .map(|item| item.id.clone());
            let Some(stop_id) = oldest_idle else {
                return Err("Too many pull requests are being fixed right now. Stop one first.".to_string());
            };
            if let Some(item) = watches.iter_mut().find(|item| item.id == stop_id) {
                item.state = WatchState::Stopped { why: STOP_TOO_MANY.to_string() };
            }
        }
        trim(&mut watches, WATCH_RECORDS_CAP, |item| is_watching(&item.state));
        write_json(&self.watches_path(), &watches)
    }

    /// All fix tries, newest first. Missing or unreadable file = none.
    pub fn fixes(&self) -> Vec<FixRun> {
        read_json::<Vec<FixRun>>(&self.fixes_path()).unwrap_or_default()
    }

    /// One fix by its id. A bad id finds nothing.
    pub fn fix_by_id(&self, id: &str) -> Option<FixRun> {
        if !valid_fix_id(id) {
            return None;
        }
        self.fixes().into_iter().find(|fix| fix.id == id)
    }

    /// The fix waiting on this card (`Asking` or `AskPush`). Only ids stored in a record match.
    pub fn fix_by_question(&self, question_id: &str) -> Option<FixRun> {
        if question_id.is_empty() {
            return None;
        }
        self.fixes().into_iter().find(|fix| match &fix.state {
            FixState::Asking { question_id: stored } | FixState::AskPush { question_id: stored, .. } => {
                stored == question_id
            }
            _ => false,
        })
    }

    /// Insert or update one fix. An update keeps its place; a new fix goes first. Past
    /// `FIX_RECORDS_CAP` the oldest finished fix is dropped first. A finished fix is stored with
    /// its log lines emptied (its `sha8` and notes stay).
    pub fn put_fix(&self, fix: &FixRun) -> Result<(), String> {
        if !valid_fix_id(&fix.id) {
            return Err("bad fix id".to_string());
        }
        if !valid_watch_id(&fix.watch_id) {
            return Err("bad watch id".to_string());
        }
        if !valid_sha(&fix.failing_sha) {
            return Err("bad commit id".to_string());
        }
        let mut clean = fix.clone();
        if !fix_active(&clean.state) {
            clean.log.jobs.clear();
        }
        let _guard = lock();
        let mut fixes = self.fixes();
        match fixes.iter().position(|existing| existing.id == clean.id) {
            Some(index) => fixes[index] = clean,
            None => fixes.insert(0, clean),
        }
        trim(&mut fixes, FIX_RECORDS_CAP, |item| fix_active(&item.state));
        write_json(&self.fixes_path(), &fixes)
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn watches_path(&self) -> PathBuf {
        self.root.join("watches.json")
    }

    fn fixes_path(&self) -> PathBuf {
        self.root.join("fixes.json")
    }
}

/// A new watch id: `cw-` + 8 lowercase hex characters.
pub fn new_watch_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("cw-{}", &hex[..8])
}

/// A new fix id: `cf-` + 12 lowercase hex characters.
pub fn new_fix_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("cf-{}", &hex[..12])
}

fn is_watching(state: &WatchState) -> bool {
    matches!(state, WatchState::Watching)
}

fn is_live(state: &WatchState) -> bool {
    matches!(state, WatchState::Watching | WatchState::Paused { .. })
}

/// Drop from the end (the oldest records) until `cap` is met. A record that is still active is
/// kept while any other record can go instead.
fn trim<T>(items: &mut Vec<T>, cap: usize, active: impl Fn(&T) -> bool) {
    while items.len() > cap {
        let drop_at = items.iter().rposition(|item| !active(item)).unwrap_or(items.len() - 1);
        items.remove(drop_at);
    }
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    write_private(path, &text)
}
