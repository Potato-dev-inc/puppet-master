//! Manual-approval detection for freshly started workers: which screens or native permission
//! lists mean a human must act before a prompt may be typed, and the wait for that to clear.

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pty::{registry_read_snapshot, PaneRegistry};

pub(super) fn explicit_manual_approval_required(
    agent_type: &str,
    transcript: &str,
    pending_permission_ids: &[String],
) -> bool {
    if agent_type == "opencode_native" && !pending_permission_ids.is_empty() {
        return true;
    }
    let lower = transcript.to_ascii_lowercase();
    [
        "approval required",
        "requires your approval",
        "permission required",
        "permission request",
        "allow this command",
        "do you want to allow",
        "approve this command",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
}

pub(super) fn startup_manual_approval_required(
    agent_type: &str,
    pane_status: Option<&str>,
    current_screen: &str,
    pending_permission_ids: &[String],
) -> bool {
    if agent_type == "opencode_native" && !pending_permission_ids.is_empty() {
        return true;
    }
    // A dialog the TUI profile recognises (Cursor's workspace trust, a command approval) blocks
    // typing whatever the pane status says: status trails the screen, and a prompt typed into the
    // dialog is read as hotkeys (`a` trusts, `q` quits).
    if agent_type != "opencode_native"
        && crate::pane_prompt::detect_in_screen(agent_type, current_screen, false).is_some()
    {
        return true;
    }
    // A startup gate (`TuiProfile::startup_gate`) still up. One that only the user may pass (trust,
    // sign-in) is already a prompt card caught above; what this adds is a dismissible notice that
    // ignored its keys. Either way the task must not be typed into it.
    if agent_type != "opencode_native"
        && crate::tui::profile_for(agent_type).startup_gate(current_screen).is_some()
    {
        return true;
    }
    pane_status == Some("waiting_input")
        && explicit_manual_approval_required(agent_type, current_screen, pending_permission_ids)
}

pub(super) fn native_status_unavailable(
    error: impl std::fmt::Display,
) -> crate::operations::OperationError {
    crate::operations::OperationError::new(
        "NATIVE_STATUS_UNAVAILABLE",
        format!("could not verify native worker permissions: {error}"),
        true,
    )
}

pub(super) fn wait_for_startup_permission_resolution(
    registry: &Arc<Mutex<PaneRegistry>>,
    project: &str,
    operation_id: &str,
    pane_id: &str,
    agent_type: &str,
) -> Result<String, crate::operations::OperationError> {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut wake_generation = 0_u64;
    // The startup gate (folder trust) last seen on this pane. If the agent exits while one was
    // showing, the user declined it.
    let mut gate: Option<serde_json::Value> = None;
    // When the screen was first seen without the dialog (a drawn screen), for `RESOLVED_CONFIRM`.
    let mut clear_since: Option<Instant> = None;
    loop {
        if let Some(seen) = crate::pane_prompt::startup_gate(
            agent_type,
            &registry_read_snapshot(registry, pane_id).unwrap_or_default(),
        ) {
            crate::pane_prompt::note_gate_seen(pane_id);
            gate = Some(seen);
        }
        let operation = crate::operations::get_operation(project, operation_id)?;
        if operation.status == crate::operations::OperationStatus::Cancelling
            || operation.status == crate::operations::OperationStatus::Cancelled
        {
            return Err(crate::operations::OperationError::new(
                "OPERATION_CANCELLED",
                "operation was cancelled while waiting for manual permission resolution",
                false,
            ));
        }
        // A decline is final: the user answered "deny" on the gate's card, so the agent is leaving
        // (or has left). Whether its pane shows that yet, or exits at all, does not matter.
        if crate::pane_prompt::gate_was_declined(pane_id) {
            return Err(declined_error(pane_id, gate.as_ref()));
        }
        let pane = registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
            .ok_or_else(|| {
                gate_declined(pane_id, gate.as_ref()).unwrap_or_else(|| {
                    crate::operations::OperationError::new(
                        "PANE_UNAVAILABLE",
                        "assigned pane disappeared while waiting for permission resolution",
                        true,
                    )
                })
            })?;
        if pane.status == "error" {
            if let Some(declined) = gate_declined(pane_id, gate.as_ref()) {
                return Err(declined);
            }
            return Err(crate::operations::OperationError::new(
                "PANE_READINESS_FAILED",
                "assigned pane entered an error state while waiting for permission resolution",
                true,
            ));
        }
        let screen = registry_read_snapshot(registry, pane_id).unwrap_or_default();
        let permissions = if agent_type == "opencode_native" {
            crate::opencode::status::worker_status(registry, pane_id)
                .map_err(|error| {
                    crate::operations::OperationError::new("NATIVE_STATUS_UNAVAILABLE", error, true)
                })?
                .pending_permission_ids
        } else {
            Vec::new()
        };
        // The gate was on screen and the agent is gone: declining the folder-trust dialog exits the
        // agent, but the pane's host shell stays alive at its prompt (live: the task was typed into
        // PowerShell and the run "completed" with its error). A gate seen and then a bare host-shell
        // prompt where the agent was is a decline, whatever the pane status says and whether or not
        // the dialog's text is still on screen above the prompt.
        if gate.is_some()
            && agent_type != "opencode_native"
            && crate::tui::agent_gone_to_host_shell(agent_type, &screen)
        {
            return Err(declined_error(pane_id, gate.as_ref()));
        }
        // Same test that parked the run: phrases a CLI prints, or a dialog its TUI profile
        // recognises (Cursor's trust dialog prints none of the phrases).
        let still_blocked = startup_still_blocked(agent_type, &screen, &permissions);
        // A dialog is gone only when the screen shows something else and keeps not showing it:
        // one look at a blank or half-drawn redraw of the same dialog (a resize, a key the person
        // pressed inside it) used to release the run, which then found the dialog again and
        // failed PANE_BLOCKED_BY_PROMPT (live, plain mode, Claude's folder-trust question).
        if still_blocked || (agent_type != "opencode_native" && screen.trim().is_empty()) {
            clear_since = None;
        } else {
            let since = *clear_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= RESOLVED_CONFIRM {
                return Ok(pane.status);
            }
        }
        if Instant::now() >= deadline {
            return Err(crate::operations::OperationError::new(
                "APPROVAL_WAIT_FAILED",
                "permission prompt remained unresolved for five minutes",
                true,
            ));
        }
        // While a release is being confirmed, look again soon even if nothing is printed.
        let wake_by = match clear_since {
            Some(_) => deadline.min(Instant::now() + RESOLVED_CONFIRM / 4),
            None => deadline,
        };
        crate::pane_wait_notify::wait_for_change(wake_by, &mut wake_generation);
    }
}

/// How long a startup dialog must stay off a drawn screen before the run is released.
pub(super) const RESOLVED_CONFIRM: Duration = Duration::from_millis(400);

/// Whether a startup dialog (or a CLI's approval phrase, or a native permission) still blocks the pane.
pub(super) fn startup_still_blocked(agent_type: &str, screen: &str, permissions: &[String]) -> bool {
    explicit_manual_approval_required(agent_type, screen, permissions)
        || (agent_type != "opencode_native"
            && (crate::pane_prompt::detect_in_screen(agent_type, screen, false).is_some()
                || crate::tui::profile_for(agent_type).startup_gate(screen).is_some()))
}

/// The run's error when the pane's agent exited while a startup gate was on screen: the user
/// declined it (the folder-trust dialog offers only "trust" or "exit"). Not recoverable and not a
/// pre-turn failure, so the run neither retries nor falls back to a headless run that would work
/// in the folder the user just refused.
/// [`gate_declined`] for a decline that is known for certain (the user answered "deny", or the
/// agent's pane fell back to its shell): with no gate details on hand it is the folder-trust error.
pub(crate) fn declined_error(pane_id: &str, gate: Option<&serde_json::Value>) -> crate::operations::OperationError {
    gate_declined(pane_id, gate).unwrap_or_else(|| gate_declined(pane_id, Some(&serde_json::json!({"kind": "workspace_trust"}))).expect("a trust decline"))
}

fn gate_declined(pane_id: &str, gate: Option<&serde_json::Value>) -> Option<crate::operations::OperationError> {
    let gate = gate?;
    if gate.get("kind").and_then(|v| v.as_str()) == Some("startup_screen") {
        // A sign-in or setup screen the agent exited on (the user closed it, or it failed).
        let summary = gate.get("summary").and_then(|v| v.as_str()).unwrap_or("A startup screen");
        let mut error = crate::operations::OperationError::new(
            "STARTUP_SCREEN_NOT_COMPLETED",
            format!("{summary}, and the agent exited before it was done, so the task was not typed. Finish it, then start the run again."),
            false,
        );
        error.context = serde_json::json!({"pane_id": pane_id, "kind": "startup_screen", "startup_gate": gate.get("startup_gate")});
        return Some(error);
    }
    let folder = gate.get("folder").and_then(|v| v.as_str());
    let message = match folder {
        Some(folder) => format!("The folder {folder} was not trusted, so the agent exited and the task was not typed. Start the run again and choose Trust to let the agent work there."),
        None => "The folder was not trusted, so the agent exited and the task was not typed. Start the run again and choose Trust to let the agent work there.".to_string(),
    };
    let mut error = crate::operations::OperationError::new("WORKSPACE_TRUST_DECLINED", message, false);
    error.context = serde_json::json!({"pane_id": pane_id, "kind": gate.get("kind"), "folder": folder});
    Some(error)
}
