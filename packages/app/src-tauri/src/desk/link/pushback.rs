//! When the controlled computer sends the pointer back home. Pure: no host, no link, so the counting rule is tested on
//! every platform.
//!
//! The pointer goes home only after a deliberate push, not on the first move that lands on the entry edge: a hand that
//! jitters at the edge, or a drag toward the other computer, must not hand the mouse back. A push is an outward move
//! that leaves the pointer on the entry edge. Pushes in a row count; any move that is not a push (inward, sideways, or
//! off the edge) starts the count again, and so does any key or button event.
//!
//! Two pushes in a row bring the pointer home. While any mouse button is held on this computer, three are needed, so a
//! press-and-drag cannot be thrown back by one bump.

/// Pushes in a row, with no button held here, that send the pointer home (as `DWELL_LOOKS` for the PC side).
pub const PUSHES_TO_RETURN: u32 = 2;
/// Pushes in a row while any button is held here.
pub const PUSHES_WHILE_HELD: u32 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushBack {
    run: u32,
}

impl PushBack {
    /// One motion. `on_entry_edge`: this move was outward and left the pointer on the entry edge. `button_held`: a
    /// mouse button is down on this computer. Returns true when the pointer should go home now; the count then starts
    /// again from zero.
    pub fn motion(&mut self, on_entry_edge: bool, button_held: bool) -> bool {
        if !on_entry_edge {
            self.run = 0;
            return false;
        }
        self.run = self.run.saturating_add(1);
        let needed = if button_held { PUSHES_WHILE_HELD } else { PUSHES_TO_RETURN };
        if self.run >= needed {
            self.run = 0;
            true
        } else {
            false
        }
    }

    /// A key or button event on this computer: the count starts again.
    pub fn reset(&mut self) {
        self.run = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_jitter_at_the_edge_does_nothing() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        // The pointer rests there without another push: still nothing.
        assert!(!p.motion(false, false));
    }

    #[test]
    fn two_pushes_in_a_row_return_the_pointer() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        assert!(p.motion(true, false));
    }

    #[test]
    fn a_move_that_is_not_a_push_in_between_starts_the_count_again() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        assert!(!p.motion(false, false), "a move back inward is not a push");
        assert!(!p.motion(true, false), "the count began again");
        assert!(p.motion(true, false));
    }

    #[test]
    fn a_button_held_needs_three_pushes() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, true));
        assert!(!p.motion(true, true));
        assert!(p.motion(true, true));
    }

    #[test]
    fn leaving_the_edge_resets_the_count() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        // The pointer left the edge (a push that did not land on it).
        assert!(!p.motion(false, false));
        assert!(!p.motion(true, false));
    }

    #[test]
    fn a_key_or_button_event_resets_the_count() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        p.reset();
        assert!(!p.motion(true, false), "the press in between started the count again");
        assert!(p.motion(true, false));
    }

    #[test]
    fn pressing_a_button_mid_run_raises_the_bar_to_three() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        // The button goes down: the run carries on, but now three in a row are needed.
        assert!(!p.motion(true, true));
        assert!(p.motion(true, true));
    }

    #[test]
    fn after_returning_the_count_starts_from_zero() {
        let mut p = PushBack::default();
        assert!(!p.motion(true, false));
        assert!(p.motion(true, false));
        assert!(!p.motion(true, false));
    }
}
