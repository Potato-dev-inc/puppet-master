//! Background dispatch of a queued operation: picks or spawns a worker pane, waits for it to
//! be ready (resolving any startup permission prompt) and delivers the task prompt.

use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use super::approvals::{
    native_status_unavailable, startup_manual_approval_required,
    wait_for_startup_permission_resolution,
};
use super::delegation::cleanup_failed_operation;
use super::locks::acquire_operation_locks;
use super::startup_gate::dismiss_for_operation;
use super::pane_util::{
    emit_panes_changed, note_pane_parent, pane_ready_for_dispatch, present_spawned_pane, read_only_pane_matches,
    spawn_pane_with_timeout, wait_for_pane_idle,
};
use super::sse::push_operation_sse;
use super::state::PANE_ASSIGNMENT_SERIAL;
use super::store::{append_operation_event, lease_expires_at};
use crate::events::{SystemEvent, TaskId};
use crate::operations::{OperationError, OperationSnapshot};
use crate::pty::{registry_read_snapshot, registry_write_input, PaneRegistry, SpawnPaneArgs};

type PaneAssignmentGuard = parking_lot::MutexGuard<'static, ()>;

#[cfg(test)]
pub(crate) fn check_agent_dispatchable(
    operation: &OperationSnapshot,
    agent_type: &str,
) -> Result<(), OperationError> {
    ensure_agent_dispatchable(operation, agent_type)
}

pub fn dispatch_existing_operation(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    project: &str,
    id: &str,
) -> Result<(), OperationError> {
    let snapshot = crate::operations::get_operation(project, id)?;
    if snapshot.status != crate::operations::OperationStatus::Queued {
        return Err(OperationError::new(
            "OPERATION_ALREADY_DISPATCHED",
            "operation must be queued",
            false,
        ));
    }
    let registry_for_dispatch = registry.clone();
    let app_for_dispatch = app.clone();
    let operation_project = snapshot.project_path.clone();
    let operation_id = snapshot.operation_id.clone();
    let registry_for_cleanup = registry.clone();
    let app_for_cleanup = app.clone();
    thread::spawn(move || {
        tracing::info!(operation_id = %operation_id, "operation dispatch started");
        let fell_back = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fell_back_in_dispatch = fell_back.clone();
        let registry_for_fallback = registry_for_cleanup.clone();
        let result = crate::operations::run_operation_dispatch(
            &operation_project,
            &operation_id,
            move |operation| {
                // Visible-first: no visible mode, or a pane that fails before the turn starts,
                // re-queues this operation headless instead of failing it.
                if crate::agent_runs::convert_to_fallback(operation, None, &registry_for_fallback) {
                    fell_back_in_dispatch.store(true, std::sync::atomic::Ordering::SeqCst);
                    return Err(OperationError::new("FALLBACK_HEADLESS", "re-queued headless", false));
                }
                let outcome =
                    dispatch_operation(operation, &registry_for_dispatch, &app_for_dispatch);
                if let Err(error) = &outcome {
                    if crate::agent_runs::convert_to_fallback(
                        operation,
                        Some(&error.code),
                        &registry_for_fallback,
                    ) {
                        fell_back_in_dispatch.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                outcome
            },
        );
        if fell_back.load(std::sync::atomic::Ordering::SeqCst) {
            crate::agent_runs::start_fallback_headless(
                &operation_project,
                &operation_id,
                &registry_for_cleanup,
                &app_for_cleanup,
            );
            return;
        }
        match result {
            Ok(updated) => push_operation_sse(&updated),
            Err(_) => {
                if let Ok(updated) =
                    crate::operations::get_operation(&operation_project, &operation_id)
                {
                    cleanup_failed_operation(&updated, &registry_for_cleanup, &app_for_cleanup);
                    push_operation_sse(&updated);
                }
            }
        }
    });
    crate::agent_runs::supervise_existing(snapshot, registry.clone(), app.clone());
    Ok(())
}

/// The dispatch body, run on the dispatch thread once the operation is `Starting`.
fn dispatch_operation(
    operation: &mut OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) -> Result<(), OperationError> {
    let pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
    let state = crate::operations::get_operation(&operation.project_path, &operation.operation_id)?;
    if state.status != crate::operations::OperationStatus::Starting {
        return Err(crate::operations::OperationError::new(
            "OPERATION_CANCELLED",
            "operation was cancelled before pane assignment",
            false,
        ));
    }
    let task_id = operation.task_id.clone().unwrap_or_else(|| TaskId::new().0);
    operation.task_id = Some(task_id.clone());
    append_operation_event(
        &operation.project_path,
        SystemEvent::TaskCreated {
            task_id: TaskId(task_id.to_string()),
            title: operation.task.clone(),
            exclusive: operation.exclusive,
        },
    )
    .map_err(|err| crate::operations::OperationError::new("EVENT_WRITE_FAILED", err, true))?;
    *operation = crate::operations::checkpoint_operation(operation)?;
    acquire_operation_locks(operation)?;
    let agent_type = operation.agent_type.clone();
    ensure_agent_dispatchable(operation, &agent_type)?;
    let (pane_id, pane_created, standby) =
        assign_pane(operation, registry, app, &agent_type, pane_assignment_guard)?;
    // A claimed standby was health-checked at an empty input box when it was claimed: there is no
    // startup to wait for and no dialog to resolve.
    if pane_created && !standby {
        await_pane_ready(operation, registry, app, &pane_id, &agent_type)?;
    }
    send_task_prompt(operation, registry, app, &task_id, &pane_id, &agent_type)?;
    crate::startup_timing::mark(&pane_id, "prompt_sent");
    operation.stage = Some("dispatched_waiting_for_task_completion".into());
    emit_panes_changed(registry, app);
    Ok(())
}

/// The agent type must be known, dispatchable and (for read-only runs) read-only capable.
fn ensure_agent_dispatchable(
    operation: &OperationSnapshot,
    agent_type: &str,
) -> Result<(), OperationError> {
    if let Some(error) = crate::tui::registry::unavailable(agent_type) {
        return Err(error);
    }
    let profile = crate::tui::require_registered(agent_type).map_err(|unknown| {
        OperationError::unknown_agent_type(unknown.id, unknown.valid)
    })?;
    if !profile.features().dispatchable {
        return Err(crate::operations::OperationError::new(
            "AGENT_NOT_DISPATCHABLE",
            profile.not_dispatchable_reason(),
            false,
        ));
    }
    if operation.read_only
        && !crate::agent_runs::read_only_supported_for(agent_type, operation.pane_id.as_deref())
    {
        return Err(crate::operations::OperationError::new(
            "READ_ONLY_UNSUPPORTED",
            crate::agent_runs::read_only_unsupported_message(
                agent_type,
                operation.pane_id.is_some(),
            ),
            false,
        ));
    }
    Ok(())
}

/// Chooses (or spawns) the worker pane and reserves it for the operation. Takes the
/// pane-assignment guard and releases it once the reservation is recorded; the guard is also
/// dropped around waits and spawns so they never block other assignments.
fn assign_pane(
    operation: &mut OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    agent_type: &str,
    mut pane_assignment_guard: PaneAssignmentGuard,
) -> Result<(String, bool, bool), OperationError> {
    let project_path = std::path::PathBuf::from(&operation.project_path);
    if let Some(requested_id) = operation.pane_id.clone() {
        if agent_type == "opencode_native" {
            drop(pane_assignment_guard);
            let _ = crate::opencode::status::wait_until_native_accepts_prompt(
                registry,
                &requested_id,
                Duration::from_secs(8),
            );
            pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
        } else {
            // Pane status trails the last output; give a just-finished turn a moment to
            // read idle instead of failing the follow-up with PANE_UNAVAILABLE.
            drop(pane_assignment_guard);
            wait_for_pane_idle(registry, &requested_id, Duration::from_secs(15));
            pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
        }
    }
    let existing_panes = registry.lock().list();
    // A first-turn run with no explicit pane must not inherit a pane an earlier run
    // used: its terminal still holds that task's conversation. Panes the user opened
    // themselves stay adoptable.
    let panes_used_by_runs: std::collections::HashSet<String> =
        if operation.pane_id.is_none() && operation.turn_index == 0 {
            crate::operations::list_operations(&operation.project_path)
                .unwrap_or_default()
                .into_iter()
                .filter(|other| other.operation_id != operation.operation_id)
                .filter_map(|other| other.pane_id)
                .collect()
        } else {
            Default::default()
        };
    // A read-only run only reuses panes started read-only; other runs never land in
    // one (the worker there cannot write).
    let eligible = |pane: &&crate::pty::PaneInfo| {
        read_only_pane_matches(agent_type, operation.read_only, &pane.id)
            && pane.account.as_deref() == operation.worker.account.as_deref()
            // an isolated run gets its own pane in its own worktree, never a shared one
            && operation.worker.worktree.is_none()
            && !panes_used_by_runs.contains(&pane.id)
            && (operation
                .owner_session_id
                .as_deref()
                .map_or(true, |session| {
                    crate::mcp_sessions::session_controls_pane(session, &pane.id)
                }))
            && pane_ready_for_dispatch(pane, agent_type, registry)
            && crate::project_path::normalize_project_path(std::path::Path::new(&pane.cwd))
                .ok()
                .as_deref()
                == Some(project_path.as_path())
            && crate::operations::conflicting_pane_operation(
                &operation.project_path,
                &pane.id,
                &operation.operation_id,
            )
            .ok()
            .flatten()
            .is_none()
    };
    // Opt-in warm pool: a run that needs a new pane may take a pre-launched one (see `standby`).
    let pool_key = standby_key(operation, agent_type);
    let mut standby_claim: Option<crate::standby::VisibleClaim> = None;
    let (pane_id, pane_created) = if let Some(requested_id) = operation.pane_id.clone() {
        let pane = existing_panes
            .iter()
            .find(|pane| pane.id == requested_id)
            .ok_or_else(|| {
                crate::agent_runs::pane_close::bound_pane_missing_error(&requested_id)
            })?;
        if !pane_ready_for_dispatch(pane, agent_type, registry) {
            let mut error = crate::operations::OperationError::new(
                "PANE_UNAVAILABLE",
                "requested pane is not ready to accept work for this agent type",
                true,
            );
            error.context = json!({
                "pane_id": pane.id,
                "pane_status": pane.status,
                "agent_type": pane.agent_type,
                "blocked_by": if agent_type == "opencode_native" {
                    crate::opencode::status::native_prompt_blockers(registry, &pane.id)
                } else {
                    Vec::new()
                },
            });
            return Err(error);
        }
        if operation.worker.account.is_some() && pane.account != operation.worker.account {
            let mut error = crate::operations::OperationError::new(
                "ACCOUNT_MISMATCH",
                "requested pane runs under a different account",
                false,
            );
            error.context = json!({"pane_id": pane.id, "pane_account": pane.account});
            return Err(error);
        }
        let pane_cwd = std::path::PathBuf::from(&pane.cwd);
        if !crate::project_path::workspace_covers(&project_path, &pane_cwd)
            && !crate::project_path::workspace_covers(&pane_cwd, &project_path)
        {
            let mut error = crate::operations::OperationError::new(
                "WORKSPACE_MISMATCH",
                format!(
                    "worker workspace is {}; supplied {}",
                    pane.cwd, operation.project_path
                ),
                false,
            );
            error.context = json!({
                "expected": pane.cwd,
                "supplied": operation.project_path,
                "pane_id": pane.id,
            });
            return Err(error);
        }
        if let Some(active) = crate::operations::conflicting_pane_operation(
            &operation.project_path,
            &pane.id,
            &operation.operation_id,
        )? {
            let mut error = crate::operations::OperationError::new(
                "PANE_BUSY",
                "requested pane already has an active operation",
                true,
            );
            error.context = json!({"pane_id": pane.id, "operation_id": active.operation_id});
            return Err(error);
        }
        (requested_id, false)
    } else if let Some(pane) = existing_panes.iter().find(eligible) {
        (pane.id.clone(), false)
    } else if let Some(claim) = pool_key
        .as_ref()
        .and_then(crate::standby::claim_visible)
    {
        let pane_id = claim.pane_id().to_string();
        tracing::info!(
            operation_id = %operation.operation_id,
            pane_id = %pane_id,
            warm_ms = claim.age_ms(),
            "dispatch claimed a standby worker"
        );
        standby_claim = Some(claim);
        (pane_id, true)
    } else {
        drop(pane_assignment_guard);
        operation.stage = Some("spawning_pane".into());
        *operation = crate::operations::save_operation_snapshot(operation)?;
        tracing::info!(
            operation_id = %operation.operation_id,
            agent = %agent_type,
            "dispatch spawning worker pane"
        );
        let pane_id = spawn_pane_with_timeout(
            registry,
            app,
            SpawnPaneArgs {
                agent_type: agent_type.to_string(),
                cwd: Some(operation.work_dir().to_string()),
                cols: None,
                rows: None,
                extra_args: None,
                pane_id: None,
                read_only: operation.read_only,
                account: operation.worker.account.clone(),
            },
            Duration::from_secs(45),
        )?;
        pane_assignment_guard = PANE_ASSIGNMENT_SERIAL.lock();
        (pane_id, true)
    };
    if let Some(key) = &pool_key {
        crate::standby::note_run(key, &operation.project_path);
    }
    if pane_created {
        // Nested under the pane of the agent that asked for this run, when it ran from one.
        note_pane_parent(registry, operation.owner_session_id.as_deref(), &pane_id);
    }
    if pane_created && standby_claim.is_none() {
        present_spawned_pane(registry, app, &pane_id, operation.placement.as_deref());
    }
    operation.pane_created = pane_created;
    operation.pane_id = Some(pane_id.clone());
    if standby_claim.is_some() {
        // Persisted now: `reserve_pane` below reloads the snapshot from disk.
        operation.worker.standby_used = true;
        operation.stage = Some("standby_claimed".into());
        *operation = crate::operations::save_operation_snapshot(operation)?;
    }
    if let Some(session) = operation.owner_session_id.as_deref() {
        if !pane_created && !crate::mcp_sessions::session_controls_pane(session, &pane_id) {
            return Err(crate::operations::OperationError::new(
                "AUTHORIZATION_DENIED",
                "take over this pane explicitly before assigning work",
                false,
            ));
        }
        crate::mcp_sessions::register_owned_pane(session, &pane_id)?;
    }
    *operation = crate::operations::reserve_pane(
        &operation.project_path,
        &operation.operation_id,
        &pane_id,
        pane_created,
    )?;
    // Reserved and owned: the standby is the run's pane now. Reveal it like a run-created pane.
    // Any error above dropped the claim instead, which shuts the hidden pane down (no leak).
    let standby = standby_claim.is_some();
    if let Some(claim) = standby_claim {
        claim.commit();
        present_spawned_pane(registry, app, &pane_id, operation.placement.as_deref());
    }
    drop(pane_assignment_guard);
    Ok((pane_id, pane_created, standby))
}

/// The pool key this run would claim: only a run that spawns its own visible pane for a backend
/// with a standby mode has one.
fn standby_key(operation: &OperationSnapshot, agent_type: &str) -> Option<crate::standby::PoolKey> {
    if operation.worker.worktree.is_some() {
        return None; // warm workers are launched in the project folder, not a worktree
    }
    crate::standby::key_for_run(
        agent_type,
        &operation.project_path,
        operation.worker.requested_model.as_deref(),
        operation.read_only,
        operation.worker.allow_shell,
        false,
        operation.worker.account.as_deref(),
    )
}

/// Waits for a freshly spawned worker to reach a safe ready state, parking the run while a
/// startup permission prompt is unresolved.
fn await_pane_ready(
    operation: &mut OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    agent_type: &str,
) -> Result<(), OperationError> {
    operation.stage = Some("waiting_for_pane_readiness".into());
    *operation = crate::operations::save_operation_snapshot(operation)?;
    // Native workers take prompts over HTTP: skip the TUI-paint wait when
    // serve is healthy and attach is live (status reused below).
    let fast_native = if agent_type == "opencode_native" {
        crate::pane_wait::native_fast_dispatch_ready(registry, pane_id)
    } else {
        None
    };
    let (readiness, fast_native_status) = match fast_native {
        Some((readiness, status)) => (readiness, Some(status)),
        None => (
            crate::pane_wait::wait_for_dispatch_ready(registry, pane_id, 120_000).map_err(
                |err| crate::operations::OperationError::new("PANE_READINESS_FAILED", err, true),
            )?,
            None,
        ),
    };
    crate::startup_timing::mark(pane_id, "dispatch_ready");
    if readiness.reason == "timeout"
        || readiness.status.as_deref() == Some("error")
        || readiness.reason == "gone"
    {
        let mut error = crate::operations::OperationError::new(
            "PANE_NOT_READY",
            format!(
                "worker pane did not reach a safe ready state ({})",
                readiness.reason
            ),
            true,
        );
        error.context =
            json!({"pane_id":pane_id,"status":readiness.status,"reason":readiness.reason});
        return Err(error);
    }
    // An update notice or similar the TUI shows before its input box: close it with the keys the
    // profile declares (never a trust question or login) before any task is typed.
    if agent_type != "opencode_native" {
        let _ = dismiss_for_operation(operation, registry, app, pane_id, agent_type);
    }
    let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
    let native_permissions = if agent_type == "opencode_native" {
        let status = match fast_native_status {
            Some(status) => status,
            None => crate::opencode::status::worker_status(registry, pane_id)
                .map_err(native_status_unavailable)?,
        };
        if let Some(model) = status.session_model.or(status.last_user_model) {
            operation.worker.resolved_model =
                Some(format!("{}/{}", model.provider_id, model.model_id));
        }
        status.pending_permission_ids
    } else {
        Vec::new()
    };
    if startup_manual_approval_required(
        agent_type,
        readiness.status.as_deref(),
        &screen,
        &native_permissions,
    ) {
        park_on_startup_dialog(operation, registry, pane_id, agent_type, &screen, &native_permissions)?;
    }
    Ok(())
}

/// A dialog blocks the fresh pane (workspace trust, a command approval, a sign-in): parks the run
/// as `waiting_input` with the dialog as its required action, waits for the user's answer, and
/// resumes the dispatch once the pane has settled. Fails the run when the wait ends badly
/// (`WORKSPACE_TRUST_DECLINED`, the pane died, five minutes unanswered, cancelled).
pub(super) fn park_on_startup_dialog(
    operation: &mut OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    agent_type: &str,
    screen: &str,
    native_permissions: &[String],
) -> Result<(), OperationError> {
    let required_action = if !native_permissions.is_empty() {
        json!({"kind":"permission_required","pane_id":pane_id,"permission_ids":native_permissions})
    } else if let Some(classified) = crate::pane_prompt::required_action(registry, pane_id) {
        // A prompt the TUI profile recognises (Cursor trust / approval) gets its
        // id, kind and options now, so the caller can answer_prompt it.
        classified
    } else if let Some(gate) = crate::tui::profile_for(agent_type).startup_gate(screen) {
        json!({"kind":"manual_approval_required","pane_id":pane_id,"startup_gate":gate.id,"detail":format!("{}. Resolve it in the pane; Puppet Master will not answer it for you.", gate.summary)})
    } else {
        json!({"kind":"manual_approval_required","pane_id":pane_id,"detail":"Resolve the displayed permission prompt manually; the bridge will not approve it."})
    };
    *operation = crate::operations::mark_operation_startup_wait(
        &operation.project_path,
        &operation.operation_id,
        Some("waiting_input".into()),
        Some(required_action),
        if agent_type == "opencode_native" {
            crate::operations::StateSource::Native
        } else {
            crate::operations::StateSource::Inferred
        },
    )?;
    push_operation_sse(operation);
    let mut ready_state;
    loop {
        ready_state = wait_for_startup_permission_resolution(
            registry,
            &operation.project_path,
            &operation.operation_id,
            pane_id,
            agent_type,
        )?;
        if agent_type == "opencode_native" {
            break;
        }
        // The TUI is still redrawing after a dialog it just had open (Cursor shows
        // "Trusting workspace..." and then a fresh input box). A prompt typed in that
        // window is swallowed, so wait for the pane to read idle and hold still.
        // This happens while the run is still parked: once resumed, status events
        // move it out of Starting and the dispatch would abort.
        *operation = crate::operations::mark_operation_startup_settling(
            &operation.project_path,
            &operation.operation_id,
            crate::operations::StateSource::Inferred,
        )?;
        push_operation_sse(operation);
        wait_for_pane_idle(registry, pane_id, Duration::from_secs(20));
        // Positive readiness after the person passed the screen (live v0.2.0 hunt, Codex 0.162.1:
        // after Trust the composer was drawn at once but took keys seconds later): the input box
        // read ready AND the pane quiet for a moment, before the task is typed.
        settle_before_typing(registry, pane_id, agent_type);
        if let Some(pane) = registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
        {
            ready_state = pane.status;
        }
        // The settled screen shows the dialog again (the release was a redraw of it, or the agent
        // asked again): park on it once more instead of resuming into a screen the task cannot go
        // into (that ended the run PANE_BLOCKED_BY_PROMPT while the person was answering).
        let settled = registry_read_snapshot(registry, pane_id).unwrap_or_default();
        if !super::approvals::startup_still_blocked(agent_type, &settled, &[]) {
            break;
        }
        let again = crate::pane_prompt::required_action(registry, pane_id).unwrap_or_else(|| {
            json!({"kind":"manual_approval_required","pane_id":pane_id,"detail":"Resolve the displayed prompt in the pane; Puppet Master will not answer it for you."})
        });
        *operation = crate::operations::mark_operation_startup_wait(
            &operation.project_path,
            &operation.operation_id,
            Some("waiting_input".into()),
            Some(again),
            crate::operations::StateSource::Inferred,
        )?;
        push_operation_sse(operation);
    }
    *operation = crate::operations::resume_operation_dispatch_when_ready(
        &operation.project_path,
        &operation.operation_id,
        &ready_state,
        if agent_type == "opencode_native" {
            crate::operations::StateSource::Native
        } else {
            crate::operations::StateSource::Inferred
        },
    )?;
    push_operation_sse(operation);
    Ok(())
}

/// Records the baseline, writes the task prompt into the pane and verifies delivery.
fn send_task_prompt(
    operation: &mut OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    task_id: &str,
    pane_id: &str,
    agent_type: &str,
) -> Result<(), OperationError> {
    operation.stage = Some("dispatching".into());
    *operation = crate::operations::save_operation_snapshot(operation)?;
    append_operation_event(
        &operation.project_path,
        SystemEvent::TaskClaimed {
            task_id: TaskId(task_id.to_string()),
            agent_id: pane_id.to_string(),
            lease_expires_at_ms: lease_expires_at(None),
        },
    )
    .map_err(|err| crate::operations::OperationError::new("EVENT_WRITE_FAILED", err, true))?;
    *operation = crate::operations::save_operation_snapshot(operation)?;
    let prompt = crate::agent_runs::delivery::dispatch_prompt(
        &operation.task,
        operation.worker_has_mcp_tools,
        task_id,
        pane_id,
        &operation.project_path,
    );
    // A startup notice the TUI drew after readiness was judged is closed before the baseline is
    // taken (a dialog is not closed; the delivery below refuses to type over one).
    if agent_type != "opencode_native" {
        let _ = dismiss_for_operation(operation, registry, app, pane_id, agent_type);
    }
    // The state of the pane's folder just before the agent gets the task (checkpoint "Before"). A
    // follow-up on a known pane was snapshotted already by `run_operation_dispatch` (this one is
    // then debounced); a pane this run just spawned has not been, and its first turn would have no
    // checkpoints. For an isolated run the folder is the worktree.
    crate::checkpoints::before_dispatch(pane_id, &operation.task);
    let mut dispatch_guard = Some(PANE_ASSIGNMENT_SERIAL.lock());
    let current =
        crate::operations::get_operation(&operation.project_path, &operation.operation_id)?;
    if current.status != crate::operations::OperationStatus::Starting {
        return Err(crate::operations::OperationError::new(
            "OPERATION_CANCELLED",
            "operation was cancelled before prompt dispatch",
            false,
        ));
    }
    if let Some(session) = operation.owner_session_id.as_deref() {
        crate::mcp_sessions::register_owned_pane(session, pane_id)?;
    }
    if agent_type == "opencode_native" {
        let baseline =
            crate::opencode::messages::read_pane_messages(registry, pane_id, 10_000, None)
                .map_err(|error| {
                    crate::operations::OperationError::new("NATIVE_STATUS_UNAVAILABLE", error, true)
                })?;
        if !baseline.session_id.is_empty() {
            operation.worker.provider_session_id = Some(baseline.session_id.clone());
        }
        operation.message_baseline_ids = baseline
            .messages
            .into_iter()
            .filter_map(|message| message.id)
            .collect();
    }
    take_output_baseline(operation, registry, pane_id);
    crate::agent_runs::record_user_task(&operation.project_path, &operation.operation_id, &prompt);
    operation.started_at_ms = Some(crate::event_log::now_ms().max(0) as u64);
    *operation = crate::operations::save_operation_snapshot(operation)?;
    // A pane that never showed a screen that takes a task is parked once (the user can bring the
    // agent to its input box) and then delivered to again; never typed into, never sent headless.
    let mut parked = false;
    // A dialog that drew after readiness was judged (live r13: Codex drew its input box, then its
    // folder-trust dialog 1.7 s later, while the task was about to be typed; or the pane read
    // `running` before the TUI painted its gate) parks the run on that dialog like one seen at
    // startup; the task, never typed, is delivered once it is answered.
    let mut dialog_parks = 0;
    // Rounds of "typed, never showed" on a live pane (settle, type again).
    let mut not_taken_rounds = 0;
    loop {
        let outcome = crate::operations::dispatch_input_if_active(
            &operation.project_path,
            &operation.operation_id,
            |active| deliver_task(active, registry, app, pane_id, agent_type, &prompt),
        );
        match outcome {
            // Only a user-only startup gate (folder trust, sign-in: any agent's, as its profile
            // reads it) is parked on, and only when the refusal left no copy of the task in the box,
            // so the delivery after the answer types it exactly once. A command approval or a
            // dismissible notice keeps its own handling.
            Err(error)
                if dialog_parks < MAX_DIALOG_PARKS
                    && blocked_before_typing(&error)
                    && late_startup_gate(&error, agent_type, registry, pane_id).is_some() =>
            {
                dialog_parks += 1;
                // Parking can take minutes: never hold the pane-assignment lock through it.
                drop(dispatch_guard.take());
                crate::agent_runs::transcript::append(
                    &operation.project_path,
                    &operation.operation_id,
                    "readiness",
                    &crate::redact::text(&format!("Parked on a dialog that appeared before the task was typed: {}", error.message)),
                );
                let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
                park_on_startup_dialog(operation, registry, pane_id, agent_type, &screen, &[])?;
                // The screen before the park was the dialog: what the agent draws after the answer
                // (Codex: its banner and folder line) is not the turn's output (live r16: the result
                // was the folder path). Measure the turn from the screen as it is now.
                take_output_baseline(operation, registry, pane_id);
                // The run's time limit counts from the delivery, not from before the park.
                operation.started_at_ms = Some(crate::event_log::now_ms().max(0) as u64);
                *operation = crate::operations::save_operation_snapshot(operation)?;
                dispatch_guard = Some(PANE_ASSIGNMENT_SERIAL.lock());
            }
            Err(error) if error.code == crate::agent_runs::readiness::SCREEN_NOT_READY && !parked => {
                parked = true;
                // Parking can take minutes: never hold the pane-assignment lock through it.
                drop(dispatch_guard.take());
                park_on_unknown_screen(operation, registry, pane_id, agent_type, &error, UNKNOWN_SCREEN_PARK)?;
                dispatch_guard = Some(PANE_ASSIGNMENT_SERIAL.lock());
            }
            // The typed task never showed in a pane whose agent is alive (still loading: it drew
            // its input box before it took keys). Wait for the pane to settle and type again: the
            // profile's clear key empties the box first, so a late copy is not doubled. Never a
            // headless re-run while the person's visible pane is alive (live v0.2.0 hunt, cx4/cx5).
            Err(error) if not_taken_by_live_pane(&error) && not_taken_rounds < MAX_NOT_TAKEN_ROUNDS => {
                not_taken_rounds += 1;
                drop(dispatch_guard.take());
                crate::agent_runs::transcript::append(
                    &operation.project_path,
                    &operation.operation_id,
                    "readiness",
                    &crate::redact::text(&format!(
                        "The task did not show in the agent's input box (it may still be loading). Waiting for the pane to settle, then typing it again (try {} of {MAX_NOT_TAKEN_ROUNDS}); the visible pane is kept, no background re-run.",
                        not_taken_rounds
                    )),
                );
                settle_before_typing(registry, pane_id, agent_type);
                dispatch_guard = Some(PANE_ASSIGNMENT_SERIAL.lock());
            }
            Err(error) if not_taken_by_live_pane(&error) => {
                // Still not taken: end the run with the screen, keep the pane open for the person.
                return Err(not_taken_error(&error, registry, pane_id));
            }
            other => {
                other?;
                break;
            }
        }
    }
    drop(dispatch_guard);
    // The agent has the task: the turn starts here for checkpoints and Last turn, even when the
    // pane's status never said `running` (a pane that went from boot straight into the task). Only
    // while the pane still reads running: a pane that already read idle again has had its events.
    let working = registry
        .lock()
        .list()
        .into_iter()
        .any(|pane| pane.id == pane_id && pane.status == "running");
    if working {
        crate::checkpoints::note_dispatched(pane_id);
        crate::turn_summary::note_dispatched(pane_id);
    }
    Ok(())
}

/// The startup gate (folder trust, sign-in: a screen only the user may pass) a refused delivery ran
/// into, when the refusal is a `PANE_BLOCKED_BY_PROMPT` and the pane still shows such a gate. When
/// the refusal also left no copy of the task in the box (`blocked_before_typing`), the run parks
/// (`park_on_startup_dialog`) rather than fails. A command approval or a dismissible notice is not
/// one: those keep their own handling.
pub(super) fn late_startup_gate(
    error: &OperationError,
    agent_type: &str,
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Option<serde_json::Value> {
    if error.code != "PANE_BLOCKED_BY_PROMPT" || agent_type == "opencode_native" {
        return None;
    }
    let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
    crate::pane_prompt::startup_gate(agent_type, &screen)
}

/// Types the task into the pane under the typing permit (`agent_runs::readiness`), as the dispatch
/// closure: a verified delivery for a profile that can read its input box, a plain typed submit for
/// the others, the agent's own API for a native pane. Nothing is typed into a screen that is not
/// `Ready`; the refusals say why.
fn deliver_task(
    active: &OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    agent_type: &str,
    prompt: &str,
) -> Result<(), OperationError> {
    use crate::agent_runs::delivery::{self, Delivery};
    if active.pane_id.as_deref() != Some(pane_id) {
        return Err(crate::operations::OperationError::new(
            "PANE_RESERVATION_LOST",
            "operation no longer owns its dispatch pane",
            false,
        ));
    }
    let profile = crate::tui::profile_for(agent_type);
    if agent_type == "opencode_native" {
        // The agent's own protocol: the prompt is never typed into the screen.
        return registry_write_input(
            registry,
            app,
            pane_id,
            prompt,
            true,
            true,
            None,
            crate::pty::WriteOrigin::Structured,
        )
        .map_err(|err| {
            let code = if err.starts_with(crate::opencode::delivery::PROMPT_NOT_DELIVERED) {
                crate::opencode::delivery::PROMPT_NOT_DELIVERED
            } else {
                "DISPATCH_FAILED"
            };
            crate::operations::OperationError::new(code, err, true)
        });
    }
    // A dialog on screen (trust, command approval) takes hotkeys: typing the task over it would
    // answer it with the task's letters and lose the task. A host shell prompt: the agent is gone.
    let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
    if let Some(blocked) = crate::pane_prompt::dispatch_blocker(agent_type, &screen) {
        if blocked.code != "AGENT_EXITED" {
            return Err(mark_not_typed(blocked));
        }
    }
    let mut io = delivery::PaneIo { registry, app, pane_id };
    let mut trace = Vec::new();
    // Clear, type, verify the text is in the input box (when the profile can read it), submit, verify a turn started.
    let outcome = delivery::deliver_text(&mut io, profile.as_ref(), profile.clear_input_sequence(), prompt, &mut trace);
    if profile.verifies_prompt_delivery() {
        delivery::record_delivery(&active.project_path, &active.operation_id, None, &outcome, &trace);
    }
    if !crate::agent_runs::readiness::has_recogniser(profile.as_ref())
        && !matches!(outcome, Delivery::NotReady { .. } | Delivery::AgentGone(_) | Delivery::Blocked(_))
    {
        crate::agent_runs::transcript::append(
            &active.project_path,
            &active.operation_id,
            "readiness",
            &crate::redact::text("READINESS_INFERRED: this agent has no input-box recogniser, so readiness was inferred (something drawn, no dialog, no shell prompt, not busy)"),
        );
    }
    match outcome {
        Delivery::Started | Delivery::SubmittedUnconfirmed => Ok(()),
        Delivery::NotVisible { cleared } => {
            let mut error = crate::operations::OperationError::new(
                "PROMPT_NOT_SUBMITTED",
                delivery::not_visible_message(cleared),
                true,
            );
            // The dispatch loop waits for a live pane and types again instead of letting this
            // fall back to headless (`not_taken_by_live_pane`).
            if agent_alive(registry, pane_id) {
                error.context = json!({ "pane_id": pane_id, "pane_alive": true, "cleared": cleared });
            }
            Err(error)
        }
        Delivery::Blocked(gate) => {
            let error = crate::pane_prompt::gate_blocked_error(gate);
            // Delivered again after the dialog is answered only when that cannot leave two copies:
            // nothing was typed, or the profile clears its box before it types (the typed text is
            // then cleared first, and the box must read empty before the task goes in again).
            let typed = trace.iter().any(|line| line.contains(delivery::TYPING_TRACE));
            Err(if !typed || profile.clear_input_sequence().is_some() { mark_not_typed(error) } else { error })
        }
        Delivery::AgentGone(shell) => {
            Err(crate::agent_runs::readiness::host_shell_error(&shell, profile.as_ref(), pane_id))
        }
        Delivery::NotReady { why, screen } => {
            Err(crate::agent_runs::readiness::not_ready_error(profile.id(), &why, &screen, pane_id))
        }
        Delivery::WriteFailed(err) => Err(crate::operations::OperationError::new("DISPATCH_FAILED", err, true)),
    }
}

/// The screen right before the task is typed, the baseline the turn's output is measured against
/// (stored on the run as `output_baseline`; the supervisor adopts it). A ConPTY screen can lag and
/// read blank; a blank baseline makes startup furniture look like new output, so retry briefly
/// before settling for none (completion also guards that case).
fn take_output_baseline(operation: &mut OperationSnapshot, registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) {
    // The pane's whole text and its hyperlinks as they stand before the task is typed: a reply taller than the pane,
    // and the links in it, are measured from here.
    if let Ok(full) = crate::pty::registry_read_full_text(registry, pane_id) {
        crate::pty::pane_facts::mark_turn(pane_id, &full);
    }
    for attempt in 0..4 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(150));
        }
        if let Ok(screen) = registry_read_snapshot(registry, pane_id) {
            operation.output_baseline = crate::operations::compact_output_baseline(&screen);
        }
        if operation.output_baseline.is_some() {
            break;
        }
    }
}

/// How many times one delivery parks on a dialog that drew before the task was typed.
const MAX_DIALOG_PARKS: u32 = 3;

/// How many times a task that never showed in a live pane's input box is typed again (after the
/// pane settled) before the run ends with the screen. Never a headless re-run of a live pane.
const MAX_NOT_TAKEN_ROUNDS: u32 = 3;
/// How long a pane must print nothing (and read ready) to count as settled before typing.
const SETTLE_QUIET: Duration = Duration::from_millis(1_500);
/// Longest wait for that; past it delivery tries anyway (it has its own checks).
const SETTLE_MAX: Duration = Duration::from_secs(30);
const SETTLE_POLL: Duration = Duration::from_millis(100);

/// The pane is there and its agent process is not known to have exited.
fn agent_alive(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> bool {
    let present = registry.lock().panes.contains_key(pane_id);
    present
        && crate::agent_runs::delivery::observe_pane(registry, pane_id).agent
            != crate::agent_runs::readiness::AgentProcess::Exited
}

/// A delivery that typed the task into a live pane and never saw it (`deliver_task` marks it).
fn not_taken_by_live_pane(error: &OperationError) -> bool {
    error.code == "PROMPT_NOT_SUBMITTED" && error.context.get("pane_alive").and_then(|v| v.as_bool()) == Some(true)
}

/// Code of a run whose live pane never took the typed task. Not a pre-turn fallback code
/// (`fallback::is_pre_turn_failure`): the person's visible pane is kept, never re-run headless.
pub(crate) const PROMPT_NOT_TAKEN: &str = "PROMPT_NOT_TAKEN";

fn not_taken_error(error: &OperationError, registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> OperationError {
    let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
    let cleared = error.context.get("cleared").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut out = OperationError::new(
        PROMPT_NOT_TAKEN,
        format!(
            "The agent showed its input box but did not take the task: the typed text never showed there, after waiting for the pane to settle {MAX_NOT_TAKEN_ROUNDS} times. Nothing was submitted and the pane is left open. {}",
            if cleared { "The input box reads empty: send the task again when the agent is ready." } else { "The box may still hold the typed text: check the pane before sending the task again." }
        ),
        true,
    );
    out.context = json!({ "pane_id": pane_id, "screen": crate::agent_runs::readiness::screen_tail(&screen, 12), "cleared": cleared });
    out
}

/// Positive readiness before typing (`readiness::wait_until_settled`): the input box reads ready and
/// the pane printed nothing for [`SETTLE_QUIET`], at most [`SETTLE_MAX`]. Best effort: delivery
/// still makes every one of its own checks after it.
fn settle_before_typing(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str, agent_type: &str) {
    if agent_type == "opencode_native" {
        return;
    }
    let profile = crate::tui::profile_for(agent_type);
    let start = std::time::Instant::now();
    let outcome = crate::agent_runs::readiness::wait_until_settled(
        profile.as_ref(),
        || crate::agent_runs::delivery::observe_pane(registry, pane_id),
        || start.elapsed().as_millis() as u64,
        || thread::sleep(SETTLE_POLL),
        SETTLE_QUIET,
        SETTLE_MAX,
    );
    tracing::debug!(pane_id, ?outcome, waited_ms = start.elapsed().as_millis() as u64, "settle before typing");
}

/// Marks a `PANE_BLOCKED_BY_PROMPT` refusal as one that leaves no copy of the task in the pane's box
/// once delivery runs again (nothing typed, or a box delivery clears before typing).
fn mark_not_typed(mut error: OperationError) -> OperationError {
    if error.code == "PANE_BLOCKED_BY_PROMPT" {
        if let Some(context) = error.context.as_object_mut() {
            context.insert("not_typed".into(), json!(true));
        } else {
            error.context = json!({"not_typed": true});
        }
    }
    error
}

/// A dialog refused the delivery before any of the task was typed: the run can wait for the
/// person's answer and deliver then, exactly once.
pub(super) fn blocked_before_typing(error: &OperationError) -> bool {
    error.code == "PANE_BLOCKED_BY_PROMPT" && error.context.get("not_typed").and_then(|v| v.as_bool()) == Some(true)
}

/// How long a run parked on a screen that is not ready keeps looking before it ends.
const UNKNOWN_SCREEN_PARK: Duration = Duration::from_secs(300);

/// The pane never read `Ready` within delivery's window (a blank pane, a splash, a screen no rule
/// recognises, a turn still running). Parks the run as `waiting_input` with the screen and a plain
/// explanation, keeps looking (and re-checking gates) and resumes the dispatch once the pane reads
/// ready or shows a screen delivery can close itself. Ends the run when the agent is gone, the user
/// declined a dialog, the run was cancelled, or five minutes pass (`SCREEN_NOT_READY`). Never types
/// anything and never falls back to a background run.
pub(super) fn park_on_unknown_screen(
    operation: &mut OperationSnapshot,
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    agent_type: &str,
    not_ready: &OperationError,
    park_for: Duration,
) -> Result<(), OperationError> {
    let screen = not_ready.context.get("screen").and_then(|s| s.as_str()).unwrap_or_default().to_string();
    let detail = not_ready.message.replace(" Get the agent to its input box, then start the run again.", "");
    let required_action = json!({
        "kind": "screen_not_ready",
        "pane_id": pane_id,
        "detail": format!("{detail} The run is waiting: bring the agent to its input box in the pane (close any menu or message) and it continues by itself."),
        "screen": screen,
    });
    *operation = crate::operations::mark_operation_startup_wait(
        &operation.project_path,
        &operation.operation_id,
        Some("waiting_input".into()),
        Some(required_action),
        crate::operations::StateSource::Inferred,
    )?;
    push_operation_sse(operation);
    crate::agent_runs::transcript::append(
        &operation.project_path,
        &operation.operation_id,
        "readiness",
        &crate::redact::text(&format!("Parked: {}", not_ready.message)),
    );
    let profile = crate::tui::profile_for(agent_type);
    let (project, id) = (operation.project_path.clone(), operation.operation_id.clone());
    let deadline = std::time::Instant::now() + park_for;
    let end = crate::agent_runs::readiness::wait_while_parked(
        profile.as_ref(),
        || crate::agent_runs::delivery::observe_pane(registry, pane_id),
        || {
            crate::operations::get_operation(&project, &id)
                .map(|op| op.status != crate::operations::OperationStatus::WaitingInput)
                .unwrap_or(true)
        },
        || crate::pane_prompt::gate_was_declined(pane_id),
        || std::time::Instant::now() >= deadline,
        || thread::sleep(Duration::from_millis(400)),
    );
    use crate::agent_runs::readiness::ParkEnd;
    match end {
        ParkEnd::Resume => {
            let pane_state = registry
                .lock()
                .list()
                .into_iter()
                .find(|pane| pane.id == pane_id)
                .map(|pane| pane.status)
                .unwrap_or_else(|| "running".into());
            *operation = crate::operations::resume_operation_dispatch_when_ready(
                &operation.project_path,
                &operation.operation_id,
                &pane_state,
                crate::operations::StateSource::Inferred,
            )?;
            // The task is typed now: the run's time limit counts from here, not from before the
            // park (a parked run otherwise resumed already past its deadline).
            operation.started_at_ms = Some(crate::event_log::now_ms().max(0) as u64);
            *operation = crate::operations::save_operation_snapshot(operation)?;
            push_operation_sse(operation);
            Ok(())
        }
        ParkEnd::Gone(shell) => {
            Err(crate::agent_runs::readiness::host_shell_error(&shell, profile.as_ref(), pane_id))
        }
        ParkEnd::Declined => Err(crate::bridge::startup_decline_error(pane_id, None)),
        ParkEnd::Cancelled => Err(crate::operations::OperationError::new(
            "OPERATION_CANCELLED",
            "operation was cancelled while waiting for the agent's input box",
            false,
        )),
        ParkEnd::TimedOut { why, screen } => {
            Err(crate::agent_runs::readiness::not_ready_error(profile.id(), &why, &screen, pane_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::{registry, TuiProfile};
    use std::sync::Arc;

    /// A profile that only exists in this test: its id is not an `AgentType` variant.
    struct GateProbe;

    impl TuiProfile for GateProbe {
        fn id(&self) -> &'static str {
            "gate_probe_t08"
        }
    }

    /// Every gate asks the registry: a registered id that `AgentType` does not know passes the
    /// dispatch gate and the agent-context gate, and is rejected again once it is unregistered.
    #[test]
    fn gates_use_registry() {
        let project = std::env::temp_dir().join(format!("pm-gates-{}", uuid::Uuid::new_v4()));
        let request = crate::operations::DelegateWorkRequest {
            project_path: project.to_string_lossy().into_owned(),
            task: "inspect".into(),
            agent_type: "codex".into(),
            pane_id: None,
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane: false,
            placement: None,
            owner_session_id: Some("gates".into()),
            worker_has_mcp_tools: false,
            agent_run_id: None,
            turn_index: 0,
            worker: Default::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        let operation = crate::operations::create_operation(request).unwrap().0;
        let id = GateProbe.id();
        assert!(crate::pty::agents::AgentType::parse(id).is_none());

        // Inject, probe the gates, restore: other tests read the same process-wide registry, so
        // keep the window with the extra profile as short as possible.
        let _serial = registry::test_serial();
        let before = registry::snapshot();
        let mut with_probe: Vec<Arc<dyn TuiProfile>> = before.iter().cloned().collect();
        with_probe.push(Arc::new(GateProbe));
        registry::registry().swap(with_probe);
        let dispatch = super::check_agent_dispatchable(&operation, id);
        let context = crate::agent_contexts::registered_context_agent(id);
        registry::registry().swap(before.iter().cloned().collect());

        assert!(dispatch.is_ok(), "registered profile passes the dispatch gate: {dispatch:?}");
        assert_eq!(context.ok(), Some(None), "registered, but no code context profile");

        let error = super::check_agent_dispatchable(&operation, id).unwrap_err();
        assert_eq!(error.code, "UNKNOWN_AGENT_TYPE");
        assert!(crate::agent_contexts::registered_context_agent(id).is_err());
        assert_eq!(
            crate::agent_contexts::registered_context_agent("CODEX").ok(),
            Some(Some(crate::pty::agents::AgentType::Codex))
        );
    }
}
