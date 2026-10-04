//! The extra fields `inspect_agent` adds to a run view (worker facts, continuity, stage timings).

use super::{opt_string, visible_mismatch};
use crate::operations::{OperationSnapshot, OperationStatus};
use serde_json::{json, Value};

pub fn apply_inspect_fields(
    value: &mut Value,
    snapshot: &OperationSnapshot,
    siblings: &[OperationSnapshot],
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let handle = if snapshot.agent_run_id.is_empty() {
        snapshot.operation_id.as_str()
    } else {
        snapshot.agent_run_id.as_str()
    };
    let queued: Vec<String> = siblings
        .iter()
        .filter(|item| item.agent_run_id == handle && item.status == OperationStatus::Queued)
        .map(|item| item.operation_id.clone())
        .collect();
    object.insert("backend".into(), Value::String(snapshot.agent_type.clone()));
    // Callers that never learned a backend cannot take live steers (Cursor) see this up front
    // instead of finding out from an `expired` receipt.
    object.insert(
        "supports_live_steer".into(),
        Value::Bool(crate::agent_runs::capabilities::for_snapshot(snapshot).live_messages),
    );
    object.insert("current_task".into(), Value::String(snapshot.task.clone()));
    object.insert(
        "name".into(),
        snapshot
            .worker
            .name
            .clone()
            .map(Value::String)
            .unwrap_or(Value::String(handle.to_string())),
    );
    object.insert("role".into(), opt_string(snapshot.worker.role.as_deref()));
    object.insert("scope".into(), opt_string(snapshot.worker.scope.as_deref()));
    object.insert(
        "workspace".into(),
        snapshot
            .worker
            .workspace
            .clone()
            .map(Value::String)
            .unwrap_or_else(|| Value::String(snapshot.project_path.clone())),
    );
    object.insert(
        "provider_session_id".into(),
        opt_string(snapshot.worker.provider_session_id.as_deref()),
    );
    object.insert(
        "session_reset".into(),
        Value::Bool(snapshot.worker.session_reset),
    );
    object.insert(
        "context_policy".into(),
        Value::String(snapshot.worker.context_policy.as_str().into()),
    );
    object.insert(
        "context_continuity".into(),
        Value::String(snapshot.worker.context_continuity.as_str().into()),
    );
    object.insert(
        "requested_model".into(),
        opt_string(snapshot.worker.requested_model.as_deref()),
    );
    object.insert(
        "resolved_model".into(),
        opt_string(snapshot.worker.resolved_model.as_deref()),
    );
    object.insert(
        "requested_reasoning".into(),
        opt_string(snapshot.worker.requested_reasoning.as_deref()),
    );
    object.insert(
        "resolved_reasoning".into(),
        opt_string(snapshot.worker.resolved_reasoning.as_deref()),
    );
    object.insert(
        "model_mismatch".into(),
        Value::Bool(visible_mismatch(
            snapshot.worker.requested_model.as_deref(),
            snapshot.worker.resolved_model.as_deref(),
            snapshot.worker.requested_reasoning.as_deref(),
            snapshot.worker.resolved_reasoning.as_deref(),
        )),
    );
    object.insert("current_turn".into(), json!(snapshot.turn_index));
    let pending = if snapshot.worker.pending_messages.is_empty() {
        crate::agent_runs::messaging::pending_texts(&snapshot.project_path, handle)
    } else {
        snapshot.worker.pending_messages.clone()
    };
    object.insert("pending_messages".into(), json!(pending));
    object.insert("queued_tasks".into(), json!(queued));
    object.insert(
        "ownership".into(),
        opt_string(snapshot.owner_session_id.as_deref()),
    );
    object.insert(
        "event_cursor".into(),
        json!(snapshot.worker.event_cursor.max(snapshot.revision)),
    );
    object.insert(
        "closed".into(),
        json!(snapshot.worker.closed || snapshot.worker.pane_disposed),
    );
    object.insert("pane_disposed".into(), json!(snapshot.worker.pane_disposed));
    object.insert("stage_timings".into(), stage_timings(snapshot));
    object.insert(
        "capabilities".into(),
        crate::agent_runs::capabilities::for_snapshot(snapshot).value(),
    );
    if let Some(receipt) =
        crate::agent_runs::messaging::latest_steer(&snapshot.project_path, handle)
    {
        object.insert(
            "last_steer".into(),
            json!({
                "message_id": receipt.message_id,
                "state": receipt.disposition.as_str(),
                "turn_id": receipt.turn_id,
                "result": receipt.result,
            }),
        );
    }
}

/// Per-stage durations: each stage lasts until the next one starts (the last until the run
/// finished, or `updated_at_ms` while it is still running). The first is measured from creation.
fn stage_timings(snapshot: &OperationSnapshot) -> Value {
    let log = &snapshot.worker.stage_log;
    let end = snapshot.finished_at_ms.unwrap_or(snapshot.updated_at_ms);
    let mut rows = Vec::new();
    for (index, mark) in log.iter().enumerate() {
        let until = log.get(index + 1).map_or(end, |next| next.at_ms);
        rows.push(json!({
            "stage": mark.stage,
            "entered_after_ms": mark.at_ms.saturating_sub(snapshot.created_at_ms),
            "duration_ms": until.saturating_sub(mark.at_ms),
        }));
    }
    json!(rows)
}
