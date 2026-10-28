//! Night Shift's private files, under `<app data>/night-shift/` (SPEC n32 section 3.2):
//!
//! - `run.json`: the current or last run.
//! - `history/<run id>.json`: finished runs. Puppet Master never deletes these. `history()` lists the newest 14.
//! - `settings.json`: the saved Tonight settings.
//! - `projects.json`: declared checks per project. The value is raw JSON here. `checks.rs` types it.
//!
//! Every write goes through `secure_file::write_private` (atomic, owner-only on unix). A file name only
//! ever comes from a validated id (S12). A corrupt file reads as nothing and is left in place, so
//! nothing panics and nothing is lost on a read.

use super::model::{validate_ids, valid_run_id, NightRun, Settings, HISTORY_CAP};
use crate::secure_file::write_private;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One writer at a time in this process (read-modify-write of `projects.json` and of the run revision).
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// One row of the history list: a finished run, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: String,
    pub finished_ms: u64,
    pub line: String,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// The app's own store: `<app data>/night-shift`.
    #[cfg(not(test))]
    pub fn live() -> Store {
        Store::at(crate::app_paths::app_data_dir().join("night-shift"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The current or last run. Missing, unreadable, or failing the id check = none. The file stays.
    pub fn load(&self) -> Option<NightRun> {
        let run: NightRun = read_json(&self.run_path())?;
        validate_ids(&run).ok()?;
        Some(run)
    }

    /// Save the current run and bump its `rev` by one. A run with a bad id writes nothing.
    pub fn save(&self, run: &mut NightRun) -> Result<(), String> {
        validate_ids(run)?;
        let _guard = lock();
        run.rev += 1;
        let written = self.write_json(&self.run_path(), &*run);
        if written.is_err() {
            run.rev -= 1;
        }
        written
    }

    /// One finished run from `history/`, by id. A bad id, a missing or unreadable file, or a file whose
    /// own id differs = none. Only ids that pass `valid_run_id` become a file name (S12).
    pub fn get(&self, id: &str) -> Option<NightRun> {
        if !valid_run_id(id) {
            return None;
        }
        let run: NightRun = read_json(&self.history_path(id))?;
        (run.id == id && validate_ids(&run).is_ok()).then_some(run)
    }

    /// Copy a finished run to `history/<id>.json`. Older files are never deleted.
    pub fn archive(&self, run: &NightRun) -> Result<(), String> {
        validate_ids(run)?;
        self.write_json(&self.history_path(&run.id), run)
    }

    /// The newest 14 finished runs, newest first. Files that do not parse are skipped.
    pub fn history(&self) -> Vec<HistoryRow> {
        let Ok(entries) = fs::read_dir(self.root.join("history")) else {
            return Vec::new();
        };
        let mut runs: Vec<NightRun> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(".json")?;
                if !valid_run_id(id) {
                    return None;
                }
                read_json::<NightRun>(&entry.path()).filter(|run| run.id == id && validate_ids(run).is_ok())
            })
            .collect();
        runs.sort_by(|a, b| finished_at(b).cmp(&finished_at(a)).then_with(|| b.id.cmp(&a.id)));
        runs.truncate(HISTORY_CAP);
        runs.iter()
            .map(|run| {
                let counts = run.summary_counts();
                HistoryRow {
                    id: run.id.clone(),
                    finished_ms: finished_at(run),
                    line: format!("{} of {} done", counts.done, counts.total),
                }
            })
            .collect()
    }

    /// The saved Tonight settings, clamped. Missing or unreadable = the defaults.
    pub fn settings(&self) -> Settings {
        read_json::<Settings>(&self.settings_path()).map(|saved| saved.clamped()).unwrap_or_default()
    }

    /// Save the settings after clamping them. Returns what was saved.
    pub fn save_settings(&self, settings: &Settings) -> Result<Settings, String> {
        let clean = settings.clamped();
        let _guard = lock();
        self.write_json(&self.settings_path(), &clean)?;
        Ok(clean)
    }

    /// The raw declared-checks value for one project (typed by `checks.rs`), if any.
    pub fn declared_raw(&self, project: &str) -> Option<serde_json::Value> {
        self.read_projects().ok()?.remove(project)
    }

    /// Set the raw declared-checks value for one project. Other projects are kept.
    pub fn put_declared_raw(&self, project: &str, value: serde_json::Value) -> Result<(), String> {
        if project.trim().is_empty() {
            return Err("no project to declare checks for".to_string());
        }
        let _guard = lock();
        let mut all = self.read_projects()?;
        all.insert(project.to_string(), value);
        self.write_json(&self.projects_path(), &all)
    }

    fn run_path(&self) -> PathBuf {
        self.root.join("run.json")
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn projects_path(&self) -> PathBuf {
        self.root.join("projects.json")
    }

    /// Only called with an id that passed `validate_ids`.
    fn history_path(&self, id: &str) -> PathBuf {
        self.root.join("history").join(format!("{id}.json"))
    }

    /// A missing file is empty. A file that does not parse is an error, so a save never wipes other
    /// projects' declared checks.
    fn read_projects(&self) -> Result<BTreeMap<String, serde_json::Value>, String> {
        match fs::read_to_string(self.projects_path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|_| "projects.json does not parse; left in place".to_string()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(err) => Err(err.to_string()),
        }
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
        write_private(path, &text)
    }
}

/// When a run ended, for the history order. A run that never got a finish time uses its creation time.
fn finished_at(run: &NightRun) -> u64 {
    run.finished_ms.unwrap_or(run.created_ms)
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
    use super::super::model::{sample_run, Class, Outcome, RunState, Settings, HISTORY_CAP};
    use super::*;
    use std::fs;

    /// A temp folder for one test, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            Scratch(std::env::temp_dir().join(format!("pm-ns-store-{}", uuid::Uuid::new_v4().simple())))
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

    fn finished_run(index: u64, finished_ms: u64) -> NightRun {
        let mut run = sample_run(&NightRun::new_id());
        run.state = RunState::Done;
        run.finished_ms = Some(finished_ms);
        run.created_ms = index;
        run
    }

    #[test]
    fn save_then_load_round_trips_and_bumps_rev() {
        let scratch = Scratch::new();
        let store = scratch.store();
        assert!(store.load().is_none());

        let mut run = sample_run(&NightRun::new_id());
        store.save(&mut run).expect("first save");
        assert_eq!(run.rev, 1);
        let back = store.load().expect("loads");
        assert_eq!(back, run);

        store.save(&mut run).expect("second save");
        assert_eq!(run.rev, 2);
        assert_eq!(store.load().expect("loads").rev, 2);
    }

    #[test]
    fn a_bad_id_writes_no_file() {
        let scratch = Scratch::new();
        let store = scratch.store();

        let mut run = sample_run(&NightRun::new_id());
        run.id = "ns-../../etc".to_string();
        assert!(store.save(&mut run).is_err());
        assert_eq!(run.rev, 0, "a refused save does not bump the revision");
        assert!(!scratch.0.join("run.json").exists());

        let mut bad_task = sample_run(&NightRun::new_id());
        bad_task.tasks[0].spec.id = "nt-XYZ".to_string();
        assert!(store.archive(&bad_task).is_err());
        assert!(!scratch.0.join("history").exists());
        assert!(store.save(&mut bad_task).is_err());
        assert!(!scratch.0.join("run.json").exists());
    }

    #[test]
    fn a_corrupt_run_file_reads_as_none_and_is_left_in_place() {
        let scratch = Scratch::new();
        let store = scratch.store();
        fs::create_dir_all(&scratch.0).expect("dir");
        fs::write(scratch.0.join("run.json"), "{ not json").expect("write");
        assert!(store.load().is_none());
        assert_eq!(fs::read_to_string(scratch.0.join("run.json")).expect("still there"), "{ not json");
    }

    #[test]
    fn history_lists_the_newest_fourteen_first_and_keeps_every_file() {
        let scratch = Scratch::new();
        let store = scratch.store();
        for index in 0..16u64 {
            store.archive(&finished_run(index, 1_000 + index)).expect("archive");
        }
        let rows = store.history();
        assert_eq!(rows.len(), HISTORY_CAP);
        assert_eq!(rows[0].finished_ms, 1_015, "newest first");
        assert_eq!(rows[HISTORY_CAP - 1].finished_ms, 1_002);
        assert!(rows.windows(2).all(|pair| pair[0].finished_ms > pair[1].finished_ms));
        assert_eq!(rows[0].line, "0 of 1 done");
        let files = fs::read_dir(scratch.0.join("history")).expect("history dir").count();
        assert_eq!(files, 16, "old files are never deleted, only left off the list");
    }

    #[test]
    fn get_reads_one_archived_run_and_refuses_a_bad_id_or_a_mismatch() {
        let scratch = Scratch::new();
        let store = scratch.store();
        let run = finished_run(3, 4_000);
        store.archive(&run).expect("archive");
        let back = store.get(&run.id).expect("the archived run");
        assert_eq!(back.id, run.id);
        assert_eq!(back.finished_ms, Some(4_000));
        assert!(store.get("ns-00000000000g").is_none(), "not a run id");
        assert!(store.get("../run").is_none(), "no path from an id");
        assert!(store.get(&NightRun::new_id()).is_none(), "no such file");
        // A file renamed to another run's id is not that run.
        let other = finished_run(4, 5_000);
        fs::write(
            scratch.0.join("history").join(format!("{}.json", other.id)),
            serde_json::to_string(&run).expect("json"),
        )
        .expect("write");
        assert!(store.get(&other.id).is_none(), "the file's own id must match");
    }

    #[test]
    fn history_skips_files_that_are_not_runs() {
        let scratch = Scratch::new();
        let store = scratch.store();
        store.archive(&finished_run(1, 5)).expect("archive");
        fs::write(scratch.0.join("history").join("notes.json"), "{}").expect("write");
        fs::write(scratch.0.join("history").join("ns-000000000000.json"), "garbage").expect("write");
        assert_eq!(store.history().len(), 1);
    }

    #[test]
    fn settings_are_clamped_on_save_and_on_read() {
        let scratch = Scratch::new();
        let store = scratch.store();
        assert_eq!(store.settings(), Settings::default());

        let saved = store
            .save_settings(&Settings { parallel: 9, turns: 99, minutes: 1, stop_hhmm: "99:99".into(), ..Settings::default() })
            .expect("save");
        assert_eq!((saved.parallel, saved.turns, saved.minutes), (2, 40, 20));
        assert_eq!(saved.stop_hhmm, "07:00");
        assert_eq!(store.settings(), saved);

        fs::write(scratch.0.join("settings.json"), r#"{"turns":0,"parallel":0}"#).expect("write");
        let read = store.settings();
        assert_eq!((read.turns, read.parallel), (3, 1));
        assert_eq!(read.stop_hhmm, "07:00", "missing fields read as the defaults");
    }

    #[test]
    fn declared_raw_keeps_other_projects_and_refuses_a_corrupt_file() {
        let scratch = Scratch::new();
        let store = scratch.store();
        assert!(store.declared_raw("/work/app").is_none());
        store.put_declared_raw("/work/app", serde_json::json!([["npm", "test"]])).expect("put");
        store.put_declared_raw("/work/site", serde_json::json!([])).expect("put");
        assert_eq!(store.declared_raw("/work/app"), Some(serde_json::json!([["npm", "test"]])));
        assert!(store.put_declared_raw("  ", serde_json::json!([])).is_err());

        fs::write(scratch.0.join("projects.json"), "{ broken").expect("write");
        assert!(store.put_declared_raw("/work/app", serde_json::json!([])).is_err());
        assert_eq!(fs::read_to_string(scratch.0.join("projects.json")).expect("kept"), "{ broken");
    }

    #[test]
    fn archived_runs_keep_their_outcome_classes() {
        let scratch = Scratch::new();
        let store = scratch.store();
        let mut run = finished_run(1, 9);
        run.tasks[0].outcome = Some(Outcome {
            class: Class::NeedsLook,
            headline: String::new(),
            reason: None,
            tests: None,
            tests_before: None,
            checks_declared: false,
            test_change_lines: Vec::new(),
            files: None,
            turns: 0,
            minutes: 0,
            summaries: Vec::new(),
            copy: None,
        });
        store.archive(&run).expect("archive");
        let rows = store.history();
        assert_eq!(rows[0].line, "0 of 1 done");
        let back: NightRun = read_json(&scratch.0.join("history").join(format!("{}.json", run.id))).expect("reads");
        assert_eq!(back, run);
    }
}
