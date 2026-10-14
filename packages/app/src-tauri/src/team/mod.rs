//! Team ("Heads-up", COWORK-PLAN.md): a quiet channel between teammates' Puppet Masters. When my
//! agent pushes, Puppet Master offers to tell the team; when a teammate's agent pushes, a card
//! offers Pull now, Queue my agent or Dismiss.
//!
//! v0 (this module): one shared folder as the wire, no crypto, no GitHub polling. The parts:
//! - [`event`]: the `TeamEvent` schema and the checks every inbound event passes;
//! - [`transport`]: the `TeamTransport` trait and the `FolderTransport` (a relay drops in later);
//! - [`config`]: `team.json`;
//! - [`sources`]: pushes from remote-tracking refs, PRs through `gh`;
//! - [`store`]: drafts, cards, queued updates, the replay guard;
//! - [`actions`]: what a click does in git, with the safety rules;
//! - [`service`] / [`commands`]: the live wiring and the cockpit commands.
//!
//! Boundaries: team traffic never uses the bridge or device pairing (plan §0); a teammate's event
//! is untrusted data that can only become a card; nothing is sent, pulled, merged or applied
//! without the person's click; nothing is ever pushed.

pub mod actions;
pub mod commands;
pub mod config;
pub mod event;
pub mod git;
pub mod project_key;
pub mod service;
pub mod sources;
pub mod store;
pub mod transport;
#[cfg(test)]
mod tests;

pub use service::init;
