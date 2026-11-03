//! Turns one raw pointer event from the operating system into a `PointerMove`. Pure: no clock, no I/O, no OS calls, so
//! the rounding and clamping rules are tested on every platform (the macOS watch in `os/macos/pointer_watch.rs` is
//! only built on a Mac).

use super::remote::PointerMove;

/// One raw move: the location as the system reports it (fractional points) and the hardware delta (whole units, 64-bit
/// as the system hands them over). The location is rounded to the nearest whole point, half away from zero. The
/// delta is clamped to the `i32` range, so an absurd value can never wrap around into a small, plausible one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn to_move(loc_x: f64, loc_y: f64, dx: i64, dy: i64) -> PointerMove {
    PointerMove {
        // `as` saturates at the `i32` limits and maps NaN to 0, so a bad location cannot panic or wrap.
        x: loc_x.round() as i32,
        y: loc_y.round() as i32,
        dx: clamp_delta(dx),
        dy: clamp_delta(dy),
    }
}

fn clamp_delta(d: i64) -> i32 {
    d.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_location_and_small_delta_pass_through() {
        let m = to_move(640.0, 480.0, 3, -2);
        assert_eq!(m, PointerMove { x: 640, y: 480, dx: 3, dy: -2 });
    }

    #[test]
    fn location_rounds_to_nearest_point() {
        assert_eq!(to_move(10.4, 20.6, 0, 0), PointerMove { x: 10, y: 21, dx: 0, dy: 0 });
    }

    #[test]
    fn half_points_round_away_from_zero() {
        assert_eq!(to_move(0.5, 2.5, 0, 0).x, 1);
        assert_eq!(to_move(0.5, 2.5, 0, 0).y, 3);
        assert_eq!(to_move(-0.5, -2.5, 0, 0), PointerMove { x: -1, y: -3, dx: 0, dy: 0 });
    }

    #[test]
    fn negative_location_is_kept() {
        // A second screen to the left of the main one has negative x in the global space.
        assert_eq!(to_move(-1440.0, -12.0, -7, 7).x, -1440);
        assert_eq!(to_move(-1440.0, -12.0, -7, 7).y, -12);
    }

    #[test]
    fn huge_delta_clamps_to_i32_range() {
        let m = to_move(0.0, 0.0, i64::MAX, i64::MIN);
        assert_eq!(m.dx, i32::MAX);
        assert_eq!(m.dy, i32::MIN);
    }

    #[test]
    fn delta_just_outside_i32_clamps_not_wraps() {
        let m = to_move(0.0, 0.0, i64::from(i32::MAX) + 1, i64::from(i32::MIN) - 1);
        assert_eq!(m.dx, i32::MAX);
        assert_eq!(m.dy, i32::MIN);
    }

    #[test]
    fn delta_at_i32_limits_is_kept() {
        let m = to_move(0.0, 0.0, i64::from(i32::MAX), i64::from(i32::MIN));
        assert_eq!((m.dx, m.dy), (i32::MAX, i32::MIN));
    }

    #[test]
    fn nan_location_becomes_zero_and_huge_location_saturates() {
        assert_eq!(to_move(f64::NAN, 0.0, 0, 0).x, 0);
        assert_eq!(to_move(1e300, -1e300, 0, 0), PointerMove { x: i32::MAX, y: i32::MIN, dx: 0, dy: 0 });
    }

    #[test]
    fn zero_delta_is_zero_not_dropped() {
        // Dropping a zero-delta move is the watch's decision (it is still a position report), not this function's.
        assert_eq!(to_move(5.0, 5.0, 0, 0), PointerMove { x: 5, y: 5, dx: 0, dy: 0 });
    }
}
