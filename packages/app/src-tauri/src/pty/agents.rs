//! Agent type presets — mirrors `packages/shared/src/agents.ts`.
//!
//! Kept in sync manually rather than via a build script so both sides can
//! evolve independently. The `agent_type` strings here are the same as the
//! TypeScript enum.

use crate::platform::{current_os, Os};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    Opencode,
    OpencodeNative,
    Cmd,
    Powershell,
    Bash,
    Cursor,
    #[serde(rename = "cursor_agent")]
    CursorAgent,
}

#[allow(dead_code)]
impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
            AgentType::Opencode => "opencode",
            AgentType::OpencodeNative => "opencode_native",
            AgentType::Cmd => "cmd",
            AgentType::Powershell => "powershell",
            AgentType::Bash => "bash",
            AgentType::Cursor => "cursor",
            AgentType::CursorAgent => "cursor_agent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "opencode_native" => Some(Self::OpencodeNative),
            "cmd" => Some(Self::Cmd),
            "powershell" => Some(Self::Powershell),
            "bash" => Some(Self::Bash),
            "cursor" => Some(Self::Cursor),
            "cursor_agent" => Some(Self::CursorAgent),
            _ => None,
        }
    }
}

/// Returns the executable + base args for an agent type. On Windows, .cmd /
/// .bat wrappers (npm-installed CLIs) require special invocation through
/// `cmd.exe /C` because CreateProcess does not execute them directly.
pub fn resolve_command(agent: AgentType) -> (&'static str, Vec<&'static str>) {
    match current_os() {
        Os::Windows => resolve_windows(agent),
        Os::MacOs => resolve_macos(agent),
        Os::Linux => resolve_linux(agent),
    }
}

fn resolve_windows(agent: AgentType) -> (&'static str, Vec<&'static str>) {
    match agent {
        AgentType::Claude => ("claude", vec![]),
        AgentType::Codex => (
            "codex",
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never",
            ],
        ),
        AgentType::Opencode => ("opencode", vec![]),
        AgentType::OpencodeNative => ("opencode", vec![]),
        AgentType::Cmd => ("cmd.exe", vec!["/K"]),
        AgentType::Powershell => ("powershell.exe", vec!["-NoLogo", "-NoExit"]),
        AgentType::Bash => ("bash.exe", vec!["--login"]),
        AgentType::Cursor => ("cursor.cmd", vec![]),
        // Cursor Agent installs a PowerShell shim on Windows; spawn it inside
        // an interactive PowerShell host instead of asking ConPTY to execute
        // the `.ps1` file directly.
        AgentType::CursorAgent => ("cursor-agent.ps1", vec![]),
    }
}

fn resolve_macos(agent: AgentType) -> (&'static str, Vec<&'static str>) {
    match agent {
        AgentType::Claude => ("claude", vec![]),
        AgentType::Codex => (
            "codex",
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never",
            ],
        ),
        AgentType::Opencode => ("opencode", vec![]),
        AgentType::OpencodeNative => ("opencode", vec![]),
        AgentType::Cmd => ("zsh", vec!["-l"]),
        AgentType::Powershell => ("pwsh", vec!["-NoLogo", "-NoExit"]),
        AgentType::Bash => ("bash", vec!["--login"]),
        AgentType::Cursor => ("cursor", vec![]),
        AgentType::CursorAgent => ("cursor-agent", vec![]),
    }
}

fn resolve_linux(agent: AgentType) -> (&'static str, Vec<&'static str>) {
    match agent {
        AgentType::Claude => ("claude", vec![]),
        AgentType::Codex => (
            "codex",
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never",
            ],
        ),
        AgentType::Opencode => ("opencode", vec![]),
        AgentType::OpencodeNative => ("opencode", vec![]),
        AgentType::Cmd => ("bash", vec!["--login"]),
        AgentType::Powershell => ("pwsh", vec!["-NoLogo", "-NoExit"]),
        AgentType::Bash => ("bash", vec!["--login"]),
        AgentType::Cursor => ("cursor", vec![]),
        AgentType::CursorAgent => ("cursor-agent", vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_stays_interactive() {
        for (os, command, expected) in [
            (Os::Windows, "powershell.exe", vec!["-NoLogo", "-NoExit"]),
            (Os::MacOs, "pwsh", vec!["-NoLogo", "-NoExit"]),
            (Os::Linux, "pwsh", vec!["-NoLogo", "-NoExit"]),
        ] {
            let actual = match os {
                Os::Windows => resolve_windows(AgentType::Powershell),
                Os::MacOs => resolve_macos(AgentType::Powershell),
                Os::Linux => resolve_linux(AgentType::Powershell),
            };
            assert_eq!(actual, (command, expected));
        }
    }

    #[test]
    fn cursor_ide_and_cursor_agent_have_distinct_launch_commands() {
        assert_eq!(AgentType::parse("cursor").unwrap().as_str(), "cursor");
        assert_eq!(
            AgentType::parse("cursor_agent").unwrap().as_str(),
            "cursor_agent"
        );
        assert_eq!(resolve_windows(AgentType::Cursor).0, "cursor.cmd");
        assert_eq!(
            resolve_windows(AgentType::CursorAgent).0,
            "cursor-agent.ps1"
        );
        assert_eq!(
            serde_json::to_string(&AgentType::CursorAgent).unwrap(),
            "\"cursor_agent\""
        );
    }

    #[test]
    fn bash_agent_uses_bash_across_platforms() {
        assert_eq!(
            resolve_windows(AgentType::Bash),
            ("bash.exe", vec!["--login"])
        );
        assert_eq!(resolve_macos(AgentType::Bash), ("bash", vec!["--login"]));
        assert_eq!(resolve_linux(AgentType::Bash), ("bash", vec!["--login"]));
    }
}
