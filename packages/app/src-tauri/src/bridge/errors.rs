//! Error mapping between `OperationError` and the bridge's `(status, json)` HTTP errors.

use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

use crate::pty::PaneRegistry;

/// A bridge response: `(status, json body)`.
pub(super) type HttpResponse = (u16, serde_json::Value);

pub(super) fn operation_http_error(
    error: crate::operations::OperationError,
) -> (u16, serde_json::Value) {
    let status = match error.code.as_str() {
        "OPERATION_NOT_FOUND" | "AGENT_NOT_FOUND" | "PANE_NOT_FOUND" => 404,
        "AUTHORIZATION_DENIED" => 403,
        "MODE_MISMATCH"
        | "BUDGET_EXHAUSTED"
        | "AGENT_DEF_APPROVAL_STALE"
        | "AGENT_DEF_UNAPPROVED"
        | "AGENT_DEF_BINARY_CHANGED" => 409,
        "AGENT_NOT_INSTALLED" | "AGENT_DEF_SHIM_NEEDS_NATIVE" => 400,
        "WORKTREE_NOT_FOUND" => 404,
        "MERGE_CONFLICT"
        | "ORIGIN_DIRTY_OVERLAP"
        | "ORIGIN_DETACHED"
        | "ORIGIN_MERGE_IN_PROGRESS"
        | "RUN_ACTIVE" => 409,
        "NOT_A_GIT_REPO" | "NO_COMMITS" | "GIT_UNAVAILABLE" => 400,
        "INVALID_ARGUMENT"
        | "INVALID_TIMEOUT"
        | "INVALID_JSON"
        | "MISSING_FIELD"
        | "INVALID_FIELD"
        | "SESSION_REQUIRED"
        | "READ_ONLY_UNSUPPORTED"
        | "AGENT_NOT_DISPATCHABLE"
        | "WORKER_MCP_TOOLS_UNAVAILABLE" => 400,
        "RESOURCE_LOCKED"
        | "IDEMPOTENCY_KEY_CONFLICT"
        | "OPERATION_CANCELLED"
        | "DISPATCH_CANCELLED"
        | "PANE_BUSY"
        | "ACCOUNT_MISMATCH"
        | "OPERATION_ALREADY_DISPATCHED"
        | "CANCELLATION_IN_PROGRESS"
        | "CANCELLATION_REQUIRES_WORKER_CONTROL"
        | "PANE_UNAVAILABLE"
        | "PANE_RESERVATION_LOST"
        | "INVALID_STATE_TRANSITION" => 409,
        "INVALID_TASK"
        | "INVALID_IDEMPOTENCY_KEY"
        | "INVALID_ACCEPTANCE_CRITERIA"
        | "INVALID_PROJECT_PATH"
        | "INVALID_OPERATION_ID"
        | "INVALID_AGENT_TYPE"
        | "UNKNOWN_AGENT_TYPE"
        | "INVALID_LOCK"
        | "COMPLETION_EVIDENCE_REQUIRED" => 400,
        "PROJECT_PATH_REQUIRED" => 400,
        _ => 503,
    };
    (
        status,
        json!({"error": error.message, "message": error.message, "code": error.code, "recoverable": error.recoverable, "retry_after_ms": error.retry_after_ms, "context": error.context}),
    )
}

pub(super) fn operation_error_from_http(
    error: (u16, serde_json::Value),
) -> crate::operations::OperationError {
    serde_json::from_value(error.1.clone()).unwrap_or_else(|_| {
        crate::operations::OperationError::new(
            "BRIDGE_ERROR",
            error
                .1
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("bridge operation failed"),
            error.0 >= 500,
        )
    })
}

/// Upgrade a bare AGENT_NOT_FOUND into a kind-aware message when the id is really a pane id.
pub(super) fn enrich_handle_error(
    error: crate::operations::OperationError,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> crate::operations::OperationError {
    if error.code != "AGENT_NOT_FOUND" {
        return error;
    }
    let (project, cwds) = {
        let guard = registry.lock();
        (
            guard.project_path.clone(),
            guard
                .panes
                .iter()
                .map(|(id, pane)| (id.clone(), pane.info.cwd.clone()))
                .collect::<std::collections::HashMap<_, _>>(),
        )
    };
    let ids: Vec<String> = cwds.keys().cloned().collect();
    crate::mcp_sessions::enrich_handle_error_with(error, &ids, |pane_id| {
        crate::agent_runs::latest_handle_for_pane(&project, pane_id)
            .ok()
            .or_else(|| {
                cwds.get(pane_id)
                    .and_then(|cwd| crate::agent_runs::latest_handle_for_pane(cwd, pane_id).ok())
            })
    })
}

/// What a route handler returns.
pub(super) type RouteResult = Result<HttpResponse, HttpResponse>;

#[cfg(test)]
mod budget_tests {
    use super::operation_http_error;
    use crate::operations::OperationError;

    #[test]
    fn budget_exhausted_is_a_conflict_the_caller_can_act_on_not_an_outage() {
        let mut error = OperationError::new("BUDGET_EXHAUSTED", "paused", true);
        error.context = serde_json::json!({"reason": "turns"});
        let (status, body) = operation_http_error(error);
        assert_eq!(status, 409);
        assert_eq!(body["code"], "BUDGET_EXHAUSTED");
        assert_eq!(body["context"]["reason"], "turns");
    }
}
