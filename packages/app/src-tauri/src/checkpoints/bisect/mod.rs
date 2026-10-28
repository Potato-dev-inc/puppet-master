//! "What broke it": find the agent turn that broke the person's tests (R14 #6, SPEC-n33).
//!
//! Invariants (SPEC-n33 section 4): tests run only in a throw-away copy under
//! `what-broke-it/copies/<8 hex>` and never in the person's folder; only the person's declared
//! command runs; only the PID tree of the child this code started is ever killed; raw test output
//! never leaves `exec.rs` unredacted; nothing here writes to a pane or answers an agent.
//!
//! This is the minimal skeleton that WB-4 needs. WB-1 adds the other modules.

/// One declared test command, run in a copy folder (SPEC-n33 3.4).
#[allow(dead_code)] // the first caller is WB-13 (`live.rs`)
pub mod exec;
