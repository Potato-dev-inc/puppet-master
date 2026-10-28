//! What Broke It: finds the agent turn that broke the tests (spec n33, section 3).
//!
//! Invariants (spec section 4): nothing here writes into a pane or sends keys to one, nothing runs
//! a command except through `exec.rs`, and nothing touches the person's folder. Every pure part is
//! unit-tested with no I/O. Names taken from test output are redacted by the caller before they are
//! saved or shown.

pub mod failnames;
