use serde::Serialize;
use std::collections::BTreeMap;

use crate::events::{EventEntry, PaneId, ResourceId, SystemEvent, TaskId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceStateProjection {
    pub panes: Vec<PaneStateProjection>,
    pub task_count: usize,
    pub lock_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaneStateProjection {
    pub pane_id: PaneId,
    pub agent_type: Option<String>,
    pub pid: Option<u32>,
    pub cwd: Option<String>,
    pub status: String,
    pub input_events: usize,
    pub output_events: usize,
    pub killed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskProjection {
    pub id: TaskId,
    pub title: String,
    pub status: String,
    pub exclusive: bool,
    pub claimed_by: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub reviewer_id: Option<String>,
    pub evidence: Option<String>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LockProjection {
    pub resource_id: ResourceId,
    pub resource_type: String,
    pub owner: String,
    pub lease_expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentInboxProjection {
    pub agent_id: String,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditEntryProjection {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor: String,
    pub event_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadModels {
    pub workspace: WorkspaceStateProjection,
    pub tasks: Vec<TaskProjection>,
    pub locks: Vec<LockProjection>,
    pub audit: Vec<AuditEntryProjection>,
}

#[derive(Debug, Default)]
struct PaneAccumulator {
    agent_type: Option<String>,
    pid: Option<u32>,
    cwd: Option<String>,
    status: String,
    input_events: usize,
    output_events: usize,
    killed: bool,
}

#[derive(Debug, Default)]
struct TaskAccumulator {
    title: String,
    status: String,
    exclusive: bool,
    claimed_by: Option<String>,
    lease_expires_at_ms: Option<i64>,
    reviewer_id: Option<String>,
    evidence: Option<String>,
    blocked_reason: Option<String>,
}

#[derive(Debug)]
struct LockAccumulator {
    resource_type: String,
    owner: String,
    lease_expires_at_ms: Option<i64>,
}

pub fn build_read_models(entries: &[EventEntry]) -> ReadModels {
    let mut panes: BTreeMap<PaneId, PaneAccumulator> = BTreeMap::new();
    let mut tasks: BTreeMap<TaskId, TaskAccumulator> = BTreeMap::new();
    let mut locks: BTreeMap<ResourceId, LockAccumulator> = BTreeMap::new();
    let mut audit = Vec::with_capacity(entries.len());
    let mut killed_panes = Vec::new();

    for entry in entries {
        audit.push(AuditEntryProjection {
            event_id: entry.id.0.clone(),
            timestamp_ms: entry.timestamp_ms,
            actor: entry.actor.0.clone(),
            event_type: event_type_name(&entry.payload).to_string(),
        });

        match &entry.payload {
            SystemEvent::PaneSpawned {
                pane_id,
                agent_type,
                pid,
                cwd,
                ..
            } => {
                let pane = panes.entry(pane_id.clone()).or_default();
                pane.agent_type = Some(agent_type.clone());
                pane.pid = Some(*pid);
                pane.cwd = Some(cwd.clone());
                pane.status = "running".to_string();
                pane.killed = false;
            }
            SystemEvent::PaneKilled { pane_id } => {
                let pane = panes.entry(pane_id.clone()).or_default();
                pane.status = "killed".to_string();
                pane.killed = true;
                killed_panes.push(pane_id.0.clone());
            }
            SystemEvent::PaneInputWritten { pane_id, .. } => {
                panes.entry(pane_id.clone()).or_default().input_events += 1;
            }
            SystemEvent::PaneOutputObserved { pane_id, .. } => {
                panes.entry(pane_id.clone()).or_default().output_events += 1;
            }
            SystemEvent::PaneStatusChanged { pane_id, status } => {
                panes.entry(pane_id.clone()).or_default().status = status.clone();
            }
            SystemEvent::McpToolCalled { .. } | SystemEvent::McpToolCompleted { .. } => {}
            SystemEvent::TaskCreated {
                task_id,
                title,
                exclusive,
            } => {
                let task = tasks.entry(task_id.clone()).or_default();
                task.title = title.clone();
                task.exclusive = *exclusive;
                task.status = "open".to_string();
            }
            SystemEvent::TaskClaimed {
                task_id,
                agent_id,
                lease_expires_at_ms,
            }
            | SystemEvent::TaskLeaseRenewed {
                task_id,
                agent_id,
                lease_expires_at_ms,
            } => {
                let task = tasks.entry(task_id.clone()).or_default();
                task.claimed_by = Some(agent_id.clone());
                task.lease_expires_at_ms = Some(*lease_expires_at_ms);
                if task.status != "completed" && task.status != "blocked" {
                    task.status = "claimed".to_string();
                }
            }
            SystemEvent::TaskStatusUpdated { task_id, status } => {
                tasks.entry(task_id.clone()).or_default().status = status.clone();
            }
            SystemEvent::TaskCompleted {
                task_id,
                agent_id,
                evidence,
            } => {
                let task = tasks.entry(task_id.clone()).or_default();
                task.claimed_by = Some(agent_id.clone());
                task.status = "completed".to_string();
                task.evidence = Some(evidence.clone());
            }
            SystemEvent::TaskBlocked {
                task_id,
                agent_id,
                reason,
            } => {
                let task = tasks.entry(task_id.clone()).or_default();
                task.claimed_by = Some(agent_id.clone());
                task.status = "blocked".to_string();
                task.blocked_reason = Some(reason.clone());
            }
            SystemEvent::ReviewerAssigned {
                task_id,
                reviewer_id,
            } => {
                tasks.entry(task_id.clone()).or_default().reviewer_id = Some(reviewer_id.clone());
            }
            SystemEvent::ResourceLockAcquired {
                resource_id,
                resource_type,
                owner_id,
                lease_expires_at_ms,
            } => {
                locks.insert(
                    resource_id.clone(),
                    LockAccumulator {
                        resource_type: resource_type.clone(),
                        owner: owner_id.clone(),
                        lease_expires_at_ms: *lease_expires_at_ms,
                    },
                );
            }
            SystemEvent::ResourceLockReleased { resource_id, .. }
            | SystemEvent::ResourceLockExpired { resource_id } => {
                locks.remove(resource_id);
            }
        }
    }

    for pane_id in killed_panes {
        locks.retain(|_, lock| lock.owner != pane_id);
    }

    let now_ms = crate::event_log::now_ms();
    for task in tasks.values_mut() {
        if task.status == "claimed"
            && task
                .lease_expires_at_ms
                .is_some_and(|expires_at| expires_at <= now_ms)
        {
            task.status = "stale".to_string();
            task.claimed_by = None;
        }
    }
    locks.retain(|_, lock| {
        lock.lease_expires_at_ms
            .map(|expires_at| expires_at > now_ms)
            .unwrap_or(true)
    });

    let panes = panes
        .into_iter()
        .map(|(pane_id, pane)| PaneStateProjection {
            pane_id,
            agent_type: pane.agent_type,
            pid: pane.pid,
            cwd: pane.cwd,
            status: if pane.status.is_empty() {
                "unknown".to_string()
            } else {
                pane.status
            },
            input_events: pane.input_events,
            output_events: pane.output_events,
            killed: pane.killed,
        })
        .collect::<Vec<_>>();

    let tasks = tasks
        .into_iter()
        .map(|(id, task)| TaskProjection {
            id,
            title: task.title,
            status: if task.status.is_empty() {
                "open".to_string()
            } else {
                task.status
            },
            exclusive: task.exclusive,
            claimed_by: task.claimed_by,
            lease_expires_at_ms: task.lease_expires_at_ms,
            reviewer_id: task.reviewer_id,
            evidence: task.evidence,
            blocked_reason: task.blocked_reason,
        })
        .collect::<Vec<_>>();

    let locks = locks
        .into_iter()
        .map(|(resource_id, lock)| LockProjection {
            resource_id,
            resource_type: lock.resource_type,
            owner: lock.owner,
            lease_expires_at_ms: lock.lease_expires_at_ms,
        })
        .collect::<Vec<_>>();

    ReadModels {
        workspace: WorkspaceStateProjection {
            panes,
            task_count: tasks.len(),
            lock_count: locks.len(),
        },
        tasks,
        locks,
        audit,
    }
}

pub fn agent_inbox(agent_id: String) -> AgentInboxProjection {
    AgentInboxProjection {
        agent_id,
        messages: Vec::new(),
    }
}

fn event_type_name(event: &SystemEvent) -> &'static str {
    match event {
        SystemEvent::PaneSpawned { .. } => "PaneSpawned",
        SystemEvent::PaneKilled { .. } => "PaneKilled",
        SystemEvent::PaneInputWritten { .. } => "PaneInputWritten",
        SystemEvent::PaneOutputObserved { .. } => "PaneOutputObserved",
        SystemEvent::PaneStatusChanged { .. } => "PaneStatusChanged",
        SystemEvent::McpToolCalled { .. } => "McpToolCalled",
        SystemEvent::McpToolCompleted { .. } => "McpToolCompleted",
        SystemEvent::TaskCreated { .. } => "TaskCreated",
        SystemEvent::TaskClaimed { .. } => "TaskClaimed",
        SystemEvent::TaskLeaseRenewed { .. } => "TaskLeaseRenewed",
        SystemEvent::TaskStatusUpdated { .. } => "TaskStatusUpdated",
        SystemEvent::TaskCompleted { .. } => "TaskCompleted",
        SystemEvent::TaskBlocked { .. } => "TaskBlocked",
        SystemEvent::ReviewerAssigned { .. } => "ReviewerAssigned",
        SystemEvent::ResourceLockAcquired { .. } => "ResourceLockAcquired",
        SystemEvent::ResourceLockReleased { .. } => "ResourceLockReleased",
        SystemEvent::ResourceLockExpired { .. } => "ResourceLockExpired",
    }
}

#[cfg(test)]
mod tests {
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
                },
            ),
        ];

        let models = build_read_models(&entries);
        assert!(models.locks.is_empty());
    }
}
