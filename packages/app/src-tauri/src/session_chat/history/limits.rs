//! Caps for reading an agent store. A hostile or huge store (zip bomb, runaway database) can never make the app
//! hang or fill the disk: each read stops at one of these numbers.

use std::time::Duration;

/// Caps for listing an agent's chats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListLimits {
    /// Only chats updated at or after this time (ms since the epoch). `None` = all.
    pub since_ms: Option<i64>,
    /// Chats kept, newest first.
    pub max_chats: usize,
    /// Files larger than this are counted as `too_big` and not read.
    pub max_file_bytes: u64,
    /// Folder entries looked at in total.
    pub max_entries: usize,
    /// Wall-clock time the walk may take.
    pub budget: Duration,
}

impl Default for ListLimits {
    fn default() -> Self {
        Self {
            since_ms: None,
            max_chats: 2000,
            max_file_bytes: 64 * 1024 * 1024,
            max_entries: 20_000,
            budget: Duration::from_secs(8),
        }
    }
}

/// Caps for reading one chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    /// Items returned per chat before the read stops (`cut_short`).
    pub max_items: usize,
    /// Decompressed bytes read per chat before the read stops.
    pub max_decompressed: u64,
    /// Items handed to the sink per call.
    pub chunk: usize,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self { max_items: 3000, max_decompressed: 256 * 1024 * 1024, chunk: 200 }
    }
}
