//! Stillness rule for experimental standbys (no agent-specific ready screen). A pane only counts as
//! ready when its screen text is identical at two observations at least `STILL_MS` apart, so a CLI
//! that is still drawing its banner or loading is never typed into. Pure state: the host passes the
//! time, so the rule is tested without a PTY or a clock.

/// How long the same screen text must hold before it is taken as settled.
pub const STILL_MS: i64 = 2_000;

/// The text last observed on one pane and when it first showed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stillness {
    text: Option<String>,
    since_ms: i64,
}

impl Stillness {
    /// Record one observation. `true` when this screen text was already on screen at an earlier
    /// observation at least `STILL_MS` ago and has not changed since. Any change restarts the clock.
    pub fn observe(&mut self, text: &str, now_ms: i64) -> bool {
        match &self.text {
            Some(previous) if previous == text => now_ms - self.since_ms >= STILL_MS,
            _ => {
                self.text = Some(text.to_string());
                self.since_ms = now_ms;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_sighting_is_never_still() {
        let mut s = Stillness::default();
        assert!(!s.observe("> ", 0));
    }

    #[test]
    fn the_same_text_two_seconds_later_is_still() {
        let mut s = Stillness::default();
        assert!(!s.observe("> ", 0));
        assert!(!s.observe("> ", 1_000), "one second is not enough");
        assert!(s.observe("> ", 2_000));
        assert!(s.observe("> ", 3_000), "stays still while unchanged");
    }

    #[test]
    fn any_change_restarts_the_clock() {
        let mut s = Stillness::default();
        s.observe("banner", 0);
        assert!(!s.observe("banner\nloading", 1_500));
        assert!(!s.observe("banner\nloading", 3_000), "measured from the change, 1.5s ago");
        assert!(s.observe("banner\nloading", 3_500));
    }

    #[test]
    fn a_changed_screen_that_returns_to_an_old_text_starts_over() {
        let mut s = Stillness::default();
        s.observe("A", 0);
        s.observe("B", 1_000);
        assert!(!s.observe("A", 2_500), "A was replaced, so this is a new sighting");
        assert!(s.observe("A", 4_500));
    }
}
