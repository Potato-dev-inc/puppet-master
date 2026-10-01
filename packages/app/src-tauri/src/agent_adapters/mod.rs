pub mod bash;
pub mod claude;
pub mod codex;
pub mod launch;
pub mod opencode;

use crate::events::{PaneId, SystemEvent};

#[allow(dead_code)]
pub trait AgentAdapter {
    fn agent_type(&self) -> &'static str;
    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent>;
    fn launch_spec(
        &self,
        project: &str,
        task: &str,
        read_only: bool,
    ) -> Result<launch::LaunchSpec, String> {
        launch::launch_spec(self.agent_type(), project, task, read_only)
    }
    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        launch::extract_result(self.agent_type(), output, exit_code)
    }
    fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<serde_json::Value> {
        launch::detect_prompt(self.agent_type(), screen, allow_broad)
    }
    fn prompt_reply(
        &self,
        prompt: &serde_json::Value,
        choice: &str,
        allow_broad: bool,
    ) -> Result<launch::Reply, String> {
        launch::prompt_reply(self.agent_type(), prompt, choice, allow_broad)
    }
}

#[derive(Default)]
pub struct HeuristicAdapter {
    agent_type: &'static str,
}

impl HeuristicAdapter {
    pub fn new(agent_type: &'static str) -> Self {
        Self { agent_type }
    }
}

impl AgentAdapter for HeuristicAdapter {
    fn agent_type(&self) -> &'static str {
        self.agent_type
    }

    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent> {
        let lower = text.to_ascii_lowercase();
        let mut observations = Vec::new();

        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "agent_ready",
            &lower,
            text,
            &["ready", "waiting for input"],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "agent_idle",
            &lower,
            text,
            &["idle", "awaiting"],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "agent_blocked",
            &lower,
            text,
            &["blocked", "cannot continue", "need clarification"],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "permission_prompt",
            &lower,
            text,
            &["permission", "approval required", "allow this command"],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "command_started",
            &lower,
            text,
            &["running command", "$ ", "> "],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "command_finished",
            &lower,
            text,
            &["exit code", "process exited", "command finished"],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "test_failure",
            &lower,
            text,
            &["test failed", "tests failed", "failures:", "failed."],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "test_success",
            &lower,
            text,
            &["test result: ok", "tests passed", "all tests passed"],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "file_change_mention",
            &lower,
            text,
            &["modified ", "created ", "wrote ", "updated "],
        );
        push_if(
            &mut observations,
            pane_id,
            self.agent_type,
            "final_report",
            &lower,
            text,
            &["final answer", "summary:", "completed:"],
        );

        observations
    }
}

pub fn adapter_for(agent_type: &str) -> Box<dyn AgentAdapter + Send> {
    match agent_type {
        "claude" => Box::new(claude::ClaudeAdapter::default()),
        "codex" => Box::new(codex::CodexAdapter::default()),
        "cursor_agent" => Box::new(HeuristicAdapter::new("cursor_agent")),
        "opencode" => Box::new(opencode::OpenCodeAdapter::default()),
        "opencode_native" => Box::new(opencode::OpenCodeNativeAdapter::default()),
        "bash" | "cmd" | "powershell" => Box::new(HeuristicAdapter::new(Box::leak(
            agent_type.to_string().into_boxed_str(),
        ))),
        other => Box::new(HeuristicAdapter::new(Box::leak(
            other.to_string().into_boxed_str(),
        ))),
    }
}

fn push_if(
    observations: &mut Vec<SystemEvent>,
    pane_id: &str,
    agent_type: &str,
    observation: &str,
    lower: &str,
    original: &str,
    needles: &[&str],
) {
    if needles.iter().any(|needle| lower.contains(needle)) {
        observations.push(SystemEvent::AgentObservation {
            pane_id: PaneId(pane_id.to_string()),
            agent_type: agent_type.to_string(),
            observation: observation.to_string(),
            text: Some(original.trim().chars().take(500).collect()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_permission_and_test_success() {
        let mut adapter = adapter_for("codex");
        let observations = adapter.observe(
            "pane-1",
            "approval required before running command\ntest result: ok. 3 passed",
        );
        let names = observations
            .iter()
            .filter_map(|event| match event {
                SystemEvent::AgentObservation { observation, .. } => Some(observation.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(names.contains(&"permission_prompt"));
        assert!(names.contains(&"test_success"));
    }

    #[test]
    fn detects_blocked_state() {
        let mut adapter = adapter_for("claude");
        let observations = adapter.observe("pane-1", "Blocked: need clarification from user");
        assert!(matches!(
            &observations[0],
            SystemEvent::AgentObservation { observation, .. } if observation == "agent_blocked"
        ));
    }
}
