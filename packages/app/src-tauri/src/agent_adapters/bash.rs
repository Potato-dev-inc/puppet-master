use super::{AgentAdapter, HeuristicAdapter};
use crate::events::SystemEvent;

pub struct BashAdapter {
    inner: HeuristicAdapter,
}

impl Default for BashAdapter {
    fn default() -> Self {
        Self {
            inner: HeuristicAdapter::new("bash"),
        }
    }
}

impl AgentAdapter for BashAdapter {
    fn agent_type(&self) -> &'static str {
        self.inner.agent_type()
    }

    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent> {
        self.inner.observe(pane_id, text)
    }
}
