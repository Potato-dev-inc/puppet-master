//! Static per-agent context profiles (capabilities, model detection, instructions).

use super::*;

pub fn list_agent_context_profiles() -> Vec<AgentContextProfile> {
    [
        AgentType::Claude,
        AgentType::Codex,
        AgentType::Opencode,
        AgentType::OpencodeNative,
        AgentType::Cmd,
        AgentType::Powershell,
        AgentType::Bash,
        AgentType::Cursor,
    ]
    .iter()
    .map(|agent_type| get_agent_context_profile(*agent_type))
    .collect()
}

pub fn get_agent_context_profile(agent_type: AgentType) -> AgentContextProfile {
    match agent_type {
        AgentType::Claude => AgentContextProfile {
            agent_type,
            label: "Claude Code",
            default_model: None,
            model_detection: ModelDetection::CliBanner,
            smartness: 9,
            strengths: &[
                AgentCapability::CodebaseReasoning,
                AgentCapability::Implementation,
                AgentCapability::Review,
                AgentCapability::Debugging,
            ],
            context_notes: &[
                "Strong default for broad repository understanding, multi-file edits, and code review.",
                "Usually exposes its active model in the TUI banner or startup text when configured by the CLI.",
            ],
            best_for: &[
                "planning complex changes",
                "reviewing diffs",
                "large refactors",
                "implementation with tests",
            ],
            planned_sidebar_actions: &[
                "delegate task",
                "ask for review",
                "compare plan against Codex",
                "summarize current pane",
            ],
        },
        AgentType::Codex => AgentContextProfile {
            agent_type,
            label: "Codex CLI",
            default_model: None,
            model_detection: ModelDetection::CliBanner,
            smartness: 9,
            strengths: &[
                AgentCapability::Implementation,
                AgentCapability::Debugging,
                AgentCapability::TerminalOps,
                AgentCapability::CodebaseReasoning,
            ],
            context_notes: &[
                "Good default for surgical coding, build fixes, and terminal-native verification loops.",
                "Model may be supplied by the Codex config or surfaced in the TUI; inspect terminal buffer for the best available signal.",
            ],
            best_for: &[
                "coding fixes",
                "running tests",
                "debugging build failures",
                "iterative verification",
            ],
            planned_sidebar_actions: &[
                "delegate implementation",
                "run verification loop",
                "ask for status",
                "handoff focused bug",
            ],
        },
        AgentType::Opencode => AgentContextProfile {
            agent_type,
            label: "OpenCode",
            default_model: None,
            model_detection: ModelDetection::Configuration,
            smartness: 7,
            strengths: &[
                AgentCapability::Implementation,
                AgentCapability::TerminalOps,
                AgentCapability::Debugging,
            ],
            context_notes: &[
                "Useful as an additional coding terminal when you want parallel exploration.",
                "Model routing is provider-config dependent, so treat detection as advisory unless the buffer shows an explicit model.",
            ],
            best_for: &[
                "parallel edits",
                "alternative implementation passes",
                "lighter bug fixes",
            ],
            planned_sidebar_actions: &[
                "delegate parallel attempt",
                "ask for alternative",
                "compare output",
            ],
        },
        AgentType::OpencodeNative => AgentContextProfile {
            agent_type,
            label: "OpenCode (API)",
            default_model: None,
            model_detection: ModelDetection::Configuration,
            smartness: 8,
            strengths: &[
                AgentCapability::Implementation,
                AgentCapability::TerminalOps,
                AgentCapability::Debugging,
            ],
            context_notes: &[
                "Runs opencode serve locally; orchestrator input uses prompt_async instead of PTY typing.",
                "The attach TUI is for visibility; bridge/MCP writes target the API session.",
            ],
            best_for: &[
                "orchestrated workers",
                "API-driven prompts",
                "parallel OpenCode sessions",
            ],
            planned_sidebar_actions: &[
                "delegate via API",
                "inspect session health",
                "reply to permission prompts",
            ],
        },
        AgentType::Cmd => AgentContextProfile {
            agent_type,
            label: "Command Prompt",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 1,
            strengths: &[AgentCapability::TerminalOps],
            context_notes: &[
                "Plain cmd.exe-style shell pane. It has no model; use it for deterministic Windows command workflows.",
            ],
            best_for: &[
                "Windows command prompt workflows",
                "batch commands",
                "environment inspection",
            ],
            planned_sidebar_actions: &["run command", "capture output", "prepare environment"],
        },
        AgentType::Powershell => AgentContextProfile {
            agent_type,
            label: "PowerShell",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 1,
            strengths: &[AgentCapability::TerminalOps],
            context_notes: &[
                "Plain shell pane. It has no model; use it for deterministic commands and scripts.",
            ],
            best_for: &["build commands", "file inspection", "manual scripts"],
            planned_sidebar_actions: &["run command", "capture output", "prepare environment"],
        },
        AgentType::Bash => AgentContextProfile {
            agent_type,
            label: "Bash",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 1,
            strengths: &[AgentCapability::TerminalOps],
            context_notes: &["Plain shell pane. It has no model; use it for POSIX-flavored commands."],
            best_for: &["shell commands", "cross-platform scripts", "log inspection"],
            planned_sidebar_actions: &["run command", "capture output", "prepare environment"],
        },
        AgentType::Cursor => AgentContextProfile {
            agent_type,
            label: "Cursor IDE",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 6,
            strengths: &[
                AgentCapability::UiOrchestration,
                AgentCapability::Implementation,
            ],
            context_notes: &[
                "IDE launcher rather than a terminal TUI. Treat model information as unavailable unless a future Cursor bridge reports it.",
            ],
            best_for: &["opening the workspace visually", "manual user-guided edits"],
            planned_sidebar_actions: &["open project", "focus editor", "handoff manual review"],
        },
        AgentType::CursorAgent => AgentContextProfile {
            agent_type,
            label: "Cursor Agent CLI",
            default_model: None,
            model_detection: ModelDetection::CliBanner,
            smartness: 8,
            strengths: &[AgentCapability::CodebaseReasoning, AgentCapability::Implementation, AgentCapability::Review],
            context_notes: &["Headless Cursor Agent CLI. Read-only delegated tasks use its documented plan mode."],
            best_for: &["headless code tasks", "structured result reporting"],
            planned_sidebar_actions: &["delegate task", "inspect result", "review changes"],
        },
    }
}
