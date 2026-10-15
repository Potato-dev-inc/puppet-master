//! Unit tests for the read-model projections.

use super::*;
use crate::actors::ActorId;
use crate::events::{CommandId, EventEntry};

#[test]
fn rebuilds_pane_state_from_events() {
    let entries = vec![
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneSpawned {
                pane_id: PaneId("pane-1".to_string()),
                agent_type: "codex".to_string(),
                pid: 10,
                cwd: "/repo".to_string(),
                cols: 120,
                rows: 30,
            },
        ),
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneInputWritten {
                pane_id: PaneId("pane-1".to_string()),
                byte_count: 12,
                append_newline: true,
            },
        ),
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneStatusChanged {
                pane_id: PaneId("pane-1".to_string()),
                status: "idle".to_string(),
            },
        ),
    ];

    let first = build_read_models(&entries);
    let rebuilt = build_read_models(&entries);
    assert_eq!(first, rebuilt);
    assert_eq!(rebuilt.workspace.panes.len(), 1);
    assert_eq!(rebuilt.workspace.panes[0].status, "idle");
    assert_eq!(rebuilt.workspace.panes[0].input_events, 1);
    assert_eq!(rebuilt.audit.len(), 3);
}

#[test]
fn stale_task_lease_rebuilds_as_stale() {
    let entries = vec![
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::TaskCreated {
                task_id: TaskId("task-1".to_string()),
                title: "Implement thing".to_string(),
                exclusive: true,
            },
        ),
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::TaskClaimed {
                task_id: TaskId("task-1".to_string()),
                agent_id: "agent-a".to_string(),
                lease_expires_at_ms: 1,
            },
        ),
    ];

    let models = build_read_models(&entries);
    assert_eq!(models.tasks.len(), 1);
    assert_eq!(models.tasks[0].status, "stale");
    assert_eq!(models.tasks[0].claimed_by, None);
}

#[test]
fn pane_exit_removes_owned_locks() {
    let entries = vec![
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::ResourceLockAcquired {
                resource_id: ResourceId("file:src/lib.rs".to_string()),
                resource_type: "file".to_string(),
                owner_id: "pane-1".to_string(),
                lease_expires_at_ms: None,
            },
        ),
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneKilled {
                pane_id: PaneId("pane-1".to_string()),
                reason: None,
            },
        ),
    ];

    let models = build_read_models(&entries);
    assert!(models.locks.is_empty());
}

#[test]
fn rebuilds_session_goal_from_events() {
    let entries = vec![EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::SessionGoalUpdated {
            current_goal: Some("Ship Rust MCP".to_string()),
        },
    )];

    let models = build_read_models(&entries);
    assert_eq!(
        models.session.current_goal,
        Some("Ship Rust MCP".to_string())
    );
}

#[test]
fn rebuilds_pane_roles_into_workspace_and_session() {
    let pane_id = PaneId("pane-1".to_string());
    let entries = vec![
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneSpawned {
                pane_id: pane_id.clone(),
                agent_type: "codex".to_string(),
                pid: 10,
                cwd: "/repo".to_string(),
                cols: 120,
                rows: 30,
            },
        ),
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneRoleSet {
                pane_id,
                role: PaneRole::Implementer,
            },
        ),
    ];

    let models = build_read_models(&entries);
    assert_eq!(
        models.session.pane_roles.get("pane-1"),
        Some(&PaneRole::Implementer)
    );
    assert_eq!(models.workspace.panes[0].role, Some(PaneRole::Implementer));
}

#[test]
fn rebuilds_latest_pane_digest() {
    let entries = vec![EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::PaneDigestUpdated {
            pane_id: PaneId("pane-1".to_string()),
            summary: "Ready for review".to_string(),
            source: "manual".to_string(),
        },
    )];

    let models = build_read_models(&entries);
    let digest = models.session.pane_digests.get("pane-1").unwrap();
    assert_eq!(digest.summary, "Ready for review");
    assert_eq!(digest.source, "manual");
}

#[test]
fn rebuilds_lock_conflict_state() {
    let entries = vec![EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::ResourceLockConflict {
            resource_id: ResourceId("file:README.md".to_string()),
            requested_owner_id: "agent-b".to_string(),
            existing_owner_id: "agent-a".to_string(),
        },
    )];

    let models = build_read_models(&entries);
    assert_eq!(models.session.lock_conflicts.len(), 1);
    assert_eq!(
        models.session.lock_conflicts[0].resource_id,
        "file:README.md"
    );
}

#[test]
fn rebuilds_orchestrator_standby_policy() {
    let entries = vec![EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::OrchestratorStandbyPolicyUpdated {
            standby_poll_ms: 2_000,
            standby_max_ms: 120_000,
        },
    )];

    let models = build_read_models(&entries);
    assert_eq!(models.session.orchestrator.standby_poll_ms, 2_000);
    assert_eq!(models.session.orchestrator.standby_max_ms, 120_000);
}
