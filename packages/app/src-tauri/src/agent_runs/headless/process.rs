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
    // A turn runs with the profile it was admitted under: editing or unregistering the definition
    // mid-run must not change how this turn is launched, parsed or finished.
    let profile = crate::tui::profile_for(&agent_type);
    crate::one_brain::sync_on_spawn(&project);
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
    let account = snapshot
        .as_ref()
        .and_then(|current| current.worker.account.clone())
        .filter(|id| !id.trim().is_empty());
    let requested_model = snapshot
        .as_ref()
        .and_then(|current| current.worker.requested_model.clone())
        .filter(|model| !model.trim().is_empty());
    if profile.probes_default_model()
        && snapshot
            .as_ref()
            .is_some_and(|current| current.worker.resolved_model.is_none())
    {
        // Some headless CLIs never say which model ran; record the CLI's configured default so
        // resolved_model is a real answer instead of null. Off-thread: it may shell out.
        let (project, operation_id) = (project.clone(), operation_id.clone());
        let probe_profile = profile.clone();
        let _ = thread::Builder::new()
            .name("puppet-default-model".into())
            .spawn(move || {
                if let Some(model) = probe_profile.default_model() {
                    // A configured default is a best guess, not a confirmation of what ran.
                    let _ = operations::record_unconfirmed_model(&project, &operation_id, &model);
                }
            });
    }
    // An isolated run launches in its worktree; everything else keeps the project folder.
    let launch_dir = snapshot
        .as_ref()
        .map(|current| current.work_dir().to_string())
        .unwrap_or_else(|| project.clone());
    let isolated = snapshot
        .as_ref()
        .is_some_and(|current| current.worker.worktree.is_some());
    let spec = match crate::tui::headless_launch_with(
        &*profile,
        &agent_type,
        &crate::tui::HeadlessLaunch {
            project: &launch_dir,
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
    // Opt-in warm pool: a pre-launched process (startup already paid) takes the task over stdin.
    let pool_key = (resume_session.is_none() && !trust_workspace && !isolated)
        .then(|| {
            crate::standby::key_for_run(
                &agent_type,
                &project,
                requested_model.as_deref(),
                read_only,
                allow_shell,
                true,
                account.as_deref(),
            )
        })
        .flatten();
    let mut standby = pool_key
        .as_ref()
        .and_then(crate::standby::claim_headless)
        .and_then(|claim| claim.into_process());
    if let Some(key) = &pool_key {
        crate::standby::note_run(key, &project);
    }
    let account_env = match account.as_deref() {
        Some(id) => match crate::accounts::launch_env(&agent_type, id) {
            Ok(env) => env,
            Err(message) => {
                fail_start(&project, &operation_id, "LAUNCH_FAILED", &message);
                publish(&project, &operation_id, &registry, &app);
                return;
            }
        },
        None => crate::tui::LaunchEnv::default(),
    };
    let mut command = headless_command(&spec, &account_env);
    let mut readers = Vec::new();
    let dispatch = operations::run_operation_dispatch(&project, &operation_id, |_| {
        operations::dispatch_input_if_active(&project, &operation_id, |_| {
            let from_standby = standby
                .take()
                .and_then(|process| hand_task_to_standby(process, &profile, &task));
            if from_standby.is_some() {
                operations::mark_standby_claimed(&project, &operation_id);
            }
            let child = match from_standby {
                Some(child) => child,
                None => command.spawn().map_err(|error| {
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
            })?,
            };
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
                if profile.headless_result_ready(&stdout) {
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
        finish_from_output(&project, &operation_id, &profile, &spec.program, code);
    }
    children().lock().remove(&operation_id);
    publish(&project, &operation_id, &registry, &app);
}

/// Gives a claimed standby process its task and closes its stdin. `None` (the process is shut
/// down) when it died while it waited or cannot take the task: the run then spawns normally, so a
/// bad standby is never a user-visible failure.
fn hand_task_to_standby(
    mut process: crate::standby::StandbyProcess,
    profile: &Arc<dyn crate::tui::TuiProfile>,
    task: &str,
) -> Option<std::process::Child> {
    use std::io::Write;
    let Some(input) = profile.headless_standby_input(task) else {
        let _ = process.child.kill();
        let _ = process.child.wait();
        return None;
    };
    let delivered = process.child.stdin.take().is_some_and(|mut stdin| {
        stdin
            .write_all(input.as_bytes())
            .and_then(|()| stdin.flush())
            .is_ok()
        // `stdin` drops here: EOF tells the CLI the task is complete.
    });
    if delivered && process.child.try_wait().map_or(false, |status| status.is_none()) {
        Some(process.child)
    } else {
        let _ = process.child.kill();
        let _ = process.child.wait();
        None
    }
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

/// The child process for a headless run: the spec's program, args and cwd, piped output, and the
/// environment (the spec's `[env]` first, then the account's, which wins).
pub(crate) fn headless_command(spec: &crate::tui::LaunchSpec, account_env: &crate::tui::LaunchEnv) -> Command {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spec.apply_to_command(&mut command);
    for (key, value) in &account_env.set {
        command.env(key, value);
    }
    for key in &account_env.remove {
        command.env_remove(key);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

#[cfg(test)]
mod env_tests {
    use super::*;
    use crate::tui::{LaunchEnv, LaunchSpec};
    use std::io::Read;

    fn echo_spec(name: &str) -> LaunchSpec {
        let (program, args) = if cfg!(windows) {
            ("cmd.exe", vec!["/C".to_string(), format!("echo VAL=%{name}%")])
        } else {
            ("sh", vec!["-c".to_string(), format!("echo VAL=${name}")])
        };
        LaunchSpec::new(program, args, std::env::temp_dir(), false)
    }

    fn stdout_of(mut command: Command) -> String {
        let mut child = command.spawn().expect("spawn");
        let mut out = String::new();
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        let _ = child.wait();
        out
    }

    #[test]
    fn headless_run_gets_spec_env_then_account_env() {
        let mut spec = echo_spec("PM_HL_T");
        spec.env = vec![("PM_HL_T".into(), "from-spec".into())];
        assert!(stdout_of(headless_command(&spec, &LaunchEnv::default())).contains("VAL=from-spec"));
        let account = LaunchEnv { set: vec![("PM_HL_T".into(), "from-account".into())], remove: vec![] };
        assert!(stdout_of(headless_command(&spec, &account)).contains("VAL=from-account"));
    }

    #[test]
    fn headless_run_drops_removed_variables() {
        std::env::set_var("PM_HL_DROP", "leaked-value");
        let mut spec = echo_spec("PM_HL_DROP");
        spec.env_remove = vec!["PM_HL_DROP".into()];
        let out = stdout_of(headless_command(&spec, &LaunchEnv::default()));
        assert!(!out.contains("leaked-value"), "{out}");
    }
}
