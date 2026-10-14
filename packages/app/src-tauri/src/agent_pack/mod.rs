//! "Add Puppet Master to your agents": installs the skill pack (`integrations/skills`) and Puppet
//! Master's own MCP entry into each detected agent, with a preview, an apply and an undo.
//!
//! - `assets`   the pack files, embedded at build time (the same bytes the plugins ship)
//! - `host`     `SkillsHostTable`: where an agent reads skills, declared by the agent's profile
//! - `plan`     preview / apply / undo over the declared hosts
//! - `record`   what apply changed (created, replaced, backup), so undo can restore it
//! - `service`  JSON front shared by the Tauri commands and the tests
//!
//! Where an agent keeps its skills (`TuiProfile::skills_host`) and its MCP servers
//! (`TuiProfile::mcp_host`, the toolbelt's `[mcp_host]` table) is DATA the agent declares. Nothing
//! in this module matches an agent name. The MCP entry is rendered and edited with the toolbelt's
//! declarative writer (`mcp_install::declarative`, `toolbelt::edit`).

pub mod assets;
pub mod host;
pub mod plan;
pub mod record;
pub mod service;

#[cfg(test)]
mod tests;

pub use host::{skills_host_from_toml, SkillsHostTable};

/// Marker line every pack file carries, so an update can tell "ours" from a file the user wrote.
pub const MARKER_PREFIX: &str = "puppet-master-pack:";
