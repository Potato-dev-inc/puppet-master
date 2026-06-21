# @puppet-master/mcp

MCP server for [Puppet Master](https://github.com/Potato-dev-inc/puppet-master) — connect Cursor, Claude Desktop, Codex CLI, and other MCP hosts to orchestrate real terminal agents on your machine.

## What is Puppet Master?

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
2. `list_panes` — see live workers (reuse existing panes when possible)
3. `create_task` → `acquire_resource_lock` — coordinate before delegating
4. `read_agent_context` / `inspect_agent_model` — pick the right worker
5. `spawn_agent` — only if no suitable pane exists
6. `build_context_pack` — compact handoff prompt for the worker
7. `write_terminal_input` — delegate with `append_newline: true`
8. `read_terminal_buffer` — confirm receipt (once; avoid polling loops)
9. `complete_task` — close out with evidence

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
| `write_terminal_input` | Send text as if typed (`append_newline` defaults to `true`) |
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
| `delegate_task` | Validate structured delegation input and render a worker prompt |
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
