//! Registry of live headless children and the ways to stop one: terminate its process tree, cancel
//! it by operation id, and recognise a run parked on a workspace-trust screen (no child alive).

use crate::operations::{OperationError, OperationSnapshot, OperationStatus};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashMap;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::Duration;

pub(super) struct HeadlessControl {
    pub(super) child: Mutex<Child>,
    pub(super) cancelled: AtomicBool,
}

static CHILDREN: OnceLock<Mutex<HashMap<String, Arc<HeadlessControl>>>> = OnceLock::new();

pub(super) fn children() -> &'static Mutex<HashMap<String, Arc<HeadlessControl>>> {
    CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn terminate(control: &HeadlessControl) -> Result<(), OperationError> {
    let mut child = control.child.lock();
    let pid = child.id();
    if child
        .try_wait()
        .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?
        .is_some()
    {
        control.cancelled.store(true, Ordering::Release);
        return Ok(());
    }
    #[cfg(windows)]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
        if !output.status.success() && child.try_wait().ok().flatten().is_none() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut error = OperationError::new(
                "WORKER_STOP_FAILED",
                "taskkill could not stop the worker process tree",
                true,
            );
            error.context = json!({
                "exit_code": output.status.code(),
                "stderr": stderr.trim(),
                "pid": pid,
                "stopped": false
            });
            return Err(error);
        }
    }
    #[cfg(unix)]
    {
        let process_group = format!("-{pid}");
        let term_status = Command::new("kill")
            .args(["-TERM", process_group.as_str()])
            .status()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
        if !term_status.success() && child.try_wait().ok().flatten().is_none() {
            return Err(OperationError::new(
                "WORKER_STOP_FAILED",
                "could not signal worker process group",
                true,
            ));
        }
        thread::sleep(Duration::from_millis(150));
        let kill_status = Command::new("kill")
            .args(["-KILL", process_group.as_str()])
            .status()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
        if !kill_status.success() && child.try_wait().ok().flatten().is_none() {
            return Err(OperationError::new(
                "WORKER_STOP_FAILED",
                "could not terminate worker process group",
                true,
            ));
        }
    }
    if child
        .try_wait()
        .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?
        .is_none()
    {
        child
            .kill()
            .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
    }
    child
        .wait()
        .map_err(|error| OperationError::new("WORKER_STOP_FAILED", error.to_string(), true))?;
    control.cancelled.store(true, Ordering::Release);
    Ok(())
}

pub(in crate::agent_runs) fn cancel_headless(
    operation: &OperationSnapshot,
) -> Result<(), OperationError> {
    let control = children().lock().get(&operation.operation_id).cloned();
    let Some(control) = control else {
        // A run parked on a workspace-trust prompt has already exited; cancelling just closes it.
        if parked_on_trust(operation) {
            return Ok(());
        }
        return Err(OperationError::new(
            "WORKER_NOT_FOUND",
            "worker process is not active",
            false,
        ));
    };
    terminate(&control)
}

/// A headless run whose process exited on the workspace-trust screen. No worker is alive, so
/// the only ways forward are a retry or a cancel. Status is not checked here because the cancel
/// hook runs after the operation has already moved to `Cancelling`.
fn parked_on_trust(operation: &OperationSnapshot) -> bool {
    operation.pane_id.is_none()
        && matches!(
            operation.status,
            OperationStatus::WaitingInput | OperationStatus::Cancelling
        )
        && operation
            .required_action
            .as_ref()
            .and_then(|action| action.get("kind"))
            .and_then(serde_json::Value::as_str)
            == Some("workspace_trust")
        && !children().lock().contains_key(&operation.operation_id)
}

/// Parked on trust and still waiting (not already being cancelled): eligible for a send_agent retry.
pub(in crate::agent_runs) fn blocked_on_trust(operation: &OperationSnapshot) -> bool {
    operation.status == OperationStatus::WaitingInput && parked_on_trust(operation)
}
