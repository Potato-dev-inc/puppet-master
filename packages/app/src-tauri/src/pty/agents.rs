//! Agent type presets — mirrors `packages/shared/src/agents.ts`.
//!
//! Kept in sync manually rather than via a build script so both sides can
//! evolve independently. The `agent_type` strings here are the same as the
//! TypeScript enum.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    Opencode,
    Powershell,
    Bash,
    Cursor,
}

#[allow(dead_code)]
impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
            AgentType::Opencode => "opencode",
            AgentType::Powershell => "powershell",
            AgentType::Bash => "bash",
            AgentType::Cursor => "cursor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "powershell" => Some(Self::Powershell),
            "bash" => Some(Self::Bash),
            "cursor" => Some(Self::Cursor),
            _ => None,
        }
    }
}

/// Returns the executable + base args for an agent type. On Windows, .cmd /
/// .bat wrappers (npm-installed CLIs) require special invocation through
/// `cmd.exe /C` because CreateProcess does not execute them directly.
pub fn resolve_command(agent: AgentType) -> (&'static str, Vec<&'static str>) {
    match agent {
        AgentType::Claude => ("claude.exe", vec![]),
        AgentType::Codex => (
            "codex.exe",
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never",
            ],
        ),
        AgentType::Opencode => ("opencode.cmd", vec![]),
        AgentType::Powershell => ("powershell.exe", vec!["-NoLogo"]),
        AgentType::Bash => ("bash.exe", vec!["--login"]),
        AgentType::Cursor => ("cursor.cmd", vec![]),
    }
}
