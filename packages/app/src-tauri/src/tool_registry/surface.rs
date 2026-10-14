//! The *listed* tool surface: which callable tools `tools/list` advertises by default.
//!
//! Two questions are kept apart on purpose:
//!
//! * **Callable** (`mode::tool_visible_in_mode`): may this connection call the tool? Enforced by
//!   the shim and by the bridge on every MCP-tagged route. This never changes here.
//! * **Listed** (this module): does `tools/list` advertise it? Hosts such as Claude Code defer
//!   every listed tool behind a search step once the list is long, so the default lists only the
//!   tools an orchestrator needs. An unlisted tool still works when called by name.
//!
//! Tiers: `Core` is listed in every mode that can call it; `Advanced` is callable but only listed
//! in `all` mode; `Alias` is a pure duplicate of another tool (same route, same behaviour) kept so
//! existing agents never break, and is also only listed in `all` mode.

use super::types::McpMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTier {
    Core,
    Advanced,
    Alias,
}

/// Bump when the listed surface changes without any tool name, description, route or schema
/// changing (it feeds `catalog_version`).
pub(super) const SURFACE_REVISION: u32 = 1;

/// Pure aliases: `(alias, canonical tool)`. Same route and behaviour as the canonical tool.
/// `send_agent` is NOT a sibling of `send_message`: it posts to `/agents/followup` exactly like
/// `followup_task` (a new turn), while `send_message` steers the current turn at `/agents/send`.
pub const ALIASES: &[(&str, &str)] = &[
    ("list_workers", "list_agents"),
    ("send_agent", "followup_task"),
    ("cancel_agent", "interrupt_agent"),
    ("wait_for_model", "wait_for_panes"),
    ("wait_for_task", "wait_for_panes"),
    ("wait_for_worker", "wait_for_panes"),
];

/// Callable but not listed by default: reconnect and ownership plumbing, OpenCode-specific tools,
/// the task board and locks, session-context and digest bookkeeping, and rarely used pane tools.
const ADVANCED: &[&str] = &[
    // Reconnect / ownership handoff between coordinators.
    "session_identity",
    "attach_agents",
    "release_lease",
    "transfer_agent",
    // OpenCode-specific.
    "reply_opencode_permission",
    "reply_opencode_question",
    "read_opencode_key_status",
    "rotate_opencode_key",
    "read_opencode_worker_status",
    "read_opencode_messages",
    "switch_agent_model",
    // Task board, locks, orchestrator and session bookkeeping.
    "create_task",
    "claim_task",
    "report_task_status",
    "complete_task",
    "list_tasks",
    "acquire_resource_lock",
    "release_resource_lock",
    "read_orchestrator_state",
    "update_orchestrator_state",
    "read_librarian_prompt",
    "read_project_ir_status",
    "build_context_pack",
    "read_session_context",
    "update_session_context",
    "set_pane_role",
    "read_pane_digest",
    "update_pane_digest",
    "list_agent_contexts",
    // Prompt rendering only; run_agent / delegate_work start the work.
    "delegate_task",
];

pub fn tool_tier(name: &str) -> ToolTier {
    if ALIASES.iter().any(|(alias, _)| *alias == name) {
        ToolTier::Alias
    } else if ADVANCED.contains(&name) {
        ToolTier::Advanced
    } else {
        ToolTier::Core
    }
}

/// The canonical tool an alias duplicates, if `name` is a pure alias.
pub fn alias_of(name: &str) -> Option<&'static str> {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map(|(_, canonical)| *canonical)
}

/// Whether `tools/list` advertises `name` in `mode`. Callers also check callability.
pub fn tool_listed_in_mode(name: &str, mode: McpMode) -> bool {
    name == "set_mode" || mode == McpMode::All || tool_tier(name) == ToolTier::Core
}

/// Stable code per tier for the catalog hash.
pub(super) fn tier_code(name: &str) -> u8 {
    match tool_tier(name) {
        ToolTier::Core => 0,
        ToolTier::Advanced => 1,
        ToolTier::Alias => 2,
    }
}
