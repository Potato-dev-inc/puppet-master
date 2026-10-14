//! Wire types of the tool catalog: tool/resource/prompt definitions and the MCP mode enum.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolSafety {
    ReadOnly,
    Mutating,
    Destructive,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolVisibility {
    pub sidebar: bool,
    pub external_mcp: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub safety: ToolSafety,
    pub visibility: ToolVisibility,
    pub method: &'static str,
    pub path: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMode {
    Agent,
    Shell,
    Both,
    /// Everything callable, and everything listed: the full catalog including aliases and
    /// advanced tools. `Both` makes the same tools callable but lists only the core surface.
    All,
}

impl McpMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "shell" => Some(Self::Shell),
            "both" => Some(Self::Both),
            "all" => Some(Self::All),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
            Self::Both => "both",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResourceDefinition {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "mimeType")]
    pub mime_type: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PromptDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: Vec<PromptArgument>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PromptArgument {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}
