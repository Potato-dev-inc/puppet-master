//! Text Your Agents: in-memory references that tie chat messages to panes and companions.
//!
//! All of this is lost on restart, on purpose:
//! - the numbered pane list that `/panes` shows, used by `/to N` (30 minutes);
//! - which pane or companion each message the bot sent was about, so a reply-to works (24 hours,
//!   at most 200 messages);
//! - the conversation windows (15 minutes) that let a companion's reply come back to the chat.

use std::collections::{HashMap, VecDeque};

/// The numbered pane list stays usable this long.
pub const LIST_TTL_MS: u64 = 30 * 60_000;
/// A sent message stays linked to its target this long.
pub const MESSAGE_TTL_MS: u64 = 24 * 3_600_000;
/// At most this many sent messages are linked. The oldest link goes first.
pub const MAX_MESSAGE_REFS: usize = 200;
/// A conversation window stays open this long after it was opened or refreshed.
pub const WINDOW_TTL_MS: u64 = 15 * 60_000;

/// What a chat message or conversation is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Pane(String),
    Companion(String),
}

#[derive(Default)]
pub struct Refs {
    /// When the list was set, and the pane ids in number order (1 is the first).
    list: Option<(u64, Vec<String>)>,
    /// (Telegram message id, target, when it was remembered), oldest first.
    messages: VecDeque<(i64, Target, u64)>,
    /// Open conversation windows and the time each one closes.
    windows: HashMap<Target, u64>,
}

impl Refs {
    /// Replace the numbered pane list. Numbers start at 1.
    pub fn set_list(&mut self, panes: Vec<String>, now: u64) {
        self.list = Some((now, panes));
    }

    /// The pane with number `n` (1-based), if the list is still fresh.
    pub fn pane_at(&self, n: usize, now: u64) -> Option<String> {
        let (at, panes) = self.list.as_ref()?;
        if n == 0 || now >= at.saturating_add(LIST_TTL_MS) {
            return None;
        }
        panes.get(n - 1).cloned()
    }

    /// Link a sent message to its target. A message id is linked once; a new link replaces the old one.
    pub fn remember(&mut self, message_id: i64, t: Target, now: u64) {
        self.messages.retain(|(id, _, at)| *id != message_id && now < at.saturating_add(MESSAGE_TTL_MS));
        while self.messages.len() >= MAX_MESSAGE_REFS {
            self.messages.pop_front();
        }
        self.messages.push_back((message_id, t, now));
    }

    /// The target of a message the bot sent, if the link is still fresh.
    pub fn target_of(&self, message_id: i64, now: u64) -> Option<Target> {
        self.messages
            .iter()
            .find(|(id, _, _)| *id == message_id)
            .filter(|(_, _, at)| now < at.saturating_add(MESSAGE_TTL_MS))
            .map(|(_, t, _)| t.clone())
    }

    /// Open (or refresh) the conversation window for `t`.
    pub fn open_window(&mut self, t: &Target, now: u64) {
        self.windows.retain(|_, until| now < *until);
        self.windows.insert(t.clone(), now.saturating_add(WINDOW_TTL_MS));
    }

    /// Is the conversation window for `t` still open?
    pub fn in_window(&self, t: &Target, now: u64) -> bool {
        self.windows.get(t).is_some_and(|until| now < *until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str) -> Target {
        Target::Pane(id.to_string())
    }

    #[test]
    fn pane_list_numbers_from_one_and_expires_after_30_minutes() {
        let mut r = Refs::default();
        assert_eq!(r.pane_at(1, 0), None);
        r.set_list(vec!["p-a".to_string(), "p-b".to_string()], 1_000);
        assert_eq!(r.pane_at(1, 1_000).as_deref(), Some("p-a"));
        assert_eq!(r.pane_at(2, 1_000).as_deref(), Some("p-b"));
        assert_eq!(r.pane_at(0, 1_000), None);
        assert_eq!(r.pane_at(3, 1_000), None);
        assert_eq!(r.pane_at(1, 1_000 + LIST_TTL_MS - 1).as_deref(), Some("p-a"));
        assert_eq!(r.pane_at(1, 1_000 + LIST_TTL_MS), None);
    }

    #[test]
    fn a_new_pane_list_replaces_the_old_one() {
        let mut r = Refs::default();
        r.set_list(vec!["old".to_string()], 0);
        r.set_list(vec!["new".to_string()], 10);
        assert_eq!(r.pane_at(1, 11).as_deref(), Some("new"));
    }

    #[test]
    fn message_link_is_found_and_expires_after_24_hours() {
        let mut r = Refs::default();
        r.remember(7, pane("p-a"), 0);
        assert_eq!(r.target_of(7, 0), Some(pane("p-a")));
        assert_eq!(r.target_of(7, MESSAGE_TTL_MS - 1), Some(pane("p-a")));
        assert_eq!(r.target_of(7, MESSAGE_TTL_MS), None);
        assert_eq!(r.target_of(8, 0), None);
    }

    #[test]
    fn remembering_the_same_message_again_replaces_the_link() {
        let mut r = Refs::default();
        r.remember(7, pane("p-a"), 0);
        r.remember(7, Target::Companion("c-1".to_string()), 5);
        assert_eq!(r.target_of(7, 6), Some(Target::Companion("c-1".to_string())));
    }

    #[test]
    fn message_links_cap_at_200_and_drop_the_oldest() {
        let mut r = Refs::default();
        for i in 0..=MAX_MESSAGE_REFS as i64 {
            r.remember(i, pane("p"), 0);
        }
        assert_eq!(r.target_of(0, 1), None);
        assert_eq!(r.target_of(1, 1), Some(pane("p")));
        assert_eq!(r.target_of(MAX_MESSAGE_REFS as i64, 1), Some(pane("p")));
        assert_eq!(r.messages.len(), MAX_MESSAGE_REFS);
    }

    #[test]
    fn conversation_window_lasts_15_minutes_and_is_per_target() {
        let mut r = Refs::default();
        assert!(!r.in_window(&pane("p-a"), 0));
        r.open_window(&pane("p-a"), 0);
        assert!(r.in_window(&pane("p-a"), 0));
        assert!(r.in_window(&pane("p-a"), WINDOW_TTL_MS - 1));
        assert!(!r.in_window(&pane("p-a"), WINDOW_TTL_MS));
        assert!(!r.in_window(&pane("p-b"), 0));
    }

    #[test]
    fn reopening_a_window_extends_it() {
        let mut r = Refs::default();
        let companion = Target::Companion("mochi".to_string());
        r.open_window(&companion, 0);
        r.open_window(&companion, 10 * 60_000);
        // The first window would have closed at 15 minutes; the refreshed one closes at 25.
        assert!(r.in_window(&companion, 20 * 60_000));
        assert!(r.in_window(&companion, 10 * 60_000 + WINDOW_TTL_MS - 1));
        assert!(!r.in_window(&companion, 10 * 60_000 + WINDOW_TTL_MS));
    }
}
