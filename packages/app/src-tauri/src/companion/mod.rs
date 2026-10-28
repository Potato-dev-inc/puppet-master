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
//! - `skills`: `skills/<slug>.md`, ways of working the person approved (proposed through the pending list)
//!
//! The runtime side (piece P2):
//!
//! - `keeper`: the pure state machine of one companion (keep one pane alive, restart with a growing
//!   wait, sleeping when the person closes the pane, the state table, the nap rule)
//! - `schedule`: check-ins (due, next, the 2 h rule, the daily cap), with the clock passed in
//! - `host`: what the service needs from the app (`Host` trait); `live` implements it over the real
//!   registry, outbox, attention list and session chat, and runs the thread `companion-keeper`
//! - `service`: the tick over every companion; `manage`: create, edit, wake, sleep, memory, check-ins
//! - `moving`: move a companion to another agent and back (never while it is busy)
//! - `today`: `today.md`, the day's turns of the person's other agents (opt-in per companion)
//! - `view`: `CompanionView` for the desktop (with the folder) and the phone (without)
//!
//! The foreman side (it finds what was said, it hires helpers):
//!
//! - `recall`: `companion_recall`, a search of the companion's own past chats (`memory/pending.md` stays
//!   the only way to remember: there is no second memory path)
//! - `caller`: is this MCP connection really inside a companion's pane (the process tree, not a header)
//! - `delegate`: the `delegated` feed line, the helper turn budget, the home-folder rule
//! - `guard`: the hard rule, a companion never answers a helper's trust, permission or question card
//! - `peer`: the same rule for a plain loopback request (a shell `curl`): who owns the TCP connection
//!
//! Only `live.rs` starts a thread or a process, and nothing here names an agent. A message to the agent
//! goes through `pane_outbox` only: the companion never types into a pane or answers its dialogs.

pub mod agents;
pub mod brief;
pub mod caller;
pub mod delegate;
pub mod files;
pub mod guard;
pub mod host;
pub mod keeper;
pub mod live;
pub mod manage;
pub mod memory;
pub mod model;
pub mod moving;
pub mod peer;
pub mod personas;
pub mod recall;
pub mod schedule;
pub mod service;
pub mod skills;
pub mod store;
pub mod today;
pub mod view;

pub use service::Service;

#[cfg(test)]
mod foreman_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod move_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod skill_tests;
#[cfg(test)]
pub mod test_host;
#[cfg(test)]
mod today_tests;
#[cfg(test)]
mod tests;
