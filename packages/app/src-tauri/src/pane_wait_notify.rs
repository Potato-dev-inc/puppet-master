//! Wake long-poll waiters when pane/task/model state changes.

use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use std::time::{Duration, Instant};

struct WaitNotify {
    generation: Mutex<u64>,
    condvar: Condvar,
}

static WAIT_NOTIFY: Lazy<WaitNotify> = Lazy::new(|| WaitNotify {
    generation: Mutex::new(0),
    condvar: Condvar::new(),
});

pub fn bump_waiters() {
    let mut generation = WAIT_NOTIFY.generation.lock();
    *generation = generation.saturating_add(1);
    WAIT_NOTIFY.condvar.notify_all();
}

/// Block until generation changes or `deadline`, whichever comes first.
pub fn wait_for_change(deadline: Instant, last_generation: &mut u64) {
    let mut generation = WAIT_NOTIFY.generation.lock();
    while *generation == *last_generation && Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        WAIT_NOTIFY.condvar.wait_for(&mut generation, remaining);
    }
    *last_generation = *generation;
}

#[allow(dead_code)]
pub fn wait_for_change_ms(timeout_ms: u64, last_generation: &mut u64) {
    wait_for_change(Instant::now() + Duration::from_millis(timeout_ms), last_generation);
}
