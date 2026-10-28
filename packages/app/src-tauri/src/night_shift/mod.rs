//! Night Shift: a list of tasks that Puppet Master works through while the person sleeps.
//!
//! Each task runs in its own separate copy of its project (the same copies missions use). The
//! declared checks decide whether it passed. Nothing is merged until the person presses Keep in the
//! morning. SPEC n32, sections 3 and 4.
//!
//! Invariants every file in this folder keeps:
//! - Gates (trust, permission, login) and agent questions are never answered. Nothing here has a
//!   write path into a pane (`write_input`, `send_keys`, `press_key`, `answer_prompt`).
//! - Only the person merges. Keep and Throw away are desktop commands only. No MCP tool, bridge
//!   route or companion path reaches `review`.
//! - "Tests pass" only ever means the checks the person declared, run from a snapshot taken at Start.
//!   A green run whose tests were changed is `NeedsLook`.
//! - Ids come only from validated patterns (`ns-` + 12 hex, `nt-` + 8 hex), so they are safe as file names.
//! - Generic code never names an agent. Labels and candidates come from the registry.
//! - Privacy: no task text, paths or names leave the computer in a push. Saved and shared text is redacted.
//!
//! Layout: `model` (data types, constants), `store` (private files), `checks` (declared tests),
//! `plan` (who runs next), `classify` and `report` (results and words), `page` (saved page),
//! `view` (UI views), `review` (Keep and Throw away), `runner` (the supervisor), `io`, `io_start`,
//! `io_facts` (the adapters), `power` (keep awake), `quiet` (silent panes and runs).
//!
//! Only `model` and `store` have code so far. The other files are empty stubs for later tasks.

// NS-1 defines the data and the store. Later tasks use the rest, so unused items are expected for now.
#![allow(dead_code)]

pub mod checks;
pub mod classify;
pub mod io;
pub mod io_facts;
pub mod io_start;
pub mod model;
pub mod page;
pub mod plan;
pub mod power;
pub mod quiet;
pub mod report;
pub mod review;
pub mod runner;
pub mod store;
pub mod view;
