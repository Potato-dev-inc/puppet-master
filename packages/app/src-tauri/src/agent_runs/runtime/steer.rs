//! `send_message`: steering a running turn live, by restart, or by queueing for the next turn.

use super::{
    access::current_agent, control::interrupt_active_turns, followup::send_agent, view::SteerView,
};
use crate::agent_runs::capabilities::{self};
use crate::agent_runs::messaging::{self, DeliveryMode, Disposition};
use crate::agent_runs::persist::FollowupOpts;
use crate::agent_runs::{native, transcript};
use crate::operations::{OperationError, OperationSnapshot, OperationStatus};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

pub fn send_message(
    project: &str,
    handle: &str,
    message: String,
    session_id: Option<&str>,
    delivery: Option<&str>,
    idempotency_key: Option<&str>,
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, OperationError> {
    let mut snapshot = current_agent(project, handle, session_id)?;
    // A backend that takes live steers has no session to steer until its prompt is dispatched.
    // Sending now would queue the message and expire it at run end, losing the correction, so
    // hold a short moment for dispatch instead (the pane takes a few seconds to open).
    if capabilities::for_snapshot(&snapshot).live_messages
        && !snapshot.status.terminal()
        && snapshot.stage.as_deref() != Some("dispatched_waiting_for_task_completion")
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(250));
            match current_agent(project, handle, session_id) {
                Ok(latest) => {
                    let ready = latest.stage.as_deref()
                        == Some("dispatched_waiting_for_task_completion")
                        || latest.status.terminal()
                        || latest.status == OperationStatus::WaitingInput;
                    snapshot = latest;
                    if ready {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }
    // Register the steer before re-reading the turn so the supervisor cannot complete it on the
    // superseded answer between this check and the message reaching the session.
    let _steer_in_flight = (capabilities::for_snapshot(&snapshot).live_messages
        && !snapshot.status.terminal())
    .then(|| native::SteerGuard::begin(&snapshot.operation_id));
    if _steer_in_flight.is_some() {
        snapshot = current_agent(project, handle, session_id)?;
    }
    let mode = DeliveryMode::parse(delivery);
    let mut receipt =
        messaging::send_message(project, handle, &message, &snapshot, mode, idempotency_key)?;
    if receipt.disposition == Disposition::AcceptedViaRestart {
        if let Err(error) =
            restart_with_steer(project, handle, &message, session_id, &registry, &app)
        {
            if mode == DeliveryMode::Live {
                return Err(error);
            }
            // The restart did not happen (or could not be started): hold the message for the
            // next turn like any other backend that cannot take live steers.
            receipt.disposition = Disposition::Queued;
            messaging::update_receipt(project, handle, &receipt)?;
        }
    }
    if receipt.disposition == Disposition::Accepted {
        if let Err(error) = deliver_live(&snapshot, &message, &registry, &app) {
            if mode == DeliveryMode::Live {
                return Err(error);
            }
            receipt.disposition = Disposition::Queued;
            messaging::update_receipt(project, handle, &receipt)?;
        }
    }
    if matches!(
        receipt.disposition,
        Disposition::Accepted
            | Disposition::AcceptedViaRestart
            | Disposition::Queued
            | Disposition::Deferred
    ) {
        transcript::append(project, &snapshot.operation_id, "user_steer", &crate::redact::text(&message));
    }
    let mut value = messaging::receipt_value(&receipt);
    let live = capabilities::for_snapshot(&snapshot).live_messages;
    if let Some(object) = value.as_object_mut() {
        object.insert("supports_live_steer".into(), json!(live));
        if receipt.disposition == Disposition::AcceptedViaRestart {
            object.insert(
                "note".into(),
                json!("steered by restart: the running turn was stopped and this message started a new turn on the same provider session (--resume); unfinished work from the stopped turn is not carried over. Use wait_agents on this handle for the new turn's result"),
            );
        }
        if !live
            && matches!(
                receipt.disposition,
                Disposition::Queued | Disposition::Deferred
            )
        {
            object.insert(
                "note".into(),
                json!("this backend cannot take live steers: the message is queued for the next turn (followup_task) and will not change the turn already running"),
            );
        }
    }
    if let Ok(current) = current_agent(project, handle, session_id) {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "next_cursor".into(),
                json!(current.worker.event_cursor.max(current.revision)),
            );
            object.insert(
                "last_steer".into(),
                serde_json::to_value(SteerView {
                    message_id: receipt.message_id.clone(),
                    state: receipt.disposition.as_str().into(),
                    turn_id: receipt.turn_id.clone(),
                    result: receipt.result.clone(),
                })
                .unwrap_or(serde_json::Value::Null),
            );
        }
    }
    Ok(value)
}

/// Steer a busy headless run by stopping its child and starting the next turn on the same handle
/// with the message as the prompt; the launch carries `--resume <provider session>`.
fn restart_with_steer(
    project: &str,
    handle: &str,
    message: &str,
    session_id: Option<&str>,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) -> Result<(), OperationError> {
    interrupt_active_turns(
        project,
        handle,
        session_id,
        |snapshot| {
            if !messaging::can_restart_steer(snapshot) {
                return Err(OperationError::new(
                    "DELIVERY_UNSUPPORTED",
                    "the run can no longer be restarted with a steer",
                    false,
                ));
            }
            match crate::agent_runs::headless::cancel_headless(snapshot) {
                Err(error) if error.code != "WORKER_NOT_FOUND" => Err(error),
                _ => Ok(()),
            }
        },
        |interrupted| crate::bridge::publish_operation(interrupted, registry, app),
    )?;
    send_agent(
        project,
        handle,
        message.to_string(),
        session_id.map(str::to_string),
        registry.clone(),
        app.clone(),
        FollowupOpts {
            background: true,
            wait_ms: Some(0),
            ..FollowupOpts::default()
        },
    )
    .map(|_| ())
}

fn deliver_live(
    snapshot: &OperationSnapshot,
    message: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
) -> Result<(), OperationError> {
    if !capabilities::for_snapshot(snapshot).live_messages {
        return Err(OperationError::new(
            "DELIVERY_UNSUPPORTED",
            "live steering is only supported for backends that claim live_messages",
            false,
        ));
    }
    let pane_id = snapshot.pane_id.as_deref().ok_or_else(|| {
        OperationError::new(
            "DELIVERY_UNSUPPORTED",
            "native worker has no pane for live steering",
            true,
        )
    })?;
    crate::opencode::write_native_input(registry, app, pane_id, message, None)
        .map_err(|error| OperationError::new("DELIVERY_FAILED", error, true))
}
