//! Team ("Heads-up", COWORK-PLAN.md): a quiet channel between teammates' Puppet Masters. When my
//! agent pushes, Puppet Master offers to tell the team; when a teammate's agent pushes, a card
//! offers Pull now, Queue my agent or Dismiss. GitHub facts (PRs opened, merged, closed; CI red or
//! green) arrive as the same cards, and an open pull request offers "Merge when green".
//!
//! The parts:
//! - [`event`]: the `TeamEvent` schema and the checks every inbound event passes;
//! - [`crypto`], [`keystore`]: Ed25519, XChaCha20-Poly1305, HPKE; keys in the credential store;
//! - [`roster`]: the signed member list; [`secure`]: sealing and the checks before `ingest`;
//! - [`invite`], [`membership`]: invites, joining, approving, removing (with key rotation);
//! - [`transport`]: the `TeamTransport` trait, the `FolderTransport` and the `RelayTransport`;
//! - [`config`]: `team.json`;
//! - [`sources`]: pushes from remote-tracking refs, PRs and CI through `gh`;
//! - [`store`]: drafts, cards, queued updates, the replay guard and gap counters;
//! - [`actions`], [`merge_green`]: what a click does in git and on GitHub, with the safety rules;
//! - [`mcp`]: the agent tools (`team_roster`, `team_events`, `team_wait`, `team_notify`): read-only, and drafts only;
//! - [`service`] / [`commands`] / [`phone`]: the live wiring, the cockpit commands, the phone route.
//!
//! Boundaries: team traffic never uses the bridge or device pairing (plan §0); a teammate's event
//! is untrusted data that can only become a card, and only after its signature, its sender's place
//! on the roster and its decryption were checked; nothing is sent, pulled, merged or applied
//! without the person's click; nothing is ever pushed; keys never leave the credential store.

pub mod actions;
pub mod commands;
pub mod config;
pub mod crypto;
pub mod event;
pub mod git;
pub mod invite;
pub mod keystore;
#[cfg(any(test, feature = "dev-fake-tui"))]
pub mod leakcheck;
pub mod mcp;
pub mod membership;
pub mod merge_green;
pub mod phone;
pub mod project_key;
pub mod roster;
pub mod secure;
pub mod service;
pub mod sources;
pub mod store;
pub mod transport;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_v1;
#[cfg(test)]
mod tests_hardening;
#[cfg(test)]
mod tests_mcp;

pub use service::init;
