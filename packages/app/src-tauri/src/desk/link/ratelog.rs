//! A log line at most once per `every_ms` for each reason. Pure: no clock, no log, the caller says what time it is, so
//! the rule is tested on every platform.
//!
//! Used for "desk: not crossing reason=..." while the pointer rests on an edge: the hub looks at the pointer every
//! 10 ms, so without a limit one stuck reason would write a hundred lines a second.

use std::collections::HashMap;

pub struct RateLimit {
    every_ms: u64,
    /// When each reason was last allowed through.
    last: HashMap<&'static str, u64>,
}

impl RateLimit {
    pub fn new(every_ms: u64) -> Self {
        Self { every_ms, last: HashMap::new() }
    }

    /// True when a line for `reason` may be written at `now_ms`; then the next one waits `every_ms`.
    pub fn allow(&mut self, reason: &'static str, now_ms: u64) -> bool {
        match self.last.get(reason) {
            Some(&t) if now_ms.saturating_sub(t) < self.every_ms => false,
            _ => {
                self.last.insert(reason, now_ms);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimit;

    #[test]
    fn one_line_per_second_per_reason() {
        let mut r = RateLimit::new(1_000);
        assert!(r.allow("paused", 0), "the first line goes through");
        assert!(!r.allow("paused", 10), "the same reason inside the second is held back");
        assert!(!r.allow("paused", 999));
        assert!(r.allow("paused", 1_000), "a second later the next line goes through");
        assert!(!r.allow("paused", 1_500));
    }

    #[test]
    fn each_reason_has_its_own_clock() {
        let mut r = RateLimit::new(1_000);
        assert!(r.allow("paused", 0));
        assert!(r.allow("button held", 5), "a different reason is not held back by the first");
        assert!(!r.allow("button held", 6));
        assert!(!r.allow("paused", 7));
    }

    #[test]
    fn an_earlier_time_than_the_last_line_is_held_back() {
        let mut r = RateLimit::new(1_000);
        assert!(r.allow("input off", 5_000));
        // An earlier time reads as no time passed: the line waits, and a second after the last one it is let through.
        assert!(!r.allow("input off", 100));
        assert!(r.allow("input off", 6_000));
    }
}
