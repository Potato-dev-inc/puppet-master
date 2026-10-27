//! How a pane's chat is read: from the session file its agent writes, or off its screen. One answer for the
//! desktop command and the phone route, so both treat a pane the same way.

use super::model::ChatItem;
use super::source::SessionSource;
use super::watch::{Hold, Watchers};

/// What a pane's chat is read from.
#[derive(Clone)]
pub enum Opening {
    /// The agent's own session file.
    File(&'static dyn SessionSource),
    /// The terminal's text, for an agent whose file Puppet Master cannot read. Holds the agent type.
    Screen(String),
}

impl Opening {
    /// How this agent's chat is read; `None` for a pane that has no agent to read a conversation from (a shell).
    pub fn for_agent(agent_type: &str) -> Option<Opening> {
        let profile = crate::tui::profile_for(agent_type);
        match profile.session_chat() {
            Some(source) => Some(Opening::File(source)),
            None if profile.screen_chat() => Some(Opening::Screen(agent_type.to_string())),
            None => None,
        }
    }

    /// `"file"` or `"screen"`: where the items come from.
    pub fn source_name(&self) -> &'static str {
        match self {
            Opening::File(_) => "file",
            Opening::Screen(_) => "screen",
        }
    }

    /// Starts the pane's reader (or joins it) as `hold`; the items so far.
    pub fn open(&self, watchers: &Watchers, pane_id: &str, hold: Hold) -> Vec<ChatItem> {
        match self {
            Opening::File(source) => watchers.open(pane_id, hold, *source),
            Opening::Screen(agent_type) => watchers.open_screen(pane_id, hold, agent_type),
        }
    }
}

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;
