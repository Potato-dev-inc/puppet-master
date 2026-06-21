use serde::{Deserialize, Serialize};

use crate::projections::{LockProjection, ReadModels, TaskProjection};

#[derive(Debug, Clone, Deserialize)]
pub struct ContextPackRequest {
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub user_constraints: Option<Vec<String>>,
    pub manager_instructions: Option<String>,
    pub raw_scrollback: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPack {
    pub prompt: String,
    pub expected_report_format: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub ownership_boundaries: Vec<String>,
    pub evidence_requirements: Vec<String>,
    pub estimated_raw_scrollback_bytes: usize,
    pub context_pack_bytes: usize,
}

pub fn build_context_pack(request: ContextPackRequest, read_models: &ReadModels) -> ContextPack {
    let task = request
        .task_id
        .as_deref()
        .and_then(|id| read_models.tasks.iter().find(|task| task.id.0 == id));
    let locks = locks_for_agent(request.agent_id.as_deref(), &read_models.locks);
    let constraints = request.user_constraints.unwrap_or_default();
    let manager_instructions = request.manager_instructions.unwrap_or_default();

    let mut prompt_parts = Vec::new();
    if let Some(task) = task {
        prompt_parts.push(format_task(task));
    } else {
        prompt_parts.push("Task: unscoped coordination request".to_string());
    }
    if !manager_instructions.trim().is_empty() {
        prompt_parts.push(format!(
            "Manager instructions: {}",
            manager_instructions.trim()
        ));
    }
    if !constraints.is_empty() {
        prompt_parts.push(format!("User constraints: {}", constraints.join("; ")));
    }
    if !locks.is_empty() {
        prompt_parts.push(format!(
            "Current locks: {}",
            locks
                .iter()
                .map(|lock| format!("{} owned by {}", lock.resource_id.0, lock.owner))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let allowed_tools = match request.agent_id.as_deref() {
        Some(agent_id) if agent_id.contains("tester") => vec![
            "read_terminal_buffer".to_string(),
            "report_task_status".to_string(),
            "complete_task".to_string(),
        ],
        _ => vec![
            "read_terminal_buffer".to_string(),
            "write_terminal_input".to_string(),
            "report_task_status".to_string(),
            "complete_task".to_string(),
            "acquire_resource_lock".to_string(),
            "release_resource_lock".to_string(),
        ],
    };

    let mut ownership_boundaries = vec![
        "Use task and lock tools before taking exclusive ownership.".to_string(),
        "Do not overwrite resources locked by another owner.".to_string(),
    ];
    if let Some(agent_id) = request.agent_id {
        ownership_boundaries.push(format!("Report progress as {agent_id}."));
    }

    let evidence_requirements = vec![
        "List files changed or inspected.".to_string(),
        "Include exact test command and result.".to_string(),
        "Report blockers with the smallest reproducible detail.".to_string(),
    ];

    let prompt = prompt_parts.join("\n");
    let estimated_raw_scrollback_bytes = request.raw_scrollback.as_deref().unwrap_or("").len();
    let context_pack_bytes = prompt.len();

    ContextPack {
        prompt,
        expected_report_format: vec![
            "status".to_string(),
            "summary".to_string(),
            "evidence".to_string(),
            "next_step_or_blocker".to_string(),
        ],
        allowed_tools,
        ownership_boundaries,
        evidence_requirements,
        estimated_raw_scrollback_bytes,
        context_pack_bytes,
    }
}

fn format_task(task: &TaskProjection) -> String {
    format!(
        "Task {}: {} [status={}, claimed_by={}]",
        task.id.0,
        task.title,
        task.status,
        task.claimed_by.as_deref().unwrap_or("unclaimed")
    )
}

fn locks_for_agent<'a>(
    agent_id: Option<&str>,
    locks: &'a [LockProjection],
) -> Vec<&'a LockProjection> {
    match agent_id {
        Some(agent_id) => locks
            .iter()
            .filter(|lock| lock.owner == agent_id)
            .collect::<Vec<_>>(),
        None => locks.iter().collect::<Vec<_>>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::TaskId;
    use crate::projections::{ReadModels, WorkspaceStateProjection};

    #[test]
    fn context_pack_is_smaller_than_raw_scrollback() {
        let models = ReadModels {
            workspace: WorkspaceStateProjection {
                panes: Vec::new(),
                task_count: 1,
                lock_count: 0,
            },
            tasks: vec![TaskProjection {
                id: TaskId("task-1".to_string()),
                title: "Run targeted tests".to_string(),
                status: "claimed".to_string(),
                exclusive: true,
                claimed_by: Some("tester-1".to_string()),
                lease_expires_at_ms: None,
                reviewer_id: None,
                evidence: None,
                blocked_reason: None,
            }],
            locks: Vec::new(),
            audit: Vec::new(),
        };
        let raw = "irrelevant terminal history\n".repeat(100);
        let pack = build_context_pack(
            ContextPackRequest {
                task_id: Some("task-1".to_string()),
                agent_id: Some("tester-1".to_string()),
                user_constraints: Some(vec!["keep changes scoped".to_string()]),
                manager_instructions: Some("Verify the implementation.".to_string()),
                raw_scrollback: Some(raw),
            },
            &models,
        );
        assert!(pack.context_pack_bytes < pack.estimated_raw_scrollback_bytes);
        assert!(pack.prompt.contains("task-1"));
        assert!(pack
            .evidence_requirements
            .iter()
            .any(|item| item.contains("test command")));
    }
}
