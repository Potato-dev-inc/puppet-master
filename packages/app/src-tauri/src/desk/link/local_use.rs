//! Pure rules for the hub's edge decisions: when a person is using a controlled computer's own mouse, and when a
//! computer that handed the pointer back may cross again. No host, no link, no clock: the caller says what it sees,
//! so every rule is tested on every platform.

/// The pointer must be more than this many units from where this computer last put it (a jitter is less).
pub const JUMP_UNITS: i64 = 12;
/// Remote motion handled within this many ms means a move that does not match is the other computer's own motion.
pub const FRESH_MOTION_MS: u64 = 80;

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

#[cfg(test)]
mod tests {
    use super::*;

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
