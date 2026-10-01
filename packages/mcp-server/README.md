# @puppet-master/mcp

MCP server for [Puppet Master](https://github.com/Potato-dev-inc/puppet-master) — connect Cursor, Claude Desktop, Codex CLI, and other MCP hosts to orchestrate real terminal agents on your machine.

## What is Puppet Master?

New MCP connections default to **agent mode** with worker tools: `run_agent`,
`wait_agents`, `send_message`, `followup_task`, `interrupt_agent`, `inspect_agent`,
`agent_transcript`, `close_agent`, plus wrappers `send_agent` / `cancel_agent`,
`answer_prompt`, `list_agents`, `take_over`, and `set_mode`. `run_agent` performs
dispatch plus a bounded wait; `background: true` returns an immediate handle. Use
`set_mode({"mode":"shell"})` for terminal tools or `both` for the full catalog.
Catalog changes emit `notifications/tools/list_changed`. Hosts that ignore it can start in a chosen mode with `--mode <agent|shell|both>` or `PUPPET_MASTER_MODE`. See the
[agent and shell contract](../../docs/orchestrator/agent-shell-modes.md) and
[button GUI guide](../../docs/orchestrator/mcp-gui.md).

**Puppet Master** is a multi-agent terminal orchestrator. It spawns real PTY sessions for Claude Code, Codex CLI, OpenCode, Cursor, PowerShell, and Bash, then coordinates them like a senior engineer at the keyboard: breaking work into tasks, assigning worker panes, enforcing resource locks, handing off context packs, and watching for prompts or blockers.

### Puppet Master Desktop

The **desktop app** (built with Tauri + React) is the main control surface:

- **Terminal grid** — live xterm.js panes for each agent, with status LEDs (`running`, `waiting_input`, `idle`, `error`)
- **Puppet Master sidebar** — built-in LLM orchestrator, MCP activity log, tasks, locks, and context packs
- **Embedded HTTP bridge** — a local API on `127.0.0.1` (ports `17321`–`17399`) that owns pane lifecycle and coordination state
- **Mobile PWA** — optional mirror mode to steer panes from your phone over the same bridge

Download installers from [GitHub Releases](https://github.com/Potato-dev-inc/puppet-master/releases), or launch from source with `npm run tauri dev` in the main repo.

### What this package does

`@puppet-master/mcp` is the **stdio bridge** between external MCP hosts and the running desktop app. Your AI client speaks JSON-RPC over stdio; this package launches the Rust `puppet-master-mcp` binary, which translates tool calls into HTTP requests against the local bridge. The bridge then drives the Rust PTY manager inside Puppet Master Desktop. A legacy TypeScript wrapper is still shipped as a one-release fallback, but it reads the tool registry from the Rust bridge instead of carrying a separate tool list.

```
Cursor / Claude Desktop / Codex CLI
        │  stdio JSON-RPC (MCP)
        ▼
@puppet-master/mcp  ← npm shim + Rust stdio MCP binary
        │  HTTP on 127.0.0.1
        ▼
Puppet Master Desktop (embedded bridge + Rust PTY manager)
        │
        ▼
Real terminal panes (Claude, Codex, Bash, …)
```

Every orchestration path — desktop sidebar, mobile PWA, CLI orchestrator panes, and external MCP — hits the **same** bridge API. There is no duplicate logic.

## Prerequisites

1. **Puppet Master Desktop must be running.** The desktop app starts the bridge and writes a port file on launch. Without it, this package exits immediately with a clear error instead of hanging.

   - Install from [releases](https://github.com/Potato-dev-inc/puppet-master/releases), or run from the repo: `npm run tauri dev`
   - The CLI launcher (`npx puppet-master` from the main repo) also starts the GUI

2. **Node.js 22+** — required for the npm/npx launcher. The MCP protocol server itself is the bundled Rust binary.

3. **Bridge port file** — written automatically when the app starts:

   | OS      | Path |
   |---------|------|
   | Windows | `%APPDATA%\com.puppetmaster.app\puppet-master.bridge.port` |
   | macOS   | `~/Library/Application Support/com.puppetmaster.app/puppet-master.bridge.port` |
   | Linux   | `~/.local/share/com.puppetmaster.app/puppet-master.bridge.port` |

## Install & register

### Cursor

**Settings → Features → Model Context Protocol → Add new global MCP server:**

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

**Windows note:** if Cursor shows `MCP not connected`, use the full Node path and set the bridge port file explicitly (Cursor often cannot resolve bare `npx` on PATH):

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

### Claude Desktop

**File → Settings → Developer → Edit Config** (`claude_desktop_config.json`):

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

Restart Claude Desktop. Tools appear as `mcp__puppet-master__*`.

### Codex CLI

```bash
codex mcp add puppet-master -- npx -y @puppet-master/mcp
```

Verify: `codex mcp list` · Remove: `codex mcp remove puppet-master`

### Run standalone (debug)

```bash
npx @puppet-master/mcp
```

Or from the monorepo: `npm run mcp`

## Recommended orchestration flow

Whether you orchestrate from Cursor, Claude Desktop, or the built-in sidebar, the tool surface is identical. A typical external flow:

1. `bridge_health` — confirm Puppet Master is running
2. `delegate_work` — submit a task with an absolute `project_path` and caller-generated `idempotency_key`
3. `wait_for_operation` — wait from the returned `operation_id` and `revision`; continue waiting when the result is nonterminal
4. `get_operation` — inspect the latest durable snapshot when needed; `cancel_operation` requests cancellation and uses worker control for dispatched work.

This is the preferred simple flow. It does not require scraping terminal output. For work already prepared manually, `delegate_task` only validates and renders a prompt; it does not dispatch work.

```json
{
  "project_path": "C:/work/my-project",
  "task": "Fix the failing parser tests and report the changed files",
  "agent_type": "codex",
  "idempotency_key": "parser-fix-2026-09-30-01",
  "acceptance_criteria": ["Relevant tests pass", "Summarize changed files"]
}
```

Pass the same idempotency key when retrying the same request. It returns the existing operation instead of dispatching duplicate work; reusing the key with different work returns an idempotency conflict. Use a new key for a distinct request. `acceptance_criteria` is required and must include at least one non-empty criterion.

Operation snapshots include `operation_id`, `status`, monotonically increasing `revision`, `source` (`native` or `inferred`), `stage`, `pane_state`, `required_action`, `progress_pct`, `result`, and structured `error` when present (`code`, `message`, `recoverable`, optional `retry_after_ms`, `context`). Status is one of `queued`, `starting`, `running`, `waiting_input`, `cancelling`, `completed`, `failed`, or `cancelled`. `pane_state` and `required_action` provide structured observations when an agent is blocked. Progress is `null` unless the worker provides authoritative progress. A pane becoming idle or disappearing is not completion; `completed` requires explicit result evidence. If the worker presents an approval prompt during startup, `delegate_work` has already returned its operation ID and the operation moves to `waiting_input` while retaining the pane. Resolve the prompt manually; Puppet Master never approves it. The bridge watches for the prompt to clear, records the observed pane state, and resumes dispatch. `cancel_operation` requests cancellation of queued or running operations. For dispatched work, the adapter aborts a native session, terminates a process created for that operation, or sends Ctrl+C to a reused pane; adapter failures are returned as typed errors.

`wait_for_operation` accepts `project_path`, `operation_id`, optional `after_revision`, optional `until` statuses, and optional `timeout_ms`. Its `reason` is `matched_state`, `revision_changed`, `terminal`, or `timeout`; a timeout or revision change does not mean success. Re-check the snapshot and wait again while the operation remains nonterminal. When the MCP host supplies a progress token, revision changes produce progress notifications containing the current operation snapshot. Request cancellation (`notifications/cancelled`) wakes the waiting tool call and returns a typed cancellation error. To follow startup approval handling, wait for `waiting_input`, then wait again from that revision; the same operation continues after the prompt is cleared.

**Pane rules:** panes with id `puppet-master-orchestrator-*` are dedicated orchestrators. Never `write_terminal_input` or `kill_pane_process` on them — delegate only to worker panes.

Paste this into a Cursor rule or project prompt:

```text
When using the puppet-master MCP server, first call bridge_health, then list_panes.
Reuse existing panes. Before delegating, inspect the target pane with read_agent_context
and inspect_agent_model when choosing between agents. Only spawn a new agent if no
suitable pane exists. Send prompts with write_terminal_input append_newline=true,
then read_terminal_buffer once to confirm receipt.
```

## Tools

| Tool | Purpose |
|------|---------|
| `bridge_health` | Confirm the local bridge is reachable; returns version metadata |
| `list_panes` | Live panes: id, agent type, pid, status, cwd, size |
| `list_agent_contexts` | Supported agents with strengths and routing hints |
| `read_agent_context` | Agent profile or live pane context + buffer preview |
| `inspect_agent_model` | Parse recent output for active model signal |
| `spawn_agent` | New pane — `claude`, `codex`, `opencode`, `powershell`, `bash`, `cursor` |
| `read_terminal_buffer` | Scrollback (last N lines, default 200) |
| `write_terminal_input` | Send text as if typed (`append_newline` defaults to `true`; optional `model_provider` + `model_id` for `opencode_native`) |
| `kill_pane_process` | Terminate a worker pane and its child process |
| `create_task` | Create a coordination task before delegating work |
| `claim_task` | Claim or renew a task lease for a worker |
| `report_task_status` | Update task status (in progress, blocked, etc.) |
| `complete_task` | Complete a task with evidence from the worker |
| `list_tasks` | List project-local task projections |
| `acquire_resource_lock` | Exclusive lock on file, directory, command, port, branch, or pane |
| `release_resource_lock` | Release a lock owned by a worker |
| `build_context_pack` | Compact handoff prompt from task, locks, constraints, and scrollback |
| `read_session_context` | Read current goal, pane roles, pane digests, timeline, conflicts, and orchestrator policy |
| `update_session_context` | Update session context fields, currently `current_goal` |
| `set_pane_role` | Assign a pane role: implementer, reviewer, shell, orchestrator, or observer |
| `read_pane_digest` | Read the latest digest for a pane |
| `update_pane_digest` | Store a manual pane digest in the Rust event log |
| `delegate_task` | Validate structured delegation input and render a worker prompt; does not dispatch |
| `delegate_work` | Create/reuse a durable asynchronous operation with an idempotency key |
| `get_operation` | Read the latest operation snapshot |
| `wait_for_operation` | Wait for a new revision, selected status, terminal state, or timeout |
| `cancel_operation` | Request queued or running operation cancellation |
| `read_orchestrator_state` | Read Rust-owned orchestration runtime state |
| `update_orchestrator_state` | Update standby polling policy |

Coordination state (tasks, locks, audit log) is scoped per project and stored in `<project>/.puppet-master/events.jsonl`.

## Troubleshooting

**`Puppet Master bridge port file not found`**

Start Puppet Master Desktop first, then retry. The MCP server fails fast when the GUI is not running.

**Tools return errors or empty panes**

- Confirm the desktop app is open and a project folder is selected
- Check `bridge_health` — it should return OK
- Ensure agent binaries (Claude Code, Codex, etc.) are on `PATH` for spawns to work

**MCP host cannot connect (Windows)**

Use the full `node.exe` path and `PUPPET_MASTER_BRIDGE_PORT_FILE` env var in your MCP config (see Cursor section above).

## Further reading

- [MCP_HOSTS.md](https://github.com/Potato-dev-inc/puppet-master/blob/main/MCP_HOSTS.md) — detailed host setup and tool reference
- [ROUTING.md](https://github.com/Potato-dev-inc/puppet-master/blob/main/ROUTING.md) — sidebar orchestration (API vs CLI backends)
- [Main README](https://github.com/Potato-dev-inc/puppet-master#readme) — architecture, coordination model, and release builds

## License

MIT
