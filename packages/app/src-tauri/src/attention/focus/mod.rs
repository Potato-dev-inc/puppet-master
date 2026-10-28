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
mod model_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod view_tests;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);

/// True only while a session is on and now is before its deadline. Cheap and lock-free; every hook calls it first.
pub fn is_on() -> bool {
    is_on_at(crate::attention::now_ms())
}

/// `is_on` for a given time (tests and the engine).
pub fn is_on_at(now_ms: u64) -> bool {
    ON.load(Ordering::SeqCst) && now_ms < DEADLINE_MS.load(Ordering::SeqCst)
}

/// Set by the engine when a session starts, changes or ends. `None` turns Focus off.
pub(crate) fn set_session(deadline_ms: Option<u64>) {
    match deadline_ms {
        Some(deadline) => {
            DEADLINE_MS.store(deadline, Ordering::SeqCst);
            ON.store(true, Ordering::SeqCst);
        }
        None => {
            ON.store(false, Ordering::SeqCst);
            DEADLINE_MS.store(0, Ordering::SeqCst);
        }
    }
}
