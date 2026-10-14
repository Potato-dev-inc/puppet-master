//! When each pane was last resized, so a redraw our own resize caused is not read as work.
//!
//! Selecting a pane remounts its terminal, which resizes the PTY, and the agent reprints its screen
//! (see `TuiProfile::reprints_on_resize`). That output changes visible cells, so the classifier reads
//! `running` and an idle pane flashed "Working 0s" for seconds (live, Codex, VERIFY #9). A profile that
//! can say "no work is showing" (`busy_marker`) already settles this; this clock is the generic guard
//! for the profiles that cannot: for a short window after a resize an idle pane only flips to running
//! when the profile's busy evidence appears (`pty::status::settle_status_after_resize`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Long enough for a TUI to finish reprinting after a resize, short enough that a task typed right
/// after is not held back (the profile's busy marker, when it has one, always wins).
pub const SETTLE_WINDOW: Duration = Duration::from_millis(2_000);

static RESIZED: Lazy<Mutex<HashMap<String, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// `pane_id`'s PTY was just resized.
pub fn note_resize(pane_id: &str) {
    RESIZED.lock().insert(pane_id.to_string(), Instant::now());
}

/// Whether `pane_id` was resized within [`SETTLE_WINDOW`].
pub fn recently_resized(pane_id: &str) -> bool {
    recently_resized_at(pane_id, Instant::now())
}

fn recently_resized_at(pane_id: &str, now: Instant) -> bool {
    RESIZED
        .lock()
        .get(pane_id)
        .is_some_and(|at| now.saturating_duration_since(*at) < SETTLE_WINDOW)
}

/// The pane is gone.
pub fn forget(pane_id: &str) {
    RESIZED.lock().remove(pane_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resize_counts_for_the_settle_window_only() {
        let pane = "resize-clock-test";
        assert!(!recently_resized(pane), "never resized");
        note_resize(pane);
        assert!(recently_resized(pane));
        let later = Instant::now() + SETTLE_WINDOW + Duration::from_millis(5);
        assert!(!recently_resized_at(pane, later), "the window closes");
        forget(pane);
        assert!(!recently_resized(pane));
    }
}
