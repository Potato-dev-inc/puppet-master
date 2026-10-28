//! Branch from here: the private store under `<app data>/branches/` (SPEC n33 section 3.2).
//!
//! - `records.json`: branch records, newest first, at most `RECORDS_CAP`.
//! - `briefs/<id>.md`: the brief the new agent got. Private: never sent to the bridge, push, team
//!   or phone (S3).
//! - `hooks/`: an empty folder, used as the hooks path for PM's own git calls (S8).
//!
//! Every write goes through `secure_file::write_private` (atomic, owner-only on unix). A file name
//! only ever comes from a validated record id (`id_ok`), never from user text (S10). A bad or
//! missing file reads as empty; nothing panics.

use super::model::{Branch, RECORDS_CAP};
use crate::secure_file::{create_private_dir, write_private};
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

    /// The app's own store: `<app data>/branches`.
    #[cfg(not(test))]
    pub fn live() -> Store {
        Store::at(crate::app_paths::app_data_dir().join("branches"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All records, newest first. Missing or unreadable file = none.
    pub fn records(&self) -> Vec<Branch> {
        read_json::<Vec<Branch>>(&self.records_path()).unwrap_or_default()
    }

    pub fn by_id(&self, id: &str) -> Option<Branch> {
        if !id_ok(id) {
            return None;
        }
        self.records().into_iter().find(|record| record.id == id)
    }

    /// The newest branch made from `pane_id` (the source side).
    pub fn from_pane(&self, pane_id: &str) -> Option<Branch> {
        self.records().into_iter().find(|record| record.from.pane_id == pane_id)
    }

    /// The newest branch whose new pane is `pane_id`.
    pub fn to_pane(&self, pane_id: &str) -> Option<Branch> {
        self.records().into_iter().find(|record| record.to.pane_id == pane_id)
    }

    /// Insert or update one record. An update keeps its place. A new record goes first, and the
    /// oldest ones are dropped past `RECORDS_CAP`. A bad id writes nothing.
    pub fn put(&self, record: &Branch) -> Result<(), String> {
        if !id_ok(&record.id) {
            return Err("bad branch id".to_string());
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

    /// Write a brief for branch `id`. Returns the absolute file path. A bad id writes nothing.
    pub fn write_brief(&self, id: &str, text: &str) -> io::Result<String> {
        let path = self.brief_path(id)?;
        write_private(&path, text).map_err(io::Error::other)?;
        Ok(path.display().to_string())
    }

    /// Read the brief for branch `id`. A bad id reads nothing.
    pub fn read_brief(&self, id: &str) -> io::Result<String> {
        fs::read_to_string(self.brief_path(id)?)
    }

    /// The empty hooks folder for PM's own git calls (`harden::git_env`). Created if missing.
    pub fn empty_hooks_dir(&self) -> Result<PathBuf, String> {
        let dir = self.root.join("hooks");
        create_private_dir(&dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
        Ok(dir)
    }

    fn records_path(&self) -> PathBuf {
        self.root.join("records.json")
    }

    fn brief_path(&self, id: &str) -> io::Result<PathBuf> {
        if !id_ok(id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad branch id"));
        }
        Ok(self.root.join("briefs").join(format!("{id}.md")))
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
        write_private(path, &text)
    }
}

/// `bf-` + 12 lowercase hex characters. The only text that ever becomes a file name.
pub fn id_ok(id: &str) -> bool {
    match id.strip_prefix("bf-") {
        Some(hex) => hex.len() == 12 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}

/// A new branch id: `bf-` + 12 lowercase hex characters.
pub fn new_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("bf-{}", &hex[..12])
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}
