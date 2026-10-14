//! Resolving a run handle to its snapshot with the session ownership gate, plus transcript reads.

use crate::agent_runs::transcript;
use crate::operations::{self, OperationError, OperationSnapshot};

pub fn transcript_page(
    project: &str,
    operation_id: &str,
    after: u64,
) -> Result<transcript::TranscriptPage, OperationError> {
    transcript_page_with(project, operation_id, after, false)
}

/// `details` also returns the raw delivery diagnostics (`delivery_trace`).
pub fn transcript_page_with(
    project: &str,
    operation_id: &str,
    after: u64,
    details: bool,
) -> Result<transcript::TranscriptPage, OperationError> {
    transcript::read_with(project, operation_id, after, details)
        .map_err(|error| OperationError::new("TRANSCRIPT_READ_FAILED", error.to_string(), true))
}

pub fn transcript_for_handle(
    project: &str,
    handle: &str,
    after: u64,
) -> Result<transcript::TranscriptPage, OperationError> {
    transcript_for_handle_with(project, handle, after, false)
}

pub fn transcript_for_handle_with(
    project: &str,
    handle: &str,
    after: u64,
    details: bool,
) -> Result<transcript::TranscriptPage, OperationError> {
    let ids = operations::turns_for_handle(project, handle)?
        .into_iter()
        .map(|snapshot| snapshot.operation_id)
        .collect::<Vec<_>>();
    transcript::read_turns_with(project, &ids, after, details)
        .map_err(|error| OperationError::new("TRANSCRIPT_READ_FAILED", error.to_string(), true))
}

pub fn record_user_task(project: &str, operation_id: &str, task: &str) {
    transcript::append(project, operation_id, "user_task", task);
}

pub fn current_agent(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    current_agent_with_access(project, handle, session_id, RunAccess::Mutate)
}

#[derive(Clone, Copy)]
enum RunAccess {
    Read,
    Wait,
    Mutate,
}

fn current_agent_with_access(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
    access: RunAccess,
) -> Result<OperationSnapshot, OperationError> {
    let snapshot = operations::resolve_agent_run(project, handle)?;
    check_snapshot_access(&snapshot, handle, session_id, access)?;
    Ok(snapshot)
}

/// Ownership gate for a run that was already resolved (so a caller that located the run itself
/// does not re-resolve it through another project).
pub(in crate::agent_runs) fn check_snapshot_mutate_access(
    snapshot: &OperationSnapshot,
    handle: &str,
    session_id: Option<&str>,
) -> Result<(), OperationError> {
    check_snapshot_access(snapshot, handle, session_id, RunAccess::Mutate)
}

fn check_snapshot_access(
    snapshot: &OperationSnapshot,
    handle: &str,
    session_id: Option<&str>,
    access: RunAccess,
) -> Result<(), OperationError> {
    if let Some(session) = session_id {
        if crate::mcp_sessions::agent_is_registered(handle) {
            let check = match access {
                RunAccess::Mutate => crate::mcp_sessions::check_run_access(session, handle),
                RunAccess::Wait => crate::mcp_sessions::check_run_wait_access(session, handle),
                RunAccess::Read => crate::mcp_sessions::check_run_read_access(session, handle),
            };
            check?;
        } else if snapshot
            .owner_session_id
            .as_deref()
            .is_some_and(|owner| owner != session)
        {
            return Err(OperationError::new(
                "AUTHORIZATION_DENIED",
                "agent handle belongs to a different MCP session",
                false,
            ));
        }
    }
    Ok(())
}

pub fn current_agent_for_read(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    current_agent_with_access(project, handle, session_id, RunAccess::Read)
}

pub fn current_agent_for_wait(
    project: &str,
    handle: &str,
    session_id: Option<&str>,
) -> Result<OperationSnapshot, OperationError> {
    current_agent_with_access(project, handle, session_id, RunAccess::Wait)
}
