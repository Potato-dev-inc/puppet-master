# Registering Puppet Master with MCP hosts

`@puppet-master/mcp` is a stdio MCP server. Register it with any host that
supports MCP — the package shells out to the local HTTP bridge that the
Tauri GUI is already running.

## Prerequisites

1. Start the GUI: `npx puppet-master` (or run `npm run tauri dev` from the repo).
2. Verify the bridge port file exists (Windows: `%TEMP%\puppet-master.bridge.port`).

## Cursor

**Cursor → Settings → Features → Model Context Protocol → Add new global MCP server:**

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "npx",
      "args": ["-y", "@puppet-master/mcp"]
    }
  }
}
```

Cursor will discover the Puppet Master tools and let the agent use them.

### Cursor Orchestrator Instructions

When Cursor is using Puppet Master as an MCP server, tell the Cursor agent to follow this order:

1. Call `bridge_health` to confirm Puppet Master is running.
2. Call `list_panes` before doing anything else.
3. Reuse an existing matching agent pane when possible. Do not spawn duplicate Claude/Codex/OpenCode panes unless the user explicitly asks for another one.
4. For any live agent pane you may delegate to, call `read_agent_context` with `pane_id`.
5. If choosing between multiple agents, call `inspect_agent_model` for each candidate pane and prefer the stronger/smarter fit for the task.
6. Delegate with `write_terminal_input` using `append_newline: true`.
7. After delegating, call `read_terminal_buffer` once to confirm the agent received the task.
8. Avoid polling buffers repeatedly without sending new instructions.

You can paste this into Cursor as a project rule or include it in the prompt:

```text
When using the puppet-master MCP server, first call bridge_health, then list_panes.
Reuse existing panes. Before delegating, inspect the target pane with read_agent_context
and inspect_agent_model when choosing between agents. Only spawn a new agent if no
suitable pane exists. Send prompts with write_terminal_input append_newline=true,
then read_terminal_buffer once to confirm receipt.
```

## Claude Desktop

**File → Settings → Developer → Edit Config** opens `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "npx",
      "args": ["-y", "@puppet-master/mcp"]
    }
  }
}
```

Restart Claude Desktop. The tools appear in the prompt as `mcp__puppet-master__*`.

## Codex CLI

```bash
codex mcp add puppet-master -- npx -y @puppet-master/mcp
```

Verify: `codex mcp list`. Remove: `codex mcp remove puppet-master`.

## Verifying the connection

If Puppet Master is **not** running, the MCP server exits with:

```
Puppet Master bridge port file not found at "puppet-master.bridge.port".
Start Puppet Master first (`npx puppet-master`).
```

If you see this, start the GUI and try again.

## Tool reference

The tools exposed by `@puppet-master/mcp` (and used by the built-in
Puppet Master LLM):

### `bridge_health`
No arguments. Confirms the bridge is reachable and returns version metadata.

### `list_panes`
No arguments. Returns:
```json
[
  {
    "id": "uuid",
    "agent_type": "claude",
    "pid": 1234,
    "status": "running" | "waiting_input" | "idle" | "error",
    "created_at": 1700000000000,
    "last_output_at": 1700000000000,
    "cwd": "C:\\path",
    "cols": 120,
    "rows": 30
  }
]
```

### `list_agent_contexts`
No arguments. Returns supported agent profiles with strengths, smartness score,
best-fit task types, and planned sidebar actions.

### `read_agent_context`
```json
{
  "agent_type": "claude",
  "pane_id": "uuid"
}
```
Pass either `agent_type` for a static profile or `pane_id` for live pane context.

### `inspect_agent_model`
```json
{ "pane_id": "uuid", "lines": 200 }
```
Returns the best-known model signal from recent terminal output plus an advisory
smartness score.

### `spawn_agent`
```json
{
  "agent_type": "claude" | "codex" | "opencode" | "powershell" | "bash" | "cursor",
  "cwd": "C:\\optional\\path",        // optional, defaults to current project
  "cols": 120, "rows": 30,            // optional
  "pane_id": "stable-id"              // optional, caller-supplied
}
```
Returns `{ "pane_id": "..." }`.

### `read_terminal_buffer`
```json
{ "pane_id": "uuid", "lines": 200 }
```
Returns plain-text recent scrollback.

### `write_terminal_input`
```json
{
  "pane_id": "uuid",
  "text": "y",
  "append_newline": true   // false for partial input
}
```

### `kill_pane_process`
```json
{ "pane_id": "uuid" }
```

## Architecture note

```
external MCP host (Cursor / Claude Desktop / Codex)
        │ stdio JSON-RPC
        ▼
@puppet-master/mcp  (this package)
        │ HTTP on 127.0.0.1
        ▼
@puppet-master/bridge  (Node daemon spawned by the GUI)
        │ HTTP on 127.0.0.1
        ▼
Tauri / Rust PaneRegistry  (owns the actual PTYs)
```

All three layers live in this monorepo and are coordinated by the GUI.
The bridge and the Rust PTY manager are spawned together when the GUI
starts; external MCP clients only need `@puppet-master/mcp`.
