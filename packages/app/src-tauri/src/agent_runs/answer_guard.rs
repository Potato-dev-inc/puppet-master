//! Safety rails around `answer_prompt` for an agent-run handle. Everything that can refuse an
//! answer (which project the handle lives in, who owns it, whether this prompt was already
//! answered) is decided here, before a key is sent to the pane. The receipt it hands back is
//! truthful about what happened after the key went out.

use super::runtime::{self, AgentRunView};
use crate::operations::{self, OperationError, OperationSnapshot};
use serde_json::{json, Value};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// How long after a delivered answer the same prompt id still counts as that answer even if the
/// pane has not redrawn yet (a retry in that window must not press the key a second time).
const REPLAY_WINDOW_MS: u64 = 5_000;
/// Receipts kept per process; the oldest are dropped past this.
const LEDGER_CAP: usize = 512;

#[derive(Clone)]
struct Applied {
    choice: String,
    at_ms: u64,
}

type Ledger = HashMap<(String, String), Applied>;

fn ledger() -> &'static Mutex<Ledger> {
    static LEDGER: OnceLock<Mutex<Ledger>> = OnceLock::new();
    LEDGER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Answers are checked against the ledger and then sent as a keystroke; two concurrent answers
/// for one prompt must not both pass the check before either has recorded itself, or the key is
/// pressed twice (the TUI then receives a stray character). Hold this across check, send, record.
pub(super) fn serialize_answers() -> parking_lot::MutexGuard<'static, ()> {
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| Mutex::new(())).lock()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Find the run `handle` names and check this connection may answer for it. Nothing here touches
/// the pane, so every refusal happens before a key is sent.
///
/// * explicit `project_path`: the handle must live there, otherwise `PROJECT_MISMATCH`;
/// * omitted: the bridge's selected project wins when it owns the handle, else the single
///   indexed project that does; several owners is `AMBIGUOUS_HANDLE`.
pub(super) fn resolve_target(
    default_project: &str,
    explicit_project: Option<&str>,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    let explicit = explicit_project.filter(|path| !path.trim().is_empty());
    let search_from = explicit.unwrap_or(default_project);
    let chosen = match operations::latest_agent_run(search_from, handle) {
        Ok(snapshot) => snapshot,
        Err(not_here) => {
            let mut elsewhere = operations::agent_runs_by_project(search_from, handle)?;
            match (explicit, elsewhere.len()) {
                (_, 0) => return Err(not_here),
                (None, 1) => elsewhere.remove(0),
                (None, _) => {
                    let projects: Vec<&str> =
                        elsewhere.iter().map(|s| s.project_path.as_str()).collect();
                    let mut error = OperationError::new(
                        "AMBIGUOUS_HANDLE",
                        format!(
                            "handle `{handle}` exists in {} projects; nothing was sent. Pass project_path to say which one",
                            projects.len()
                        ),
                        false,
                    );
                    error.context =
                        json!({"handle": handle, "projects": projects, "sent_input": false});
                    return Err(error);
                }
                (Some(asked), _) => {
                    // Ownership first, so a caller without access learns nothing about the run.
                    runtime::check_snapshot_mutate_access(&elsewhere[0], handle, session_id)?;
                    let mut error = OperationError::new(
                        "PROJECT_MISMATCH",
                        format!(
                            "handle `{handle}` belongs to project {}, not {asked}; nothing was sent",
                            elsewhere[0].project_path
                        ),
                        false,
                    );
                    error.context = json!({
                        "handle": handle,
                        "requested_project": asked,
                        "handle_project": elsewhere[0].project_path,
                        "sent_input": false,
                    });
                    return Err(error);
                }
            }
        }
    };
    runtime::check_snapshot_mutate_access(&chosen, handle, session_id)?;
    Ok(chosen)
}

fn answer_block(
    snapshot: &OperationSnapshot,
    prompt_id: &str,
    choice: &str,
    already_applied: bool,
    at_ms: u64,
) -> Value {
    json!({
        "applied": true,
        "already_applied": already_applied,
        "prompt_id": prompt_id,
        "choice": choice,
        "handle": snapshot.agent_run_id,
        "operation_id": snapshot.operation_id,
        "answered_at_ms": at_ms,
    })
}

fn view_with_answer(snapshot: &OperationSnapshot, answer: Value) -> Value {
    let mut value = serde_json::to_value(AgentRunView::from(snapshot.clone()))
        .unwrap_or_else(|_| json!({"handle": snapshot.agent_run_id}));
    if let Some(object) = value.as_object_mut() {
        object.insert("answer".into(), answer);
    }
    value
}

/// The receipt of an answer already delivered for this very prompt and choice, or `None` when
/// the key should be pressed. A retry is recognised when the prompt is gone from the screen, or
/// when it is still drawn within `REPLAY_WINDOW_MS` of the press (not yet redrawn). The same
/// command prompting again later carries the same id, so an old receipt alone never blocks it.
pub(super) fn replay_receipt(
    snapshot: &OperationSnapshot,
    prompt_id: &str,
    choice: &str,
    prompt_still_pending: bool,
) -> Option<Value> {
    let applied = ledger()
        .lock()
        .get(&(snapshot.operation_id.clone(), prompt_id.to_string()))
        .cloned()?;
    if applied.choice != choice {
        return None;
    }
    if prompt_still_pending && now_ms().saturating_sub(applied.at_ms) > REPLAY_WINDOW_MS {
        return None;
    }
    let answer = answer_block(snapshot, prompt_id, choice, true, applied.at_ms);
    Some(view_with_answer(snapshot, answer))
}

/// Remember that `choice` was delivered for this prompt. Call right after the key is sent and
/// before any bookkeeping that could fail.
pub(super) fn record_applied(snapshot: &OperationSnapshot, prompt_id: &str, choice: &str) {
    let mut ledger = ledger().lock();
    if ledger.len() >= LEDGER_CAP {
        if let Some(oldest) = ledger
            .iter()
            .min_by_key(|(_, applied)| applied.at_ms)
            .map(|(key, _)| key.clone())
        {
            ledger.remove(&oldest);
        }
    }
    ledger.insert(
        (snapshot.operation_id.clone(), prompt_id.to_string()),
        Applied {
            choice: choice.to_string(),
            at_ms: now_ms(),
        },
    );
}

/// After the key went out: record it, then run the bookkeeping (`observe`). A bookkeeping
/// failure cannot undo the keystroke, so it becomes a warning on an applied receipt, never an
/// error.
pub(super) fn finish(
    snapshot: &OperationSnapshot,
    prompt_id: &str,
    choice: &str,
    observe: impl FnOnce(&OperationSnapshot) -> Result<OperationSnapshot, OperationError>,
) -> Value {
    record_applied(snapshot, prompt_id, choice);
    let at_ms = now_ms();
    match observe(snapshot) {
        Ok(updated) => {
            let answer = answer_block(&updated, prompt_id, choice, false, at_ms);
            view_with_answer(&updated, answer)
        }
        Err(error) => {
            let mut answer = answer_block(snapshot, prompt_id, choice, false, at_ms);
            answer["warning"] = Value::String(format!(
                "the answer was delivered to the pane, but updating the run state failed ({}: {}); do not answer again, re-check with inspect_agent",
                error.code, error.message
            ));
            view_with_answer(snapshot, answer)
        }
    }
}

#[cfg(test)]
pub(super) fn age_receipts_for_test(snapshot: &OperationSnapshot, prompt_id: &str) {
    if let Some(applied) = ledger()
        .lock()
        .get_mut(&(snapshot.operation_id.clone(), prompt_id.to_string()))
    {
        applied.at_ms = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{DelegateWorkRequest, StateSource, WorkerPersist};

    fn project() -> String {
        let dir = std::env::temp_dir().join(format!("pm-answer-guard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn run(project: &str, handle: &str, owner: Option<&str>) -> OperationSnapshot {
        let mut req = run_request(project, handle);
        req.owner_session_id = owner.map(str::to_string);
        operations::create_operation(req).unwrap().0
    }

    fn run_request(project: &str, handle: &str) -> DelegateWorkRequest {
        DelegateWorkRequest {
            project_path: project.to_string(),
            task: "t".into(),
            agent_type: "cursor_agent".into(),
            pane_id: Some("pane-x".into()),
            idempotency_key: format!("k-{handle}-{}", uuid::Uuid::new_v4()),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane: true,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some(handle.into()),
            turn_index: 0,
            worker: WorkerPersist::default(),
            context_policy: None,
            checks: Vec::new(),
        }
    }

    fn uniq(prefix: &str) -> String {
        format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
    }

    fn real_observe(
        project: &str,
        snapshot: &OperationSnapshot,
    ) -> Result<OperationSnapshot, OperationError> {
        operations::mark_operation_observation(
            project,
            &snapshot.operation_id,
            Some("running".into()),
            None,
            StateSource::Native,
        )
    }

    #[test]
    fn a_pane_answer_resolves_to_the_run_that_owns_the_pane_so_it_shares_the_replay_ledger() {
        let (default, home) = (project(), project());
        let handle = uniq("run-pane");
        run(&home, &handle, None);
        let (found_project, found_handle) =
            super::super::routes::run_for_pane(&[default.clone(), home.clone()], "pane-x")
                .expect("run bound to the pane");
        assert_eq!((found_project, found_handle), (home, handle));
        assert!(super::super::routes::run_for_pane(&[default], "pane-x").is_none());
    }

    #[test]
    fn concurrent_answers_for_one_prompt_press_the_key_once() {
        let (home, handle) = (project(), uniq("run-conc"));
        let snapshot = run(&home, &handle, None);
        let presses = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (snapshot, presses) = (snapshot.clone(), presses.clone());
                std::thread::spawn(move || {
                    let _serial = serialize_answers();
                    if replay_receipt(&snapshot, "p-conc", "allow_once", true).is_none() {
                        presses.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        record_applied(&snapshot, "p-conc", "allow_once");
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(presses.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn omitted_project_resolves_a_handle_that_lives_in_another_project() {
        // The reported bug: the bridge default project differs from the run's project.
        let (default, home) = (project(), project());
        let handle = uniq("run-omit");
        run(&home, &handle, None);
        let found = resolve_target(&default, None, &handle, None).unwrap();
        assert_eq!(found.project_path, home);
    }

    #[test]
    fn bookkeeping_after_delivery_targets_the_runs_own_project() {
        let (default, home) = (project(), project());
        let handle = uniq("run-book");
        run(&home, &handle, None);
        let snapshot = resolve_target(&default, None, &handle, None).unwrap();
        let out = finish(&snapshot, "p1", "allow_once", |s| real_observe(&s.project_path, s));
        assert_eq!(out["answer"]["applied"], true);
        assert!(out["answer"].get("warning").is_none(), "{out}");
    }

    #[test]
    fn explicit_project_resolves() {
        let home = project();
        let handle = uniq("run-explicit");
        run(&home, &handle, None);
        let found = resolve_target("C:/elsewhere", Some(&home), &handle, None).unwrap();
        assert_eq!(found.agent_run_id, handle);
    }

    #[test]
    fn explicit_project_that_is_not_the_handles_project_is_rejected() {
        let (home, other) = (project(), project());
        let handle = uniq("run-mismatch");
        run(&home, &handle, None);
        let error = resolve_target(&other, Some(&other), &handle, None).unwrap_err();
        assert_eq!(error.code, "PROJECT_MISMATCH");
        assert_eq!(error.context["handle_project"], home);
    }

    #[test]
    fn same_handle_in_two_projects_is_ambiguous_without_a_project() {
        let (a, b, default) = (project(), project(), project());
        let handle = uniq("run-ambig");
        run(&a, &handle, None);
        run(&b, &handle, None);
        let error = resolve_target(&default, None, &handle, None).unwrap_err();
        assert_eq!(error.code, "AMBIGUOUS_HANDLE");
        // An explicit project disambiguates.
        assert!(resolve_target(&default, Some(&a), &handle, None).is_ok());
    }

    fn spellings(dir: &str) -> Vec<String> {
        let mut out = vec![format!("{dir}{}", std::path::MAIN_SEPARATOR)];
        #[cfg(windows)]
        {
            out.push(dir.replace('\\', "/"));
            out.push(dir.to_ascii_uppercase());
            out.push(format!(r"\\?\{dir}"));
        }
        out
    }

    #[test]
    fn equivalent_spellings_of_one_project_are_not_ambiguous() {
        // B2: run_agent with a forward-slash project_path, replayed, then answer_prompt(handle)
        // without a project: the one project was indexed under two spellings.
        let (home, default) = (project(), project());
        for spelling in spellings(&home) {
            let handle = uniq("run-spelling");
            run(&spelling, &handle, None);
            run_turn(&home, &handle, 1);
            let found = resolve_target(&default, None, &handle, None)
                .unwrap_or_else(|e| panic!("{spelling}: {} {}", e.code, e.message));
            assert_eq!(found.agent_run_id, handle);
            // Explicit project in yet another spelling still resolves.
            assert!(resolve_target(&default, Some(&spelling), &handle, None).is_ok());
            assert_eq!(operations::agent_runs_by_project(&default, &handle).unwrap().len(), 1);
        }
    }

    #[test]
    fn project_index_stores_one_entry_per_project() {
        let home = project();
        let handle = uniq("run-index");
        for (turn, spelling) in spellings(&home).iter().enumerate() {
            run_turn(spelling, &handle, turn as u32);
        }
        let key = crate::project_path::comparable_project_path(std::path::Path::new(&home));
        let entries = operations::indexed_project_keys_for_test(&key);
        assert_eq!(entries, 1, "one index entry per project, got {entries}");
    }

    #[test]
    fn distinct_projects_stay_ambiguous_even_when_spelled_differently() {
        let (a, b, default) = (project(), project(), project());
        let handle = uniq("run-distinct");
        run(&a, &handle, None);
        for (turn, spelling) in spellings(&b).iter().enumerate() {
            run_turn(spelling, &handle, turn as u32);
        }
        let error = resolve_target(&default, None, &handle, None).unwrap_err();
        assert_eq!(error.code, "AMBIGUOUS_HANDLE");
        assert_eq!(error.context["projects"].as_array().unwrap().len(), 2);
    }

    fn run_turn(project: &str, handle: &str, turn: u32) -> OperationSnapshot {
        let mut req = run_request(project, handle);
        req.turn_index = turn;
        operations::create_operation(req).unwrap().0
    }

    #[test]
    fn a_run_owned_by_another_connection_cannot_be_answered() {
        let (home, default) = (project(), project());
        let handle = uniq("run-foreign");
        run(&home, &handle, Some("session-owner"));
        let error = resolve_target(&default, None, &handle, Some("session-intruder")).unwrap_err();
        assert_eq!(error.code, "AUTHORIZATION_DENIED");
        assert!(resolve_target(&default, None, &handle, Some("session-owner")).is_ok());
    }

    #[test]
    fn duplicate_submission_replays_the_receipt_without_a_second_keystroke() {
        let home = project();
        let handle = uniq("run-dup");
        let snapshot = run(&home, &handle, None);
        assert!(replay_receipt(&snapshot, "p1", "allow_once", false).is_none());
        record_applied(&snapshot, "p1", "allow_once");
        let replay = replay_receipt(&snapshot, "p1", "allow_once", false).expect("replay");
        assert_eq!(replay["answer"]["already_applied"], true);
        assert_eq!(replay["answer"]["applied"], true);
        // Still drawn straight after the press (the screen has not redrawn): still a replay.
        assert!(replay_receipt(&snapshot, "p1", "allow_once", true).is_some());
        // A different prompt, or a different choice, is not a replay.
        assert!(replay_receipt(&snapshot, "p2", "allow_once", false).is_none());
        assert!(replay_receipt(&snapshot, "p1", "deny", false).is_none());
    }

    #[test]
    fn the_same_command_prompting_again_later_is_a_new_prompt() {
        let home = project();
        let snapshot = run(&home, &uniq("run-again"), None);
        record_applied(&snapshot, "p1", "allow_once");
        age_receipts_for_test(&snapshot, "p1");
        // Old receipt, and the identical prompt is on screen again: answer it for real.
        assert!(replay_receipt(&snapshot, "p1", "allow_once", true).is_none());
        // Old receipt and nothing on screen: the earlier answer is what a retry means.
        assert!(replay_receipt(&snapshot, "p1", "allow_once", false).is_some());
    }

    #[test]
    fn a_failed_bookkeeping_step_still_reports_the_approval_as_applied() {
        let home = project();
        let snapshot = run(&home, &uniq("run-warn"), None);
        let out = finish(&snapshot, "p1", "allow_once", |_| {
            Err(OperationError::new("OPERATION_NOT_FOUND", "gone", false))
        });
        assert_eq!(out["answer"]["applied"], true);
        assert_eq!(out["answer"]["prompt_id"], "p1");
        assert!(out["answer"]["warning"].as_str().unwrap().contains("OPERATION_NOT_FOUND"));
        // The delivered answer is on record, so a retry is a replay rather than a second press.
        assert!(replay_receipt(&snapshot, "p1", "allow_once", false).is_some());
    }

}
