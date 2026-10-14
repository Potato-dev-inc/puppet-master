#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::actors::ActorId;
use crate::session_context::PaneRole;

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
    /// The lock identity for `type:name`. A `file` lock names a path, so every spelling of the
    /// same relative path (`./a.rs`, `src\a.rs`, `src//a.rs`, `src/x/../a.rs`) maps to one id;
    /// without that, two callers holding the "same" file under different spellings both succeed.
    pub fn from_parts(resource_type: &str, name: &str) -> Self {
        let resource_type = resource_type.trim();
        let name = name.trim();
        if resource_type.eq_ignore_ascii_case("file") {
            return Self(format!("{resource_type}:{}", normalize_lock_path(name)));
        }
        Self(format!("{resource_type}:{name}"))
    }
}

/// Lexical path normalisation for `file` locks: `\` becomes `/`, `.` and empty segments drop,
/// `x/..` collapses. A leading `..` is kept (it names something outside the project).
fn normalize_lock_path(name: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in name.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(last) if *last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return name.to_string();
    }
    let joined = parts.join("/");
    if name.starts_with('/') || name.starts_with('\\') {
        format!("/{joined}")
    } else {
        joined
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
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
    ResourceLockConflict {
        resource_id: ResourceId,
        requested_owner_id: String,
        existing_owner_id: String,
    },
    ResourceLockReleased {
        resource_id: ResourceId,
        owner_id: String,
    },
    ResourceLockExpired {
        resource_id: ResourceId,
    },
    AgentObservation {
        pane_id: PaneId,
        agent_type: String,
        observation: String,
        text: Option<String>,
    },
    SessionGoalUpdated {
        current_goal: Option<String>,
    },
    PaneRoleSet {
        pane_id: PaneId,
        role: PaneRole,
    },
    PaneDigestUpdated {
        pane_id: PaneId,
        summary: String,
        source: String,
    },
    DelegationPrepared {
        task_id: Option<TaskId>,
        target_pane_id: Option<PaneId>,
        intent: String,
    },
    OrchestratorStandbyPolicyUpdated {
        standby_poll_ms: u64,
        standby_max_ms: u64,
    },
    OpenCodeKeySwap {
        pane_id: PaneId,
        event: String,
        from_profile: String,
        to_profile: Option<String>,
    },
    PaneModelSwitched {
        pane_id: PaneId,
        provider_id: String,
        model_id: String,
    },
    PaneTuiReattached {
        pane_id: PaneId,
        attach_generation: u64,
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
