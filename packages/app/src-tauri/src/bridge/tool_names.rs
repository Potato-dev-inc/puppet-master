//! Maps bridge routes to the MCP tool names recorded in the event log.

pub(super) fn bridge_tool_name(method: &str, segments: &[&str]) -> Option<String> {
    match (method, segments) {
        ("GET", ["health"]) => Some("bridge_health".to_string()),
        ("GET", ["agent-contexts"]) => Some("list_agent_contexts".to_string()),
        ("POST", ["questions", "ask"]) => Some("ask_user".to_string()),
        ("GET", ["panes"]) => Some("list_panes".to_string()),
        ("POST", ["panes"]) => Some("spawn_agent".to_string()),
        ("DELETE", ["panes", _]) => Some("kill_pane_process".to_string()),
        ("GET", ["panes", _, "buffer"]) => Some("read_terminal_buffer".to_string()),
        ("GET", ["panes", _, "snapshot"]) => Some("read_terminal_snapshot".to_string()),
        ("POST", ["panes", _, "input"]) => Some("write_terminal_input".to_string()),
        ("POST", ["panes", _, "key"]) => Some("press_key".to_string()),
        ("POST", ["panes", _, "detach"]) => Some("detach_terminal_pane".to_string()),
        ("POST", ["panes", "wait", "model"]) => Some("wait_for_model".to_string()),
        ("POST", ["panes", "wait", "task"]) => Some("wait_for_task".to_string()),
        ("POST", ["panes", "wait", "worker"]) => Some("wait_for_worker".to_string()),
        ("POST", ["panes", "wait"]) => Some("wait_for_panes".to_string()),
        ("GET", ["events", "recent"]) => Some("read_recent_events".to_string()),
        ("GET", ["panes", _, "model"]) => Some("inspect_agent_model".to_string()),
        ("POST", ["panes", _, "model"]) => Some("switch_agent_model".to_string()),
        ("GET", ["panes", _, "agent-context"]) => Some("read_agent_context".to_string()),
        ("GET", ["events", "replay", "panes"]) => Some("replay_pane_timeline".to_string()),
        ("GET", ["workspace", "state"]) => Some("get_workspace_state".to_string()),
        ("GET", ["tasks"]) => Some("list_tasks".to_string()),
        ("GET", ["locks"]) => Some("list_locks".to_string()),
        ("GET", ["agents", _, "inbox"]) => Some("read_agent_inbox".to_string()),
        ("GET", ["audit"]) => Some("get_audit".to_string()),
        ("POST", ["context-packs"]) => Some("build_context_pack".to_string()),
        ("GET", ["project-ir", "status"]) => Some("read_project_ir_status".to_string()),
        ("GET", ["librarian", "prompt"]) => Some("read_librarian_prompt".to_string()),
        ("GET", ["session", "context"]) => Some("read_session_context".to_string()),
        ("PATCH", ["session", "context"]) => Some("update_session_context".to_string()),
        ("POST", ["panes", _, "role"]) => Some("set_pane_role".to_string()),
        ("GET", ["panes", _, "digest"]) => Some("read_pane_digest".to_string()),
        ("POST", ["panes", _, "digest"]) => Some("update_pane_digest".to_string()),
        ("POST", ["delegate-task"]) => Some("delegate_task".to_string()),
        ("POST", ["operations", "delegate"]) => Some("delegate_work".to_string()),
        ("GET", ["operations", _]) => Some("get_operation".to_string()),
        ("POST", ["operations", _, "wait"]) => Some("wait_for_operation".to_string()),
        ("POST", ["operations", _, "cancel"]) => Some("cancel_operation".to_string()),
        ("GET", ["orchestrator", "state"]) => Some("read_orchestrator_state".to_string()),
        ("PATCH", ["orchestrator", "state"]) => Some("update_orchestrator_state".to_string()),
        ("GET", ["opencode", "keys", "status"]) => Some("read_opencode_key_status".to_string()),
        ("PATCH", ["opencode", "keys", "settings"])
        | ("POST", ["opencode", "keys", "settings"]) => {
            Some("set_opencode_key_settings".to_string())
        }
        ("POST", ["opencode", "keys", "rotate"]) => Some("rotate_opencode_key".to_string()),
        ("GET", ["opencode", "worker-status"]) => Some("read_opencode_worker_status".to_string()),
        ("GET", ["panes", _, "opencode", "status"]) => {
            Some("read_opencode_worker_status".to_string())
        }
        ("GET", ["panes", _, "opencode", "messages"]) => Some("read_opencode_messages".to_string()),
        ("POST", ["panes", _, "opencode", "permissions", _, "reply"]) => {
            Some("reply_opencode_permission".to_string())
        }
        ("POST", ["panes", _, "opencode", "question", "reply"]) => {
            Some("reply_opencode_question".to_string())
        }
        ("POST", ["tasks"]) => Some("create_task".to_string()),
        ("POST", ["tasks", _, "claim"]) => Some("claim_task".to_string()),
        ("POST", ["tasks", _, "lease"]) => Some("renew_task_lease".to_string()),
        ("POST", ["tasks", _, "status"]) => Some("report_task_status".to_string()),
        ("POST", ["tasks", _, "complete"]) => Some("complete_task".to_string()),
        ("POST", ["tasks", _, "block"]) => Some("block_task".to_string()),
        ("POST", ["tasks", _, "reviewer"]) => Some("assign_reviewer".to_string()),
        ("POST", ["locks"]) => Some("acquire_resource_lock".to_string()),
        ("POST", ["locks", "release"]) => Some("release_resource_lock".to_string()),
        ("POST", ["locks", _, "expire"]) => Some("expire_resource_lock".to_string()),
        _ => None,
    }
}
