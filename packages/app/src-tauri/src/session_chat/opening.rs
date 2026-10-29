//! How a pane's chat is read: from the session file its agent writes, or off its screen. One answer for the
//! desktop command and the phone route, so both treat a pane the same way.

use super::api::SessionApi;
use super::model::ChatItem;
use super::source::SessionSource;
use super::watch::{Hold, Watchers};

/// What a pane's chat is read from.
#[derive(Clone)]
pub enum Opening {
    /// The agent's own session file.
    File(&'static dyn SessionSource),
    /// The agent's session file once it has something to show, and the terminal's text until then (a file that may
    /// never be found). Holds the agent type.
    FileOrScreen(&'static dyn SessionSource, String),
    /// The terminal's text, for an agent whose file Puppet Master cannot read. Holds the agent type.
    Screen(String),
    /// The agent's local session API, and the terminal's text while the API does not answer. Holds the agent type.
    Api(&'static dyn SessionApi, String),
}

impl Opening {
    /// How this agent's chat is read; `None` for a pane that has no agent to read a conversation from (a shell).
    pub fn for_agent(agent_type: &str) -> Option<Opening> {
        let profile = crate::tui::profile_for(agent_type);
        match profile.session_chat() {
            Some(source) if source.screen_until_found() && profile.screen_chat() => Some(Opening::FileOrScreen(source, agent_type.to_string())),
            Some(source) => Some(Opening::File(source)),
            None if profile.session_api().is_some() && profile.screen_chat() => {
                profile.session_api().map(|api| Opening::Api(api, agent_type.to_string()))
            }
            None if profile.screen_chat() => Some(Opening::Screen(agent_type.to_string())),
            None => None,
        }
    }

    /// Whether the chat is read from the agent's session file and from nothing else (so it can only ever be a file's).
    pub fn is_file_only(&self) -> bool {
        matches!(self, Opening::File(_))
    }

    /// `"file"`, `"screen"` or `"api"`: where the items come from. For `Api` and `FileOrScreen` this is what it starts as; the
    /// pane's reader knows what it reads from now (`Watchers::source_of`).
    pub fn source_name(&self) -> &'static str {
        match self {
            Opening::File(_) => "file",
            Opening::FileOrScreen(..) => "screen",
            Opening::Screen(_) => "screen",
            Opening::Api(..) => "api",
        }
    }

    /// Where the open pane's items come from now (an API chat falls back to the screen and back).
    pub fn source_now(&self, watchers: &Watchers, pane_id: &str) -> &'static str {
        watchers.source_of(pane_id).unwrap_or_else(|| self.source_name())
    }

    /// Starts the pane's reader (or joins it) as `hold`; the items so far.
    pub fn open(&self, watchers: &Watchers, pane_id: &str, hold: Hold) -> Vec<ChatItem> {
        match self {
            Opening::File(source) => watchers.open(pane_id, hold, *source),
            Opening::FileOrScreen(source, agent_type) => watchers.open_screen_first(pane_id, hold, *source, agent_type),
            Opening::Screen(agent_type) => watchers.open_screen(pane_id, hold, agent_type),
            Opening::Api(api, agent_type) => watchers.open_api(pane_id, hold, *api, agent_type),
        }
    }
}

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;
