//! `/tasks/*` routes: list, create, claim, lease renewal, status, completion, blocking and
//! reviewer assignment.

use serde::Deserialize;
use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;
use crate::bridge::http::{parse_json, requested_project_path};
use crate::bridge::locks::release_operation_locks;
use crate::bridge::projects::{resolve_task_project, validate_task_owner};
use crate::bridge::sse::push_operation_sse;
use crate::bridge::store::{append_operation_event, lease_expires_at, rebuild_read_models};
use crate::events::{SystemEvent, TaskId};

#[derive(Debug, Deserialize)]
struct CreateTaskBody {
    title: String,
    #[serde(default)]
    exclusive: bool,
}

#[derive(Debug, Deserialize)]
struct ClaimTaskBody {
    agent_id: String,
    lease_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TaskStatusBody {
    status: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CompleteTaskBody {
    agent_id: String,
    evidence: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BlockTaskBody {
    agent_id: String,
    reason: String,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssignReviewerBody {
    reviewer_id: String,
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["tasks"]) => list(),
        ("POST", ["tasks"]) => create(cx),
        ("POST", ["tasks", id, "claim"]) => claim(cx, id),
        ("POST", ["tasks", id, "lease"]) => lease(cx, id),
        ("POST", ["tasks", id, "status"]) => status(cx, id),
        ("POST", ["tasks", id, "complete"]) => complete(cx, id),
        ("POST", ["tasks", id, "block"]) => block(cx, id),
        ("POST", ["tasks", id, "reviewer"]) => reviewer(cx, id),
        _ => return None,
    })
}

fn list() -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.tasks).unwrap()))
}

fn create(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req: CreateTaskBody = parse_json(body)?;
    let task_id = TaskId::new();
    crate::event_log::append_bridge_event(SystemEvent::TaskCreated {
        task_id: task_id.clone(),
        title: req.title,
        exclusive: req.exclusive,
    });
    Ok((201, json!({ "task_id": task_id.0 })))
}

fn claim(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let task_id = TaskId(id.to_string());
    let req: ClaimTaskBody = parse_json(body)?;
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    let task = read_models
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (404, json!({ "error": "unknown task" })))?;
    if task.exclusive && task.claimed_by.is_some() && task.status == "claimed" {
        return Err((409, json!({ "error": "task already claimed" })));
    }
    crate::event_log::append_bridge_event(SystemEvent::TaskClaimed {
        task_id: task_id.clone(),
        agent_id: req.agent_id,
        lease_expires_at_ms: lease_expires_at(req.lease_ms),
    });
    Ok((200, json!({ "task_id": task_id.0, "claimed": true })))
}

fn lease(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let task_id = TaskId(id.to_string());
    let req: ClaimTaskBody = parse_json(body)?;
    crate::event_log::append_bridge_event(SystemEvent::TaskLeaseRenewed {
        task_id: task_id.clone(),
        agent_id: req.agent_id,
        lease_expires_at_ms: lease_expires_at(req.lease_ms),
    });
    Ok((200, json!({ "task_id": task_id.0, "renewed": true })))
}

fn status(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let task_id = TaskId(id.to_string());
    let req: TaskStatusBody = parse_json(body)?;
    if req.status == "completed" {
        return Err(operation_http_error(
            crate::operations::OperationError::new(
                "COMPLETION_EVIDENCE_REQUIRED",
                "use complete_task with evidence to complete a task",
                false,
            ),
        ));
    }
    let project = resolve_task_project(req.project_path.as_deref(), &task_id.0, registry)?;
    if req.status == "blocked" {
        let agent_id = req.agent_id.as_deref().ok_or_else(|| (400, json!({"error":"agent_id is required when blocking a task","code":"AGENT_ID_REQUIRED","recoverable":false,"retry_after_ms":null,"context":null})))?;
        validate_task_owner(&project.to_string_lossy(), &task_id.0, agent_id)?;
    }
    let event = if req.status == "blocked" {
        SystemEvent::TaskBlocked {
            task_id: task_id.clone(),
            agent_id: req.agent_id.clone().unwrap_or_else(|| "agent".into()),
            reason: req
                .reason
                .clone()
                .unwrap_or_else(|| "task reported blocked".into()),
        }
    } else {
        SystemEvent::TaskStatusUpdated {
            task_id: task_id.clone(),
            status: req.status.clone(),
        }
    };
    append_operation_event(&project.to_string_lossy(), event).map_err(|err| (500, json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true,"retry_after_ms":null,"context":null})))?;
    if req.status == "blocked" {
        match crate::operations::operation_for_task(&project.to_string_lossy(), &task_id.0) {
            Ok(operation) => {
                let error = crate::operations::OperationError::new(
                    "TASK_BLOCKED",
                    req.reason
                        .clone()
                        .unwrap_or_else(|| "task reported blocked".into()),
                    true,
                );
                let snapshot = crate::operations::mark_operation_state(
                    &project.to_string_lossy(),
                    &operation.operation_id,
                    crate::operations::OperationStatus::Failed,
                    crate::operations::StateSource::Native,
                    Some("task_blocked".into()),
                    None,
                    Some(error),
                )
                .map_err(operation_http_error)?;
                let _ = release_operation_locks(&snapshot);
                push_operation_sse(&snapshot);
            }
            Err(error) if error.code == "OPERATION_NOT_FOUND" => {}
            Err(error) => return Err(operation_http_error(error)),
        }
    }
    Ok((200, json!({ "task_id": task_id.0, "ok": true })))
}

fn complete(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let query = cx.query;
    let registry = cx.registry;
    let task_id = TaskId(id.to_string());
    let req: CompleteTaskBody = parse_json(body)?;
    let evidence = req.evidence.unwrap_or_default();
    if evidence.trim().is_empty() {
        return Err(operation_http_error(
            crate::operations::OperationError::new(
                "COMPLETION_EVIDENCE_REQUIRED",
                "evidence is required for task completion",
                false,
            ),
        ));
    }
    let project = resolve_task_project(
        requested_project_path(req.project_path.as_deref(), query).as_deref(),
        &task_id.0,
        registry,
    )?;
    validate_task_owner(&project.to_string_lossy(), &task_id.0, &req.agent_id)?;
    let snapshots =
        crate::operations::mark_task_completed(&project.to_string_lossy(), &task_id.0, &evidence)
            .map_err(operation_http_error)?;
    let task_event = SystemEvent::TaskCompleted {
        task_id: task_id.clone(),
        agent_id: req.agent_id,
        evidence: evidence.clone(),
    };
    append_operation_event(&project.to_string_lossy(), task_event).map_err(|err| (500, json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true,"retry_after_ms":null,"context":null})))?;
    for snapshot in snapshots {
        release_operation_locks(&snapshot).map_err(|err| {
            operation_http_error(crate::operations::OperationError::new(
                "LOCK_RELEASE_FAILED",
                err,
                true,
            ))
        })?;
        push_operation_sse(&snapshot);
    }
    Ok((200, json!({ "task_id": task_id.0, "completed": true })))
}

fn block(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let task_id = TaskId(id.to_string());
    let req: BlockTaskBody = parse_json(body)?;
    let reason = req.reason.clone();
    let project = resolve_task_project(req.project_path.as_deref(), &task_id.0, registry)?;
    validate_task_owner(&project.to_string_lossy(), &task_id.0, &req.agent_id)?;
    let event = SystemEvent::TaskBlocked {
        task_id: task_id.clone(),
        agent_id: req.agent_id,
        reason: req.reason,
    };
    append_operation_event(&project.to_string_lossy(), event).map_err(|err| (500, json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true,"retry_after_ms":null,"context":null})))?;
    match crate::operations::operation_for_task(&project.to_string_lossy(), &task_id.0) {
        Ok(operation) => {
            let error = crate::operations::OperationError::new("TASK_BLOCKED", reason, true);
            let snapshot = crate::operations::mark_operation_state(
                &project.to_string_lossy(),
                &operation.operation_id,
                crate::operations::OperationStatus::Failed,
                crate::operations::StateSource::Native,
                Some("task_blocked".into()),
                None,
                Some(error),
            )
            .map_err(operation_http_error)?;
            release_operation_locks(&snapshot).map_err(|err| {
                operation_http_error(crate::operations::OperationError::new(
                    "LOCK_RELEASE_FAILED",
                    err,
                    true,
                ))
            })?;
            push_operation_sse(&snapshot);
        }
        Err(error) if error.code == "OPERATION_NOT_FOUND" => {}
        Err(error) => return Err(operation_http_error(error)),
    }
    Ok((200, json!({ "task_id": task_id.0, "blocked": true })))
}

fn reviewer(cx: &Ctx, id: &str) -> RouteResult {
    let body = cx.body;
    let task_id = TaskId(id.to_string());
    let req: AssignReviewerBody = parse_json(body)?;
    crate::event_log::append_bridge_event(SystemEvent::ReviewerAssigned {
        task_id: task_id.clone(),
        reviewer_id: req.reviewer_id,
    });
    Ok((200, json!({ "task_id": task_id.0, "ok": true })))
}
