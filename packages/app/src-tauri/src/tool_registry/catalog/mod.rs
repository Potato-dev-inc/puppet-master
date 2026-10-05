//! The tool catalog, assembled from per-family modules, plus its version hash.

mod agents;
mod context;
mod opencode_reply;
mod opencode_status;
mod operations;
mod orchestrator;
mod panes;
mod tasks;
mod waits;

use super::types::ToolDefinition;

/// Coordinator-facing MCP initialize text (stdio binary and HTTP catalog consumers).
pub fn mcp_instructions() -> &'static str {
    "Puppet Master MCP (agent mode). For a tiny question call quick_ask(task) and read its answer; its receipt lists what was not verified. Start work with one call: run_agent(agent_type, project_path, task) creates its own pane in project_path, runs the task and returns the result and a handle; no spawn_agent needed. To reuse an existing worker: list_agents → run_agent(worker_id or pane_id, task) → handle; send_message(handle) steers the current turn (receipt ≠ result); followup_task(handle, task) starts the next turn on the same conversation; wait_agents(handles) collects answers (default until: completed, failed, cancelled, needs_input). When run_agent/followup_task/delegate_work or WAIT_TIMEOUT returns a still-running turn it includes a watch_command (a settled turn has none; read its result directly); run that watch_command (plain `npx puppet-master watch …`) via your agent harness shell tool's background mode — do not use detached Start-Process; the harness must own stdout so it can wake when the turn finishes. run_agent/followup_task mint idempotency_key if omitted — reuse it after WAIT_TIMEOUT. Do not close the user's pane as cleanup. OpenCode yes/no: answer_prompt, not press_key."
}

pub fn catalog_version() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for tool in tools() {
        tool.name.hash(&mut hasher);
        tool.description.hash(&mut hasher);
        tool.method.hash(&mut hasher);
        tool.path.hash(&mut hasher);
        tool.input_schema.to_string().hash(&mut hasher);
    }
    format!("{}-{:016x}", tools().len(), hasher.finish())
}

pub fn external_mcp_tool_count() -> usize {
    tools()
        .into_iter()
        .filter(|tool| tool.visibility.external_mcp)
        .count()
}

/// Every tool, in catalog order. `catalog_version` hashes this order, so keep family order stable.
pub fn tools() -> Vec<ToolDefinition> {
    let mut all = Vec::new();
    all.extend(agents::tools());
    all.extend(panes::tools());
    all.extend(tasks::tools());
    all.extend(context::tools());
    all.extend(operations::tools());
    all.extend(orchestrator::tools());
    all.extend(opencode_status::tools());
    all.extend(waits::tools());
    all.extend(opencode_reply::tools());
    all
}
