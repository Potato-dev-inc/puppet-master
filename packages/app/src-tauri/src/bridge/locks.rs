//! Operation resource locks: acquire/release against the project event log, startup recovery
//! of interrupted operations, and the fail-fast conflict check used before dispatch.

use serde_json::json;

use super::state::RESOURCE_LOCK_SERIAL;
use super::store::{append_operation_event, rebuild_operation_read_models};
use crate::events::{ResourceId, SystemEvent};

pub(super) fn parse_operation_lock(
    value: &str,
) -> Result<(String, String), crate::operations::OperationError> {
    let Some((resource_type, name)) = value.split_once(':') else {
        return Err(crate::operations::OperationError::new(
            "INVALID_LOCK",
            "requested locks must use type:name format",
            false,
        ));
    };
    if resource_type.trim().is_empty() || name.trim().is_empty() {
        return Err(crate::operations::OperationError::new(
            "INVALID_LOCK",
            "requested lock type and name must be non-empty",
            false,
        ));
    }
    Ok((resource_type.trim().to_string(), name.trim().to_string()))
}

/// Acquire every lock requested by `operation`, owned by the operation id. Re-acquiring a lock
/// this operation already holds is a no-op; a lock held by anyone else fails with
/// RESOURCE_LOCKED (context: resource_id, owner_id). Callers release via `release_operation_locks`,
/// which only frees locks this operation owns, so a partial acquisition is safe to roll back.
pub(crate) fn acquire_operation_locks(
    operation: &crate::operations::OperationSnapshot,
) -> Result<(), crate::operations::OperationError> {
    if operation.locks.is_empty() {
        return Ok(());
    }
    let _lock_guard = RESOURCE_LOCK_SERIAL.lock();
    let mut requested = Vec::new();
    for lock_name in &operation.locks {
        requested.push(parse_operation_lock(lock_name)?);
    }
    for (resource_type, resource_name) in requested {
        let resource_id = ResourceId::from_parts(&resource_type, &resource_name);
        let models = rebuild_operation_read_models(&operation.project_path).map_err(|err| {
            crate::operations::OperationError::new("STATE_UNAVAILABLE", err, true)
        })?;
        if let Some(existing) = models
            .locks
            .iter()
            .find(|lock| lock.resource_id == resource_id)
        {
            if existing.owner == operation.operation_id {
                continue;
            }
            let mut error = crate::operations::OperationError::new(
                "RESOURCE_LOCKED",
                "a requested resource is already locked",
                true,
            );
            error.retry_after_ms = Some(1000);
            error.context = json!({"resource_id":resource_id.0,"owner_id":existing.owner});
            return Err(error);
        }
        append_operation_event(
            &operation.project_path,
            SystemEvent::ResourceLockAcquired {
                resource_id,
                resource_type,
                owner_id: operation.operation_id.clone(),
                lease_expires_at_ms: None,
            },
        )
        .map_err(|err| crate::operations::OperationError::new("EVENT_WRITE_FAILED", err, true))?;
    }
    Ok(())
}

/// Fail operations a previous app process left active, and free the resource locks they held.
/// Locks are durable event-log entries, so without this an interrupted run would keep its files
/// locked across every later restart.
pub(crate) fn recover_interrupted_operations_on_startup(
) -> Result<Vec<crate::operations::OperationSnapshot>, crate::operations::OperationError> {
    let recovered = crate::operations::recover_all_interrupted_operations()?;
    for snapshot in &recovered {
        if let Err(err) = release_operation_locks(snapshot) {
            tracing::warn!(%err, operation_id = %snapshot.operation_id, "failed to release locks of interrupted operation");
        }
    }
    Ok(recovered)
}

/// Release the locks held by `operation`. Locks currently owned by a different operation are
/// left alone (the release event is not owner-checked by the projection).
pub(crate) fn release_operation_locks(
    operation: &crate::operations::OperationSnapshot,
) -> Result<(), String> {
    if operation.locks.is_empty() {
        return Ok(());
    }
    let _lock_guard = RESOURCE_LOCK_SERIAL.lock();
    let models = rebuild_operation_read_models(&operation.project_path)?;
    for lock in &operation.locks {
        if let Some((resource_type, name)) = lock.split_once(':') {
            let resource_id = ResourceId::from_parts(resource_type.trim(), name.trim());
            let owned = models.locks.iter().any(|held| {
                held.resource_id == resource_id && held.owner == operation.operation_id
            });
            if !owned {
                continue;
            }
            append_operation_event(
                &operation.project_path,
                SystemEvent::ResourceLockReleased {
                    resource_id,
                    owner_id: operation.operation_id.clone(),
                },
            )?;
        }
    }
    Ok(())
}

/// Fail fast: acquire the operation's locks before any worker starts. On conflict the queued
/// operation is marked failed (never left running) and the RESOURCE_LOCKED error is returned.
pub(crate) fn fail_operation_on_lock_conflict(
    snapshot: &crate::operations::OperationSnapshot,
) -> Result<(), crate::operations::OperationError> {
    let Err(mut error) = acquire_operation_locks(snapshot) else {
        return Ok(());
    };
    if let Some(context) = error.context.as_object_mut() {
        context.insert(
            "operation_id".into(),
            serde_json::Value::String(snapshot.operation_id.clone()),
        );
    }
    let _ = release_operation_locks(snapshot);
    let _ = crate::operations::mark_operation_state(
        &snapshot.project_path,
        &snapshot.operation_id,
        crate::operations::OperationStatus::Failed,
        crate::operations::StateSource::Native,
        Some("failed".into()),
        None,
        Some(error.clone()),
    );
    Err(error)
}
