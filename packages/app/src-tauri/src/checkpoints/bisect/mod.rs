//! What broke it (SPEC n33): find the first agent turn after which the person's declared test command fails.
//!
//! It walks the save points of a folder (the end of each agent turn), runs the declared command in a
//! separate copy built from those save points, and narrows the range by halving. The result names the
//! turn, or says plainly why it cannot tell. Undo puts back only that turn's own lines, all or nothing.
//!
//! Invariants (SPEC n33 section 4). Every file in this folder keeps them:
//! - S1: tests run only in a copy under `<app data>/what-broke-it/copies/`, never in the person's folder.
//! - S2: only the desktop window starts a search or an undo. No bridge route, MCP tool, companion or phone path.
//! - S3: only the person's declared command runs, as argv, never a shell string.
//! - S5/S6: one job at a time; every run has a timeout and a Stop; only the PID this job started is killed.
//! - S7: a failure is run twice; a different answer is "not sure". Never blame a turn on a coin flip.
//! - S8: undo is all-or-nothing and refuses while an agent in the folder works; a safety save point comes first.
//! - S9/S10: output is redacted before it is stored; only the tail of one failing run is kept.
//! - S11: ids are checked (`store::valid_id`) before they touch a path. Copy folders get generated names.
//! - S13: generic code names no agent. Agent labels come from turn summaries.
//! - S14: nothing is typed into any pane.
//!
//! Pure parts (`model`, `points`, `blame`, `search`, `failnames`, `words`, `views`) do no I/O and are unit-tested.
//! I/O parts (`store`, `copy`, `exec`, `revert`, `live`, `job`) take their paths and clocks from their callers.
//! `live` is the window's entry point (`Live`), `job` is the search loop it runs on a thread.

pub mod blame;
pub mod copy;
pub mod exec;
pub mod failnames;
pub mod job;
pub mod live;
pub mod model;
pub mod points;
pub mod revert;
pub mod search;
pub mod store;
pub mod views;
pub mod words;

#[cfg(test)]
mod engine_tests;
