# Glossary

| Term | Meaning |
|------|---------|
| **Bridge** | Loopback HTTP server inside the Tauri process (`bridge.rs`). Default `127.0.0.1:17321`. |
| **Shim** | `puppet-master-mcp` stdio MCP binary. Maps tools to HTTP. Owns no panes. |
| **Catalog** | MCP tool list in `tool_registry.rs`. `catalog_version` hashes it. |
| **SCHEMA_DRIFT** | Shim catalog ≠ running app catalog. Rebuild both. |
| **build_id** | `PM_BUILD_ID` stamped into the **lib** at compile time. Not in the MCP exe. |
| **Pane** | One PTY session (`pane_id`). |
| **Handle** | Worker id for the agent API. Not a pane id. |
| **Operation** | Durable JSON snapshot of one turn under `.puppet-master/operations/`. |
| **Worker** | Managed agent bound to a handle (and usually a pane). |
| **Orchestrator** | The caller that delegates; must not do the implementation work. |
| **TuiProfile** | Per-agent trait: launch, screen, prompts, extract. |
| **Headless** | CLI run without a visible TUI; structured stdout. |
| **opencode_native** | OpenCode HTTP API + attached TUI. Not screen scraping. |
| **result_capture** | `Authoritative` / `Inferred` / `Missing` — how the reply was obtained. |
| **Receipt** | Structured outcome on a finished (or in-progress) run. A steer receipt is **not** a result. |
| **Lease / ownership** | Which MCP session may mutate a handle or pane. |
| **Mode** | `agent` / `shell` / `both` — which tools `tools/list` exposes. |
| **WAIT_TIMEOUT** | Foreground wait ended; turn still running. Reuse the idempotency key. |
| **watch** | CLI long-poll of one operation. Exit does not cancel the run. |
| **Context pack** | Compact handoff prompt from task, locks, constraints, evidence. |
| **Lock** | Exclusive claim on a file, dir, command, port, branch, or pane. |
| **Prompt id** | Stable id for an on-screen approval (`prompt_id_for`). Required by `answer_prompt`. |
| **keep_pane** | Do not dispose a run-spawned pane after the turn. Interrupt implies keep, then cancel may still release. |
