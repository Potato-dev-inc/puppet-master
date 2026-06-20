#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::actors::ActorId;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PaneId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceId(pub String);

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl CommandId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl TaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl ResourceId {
    pub fn from_parts(resource_type: &str, name: &str) -> Self {
        Self(format!("{resource_type}:{name}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    pub id: EventId,
    pub timestamp_ms: i64,
    pub actor: ActorId,
    pub correlation_id: CommandId,
    pub payload: SystemEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum SystemEvent {
    PaneSpawned {
        pane_id: PaneId,
        agent_type: String,
        pid: u32,
        cwd: String,
        cols: u16,
        rows: u16,
    },
    PaneKilled {
        pane_id: PaneId,
    },
    PaneInputWritten {
        pane_id: PaneId,
        byte_count: usize,
        append_newline: bool,
    },
    PaneOutputObserved {
        pane_id: PaneId,
        byte_count: usize,
    },
    PaneStatusChanged {
        pane_id: PaneId,
        status: String,
    },
    McpToolCalled {
        tool: String,
    },
    McpToolCompleted {
        tool: String,
        ok: bool,
        status: u16,
    },
    TaskCreated {
        task_id: TaskId,
        title: String,
        exclusive: bool,
    },
    TaskClaimed {
        task_id: TaskId,
        agent_id: String,
        lease_expires_at_ms: i64,
    },
    TaskLeaseRenewed {
        task_id: TaskId,
        agent_id: String,
        lease_expires_at_ms: i64,
    },
    TaskStatusUpdated {
        task_id: TaskId,
        status: String,
    },
    TaskCompleted {
        task_id: TaskId,
        agent_id: String,
        evidence: String,
    },
    TaskBlocked {
        task_id: TaskId,
        agent_id: String,
        reason: String,
    },
    ReviewerAssigned {
        task_id: TaskId,
        reviewer_id: String,
    },
    ResourceLockAcquired {
        resource_id: ResourceId,
        resource_type: String,
        owner_id: String,
        lease_expires_at_ms: Option<i64>,
    },
    ResourceLockReleased {
        resource_id: ResourceId,
        owner_id: String,
    },
    ResourceLockExpired {
        resource_id: ResourceId,
    },
}

impl EventEntry {
    pub fn new(actor: ActorId, correlation_id: CommandId, payload: SystemEvent) -> Self {
        Self {
            id: EventId::new(),
            timestamp_ms: crate::event_log::now_ms(),
            actor,
            correlation_id,
            payload,
        }
    }
}
