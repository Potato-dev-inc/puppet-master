use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::project_ir;
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
    pub project_ir_included: bool,
    pub project_ir_stale: bool,
    pub project_ir_indexer_command: String,
}

pub fn build_context_pack(
    request: ContextPackRequest,
    read_models: &ReadModels,
    project_root: Option<&Path>,
    librarian_indexer_path: Option<&str>,
) -> ContextPack {
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

    let ir_status = project_root
        .map(|root| project_ir::status(root, librarian_indexer_path))
        .unwrap_or_else(|| project_ir::status(Path::new("."), librarian_indexer_path));
    let mut prompt = prompt_parts.join("\n");
    let project_ir_included = if let Some(root) = project_root {
        if let Some(excerpt) = project_ir::read_overview_excerpt(root) {
            let sha_note = excerpt
                .git_sha
                .as_deref()
                .map(|sha| format!(" (indexed at git {sha})"))
                .unwrap_or_default();
            prompt.push_str(&format!(
                "\n\nProject overview (librarian index{sha_note}):\n{}",
                excerpt.overview.trim()
            ));
            true
        } else {
            false
        }
    } else {
        false
    };

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
        project_ir_included,
        project_ir_stale: ir_status.stale,
        project_ir_indexer_command: ir_status.indexer_command,
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
    use crate::session_context::SessionContextProjection;

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
            session: SessionContextProjection::default(),
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
            None,
            None,
        );
        assert!(pack.context_pack_bytes < pack.estimated_raw_scrollback_bytes);
        assert!(pack.prompt.contains("task-1"));
        assert!(pack
            .evidence_requirements
            .iter()
            .any(|item| item.contains("test command")));
    }

    #[test]
    fn context_pack_appends_project_ir_overview() {
        let dir = std::env::temp_dir().join(format!(
            "pm-context-pack-ir-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let storage = dir.join(".puppet-master");
        std::fs::create_dir_all(&storage).expect("mkdir");
        std::fs::write(
            storage.join("project-ir.json"),
            r#"{"overview":"packages/app is the desktop shell","generated_at_ms":1}"#,
        )
        .expect("write ir");
        std::fs::write(
            storage.join("project-ir.meta.json"),
            r#"{"git_sha":"deadbeef","generated_at_ms":1}"#,
        )
        .expect("write meta");

        let models = ReadModels {
            workspace: WorkspaceStateProjection {
                panes: Vec::new(),
                task_count: 0,
                lock_count: 0,
            },
            tasks: Vec::new(),
            locks: Vec::new(),
            audit: Vec::new(),
            session: SessionContextProjection::default(),
        };
        let pack = build_context_pack(
            ContextPackRequest {
                task_id: None,
                agent_id: None,
                user_constraints: None,
                manager_instructions: None,
                raw_scrollback: None,
            },
            &models,
            Some(&dir),
            None,
        );
        assert!(pack.project_ir_included);
        assert!(pack.prompt.contains("packages/app is the desktop shell"));
        assert!(pack.prompt.contains("deadbeef"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
