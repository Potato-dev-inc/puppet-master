//! Run a command in an explicitly selected interactive shell pane.
mod command;
#[cfg(test)]
mod tests;

use crate::operations::{OperationError, OperationStatus};
use crate::pty::{registry_read_buffer, PaneRegistry};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

static PANE_LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Execute in the named shell pane. It never chooses a pane implicitly.
pub fn execute(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    command: &str,
    timeout_ms: u64,
) -> Result<Value, OperationError> {
    execute_in(registry, pane_id, command, timeout_ms, None)
}

/// Execute in a selected shell pane and optionally set its working directory
/// immediately before the command.
pub fn execute_in(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    command: &str,
    timeout_ms: u64,
    cwd: Option<&str>,
) -> Result<Value, OperationError> {
    if pane_id.trim().is_empty() {
        return Err(OperationError::new(
            "PANE_REQUIRED",
            "pane_id is required",
            false,
        ));
    }
    if command.trim().is_empty() || command.contains('\0') || command.len() > 64 * 1024 {
        return Err(OperationError::new(
            "INVALID_COMMAND",
            "command must be non-empty, contain no NUL bytes, and be at most 64 KiB",
            false,
        ));
    }

    let pane_lock = PANE_LOCKS
        .lock()
        .entry(pane_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    let _serialized = pane_lock.lock();

    let (agent_type, writer, exited) = {
        let reg = registry.lock();
        let pane = reg.panes.get(pane_id).ok_or_else(|| {
            OperationError::new(
                "PANE_NOT_FOUND",
                crate::pty::registry::unknown_pane_message(&pane_id),
                false,
            )
        })?;
        if !crate::pty::status::is_shell_agent(&pane.info.agent_type) {
            return Err(OperationError::new(
                "SHELL_PANE_REQUIRED",
                "shell execution requires an explicitly selected cmd, powershell, or bash pane",
                false,
            ));
        }
        if *pane.exited.lock() {
            return Err(OperationError::new(
                "PANE_UNAVAILABLE",
                "shell pane has exited",
                true,
            ));
        }
        (
            pane.info.agent_type.clone(),
            pane.writer.clone(),
            pane.exited.clone(),
        )
    };

    wait_for_shell_prompt(registry, pane_id, &agent_type)?;

    let marker = format!("__PM_EXEC_{}__", Uuid::new_v4().simple());
    let payload = command::build(&agent_type, command, &marker, cwd);
    let started = Instant::now();
    {
        let mut writer = writer.lock();
        writer.write_all(payload.as_bytes()).map_err(|err| {
            OperationError::new("PANE_IO_ERROR", format!("write command: {err}"), true)
        })?;
        writer.write_all(b"\r").map_err(|err| {
            OperationError::new("PANE_IO_ERROR", format!("submit command: {err}"), true)
        })?;
        writer.flush().map_err(|err| {
            OperationError::new("PANE_IO_ERROR", format!("flush command: {err}"), true)
        })?;
    }

    let timeout = Duration::from_millis(timeout_ms.clamp(1, 600_000));
    loop {
        let output = registry_read_buffer(registry, pane_id, 10_000).unwrap_or_default();
        let partial = command::output_for_command(&output, &marker);
        if let Some((exit_code, cwd)) = command::parse_completion(&output, &marker, &agent_type) {
            return Ok(json!({
                "pane_id": pane_id,
                "exit_code": exit_code,
                "duration_ms": started.elapsed().as_millis() as u64,
                "cwd": cwd,
                "stdout": partial,
                "status": OperationStatus::Completed,
            }));
        }
        if *exited.lock() {
            let mut error = OperationError::new(
                "PANE_UNAVAILABLE",
                "shell exited before command completion marker",
                true,
            );
            error.context = json!({"status": "failed", "pane_id": pane_id, "stdout": partial, "exit_code": null, "duration_ms": started.elapsed().as_millis() as u64});
            return Err(error);
        }
        if started.elapsed() >= timeout {
            // The explicit pane_id and shell-type checks above are required
            // before sending Ctrl-C; agent/TUI panes are never interrupted here.
            let _ = writer.lock().write_all(&[0x03]);
            let _ = writer.lock().flush();
            let mut error = OperationError::new(
                "COMMAND_TIMEOUT",
                "shell command timed out and was interrupted",
                true,
            );
            error.retry_after_ms = Some(250);
            error.context = json!({"status": "timeout", "pane_id": pane_id, "stdout": partial, "exit_code": null, "duration_ms": started.elapsed().as_millis() as u64});
            return Err(error);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_shell_prompt(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    agent_type: &str,
) -> Result<(), OperationError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let info = registry
            .lock()
            .panes
            .get(pane_id)
            .map(|pane| pane.info())
            .ok_or_else(|| {
                OperationError::new("PANE_NOT_FOUND", "shell pane disappeared", false)
            })?;
        match info.status.as_str() {
            "waiting_input" => return Ok(()),
            "idle" => {
                let screen =
                    crate::pty::registry_read_snapshot(registry, pane_id).unwrap_or_default();
                if crate::pty::status::shell_prompt_visible(agent_type, &screen) {
                    return Ok(());
                }
            }
            "error" => {
                return Err(OperationError::new(
                    "PANE_UNAVAILABLE",
                    "shell pane exited before it became ready",
                    true,
                ));
            }
            _ if Instant::now() >= deadline => {
                let mut error = OperationError::new(
                    "PANE_NOT_READY",
                    "shell pane did not reach its prompt within 10 seconds",
                    true,
                );
                error.retry_after_ms = Some(500);
                error.context = json!({"pane_id":pane_id,"status":info.status});
                return Err(error);
            }
            _ => thread::sleep(Duration::from_millis(25)),
        }
    }
}
