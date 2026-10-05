//! One-line, human-readable summaries of system events for the audit projection.

use crate::events::SystemEvent;

pub(super) fn event_summary(event: &SystemEvent) -> String {
    match event {
        SystemEvent::PaneSpawned {
            pane_id,
            agent_type,
            ..
        } => format!("pane {} spawned as {agent_type}", pane_id.0),
        SystemEvent::PaneKilled { pane_id, reason } => {
            if let Some(reason) = reason {
                format!("pane {} killed ({})", pane_id.0, reason)
            } else {
                format!("pane {} killed", pane_id.0)
            }
        }
        SystemEvent::PaneInputWritten { pane_id, .. } => {
            format!("input written to pane {}", pane_id.0)
        }
        SystemEvent::PaneOutputObserved { pane_id, .. } => {
            format!("output observed from pane {}", pane_id.0)
        }
        SystemEvent::PaneStatusChanged { pane_id, status } => {
            format!("pane {} status changed to {status}", pane_id.0)
        }
        SystemEvent::McpToolCalled { tool } => format!("MCP tool called: {tool}"),
        SystemEvent::McpToolCompleted { tool, ok, status } => {
            format!("MCP tool completed: {tool} ok={ok} status={status}")
        }
        SystemEvent::TaskCreated { task_id, title, .. } => {
            format!("task {} created: {title}", task_id.0)
        }
        SystemEvent::TaskClaimed {
            task_id, agent_id, ..
        } => {
            format!("task {} claimed by {agent_id}", task_id.0)
        }
        SystemEvent::TaskLeaseRenewed {
            task_id, agent_id, ..
        } => {
            format!("task {} lease renewed by {agent_id}", task_id.0)
        }
        SystemEvent::TaskStatusUpdated { task_id, status } => {
            format!("task {} status updated to {status}", task_id.0)
        }
        SystemEvent::TaskCompleted {
            task_id, agent_id, ..
        } => {
            format!("task {} completed by {agent_id}", task_id.0)
        }
        SystemEvent::TaskBlocked {
            task_id, agent_id, ..
        } => {
            format!("task {} blocked by {agent_id}", task_id.0)
        }
        SystemEvent::ReviewerAssigned {
            task_id,
            reviewer_id,
        } => {
            format!("task {} reviewer assigned to {reviewer_id}", task_id.0)
        }
        SystemEvent::ResourceLockAcquired {
            resource_id,
            owner_id,
            ..
        } => {
            format!("resource {} locked by {owner_id}", resource_id.0)
        }
        SystemEvent::ResourceLockConflict {
            resource_id,
            requested_owner_id,
            existing_owner_id,
        } => format!(
            "resource {} lock conflict: {requested_owner_id} blocked by {existing_owner_id}",
            resource_id.0
        ),
        SystemEvent::ResourceLockReleased {
            resource_id,
            owner_id,
        } => {
            format!("resource {} released by {owner_id}", resource_id.0)
        }
        SystemEvent::ResourceLockExpired { resource_id } => {
            format!("resource {} expired", resource_id.0)
        }
        SystemEvent::AgentObservation { pane_id, .. } => {
            format!("agent observation for pane {}", pane_id.0)
        }
        SystemEvent::SessionGoalUpdated { current_goal } => current_goal
            .as_ref()
            .map(|goal| format!("current goal updated: {goal}"))
            .unwrap_or_else(|| "current goal cleared".to_string()),
        SystemEvent::PaneRoleSet { pane_id, role } => {
            format!("pane {} role set to {}", pane_id.0, role.as_str())
        }
        SystemEvent::PaneDigestUpdated { pane_id, .. } => {
            format!("pane {} digest updated", pane_id.0)
        }
        SystemEvent::DelegationPrepared { intent, .. } => {
            format!("delegation prepared: {intent}")
        }
        SystemEvent::OrchestratorStandbyPolicyUpdated {
            standby_poll_ms,
            standby_max_ms,
        } => format!(
            "orchestrator standby policy updated: poll={standby_poll_ms}ms max={standby_max_ms}ms"
        ),
        SystemEvent::OpenCodeKeySwap {
            pane_id,
            event,
            from_profile,
            to_profile,
        } => {
            let to = to_profile
                .as_deref()
                .map(|profile| format!(" -> {profile}"))
                .unwrap_or_default();
            format!(
                "opencode key {event} on pane {} ({from_profile}{to})",
                pane_id.0
            )
        }
        SystemEvent::PaneModelSwitched {
            pane_id,
            provider_id,
            model_id,
        } => format!(
            "pane {} model switched to {provider_id}/{model_id}",
            pane_id.0
        ),
        SystemEvent::PaneTuiReattached {
            pane_id,
            attach_generation,
        } => format!(
            "pane {} tui reattached (generation {attach_generation})",
            pane_id.0
        ),
    }
}
