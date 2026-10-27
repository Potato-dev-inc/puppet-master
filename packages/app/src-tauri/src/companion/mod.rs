//! Companion: an always-on helper that Puppet Master keeps alive in one agent pane. It runs on an
//! agent the person already has, so it costs no extra model fees.
//!
//! This folder is the data and file side (piece P1 of the spec): what a companion is saved as, where
//! its files live, what the brief says, how proposed memories are kept or dropped.
//!
//! - `model`: the data types, the caps and the validation rules (wire format: camelCase JSON)
//! - `store`: `companions.json`, `runtime.json`, `feed.jsonl` and the approved memory copies
//! - `files`: the home folder (SOUL, USER, MEMORY, the pending list, the marker file)
//! - `brief`: the brief text, and its marker block inside the agent's instruction file
//! - `memory`: pending review, keep and drop, outside changes, the texts a brief is built from
//! - `agents`: which agents can be a companion, and the launch-arg check
//! - `personas`: the four SOUL templates
//!
//! Nothing here starts a process or a thread, and nothing here names an agent.

// P1 is the data and file side only. The keeper, the routes and the commands (P2) call these; until
// then most items are unused in the library build. P2 removes this allow.
#![allow(dead_code)]

pub mod agents;
pub mod brief;
pub mod files;
pub mod memory;
pub mod model;
pub mod personas;
pub mod store;
#[cfg(test)]
mod tests;
