//! Panes the registry holds but does not show: warm standby workers (`crate::standby`) spawn
//! hidden, so no pane list (UI, bridge `/panes`, workspace state, run adoption) offers them until a
//! run claims one and reveals it.
//!
//! The set lives outside `PaneRegistry` so a pane can be marked hidden BEFORE it is spawned (the
//! spawn announces a panes change, and the first refresh must already leave it out).

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashSet;

static HIDDEN: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Mark `pane_id` hidden. Call before the pane is spawned.
pub fn hide(pane_id: &str) {
    HIDDEN.lock().insert(pane_id.to_string());
}

/// Show the pane in pane lists again. Returns whether it was hidden.
pub fn reveal(pane_id: &str) -> bool {
    HIDDEN.lock().remove(pane_id)
}

pub fn is_hidden(pane_id: &str) -> bool {
    HIDDEN.lock().contains(pane_id)
}

/// Forget a pane that left the registry.
pub(crate) fn forget(pane_id: &str) {
    HIDDEN.lock().remove(pane_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_and_reveal_round_trip() {
        hide("hidden-test-pane");
        assert!(is_hidden("hidden-test-pane"));
        assert!(reveal("hidden-test-pane"));
        assert!(!is_hidden("hidden-test-pane"));
        assert!(!reveal("hidden-test-pane"), "revealing twice reports false");
    }
}
