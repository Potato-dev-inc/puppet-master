//! Tauri commands of "Red to Green" (SPEC n33 section 3.6). Desktop only: no bridge route and no MCP
//! tool. The commands are thin: the engine lives in `ci_fix::live`, the saved settings in `ci_fix::store`.
//! What this file adds: the checks of the arguments before the engine sees them (a bad id, a button that
//! does not belong to that kind of row, an empty copy handle) and the settings clamped before they are saved.
//!
//! Every command runs its work off the UI thread (git and `gh` calls can take a moment) and returns an
//! `OperationError` (a code and one plain sentence) when it refuses. The names and argument names match
//! the TypeScript side (`packages/app/src/lib/ci-fix.ts`).

use crate::ci_fix::live;
use crate::ci_fix::model::{self, Settings};
use crate::ci_fix::view::{CiFixView, FixLogView, WatchView};
use crate::operations::OperationError;

/// The buttons a watch row can take (the engine checks them again).
const WATCH_ACTIONS: [&str; 2] = ["stop_watching", "watch_again"];
/// The buttons a fix row can take (the engine checks them again).
const FIX_ACTIONS: [&str; 3] = ["ask_again", "push", "dont_push"];
/// The longest copy handle the engine is asked about. Real handles are a few dozen characters.
const MAX_HANDLE_CHARS: usize = 200;

fn invalid(message: impl Into<String>) -> OperationError {
    OperationError::new("INVALID_ARGUMENT", message, true)
}

/// Runs one blocking job on Tauri's pool. A panic or a cancelled job becomes an `INTERNAL` error.
async fn off_ui<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T, OperationError> {
    tauri::async_runtime::spawn_blocking(job).await.map_err(|error| OperationError::new("INTERNAL", error.to_string(), false))
}

/// `ci_fix_act` takes a watch id with a watch button, or a fix id with a fix button. Anything else is refused
/// here, so a fix id never reaches a watch button (and the other way round).
fn check_act_args(id: &str, action: &str) -> Result<(), OperationError> {
    if model::valid_watch_id(id) {
        if WATCH_ACTIONS.contains(&action) {
            return Ok(());
        }
        return Err(invalid("That button is not for a pull request."));
    }
    if model::valid_fix_id(id) {
        if FIX_ACTIONS.contains(&action) {
            return Ok(());
        }
        return Err(invalid("That button is not for a fix."));
    }
    Err(invalid("That is not a pull request or a fix id."))
}

/// `ci_fix_offer` needs the copy's handle: one line of text, not empty, not too long.
fn check_offer_args(handle: &str) -> Result<(), OperationError> {
    let handle = handle.trim();
    if handle.is_empty() || handle.chars().count() > MAX_HANDLE_CHARS || handle.chars().any(char::is_control) {
        return Err(invalid("That is not a copy."));
    }
    Ok(())
}

/// The project folder the person picked, or `None` when none was sent (every known project is searched then).
fn project_arg(project_path: Option<String>) -> Option<String> {
    project_path.map(|path| path.trim().to_string()).filter(|path| !path.is_empty())
}

/// The settings as they are saved: every number pulled into its range. The store clamps again on save.
fn settings_for_save(settings: &Settings) -> Settings {
    settings.clamped()
}

/// `ci_fix_log` needs a fix id (the stored snapshot is looked up by it).
fn check_fix_id(fix_id: &str) -> Result<(), OperationError> {
    if model::valid_fix_id(fix_id) {
        return Ok(());
    }
    Err(invalid("That is not a fix id."))
}

// ------------------------------------------------------------------------------- commands

/// What the Settings page shows: the settings, the GitHub status, the watches and the latest fixes.
#[tauri::command]
pub async fn ci_fix_view() -> Result<CiFixView, OperationError> {
    off_ui(live::view).await
}

/// Save the settings (clamped) and tell the windows.
#[tauri::command]
pub async fn ci_fix_save_settings(settings: Settings) -> Result<CiFixView, OperationError> {
    let clean = settings_for_save(&settings);
    off_ui(move || live::save_settings(&clean, "desktop")).await?
}

/// The PR block's `Fix with <agent>` / `Watch CI`: watch the pull request of this copy, and raise the fix card
/// now when its head already failed.
#[tauri::command]
pub async fn ci_fix_offer(project_path: Option<String>, handle: String) -> Result<WatchView, OperationError> {
    check_offer_args(&handle)?;
    let project = project_arg(project_path);
    let handle = handle.trim().to_string();
    off_ui(move || live::offer(project.as_deref(), &handle)).await?
}

/// One button of a watch row or a fix row (the same paths as the cards; `who` is the desktop).
#[tauri::command]
pub async fn ci_fix_act(id: String, action: String) -> Result<CiFixView, OperationError> {
    check_act_args(&id, &action)?;
    off_ui(move || live::act(&id, &action, "desktop")).await?
}

/// The cleaned log a fix started with: the stored snapshot only (empty once the fix ended).
#[tauri::command]
pub async fn ci_fix_log(fix_id: String) -> Result<FixLogView, OperationError> {
    check_fix_id(&fix_id)?;
    off_ui(move || live::log(&fix_id)).await?
}

/// Look at GitHub now instead of waiting for the next poll.
#[tauri::command]
pub async fn ci_fix_poll_now() -> Result<(), OperationError> {
    off_ui(live::poll_now).await
}

// ------------------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::AgentRunRequest;
    use crate::ci_fix::engine::Engine;
    use crate::ci_fix::live::{Io, RunFacts, Started};
    use crate::ci_fix::store::Store;
    use crate::issue_intake::live::RepoFacts;
    use crate::issue_intake::model::AgentPick;
    use crate::issue_intake::pushcheck::DiffText;
    use crate::questions::{NewQuestion, Question};
    use crate::team::sources::github::GhResponse;
    use crate::worktrees::WorktreeRef;
    use serde_json::Value;
    use std::path::Path;
    use std::sync::Arc;

    // Valid ids (a watch is `cw-` + 8 hex, a fix is `cf-` + 12 hex).
    const WATCH: &str = "cw-0a1b2c3d";
    const FIX: &str = "cf-0a1b2c3d4e5f";
    const PROJECT: &str = "C:/Users/you/work/billing";
    const HANDLE: &str = "issue-42-a1b2";

    #[test]
    fn act_refuses_a_bad_id() {
        for id in ["", "nope", "cw-0a1b2c3", "CW-0A1B2C3D", "cf-0a1b2c3d4e5", " cw-0a1b2c3d"] {
            let err = check_act_args(id, "stop_watching").unwrap_err();
            assert_eq!(err.code, "INVALID_ARGUMENT", "id {id:?}");
        }
    }

    #[test]
    fn act_refuses_a_bad_action() {
        for action in ["", "delete", "Stop_Watching", "push "] {
            assert_eq!(check_act_args(WATCH, action).unwrap_err().code, "INVALID_ARGUMENT", "watch, action {action:?}");
            assert_eq!(check_act_args(FIX, action).unwrap_err().code, "INVALID_ARGUMENT", "fix, action {action:?}");
        }
    }

    #[test]
    fn act_refuses_a_button_of_the_other_kind() {
        // A fix button on a watch id, and a watch button on a fix id.
        for action in ["push", "dont_push", "ask_again"] {
            assert_eq!(check_act_args(WATCH, action).unwrap_err().code, "INVALID_ARGUMENT", "watch, {action}");
        }
        for action in ["stop_watching", "watch_again"] {
            assert_eq!(check_act_args(FIX, action).unwrap_err().code, "INVALID_ARGUMENT", "fix, {action}");
        }
    }

    #[test]
    fn act_accepts_each_button_of_its_kind() {
        for action in WATCH_ACTIONS {
            assert!(check_act_args(WATCH, action).is_ok(), "watch, {action}");
        }
        for action in FIX_ACTIONS {
            assert!(check_act_args(FIX, action).is_ok(), "fix, {action}");
        }
    }

    #[test]
    fn offer_needs_a_plain_copy_handle() {
        assert!(check_offer_args(HANDLE).is_ok());
        assert!(check_offer_args("  issue-42-a1b2  ").is_ok());
        assert_eq!(check_offer_args("").unwrap_err().code, "INVALID_ARGUMENT");
        assert_eq!(check_offer_args("   ").unwrap_err().code, "INVALID_ARGUMENT");
        assert_eq!(check_offer_args("two\nlines").unwrap_err().code, "INVALID_ARGUMENT");
        assert_eq!(check_offer_args(&"a".repeat(MAX_HANDLE_CHARS + 1)).unwrap_err().code, "INVALID_ARGUMENT");
    }

    #[test]
    fn empty_project_means_every_known_project() {
        assert_eq!(project_arg(None), None);
        assert_eq!(project_arg(Some("   ".to_string())), None);
        assert_eq!(project_arg(Some(format!(" {PROJECT} "))), Some(PROJECT.to_string()));
    }

    #[test]
    fn log_needs_a_fix_id() {
        assert!(check_fix_id(FIX).is_ok());
        assert_eq!(check_fix_id(WATCH).unwrap_err().code, "INVALID_ARGUMENT");
        assert_eq!(check_fix_id("").unwrap_err().code, "INVALID_ARGUMENT");
    }

    #[test]
    fn settings_are_clamped_on_save() {
        let wild = Settings {
            enabled: true,
            tries_per_pr: 9,
            turns_per_try: 0,
            minutes_per_try: 500,
            fixes_per_day: 0,
        };
        let saved = settings_for_save(&wild);
        assert!(saved.enabled, "the switch is kept as it is");
        assert_eq!(saved.tries_per_pr, 3);
        assert_eq!(saved.turns_per_try, 1);
        assert_eq!(saved.minutes_per_try, 60);
        assert_eq!(saved.fixes_per_day, 1);

        let low = Settings { enabled: false, tries_per_pr: 0, turns_per_try: 99, minutes_per_try: 1, fixes_per_day: 99 };
        let saved = settings_for_save(&low);
        assert!(!saved.enabled);
        assert_eq!((saved.tries_per_pr, saved.turns_per_try, saved.minutes_per_try, saved.fixes_per_day), (1, 50, 5, 20));

        // Values in range are not changed.
        let ok = Settings::default();
        assert_eq!(settings_for_save(&ok), ok);
    }

    /// A world for the `offer` test: one copy with no pull request, and a store where the switch is `enabled`.
    /// Only `now_ms`, `store` and `find_copy_any` are reached before the engine refuses the copy; every other
    /// method is a stub that must never run in this test.
    struct NoPrWorld {
        store: Store,
        copy: WorktreeRef,
    }

    impl NoPrWorld {
        fn new(enabled: bool) -> Self {
            let root = std::env::temp_dir().join(format!("pm-ci-fix-cmd-{}", uuid::Uuid::new_v4().simple()));
            let store = Store::at(root);
            store.save_settings(&Settings { enabled, ..Settings::default() }).expect("settings");
            let copy: WorktreeRef = serde_json::from_value(serde_json::json!({
                "path": "C:/Users/you/work/billing-copy",
                "cwd": "C:/Users/you/work/billing-copy",
                "branch": "pm/issue-42-a1b2",
                "base": "main",
                "base_commit": "0000000000000000000000000000000000000000",
                "repo": PROJECT,
                "handle": HANDLE,
            }))
            .expect("copy");
            Self { store, copy }
        }
    }

    impl Io for NoPrWorld {
        fn now_ms(&self) -> u64 {
            1_700_000_000_000
        }
        fn store(&self) -> &Store {
            &self.store
        }
        fn spawn(&self, _name: &str, _job: Box<dyn FnOnce() + Send>) {
            unreachable!("offer does not spawn")
        }
        fn changed(&self) {}
        fn audit(&self, _event: &str, _who: &str, _summary: &str, _detail: Value) {}
        fn gh_state(&self) -> String {
            unreachable!("offer does not read GitHub")
        }
        fn gh_api(&self, _path: &str, _etag: Option<&str>) -> Result<GhResponse, String> {
            unreachable!("offer does not read GitHub")
        }
        fn job_log(&self, _path: &str) -> Result<String, String> {
            unreachable!("offer does not read logs")
        }
        fn repo_facts(&self, _dir: &Path) -> Result<RepoFacts, String> {
            unreachable!("offer does not read the repository")
        }
        fn folder_owner_repo(&self, _folder: &str) -> Option<String> {
            unreachable!("offer checks the copy's pull request first")
        }
        fn find_copy(&self, _project: &str, _handle: &str) -> Option<WorktreeRef> {
            unreachable!("offer uses find_copy_any")
        }
        fn find_copy_any(&self, project: Option<&str>, _handle: &str) -> Option<(String, WorktreeRef)> {
            Some((project.unwrap_or(PROJECT).to_string(), self.copy.clone()))
        }
        fn git_env(&self) -> Result<Vec<(String, String)>, OperationError> {
            unreachable!("offer does not run git")
        }
        fn head_sha(&self, _copy: &WorktreeRef, _env: &[(String, String)]) -> Result<String, OperationError> {
            unreachable!("offer does not run git")
        }
        fn current_branch(&self, _copy: &WorktreeRef, _env: &[(String, String)]) -> Result<Option<String>, OperationError> {
            unreachable!("offer does not run git")
        }
        fn is_ancestor(&self, _copy: &WorktreeRef, _a: &str, _b: &str, _env: &[(String, String)]) -> Result<bool, OperationError> {
            unreachable!("offer does not run git")
        }
        fn fingerprint(&self, _copy: &WorktreeRef) -> Result<String, OperationError> {
            unreachable!("offer does not run git")
        }
        fn commit_pending(&self, _copy: &WorktreeRef, _env: &[(String, String)]) -> Result<bool, OperationError> {
            unreachable!("offer does not run git")
        }
        fn changed_files(&self, _copy: &WorktreeRef, _base: &str, _env: &[(String, String)]) -> Result<Vec<String>, OperationError> {
            unreachable!("offer does not run git")
        }
        fn name_status(&self, _copy: &WorktreeRef, _base: &str, _env: &[(String, String)]) -> Result<Vec<(String, String)>, OperationError> {
            unreachable!("offer does not run git")
        }
        fn push_diff(&self, _copy: &WorktreeRef, _base: &str, _env: &[(String, String)]) -> Result<DiffText, OperationError> {
            unreachable!("offer does not run git")
        }
        fn commit_count(&self, _copy: &WorktreeRef, _base: &str, _env: &[(String, String)]) -> Result<u32, OperationError> {
            unreachable!("offer does not run git")
        }
        fn scan_added(&self, _diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
            unreachable!("offer does not scan diffs")
        }
        fn remote_branch_sha(&self, _copy: &WorktreeRef, _env: &[(String, String)]) -> Result<Option<String>, OperationError> {
            unreachable!("offer does not read the remote")
        }
        fn push_fix(
            &self,
            _project: &str,
            _handle: &str,
            _expected_remote: &str,
            _default_branch: Option<&str>,
            _env: &[(String, String)],
        ) -> Result<String, OperationError> {
            unreachable!("offer never pushes")
        }
        fn write_log_file(&self, _copy: &WorktreeRef, _name: &str, _text: &str) -> Result<(), String> {
            unreachable!("offer writes no file")
        }
        fn agent_ready(&self, _agent_type: &str) -> bool {
            unreachable!("offer checks the copy's pull request first")
        }
        fn agent_label(&self, _agent_type: &str) -> String {
            unreachable!("offer checks the copy's pull request first")
        }
        fn agent_of_copy(&self, _project: &str, _handle: &str) -> Option<AgentPick> {
            unreachable!("offer checks the copy's pull request first")
        }
        fn made_by_issue_run(&self, _handle: &str) -> bool {
            unreachable!("offer checks the copy's pull request first")
        }
        fn run_snapshot(&self, _project: &str, _handle: &str) -> Option<RunFacts> {
            unreachable!("offer checks the copy's pull request first")
        }
        fn copy_busy(&self, _project: &str, _copy: &WorktreeRef) -> bool {
            unreachable!("offer checks the copy's pull request first")
        }
        fn turn_sentence(&self, _pane_id: &str) -> Option<String> {
            unreachable!("offer checks the copy's pull request first")
        }
        fn followup(&self, _project: &str, _handle: &str, _task: String, _key: &str) -> Result<Started, OperationError> {
            unreachable!("offer never starts a fix")
        }
        fn run_agent(&self, _request: AgentRunRequest) -> Result<Started, OperationError> {
            unreachable!("offer never starts a fix")
        }
        fn ask(&self, _new: NewQuestion, _fix_id: Option<&str>) -> Result<String, String> {
            unreachable!("offer with no pull request asks nothing")
        }
        fn ask_as(&self, _new: NewQuestion, _id: &str, _fix_id: Option<&str>) -> Result<String, String> {
            unreachable!("offer with no pull request asks nothing")
        }
        fn question(&self, _id: &str) -> Option<Question> {
            unreachable!("offer with no pull request asks nothing")
        }
        fn dismiss(&self, _id: &str) {
            unreachable!("offer with no pull request dismisses nothing")
        }
    }

    #[test]
    fn offer_with_the_switch_off_says_so_first() {
        let engine = Arc::new(Engine::new(NoPrWorld::new(false)));
        assert_eq!(engine.offer(Some(PROJECT), HANDLE).unwrap_err().code, "CI_FIX_OFF");
    }

    #[test]
    fn offer_on_a_copy_without_a_pull_request_says_no_pr() {
        let engine = Arc::new(Engine::new(NoPrWorld::new(true)));
        assert_eq!(engine.offer(Some(PROJECT), HANDLE).unwrap_err().code, "NO_PR");
        assert_eq!(engine.offer(None, HANDLE).unwrap_err().code, "NO_PR");
    }
}
