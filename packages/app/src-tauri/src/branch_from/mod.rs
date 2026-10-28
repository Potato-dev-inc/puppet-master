//! Branch from here (SPEC n33): from any open chat, make a separate copy of the files as they were
//! at a save point, and start an agent in that copy with a short, fenced brief of the chat.
//!
//! Invariants (SPEC n33 section 4):
//! - The original folder and the old pane are never changed. A branch only reads save points.
//! - Chat text goes into the brief only inside fences, and only after cleaning. No tool calls,
//!   tool output, diffs, terminal text or environment are read.
//! - The brief and the person's message live in `<app data>/branches/` (private). Views carry no
//!   brief, message or path. No bridge, push, team or phone route touches this module.
//! - PM answers no trust, permission or login screen. The new agent runs with trust and shell off.
//! - Only the desktop window starts a branch (Tauri commands). No MCP tool, no card, no agent.
//! - A file name is built only from an id that passed `store::id_ok`.

pub mod model;
pub mod store;
pub mod point;
pub mod brief;
pub mod offers;
pub mod copy;
pub mod view;
pub mod live;

#[cfg(test)]
mod store_tests;
