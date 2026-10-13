//! Durable, revisioned operations exposed to orchestration clients.
//!
//! Pane lifecycle and task lifecycle are separate: an idle or missing pane never
//! implies that the operation's task completed.
//!
//! This file is the module hub; the behavior lives in submodules and is re-exported here so
//! callers keep using `crate::operations::<item>`:
//! - `model`: snapshot, request and error types
//! - `store`: on-disk records, indexes, change notification
//! - `create`: request validation, idempotency, queued snapshot
//! - `dispatch` / `startup`: claiming an operation, pane reservation, startup readiness
//! - `transitions` / `worker`: status changes and worker/session metadata
//! - `finish` / `cancel` / `recovery`: completion, cancellation, restart recovery
//! - `query` / `wait` / `view`: reads, blocking waits, API shaping

mod cancel;
mod create;
mod dispatch;
mod finish;
mod model;
mod query;
mod recovery;
mod startup;
mod store;
mod transitions;
mod view;
mod wait;
mod worker;
mod worktree;

pub use cancel::*;
pub use create::*;
pub use dispatch::*;
pub use finish::*;
pub use model::*;
pub use query::*;
pub use recovery::*;
pub use startup::*;
pub use transitions::*;
pub use view::*;
pub use wait::*;
pub use worker::*;
pub use worktree::*;

#[cfg(test)]
mod tests;

/// Test helper: how many index entries name the project with this comparable key.
#[cfg(test)]
pub(crate) fn indexed_project_keys_for_test(key: &str) -> usize {
    store::indexed_projects()
        .unwrap_or_default()
        .iter()
        .filter(|entry| store::project_key(entry) == key)
        .count()
}
