//! Keep going: the private store under `<app data>/keep-going/` (SPEC n32 section 3.2).
//!
//! - `records.json`: hand-over records, newest first, at most `RECORDS_CAP`.
//! - `settings.json`: the Keep going settings (order and come-back switch).
//! - `briefs/<id>.md`: the brief the new agent got. Private: never sent to the bridge, push, team
//!   or phone (S3).
//!
//! Every write goes through `secure_file::write_private` (atomic, owner-only on unix). A file name
//! only ever comes from a validated record id (`valid_id`), never from user text (S11). A bad or
//! missing file reads as empty; nothing panics.

use super::model::{valid_id, Handover, Settings, Target, RECORDS_CAP, SETTINGS_ORDER_CAP};
use crate::secure_file::write_private;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One writer at a time in this process (read-modify-write of `records.json`).
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// The app's own store: `<app data>/keep-going`.
    #[cfg(not(test))]
    pub fn live() -> Store {
        Store::at(crate::app_paths::app_data_dir().join("keep-going"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All records, newest first. Missing or unreadable file = none.
    pub fn records(&self) -> Vec<Handover> {
        read_json::<Vec<Handover>>(&self.records_path()).unwrap_or_default()
    }

    pub fn by_id(&self, id: &str) -> Option<Handover> {
        if !valid_id(id) {
            return None;
        }
        self.records().into_iter().find(|record| record.id == id)
    }

    /// The newest record where `pane_id` is the OLD side.
    pub fn from_pane(&self, pane_id: &str) -> Option<Handover> {
        self.records().into_iter().find(|record| record.from.pane_id == pane_id)
    }

    /// The newest record where `pane_id` is the NEW side.
    pub fn to_pane(&self, pane_id: &str) -> Option<Handover> {
        self.records().into_iter().find(|record| record.to.pane_id == pane_id)
    }

    /// Insert or update one record. An update keeps its place. A new record goes first, and the
    /// oldest ones are dropped past `RECORDS_CAP`.
    pub fn put(&self, record: &Handover) -> Result<(), String> {
        if !valid_id(&record.id) {
            return Err("bad keep-going record id".to_string());
        }
        let _guard = lock();
        let mut records = self.records();
        match records.iter().position(|existing| existing.id == record.id) {
            Some(index) => records[index] = record.clone(),
            None => records.insert(0, record.clone()),
        }
        records.truncate(RECORDS_CAP);
        self.write_json(&self.records_path(), &records)
    }

    pub fn settings(&self) -> Settings {
        match read_json::<Settings>(&self.settings_path()) {
            Some(settings) => normalize(&settings),
            None => Settings::default(),
        }
    }

    /// Save the settings. Targets that do not parse are dropped, duplicates keep their first
    /// place, and the order is capped at `SETTINGS_ORDER_CAP`. Returns what was saved.
    pub fn save_settings(&self, settings: &Settings) -> Result<Settings, String> {
        let clean = normalize(settings);
        let _guard = lock();
        self.write_json(&self.settings_path(), &clean)?;
        Ok(clean)
    }

    /// Write a brief for record `id`. Returns the absolute file path. A bad id writes nothing.
    pub fn write_brief(&self, id: &str, text: &str) -> io::Result<String> {
        let path = self.brief_path(id)?;
        write_private(&path, text).map_err(io::Error::other)?;
        Ok(path.display().to_string())
    }

    /// Read the brief for record `id`. A bad id reads nothing.
    pub fn read_brief(&self, id: &str) -> io::Result<String> {
        fs::read_to_string(self.brief_path(id)?)
    }

    fn records_path(&self) -> PathBuf {
        self.root.join("records.json")
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn brief_path(&self, id: &str) -> io::Result<PathBuf> {
        if !valid_id(id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad keep-going id"));
        }
        Ok(self.root.join("briefs").join(format!("{id}.md")))
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
        write_private(path, &text)
    }
}

/// A new record id: `kg-` + 12 lowercase hex characters.
pub fn new_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("kg-{}", &hex[..12])
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Keep only targets that still parse, drop repeats (first place wins), cap the order.
fn normalize(settings: &Settings) -> Settings {
    let mut order: Vec<Target> = Vec::new();
    for target in &settings.order {
        let valid = Target::parse(&target.key()).is_some_and(|parsed| parsed == *target);
        if valid && !order.contains(target) && order.len() < SETTINGS_ORDER_CAP {
            order.push(target.clone());
        }
    }
    Settings { order, ask_back: settings.ask_back }
}
