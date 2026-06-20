use serde::Serialize;
use std::collections::BTreeMap;

use crate::events::{EventEntry, PaneId, SystemEvent};

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
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LockProjection {
    pub resource_id: String,
    pub owner: String,
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

pub fn build_read_models(entries: &[EventEntry]) -> ReadModels {
    let mut panes: BTreeMap<PaneId, PaneAccumulator> = BTreeMap::new();
    let mut audit = Vec::with_capacity(entries.len());

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
        }
    }

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

    ReadModels {
        workspace: WorkspaceStateProjection {
            panes,
            task_count: 0,
            lock_count: 0,
        },
        tasks: Vec::new(),
        locks: Vec::new(),
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
}
