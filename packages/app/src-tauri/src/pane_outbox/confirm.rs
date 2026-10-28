//! A delivery that pressed Enter but saw no turn start comes back "submitted, unconfirmed". For the outbox that
//! used to read as sent. When the agent's input box still holds the very text that was typed, nothing was sent:
//! the Enter was swallowed (a paste still arriving, a line break). That is a failure the person has to see.

use crate::agent_runs::delivery::{self, DeliveryIo};
use crate::agent_runs::readiness;
use crate::tui::{ComposerState, TuiProfile};
use std::time::Duration;

/// Looks taken, a poll apart (about a second in all): the box must hold the text on every one of them.
const LOOKS: u32 = 10;
const POLL: Duration = Duration::from_millis(100);

/// The text is still in the input box. A profile that cannot read its box (`ComposerState::Unknown`) never says
/// so, and neither does a screen with a dialog on it: both leave the delivery's own verdict as it was.
pub(super) fn still_in_box(io: &mut dyn DeliveryIo, profile: &dyn TuiProfile, text: &str) -> bool {
    for look in 0..LOOKS {
        let verdict = readiness::assess(profile, io.observe());
        let holds = verdict.gate().is_none()
            && verdict.composer() == ComposerState::HasText
            && delivery::prompt_visible_in_input(verdict.screen(), text);
        if !holds {
            return false;
        }
        if look + 1 < LOOKS {
            io.pause(POLL);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::readiness::{AgentProcess, Observation};
    use crate::pty::WriteOrigin;
    use std::sync::Arc;

    const CLAUDE_READY: &str = include_str!("../tui/claude/fixtures/ready_fresh_2_1_299_live.txt");
    const TEXT: &str = "Please change this in the app: make this bigger and green";

    /// A Claude pane that shows `box_text` in its input box, and empties it after `empties_after` looks.
    struct Pane {
        box_text: String,
        looks: u32,
        empties_after: Option<u32>,
    }

    impl DeliveryIo for Pane {
        fn write(&mut self, _origin: WriteOrigin<'_>, _text: &str, _enter: bool) -> Result<(), String> {
            Err("a confirmation never writes".into())
        }
        fn screen(&mut self) -> String {
            if self.box_text.is_empty() {
                CLAUDE_READY.to_string()
            } else {
                CLAUDE_READY.replace("Try \"edit <filepath> to...\"", &self.box_text)
            }
        }
        fn observe(&mut self) -> Observation {
            self.looks += 1;
            if self.empties_after.is_some_and(|n| self.looks > n) {
                self.box_text.clear();
            }
            Observation { pane_id: String::new(), screen: self.screen(), output_pos: None, agent: AgentProcess::Alive }
        }
        fn busy(&mut self) -> bool {
            false
        }
        fn pause(&mut self, _duration: Duration) {}
        fn profile(&mut self) -> Arc<dyn TuiProfile> {
            crate::tui::profile_for("claude")
        }
    }

    fn claude() -> Arc<dyn TuiProfile> {
        crate::tui::profile_for("claude")
    }

    #[test]
    fn text_that_stays_in_the_box_was_not_sent() {
        let mut pane = Pane { box_text: TEXT.into(), looks: 0, empties_after: None };
        assert!(still_in_box(&mut pane, claude().as_ref(), TEXT));
    }

    #[test]
    fn a_box_that_empties_while_we_look_means_the_turn_started() {
        let mut pane = Pane { box_text: TEXT.into(), looks: 0, empties_after: Some(3) };
        assert!(!still_in_box(&mut pane, claude().as_ref(), TEXT));
    }

    #[test]
    fn an_empty_box_or_other_text_in_it_is_not_this_message_stuck() {
        let mut empty = Pane { box_text: String::new(), looks: 0, empties_after: None };
        assert!(!still_in_box(&mut empty, claude().as_ref(), TEXT));
        let mut other = Pane { box_text: "something the person typed after".into(), looks: 0, empties_after: None };
        assert!(!still_in_box(&mut other, claude().as_ref(), TEXT));
    }
}
