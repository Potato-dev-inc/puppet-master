//! Where an agent reads skills (`SKILL.md` folders). Declared by the agent's profile, like
//! `[mcp_host]` is for MCP servers.

use serde::Deserialize;

/// `dir` is the user-level skills folder, under the home folder (`~/.claude/skills`). Agents that
/// read the same folder share one write.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsHostTable {
    pub dir: String,
}

/// Parse a built-in `[skills_host]` declaration (the TOML text a code profile carries).
pub fn skills_host_from_toml(text: &str) -> Option<SkillsHostTable> {
    toml::from_str(text).ok()
}
