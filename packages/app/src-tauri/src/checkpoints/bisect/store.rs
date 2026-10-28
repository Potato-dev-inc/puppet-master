//! What broke it's private files, under `<app data>/what-broke-it/` (SPEC n33 section 3.2):
//!
//! - `findings.json`: saved results, newest first, at most `FINDINGS_CAP`.
//! - `settings.json`: the saved budget (`Limits`).
//! - `copies/`: throw-away test copies (made and removed by `copy.rs`).
//!
//! Every write goes through `secure_file::write_private` (atomic, owner-only on unix). A file that does
//! not parse reads as nothing. A save never overwrites a file that does not parse: it fails and the file
//! stays in place, so an unreadable file is never silently replaced. Ids come only from `new_id`; any id
//! from outside is checked with `valid_id` before it can name a file (S11).

use super::model::{Finding, Limits, FINDINGS_CAP};
use crate::secure_file::write_private;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One writer at a time in this process (read-modify-write of `findings.json`).
static WRITE_LOCK: Mutex<()> = Mutex::new(());

const FINDINGS_FILE: &str = "findings.json";
const SETTINGS_FILE: &str = "settings.json";
const COPIES_DIR: &str = "copies";
const ID_PREFIX: &str = "wb-";
const ID_HEX_LEN: usize = 12;

/// `wb-` + 12 lowercase hex characters. The only text that may name a finding.
pub fn valid_id(id: &str) -> bool {
    match id.strip_prefix(ID_PREFIX) {
        Some(hex) => hex.len() == ID_HEX_LEN && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}

/// A new finding id: `wb-` + 12 random lowercase hex characters.
pub fn new_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("{ID_PREFIX}{}", &hex[..ID_HEX_LEN])
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// The app's own store: `<app data>/what-broke-it`.
    #[cfg(not(test))]
    pub fn live() -> Store {
        Store::at(crate::app_paths::app_data_dir().join("what-broke-it"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder that holds the throw-away test copies.
    pub fn copies_root(&self) -> PathBuf {
        self.root.join(COPIES_DIR)
    }

    /// Saved findings, newest first. A missing or unreadable file = none. The file stays.
    pub fn findings(&self) -> Vec<Finding> {
        read_json::<Vec<Finding>>(&self.findings_path()).unwrap_or_default()
    }

    /// One saved finding. A bad id is `None` before any file is read.
    pub fn get(&self, id: &str) -> Option<Finding> {
        if !valid_id(id) {
            return None;
        }
        self.findings().into_iter().find(|finding| finding.id == id)
    }

    /// Save a finding. A new id goes to the front; a known id is replaced in place. The list is capped at
    /// `FINDINGS_CAP` (the oldest go). A file that does not parse is left in place and nothing is written.
    pub fn put(&self, finding: &Finding) -> Result<(), String> {
        if !valid_id(&finding.id) {
            return Err("That result id is not valid.".to_string());
        }
        let _guard = lock();
        let path = self.findings_path();
        let mut all = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Vec<Finding>>(&text)
                .map_err(|_| "findings.json does not parse; left in place".to_string())?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err.to_string()),
        };
        match all.iter().position(|saved| saved.id == finding.id) {
            Some(index) => all[index] = finding.clone(),
            None => all.insert(0, finding.clone()),
        }
        all.truncate(FINDINGS_CAP);
        self.write_json(&path, &all)
    }

    /// The saved budget, clamped. Missing or unreadable = the defaults.
    pub fn settings(&self) -> Limits {
        read_json::<Limits>(&self.settings_path())
            .map(|saved| Limits::clamped(saved.max_runs, saved.max_minutes))
            .unwrap_or_default()
    }

    /// Save the budget after clamping it. Returns what was saved.
    pub fn save_settings(&self, limits: Limits) -> Result<Limits, String> {
        let clean = Limits::clamped(limits.max_runs, limits.max_minutes);
        let _guard = lock();
        self.write_json(&self.settings_path(), &clean)?;
        Ok(clean)
    }

    fn findings_path(&self) -> PathBuf {
        self.root.join(FINDINGS_FILE)
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE)
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
        write_private(path, &text)
    }
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::super::model::{Outcome, Probe, Verdict};
    use super::*;

    /// A temp folder for one test, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            Scratch(std::env::temp_dir().join(format!("pm-wb-store-{}", uuid::Uuid::new_v4().simple())))
        }

        fn store(&self) -> Store {
            Store::at(self.0.clone())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn finding(id: &str, started_ms: u64) -> Finding {
        Finding {
            id: id.to_string(),
            project: "C:/you/app".into(),
            folder: "C:/you/app".into(),
            pane_key: "p1".into(),
            started_ms,
            finished_ms: started_ms + 1,
            check_name: "npm test".into(),
            check_argv: vec!["npm".into(), "test".into()],
            runs: 3,
            minutes_x10: 5,
            points: 4,
            outcome: Outcome::TestsPass,
            tail: String::new(),
            probes: vec![Probe { tree: "t".into(), verdict: Verdict::Pass }],
            undone_ms: None,
            good_idx: None,
            bad_idx: None,
        }
    }

    #[test]
    fn valid_ids_are_wb_plus_twelve_lowercase_hex() {
        assert!(valid_id("wb-0123456789ab"));
        assert!(valid_id("wb-ffffffffffff"));
        assert!(!valid_id(""));
        assert!(!valid_id("../x"));
        assert!(!valid_id("wb-../../x.."));
        assert!(!valid_id("WB-0123456789ab"));
        assert!(!valid_id("wb-0123456789AB"));
        assert!(!valid_id("wb-0123456789a"));
        assert!(!valid_id("wb-0123456789abc"));
        assert!(!valid_id("wb-0123456789ag"));
        assert!(!valid_id("ns-0123456789ab"));
        assert!(!valid_id("wb-0123456789ab\n"));
    }

    #[test]
    fn new_ids_are_valid_and_differ() {
        let a = new_id();
        let b = new_id();
        assert!(valid_id(&a), "{a}");
        assert!(valid_id(&b), "{b}");
        assert_ne!(a, b);
    }

    #[test]
    fn get_refuses_a_bad_id_before_any_lookup() {
        let scratch = Scratch::new();
        let store = scratch.store();
        assert!(store.get("../x").is_none());
        assert!(store.get("").is_none());
        assert!(store.get("wb-0123456789AB").is_none());
    }

    #[test]
    fn a_wrong_shaped_findings_file_reads_as_empty() {
        let scratch = Scratch::new();
        let store = scratch.store();
        fs::create_dir_all(store.root()).unwrap();
        for text in ["{\"not\": \"a list\"}", "\"text\"", "[1, 2]", "not json at all", ""] {
            fs::write(store.root().join(FINDINGS_FILE), text).unwrap();
            assert!(store.findings().is_empty(), "{text}");
            assert!(store.get("wb-0123456789ab").is_none());
        }
    }

    #[test]
    fn put_keeps_the_newest_first_and_drops_the_oldest_past_the_cap() {
        let scratch = Scratch::new();
        let store = scratch.store();
        for n in 0..(FINDINGS_CAP as u64 + 1) {
            let id = format!("wb-{:012x}", n + 1);
            store.put(&finding(&id, n)).unwrap();
        }
        let all = store.findings();
        assert_eq!(all.len(), FINDINGS_CAP);
        assert_eq!(all[0].id, format!("wb-{:012x}", FINDINGS_CAP as u64 + 1), "newest first");
        assert!(store.get(&format!("wb-{:012x}", 1)).is_none(), "the oldest is dropped");
        assert!(store.get(&format!("wb-{:012x}", 2)).is_some());
    }

    #[test]
    fn put_replaces_a_known_id_in_place() {
        let scratch = Scratch::new();
        let store = scratch.store();
        store.put(&finding("wb-000000000001", 1)).unwrap();
        store.put(&finding("wb-000000000002", 2)).unwrap();
        let mut changed = finding("wb-000000000001", 1);
        changed.undone_ms = Some(99);
        store.put(&changed).unwrap();
        let all = store.findings();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].id, "wb-000000000001", "the position is kept");
        assert_eq!(all[1].undone_ms, Some(99));
    }

    #[test]
    fn put_refuses_a_bad_id_and_a_file_that_does_not_parse() {
        let scratch = Scratch::new();
        let store = scratch.store();
        assert!(store.put(&finding("../x", 1)).is_err());
        fs::create_dir_all(store.root()).unwrap();
        fs::write(store.root().join(FINDINGS_FILE), "{broken").unwrap();
        assert!(store.put(&finding("wb-000000000001", 1)).is_err());
        let kept = fs::read_to_string(store.root().join(FINDINGS_FILE)).unwrap();
        assert_eq!(kept, "{broken", "the unreadable file is left in place");
    }

    #[test]
    fn settings_default_then_clamp_on_save_and_read() {
        let scratch = Scratch::new();
        let store = scratch.store();
        assert_eq!(store.settings(), Limits::default());
        let saved = store.save_settings(Limits { max_runs: 0, max_minutes: 999 }).unwrap();
        assert_eq!(saved, Limits { max_runs: 1, max_minutes: 60 });
        assert_eq!(store.settings(), saved);
    }

    #[test]
    fn copies_live_under_the_store_root() {
        let store = Store::at("C:/app-data/what-broke-it");
        assert_eq!(store.copies_root(), PathBuf::from("C:/app-data/what-broke-it").join("copies"));
    }
}
