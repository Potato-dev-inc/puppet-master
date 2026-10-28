//! Which panes still have a turn summary being built.
//!
//! A run's pane is closed the moment the run completes (the default, `keep_pane = false`), but the
//! summary of its turn is built later on a worker: it waits for the rest point's checkpoint capture
//! and then reads the pane (files changed, run receipt). Closing the pane first loses those facts, and
//! used to lose the whole summary. The pane's disposal therefore waits, for a bounded time, until
//! this registry says no summary of that pane is in flight.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
pub struct Pending {
    counts: Mutex<HashMap<String, u32>>,
    changed: Condvar,
    /// Test only: how often a wait for a pane went to sleep on `changed` (see `blocked_waits`).
    #[cfg(test)]
    blocked: Mutex<HashMap<String, u32>>,
}

impl Pending {
    /// A summary of `pane_id` is about to be built (taken on the status-event thread, before the
    /// worker starts, so a disposal that comes right after cannot miss it).
    pub fn begin(&self, pane_id: &str) {
        *self.counts.lock().entry(pane_id.to_string()).or_insert(0) += 1;
    }

    /// That summary is done (built, skipped or failed).
    pub fn end(&self, pane_id: &str) {
        let mut counts = self.counts.lock();
        if let Some(count) = counts.get_mut(pane_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(pane_id);
            }
        }
        self.changed.notify_all();
    }

    pub fn in_flight(&self, pane_id: &str) -> bool {
        self.counts.lock().contains_key(pane_id)
    }

    /// Blocks until no summary of `pane_id` is in flight, at most `timeout`. Returns whether none is.
    pub fn wait(&self, pane_id: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut counts = self.counts.lock();
        while counts.contains_key(pane_id) {
            #[cfg(test)]
            self.note_blocked(pane_id);
            if self.changed.wait_until(&mut counts, deadline).timed_out() {
                return !counts.contains_key(pane_id);
            }
        }
        true
    }

    #[cfg(test)]
    fn note_blocked(&self, pane_id: &str) {
        *self.blocked.lock().entry(pane_id.to_string()).or_insert(0) += 1;
    }

    /// Test only: how many times a wait for `pane_id` went to sleep, that is, found a summary in flight.
    #[cfg(test)]
    pub fn blocked_waits(&self, pane_id: &str) -> u32 {
        self.blocked.lock().get(pane_id).copied().unwrap_or(0)
    }
}

/// Marks one summary of a pane as in flight until it is dropped.
pub struct InFlight {
    pending: &'static Pending,
    pane_id: String,
}

impl InFlight {
    pub fn new(pending: &'static Pending, pane_id: &str) -> Self {
        pending.begin(pane_id);
        Self { pending, pane_id: pane_id.to_string() }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.pending.end(&self.pane_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn wait_returns_at_once_when_nothing_is_in_flight_and_ends_with_the_last_summary() {
        let pending = Pending::default();
        assert!(pending.wait("p", Duration::from_millis(5)));
        pending.begin("p");
        pending.begin("p");
        pending.end("p");
        assert!(pending.in_flight("p"), "one of two is still being built");
        assert!(!pending.wait("p", Duration::from_millis(40)));
        pending.end("p");
        assert!(pending.wait("p", Duration::from_millis(5)));
        assert!(!pending.in_flight("other"));
    }

    #[test]
    fn only_a_wait_with_a_summary_in_flight_goes_to_sleep() {
        // The counter is what pane_close's test reads to show a dispose did not wait. This is the
        // positive control: a real wait is counted, an idle one is not.
        let pending = Arc::new(Pending::default());
        assert!(pending.wait("p", Duration::from_millis(1)));
        assert_eq!(pending.blocked_waits("p"), 0);
        pending.begin("p");
        let waiter = {
            let pending = pending.clone();
            std::thread::spawn(move || pending.wait("p", Duration::from_secs(30)))
        };
        // A condition, not a deadline: the 30 s only stops a broken test from hanging forever.
        let until = Instant::now() + Duration::from_secs(30);
        while pending.blocked_waits("p") == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(pending.blocked_waits("p") >= 1, "the waiter went to sleep");
        pending.end("p");
        assert!(waiter.join().unwrap());
        assert!(pending.wait("p", Duration::from_millis(1)));
    }

    #[test]
    fn a_waiter_is_released_when_the_summary_ends() {
        let pending = Arc::new(Pending::default());
        pending.begin("p");
        let waiter = {
            let pending = pending.clone();
            std::thread::spawn(move || pending.wait("p", Duration::from_secs(5)))
        };
        std::thread::sleep(Duration::from_millis(40));
        pending.end("p");
        assert!(waiter.join().unwrap());
    }
}
