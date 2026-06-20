use super::{AgentAdapter, HeuristicAdapter};
use crate::events::SystemEvent;

pub struct CodexAdapter {
    inner: HeuristicAdapter,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self {
            inner: HeuristicAdapter::new("codex"),
        }
    }
}

impl AgentAdapter for CodexAdapter {
    fn agent_type(&self) -> &'static str {
        self.inner.agent_type()
    }

    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent> {
        self.inner.observe(pane_id, text)
    }
}
