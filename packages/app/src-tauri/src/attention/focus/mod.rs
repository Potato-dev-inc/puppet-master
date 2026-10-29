//! Focus (`attention::focus`): a person turns on a short quiet window (25, 45 or 90 minutes). While it is on, pings
//! that can wait are held, and cards that block an agent reach the phone and Telegram quietly. Agents keep working:
//! Focus never pauses, stops, types into, answers or changes the budget of any agent.
//!
//! Invariants (SPEC n33 section 4):
//! - Only a person starts or changes Focus (desktop, tray, paired phone). A plain local program may only end it.
//! - Blocking cards are never hidden and never held. The Inbox, the tray badge and the count are unchanged.
//! - Fails open. `is_on` compares the stored deadline with the clock on every call, so a dead tick thread, a damaged
//!   state file or a clock problem cannot keep pings held past the end.
//! - Nothing here writes to a pane. This folder only reads the attention list and the turn summaries.
//! - Nothing here names an agent. Tiers use attention and push kinds only.
//! - Held text is redacted and counted. The phone sees counts only.
//!
//! Layout: `model` (types and limits), `store` (the state file), `policy` (the three tiers), `scan` (new blocking
//! cards), `nudge` (the one 20 minute reminder), `summary` (the end summary), `view` (what desktop and phone read),
//! `live` (the engine and the real clock, sinks and events). The fast flag is here.

pub mod live;
pub mod model;
pub mod nudge;
pub mod policy;
pub mod scan;
pub mod store;
pub mod summary;
pub mod view;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
pub(crate) mod fake_io;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod view_tests;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The fast flag: on, and the deadline. The app has one (`GLOBAL`); an engine under test brings its own, so
/// parallel tests never share it.
pub struct Flag {
    on: AtomicBool,
    deadline_ms: AtomicU64,
}

impl Flag {
    pub const fn new() -> Self {
        Self { on: AtomicBool::new(false), deadline_ms: AtomicU64::new(0) }
    }

    /// True only while on and `now_ms` is before the deadline. Needs no tick: a dead tick thread cannot keep
    /// pings held past the end.
    pub fn is_on_at(&self, now_ms: u64) -> bool {
        self.on.load(Ordering::SeqCst) && now_ms < self.deadline_ms.load(Ordering::SeqCst)
    }

    /// `None` turns it off.
    pub fn set(&self, deadline_ms: Option<u64>) {
        match deadline_ms {
            Some(deadline) => {
                self.deadline_ms.store(deadline, Ordering::SeqCst);
                self.on.store(true, Ordering::SeqCst);
            }
            None => {
                self.on.store(false, Ordering::SeqCst);
                self.deadline_ms.store(0, Ordering::SeqCst);
            }
        }
    }
}

impl Default for Flag {
    fn default() -> Self {
        Self::new()
    }
}

/// The app's flag, set by the live engine.
pub(crate) static GLOBAL: Flag = Flag::new();

/// True only while a session is on and now is before its deadline. Cheap and lock-free; every hook calls it first.
pub fn is_on() -> bool {
    is_on_at(crate::attention::now_ms())
}

/// `is_on` for a given time (tests and the engine).
pub fn is_on_at(now_ms: u64) -> bool {
    GLOBAL.is_on_at(now_ms)
}

/// Set by the engine when a session starts, changes or ends. `None` turns Focus off.
pub(crate) fn set_session(deadline_ms: Option<u64>) {
    GLOBAL.set(deadline_ms);
}
