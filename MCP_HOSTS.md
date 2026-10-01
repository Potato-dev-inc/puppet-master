# Registering Puppet Master with MCP hosts

`@puppet-master/mcp` is a stdio MCP server. Register it with any host that
supports MCP — the package shells out to the local HTTP bridge that the
Tauri GUI is already running.

## Prerequisites

New connections start in **agent mode**. Use `run_agent` to delegate and wait,
then `wait_agents`, `send_message` / `followup_task`, `interrupt_agent`, or
`inspect_agent` with its handle. `send_agent` and `cancel_agent` still work as
wrappers. The terminal tools documented below require `set_mode({"mode":"shell"})`
or `set_mode({"mode":"both"})`.
See [agent and shell modes](docs/orchestrator/agent-shell-modes.md).

1. Start the GUI: `npx puppet-master` (or run `npm run tauri dev` from the repo).
2. Verify the bridge port file exists:
   - Windows: `%APPDATA%\com.puppetmaster.app\puppet-master.bridge.port`
   - macOS: `~/Library/Application Support/com.puppetmaster.app/puppet-master.bridge.port`
   - Linux: `~/.local/share/com.puppetmaster.app/puppet-master.bridge.port`

## Local repository config

Do not commit host-specific MCP config files. `.mcp.json`, `opencode.json`,
`.codex/`, `.claude/`, and `.cursor/` are local developer state because they
often contain absolute paths or credentials.

For repo-local development, copy `.mcp.json.example` and adjust it as needed.
Most hosts should prefer the published package form:

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

**Windows note:** if Cursor shows `32000 MCP not connected`, use the full Node path and the
AppData bridge port file (Cursor often cannot resolve bare `node` / `npx` on PATH):

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "C:/Program Files/nodejs/node.exe",
      "args": ["-y", "@puppet-master/mcp"],
      "env": {
        "PUPPET_MASTER_BRIDGE_PORT_FILE": "C:/Users/YOU/AppData/Roaming/com.puppetmaster.app/puppet-master.bridge.port"
      }
    }
  }
}
```

Cursor will discover the Puppet Master tools and let the agent use them.

MCP hosts can cache tool catalogs. `tools/list` is served from the local Rust
catalog (no HTTP fetch). If an agent tool is missing, the host is likely using
the legacy TypeScript fallback or a stale binary: run `npm run build:mcp`,
reconnect, and inspect `initialize.serverInfo.catalog_version` before using a
raw HTTP workaround.

**Local dev (this repo):** build the MCP launcher and point Cursor at it so you get the latest tools without waiting for npm publish:

```bash
npm run build:mcp
```

Then set Cursor MCP to (adjust paths):

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "C:/Program Files/nodejs/node.exe",
      "args": ["C:/Users/YOU/Desktop/work2/puppet-master/packages/mcp-server/dist/index.js"],
      "env": {
        "PUPPET_MASTER_BRIDGE_PORT_FILE": "C:/Users/YOU/AppData/Roaming/com.puppetmaster.app/puppet-master.bridge.port"
      }
    }
  }
}
```

Toggle the MCP server **off and on** in Cursor after rebuilding — `bridge_health` should report current `tool_count` (includes `read_opencode_messages`, `reply_opencode_question`, `wait_for_worker`).

**Orchestrator playbooks:** [docs/orchestrator/README.md](docs/orchestrator/README.md) — quickstart, OpenCode worker guide, workflows, token tips.

**Local dev tip:** use `legacy.js` instead of `index.js` so the tool catalog loads from the bridge (avoids stale `puppet-master-mcp.exe`):

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "C:/Program Files/nodejs/node.exe",
      "args": ["C:/Users/YOU/Desktop/work2/puppet-master/packages/mcp-server/dist/legacy.js"],
      "env": {
        "PUPPET_MASTER_BRIDGE_PORT_FILE": "C:/Users/YOU/AppData/Roaming/com.puppetmaster.app/puppet-master.bridge.port"
      }
    }
  }
}
```

### Cursor Orchestrator Instructions

When Cursor is using Puppet Master as an MCP server, tell the Cursor agent to follow this order:

1. Call `bridge_health` to confirm Puppet Master is running.
2. Call `list_panes` before doing anything else.
3. Reuse an existing matching agent pane when possible. Do not spawn duplicate Claude/Codex/OpenCode panes unless the user explicitly asks for another one.
4. For any live agent pane you may delegate to, call `read_agent_context` with `pane_id`.
5. If choosing between multiple agents, call `inspect_agent_model` for each candidate pane and prefer the stronger/smarter fit for the task.
6. Prefer `delegate_work` with an absolute `project_path`, `task`, and caller-generated `idempotency_key` for ordinary delegation.
7. Call `wait_for_operation` using its `operation_id` and `revision`; continue waiting while the operation is nonterminal. Use `get_operation` to inspect. `cancel_operation` requests cancellation and uses worker control when available.
8. For a manually managed pane, send with `write_terminal_input` and call its suggested wait. Do not poll `read_terminal_buffer` in a loop.
9. For `opencode_native`: use `read_opencode_messages` for output; `reply_opencode_question` for yes/no; `inspect_agent_model` or `read_opencode_worker_status` for status.
10. Reconnect MCP in Cursor after tool registry changes so the tool catalog stays fresh.

Full playbooks: [docs/orchestrator/README.md](docs/orchestrator/README.md).

You can paste this into Cursor as a project rule or include it in the prompt:

```text
When using the puppet-master MCP server, call bridge_health before delegating.
For ordinary work, use delegate_work with an absolute project_path, task, and
idempotency_key, then wait_for_operation using its operation_id and revision.
Inspect get_operation when needed. cancel_operation asks worker control to stop a dispatched operation, including aborting a native session when supported.
Retry the same request with the same idempotency_key. A timeout or revision change
is not success unless the snapshot is terminal and completed with result evidence.
waiting_input means a human response is needed; never auto-approve. A pane becoming
idle is not task completion. For manual pane workflows, use write_terminal_input
followed by its suggested wait. Do not loop read_terminal_buffer or list_panes.
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
Returns plain-text recent scrollback. Debug/evidence only for `opencode_native` — prefer `read_opencode_messages` for model text.

### `read_opencode_messages`
```json
{ "pane_id": "uuid", "limit": 20, "role": "assistant" }
```
Returns structured OpenCode session messages (`opencode_native` only): assistant/user text parts, tool/question parts, `pending_question`, and `last_assistant_text`. No TUI chrome.

### `reply_opencode_question`
```json
{ "pane_id": "uuid", "answer": "Yes", "request_id": "que_..." }
```
Answer OpenCode API yes/no menus. Prefer over `press_key`. Omit `request_id` to answer the pane's current pending question.

### `wait_for_worker`
```json
{ "pane_id": "uuid", "timeout_ms": 120000 }
```
Long-poll until settled (`idle`, `permission`, API yes/no → `tui_prompt`, etc.). Default follow-up to `write_terminal_input` via `suggested_wait`.

### `write_terminal_input`
```json
{
  "pane_id": "uuid",
  "text": "y",
  "append_newline": true,
  "model_provider": "anthropic",
  "model_id": "claude-sonnet-4"
}
```

`append_newline` defaults to `true` (set `false` for partial input). For `opencode_native` panes, `via_opencode_api` is set automatically. Optional `model_provider` + `model_id` override the OpenCode model for that prompt (Settings → Orchestrator → **OpenCode default model** is used when omitted).

### `kill_pane_process`
```json
{ "pane_id": "uuid" }
```

### `wait_for_panes` / `wait_for_model` / `wait_for_task`

Long-poll until a pane reaches a target state. Prefer these over polling buffers.

```json
{
  "pane_ids": ["uuid"],
  "until": ["idle", "model_ready", "permission"],
  "match": { "provider_id": "opencode-go", "model_id": "glm-5.2" },
  "timeout_ms": 120000
}
```

Mutating pane tools (`spawn_agent`, `write_terminal_input`, `switch_agent_model`) return `suggested_wait` — call it immediately after each mutation.

### Durable asynchronous operations

`delegate_task` validates structured input and renders a worker prompt; it does not dispatch. `delegate_work` creates a durable operation using `{ "project_path": "C:/work/project", "task": "...", "agent_type": "codex", "idempotency_key": "unique-for-this-request", "acceptance_criteria": ["... "] }`. Optional fields include `pane_id`, `task_id`, `exclusive`, and `locks`. Reusing the same key for the same request returns the existing operation and does not dispatch again; a different request with that key returns an idempotency conflict. Use a new key for a distinct request. At least one non-empty acceptance criterion is required.

```json
{ "project_path": "C:/work/project", "task": "Fix parser tests", "agent_type": "codex", "idempotency_key": "parser-fix-01", "acceptance_criteria": ["Relevant tests pass"] }
```

`get_operation` takes `operation_id` and optional `project_path`. `wait_for_operation` takes `operation_id`, optional `project_path`, and optionally `after_revision`, `until` (status list), and `timeout_ms`. It returns `{ "snapshot": ..., "reason": "matched_state" | "revision_changed" | "terminal" | "timeout" }`. `cancel_operation` takes `operation_id` and optional `project_path`, then requests cancellation. Its snapshot can pass through `cancelling`; running cancellation uses the agent adapter interruption path (abort a native session, terminate an operation-created process, or send Ctrl+C to a reused pane).

Snapshots carry `status` (`queued`, `starting`, `running`, `waiting_input`, `cancelling`, `completed`, `failed`, `cancelled`), monotonically increasing `revision`, `source` (`native` or `inferred`), optional `stage`, `pane_state`, `required_action`, `progress_pct` (null when no authoritative progress exists), optional `result`, and structured `error` fields (`code`, `message`, `recoverable`, optional `retry_after_ms`, `context`). A timeout or revision change is not success. Idle or missing panes do not imply completion; `completed` requires explicit result evidence. On a startup approval prompt, `delegate_work` has already returned its operation ID and the operation waits in `waiting_input` with the pane preserved. A human must clear the prompt; no approval is automatic. The bridge watches for resolution, records the observed pane state, and resumes dispatch. When the MCP host supplies a progress token, revision changes produce progress notifications containing the current operation snapshot. Request cancellation wakes the waiting tool call and reports a typed cancellation error. Running cancellation uses worker control, including aborting a native session when supported.

### `read_recent_events`

Debug-only event tail (not for polling loops). Optional `pane_id`, `types`, `since_id`, `limit`.

## Session and delegation tools

The Rust bridge also exposes registry-backed coordination tools for longer-running orchestration:

- `read_session_context` / `update_session_context` — read or update the current goal, pane roles, pane digests, timeline, lock conflicts, and standby policy.
- `set_pane_role` — assign `implementer`, `reviewer`, `shell`, `orchestrator`, or `observer`.
- `read_pane_digest` / `update_pane_digest` — persist a short pane summary without rereading scrollback.
- `delegate_task` — validate structured delegation input and render a worker prompt without launching a pane.
- `delegate_work` / `get_operation` / `wait_for_operation` / `cancel_operation` — create and track durable revisioned work.
- `read_orchestrator_state` / `update_orchestrator_state` — inspect or tune Rust-owned standby timing.

## Architecture note

```
external MCP host (Cursor / Claude Desktop / Codex)
        │ stdio JSON-RPC
        ▼
@puppet-master/mcp  (this package)
        │ HTTP on 127.0.0.1
        ▼
Puppet Master Desktop embedded Rust bridge
        │
        ▼
Tauri / Rust PaneRegistry  (owns the actual PTYs and coordination state)
```

The desktop app starts the embedded Rust bridge and writes the bridge port
file when it launches; external MCP clients only need `@puppet-master/mcp`.
