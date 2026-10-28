//! Assign an issue (UI name) / issue intake (code name): a person adds a repo they own, a label
//! and an agent in Settings. When someone allowed adds the label to a GitHub issue, Puppet Master
//! sees it through the person's own `gh` login, asks before it starts, and the agent works in a
//! separate copy on its own branch. The agent cannot push. When it is done, Puppet Master checks
//! the change and (on a yes) opens a draft pull request. It never merges.
//!
//! - `model`: the settings, the repo watch, the runs, the error codes and the id validators.
//! - `store`: the private files under `<app data>/issue-intake/` (config, runs, two empty folders).
//! - `pushcheck`: the push check (CI files, secret-looking files and lines, home paths, the git
//!   fingerprint, the reasons on the card).
//! - `github`, `filter`, `text`: GitHub reads, the trigger rules, and the texts (the issue file the
//!   agent reads, the task, the cards, the pull request).
//! - `live`: the `Io` trait (everything outside the engine's memory), the real `Io`, and the few
//!   functions the app calls: `start`, `on_resolved`, `act`, `view`, `poll_now`.
//! - `engine`, `answers`, `flows`: the rules. `engine` is the watch loop and the cards, `answers` is what
//!   a resolved card does, the queue and the Settings buttons; `flows` is
//!   start, finish (the check), push and throw away. Both run against a fake `Io` in the tests.
//! - `view`: what the Settings page and the commands read.
//!
//! Invariants (SPEC n32 section 4; each one is tested where it is built):
//! - S1: the issue text never goes into a prompt. It is only in a file, as data.
//! - S2: only the person (or the allowed list) can start a run. Labelers are checked on GitHub.
//! - S3: an explicit allow-list of repos; a folder must match its repo at save, start and push.
//! - S4: guarded panes get no GitHub or package tokens.
//! - S5: Puppet Master never answers an agent's own trust, permission or login prompt.
//! - S6: Puppet Master pushes only a `pm/...` branch, as a draft, after the push check.
//! - S9: a required budget per repo, a runs-per-day cap and a parallel cap.
//! - S11: file names come only from validated ids and numbers.
//! - S13: generic code never names an agent.

pub mod model;
pub mod store;
pub mod github;
pub mod filter;
pub mod text;
pub mod pushcheck;
pub mod view;
pub mod live;
mod engine;
mod flows;
mod answers;

#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod pushcheck_tests;
#[cfg(test)]
mod pushcheck_git_tests;
#[cfg(test)]
mod engine_tests;
