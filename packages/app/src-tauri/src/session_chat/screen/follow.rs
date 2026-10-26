//! One pane's screen reader as the watcher drives it: ask the pane what it shows, hand that to `ScreenChat`.

use super::{ScreenChat, ScreenRead};
use crate::session_chat::watch::{Change, PaneLookup};

pub(in crate::session_chat) struct ScreenFollower {
    chat: ScreenChat,
    /// The output position of the text read last (`ScreenRead::position`).
    known: Option<u64>,
    /// Whether the pane has been seen at all.
    seen: bool,
}

impl ScreenFollower {
    pub fn new(agent_type: &str) -> Self {
        ScreenFollower { chat: ScreenChat::new(agent_type), known: None, seen: false }
    }

    /// One look at the pane. `None` when the pane is gone.
    pub fn step(&mut self, panes: &dyn PaneLookup, pane_id: &str) -> Option<Change> {
        let ScreenRead { status, position, text } = panes.screen(pane_id, self.known)?;
        self.known = position;
        self.seen = true;
        Some(self.chat.observe(text.as_deref(), &status))
    }

    /// For a chat with nothing to show: what it waits for.
    pub fn waiting(&self) -> String {
        if self.seen {
            "Reading the terminal: no message from you or the agent on the screen yet.".to_string()
        } else {
            "Waiting for the terminal to show something.".to_string()
        }
    }
}
