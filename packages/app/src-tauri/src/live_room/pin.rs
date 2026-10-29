//! Which chat a Live Room may publish for a pane (SPEC-n32 3.3, 4 #3). A chat read from a session file is only the
//! picked pane's when the file is tied to the pane for sure: the pane has an agent session id (launch id or hook), and
//! the file the reader follows carries that id in its name. Without an id the reader takes "the newest file of the
//! folder", which can be another session's, so such a pane's chat is not shared. Once a pane is tied to an id the room
//! never moves to another: when the id changes, the chat of that pane stops for the rest of the room.
//! No agent is named here: the file name check is the same for every agent.

use std::collections::HashMap;
use std::path::Path;

/// What a room does with one pane's chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatShare {
    /// The chat is the pane's own: share it.
    Shared,
    /// The chat cannot be tied to this pane (no session id yet, or the file followed is not that session's).
    NotPinned,
    /// The pane's session changed after the room tied it to one. Not shared again in this room.
    SessionChanged,
}

/// The note viewers get once when a pane's chat is not shared. Fixed text; nothing of the chat, folder or session.
pub fn note_for(share: ChatShare) -> Option<&'static str> {
    match share {
        ChatShare::Shared => None,
        ChatShare::NotPinned => Some("This pane's chat is not shared: it can't be tied to this pane for sure."),
        ChatShare::SessionChanged => Some("This pane's chat stopped being shared: its session changed."),
    }
}

/// An agent session id shorter than this is not trusted to tell files apart.
const MIN_ID: usize = 8;

enum Pin {
    On(String),
    Ended,
}

/// The session each pane was tied to, for the life of one room (or one preview).
#[derive(Default)]
pub struct Pins {
    panes: HashMap<String, Pin>,
}

impl Pins {
    /// `id`: the pane's agent session id now (`None`: not known). `file`: the session file the pane's reader follows
    /// (`None`: no file, so nothing is read from one). `from_file`: the pane's chat is read from a session file
    /// (else off the screen, which is always the pane's own).
    pub fn check(&mut self, pane: &str, id: Option<&str>, file: Option<&Path>, from_file: bool) -> ChatShare {
        let id = id.filter(|id| id.len() >= MIN_ID);
        match self.panes.get(pane) {
            Some(Pin::Ended) => return ChatShare::SessionChanged,
            Some(Pin::On(pinned)) if id.is_some_and(|now| now != pinned) => {
                self.panes.insert(pane.to_string(), Pin::Ended);
                return ChatShare::SessionChanged;
            }
            _ => {}
        }
        if !from_file {
            return ChatShare::Shared;
        }
        if !self.panes.contains_key(pane) {
            let Some(id) = id else { return ChatShare::NotPinned };
            self.panes.insert(pane.to_string(), Pin::On(id.to_string()));
        }
        let Some(Pin::On(pinned)) = self.panes.get(pane) else { return ChatShare::NotPinned };
        match file {
            None => ChatShare::Shared,
            Some(file) if names(file, pinned) => ChatShare::Shared,
            Some(_) => ChatShare::NotPinned,
        }
    }
}

/// Whether the file's name carries the session id (`<id>.jsonl`, `rollout-...-<id>.jsonl`).
fn names(file: &Path, id: &str) -> bool {
    file.file_name().is_some_and(|name| name.to_string_lossy().to_lowercase().contains(&id.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const A: &str = "11111111-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const B: &str = "22222222-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

    fn file(id: &str) -> PathBuf {
        PathBuf::from(format!("C:/Users/you/.agent/projects/proj/{id}.jsonl"))
    }

    #[test]
    fn a_pane_without_a_session_id_shares_no_chat() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p", None, Some(&file(B)), true), ChatShare::NotPinned);
        assert_eq!(pins.check("p", Some(""), Some(&file(B)), true), ChatShare::NotPinned);
        assert_eq!(pins.check("p", Some("short"), Some(&file(B)), true), ChatShare::NotPinned);
        assert_eq!(pins.check("p", None, None, true), ChatShare::NotPinned);
    }

    #[test]
    fn a_pinned_pane_shares_only_the_file_named_after_its_id() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p", Some(A), Some(&file(A)), true), ChatShare::Shared);
        assert_eq!(pins.check("p", Some(A), Some(&file(B)), true), ChatShare::NotPinned, "another session's file");
        assert_eq!(pins.check("p", Some(A), Some(&file(A)), true), ChatShare::Shared, "back on its own file");
        assert_eq!(pins.check("p", Some(A), None, true), ChatShare::Shared, "no file, nothing to leak");
    }

    #[test]
    fn a_pane_pinned_late_starts_sharing_once() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p", None, Some(&file(B)), true), ChatShare::NotPinned);
        assert_eq!(pins.check("p", Some(A), Some(&file(B)), true), ChatShare::NotPinned, "still the foreign file");
        assert_eq!(pins.check("p", Some(A), Some(&file(A)), true), ChatShare::Shared);
    }

    #[test]
    fn a_changed_session_id_stops_the_chat_for_good() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p", Some(A), Some(&file(A)), true), ChatShare::Shared);
        assert_eq!(pins.check("p", Some(B), Some(&file(B)), true), ChatShare::SessionChanged);
        assert_eq!(pins.check("p", Some(A), Some(&file(A)), true), ChatShare::SessionChanged, "never back");
        assert_eq!(pins.check("p", None, Some(&file(A)), true), ChatShare::SessionChanged);
    }

    #[test]
    fn a_pane_whose_id_goes_missing_keeps_its_pin() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p", Some(A), Some(&file(A)), true), ChatShare::Shared);
        assert_eq!(pins.check("p", None, Some(&file(A)), true), ChatShare::Shared);
        assert_eq!(pins.check("p", None, Some(&file(B)), true), ChatShare::NotPinned);
    }

    #[test]
    fn a_chat_read_off_the_screen_needs_no_pin() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p", None, None, false), ChatShare::Shared);
    }

    #[test]
    fn panes_are_pinned_one_by_one() {
        let mut pins = Pins::default();
        assert_eq!(pins.check("p1", Some(A), Some(&file(A)), true), ChatShare::Shared);
        assert_eq!(pins.check("p2", None, Some(&file(A)), true), ChatShare::NotPinned);
        assert_eq!(pins.check("p2", Some(B), Some(&file(A)), true), ChatShare::NotPinned, "p2's file is p1's");
        assert_eq!(pins.check("p2", Some(B), Some(&file(B)), true), ChatShare::Shared);
    }

    #[test]
    fn the_notes_are_fixed_sentences() {
        assert_eq!(note_for(ChatShare::Shared), None);
        assert!(note_for(ChatShare::NotPinned).is_some());
        assert!(note_for(ChatShare::SessionChanged).is_some());
    }
}
