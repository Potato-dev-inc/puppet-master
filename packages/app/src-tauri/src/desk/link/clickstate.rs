// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The click count for one button-down, as a pure function: no OS calls, so the rule is tested on every platform.
//!
//! Presses of one button within 500 ms and 4 px of each other are one run: 1, 2, 3, and then the run starts again at 1
//! (a fourth press is a single click, not a quadruple). A press of another button, or one too late or too far away,
//! starts a new run.

/// Two presses of one button this close in time are a double click (inclusive, as in the old app).
pub const DOUBLE_CLICK_MS: u64 = 500;
/// ... and this close in pixels, per axis.
pub const DOUBLE_CLICK_SLOP_PX: i64 = 4;

/// What the previous button-down left behind: button, when (ms on the caller's clock), where, and its click count.
pub type LastClick = (u8, u64, (i32, i32), i64);

/// The click count for a button-down of `button` at `at` at `now_ms`, given the one before it (if any). Counts run
/// 1, 2, 3 and then wrap to 1.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn next_click_count(last: Option<LastClick>, button: u8, now_ms: u64, at: (i32, i32)) -> i64 {
    let Some((last_button, when, pos, count)) = last else {
        return 1;
    };
    let same_button = last_button == button;
    let in_time = now_ms.saturating_sub(when) <= DOUBLE_CLICK_MS;
    let in_place = (i64::from(pos.0) - i64::from(at.0)).abs() <= DOUBLE_CLICK_SLOP_PX
        && (i64::from(pos.1) - i64::from(at.1)).abs() <= DOUBLE_CLICK_SLOP_PX;
    if !(same_button && in_time && in_place) {
        return 1;
    }
    match count {
        1 => 2,
        2 => 3,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: (i32, i32) = (100, 100);

    #[test]
    fn the_first_press_is_a_single_click() {
        assert_eq!(next_click_count(None, 0, 1_000, HERE), 1);
    }

    #[test]
    fn presses_in_time_and_place_count_up_to_three_then_wrap_to_one() {
        let first = Some((0, 1_000, HERE, 1));
        assert_eq!(next_click_count(first, 0, 1_100, HERE), 2);
        let second = Some((0, 1_100, HERE, 2));
        assert_eq!(next_click_count(second, 0, 1_200, HERE), 3);
        let third = Some((0, 1_200, HERE, 3));
        assert_eq!(next_click_count(third, 0, 1_300, HERE), 1);
    }

    #[test]
    fn a_different_button_starts_a_new_run() {
        let left = Some((0, 1_000, HERE, 1));
        assert_eq!(next_click_count(left, 1, 1_100, HERE), 1);
        let right_double = Some((1, 1_000, HERE, 2));
        assert_eq!(next_click_count(right_double, 0, 1_100, HERE), 1);
    }

    #[test]
    fn a_press_too_slow_starts_a_new_run() {
        let first = Some((0, 1_000, HERE, 1));
        assert_eq!(next_click_count(first, 0, 1_500, HERE), 2, "exactly 500 ms still counts");
        assert_eq!(next_click_count(first, 0, 1_501, HERE), 1);
    }

    #[test]
    fn a_press_too_far_away_starts_a_new_run() {
        let first = Some((0, 1_000, HERE, 1));
        assert_eq!(next_click_count(first, 0, 1_100, (104, 96)), 2, "4 px on each axis still counts");
        assert_eq!(next_click_count(first, 0, 1_100, (105, 100)), 1);
        assert_eq!(next_click_count(first, 0, 1_100, (100, 95)), 1);
    }

    #[test]
    fn a_clock_that_went_back_counts_as_in_time() {
        let first = Some((0, 1_000, HERE, 1));
        assert_eq!(next_click_count(first, 0, 900, HERE), 2);
    }

    #[test]
    fn a_stored_count_out_of_range_starts_again_at_one() {
        assert_eq!(next_click_count(Some((0, 1_000, HERE, 0)), 0, 1_100, HERE), 1);
        assert_eq!(next_click_count(Some((0, 1_000, HERE, 7)), 0, 1_100, HERE), 1);
    }
}
