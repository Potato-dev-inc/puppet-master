use super::{AgentAdapter, HeuristicAdapter};
use crate::events::SystemEvent;

pub struct OpenCodeAdapter {
    inner: HeuristicAdapter,
}

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self {
            inner: HeuristicAdapter::new("opencode"),
        }
    }
}

impl AgentAdapter for OpenCodeAdapter {
    fn agent_type(&self) -> &'static str {
        self.inner.agent_type()
    }

    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent> {
        self.inner.observe(pane_id, text)
    }
}
