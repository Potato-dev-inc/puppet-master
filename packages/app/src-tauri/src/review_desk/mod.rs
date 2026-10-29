//! Review Desk (SPEC n33): one calm list of finished agent work, riskiest first, with Keep, Ask to
//! fix and Throw away. The person does every action from the desktop; nothing merges, pushes or
//! throws work away by itself.
//!
//! - `model`: the types (SPEC 3.2) and the strict `ItemKey` parser. `marks`: the "looked at" file.
//! - `risk`: the fixed order table (pure). `sources`: converters from copies, Night Shift, issues, turns.
//! - `live` (the `DeskIo` trait and the real `LiveIo`), `gather` (read, fill, order), `act` (Keep, Throw away,
//!   Ask to fix, detail), `fix` (the draft), `view` (what leaves Rust, and the read-only phone view).
//!
//! Invariants (SPEC 4):
//! - S1: actions are Tauri commands only; no MCP tool, bridge action route, companion or channel reaches them.
//! - S2: no timer or bulk path merges or pushes; Keep is always the person's click.
//! - S3: every action carries the item fingerprint; a changed item is refused.
//! - S4: titles, reasons and file names are plain text; control and bidi characters are removed.
//! - S5: patches and strings are redacted (strict) before they leave Rust; the phone sees no paths or diffs.
//! - S6: Throw away has a 10 s Undo window; folder restores take a "Before undo" safety point.
//! - S7: Ask to fix only fills a draft; it never sends or types into a terminal.
//! - S8: the desk never answers a trust, permission or login prompt and starts no agent.
//! - S9: an issue Keep is only the draft push check; it never merges and `Held` runs have no Keep.
//! - S10: ids are parsed strictly; handles resolve only through the worktree service.
//! - S11: the order is a fixed table over recorded facts; the reason is always shown.
//! - S12: generic code never names an agent; labels come from the registry.
//! - S13: marks and audit hold ids and counts only; fixtures hold no personal data.
//! - S14: the phone route sits behind the normal bridge auth and is GET only.
//! - S15: git work runs off the UI thread, is cached, and skips huge patches.

// The model types are read by the later RD tasks (risk, sources, gather, act, view).
#![allow(dead_code)]

pub mod fix;
pub mod gather;
pub mod act;
pub mod live;
pub mod marks;
pub mod model;
pub mod risk;
pub mod sources;
pub mod view;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod git_tests;
#[cfg(test)]
mod guard_tests;
