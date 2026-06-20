use super::{AgentAdapter, HeuristicAdapter};
use crate::events::SystemEvent;

pub struct ClaudeAdapter {
    inner: HeuristicAdapter,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self {
            inner: HeuristicAdapter::new("claude"),
        }
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn agent_type(&self) -> &'static str {
        self.inner.agent_type()
    }

    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent> {
        self.inner.observe(pane_id, text)
    }
}
