//! Lightweight stage timing for the agent-run start path.
//!
//! `mark(key, stage)` logs `startup_stage name=<stage> elapsed_ms=<since begin>`
//! so slow starts can be attributed to a stage. `begin` resets the clock for a key
//! (pane id); marks for an unknown key start their own clock.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

struct Track {
    started: Instant,
    stages: Vec<(String, u64)>,
}

static TRACKS: Lazy<Mutex<HashMap<String, Track>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const MAX_TRACKS: usize = 64;

#[allow(dead_code)]
pub fn begin(key: &str) {
    let mut tracks = TRACKS.lock();
    if tracks.len() >= MAX_TRACKS {
        tracks.clear();
    }
    tracks.insert(
        key.to_string(),
        Track {
            started: Instant::now(),
            stages: Vec::new(),
        },
    );
}

/// Record a stage; returns elapsed ms since the key's first mark/begin.
pub fn mark(key: &str, stage: &str) -> u64 {
    let mut tracks = TRACKS.lock();
    if tracks.len() >= MAX_TRACKS && !tracks.contains_key(key) {
        tracks.clear();
    }
    let track = tracks.entry(key.to_string()).or_insert_with(|| Track {
        started: Instant::now(),
        stages: Vec::new(),
    });
    let elapsed = track.started.elapsed().as_millis() as u64;
    track.stages.push((stage.to_string(), elapsed));
    tracing::info!(pane = %key, "startup_stage name={stage} elapsed_ms={elapsed}");
    elapsed
}

/// Stages recorded so far, in order.
#[allow(dead_code)]
pub fn stages(key: &str) -> Vec<(String, u64)> {
    TRACKS
        .lock()
        .get(key)
        .map(|t| t.stages.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_ordered_and_monotonic() {
        begin("t-order");
        mark("t-order", "a");
        std::thread::sleep(std::time::Duration::from_millis(5));
        mark("t-order", "b");
        let s = stages("t-order");
        assert_eq!(s[0].0, "a");
        assert_eq!(s[1].0, "b");
        assert!(s[1].1 >= s[0].1 + 4);
    }
}
