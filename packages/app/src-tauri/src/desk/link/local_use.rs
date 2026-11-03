//! Pure rules for the hub's edge decisions: when a person is using a controlled computer's own mouse, and when a
//! computer that handed the pointer back may cross again. No host, no link, no clock: the caller says what it sees,
//! so every rule is tested on every platform.

use std::collections::HashSet;

use super::geometry::Bounds;
use crate::p2p::endpoint::NodeId;

/// The pointer must be more than this many units from where this computer last put it (a jitter is less).
pub const JUMP_UNITS: i64 = 12;
/// Remote motion handled within this many ms means a move that does not match is the other computer's own motion.
pub const FRESH_MOTION_MS: u64 = 80;
/// The pointer must be at least this far from every screen edge before a computer may cross to a peer again.
pub const ARM_MARGIN: i32 = 24;

/// True when a person is using this computer's own mouse while it is being controlled: the pointer moved far from
/// where the hub put it with no fresh remote motion to explain it, or a button is held here that the other computer
/// did not press (nothing injected is held).
pub fn used_here(expected: (i32, i32), actual: (i32, i32), since_motion_ms: u64, buttons_here: bool, injected_buttons: usize) -> bool {
    let (dx, dy) = (i64::from(actual.0) - i64::from(expected.0), i64::from(actual.1) - i64::from(expected.1));
    let far = dx * dx + dy * dy > JUMP_UNITS * JUMP_UNITS;
    let stale = since_motion_ms >= FRESH_MOTION_MS;
    let jumped = far && stale;
    let held_here = buttons_here && injected_buttons == 0;
    jumped || held_here
}

/// True when `pos` is at least `margin` units inside every edge of the screen union `u`.
pub fn away_from_edges(pos: (i32, i32), u: Bounds, margin: i32) -> bool {
    pos.0 - u.left >= margin && u.right - 1 - pos.0 >= margin && pos.1 - u.top >= margin && u.bottom - 1 - pos.1 >= margin
}

/// Which peers may be crossed to. A computer starts armed. Handing the pointer back to a peer, or being let go by it,
/// disarms that peer: the pointer may still rest on the edge that faces it, and must leave the edge first. Seeing the
/// pointer away from every edge arms every peer again.
#[derive(Default)]
pub struct Arming {
    disarmed: HashSet<NodeId>,
}

impl Arming {
    pub fn disarm(&mut self, peer: NodeId) {
        self.disarmed.insert(peer);
    }

    pub fn armed(&self, peer: &NodeId) -> bool {
        !self.disarmed.contains(peer)
    }

    /// The pointer was seen away from every edge.
    pub fn saw_away(&mut self) {
        self.disarmed.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Bounds = Bounds { left: 0, top: 0, right: 1920, bottom: 1080 };

    #[test]
    fn a_pointer_in_the_middle_is_away_from_the_edges() {
        assert!(away_from_edges((960, 540), SCREEN, ARM_MARGIN));
        assert!(away_from_edges((24, 24), SCREEN, ARM_MARGIN));
        assert!(away_from_edges((1895, 1055), SCREEN, ARM_MARGIN));
    }

    #[test]
    fn a_pointer_within_the_margin_of_any_edge_is_not_away() {
        assert!(!away_from_edges((23, 540), SCREEN, ARM_MARGIN), "23 units from the left edge");
        assert!(!away_from_edges((1919, 540), SCREEN, ARM_MARGIN), "on the right edge");
        assert!(!away_from_edges((960, 0), SCREEN, ARM_MARGIN), "on the top edge");
        assert!(!away_from_edges((960, 1056), SCREEN, ARM_MARGIN), "23 units from the bottom edge");
    }

    #[test]
    fn a_computer_starts_armed_and_a_hand_back_disarms_only_that_peer() {
        let (a, b) = (NodeId([1; 32]), NodeId([2; 32]));
        let mut arming = Arming::default();
        assert!(arming.armed(&a) && arming.armed(&b));
        arming.disarm(a);
        assert!(!arming.armed(&a));
        assert!(arming.armed(&b));
    }

    #[test]
    fn seeing_the_pointer_away_from_the_edges_arms_everyone_again() {
        let a = NodeId([1; 32]);
        let mut arming = Arming::default();
        arming.disarm(a);
        arming.saw_away();
        assert!(arming.armed(&a));
    }

    #[test]
    fn a_small_jitter_is_not_a_person_here() {
        assert!(!used_here((500, 500), (505, 505), 500, false, 0), "a jitter of 7 units, with no motion for a long time");
        assert!(!used_here((500, 500), (512, 500), 500, false, 0), "exactly 12 units is not more than 12");
    }

    #[test]
    fn a_jump_just_over_the_threshold_with_stale_motion_is_a_person_here() {
        assert!(used_here((500, 500), (513, 500), 500, false, 0));
        assert!(used_here((500, 500), (500, 487), 80, false, 0), "80 ms is no longer fresh");
    }

    #[test]
    fn a_jump_with_fresh_remote_motion_is_the_other_computer() {
        assert!(!used_here((500, 500), (900, 500), 10, false, 0));
        assert!(!used_here((500, 500), (900, 500), 79, false, 0), "79 ms is still fresh");
    }

    #[test]
    fn a_held_local_button_with_nothing_injected_is_a_person_here() {
        assert!(used_here((500, 500), (500, 500), 0, true, 0));
    }

    #[test]
    fn a_held_local_button_does_not_count_while_the_other_computer_holds_one() {
        assert!(!used_here((500, 500), (500, 500), 0, true, 1), "the other computer pressed it: the press is injected");
        assert!(!used_here((500, 500), (500, 500), 0, false, 0));
    }
}
