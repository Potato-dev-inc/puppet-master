//! Lets a turn summary wait for the checkpoint "after" capture of the same rest point.
//!
//! The summary of a turn that just came to rest reads the checkpoints to count the files the turn
//! changed, so it must not be built before the "after" snapshot of THAT rest point exists. Each
//! capture request takes a per-pane ticket (taken on the status-event thread, before the capture
//! worker starts); the worker settles the ticket when the capture finished or was skipped; the
//! summary waits for its own ticket. An earlier rest point's capture finishing (an approval prompt
//! mid-turn) never releases a later waiter: live, the summary of an approval-gated edit was built
//! from the approval prompt's capture, before the end-of-turn snapshot, and read "0 files" while
//! History said "1 file".

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
struct Tickets {
    requested: u64,
    done: u64,
}

#[derive(Default)]
pub struct Settle {
    panes: Mutex<HashMap<String, Tickets>>,
    changed: Condvar,
}

impl Settle {
    /// A capture of `pane_id` is about to start. Returns its ticket.
    pub fn request(&self, pane_id: &str) -> u64 {
        let mut panes = self.panes.lock();
        let tickets = panes.entry(pane_id.to_string()).or_default();
        tickets.requested += 1;
        tickets.requested
    }

    /// The newest ticket handed out for `pane_id` (0: no capture was ever requested).
    pub fn latest(&self, pane_id: &str) -> u64 {
        self.panes.lock().get(pane_id).map_or(0, |t| t.requested)
    }

    /// The capture behind `ticket` is done (or was skipped).
    pub fn settle(&self, pane_id: &str, ticket: u64) {
        let mut panes = self.panes.lock();
        let tickets = panes.entry(pane_id.to_string()).or_default();
        tickets.done = tickets.done.max(ticket);
        self.changed.notify_all();
    }

    /// Blocks until `ticket` (or a later one) is settled, at most `timeout`. Returns whether it was.
    pub fn wait(&self, pane_id: &str, ticket: u64, timeout: Duration) -> bool {
        if ticket == 0 {
            return true;
        }
        let deadline = Instant::now() + timeout;
        let mut panes = self.panes.lock();
        while panes.get(pane_id).map_or(0, |t| t.done) < ticket {
            if self.changed.wait_until(&mut panes, deadline).timed_out() {
                return panes.get(pane_id).map_or(0, |t| t.done) >= ticket;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn an_earlier_rest_points_capture_does_not_release_a_later_waiter() {
        let settle = Arc::new(Settle::default());
        let at_approval = settle.request("p");
        let at_idle = settle.request("p");
        assert_eq!((at_approval, at_idle), (1, 2));
        // The approval prompt's capture finishes first; the end-of-turn capture is still running.
        settle.settle("p", at_approval);
        assert!(!settle.wait("p", at_idle, Duration::from_millis(60)), "still waiting for its own capture");
        let waiter = {
            let settle = settle.clone();
            std::thread::spawn(move || settle.wait("p", at_idle, Duration::from_secs(5)))
        };
        std::thread::sleep(Duration::from_millis(40));
        settle.settle("p", at_idle);
        assert!(waiter.join().unwrap(), "released by its own ticket");
    }

    #[test]
    fn a_later_settle_covers_an_earlier_ticket_and_no_ticket_never_waits() {
        let settle = Settle::default();
        let first = settle.request("p");
        let second = settle.request("p");
        settle.settle("p", second);
        assert!(settle.wait("p", first, Duration::from_millis(10)));
        assert!(settle.wait("other", settle.latest("other"), Duration::from_millis(10)), "no capture requested: nothing to wait for");
        assert_eq!(settle.latest("p"), 2);
    }

    #[test]
    fn a_capture_that_never_settles_times_out() {
        let settle = Settle::default();
        let ticket = settle.request("p");
        let started = Instant::now();
        assert!(!settle.wait("p", ticket, Duration::from_millis(80)));
        assert!(started.elapsed() >= Duration::from_millis(70));
    }
}
