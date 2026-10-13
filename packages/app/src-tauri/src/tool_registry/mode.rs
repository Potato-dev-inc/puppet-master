//! Which tools each MCP mode (agent, shell, both) exposes.

use super::catalog::tools;
use super::types::{McpMode, ToolDefinition};

pub fn tools_for_mode(mode: McpMode) -> Vec<ToolDefinition> {
    tools()
        .into_iter()
        .filter(|tool| tool.visibility.external_mcp && tool_visible_in_mode(tool.name, mode))
        .collect()
}

pub fn tool_visible_in_mode(name: &str, mode: McpMode) -> bool {
    if name == "set_mode" {
        return true;
    }
    if mode == McpMode::Both {
        return true;
    }
    const AGENT_TOOLS: &[&str] = &[
        "run_agent",
        "review_worktree",
        "quick_ask",
        "wait_agents",
        "send_agent",
        "send_message",
        "followup_task",
        "answer_prompt",
        "cancel_agent",
        "interrupt_agent",
        "list_agents",
        "list_workers",
        "list_panes",
        "inspect_agent",
        "agent_transcript",
        "close_agent",
        "take_over",
        "release",
        "attach_agents",
        "release_lease",
        "transfer_agent",
        "session_identity",
        "reply_opencode_question",
        "reply_opencode_permission",
        "read_opencode_messages",
        "read_opencode_worker_status",
        "bridge_health",
        "delegate_work",
        "get_operation",
        "wait_for_operation",
        "cancel_operation",
        "ask_user",
    ];
    const SHELL_TOOLS: &[&str] = &[
        "list_panes",
        "bridge_health",
        "read_agent_context",
        "inspect_agent_model",
        "switch_agent_model",
        "read_terminal_buffer",
        "write_terminal_input",
        "press_key",
        "spawn_agent",
        "kill_pane_process",
        "wait_for_panes",
        "wait_for_model",
        "wait_for_worker",
        "read_recent_events",
        "shell_exec",
        "release",
        "take_over",
        "ask_user",
    ];
    match mode {
        McpMode::Agent => AGENT_TOOLS.contains(&name),
        McpMode::Shell => SHELL_TOOLS.contains(&name),
        McpMode::Both => true,
    }
}
