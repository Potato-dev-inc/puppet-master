//! The chat of an agent that serves its session over a local HTTP API (`SessionApi`, implemented under
//! `src/tui/<agent>/`). `ApiFollower` asks the API for the whole conversation about once a second and sends only
//! what changed; item ids are stable (the agent's message id plus the part), so a message that grows replaces its
//! old copy. When the API cannot be reached (or does not know the session yet) the pane is read off its screen
//! as before, and the chat goes back to the API as soon as it answers again.
//! Chat text is never logged here.

use super::model::ChatItem;
use super::screen::{diff, ScreenFollower};
use super::source::PaneFacts;
use super::watch::{Change, PaneLookup};

/// Where a pane's agent serves its session API.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiEndpoint {
    /// `http://127.0.0.1:<port>`.
    pub base_url: String,
    /// The conversation to read, when the pane already knows it; `None`: the API finds it (the newest one this
    /// pane started).
    pub session_id: Option<String>,
}

/// One agent's session API: how to read a conversation from it.
pub trait SessionApi: Send + Sync {
    /// Every item of the pane's conversation as the API tells it now, with ids that stay the same from one call
    /// to the next. `Err`: the API cannot be reached, or the conversation is not there yet (the screen is read
    /// instead). Called about once a second from the pane's reader thread; it must not block for long.
    fn fetch(&self, pane: &PaneFacts, endpoint: &ApiEndpoint) -> Result<Vec<ChatItem>, String>;
}

/// Failed looks in a row before the chat is read off the screen again (a single slow answer must not flip it).
const FALL_BACK_AFTER: u8 = 3;

/// Where the items of a pane's chat come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFrom {
    Api,
    Screen,
}

impl ChatFrom {
    pub fn name(self) -> &'static str {
        match self {
            ChatFrom::Api => "api",
            ChatFrom::Screen => "screen",
        }
    }
}

pub(super) struct ApiFollower {
    api: &'static dyn SessionApi,
    screen: ScreenFollower,
    /// What the viewer has been sent from the API.
    shown: Vec<ChatItem>,
    from: ChatFrom,
    failures: u8,
}

impl ApiFollower {
    pub fn new(api: &'static dyn SessionApi, agent_type: &str) -> Self {
        ApiFollower { api, screen: ScreenFollower::new(agent_type), shown: Vec::new(), from: ChatFrom::Screen, failures: 0 }
    }

    pub fn from(&self) -> ChatFrom {
        self.from
    }

    /// One look at the pane. `None` when the pane is gone.
    pub fn step(&mut self, panes: &dyn PaneLookup, pane_id: &str) -> Option<Change> {
        let facts = panes.facts(pane_id);
        let endpoint = panes.api(pane_id);
        let fetched = match (&facts, &endpoint) {
            (Some(facts), Some(endpoint)) => Some(self.api.fetch(facts, endpoint)),
            _ => None,
        };
        match fetched {
            Some(Ok(items)) => {
                self.failures = 0;
                if self.from == ChatFrom::Screen {
                    // The viewer holds what the screen showed: everything it has is replaced.
                    self.from = ChatFrom::Api;
                    self.shown = items.clone();
                    return Some(Change::Reset(items));
                }
                Some(diff(&mut self.shown, items))
            }
            other => {
                self.failures = self.failures.saturating_add(1);
                // A look that failed while the API was answering a moment ago keeps what is shown.
                if other.is_some() && self.from == ChatFrom::Api && self.failures < FALL_BACK_AFTER {
                    return Some(Change::Nothing);
                }
                let change = self.screen.step(panes, pane_id)?;
                if self.from == ChatFrom::Api {
                    self.from = ChatFrom::Screen;
                    self.shown.clear();
                    return Some(Change::Reset(self.screen.all()));
                }
                Some(change)
            }
        }
    }

    /// For a chat with nothing to show: what it waits for.
    pub fn waiting(&self) -> String {
        match self.from {
            ChatFrom::Api => "Reading the agent's session: no message from you or the agent in it yet.".to_string(),
            ChatFrom::Screen => self.screen.waiting(),
        }
    }
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
