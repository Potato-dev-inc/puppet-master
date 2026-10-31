//! Potato Bot: a chat friend that lives in Puppet Master, with its own small brain. Phase 1 has one
//! home for it, the desktop app (a second one, a chat app, comes later). This folder is the Rust core
//! (piece P2 of the spec); the page is P3, the settings and helpers are P4.
//!
//! The brain is a separate Node program (`packages/potato-brain`). Puppet Master starts it, hands it
//! messages and takes its answers over the local bridge. It cannot do anything else:
//!
//! - `protocol`: the channel protocol v1 (inbox items, events, their size limits)
//! - `auth`: who may call `/potato/brain/*` (a new caller on the bridge) and what a brain can never do
//! - `rules`: `decide`, the one pure function that says what happens to a message
//! - `env`: the brain's environment, an allowlist built from nothing
//! - `keeper`: the pure state machine of one brain process (lazy start, idle stop, crash backoff)
//! - `sidecar`: the IO of the process (finding Node, starting hidden, stopping the whole tree)
//! - `store`: the files in `<app data>/potato/` (bots, settings, conversations, persona seeding)
//! - `cost`: pricing the tokens the brain reports and judging them against the daily limit
//! - `log`: the 200-line log ring, every line cleaned
//! - `service`: the engine that ties them together; `live`: the app's one instance and its thread
//! - `wire_names`: names the brain package fixes that are also other names (the one reasoned exception to the
//!   "no agent names in generic code" guard)
//!
//! Keys and tokens live only in the OS credential store and in the brain's environment. Nothing here
//! names an agent or a chat app: a chat app's code would live only in `potato/surfaces/<app>/`.

pub mod auth;
pub mod cost;
pub mod env;
pub mod keeper;
pub mod live;
pub mod log;
pub mod wire_names;
pub mod protocol;
pub mod rules;
pub mod service;
pub mod sidecar;
pub mod store;

#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod cost_tests;
#[cfg(test)]
mod env_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod keeper_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod rules_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod store_tests;
