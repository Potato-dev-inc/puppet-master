//! Pointer speed for one paired computer: how far its pointer moves for each step of the mouse here.
//!
//! A Windows pixel and a Mac point are not the same length, so the same hand movement lands about 1.5 to 2 times
//! further on a Mac. The person sets a speed per computer (`PeerPrefs::pointer_speed`, 1.0 = as is). The computer
//! holding the mouse scales each motion step before it is sent (`hub.rs`, `Capture::Motion`).
//!
//! Steps are whole pixels, so a slow speed would lose motion if each step were rounded alone (half of one pixel is
//! nothing). The part of a step that does not fit is carried into the next one, so the total moved stays right.
//! Pure: no clock, no I/O.

/// The slowest pointer the page and the app accept.
pub const POINTER_SPEED_MIN: f64 = 0.25;
/// The fastest pointer the page and the app accept.
pub const POINTER_SPEED_MAX: f64 = 3.0;

/// Whether a saved or typed pointer speed can be used: a finite number from `MIN` to `MAX`, both included.
pub fn is_valid(speed: f64) -> bool {
    speed.is_finite() && (POINTER_SPEED_MIN..=POINTER_SPEED_MAX).contains(&speed)
}

/// The fraction of a pixel carried from one motion step to the next, for one computer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerSpeed {
    carry_x: f64,
    carry_y: f64,
}

impl PointerSpeed {
    /// Scales one motion step `(dx, dy)` by `speed` and returns the whole pixels to send. The rest stays here, in
    /// `[-0.5, 0.5]`, and joins the next step. Rounding is symmetric, so a step and its opposite scale to opposite
    /// results. At speed 1.0 the step is returned as it is, and nothing is carried. A speed that cannot be used is
    /// treated as 1.0.
    pub fn scale(&mut self, speed: f64, dx: i32, dy: i32) -> (i32, i32) {
        if speed == 1.0 || !is_valid(speed) {
            self.carry_x = 0.0;
            self.carry_y = 0.0;
            return (dx, dy);
        }
        let x = self.carry_x + f64::from(dx) * speed;
        let y = self.carry_y + f64::from(dy) * speed;
        let (ox, oy) = (x.round(), y.round());
        self.carry_x = x - ox;
        self.carry_y = y - oy;
        (ox as i32, oy as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_one_is_the_identity_for_any_step() {
        let mut p = PointerSpeed::default();
        for (dx, dy) in [(0, 0), (1, -1), (-37, 512), (i32::MAX / 2, -(i32::MAX / 2)), (3, 0)] {
            assert_eq!(p.scale(1.0, dx, dy), (dx, dy));
        }
    }

    #[test]
    fn half_speed_over_many_small_steps_loses_no_motion() {
        let mut p = PointerSpeed::default();
        let total: i32 = (0..1000).map(|_| p.scale(0.5, 1, 0).0).sum();
        assert_eq!(total, 500, "1000 pixels at half speed is 500, not 0");
        // Three pixels a step is 1.5 a step: carried, so 200 steps give exactly 300.
        let mut q = PointerSpeed::default();
        let total: i32 = (0..200).map(|_| q.scale(0.5, 3, 0).0).sum();
        assert_eq!(total, 300);
        let mut r = PointerSpeed::default();
        let total_y: i32 = (0..1000).map(|_| r.scale(0.5, 0, -1).1).sum();
        assert_eq!(total_y, -500);
    }

    #[test]
    fn a_slow_speed_keeps_the_total_and_the_carry_stays_small() {
        let mut p = PointerSpeed::default();
        let total: i32 = (0..10).map(|_| p.scale(0.3, 1, 0).0).sum();
        assert!((total - 3).abs() <= 1, "10 pixels at 0.3 is about 3, got {total}");
        for _ in 0..100 {
            p.scale(0.3, 1, 0);
            assert!(p.carry_x.abs() <= 0.5 + 1e-9, "{}", p.carry_x);
        }
    }

    #[test]
    fn double_speed_doubles_each_step() {
        let mut p = PointerSpeed::default();
        assert_eq!(p.scale(2.0, 5, -3), (10, -6));
        assert_eq!(p.scale(2.0, 1, 0), (2, 0));
        let total: i32 = (0..50).map(|_| p.scale(2.0, 7, 0).0).sum();
        assert_eq!(total, 700);
    }

    #[test]
    fn negative_steps_scale_symmetrically() {
        let steps = [1, 2, 3, 1, 1, 4, 2, 1, 5, 1];
        let mut plus = PointerSpeed::default();
        let mut minus = PointerSpeed::default();
        for &s in steps.iter().cycle().take(200) {
            let a = plus.scale(0.5, s, s);
            let b = minus.scale(0.5, -s, -s);
            assert_eq!(b, (-a.0, -a.1), "step {s}");
        }
    }

    #[test]
    fn the_remainder_is_carried_into_the_next_step() {
        let mut p = PointerSpeed::default();
        // 0.3 of a pixel is nothing alone; two steps make 0.6, which rounds up to one pixel and carries -0.4.
        assert_eq!(p.scale(0.3, 1, 0), (0, 0));
        assert_eq!(p.scale(0.3, 1, 0), (1, 0));
        assert_eq!(p.scale(0.3, 1, 0), (0, 0), "-0.4 + 0.3 is -0.1, nothing yet");
    }

    #[test]
    fn the_range_is_a_quarter_to_three_and_nothing_else() {
        assert!(is_valid(0.25) && is_valid(1.0) && is_valid(3.0));
        for bad in [0.0, 0.24, 3.01, -1.0, f64::NAN, f64::INFINITY] {
            assert!(!is_valid(bad), "{bad}");
        }
    }
}
