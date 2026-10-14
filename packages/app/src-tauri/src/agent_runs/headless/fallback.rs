//! Visible-first runs and their automatic headless fallback.
//!
//! A run asks for a visible pane by default. It runs headless instead, recorded as
//! `fallback: {from: "visible", to: "headless", reason}` on the receipt, when
//! - `no_visible_mode`: the agent has no visible mode but can run headless, or
//! - `visible_failed`: the pane failed to launch or the prompt was not delivered before the turn
//!   started (`PROMPT_NOT_SUBMITTED`, `DISPATCH_FAILED`, `PANE_SPAWN_FAILED`).
//!
//! The same operation and handle are converted in place (`operations::convert_to_headless_fallback`),
//! so waiters keep waiting and the task is never run twice: nothing falls back once a turn started.

use super::process::{execute_headless, fail_start, publish};
use crate::operations::{self, OperationSnapshot};
use crate::pty::PaneRegistry;
use crate::tui::Modes;
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;

pub const NO_VISIBLE_MODE: &str = "no_visible_mode";
pub const VISIBLE_FAILED: &str = "visible_failed";

/// Codes that mean the turn never started (nothing ran, so a headless retry cannot duplicate work).
pub(crate) fn is_pre_turn_failure(code: &str) -> bool {
    matches!(code, "PROMPT_NOT_SUBMITTED" | "DISPATCH_FAILED" | "PANE_SPAWN_FAILED")
}

/// A run with no pane named by the caller: the app spawns (or would spawn) its own pane.
fn app_owned_pane(snapshot: &OperationSnapshot) -> bool {
    snapshot.pane_id.is_none() || snapshot.pane_created
}

/// Whether a visible run that wants a pane should start headless because the agent has none.
pub(crate) fn no_visible_mode(modes: Modes, headless_requested: bool, pane_supplied: bool) -> bool {
    !headless_requested && !pane_supplied && !modes.visible && modes.headless
}

/// The fallback reason for an operation, or None when it must stay as it is. `error_code` is the
/// failure that ended (or would end) the visible attempt; None asks only about a missing visible mode.
pub(crate) fn plan(
    snapshot: &OperationSnapshot,
    modes: Modes,
    error_code: Option<&str>,
) -> Option<&'static str> {
    if snapshot.worker.fallback.is_some()
        || !modes.headless
        || snapshot.agent_type == "opencode_native"
        || !app_owned_pane(snapshot)
    {
        return None;
    }
    match error_code {
        None if !modes.visible => Some(NO_VISIBLE_MODE),
        Some(code) if is_pre_turn_failure(code) => Some(VISIBLE_FAILED),
        _ => None,
    }
}

/// Convert the operation to a queued headless one and close the failed pane the run created.
/// True when converted: the caller must then call `start_headless` once the dispatch has unwound.
pub(crate) fn convert(
    snapshot: &OperationSnapshot,
    error_code: Option<&str>,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> bool {
    let modes = crate::tui::run_modes(&snapshot.agent_type);
    let Some(reason) = plan(snapshot, modes, error_code) else {
        return false;
    };
    let Some(converted) = operations::convert_to_headless_fallback(
        &snapshot.project_path,
        &snapshot.operation_id,
        reason,
    ) else {
        return false;
    };
    if snapshot.pane_created {
        if let Some(pane_id) = snapshot.pane_id.as_deref() {
            let _ = crate::pty::registry_kill_pane_with_reason(
                registry,
                pane_id,
                Some("visible_failed_fallback"),
            );
        }
    }
    let _ = crate::bridge::release_operation_locks(&converted);
    tracing::info!(
        operation_id = %converted.operation_id,
        reason,
        "visible run falls back to headless"
    );
    true
}

/// Launch the headless child for an operation `convert` just re-queued.
pub(crate) fn start_headless(
    project: &str,
    operation_id: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &tauri::AppHandle,
) {
    let Ok(snapshot) = operations::get_operation(project, operation_id) else {
        return;
    };
    if let Err(error) = crate::bridge::fail_operation_on_lock_conflict(&snapshot) {
        tracing::warn!(code = %error.code, "fallback run lost its locks");
        publish(project, operation_id, registry, app);
        return;
    }
    let (project, operation_id) = (project.to_string(), operation_id.to_string());
    let (registry, app) = (registry.clone(), app.clone());
    let spawn = thread::Builder::new()
        .name("puppet-agent-headless".into())
        .spawn({
            let (project, operation_id, registry, app) =
                (project.clone(), operation_id.clone(), registry.clone(), app.clone());
            move || {
                execute_headless(
                    project,
                    operation_id,
                    snapshot.agent_type,
                    snapshot.task,
                    snapshot.read_only,
                    snapshot.timeout_ms,
                    registry,
                    app,
                );
            }
        });
    if let Err(error) = spawn {
        fail_start(&project, &operation_id, "THREAD_START_FAILED", &error.to_string());
        publish(&project, &operation_id, &registry, &app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{DelegateWorkRequest, RunFallback};

    fn modes(visible: bool, headless: bool) -> Modes {
        Modes { headless, visible }
    }

    fn operation(agent: &str, pane: Option<(&str, bool)>) -> OperationSnapshot {
        let project = std::env::temp_dir().join(format!("pm-fallback-{}", uuid::Uuid::new_v4()));
        let request = DelegateWorkRequest {
            project_path: project.to_string_lossy().into_owned(),
            task: "do it".into(),
            agent_type: agent.into(),
            pane_id: pane.map(|(id, _)| id.to_string()),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane: false,
            placement: None,
            owner_session_id: Some("fallback-test".into()),
            worker_has_mcp_tools: false,
            agent_run_id: None,
            turn_index: 0,
            worker: Default::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        let mut op = operations::create_operation(request).unwrap().0;
        if let Some((_, created)) = pane {
            op.pane_created = created;
        }
        op
    }

    #[test]
    fn missing_visible_mode_falls_back_only_when_headless_can_run() {
        assert!(no_visible_mode(modes(false, true), false, false));
        assert!(!no_visible_mode(modes(true, true), false, false), "visible agents stay visible");
        assert!(!no_visible_mode(modes(false, false), false, false), "no headless mode: keep the original error");
        assert!(!no_visible_mode(modes(false, true), true, false), "explicit headless is not a fallback");
        assert!(!no_visible_mode(modes(false, true), false, true), "a named pane is never replaced");
    }

    #[test]
    fn only_pre_turn_failures_are_retried_headless() {
        let op = operation("claude", None);
        let m = modes(true, true);
        for code in ["PROMPT_NOT_SUBMITTED", "DISPATCH_FAILED", "PANE_SPAWN_FAILED"] {
            assert_eq!(plan(&op, m, Some(code)), Some(VISIBLE_FAILED), "{code}");
        }
        for code in ["TIMEOUT", "RESULT_NOT_CAPTURED", "ACCEPTANCE_FAILED", "PANE_BUSY", "CANCELLED", "WORKSPACE_TRUST_DECLINED", "STARTUP_SCREEN_NOT_COMPLETED", "PANE_BLOCKED_BY_PROMPT"] {
            assert_eq!(plan(&op, m, Some(code)), None, "{code} may follow a started turn");
        }
        assert_eq!(plan(&op, modes(false, true), None), Some(NO_VISIBLE_MODE));
        assert_eq!(plan(&op, m, None), None);
    }

    #[test]
    fn no_fallback_without_headless_for_adopted_panes_for_native_or_twice() {
        let spawned = operation("claude", Some(("pane-1", true)));
        assert_eq!(plan(&spawned, modes(true, true), Some("DISPATCH_FAILED")), Some(VISIBLE_FAILED));
        assert_eq!(plan(&spawned, modes(true, false), Some("DISPATCH_FAILED")), None);
        let adopted = operation("claude", Some(("user-pane", false)));
        assert_eq!(plan(&adopted, modes(true, true), Some("DISPATCH_FAILED")), None);
        let native = operation("opencode_native", None);
        assert_eq!(plan(&native, modes(true, true), Some("DISPATCH_FAILED")), None);
        let mut again = operation("claude", None);
        again.worker.fallback = Some(RunFallback::to_headless(VISIBLE_FAILED));
        assert_eq!(plan(&again, modes(true, true), Some("DISPATCH_FAILED")), None);
    }

    fn mark(op: &OperationSnapshot, status: operations::OperationStatus) {
        operations::mark_operation_state(
            &op.project_path,
            &op.operation_id,
            status,
            operations::StateSource::Inferred,
            Some("x".into()),
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn conversion_requeues_the_same_operation_headless_and_records_the_fallback() {
        let op = operation("claude", None);
        mark(&op, operations::OperationStatus::Starting);
        let converted = operations::convert_to_headless_fallback(
            &op.project_path,
            &op.operation_id,
            VISIBLE_FAILED,
        )
        .expect("a starting visible run converts");
        assert_eq!(converted.operation_id, op.operation_id);
        assert_eq!(converted.status, operations::OperationStatus::Queued);
        assert!(converted.pane_id.is_none() && !converted.pane_created && converted.error.is_none());
        assert_eq!(converted.worker.fallback, Some(RunFallback::to_headless(VISIBLE_FAILED)));
        assert!(
            operations::convert_to_headless_fallback(&op.project_path, &op.operation_id, VISIBLE_FAILED)
                .is_none(),
            "a second fallback is refused"
        );
        let failed = operation("claude", None);
        mark(&failed, operations::OperationStatus::Failed);
        assert!(
            operations::convert_to_headless_fallback(
                &failed.project_path,
                &failed.operation_id,
                VISIBLE_FAILED
            )
            .is_none(),
            "a terminal run is never revived"
        );
    }
}
