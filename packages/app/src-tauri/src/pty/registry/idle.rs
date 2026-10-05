//! When a pane flips from `running` to `idle`.
//!
//! A pane is idle once its SCREEN has held still for the profile's `idle_after`. Raw output does
//! not count: Cursor keeps writing cursor/attribute sequences that never change a visible cell
//! (measured: bytes for 19s after its input box was ready, with no screen change), so "no output
//! for N seconds" kept a finished pane `running` for tens of seconds.

use parking_lot::Mutex;
use std::time::{Duration, Instant};

pub(super) struct IdleClock {
    last_change: Mutex<Instant>,
}

impl IdleClock {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            last_change: Mutex::new(now),
        }
    }

    /// The visible screen changed at `at`.
    pub(super) fn screen_changed(&self, at: Instant) {
        *self.last_change.lock() = at;
    }

    /// True when the screen has not changed for at least `idle_after` as of `now`.
    pub(super) fn idle_due(&self, now: Instant, idle_after: Duration) -> bool {
        now.saturating_duration_since(*self.last_change.lock()) >= idle_after
    }
}

/// The window to apply: the profile's `quiet` one while it says the screen shows no sign of work,
/// otherwise the conservative `base` one.
pub(super) fn idle_window(busy_marker: Option<bool>, base: Duration, quiet: Duration) -> Duration {
    match busy_marker {
        Some(false) => quiet.min(base),
        _ => base,
    }
}

/// How often the idle watcher looks. Bounds how late an idle flip can be on top of `idle_after`.
pub(super) const IDLE_POLL: Duration = Duration::from_millis(100);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_only_after_the_screen_held_still_for_the_whole_window() {
        let start = Instant::now();
        let clock = IdleClock::new(start);
        let window = Duration::from_millis(1500);
        assert!(!clock.idle_due(start + Duration::from_millis(1499), window));
        assert!(clock.idle_due(start + Duration::from_millis(1500), window));
    }

    #[test]
    fn a_screen_change_restarts_the_window() {
        let start = Instant::now();
        let clock = IdleClock::new(start);
        let window = Duration::from_millis(1500);
        clock.screen_changed(start + Duration::from_millis(1000));
        assert!(!clock.idle_due(start + Duration::from_millis(2400), window));
        assert!(clock.idle_due(start + Duration::from_millis(2500), window));
    }

    #[test]
    fn the_quiet_window_applies_only_when_the_profile_says_no_work_is_showing() {
        let (base, quiet) = (Duration::from_secs(2), Duration::from_millis(700));
        assert_eq!(idle_window(Some(false), base, quiet), quiet);
        assert_eq!(idle_window(Some(true), base, quiet), base, "a spinner or stop hint keeps the long window");
        assert_eq!(idle_window(None, base, quiet), base, "unknown is conservative");
        // A profile can never make the quiet window longer than the base one.
        assert_eq!(idle_window(Some(false), quiet, base), quiet);
    }

    #[test]
    fn only_screen_changes_postpone_idleness() {
        // The reader calls `screen_changed` solely for a different snapshot, so invisible output
        // (what Cursor emits for tens of seconds) never reaches the clock.
        let start = Instant::now();
        let clock = IdleClock::new(start);
        assert!(clock.idle_due(start + Duration::from_secs(5), Duration::from_secs(5)));
    }
}
