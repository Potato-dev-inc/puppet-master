//! `/locks/*` routes: list, acquire, release and expire resource locks.

use serde::Deserialize;
use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::http::parse_json;
use crate::bridge::projects::resolve_operation_project;
use crate::bridge::state::RESOURCE_LOCK_SERIAL;
use crate::bridge::store::{
    append_operation_event, lease_expires_at, rebuild_operation_read_models, rebuild_read_models,
};
use crate::events::{ResourceId, SystemEvent};

#[derive(Debug, Deserialize)]
struct AcquireLockBody {
    resource_type: String,
    name: String,
    owner_id: String,
    lease_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReleaseLockBody {
    resource_type: String,
    name: String,
    owner_id: String,
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["locks"]) => list(),
        ("POST", ["locks"]) => acquire(cx),
        ("POST", ["locks", "release"]) => release(cx),
        ("POST", ["locks", id, "expire"]) => expire(id),
        _ => return None,
    })
}

fn list() -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.locks).unwrap()))
}

fn acquire(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: AcquireLockBody = parse_json(body)?;
    let _lock_guard = RESOURCE_LOCK_SERIAL.lock();
    let resource_id = ResourceId::from_parts(&req.resource_type, &req.name);
    let project = resolve_operation_project(None, registry)?;
    let read_models = rebuild_operation_read_models(&project.to_string_lossy())
        .map_err(|err| (500, json!({ "error": err })))?;
    if let Some(existing) = read_models
        .locks
        .iter()
        .find(|lock| lock.resource_id == resource_id)
    {
        append_operation_event(
            &project.to_string_lossy(),
            SystemEvent::ResourceLockConflict {
                resource_id: resource_id.clone(),
                requested_owner_id: req.owner_id,
                existing_owner_id: existing.owner.clone(),
            },
        )
        .map_err(|err| {
            (
                500,
                json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true}),
            )
        })?;
        return Err((409, json!({ "error": "resource already locked" })));
    }
    append_operation_event(
        &project.to_string_lossy(),
        SystemEvent::ResourceLockAcquired {
            resource_id: resource_id.clone(),
            resource_type: req.resource_type,
            owner_id: req.owner_id,
            lease_expires_at_ms: req
                .lease_ms
                .map(|lease_ms| lease_expires_at(Some(lease_ms))),
        },
    )
    .map_err(|err| {
        (
            500,
            json!({"error":err,"code":"EVENT_WRITE_FAILED","recoverable":true}),
        )
    })?;
    Ok((201, json!({ "resource_id": resource_id.0, "locked": true })))
}

fn release(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let req: ReleaseLockBody = parse_json(body)?;
    let resource_id = ResourceId::from_parts(&req.resource_type, &req.name);
    crate::event_log::append_bridge_event(SystemEvent::ResourceLockReleased {
        resource_id: resource_id.clone(),
        owner_id: req.owner_id,
    });
    Ok((
        200,
        json!({ "resource_id": resource_id.0, "released": true }),
    ))
}

fn expire(id: &str) -> RouteResult {
    let resource_id = ResourceId(id.to_string());
    crate::event_log::append_bridge_event(SystemEvent::ResourceLockExpired {
        resource_id: resource_id.clone(),
    });
    Ok((
        200,
        json!({ "resource_id": resource_id.0, "expired": true }),
    ))
}
