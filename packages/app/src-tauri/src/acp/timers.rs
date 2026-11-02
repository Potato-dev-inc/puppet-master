//! The clocks of one turn, as a pure function of time so they can be tested without waiting.
//!
//! Two windows, kept apart on purpose (Nuphos' lesson: a heartbeat proves the process is alive, not that
//! the agent is getting anywhere):
//!
//! * **inactivity**: nothing at all heard from the agent. Catches a dead or frozen adapter.
//! * **progress**: bytes keep arriving but nothing that is real work (see `state::is_progress`) for a
//!   long time. Catches an agent that chats about nothing, or a tool call that never ends.
//!
//! Time spent waiting for a person to answer a permission request is excluded: both windows restart when
//! the last question is answered. When a window runs out the agent is first asked to stop
//! (`session/cancel`); only if it does not end the turn within the grace period is it killed.

use std::time::{Duration, Instant};

use super::config::Timeouts;

#[derive(Debug, Clone, Copy)]
pub struct TurnClock {
    pub started: Instant,
    /// Last time any frame that counts arrived.
    pub last_frame: Instant,
    /// Last time real work arrived.
    pub last_progress: Instant,
    /// A permission request is open: nothing can expire.
    pub paused: bool,
    /// When we asked the agent to stop (by the person or because a window ran out).
    pub stop_asked_at: Option<Instant>,
    /// The person was asked what to do about the quiet agent and has not answered yet.
    pub quiet_asked: bool,
}

impl TurnClock {
    pub fn new(now: Instant) -> Self {
        Self { started: now, last_frame: now, last_progress: now, paused: false, stop_asked_at: None, quiet_asked: false }
    }

    pub fn frame(&mut self, now: Instant, progress: bool) {
        self.last_frame = now;
        self.quiet_asked = false;
        if progress {
            self.last_progress = now;
        }
    }

    /// The last open question was answered: the agent is working again, with fresh windows.
    pub fn resume(&mut self, now: Instant) {
        self.paused = false;
        self.quiet_asked = false;
        self.last_frame = now;
        self.last_progress = now;
    }

    /// "Keep waiting": the quiet window starts again. The progress window does not, so a turn that never does
    /// real work is still stopped in the end.
    pub fn keep_waiting(&mut self, now: Instant) {
        self.quiet_asked = false;
        self.last_frame = now;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Healthy,
    /// Waiting for a person: nothing expires.
    Paused,
    /// Nothing heard for the inactivity window.
    Quiet,
    /// Heard, but no real work for the progress window.
    Stalled,
    /// A stop was asked for and the grace period has not run out yet.
    Stopping,
    /// A stop was asked for and the agent did not end the turn in time: kill it.
    GraceOver,
}

pub fn verdict(clock: &TurnClock, policy: &Timeouts, now: Instant) -> Verdict {
    if let Some(asked) = clock.stop_asked_at {
        return if now.saturating_duration_since(asked) >= policy.hung_grace() { Verdict::GraceOver } else { Verdict::Stopping };
    }
    if clock.paused {
        return Verdict::Paused;
    }
    if now.saturating_duration_since(clock.last_frame) >= policy.inactivity() {
        return Verdict::Quiet;
    }
    if now.saturating_duration_since(clock.last_progress) >= policy.progress() {
        return Verdict::Stalled;
    }
    Verdict::Healthy
}

/// Has a session that is not in a turn been idle long enough to stop its adapter?
pub fn idle_expired(idle_since: Instant, policy: &Timeouts, now: Instant) -> bool {
    now.saturating_duration_since(idle_since) >= policy.idle_ttl()
}

#[allow(dead_code)]
pub(crate) fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Timeouts {
        Timeouts { inactivity_secs: 100, progress_secs: 1000, hung_grace_secs: 30, idle_ttl_secs: 600, ..Timeouts::default() }
    }

    #[test]
    fn a_turn_that_keeps_working_stays_healthy() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        for step in 1..=50u64 {
            let now = t0 + secs(step * 60);
            clock.frame(now, true);
            assert_eq!(verdict(&clock, &policy(), now), Verdict::Healthy);
        }
    }

    #[test]
    fn silence_trips_the_inactivity_window_not_the_progress_window() {
        let t0 = Instant::now();
        let clock = TurnClock::new(t0);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(99)), Verdict::Healthy);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(100)), Verdict::Quiet);
    }

    #[test]
    fn heartbeats_are_liveness_not_progress() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        // A bookkeeping frame every 50 s keeps the inactivity window happy...
        for step in 1..=19u64 {
            clock.frame(t0 + secs(step * 50), false);
        }
        assert_eq!(verdict(&clock, &policy(), t0 + secs(960)), Verdict::Healthy);
        // ...but the progress window still runs out.
        clock.frame(t0 + secs(1000), false);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(1000)), Verdict::Stalled);
    }

    #[test]
    fn waiting_for_a_person_never_expires_and_restarts_the_windows_after() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        clock.frame(t0 + secs(10), true);
        clock.paused = true;
        // A day later, still paused, still fine.
        assert_eq!(verdict(&clock, &policy(), t0 + secs(86_400)), Verdict::Paused);
        clock.resume(t0 + secs(86_400));
        assert_eq!(verdict(&clock, &policy(), t0 + secs(86_400 + 99)), Verdict::Healthy);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(86_400 + 100)), Verdict::Quiet);
    }

    #[test]
    fn keep_waiting_restarts_the_quiet_window_but_not_the_progress_window() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(100)), Verdict::Quiet);
        clock.quiet_asked = true;
        clock.keep_waiting(t0 + secs(100));
        assert!(!clock.quiet_asked);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(199)), Verdict::Healthy);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(200)), Verdict::Quiet);
        // Waiting for ever is not an option: with no real work the progress window still runs out.
        clock.keep_waiting(t0 + secs(990));
        assert_eq!(verdict(&clock, &policy(), t0 + secs(1000)), Verdict::Stalled);
    }

    #[test]
    fn a_frame_after_the_question_ends_the_question() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        clock.quiet_asked = true;
        clock.frame(t0 + secs(101), false);
        assert!(!clock.quiet_asked);
    }

    #[test]
    fn after_a_stop_request_the_agent_gets_its_grace_then_is_killed() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        clock.stop_asked_at = Some(t0 + secs(200));
        assert_eq!(verdict(&clock, &policy(), t0 + secs(210)), Verdict::Stopping);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(229)), Verdict::Stopping);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(230)), Verdict::GraceOver);
    }

    #[test]
    fn a_stop_request_beats_a_pause_so_a_cancelled_question_cannot_hang_forever() {
        let t0 = Instant::now();
        let mut clock = TurnClock::new(t0);
        clock.paused = true;
        clock.stop_asked_at = Some(t0);
        assert_eq!(verdict(&clock, &policy(), t0 + secs(31)), Verdict::GraceOver);
    }

    #[test]
    fn idle_expiry_uses_its_own_ttl() {
        let t0 = Instant::now();
        assert!(!idle_expired(t0, &policy(), t0 + secs(599)));
        assert!(idle_expired(t0, &policy(), t0 + secs(600)));
    }
}
