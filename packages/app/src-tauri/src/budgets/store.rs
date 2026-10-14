//! Budget records, one small JSON file per project (`<project>/.puppet-master/budgets.json`),
//! cached in memory and written through on every change.

use super::model::BudgetRecord;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

type Records = BTreeMap<String, BudgetRecord>;

static CACHE: Lazy<Mutex<HashMap<String, Records>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn key(project: &str) -> String {
    project.trim().replace('\\', "/").trim_end_matches('/').to_ascii_lowercase()
}

fn file_for(project: &str) -> PathBuf {
    Path::new(project).join(".puppet-master").join("budgets.json")
}

fn load(project: &str) -> Records {
    std::fs::read_to_string(file_for(project))
        .ok()
        .and_then(|text| serde_json::from_str::<Records>(&text).ok())
        .unwrap_or_default()
}

fn save(project: &str, records: &Records) {
    let path = file_for(project);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(text) = serde_json::to_string_pretty(records) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Runs `f` on the project's records (loaded on first use). `f` returns the value and whether
/// it changed anything; changes are written through.
fn with<R>(project: &str, f: impl FnOnce(&mut Records) -> (R, bool)) -> R {
    let mut cache = CACHE.lock();
    let records = cache.entry(key(project)).or_insert_with(|| load(project));
    let (value, changed) = f(records);
    if changed {
        save(project, records);
    }
    value
}

pub fn get(project: &str, handle: &str) -> Option<BudgetRecord> {
    with(project, |records| (records.get(handle).cloned(), false))
}

pub fn all(project: &str) -> Vec<BudgetRecord> {
    with(project, |records| (records.values().cloned().collect(), false))
}

/// The runs that draw from `handle`'s budget, with the project each one lives in. Scans every
/// project loaded in this process: a helper in another project than its parent is only seen
/// once that project has been touched since the app started.
pub fn children_of(project: &str, handle: &str) -> Vec<(String, BudgetRecord)> {
    let wanted = key(project);
    let cache = CACHE.lock();
    let mut found = Vec::new();
    for (own_key, records) in cache.iter() {
        for record in records.values() {
            if record.parent.as_deref() != Some(handle) {
                continue;
            }
            let parent_key = record.parent_project.as_deref().map(key).unwrap_or_else(|| own_key.clone());
            if parent_key == wanted {
                found.push((record.project.clone(), record.clone()));
            }
        }
    }
    found
}

pub fn put(project: &str, record: BudgetRecord) {
    with(project, |records| {
        records.insert(record.handle.clone(), record);
        ((), true)
    })
}

pub fn remove(project: &str, handle: &str) {
    with(project, |records| {
        let removed = records.remove(handle).is_some();
        ((), removed)
    })
}

/// Changes one record in place. `None` when the run has no budget.
pub fn update<R>(project: &str, handle: &str, f: impl FnOnce(&mut BudgetRecord) -> R) -> Option<R> {
    with(project, |records| match records.get_mut(handle) {
        Some(record) => (Some(f(record)), true),
        None => (None, false),
    })
}

/// Test hook: forget what is cached for a project so the next read comes from disk.
#[cfg(test)]
pub fn forget_cache(project: &str) {
    CACHE.lock().remove(&key(project));
}
