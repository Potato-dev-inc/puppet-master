//! Toolbelt: list MCP servers once, write them into every agent's own config.
//!
//! - `store`   the user's list (`toolbelt.json` in the app data folder)
//! - `sync`    the engine: per agent, add/update/remove ONLY the entries this feature owns
//!             (keys named `pm-toolbelt-<id>`), with a preview and a `.bak`
//! - `import`  read servers an agent already has and offer them for the toolbelt
//! - `edit`    format-preserving edits of one table in a JSON / TOML / YAML config
//!
//! Where an agent keeps its MCP servers is declared by the agent itself (`TuiProfile::mcp_host`,
//! or `[mcp_host]` in a declarative definition). Nothing here matches an agent name.

pub mod edit;
pub mod import;
pub mod service;
pub mod store;
pub mod sync;

#[cfg(test)]
mod tests;


use crate::tui::declarative::schema::McpHostTable;

/// Prefix of every config key the toolbelt owns. Anything else in an agent's config is the user's.
pub const KEY_PREFIX: &str = "pm-toolbelt-";

/// Parse a built-in `[mcp_host]` declaration (the TOML text a code profile carries).
pub fn host_from_toml(text: &str) -> Option<McpHostTable> {
    toml::from_str(text).ok()
}
