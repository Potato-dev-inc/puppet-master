//! "Scores on your code": which agent works best in THIS project, counted from the person's own
//! history on this computer. Read-only and local. Agents never see this page.
//!
//! - `model`: the types and the sample-size constants.
//! - `stats`: rates, the 90% Wilson interval, sample tiers and the `ahead` rule.
//! - `undo_log`: the one new write, an append-only log of Undo (nothing else recorded it).
//! - `join`: pairs turn summaries with operation records; builds the turns and runs.
//! - `aggregate`: the pure roll-up into one row per agent and account.
//! - `gather`: reads the live sources. `tests`: guard and end-to-end tests.
//!
//! Generic: agents are only ids, labelled from the registry (wizard agents included).
//! Private: no folder, pane id, prompt, reply, file name or approval text leaves this module.

// The scaffold's types are read by the later SC tasks (stats, join, aggregate, gather, page).
#![allow(dead_code)]

pub mod aggregate;
pub mod gather;
pub mod join;
pub mod model;
pub mod stats;
pub mod undo_log;
#[cfg(test)]
mod tests;
