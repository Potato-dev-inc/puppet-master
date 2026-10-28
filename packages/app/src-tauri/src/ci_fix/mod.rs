//! Red to Green (code name `ci_fix`): Puppet Master watches the CI checks of a pull request it opened
//! from one of its own copies. When checks fail, it asks the person (one card) whether the agent that
//! made the copy should fix them. The agent works in the same copy, in a pane that cannot push. When it
//! is done, Puppet Master checks the change and asks the person again before it pushes to the same
//! `pm/` branch. It never forces a push, never re-runs CI, and never changes a pull request's state.
//!
//! - `model`: the settings, the watches, the fix tries, the log snapshot, the state tags, the caps,
//!   the id validators and the error codes.
//! - `store`: the private files under `<app data>/ci-fix/` (settings, watches, fixes).
//! - `github`, `filter`, `logs`, `text`, `fixcheck`: GitHub reads, ownership rules, log cleaning, the
//!   texts, and the push check extras (later tasks).
//!
//! Invariants (SPEC n33 section 4; each one is tested where it is built):
//! - S1: CI log text never goes into a task; it is only in a file, as one fenced JSON object (data).
//! - S2: secrets in logs are redacted before anything is stored; finished fixes keep no log lines.
//! - S3: only pull requests Puppet Master opened, checked against GitHub on every poll.
//! - S4: the agent can only push from a pane that is guarded against pushing.
//! - S5: default and protected branches are never touched; explicit refspec, never a forced push.
//! - S6: a fix push is always asked, and held when CI files, git settings or history changed.
//! - S7: Puppet Master's own git calls run with hardened settings and a fingerprint is checked.
//! - S8: approval cards are the person's only; loopback and chat answers are refused.
//! - S9: Puppet Master only reads GitHub, plus its own fast-forward push of the fix branch.
//! - S10: tries per pull request, fixes a day, one fix at a time, and the watch limits bound spend.
//! - S11: start and push re-check the remote commit and the copy before they act.
//! - S12: Puppet Master never types into a pane and never answers a trust or permission prompt.
//! - S13: logs stay on this PC, redacted; only the agreed push leaves it.
//! - S14: generic code never names an agent.
//! - S15: no new bridge route or MCP tool; the phone answers through the existing question route.
//! - S16: the old raw-log send-back path is removed.

// RG-1 only stores the data. The engine that uses these items comes in later tasks.
#![allow(dead_code)]

pub mod model;
pub mod store;
pub mod github;
pub mod filter;
pub mod logs;
pub mod text;
pub mod fixcheck;

#[cfg(test)]
mod store_tests;
