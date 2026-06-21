use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_STANDBY_POLL_MS: u64 = 1_500;
pub const DEFAULT_STANDBY_MAX_MS: u64 = 10 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneRole {
    Implementer,
    Reviewer,
    Shell,
    Orchestrator,
    Observer,
}

impl PaneRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
            Self::Shell => "shell",
            Self::Orchestrator => "orchestrator",
            Self::Observer => "observer",
        }
    }

    #[allow(dead_code)]
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "implementer" => Ok(Self::Implementer),
            "reviewer" => Ok(Self::Reviewer),
            "shell" => Ok(Self::Shell),
            "orchestrator" => Ok(Self::Orchestrator),
            "observer" => Ok(Self::Observer),
            _ => Err(format!("invalid pane role: {value}")),
        }
    }
}

#[allow(dead_code)]
pub fn allowed_pane_roles() -> Vec<PaneRole> {
    vec![
        PaneRole::Implementer,
        PaneRole::Reviewer,
        PaneRole::Shell,
        PaneRole::Orchestrator,
        PaneRole::Observer,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneDigest {
    pub pane_id: String,
    pub summary: String,
    pub source: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTimelineEvent {
    pub timestamp_ms: i64,
    pub actor: String,
    pub event_type: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockConflictProjection {
    pub resource_id: String,
    pub requested_owner_id: String,
    pub existing_owner_id: String,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorStateProjection {
    pub standby_poll_ms: u64,
    pub standby_max_ms: u64,
}

impl Default for OrchestratorStateProjection {
    fn default() -> Self {
        Self {
            standby_poll_ms: DEFAULT_STANDBY_POLL_MS,
            standby_max_ms: DEFAULT_STANDBY_MAX_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionContextProjection {
    pub current_goal: Option<String>,
    pub pane_roles: BTreeMap<String, PaneRole>,
    pub pane_digests: BTreeMap<String, PaneDigest>,
    pub timeline: Vec<SessionTimelineEvent>,
    pub lock_conflicts: Vec<LockConflictProjection>,
    pub orchestrator: OrchestratorStateProjection,
}

impl Default for SessionContextProjection {
    fn default() -> Self {
        Self {
            current_goal: None,
            pane_roles: BTreeMap::new(),
            pane_digests: BTreeMap::new(),
            timeline: Vec::new(),
            lock_conflicts: Vec::new(),
            orchestrator: OrchestratorStateProjection::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegateTaskRequest {
    pub task_id: Option<String>,
    pub target_pane_id: Option<String>,
    pub intent: String,
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub locked_resources: Vec<String>,
    #[serde(default)]
    pub evidence_required: Vec<String>,
    pub token_budget_hint: Option<u64>,
    pub timeout_ms: Option<u64>,
}

impl DelegateTaskRequest {
    pub fn validated(self) -> Result<Self, String> {
        let intent = self.intent.trim().to_string();
        if intent.is_empty() {
            return Err("intent is required".to_string());
        }
        let acceptance_criteria = clean_nonempty(self.acceptance_criteria);
        if acceptance_criteria.is_empty() {
            return Err("acceptance_criteria must contain at least one item".to_string());
        }
        Ok(Self {
            task_id: clean_optional(self.task_id),
            target_pane_id: clean_optional(self.target_pane_id),
            intent,
            acceptance_criteria,
            locked_resources: clean_nonempty(self.locked_resources),
            evidence_required: clean_nonempty(self.evidence_required),
            token_budget_hint: self.token_budget_hint,
            timeout_ms: self.timeout_ms,
        })
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn clean_nonempty(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

pub fn render_codex_delegation_prompt(request: &DelegateTaskRequest) -> String {
    let mut sections = vec![
        "You are a worker agent in a Puppet Master session.".to_string(),
        format!("Task intent: {}", request.intent),
    ];
    if let Some(task_id) = &request.task_id {
        sections.push(format!("Task id: {task_id}"));
    }
    if let Some(target_pane_id) = &request.target_pane_id {
        sections.push(format!("Target pane: {target_pane_id}"));
    }
    sections.push(format!(
        "Acceptance criteria:\n{}",
        bullet_list(&request.acceptance_criteria)
    ));
    if !request.locked_resources.is_empty() {
        sections.push(format!(
            "Locked resources:\n{}",
            bullet_list(&request.locked_resources)
        ));
    }
    if !request.evidence_required.is_empty() {
        sections.push(format!(
            "Evidence required:\n{}",
            bullet_list(&request.evidence_required)
        ));
    }
    if let Some(token_budget_hint) = request.token_budget_hint {
        sections.push(format!("Token budget hint: {token_budget_hint}"));
    }
    if let Some(timeout_ms) = request.timeout_ms {
        sections.push(format!("Timeout: {timeout_ms} ms"));
    }
    sections.join("\n\n")
}

fn bullet_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_session_context_current_goal() {
        let context = SessionContextProjection {
            current_goal: Some("Finish Rust migration".to_string()),
            ..SessionContextProjection::default()
        };
        let value = serde_json::to_value(context).unwrap();
        assert_eq!(value["current_goal"], json!("Finish Rust migration"));
    }

    #[test]
    fn serializes_each_allowed_pane_role() {
        let roles = allowed_pane_roles();
        let values = roles
            .iter()
            .map(|role| serde_json::to_value(role).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![
                json!("implementer"),
                json!("reviewer"),
                json!("shell"),
                json!("orchestrator"),
                json!("observer")
            ]
        );
    }

    #[test]
    fn rejects_invalid_pane_role() {
        assert!(PaneRole::parse("driver").is_err());
    }

    #[test]
    fn serializes_pane_digest() {
        let digest = PaneDigest {
            pane_id: "pane-1".to_string(),
            summary: "Tests are running".to_string(),
            source: "manual".to_string(),
            updated_at_ms: 42,
        };
        let value = serde_json::to_value(digest).unwrap();
        assert_eq!(value["summary"], json!("Tests are running"));
    }

    #[test]
    fn validates_delegate_task_request() {
        let request = DelegateTaskRequest {
            task_id: Some("task-1".to_string()),
            target_pane_id: Some("codex-1".to_string()),
            intent: " Implement feature ".to_string(),
            acceptance_criteria: vec![" Tests pass ".to_string()],
            locked_resources: vec!["file:src/lib.rs".to_string()],
            evidence_required: vec!["Test output".to_string()],
            token_budget_hint: Some(8000),
            timeout_ms: Some(600_000),
        }
        .validated()
        .unwrap();
        assert_eq!(request.intent, "Implement feature");
        assert_eq!(request.acceptance_criteria, vec!["Tests pass"]);
    }

    #[test]
    fn rejects_missing_delegate_intent() {
        let request = DelegateTaskRequest {
            task_id: None,
            target_pane_id: None,
            intent: " ".to_string(),
            acceptance_criteria: vec!["Tests pass".to_string()],
            locked_resources: Vec::new(),
            evidence_required: Vec::new(),
            token_budget_hint: None,
            timeout_ms: None,
        };
        assert_eq!(request.validated().unwrap_err(), "intent is required");
    }

    #[test]
    fn rejects_empty_acceptance_criteria() {
        let request = DelegateTaskRequest {
            task_id: None,
            target_pane_id: None,
            intent: "Implement feature".to_string(),
            acceptance_criteria: vec![],
            locked_resources: Vec::new(),
            evidence_required: Vec::new(),
            token_budget_hint: None,
            timeout_ms: None,
        };
        assert_eq!(
            request.validated().unwrap_err(),
            "acceptance_criteria must contain at least one item"
        );
    }

    #[test]
    fn renders_codex_delegation_prompt_shape() {
        let request = DelegateTaskRequest {
            task_id: Some("task-1".to_string()),
            target_pane_id: Some("codex-1".to_string()),
            intent: "Refactor bridge routes".to_string(),
            acceptance_criteria: vec!["Route tests pass".to_string()],
            locked_resources: vec!["file:packages/app/src-tauri/src/bridge.rs".to_string()],
            evidence_required: vec!["cargo test output".to_string()],
            token_budget_hint: None,
            timeout_ms: None,
        };
        let prompt = render_codex_delegation_prompt(&request);
        assert!(prompt.contains("Task intent: Refactor bridge routes"));
        assert!(prompt.contains("- Route tests pass"));
        assert!(prompt.contains("Locked resources"));
        assert!(prompt.contains("Evidence required"));
    }

    #[test]
    fn default_orchestrator_state_matches_sidebar_policy() {
        let state = OrchestratorStateProjection::default();
        assert_eq!(state.standby_poll_ms, 1_500);
        assert_eq!(state.standby_max_ms, 600_000);
    }
}
