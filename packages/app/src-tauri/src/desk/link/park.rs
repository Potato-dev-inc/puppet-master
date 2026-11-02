// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Where the pointer is parked on this computer while it is on another one.
//!
//! The park point is the exit point (where the pointer left this computer), moved inward just far enough that a box
//! `2 * span` wide and tall around it fits inside the monitor. The pointer then stays near the edge it left by, not in
//! the middle of the screen. A monitor smaller than the box is parked at its own centre on that axis.

/// How far inside the monitor edge the park point sits at least (the reach the old cursor clip had).
pub const PARK_SPAN: i32 = 160;

/// The park point for a pointer that left at `exit`, on a monitor given as `(left, top, width, height)`.
///
/// Each axis is clamped to `[origin + span, origin + size - span]`. An axis shorter than `2 * span` uses the monitor's
/// centre on that axis. An `exit` outside the monitor is clamped like any other.
pub fn park_point(exit: (i32, i32), monitor: (i32, i32, i32, i32), span: i32) -> (i32, i32) {
    let (left, top, width, height) = monitor;
    let x = if width < 2 * span { left + width / 2 } else { exit.0.clamp(left + span, left + width - span) };
    let y = if height < 2 * span { top + height / 2 } else { exit.1.clamp(top + span, top + height - span) };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_HD: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

    #[test]
    fn each_corner_parks_span_inside_the_corner() {
        assert_eq!(park_point((0, 0), FULL_HD, PARK_SPAN), (160, 160));
        assert_eq!(park_point((1919, 0), FULL_HD, PARK_SPAN), (1760, 160));
        assert_eq!(park_point((0, 1079), FULL_HD, PARK_SPAN), (160, 920));
        assert_eq!(park_point((1919, 1079), FULL_HD, PARK_SPAN), (1760, 920));
    }

    #[test]
    fn a_point_in_the_middle_stays_where_it_is() {
        assert_eq!(park_point((960, 540), FULL_HD, PARK_SPAN), (960, 540));
    }

    #[test]
    fn an_exit_on_a_side_keeps_its_other_axis() {
        assert_eq!(park_point((1919, 400), FULL_HD, PARK_SPAN), (1760, 400));
        assert_eq!(park_point((400, 1079), FULL_HD, PARK_SPAN), (400, 920));
    }

    #[test]
    fn a_monitor_smaller_than_the_box_parks_at_its_centre_on_that_axis() {
        // Both axes too small: 300 < 320 and 200 < 320.
        assert_eq!(park_point((0, 0), (0, 0, 300, 200), PARK_SPAN), (150, 100));
        // Only the height is too small; the width is still clamped.
        assert_eq!(park_point((900, 0), (0, 0, 1000, 200), PARK_SPAN), (840, 100));
    }

    #[test]
    fn a_monitor_exactly_the_size_of_the_box_parks_at_its_centre() {
        assert_eq!(park_point((0, 0), (0, 0, 320, 320), PARK_SPAN), (160, 160));
        assert_eq!(park_point((319, 319), (0, 0, 320, 320), PARK_SPAN), (160, 160));
    }

    #[test]
    fn negative_origin_monitors_clamp_inside_their_own_box() {
        // A monitor to the left of and above the primary: spans x -1920..0, y -200..880.
        let monitor = (-1920, -200, 1920, 1080);
        assert_eq!(park_point((-1920, -200), monitor, PARK_SPAN), (-1760, -40));
        assert_eq!(park_point((-1, 879), monitor, PARK_SPAN), (-160, 720));
        assert_eq!(park_point((-960, 340), monitor, PARK_SPAN), (-960, 340));
    }

    #[test]
    fn a_small_monitor_with_negative_origin_parks_at_its_centre() {
        assert_eq!(park_point((-100, -50), (-100, -50, 200, 100), PARK_SPAN), (0, 0));
    }

    #[test]
    fn an_exit_outside_the_monitor_is_clamped_into_it() {
        assert_eq!(park_point((5000, -3000), FULL_HD, PARK_SPAN), (1760, 160));
        assert_eq!(park_point((-50, 540), FULL_HD, PARK_SPAN), (160, 540));
    }
}
