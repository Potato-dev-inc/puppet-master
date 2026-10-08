//! Session recorder (SPEC 5.13): sanitized screen snapshots from a pane the user chose to record,
//! the raw material for a draft definition (`draft`, T32). Drafts are never loaded (REC-4).

pub mod redact;
pub mod snapshots;

#[cfg(test)]
mod tests;

pub use snapshots::{is_recording, observe, session, start, stop, SnapKind, Session, Snapshot};
