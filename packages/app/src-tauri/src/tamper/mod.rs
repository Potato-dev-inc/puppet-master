//! The tamper guard: a standalone engine that notices when tests or check config were weakened so
//! that a check turns green, and says so in plain words (DESIGN-MISSIONS section 4).
//!
//! ```text
//! let snap = tamper::take(project, &SnapshotOptions::default())?;   // at the start
//! tamper::save(project, &snap)?;                                     // optional: survives restarts
//! ...                                                                // an agent works
//! let report = tamper::check(project, &snap, &CheckOptions::default());
//! if report.blocks { /* show "The checks pass, but the tests were changed" */ }
//! tamper::undo(project, &snap, &report.blocking_files());            // only on the person's say-so
//! ```
//!
//! It has no dependency on Missions. Everything about a language lives in `rules/*.toml`; the Rust
//! here is generic (lexing, counting, diffing, pairing, wording). It is deterministic: no scores,
//! no model. It only ever reads, except `undo`, which restores files the caller names.
//!
//! Layout: `model` (types), `rules` (table loader), `lexer` (comments and strings), `facts` (counts
//! and markers per file), `diff` (line diffs), `config` (check config sections), `protect` (the
//! protected set), `snapshot` (take), `signals` (compare, the eleven kinds), `store` (disk), `undo`,
//! `http` (the read-only bridge route).

// The engine's public functions have no caller in the app until Missions v0 lands; they are
// exercised by `tests.rs` and the read-only route.
#![allow(dead_code)]

mod config;
mod diff;
mod facts;
pub mod http;
mod lexer;
mod model;
mod protect;
mod rules;
mod signals;
mod snapshot;
mod store;
#[cfg(test)]
mod tests;
mod undo;

use std::path::Path;

#[allow(unused_imports)]
pub use model::{CheckKind, Finding, Kind, Report, RunCounts, Severity, Snapshot};
pub use signals::CheckOptions;
pub use snapshot::SnapshotOptions;
pub use store::TamperError;
pub use undo::UndoReport;

/// Take a snapshot of the test files and check config in `root`. Nothing is written.
pub fn take(root: &Path, opts: &SnapshotOptions) -> Result<Snapshot, TamperError> {
    snapshot::take(root, opts)
}

/// Take a snapshot and save it under `<root>/.puppet-master/tamper/<id>.json`.
pub fn take_and_save(root: &Path, opts: &SnapshotOptions) -> Result<Snapshot, TamperError> {
    let snap = snapshot::take(root, opts)?;
    store::save(root, &snap)?;
    Ok(snap)
}

pub fn save(root: &Path, snap: &Snapshot) -> Result<(), TamperError> {
    store::save(root, snap)
}

pub fn load(root: &Path, id: &str) -> Result<Snapshot, TamperError> {
    store::load(root, id)
}

/// Compare the project now with a snapshot.
pub fn check(root: &Path, snap: &Snapshot, opts: &CheckOptions) -> Report {
    signals::check(root, snap, opts)
}

/// `load` then `check`.
pub fn check_saved(root: &Path, id: &str, opts: &CheckOptions) -> Result<Report, TamperError> {
    if !root.is_dir() {
        return Err(TamperError::BadProject(root.display().to_string()));
    }
    let snap = store::load(root, id)?;
    Ok(signals::check(root, &snap, opts))
}

/// Put the named files back to how the snapshot had them (see `undo.rs`).
pub fn undo(root: &Path, snap: &Snapshot, files: &[String]) -> UndoReport {
    undo::undo(root, snap, files)
}

/// Ids of the snapshots saved for a project, newest first.
pub fn list(root: &Path) -> Vec<String> {
    store::list(root)
}

pub fn delete(root: &Path, id: &str) -> Result<(), TamperError> {
    store::delete(root, id)
}
