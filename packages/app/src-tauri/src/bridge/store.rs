//! Event-log access for the bridge: read-model rebuilds and project-scoped event appends.

use crate::events::SystemEvent;

pub(super) fn rebuild_read_models() -> Result<crate::projections::ReadModels, String> {
    let entries = crate::event_log::read_global_entries()?;
    Ok(crate::projections::build_read_models(&entries))
}

pub(super) fn rebuild_operation_read_models(
    project: &str,
) -> Result<crate::projections::ReadModels, String> {
    let log = crate::event_log::EventLog::new(crate::event_log::project_event_log_path(
        std::path::Path::new(project),
    ))?;
    Ok(crate::projections::build_read_models(&log.read_all()?))
}

pub(super) fn append_operation_event(project: &str, payload: SystemEvent) -> Result<(), String> {
    let log = crate::event_log::EventLog::new(crate::event_log::project_event_log_path(
        std::path::Path::new(project),
    ))?;
    log.append(&crate::events::EventEntry::new(
        crate::actors::ActorId::bridge(),
        crate::events::CommandId::new(),
        payload,
    ))?;
    crate::pane_wait_notify::bump_waiters();
    Ok(())
}

pub(super) fn lease_expires_at(lease_ms: Option<i64>) -> i64 {
    crate::event_log::now_ms() + lease_ms.unwrap_or(5 * 60 * 1000).max(1)
}
