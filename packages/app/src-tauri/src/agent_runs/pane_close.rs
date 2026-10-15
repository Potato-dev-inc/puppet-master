//! Central policy for when a run may close or kill a worker pane.

use crate::operations::{OperationError, OperationSnapshot};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

/// `pane_created` on the operation — true only when dispatch spawned this pane for the run.
pub fn spawned_by_run(snapshot: &OperationSnapshot) -> bool {
    snapshot.pane_created
}

pub fn should_dispose_after_terminal(snapshot: &OperationSnapshot) -> bool {
    spawned_by_run(snapshot) && !snapshot.keep_pane
}

fn pane_busy(
    project: &str,
    pane_id: &str,
    excluding_operation_id: &str,
) -> Result<bool, OperationError> {
    Ok(crate::operations::conflicting_pane_operation(project, pane_id, excluding_operation_id)?
        .is_some())
}

/// A run's isolated worktree that nobody can have a use for any more: the run is over (its pane is
/// closed or failed) and the agent changed nothing in it. Discards the checkout and its `pm/*`
/// branch and marks it discarded on the run. A worktree with changes is never touched (the Changes
/// tab is where those are merged or discarded). Returns whether one was removed.
///
/// Live (VERIFY-R13 #1): a Codex run that never got past the trust screen failed, and closing its
/// pane left the empty worktree and branch behind with nothing in the app pointing at them.
pub fn release_pristine_worktree(snapshot: &OperationSnapshot) -> bool {
    let Some(worktree) = snapshot.worker.worktree.as_ref().filter(|worktree| worktree.closed.is_none()) else {
        return false;
    };
    if !crate::worktrees::discard_if_pristine(worktree) {
        return false;
    }
    let handle = if snapshot.agent_run_id.is_empty() { worktree.handle.as_str() } else { snapshot.agent_run_id.as_str() };
    if let Ok(closed) = crate::operations::close_worktree(&snapshot.project_path, handle, "discarded") {
        crate::bridge::push_operation_sse(&closed);
    }
    true
}

/// Automatic cleanup after a terminal operation (completion, failure, cancel publish).
pub fn maybe_dispose_after_terminal(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<(), OperationError> {
    if !should_dispose_after_terminal(snapshot) {
        return Ok(());
    }
    let pane_id = snapshot
        .pane_id
        .as_deref()
        .ok_or_else(|| OperationError::new("PANE_CLEANUP_FAILED", "missing pane_id", true))?;
    if !registry.lock().panes.contains_key(pane_id) {
        return Ok(());
    }
    if pane_busy(&snapshot.project_path, pane_id, &snapshot.operation_id)? {
        tracing::info!(
            pane_id,
            operation_id = %snapshot.operation_id,
            "skipping automatic pane dispose because pane is busy"
        );
        return Ok(());
    }
    // The turn summary of the run's last turn reads this pane (files changed, receipt) after the
    // end-of-turn checkpoint settles; closing the pane first used to lose the summary altogether.
    if !crate::turn_summary::wait_for_summary(pane_id, crate::turn_summary::DISPOSE_WAIT) {
        tracing::warn!(
            pane_id,
            operation_id = %snapshot.operation_id,
            "closing the pane before its turn summary finished (waited the limit)"
        );
    }
    crate::pty::registry_kill_pane_with_reason(
        registry,
        pane_id,
        Some("operation_terminal_cleanup"),
    )
    .map_err(|error| OperationError::new("PANE_CLEANUP_FAILED", error, true))?;
    // A run that ended without a result leaves no worktree behind when it changed nothing (a
    // completed one keeps its, even empty: the Receipt/Changes tabs still read it).
    if snapshot.status != crate::operations::OperationStatus::Completed {
        release_pristine_worktree(snapshot);
    }
    Ok(())
}

/// Dispatch failed after a pane was spawned for this operation.
pub fn maybe_dispose_on_dispatch_failure(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<(), OperationError> {
    // keep_pane=true means the caller keeps the pane until close_agent, failures included
    // (a PROMPT_NOT_SUBMITTED pane is exactly what they want to look at and retry in).
    if !spawned_by_run(snapshot) || snapshot.keep_pane {
        return Ok(());
    }
    let pane_id = match snapshot.pane_id.as_deref() {
        Some(id) => id,
        None => return Ok(()),
    };
    if pane_busy(&snapshot.project_path, pane_id, &snapshot.operation_id)? {
        tracing::info!(
            pane_id,
            operation_id = %snapshot.operation_id,
            "skipping dispatch-failure pane dispose because pane is busy"
        );
        return Ok(());
    }
    let killed = crate::pty::registry_kill_pane_with_reason(
        registry,
        pane_id,
        Some("dispatch_failed_cleanup"),
    );
    release_pristine_worktree(snapshot);
    killed.map_err(|error| OperationError::new("PANE_CLEANUP_FAILED", error, true))?;
    Ok(())
}

/// Explicit `close_agent` dispose — only spawned panes may be killed.
pub fn dispose_explicit_close(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Result<(), OperationError> {
    if !spawned_by_run(snapshot) {
        return Ok(());
    }
    let pane_id = snapshot
        .pane_id
        .as_deref()
        .ok_or_else(|| OperationError::new("PANE_NOT_FOUND", "missing pane_id", false))?;
    let _ = crate::pty::registry_kill_pane_with_reason(
        registry,
        pane_id,
        Some("close_agent"),
    );
    // Closing the run: an isolated worktree it never changed is garbage, not work to keep.
    release_pristine_worktree(snapshot);
    Ok(())
}

/// Worker stop for cancel / timeout — never kill adopted panes; optional kill for spawned.
pub fn stop_worker_control(
    snapshot: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    allow_kill_spawned_pane: bool,
) -> Result<(), OperationError> {
    let pane_id = snapshot
        .pane_id
        .as_deref()
        .ok_or_else(|| OperationError::new("PANE_NOT_FOUND", "missing pane_id", false))?;
    if snapshot.agent_type == "opencode_native" {
        let link = {
            let guard = registry.lock();
            guard
                .panes
                .get(pane_id)
                .and_then(|pane| pane.opencode.as_ref())
                .map(|link| {
                    (
                        link.base_url.clone(),
                        link.session_id.clone(),
                        link.directory.clone(),
                    )
                })
        }
        .ok_or_else(|| {
            OperationError::new(
                "CANCEL_FAILED",
                "native session control is unavailable",
                true,
            )
        })?;
        super::runtime::settle_native_abort(
            &snapshot.project_path,
            &snapshot.operation_id,
            crate::opencode::client::abort_session(&link.0, &link.1, Some(&link.2)),
        )
        .map_err(|err| OperationError::new("CANCEL_FAILED", err.message, true))?;
    }
    if spawned_by_run(snapshot) && allow_kill_spawned_pane {
        if !pane_busy(&snapshot.project_path, pane_id, &snapshot.operation_id)? {
            crate::pty::registry_kill_pane_with_reason(
                registry,
                pane_id,
                Some("cancel_kill_spawned_pane"),
            )
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, true))?;
        }
    } else if snapshot.agent_type != "opencode_native" {
        let sequence = crate::pty::keys::sequence("ctrl+c")
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, false))?;
        crate::pty::registry_write_input(registry, app, pane_id, &sequence, false, false, None, crate::pty::WriteOrigin::Nudge)
            .map_err(|err| OperationError::new("CANCEL_FAILED", err, true))?;
    }
    Ok(())
}

pub fn bound_pane_missing_error(pane_id: &str) -> OperationError {
    let mut error = OperationError::new(
        "PANE_GONE",
        format!(
            "bound pane {} is no longer in the worker registry; the pane was closed or the app restarted",
            pane_id
        ),
        true,
    );
    error.context = serde_json::json!({
        "pane_id": pane_id,
        "recovery": "call list_agents or list_workers and bind a live pane_id or worker_id; do not assume the old pane_id still exists",
    });
    error
}

/// Error for a pane this app closed on its own when a run finished with `keep_pane=false`.
pub fn pane_closed_after_run_error(pane_id: &str, handle: &str) -> OperationError {
    let mut error = OperationError::new(
        "PANE_CLOSED_AFTER_RUN",
        format!(
            "pane {pane_id} was closed automatically when run {handle} finished because it was started with keep_pane=false (the default for panes the run created). take_over, switch_agent_model and live pane inspection need a live pane: start the run with keep_pane=true (and call close_agent when done). agent_transcript and inspect_agent still work for handle {handle} from the stored transcript."
        ),
        false,
    );
    error.context = serde_json::json!({
        "pane_id": pane_id,
        "handle": handle,
        "reason": "keep_pane_false",
        "recovery": "re-run with keep_pane=true; read output with agent_transcript",
    });
    error
}

/// Pure check: is `pane_id` absent because a terminal run with `keep_pane=false` disposed it?
pub fn closed_after_run_in(
    snapshots: &[OperationSnapshot],
    pane_id: &str,
    pane_alive: bool,
) -> Option<OperationError> {
    if pane_alive {
        return None;
    }
    snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.pane_id.as_deref() == Some(pane_id)
                && snapshot.status.terminal()
                && should_dispose_after_terminal(snapshot)
        })
        .max_by_key(|snapshot| snapshot.turn_index)
        .map(|snapshot| {
            let handle = if snapshot.agent_run_id.is_empty() {
                &snapshot.operation_id
            } else {
                &snapshot.agent_run_id
            };
            pane_closed_after_run_error(pane_id, handle)
        })
}

/// Registry-backed variant used by routes that need a live pane.
pub fn closed_after_run_error(
    project: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Option<OperationError> {
    let alive = registry.lock().panes.contains_key(pane_id);
    if alive {
        return None;
    }
    let snapshots = crate::operations::list_operations(project).ok()?;
    closed_after_run_in(&snapshots, pane_id, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{DelegateWorkRequest, OperationStatus, WorkerPersist};
    use crate::pty::PaneRegistry;
    use std::sync::Arc;

    fn snapshot_with_pane(
        project: &str,
        pane_id: &str,
        pane_created: bool,
        keep_pane: bool,
    ) -> OperationSnapshot {
        std::fs::create_dir_all(project).expect("project dir");
        let req = DelegateWorkRequest {
            project_path: project.to_string(),
            task: "t".into(),
            agent_type: "opencode_native".into(),
            pane_id: Some(pane_id.to_string()),
            idempotency_key: "k".into(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane,
            placement: None,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some("handle".into()),
            turn_index: 0,
            worker: WorkerPersist::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        let (mut op, _) = crate::operations::create_operation(req).unwrap();
        op.pane_created = pane_created;
        op.status = OperationStatus::Completed;
        op
    }

    fn git_repo_with_worktree(handle: &str) -> (String, crate::worktrees::WorktreeRef) {
        let project = std::env::temp_dir().join(format!("pm-wt-close-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git").args(args).current_dir(&project).output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "T"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
            git(&["config", key, value]);
        }
        std::fs::write(project.join("a.txt"), "alpha
").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "initial"]);
        let project = project.to_string_lossy().into_owned();
        let worktree = crate::worktrees::create(&project, handle).unwrap();
        (project, worktree)
    }

    fn branch_exists(project: &str, branch: &str) -> bool {
        let out = std::process::Command::new("git").args(["branch", "--list", branch]).current_dir(project).output().unwrap();
        !String::from_utf8_lossy(&out.stdout).trim().is_empty()
    }

    #[test]
    fn closing_a_run_discards_the_worktree_it_never_changed_and_keeps_one_with_work() {
        // VERIFY-R13 #1: a Codex run stuck on the trust screen was closed; its worktree and pm/*
        // branch stayed behind.
        let (project, worktree) = git_repo_with_worktree("handle");
        let mut snapshot = snapshot_with_pane(&project, "wt-pane", true, true);
        snapshot.worker.worktree = Some(worktree.clone());
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        dispose_explicit_close(&snapshot, &registry).unwrap();
        assert!(!std::path::Path::new(&worktree.path).exists(), "the empty checkout is gone");
        assert!(!branch_exists(&project, &worktree.branch), "and its branch");

        // A worktree the agent changed is work: closing the pane never throws it away.
        let (project, worktree) = git_repo_with_worktree("handle");
        std::fs::write(std::path::Path::new(&worktree.path).join("new.txt"), "work
").unwrap();
        let mut snapshot = snapshot_with_pane(&project, "wt-pane", true, true);
        snapshot.worker.worktree = Some(worktree.clone());
        dispose_explicit_close(&snapshot, &registry).unwrap();
        assert!(std::path::Path::new(&worktree.path).join("new.txt").is_file());
        assert!(branch_exists(&project, &worktree.branch));
    }

    #[test]
    fn a_failed_dispatch_does_not_leave_its_empty_worktree_behind() {
        let (project, worktree) = git_repo_with_worktree("handle");
        let mut snapshot = snapshot_with_pane(&project, "wt-pane", true, false);
        snapshot.worker.worktree = Some(worktree.clone());
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        maybe_dispose_on_dispatch_failure(&snapshot, &registry).unwrap();
        assert!(!std::path::Path::new(&worktree.path).exists());
        assert!(!branch_exists(&project, &worktree.branch));
    }

    #[test]
    fn automatic_dispose_skips_adopted_pane_even_when_keep_pane_false() {
        let project = std::env::temp_dir()
            .join(format!("pm-wp9-adopted-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let pane_id = "ui-pane";
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        registry
            .lock()
            .panes
            .insert(pane_id.to_string(), PaneRegistry::test_pane_stub(pane_id));
        let snapshot = snapshot_with_pane(&project, pane_id, false, false);
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        assert!(
            registry.lock().panes.contains_key(pane_id),
            "adopted pane must survive terminal cleanup"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn automatic_dispose_kills_idle_spawned_pane_when_keep_false() {
        let project = std::env::temp_dir()
            .join(format!("pm-wp9-spawned-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let pane_id = "spawned-pane";
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        registry
            .lock()
            .panes
            .insert(pane_id.to_string(), PaneRegistry::test_pane_stub(pane_id));
        let snapshot = snapshot_with_pane(&project, pane_id, true, false);
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        assert!(
            !registry.lock().panes.contains_key(pane_id),
            "spawned pane should be disposed when keep_pane is false"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    /// Orders what happens live when a run completes: the pane's status event starts the turn summary
    /// (which still has to read the pane), and the operation's terminal publish disposes the pane.
    #[test]
    fn automatic_dispose_waits_for_the_turn_summary_that_still_reads_the_pane() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let project = std::env::temp_dir()
            .join(format!("pm-summary-dispose-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let pane_id = "summary-pane";
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        registry
            .lock()
            .panes
            .insert(pane_id.to_string(), PaneRegistry::test_pane_stub(pane_id));
        let snapshot = snapshot_with_pane(&project, pane_id, true, false);

        // The status event: a summary of this pane is in flight; its worker reads the pane late.
        let in_flight = crate::turn_summary::summary_in_flight(pane_id);
        let pane_was_there = Arc::new(AtomicBool::new(false));
        let worker = {
            let (registry, pane_was_there) = (registry.clone(), pane_was_there.clone());
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                pane_was_there.store(registry.lock().panes.contains_key("summary-pane"), Ordering::SeqCst);
                drop(in_flight);
            })
        };
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        worker.join().unwrap();
        assert!(
            pane_was_there.load(Ordering::SeqCst),
            "the pane was closed before the turn summary had read it"
        );
        assert!(
            !registry.lock().panes.contains_key(pane_id),
            "the spawned pane is still closed afterwards"
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn automatic_dispose_does_not_wait_when_no_summary_is_in_flight() {
        let project = std::env::temp_dir()
            .join(format!("pm-summary-none-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let pane_id = "no-summary-pane";
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        registry
            .lock()
            .panes
            .insert(pane_id.to_string(), PaneRegistry::test_pane_stub(pane_id));
        let snapshot = snapshot_with_pane(&project, pane_id, true, false);
        let started = std::time::Instant::now();
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(!registry.lock().panes.contains_key(pane_id));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn dispatch_failure_keeps_a_spawned_pane_when_keep_pane_is_true_and_kills_it_otherwise() {
        for (keep, survives) in [(true, true), (false, false)] {
            let project = std::env::temp_dir()
                .join(format!("pm-keep-dispatch-{}", uuid::Uuid::new_v4()))
                .to_string_lossy()
                .into_owned();
            let registry = Arc::new(Mutex::new(PaneRegistry::default()));
            registry
                .lock()
                .panes
                .insert("p".to_string(), PaneRegistry::test_pane_stub("p"));
            let snapshot = snapshot_with_pane(&project, "p", true, keep);
            maybe_dispose_on_dispatch_failure(&snapshot, &registry).unwrap();
            assert_eq!(registry.lock().panes.contains_key("p"), survives, "keep_pane={keep}");
            let _ = std::fs::remove_dir_all(&project);
        }
    }

    #[test]
    fn spawned_by_run_tracks_pane_created() {
        let project = std::env::temp_dir()
            .join(format!("pm-wp9-track-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let snap = snapshot_with_pane(&project, "p", true, false);
        assert!(spawned_by_run(&snap));
        let adopted = snapshot_with_pane(&project, "p", false, false);
        assert!(!spawned_by_run(&adopted));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn bound_pane_missing_error_is_recoverable_pane_gone() {
        let err = bound_pane_missing_error("pane-abc");
        assert_eq!(err.code, "PANE_GONE");
        assert!(err.recoverable);
    }

    #[test]
    fn closed_after_run_reports_pane_closed_after_run_only_for_disposed_panes() {
        let project = std::env::temp_dir()
            .join(format!("pm-closed-after-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let spawned = snapshot_with_pane(&project, "p1", true, false);
        let err = closed_after_run_in(&[spawned.clone()], "p1", false).expect("closed after run");
        assert_eq!(err.code, "PANE_CLOSED_AFTER_RUN");
        assert!(err.message.contains("keep_pane=true"));
        assert!(err.message.contains("agent_transcript"));
        assert!(closed_after_run_in(&[spawned], "p1", true).is_none());
        let kept = snapshot_with_pane(&format!("{project}-kept"), "p1", true, true);
        assert!(closed_after_run_in(&[kept], "p1", false).is_none());
        let adopted = snapshot_with_pane(&format!("{project}-adopted"), "p1", false, false);
        assert!(closed_after_run_in(&[adopted], "p1", false).is_none());
        for suffix in ["", "-kept", "-adopted"] {
            let _ = std::fs::remove_dir_all(format!("{project}{suffix}"));
        }
    }
}
