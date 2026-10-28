//! MCP tool catalog: tool/resource/prompt definitions served by the bridge and the stdio shim.
//!
//! Layout: `types` (wire types), `mode` (agent/shell visibility), `schema` (shared JSON-schema
//! helpers), `catalog/` (one module per tool family), `extras` (resources and prompts).

mod catalog;
mod extras;
mod mode;
mod schema;
mod surface;
mod types;

pub use catalog::{catalog_version, companion_recall_for_pane, external_mcp_tool_count, mcp_instructions, tools};
pub use extras::{prompts, resources};
pub use mode::{callable_tools_for_mode, tool_visible_in_mode, tools_for_mode};
pub use surface::{alias_of, tool_listed_in_mode, tool_tier, ToolTier, ALIASES};
pub use types::{
    McpMode, PromptArgument, PromptDefinition, ResourceDefinition, ToolDefinition, ToolSafety,
    ToolVisibility,
};

#[cfg(test)]
mod surface_tests;
#[cfg(test)]
mod tests;
