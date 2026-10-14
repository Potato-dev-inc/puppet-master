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
    emit_panes_changed, pane_ready_for_dispatch, present_spawned_pane, read_only_pane_matches,
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
        let required_action = if !native_permissions.is_empty() {
            json!({"kind":"permission_required","pane_id":pane_id,"permission_ids":native_permissions})
        } else if let Some(classified) = crate::pane_prompt::required_action(registry, pane_id) {
            // A prompt the TUI profile recognises (Cursor trust / approval) gets its
            // id, kind and options now, so the caller can answer_prompt it.
            classified
        } else if let Some(gate) = crate::tui::profile_for(agent_type).startup_gate(&screen) {
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
        let mut ready_state = wait_for_startup_permission_resolution(
            registry,
            &operation.project_path,
            &operation.operation_id,
            pane_id,
            agent_type,
        )?;
        // The TUI is still redrawing after a dialog it just had open (Cursor shows
        // "Trusting workspace..." and then a fresh input box). A prompt typed in that
        // window is swallowed, so wait for the pane to read idle and hold still.
        // This happens while the run is still parked: once resumed, status events
        // move it out of Starting and the dispatch would abort.
        if agent_type != "opencode_native" {
            *operation = crate::operations::mark_operation_startup_settling(
                &operation.project_path,
                &operation.operation_id,
                crate::operations::StateSource::Inferred,
            )?;
            push_operation_sse(operation);
            wait_for_pane_idle(registry, pane_id, Duration::from_secs(20));
            if let Some(pane) = registry
                .lock()
                .list()
                .into_iter()
                .find(|pane| pane.id == pane_id)
            {
                ready_state = pane.status;
            }
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
    }
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
    let _dispatch_guard = PANE_ASSIGNMENT_SERIAL.lock();
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
    // Snapshot the screen right before the prompt is written. A ConPTY screen can lag
    // and read blank; a blank baseline makes startup furniture look like new output,
    // so retry briefly before settling for none (completion also guards that case).
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
    crate::agent_runs::record_user_task(&operation.project_path, &operation.operation_id, &prompt);
    operation.started_at_ms = Some(crate::event_log::now_ms().max(0) as u64);
    *operation = crate::operations::save_operation_snapshot(operation)?;
    crate::operations::dispatch_input_if_active(
        &operation.project_path,
        &operation.operation_id,
        |active| {
            if active.pane_id.as_deref() != Some(pane_id) {
                return Err(crate::operations::OperationError::new(
                    "PANE_RESERVATION_LOST",
                    "operation no longer owns its dispatch pane",
                    false,
                ));
            }
            let profile = crate::tui::profile_for(agent_type);
            if agent_type != "opencode_native" {
                // The agent exited and its host shell is at a prompt: never type a task into it.
                let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
                if crate::tui::agent_gone_to_host_shell(agent_type, &screen) {
                    return Err(crate::pane_prompt::dispatch_blocker(agent_type, &screen)
                        .expect("a host shell prompt blocks dispatch"));
                }
            }
            if profile.verifies_prompt_delivery() {
                // A dialog on screen (trust, command approval) takes hotkeys: typing the
                // task over it would answer it with the task's letters and lose the task.
                let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
                if let Some(blocked) = crate::pane_prompt::dispatch_blocker(agent_type, &screen) {
                    return Err(blocked);
                }
                // Visible panes whose TUI is readable: clear, type, verify the text
                // is in the input box, submit, verify a turn started.
                let mut io = crate::agent_runs::delivery::PaneIo {
                    registry,
                    app,
                    pane_id,
                };
                let mut trace = Vec::new();
                let outcome = crate::agent_runs::delivery::deliver_prompt_traced(
                    &mut io,
                    profile.clear_input_sequence(),
                    &prompt,
                    &mut trace,
                );
                crate::agent_runs::transcript::append(
                    &active.project_path,
                    &active.operation_id,
                    "delivery",
                    &format!("{outcome:?}\n{}", trace.join("\n")),
                );
                return match outcome {
                    crate::agent_runs::delivery::Delivery::NotVisible => {
                        Err(crate::operations::OperationError::new(
                            "PROMPT_NOT_SUBMITTED",
                            crate::agent_runs::delivery::NOT_VISIBLE_MESSAGE,
                            true,
                        ))
                    }
                    crate::agent_runs::delivery::Delivery::Blocked(gate) => {
                        Err(crate::pane_prompt::gate_blocked_error(gate))
                    }
                    crate::agent_runs::delivery::Delivery::WriteFailed(err) => Err(
                        crate::operations::OperationError::new("DISPATCH_FAILED", err, true),
                    ),
                    _ => Ok(()),
                };
            }
            if let Some(clear) = profile.clear_input_sequence() {
                // Empty the TUI's input box first (a no-op when it is already
                // empty); otherwise a leftover prompt prefixes the new one.
                let _ = registry_write_input(registry, app, pane_id, clear, false, false, None);
                thread::sleep(Duration::from_millis(250));
            }
            registry_write_input(
                registry,
                app,
                pane_id,
                &prompt,
                true,
                agent_type == "opencode_native",
                None,
            )
            .map_err(|err| {
                let code = if err.starts_with(crate::opencode::delivery::PROMPT_NOT_DELIVERED) {
                    crate::opencode::delivery::PROMPT_NOT_DELIVERED
                } else {
                    "DISPATCH_FAILED"
                };
                crate::operations::OperationError::new(code, err, true)
            })
        },
    )?;
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
