//! The chat of an agent whose session file may never be found (a wrong folder in a definition someone wrote):
//! the pane is read off its screen, as for an agent with no file, until the file has something to show. Then the
//! chat switches to the file for good: the viewer is sent the file's items in place of the screen's (a reset) and
//! told where the items come from now (`Sink::source`), like the API chat does when its API answers.

use std::time::{Duration, Instant};

use super::model::ChatItem;
use super::screen::ScreenFollower;
use super::watch::{Change, Follower, PaneLookup};

pub(super) struct ScreenFirstFollower {
    file: Follower,
    screen: ScreenFollower,
    /// The items the file gave while the screen was shown (all of them: the first look at a long file is a backlog).
    from_file: Vec<ChatItem>,
    on_file: bool,
}

impl ScreenFirstFollower {
    pub fn new(file: Follower, agent_type: &str) -> Self {
        ScreenFirstFollower { file, screen: ScreenFollower::new(agent_type), from_file: Vec::new(), on_file: false }
    }

    /// Whether the items come from the file now (else from the screen).
    pub fn on_file(&self) -> bool {
        self.on_file
    }

    /// The session file the items come from: set once the chat is on the file, `None` while the screen is shown.
    pub fn file_now(&self) -> Option<&std::path::Path> {
        if self.on_file {
            self.file.file()
        } else {
            None
        }
    }

    /// One look at the pane. `None` when the pane is gone.
    pub fn step(&mut self, panes: &dyn PaneLookup, pane_id: &str, now: Instant, relocate: Duration) -> Option<Change> {
        let facts = panes.facts(pane_id)?;
        self.file.set_others(panes.other_sessions(pane_id));
        let change = self.file.step(&facts, now, relocate);
        if self.on_file {
            return Some(change);
        }
        match change {
            Change::Reset(items) => self.from_file = items,
            Change::Append(items) | Change::Upsert(items) => self.from_file.extend(items),
            Change::Nothing => {}
        }
        if self.from_file.is_empty() {
            return self.screen.step(panes, pane_id);
        }
        self.on_file = true;
        Some(Change::Reset(std::mem::take(&mut self.from_file)))
    }

    /// For a chat with nothing to show: what it waits for.
    pub fn waiting(&self, panes: &dyn PaneLookup, pane_id: &str) -> Option<String> {
        if self.on_file {
            Some(self.file.waiting(&panes.facts(pane_id)?))
        } else {
            Some(self.screen.waiting())
        }
    }
}

#[cfg(test)]
#[path = "screen_first_tests.rs"]
mod tests;
