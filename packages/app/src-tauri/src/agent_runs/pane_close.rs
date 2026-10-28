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
    // A run handed over to another agent (Keep going): the new agent works in this copy now, so its
    // copy is never discarded when the old pane closes.
    if snapshot.error.as_ref().is_some_and(|error| error.code == crate::operations::HANDED_OFF) {
        return false;
    }
    // A run that joined a copy someone else made (a mission's agents share one) never discards
    // it: the copy's owner decides.
    if !snapshot.agent_run_id.is_empty() && !worktree.handle.is_empty() && worktree.handle != snapshot.agent_run_id {
        return false;
    }
    if !crate::worktrees::discard_if_pristine(worktree) {
        return false;
    }
    let handle = if snapshot.agent_run_id.is_empty() { worktree.handle.as_str() } else { snapshot.agent_run_id.as_str() };
    if let Ok(closed) = crate::operations::close_worktree(&snapshot.project_path, handle, "discarded") {
        crate::bridge::push_operation_sse(&closed);
    }
    true
}

/// A pane closed: when it ran in an isolated run's worktree and that run ended without a result
/// (declined trust, cancelled, failed), the worktree it never changed goes with it, through
/// [`release_pristine_worktree`]. The pane is kept while it is open (keep_pane: the person may want
/// to look at it), and its shell's working folder is the worktree, which Windows will not delete
/// under it; once the pane is closed nothing holds it. A run that completed keeps its worktree (the
/// Receipt and Changes tabs read it); a worktree with changes is never touched.
pub fn release_worktree_of_closed_pane(pane_id: &str, cwd: &str) -> bool {
    let Some(origin) = crate::worktrees::origin_of_cwd(cwd) else { return false };
    let Ok(handle) = super::latest_handle_for_pane(&origin, pane_id) else { return false };
    let Ok(snapshot) = crate::operations::latest_agent_run(&origin, &handle) else { return false };
    if snapshot.pane_id.as_deref() != Some(pane_id)
        || !matches!(snapshot.status, crate::operations::OperationStatus::Failed | crate::operations::OperationStatus::Cancelled)
    {
        return false;
    }
    release_pristine_worktree(&snapshot)
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

    /// Live v0.2.0 hunt: "Don't trust" (or Cancel) on an isolated run kept its pane, as it should, but
    /// its empty worktree stayed until the run was closed through close_agent. Closing the pane itself
    /// now takes it away; a completed run, or a worktree with work, keeps it.
    #[test]
    fn closing_the_pane_of_a_declined_run_removes_its_empty_worktree() {
        let wait_gone = |path: &str| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::path::Path::new(path).exists() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            !std::path::Path::new(path).exists()
        };
        let run_in = |project: &str, worktree: &crate::worktrees::WorktreeRef, pane: &str, status: OperationStatus| {
            let request = DelegateWorkRequest {
                project_path: project.to_string(),
                task: "t".into(),
                agent_type: "codex".into(),
                pane_id: Some(pane.to_string()),
                idempotency_key: uuid::Uuid::new_v4().to_string(),
                acceptance_criteria: None,
                task_id: None,
                exclusive: false,
                locks: vec![],
                timeout_ms: 60_000,
                read_only: false,
                keep_pane: true,
                placement: None,
                owner_session_id: None,
                worker_has_mcp_tools: false,
                // The run's own copy: a real isolated run's worktree carries the run's handle. A different
                // handle means the run joined someone else's copy (a mission's), which it never discards.
                agent_run_id: Some(worktree.handle.clone()),
                turn_index: 0,
                worker: WorkerPersist { worktree: Some(worktree.clone()), ..WorkerPersist::default() },
                context_policy: None,
                checks: Vec::new(),
            };
            let (op, _) = crate::operations::create_operation(request).unwrap();
            if status == OperationStatus::Completed {
                crate::operations::mark_operation_state(project, &op.operation_id, OperationStatus::Starting, crate::operations::StateSource::Inferred, Some("dispatching".into()), None, None).unwrap();
                crate::operations::mark_operation_state(project, &op.operation_id, OperationStatus::Running, crate::operations::StateSource::Inferred, Some("running".into()), None, None).unwrap();
                crate::operations::mark_operation_finished(project, &op.operation_id, Some("done".into()), crate::operations::ResultCapture::Inferred, false).unwrap();
            } else {
                crate::operations::mark_operation_state(project, &op.operation_id, status, crate::operations::StateSource::Inferred, Some("done".into()), None, None).unwrap();
            }
            let registry = Arc::new(Mutex::new(PaneRegistry::default()));
            let mut stub = PaneRegistry::test_pane_stub(pane);
            stub.info.cwd = worktree.path.clone();
            registry.lock().panes.insert(pane.to_string(), stub);
            registry
        };

        let (project, worktree) = git_repo_with_worktree("handle");
        let registry = run_in(&project, &worktree, "declined-pane", OperationStatus::Failed);
        assert!(std::path::Path::new(&worktree.path).exists(), "kept while the pane is open");
        crate::pty::registry_kill_pane(&registry, "declined-pane").unwrap();
        assert!(wait_gone(&worktree.path), "the empty checkout goes with the pane");
        // The branch is deleted right after the folder, on the same background thread.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while branch_exists(&project, &worktree.branch) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!branch_exists(&project, &worktree.branch), "and its branch");

        // A completed run keeps its worktree; so does a declined one the agent wrote in.
        let (project, worktree) = git_repo_with_worktree("handle");
        let registry = run_in(&project, &worktree, "done-pane", OperationStatus::Completed);
        crate::pty::registry_kill_pane(&registry, "done-pane").unwrap();
        assert!(!release_worktree_of_closed_pane("done-pane", &worktree.path));
        assert!(std::path::Path::new(&worktree.path).exists());

        let (project, worktree) = git_repo_with_worktree("handle");
        std::fs::write(std::path::Path::new(&worktree.path).join("new.txt"), "work\n").unwrap();
        let registry = run_in(&project, &worktree, "work-pane", OperationStatus::Cancelled);
        crate::pty::registry_kill_pane(&registry, "work-pane").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(std::path::Path::new(&worktree.path).join("new.txt").is_file());
    }

    /// Keep going: a run handed over to another agent is cancelled with `HANDED_OFF`. Its copy is
    /// empty (the old agent never changed it), but the new agent now works in it: closing the old pane
    /// must not discard it. The control run (no hand-over) with the same empty copy is discarded.
    #[test]
    fn a_pristine_copy_of_a_handed_off_run_is_kept_when_its_pane_closes() {
        let (project, worktree) = git_repo_with_worktree("handle");
        let mut snapshot = snapshot_with_pane(&project, "hand-pane", true, true);
        snapshot.worker.worktree = Some(worktree.clone());
        snapshot.status = OperationStatus::Cancelled;
        snapshot.error = Some(crate::operations::OperationError::new(
            crate::operations::HANDED_OFF,
            "this task moved to another agent at a usage limit",
            false,
        ));
        assert!(!release_pristine_worktree(&snapshot), "a handed-off copy is never discarded");
        assert!(std::path::Path::new(&worktree.path).exists());
        assert!(branch_exists(&project, &worktree.branch));

        let (project, worktree) = git_repo_with_worktree("handle");
        let mut control = snapshot_with_pane(&project, "plain-pane", true, true);
        control.worker.worktree = Some(worktree.clone());
        control.status = OperationStatus::Cancelled;
        assert!(release_pristine_worktree(&control), "the same empty copy goes without a hand-over");
        assert!(!std::path::Path::new(&worktree.path).exists());
    }

    #[test]
    fn a_run_that_joined_a_mission_copy_never_discards_it() {
        // Missions: every agent of a mission works in the mission's one copy. A round that failed
        // before changing anything (a declined trust prompt) must not delete the shared copy.
        let (project, worktree) = git_repo_with_worktree("mission-m1");
        let mut snapshot = snapshot_with_pane(&project, "wt-pane", true, false);
        snapshot.worker.worktree = Some(worktree.clone());
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        maybe_dispose_on_dispatch_failure(&snapshot, &registry).unwrap();
        assert!(std::path::Path::new(&worktree.path).exists(), "the mission's copy stays");
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

    /// No summary in flight: the dispose must not go to sleep waiting for one. This checks the code
    /// path (the summary wait never blocked), not elapsed time. The dispose also kills the pane's
    /// process, which is a `taskkill` subprocess on Windows: 0.5-2 s under load, so a wall-clock bound
    /// on the whole call failed in a few of 30 loaded runs, while the summary wait took ~1 ms.
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
        let blocked_before = crate::turn_summary::blocked_waits(pane_id);
        maybe_dispose_after_terminal(&snapshot, &registry).unwrap();
        assert_eq!(
            crate::turn_summary::blocked_waits(pane_id),
            blocked_before,
            "the dispose waited for a turn summary that was not in flight"
        );
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
