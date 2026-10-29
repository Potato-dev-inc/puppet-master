//! Tauri commands for Review Desk (SPEC n33 section 3.6). Desktop window only: no MCP tool, bridge route,
//! companion or channel reaches these (S1). Each command runs off the UI thread and answers with an
//! `OperationError` on refusal. The work lives in `crate::review_desk`; this file only wires the live layer,
//! the note cap and the events the other windows listen to.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use super::AppState;
use crate::operations::OperationError;
use crate::review_desk::act;
use crate::review_desk::gather;
use crate::review_desk::live::LiveIo;
use crate::review_desk::model::NOTE_MAX;
use crate::review_desk::view::{ActView, DeskView, DetailView};

/// The event the desk listens to. Its payload is empty; the desk reads again.
const DESK_EVENT: &str = "review-desk://changed";

fn live_io(app: &AppHandle, state: &State<'_, AppState>) -> LiveIo {
    LiveIo { registry: state.registry.clone(), app: app.clone() }
}

/// Runs a command body off the UI thread.
async fn off_thread<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, OperationError> + Send + 'static,
) -> Result<T, OperationError> {
    match tauri::async_runtime::spawn_blocking(job).await {
        Ok(result) => result,
        Err(error) => Err(OperationError::new("INTERNAL", error.to_string(), true)),
    }
}

/// Tells the windows what a Keep or a Throw away may have changed: the desk, the copies and their panes (a merge
/// closes the copy's panes), and the Night Shift list. Runs on the blocking thread, where the Night store is read.
fn after_action(app: &AppHandle) {
    let _ = app.emit(DESK_EVENT, json!({}));
    let _ = app.emit("worktree://changed", ());
    let _ = app.emit("pty://panes-changed", ());
    night_changed(app);
}

/// The Night Shift list reads the run again (same payload as the Night commands send). Tests never open the app's
/// real Night store (`Store::live` exists only outside tests).
#[cfg(not(test))]
fn night_changed(app: &AppHandle) {
    if let Some(run) = crate::night_shift::store::Store::live().load() {
        let state = serde_json::to_value(&run.state).unwrap_or(Value::Null);
        let _ = app.emit("night://changed", json!({ "rev": run.rev, "state": state }));
    }
}

#[cfg(test)]
fn night_changed(_app: &AppHandle) {}

/// The person's note as typed, cut to `NOTE_MAX` characters. A longer note is cut, never refused.
fn cut_note(note: &str) -> String {
    note.chars().take(NOTE_MAX).collect()
}

/// The whole desk, ordered, with the counts and the notes of the sources that could not be read.
#[tauri::command]
pub async fn review_desk_list(app: AppHandle, state: State<'_, AppState>) -> Result<DeskView, OperationError> {
    let io = live_io(&app, &state);
    off_thread(move || Ok(gather::gather(&io))).await
}

/// The number for the sidebar. The per-copy facts are cached for a minute, so this stays cheap on repeat calls.
#[tauri::command]
pub async fn review_desk_count(app: AppHandle, state: State<'_, AppState>) -> Result<u32, OperationError> {
    let io = live_io(&app, &state);
    off_thread(move || Ok(gather::gather(&io).counts.total)).await
}

/// One item's files and diff (redacted), and its extra lines.
#[tauri::command]
pub async fn review_desk_detail(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<DetailView, OperationError> {
    let io = live_io(&app, &state);
    off_thread(move || act::detail(&io, &key)).await
}

/// Keep. Only the confirm sheet (or "Looks good") calls this, with the fingerprint the person saw.
#[tauri::command]
pub async fn review_desk_keep(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    fingerprint: String,
) -> Result<ActView, OperationError> {
    let io = live_io(&app, &state);
    off_thread(move || {
        let result = act::keep(&io, &key, &fingerprint);
        after_action(&app);
        result
    })
    .await
}

/// Throw away. The window calls this only when the 10 s Undo window ends.
#[tauri::command]
pub async fn review_desk_throw_away(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    fingerprint: String,
) -> Result<ActView, OperationError> {
    let io = live_io(&app, &state);
    off_thread(move || {
        let result = act::throw_away(&io, &key, &fingerprint);
        after_action(&app);
        result
    })
    .await
}

/// Takes back a "Looks good" for one folder pane.
#[tauri::command]
pub async fn review_desk_unmark(app: AppHandle, state: State<'_, AppState>, pane_id: String) -> Result<(), OperationError> {
    let io = live_io(&app, &state);
    off_thread(move || {
        let result = act::unmark(&io, &pane_id);
        let _ = app.emit(DESK_EVENT, json!({}));
        result
    })
    .await
}

/// Ask to fix. Puts the fixed draft in the item's chat box and returns the pane to open. Never sends.
#[tauri::command]
pub async fn review_desk_fix(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    fingerprint: String,
    note: String,
) -> Result<String, OperationError> {
    let io = live_io(&app, &state);
    let note = cut_note(&note);
    off_thread(move || act::fix(&io, &key, &fingerprint, &note)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue_intake::model::IssueRun;
    use crate::night_shift::model::NightRun;
    use crate::night_shift::review::KeepView as NightKeepView;
    use crate::review_desk::live::{CopyRead, DeskIo, OpRead};
    use crate::review_desk::model::Clean;
    use crate::review_desk::sources::copies::MissionFacts;
    use crate::review_desk::sources::folder::PaneFacts;
    use crate::turn_summary::model::TurnSummary;
    use crate::worktrees::service::ListItem;
    use crate::worktrees::{ChangeSummary, MergeOutcome};
    use std::path::PathBuf;

    /// Every read and write panics. A bad key must be refused before any of them runs.
    struct Dead;

    fn nope<T>() -> T {
        panic!("a bad key must be refused before any read or write")
    }

    impl DeskIo for Dead {
        fn now_ms(&self) -> u64 { nope() }
        fn clean(&self) -> Clean { nope() }
        fn clean_patch(&self, _raw: &str) -> String { nope() }
        fn marks_root(&self) -> PathBuf { nope() }
        fn night_run(&self) -> Result<Option<NightRun>, String> { nope() }
        fn issue_runs(&self) -> Result<Vec<IssueRun>, String> { nope() }
        fn issue_folder(&self, _repo_id: &str) -> Option<String> { nope() }
        fn copies(&self) -> Result<Vec<ListItem>, String> { nope() }
        fn keep_target(&self, _project: &str) -> Option<String> { nope() }
        fn label_of(&self, _agent_type: &str) -> String { nope() }
        fn copy_read(&self, _project: &str, _handle: &str, _summary: Option<ChangeSummary>) -> CopyRead { nope() }
        fn copy_patch(&self, _project: &str, _handle: &str) -> Option<(String, bool)> { nope() }
        fn latest_op(&self, _project: &str, _handle: &str) -> Option<OpRead> { nope() }
        fn mission(&self, _project: &str, _mission_id: &str) -> Option<MissionFacts> { nope() }
        fn mission_headline(&self, _project: &str, _mission_id: &str) -> Option<String> { nope() }
        fn turns(&self, _since_ms: u64) -> Vec<(TurnSummary, Option<PathBuf>)> { nope() }
        fn panes(&self) -> Vec<PaneFacts> { nope() }
        fn checkpoint_before(&self, _pane_id: &str, _started_at_ms: u64) -> Option<String> { nope() }
        fn checkpoint_last_after(&self, _pane_id: &str) -> Option<String> { nope() }
        fn range_read(&self, _pane_id: &str, _from_id: &str, _to_id: &str) -> CopyRead { nope() }
        fn range_patch(&self, _pane_id: &str, _from_id: &str, _to_id: &str, _max_bytes: usize) -> Option<(String, bool)> { nope() }
        fn merge(&self, _project: &str, _handle: &str) -> Result<MergeOutcome, OperationError> { nope() }
        fn discard(&self, _project: &str, _handle: &str) -> Result<(), OperationError> { nope() }
        fn night_keep(&self, _task_id: &str) -> Result<NightKeepView, OperationError> { nope() }
        fn night_throw(&self, _task_id: &str) -> Result<(), OperationError> { nope() }
        fn issue_act(&self, _run_id: &str, _action: &str) -> Result<(), OperationError> { nope() }
        fn restore(&self, _pane_id: &str, _checkpoint_id: &str) -> Result<(), OperationError> { nope() }
        fn put_draft(&self, _pane_id: &str, _text: String) { nope() }
        fn audit(&self, _summary: &str, _detail: Value) { nope() }
    }

    #[test]
    fn a_bad_key_is_refused_with_plain_text_before_any_read_or_write() {
        for bad in ["copy:../x", "copy:", "night:nt-1", "issue:ir-zz", "folder:p:5-2"] {
            let keep = act::keep(&Dead, bad, "fp").err().expect("refused");
            assert_eq!(keep.code, "REVIEW_BAD_KEY", "{bad}");
            assert_eq!(keep.message, "That is not a valid review item.");
            assert_eq!(act::throw_away(&Dead, bad, "fp").err().expect("refused").code, "REVIEW_BAD_KEY");
            assert_eq!(act::fix(&Dead, bad, "fp", "").err().expect("refused").code, "REVIEW_BAD_KEY");
            assert_eq!(act::detail(&Dead, bad).err().expect("refused").code, "REVIEW_BAD_KEY");
        }
        assert_eq!(act::unmark(&Dead, "../x").err().expect("refused").code, "REVIEW_BAD_KEY");
    }

    #[test]
    fn a_long_note_is_cut_and_never_refused() {
        let long = "a".repeat(NOTE_MAX + 250);
        assert_eq!(cut_note(&long).chars().count(), NOTE_MAX);
        let accented = "é".repeat(NOTE_MAX + 1);
        assert_eq!(cut_note(&accented).chars().count(), NOTE_MAX);
        assert_eq!(cut_note("short note"), "short note");
        assert_eq!(cut_note(""), "");
    }
}
