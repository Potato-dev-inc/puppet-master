//! Running agent turns: `run_agent` (the entry), the headless child process, the per-run pane
//! supervisor, and settling a finished turn.
//!
//! - `entry`: request validation, worker choice, operation creation and dispatch.
//! - `bind`: adopting an existing worker or live pane into a request.
//! - `view`: presenting snapshots, bounded waits, and compact prompt shapes.
//! - `process`: the headless child (spawn, output readers, wait loop, terminate, cancel).
//! - `finish`: settling a headless turn from its recorded output.
//! - `supervise`: the supervisor thread that watches visible panes and enforces deadlines.
//! - `pane_turn`: pane-reading helpers the supervisor uses.

mod bind;
mod control;
mod entry;
mod finish;
mod output;
mod pane_turn;
mod process;
mod supervise;
mod view;

pub use entry::run_agent;
pub use supervise::supervise_existing;

pub(super) use control::{blocked_on_trust, cancel_headless};
#[cfg(test)]
pub(super) use finish::failure_message;
#[cfg(test)]
pub(super) use output::append_valid_utf8;
pub(super) use process::{execute_headless, publish};
pub(super) use view::compact_prompt;
