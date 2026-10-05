//! The headless child process: spawn, stream its output into the transcript, wait for it (with a
//! result-grace and the run deadline), terminate it, and cancel it by operation id.

use super::control::{children, terminate, HeadlessControl};
use super::finish::finish_from_output;
use super::output::{spawn_reader, stream_text};
use crate::agent_runs::{persist, transcript};
use crate::operations::{self, ContextPolicy, OperationError, OperationStatus, StateSource};
use parking_lot::Mutex;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(in crate::agent_runs) fn execute_headless(
    project: String,
    operation_id: String,
    agent_type: String,
    task: String,
    read_only: bool,
    timeout_ms: u64,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) {
    transcript::append(&project, &operation_id, "user_task", &task);
    let snapshot = operations::get_operation(&project, &operation_id).ok();
    let resume_session = snapshot
        .as_ref()
        .filter(|current| {
            persist::snapshot_can_resume(current)
                && current.worker.context_policy != ContextPolicy::Fresh
        })
        .and_then(|current| current.worker.provider_session_id.clone())
        .filter(|id| !id.is_empty());
    let trust_workspace = snapshot
        .as_ref()
        .is_some_and(|current| current.worker.trust_workspace);
    let allow_shell = snapshot
        .as_ref()
        .is_some_and(|current| current.worker.allow_shell);
    let requested_model = snapshot
        .as_ref()
        .and_then(|current| current.worker.requested_model.clone())
        .filter(|model| !model.trim().is_empty());
    if crate::tui::profile_for(&agent_type).probes_default_model()
        && snapshot
            .as_ref()
            .is_some_and(|current| current.worker.resolved_model.is_none())
    {
        // Some headless CLIs never say which model ran; record the CLI's configured default so
        // resolved_model is a real answer instead of null. Off-thread: it may shell out.
        let (project, operation_id) = (project.clone(), operation_id.clone());
        let probe_type = agent_type.clone();
        let _ = thread::Builder::new()
            .name("puppet-default-model".into())
            .spawn(move || {
                if let Some(model) = crate::tui::profile_for(&probe_type).default_model() {
                    // A configured default is a best guess, not a confirmation of what ran.
                    let _ = operations::record_unconfirmed_model(&project, &operation_id, &model);
                }
            });
    }
    let spec = match crate::tui::headless_launch(
        &agent_type,
        &crate::tui::HeadlessLaunch {
            project: &project,
            task: &task,
            read_only,
            resume_session: resume_session.as_deref(),
            trust_workspace,
            allow_shell,
            model: requested_model.as_deref(),
        },
    ) {
        Ok(spec) if !read_only || spec.read_only_enforced => spec,
        Ok(_) => {
            fail_start(
                &project,
                &operation_id,
                "READ_ONLY_UNSUPPORTED",
                "adapter did not enforce read-only mode",
            );
            publish(&project, &operation_id, &registry, &app);
            return;
        }
        Err(error) => {
            let (code, message) = error
                .strip_prefix("READ_ONLY_UNSUPPORTED: ")
                .map(|message| ("READ_ONLY_UNSUPPORTED", message))
                .unwrap_or(("LAUNCH_FAILED", error.as_str()));
            fail_start(&project, &operation_id, code, message);
            publish(&project, &operation_id, &registry, &app);
            return;
        }
    };
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut readers = Vec::new();
    let dispatch = operations::run_operation_dispatch(&project, &operation_id, |_| {
        operations::dispatch_input_if_active(&project, &operation_id, |_| {
            let child = command.spawn().map_err(|error| {
                let mut op_error = OperationError::new(
                    "LAUNCH_FAILED",
                    format!(
                        "spawn failed (program {:?}, cwd {}): {error}",
                        spec.program,
                        spec.cwd.display()
                    ),
                    true,
                );
                op_error.context = serde_json::json!({"program": spec.program, "cwd": spec.cwd.display().to_string()});
                op_error
            })?;
            let control = Arc::new(HeadlessControl {
                child: Mutex::new(child),
                cancelled: AtomicBool::new(false),
            });
            children()
                .lock()
                .insert(operation_id.clone(), control.clone());
            let (stdout, stderr) = {
                let mut child = control.child.lock();
                (child.stdout.take(), child.stderr.take())
            };
            if let Some(stdout) = stdout {
                if let Some(reader) =
                    spawn_reader(project.clone(), operation_id.clone(), "stdout", stdout)
                {
                    readers.push(reader);
                }
            }
            if let Some(stderr) = stderr {
                if let Some(reader) =
                    spawn_reader(project.clone(), operation_id.clone(), "stderr", stderr)
                {
                    readers.push(reader);
                }
            }
            Ok(())
        })?;
        Ok(())
    });
    if dispatch.is_err() {
        children().lock().remove(&operation_id);
        publish(&project, &operation_id, &registry, &app);
        return;
    }
    let Some(control) = children().lock().get(&operation_id).cloned() else {
        return;
    };
    operations::mark_operation_stage(&project, &operation_id, "process_running");
    let started = Instant::now();
    let mut result_seen_at: Option<Instant> = None;
    let mut last_result_poll = Instant::now();
    let exit_status: Option<i32> = loop {
        if control.cancelled.load(Ordering::Acquire) {
            break None;
        }
        let mut child = control.child.lock();
        match child.try_wait() {
            Ok(Some(status)) => break Some(status.code().unwrap_or(-1)),
            Ok(None) => {}
            Err(error) => {
                let _ = operations::mark_operation_state(
                    &project,
                    &operation_id,
                    OperationStatus::Failed,
                    StateSource::Native,
                    Some("failed".into()),
                    None,
                    Some(OperationError::new(
                        "WORKER_WAIT_FAILED",
                        error.to_string(),
                        true,
                    )),
                );
                break None;
            }
        }
        drop(child);
        // A CLI that printed its terminal result but does not exit would hold the turn open until
        // the deadline: once the result is in the output, give it a short grace to exit, then
        // stop it and finish the turn from that result.
        if last_result_poll.elapsed() >= Duration::from_millis(RESULT_POLL_MS) {
            last_result_poll = Instant::now();
            if result_seen_at.is_none() {
                let stdout = stream_text(&project, &operation_id, "stdout");
                if crate::tui::profile_for(&agent_type).headless_result_ready(&stdout) {
                    result_seen_at = Some(Instant::now());
                }
            }
        }
        if result_seen_at
            .is_some_and(|seen| seen.elapsed() >= Duration::from_millis(RESULT_EXIT_GRACE_MS))
        {
            tracing::warn!(operation_id = %operation_id, "headless child printed its result but did not exit; stopping it");
            if terminate(&control).is_ok() {
                break Some(0);
            }
        }
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            if let Err(stop_error) = terminate(&control) {
                let failure = OperationError::new("WORKER_STOP_FAILED", stop_error.message, true);
                let _ = operations::mark_operation_state(
                    &project,
                    &operation_id,
                    OperationStatus::Failed,
                    StateSource::Native,
                    Some("stop_failed".into()),
                    None,
                    Some(failure),
                );
                break None;
            }
            let mut error = OperationError::new(
                "TIMEOUT",
                "agent exceeded its wall-clock deadline and was terminated",
                true,
            );
            error.context = serde_json::json!({"timeout_ms": timeout_ms});
            let _ = operations::mark_operation_state(
                &project,
                &operation_id,
                OperationStatus::Failed,
                StateSource::Native,
                Some("timeout".into()),
                None,
                Some(error),
            );
            break None;
        }
        thread::sleep(Duration::from_millis(75));
    };
    for reader in readers {
        let _ = reader.join();
    }
    let current = operations::get_operation(&project, &operation_id).ok();
    if current.as_ref().is_some_and(|op| op.status.terminal()) {
        children().lock().remove(&operation_id);
        publish(&project, &operation_id, &registry, &app);
        return;
    }
    if let Some(code) = exit_status {
        finish_from_output(&project, &operation_id, &agent_type, &spec.program, code);
    }
    children().lock().remove(&operation_id);
    publish(&project, &operation_id, &registry, &app);
}

/// How often the wait loop looks for a printed result, and how long the child gets to exit after it.
const RESULT_POLL_MS: u64 = 400;
const RESULT_EXIT_GRACE_MS: u64 = 3000;

pub(super) fn fail_start(project: &str, id: &str, code: &str, message: &str) {
    let error = OperationError::new(code, message, false);
    let _ = operations::mark_operation_state(
        project,
        id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("failed".into()),
        None,
        Some(error),
    );
}

pub(in crate::agent_runs) fn publish(
    project: &str,
    id: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) {
    if let Ok(snapshot) = operations::get_operation(project, id) {
        let _ = crate::bridge::publish_operation(&snapshot, registry, app);
    }
}
