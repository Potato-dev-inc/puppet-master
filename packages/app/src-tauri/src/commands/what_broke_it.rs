//! Tauri commands of What broke it (SPEC n33 section 3.6). Each one is a thin wrapper over
//! `checkpoints::bisect::live::Live`: it checks the ids it is given, runs the call on the blocking pool and
//! sends errors to the window as `CODE: message`, the shape the other commands use.
//!
//! Desktop window only: no bridge route, no MCP tool, no companion or phone path (S2). Nothing here writes
//! to a pane (S14). The app's registry is reached only through `app_live`, which tests never call.

use crate::checkpoints::bisect::live::{Live, WbError, WB_BAD_ID, WB_INTERNAL};
use crate::checkpoints::bisect::store;
use crate::checkpoints::bisect::views::{
    DeclaredView, DiffView, FindingView, JobView, LimitsView, Overview, UndoPlanView, UndoReportView,
};
use crate::checkpoints::bisect::words;

/// `CODE: message`, the shape every command sends its errors in.
fn flat(e: WbError) -> String {
    format!("{}: {}", e.code, e.message)
}

/// A job or finding id (`wb-` + 12 lowercase hex). Checked before it reaches the registry or a path (S11).
fn checked_id(id: &str) -> Result<&str, String> {
    if store::valid_id(id) {
        Ok(id)
    } else {
        Err(flat(WbError::new(WB_BAD_ID, words::bad_id())))
    }
}

/// The app's one registry, on the real folders.
#[cfg(not(test))]
fn app_live() -> &'static Live {
    crate::checkpoints::bisect::live::live()
}

/// Tests never reach the app's registry (it would open the app data folder). They call the `*_in`
/// functions with their own `Live`.
#[cfg(test)]
fn app_live() -> &'static Live {
    unreachable!("tests call the *_in functions with their own registry")
}

/// Runs `work` against the app's registry on the blocking pool and flattens the result.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Live) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || work(app_live()))
        .await
        .map_err(|e| format!("{WB_INTERNAL}: {e}"))?
}

/// The sheet's first view of a pane: its tests, suggestions, save points, limits and whether a search can start.
#[tauri::command]
pub async fn wb_overview(pane_id: String) -> Result<Overview, String> {
    blocking(move |live| overview_in(live, &pane_id)).await
}

fn overview_in(live: &Live, pane_id: &str) -> Result<Overview, String> {
    live.overview(pane_id).map_err(flat)
}

/// Save the test command the person typed or picked. Shell operators are refused with the existing message.
#[tauri::command]
pub async fn wb_declare(
    pane_id: String,
    name: String,
    command_text: String,
    timeout_min: u32,
) -> Result<DeclaredView, String> {
    blocking(move |live| declare_in(live, &pane_id, &name, &command_text, timeout_min)).await
}

fn declare_in(live: &Live, pane_id: &str, name: &str, command_text: &str, timeout_min: u32) -> Result<DeclaredView, String> {
    live.declare(pane_id, name, command_text, timeout_min).map_err(flat)
}

/// Start a search on a declared test. `check_index` is an index into the list, never command text (S3).
#[tauri::command]
pub async fn wb_start(
    pane_id: String,
    check_index: usize,
    good_checkpoint_id: Option<String>,
    limits: Option<LimitsView>,
    continue_from: Option<String>,
) -> Result<JobView, String> {
    blocking(move |live| start_in(live, &pane_id, check_index, good_checkpoint_id, limits, continue_from.as_deref())).await
}

fn start_in(
    live: &Live,
    pane_id: &str,
    check_index: usize,
    good_checkpoint_id: Option<String>,
    limits: Option<LimitsView>,
    continue_from: Option<&str>,
) -> Result<JobView, String> {
    let resume = continue_from.map(checked_id).transpose()?.map(str::to_string);
    live.start(pane_id, check_index, good_checkpoint_id, limits, resume).map_err(flat)
}

/// Where a search is now, for the progress line.
#[tauri::command]
pub async fn wb_status(job_id: String) -> Result<JobView, String> {
    blocking(move |live| status_in(live, &job_id)).await
}

fn status_in(live: &Live, job_id: &str) -> Result<JobView, String> {
    live.status(checked_id(job_id)?).map_err(flat)
}

/// Ask a running search to stop. Its test process tree is killed.
#[tauri::command]
pub async fn wb_stop(job_id: String) -> Result<(), String> {
    blocking(move |live| stop_in(live, &job_id)).await
}

fn stop_in(live: &Live, job_id: &str) -> Result<(), String> {
    live.stop(checked_id(job_id)?).map_err(flat)
}

/// One result, for the card.
#[tauri::command]
pub async fn wb_finding(finding_id: String) -> Result<FindingView, String> {
    blocking(move |live| finding_in(live, &finding_id)).await
}

fn finding_in(live: &Live, finding_id: &str) -> Result<FindingView, String> {
    live.finding(checked_id(finding_id)?).map_err(flat)
}

/// The results of the pane's folder, newest first.
#[tauri::command]
pub async fn wb_findings(pane_id: String) -> Result<Vec<FindingView>, String> {
    blocking(move |live| findings_in(live, &pane_id)).await
}

fn findings_in(live: &Live, pane_id: &str) -> Result<Vec<FindingView>, String> {
    live.findings(pane_id).map_err(flat)
}

/// The files the culprit turn changed, or with `path` one file's diff.
#[tauri::command]
pub async fn wb_diff(finding_id: String, path: Option<String>) -> Result<DiffView, String> {
    blocking(move |live| diff_in(live, &finding_id, path.as_deref())).await
}

fn diff_in(live: &Live, finding_id: &str, path: Option<&str>) -> Result<DiffView, String> {
    live.diff(checked_id(finding_id)?, path).map_err(flat)
}

/// The dry run of "Undo only this turn". Writes nothing.
#[tauri::command]
pub async fn wb_undo_plan(finding_id: String) -> Result<UndoPlanView, String> {
    blocking(move |live| undo_plan_in(live, &finding_id)).await
}

fn undo_plan_in(live: &Live, finding_id: &str) -> Result<UndoPlanView, String> {
    live.undo_plan(checked_id(finding_id)?).map_err(flat)
}

/// Undo only the culprit turn. All or nothing.
#[tauri::command]
pub async fn wb_undo(finding_id: String) -> Result<UndoReportView, String> {
    blocking(move |live| undo_in(live, &finding_id)).await
}

fn undo_in(live: &Live, finding_id: &str) -> Result<UndoReportView, String> {
    live.undo(checked_id(finding_id)?).map_err(flat)
}

/// One run after an undo: do the tests pass now?
#[tauri::command]
pub async fn wb_recheck(finding_id: String) -> Result<JobView, String> {
    blocking(move |live| recheck_in(live, &finding_id)).await
}

fn recheck_in(live: &Live, finding_id: &str) -> Result<JobView, String> {
    live.recheck(checked_id(finding_id)?).map_err(flat)
}

/// The run and time limits the sheet shows.
#[tauri::command]
pub async fn wb_settings_get() -> Result<LimitsView, String> {
    blocking(settings_get_in).await
}

fn settings_get_in(live: &Live) -> Result<LimitsView, String> {
    Ok(live.settings_get())
}

/// Save new limits. They are pulled into range first and the saved values come back.
#[tauri::command]
pub async fn wb_settings_set(limits: LimitsView) -> Result<LimitsView, String> {
    blocking(move |live| settings_set_in(live, limits)).await
}

fn settings_set_in(live: &Live, limits: LimitsView) -> Result<LimitsView, String> {
    live.settings_set(limits).map_err(flat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    use parking_lot::Mutex;
    use serde_json::Value;

    use crate::checkpoints::bisect::copy::CopyError;
    use crate::checkpoints::bisect::exec::RunResult;
    use crate::checkpoints::bisect::live::{CopyHandle, LiveIo};
    use crate::checkpoints::bisect::store::Store;
    use crate::checkpoints::{Checkpoint, CheckpointError};
    use crate::checkpoints::PaneFacts;
    use crate::night_shift::checks::{Declared, Suggestion};
    use crate::turn_summary::TurnSummary;
    use crate::worktrees::preset::WorktreePreset;

    /// One pane (`pane-1`) in one folder, one declared test, no save points. No command ever runs here.
    struct Stub {
        cwd: String,
        declared: Mutex<Vec<Declared>>,
    }

    impl LiveIo for Stub {
        fn now_ms(&self) -> u64 {
            1_000_000
        }
        fn pane(&self, pane_id: &str) -> Option<PaneFacts> {
            (pane_id == "pane-1").then(|| PaneFacts {
                cwd: self.cwd.clone(),
                agent_type: "fake".into(),
                status: "idle".into(),
            })
        }
        fn panes_in_folder(&self, _folder: &Path) -> Vec<(String, PaneFacts)> {
            Vec::new()
        }
        fn run_open(&self, _pane_id: &str, _pane: &PaneFacts) -> bool {
            false
        }
        fn checkpoints_enabled(&self) -> bool {
            true
        }
        fn git_available(&self) -> bool {
            true
        }
        fn check_projects(&self, cwd: &str) -> Vec<String> {
            vec![cwd.to_string()]
        }
        fn declared(&self, _project: &str) -> Vec<Declared> {
            self.declared.lock().clone()
        }
        fn declare(&self, _project: &str, list: Vec<Declared>) -> Result<(), String> {
            *self.declared.lock() = list;
            Ok(())
        }
        fn suggestions(&self, _project: &Path) -> Vec<Suggestion> {
            Vec::new()
        }
        fn preset(&self, _cwd: &str) -> WorktreePreset {
            WorktreePreset::default()
        }
        fn summaries(&self, _project: &Path) -> Vec<TurnSummary> {
            Vec::new()
        }
        fn list_all(&self, _project: &Path) -> Result<Vec<Checkpoint>, CheckpointError> {
            Ok(Vec::new())
        }
        fn snapshot_now(&self, _project: &Path, _key: &str, _now_ms: u64) -> Result<String, CheckpointError> {
            Err(CheckpointError::NotFound)
        }
        fn new_copy(&self, _project: &Path, _copies_root: &Path) -> Result<Box<dyn CopyHandle>, CopyError> {
            unreachable!("these tests never start a search")
        }
        fn run_test(&self, _copy: &dyn CopyHandle, _argv: &[String], _timeout_ms: u64, _cancel: &AtomicBool) -> RunResult {
            unreachable!("these tests never start a search")
        }
        fn redact(&self, text: &str) -> String {
            text.to_string()
        }
        fn emit(&self, _job_id: &str, _finding_id: Option<&str>) {}
        fn audit_undo(&self, _summary: &str, _detail: Value) {}
    }

    /// A registry over a stub with one declared test (`npm test`). Returns it with the pane's folder.
    fn registry(tag: &str) -> (Live, PathBuf) {
        let root = std::env::temp_dir().join(format!("pm-wb-cmd-{tag}-{}", uuid::Uuid::new_v4().simple()));
        let cwd = root.join("shop");
        fs::create_dir_all(&cwd).expect("temp folder");
        let stub = Stub {
            cwd: cwd.to_string_lossy().to_string(),
            declared: Mutex::new(vec![Declared {
                name: "npm test".into(),
                argv: vec!["npm".into(), "test".into()],
                timeout_min: 10,
                declared_ms: 1,
            }]),
        };
        (Live::new(std::sync::Arc::new(stub), Store::at(root.join("wb"))), cwd)
    }

    #[test]
    fn a_plain_error_is_code_colon_message() {
        assert_eq!(flat(WbError::new(WB_BAD_ID, "That result id is not valid.")), "WB_BAD_ID: That result id is not valid.");
    }

    #[test]
    fn bad_ids_are_refused_before_the_registry() {
        let (live, _) = registry("ids");
        for bad in ["", "../x", "wb-XXXXXXXXXXXX", "wb-0123456789", "job-0123456789ab", "wb-0123456789AB"] {
            let err = status_in(&live, bad).unwrap_err();
            assert!(err.starts_with("WB_BAD_ID: "), "{bad}: {err}");
            assert_eq!(stop_in(&live, bad).unwrap_err(), err, "{bad}");
            assert_eq!(finding_in(&live, bad).unwrap_err(), err, "{bad}");
            assert_eq!(diff_in(&live, bad, None).unwrap_err(), err, "{bad}");
            assert_eq!(undo_plan_in(&live, bad).unwrap_err(), err, "{bad}");
            assert_eq!(undo_in(&live, bad).unwrap_err(), err, "{bad}");
            assert_eq!(recheck_in(&live, bad).unwrap_err(), err, "{bad}");
            assert_eq!(start_in(&live, "pane-1", 0, None, None, Some(bad)).unwrap_err(), err, "{bad}");
        }
    }

    #[test]
    fn a_valid_but_unknown_id_is_refused_by_the_registry() {
        let (live, _) = registry("unknown");
        let err = finding_in(&live, "wb-0123456789ab").unwrap_err();
        assert!(err.starts_with("WB_BAD_ID: "), "{err}");
        assert!(undo_in(&live, "wb-0123456789ab").unwrap_err().starts_with("WB_BAD_ID: "));
    }

    #[test]
    fn a_bad_check_index_is_refused() {
        let (live, _) = registry("index");
        let err = start_in(&live, "pane-1", 1, None, None, None).unwrap_err();
        assert!(err.starts_with("WB_NO_TEST: "), "{err}");
        assert_eq!(start_in(&live, "pane-1", usize::MAX, None, None, None).unwrap_err(), err);
    }

    #[test]
    fn a_pane_that_is_gone_is_refused() {
        let (live, _) = registry("pane");
        assert!(overview_in(&live, "pane-9").unwrap_err().starts_with("WB_UNAVAILABLE: "));
        assert!(findings_in(&live, "pane-9").unwrap_err().starts_with("WB_UNAVAILABLE: "));
        assert!(declare_in(&live, "pane-9", "Tests", "npm test", 5).unwrap_err().starts_with("WB_UNAVAILABLE: "));
    }

    #[test]
    fn declare_refuses_shell_text_and_keeps_a_plain_command() {
        let (live, _) = registry("declare");
        for text in ["npm test && rm -rf .", "npm test | more", "npm test; calc", "npm test > out.txt", "echo $(whoami)"] {
            let err = declare_in(&live, "pane-1", "Tests", text, 10).unwrap_err();
            assert!(err.starts_with("WB_NO_TEST: "), "{text}: {err}");
        }
        let view = declare_in(&live, "pane-1", "Tests", "npm test", 10).expect("a plain command is saved");
        assert_eq!(view.command_text, "npm test");
        assert_eq!(view.name, "Tests");
    }

    #[test]
    fn limits_are_clamped_and_saved() {
        let (live, _) = registry("limits");
        assert_eq!(settings_get_in(&live).unwrap(), LimitsView { max_runs: 16, max_minutes: 20 });
        let saved = settings_set_in(&live, LimitsView { max_runs: 999, max_minutes: 0 }).unwrap();
        assert_eq!(saved, LimitsView { max_runs: 30, max_minutes: 1 });
        assert_eq!(settings_get_in(&live).unwrap(), saved);
    }

    #[test]
    fn a_good_id_is_accepted() {
        assert_eq!(checked_id("wb-0123456789ab"), Ok("wb-0123456789ab"));
    }
}
