[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Potato-dev-inc/puppet-master)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

# Puppet Master

**Turn other coding TUIs into subagents** that another AI can call.

OpenCode, Cursor Agent, Claude Code, Codex, and shells already run as real terminals. Puppet Master wraps those sessions so your orchestrator — Cursor, Claude Desktop, Codex, or the in-app sidebar — can `run_agent`, wait, steer, lock files, and get a receipt. Same idea as a native Task/subagent, except the worker is an inspectable pane with a persistent session, not a hidden helper that vanishes when the turn ends.

It is the **control room**, not the coder. The orchestrator plans and delegates. The worker TUI writes the code. The run result tells you whether the agent finished **and** whether the requested checks passed — that distinction is the point.

One HTTP bridge. One tool catalog. Desktop, mobile PWA, and MCP hosts all drive the same workers.

Installers: [GitHub Releases](https://github.com/Potato-dev-inc/puppet-master/releases). Handbook: [docs/wiki](docs/wiki/README.md).

Use it from inside your agent: Settings > Bridge & MCP > **Add Puppet Master to your agents** installs the skills and the connection for Claude Code, Codex, Cursor and OpenCode (with Undo). Or add the Claude Code plugin: `/plugin marketplace add Potato-dev-inc/puppet-master`, then `/plugin install puppet-master@pm`. Details: [integrations/README.md](integrations/README.md).

---

## What you get

| Surface | Role |
|---------|------|
| **Desktop app** | Terminal grid + orchestrator sidebar (Tauri 2 + React) |
| **Mobile PWA** | Mirror desktop panes from your phone (no PTY on the device) |
| **`@puppet-master/mcp`** | Stdio MCP shim for Cursor / Claude Desktop / Codex |
| **`puppet-master` CLI** | Launch GUI / worker window; `watch` long-polls a running turn |
| **Coordination kernel** | Tasks, resource locks, operations, context packs, audit log |

---

## Vs native subagents

Native delegation (Cursor `Task`, in-host subagents, and similar) feels like a **direct conversation with a helper**. Puppet Master feels like a **control room**: you pick the backend and workspace, watch a real TUI, and get an explicit outcome.

Independent GPT evaluation of PM vs the native workflow it tested (not a ranking of other orchestrators):

| Experience | Puppet Master | Native delegation |
|---|---|---|
| Starting work | Backend, workspace, and session choices | Simple spawn |
| Receiving results | Watcher handling; sometimes approvals | Final messages arrived automatically |
| Verifying work | Built-in acceptance checks and explicit outcomes | Parent must implement verification |
| Visibility | Inspectable panes, commands, and approvals | Primarily messages and tool outputs |
| Control | Locks, cancellation, persistent sessions, steering receipts | Simpler coordination |
| Friction | More orchestration steps; some noisy or missing results | Less coordination overhead |

**PM’s strongest feature is distinguishing “the agent finished” from “the requested checks passed.”** That matters more than launching another model.

What held up in that eval:

- **OpenCode (`opencode_native`)** — structured results, working steering, and acceptance checks. The dependable path for implementation work.
- **Visible Cursor** — needs more supervision. Approvals improved; short replies can still go missing and tool chrome can leak into the result. Headless Cursor captured `[]` correctly when the visible path failed.
- **Native subagents** — still better for small questions and internal subtasks (less ceremony).

When to use which:

| Job | Prefer |
|-----|--------|
| Small questions, internal subtasks | Native delegation |
| Implementation that needs checks, locks, or a visible session | PM + OpenCode |
| Work that must run in Cursor | PM Cursor, plus independent verification |

PM currently asks for more attention than a native spawn. The capabilities are already useful; clean automatic result delivery and recovery after disconnects are the remaining bar for it to become a general default.

---

## Architecture

The desktop process embeds a loopback HTTP bridge (`127.0.0.1`, default `17321`, scans upward). MCP is a **stateless stdio shim** over that API. If the GUI is not running, MCP fails fast.

```
┌─────────────────────────────────────────────────────────────┐
│  Puppet Master desktop (Tauri)                              │
│  terminal grid (xterm.js)  ·  sidebar LLM  ·  PaneRegistry  │
└──────────────────────────┬──────────────────────────────────┘
                           │ 127.0.0.1:17321–17399
           ┌───────────────┼────────────────┬─────────────────┐
           ▼               ▼                ▼                 ▼
     MCP stdio shim    Mobile PWA      Sidebar LLM        CLI `watch`
     @puppet-master/mcp  /bridge proxy   (same HTTP)     long-poll wait
```

Two APIs, same panes:

| Path | MCP mode | Use |
|------|----------|-----|
| **Agent** — `run_agent`, `wait_agents`, `send_message`, `followup_task` | `agent` (default) | Delegate a task to a **worker handle**; persist an operation; wait for a result |
| **Shell** — `spawn_agent`, `write_terminal_input`, `press_key` | `shell` or `both` | Drive a **pane id** like a keyboard |

`spawn_agent` only opens a pane. Prefer `run_agent` for implementation work.

Worker backends: `opencode_native` (OpenCode HTTP API + attached TUI) or screen-scraped / headless CLIs. Details: [docs/wiki/architecture.md](docs/wiki/architecture.md).

---

## Quick start

```bash
npm install
npm run dev          # Tauri desktop + Vite + embedded bridge
```

That is the full stack. Put agent CLIs on `PATH` (`claude`, `codex`, `opencode`, `cursor-agent`) so preset spawns work.

### Connect an MCP host

The GUI must be running. Register `@puppet-master/mcp` — see [MCP_HOSTS.md](MCP_HOSTS.md).

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

For local development, point the host at `packages/mcp-server/dist/puppet-master-mcp.exe` after `npm run build:mcp`. Local editor config is not committed — [CONTRIBUTING.md](CONTRIBUTING.md).

New connections start in **agent mode**. Terminal tools require `set_mode({ "mode": "shell" })` or `"both"`: [agent and shell modes](docs/orchestrator/agent-shell-modes.md).

### Orchestrator loop

```
bridge_health
run_agent { task }           → handle  (or WAIT_TIMEOUT + watch_command)
wait_agents { handles }      → result / needs_input
send_message | followup_task | answer_prompt
close_agent
```

Playbooks: [docs/orchestrator](docs/orchestrator/README.md). Cheatsheet: [mcp-cheatsheet.md](docs/orchestrator/mcp-cheatsheet.md).

---

## Prerequisites

| Tool | Windows | macOS | Linux |
|------|---------|-------|-------|
| Node | 22.x | 22.x | 22.x |
| Rust | 1.96+ (`rustup`) | 1.96+ | 1.96+ |
| Build tools | MSVC 14.51+, Windows SDK 10.0.26100+ | Xcode CLT | `build-essential` |

---

## Coordination model

- **Orchestrator** — plans, delegates, waits, unblocks. Does not implement the change in the target repo.
- **Workers** — Claude, Codex, OpenCode, Cursor Agent, or a shell pane. They edit files and report back on the run.
- **Operations** — durable JSON snapshots of each turn under `<project>/.puppet-master/operations/`.
- **Tasks / locks / context packs** — project-scoped coordination so two workers do not collide.

State lives in the **target** project, not this repo:

```text
<project>/.puppet-master/
  events.jsonl
  operations/<id>.json
  operations/<id>.transcript.jsonl
  messages/
```

Add `.puppet-master/` to that project's `.gitignore`. Safe to delete when no run is active. Finished operations are kept; filter with `list_agents` (`active_only`, `ended_within_ms`).

App data (port file, settings, pairing):

- Windows: `%APPDATA%\com.puppetmaster.app`
- macOS: `~/Library/Application Support/com.puppetmaster.app`

---

## Docs

| Doc | Audience |
|-----|----------|
| [docs/wiki](docs/wiki/README.md) | Developers and coding agents — architecture, concepts, how to keep building |
| [docs/orchestrator](docs/orchestrator/README.md) | External MCP hosts driving workers |
| [MCP_HOSTS.md](MCP_HOSTS.md) | Register the MCP server |
| [ROUTING.md](ROUTING.md) | Sidebar: API vs CLI orchestrator backends |
| [docs/REPO_MAP.md](docs/REPO_MAP.md) | File/function map (agents: read before grepping) |
| [ROADMAP.md](ROADMAP.md) | Planned work |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Local config is not committed |

---

## Monorepo

```
puppet-master/
├── packages/
│   ├── app/            # Tauri 2 + React + PWA; Rust bridge + PTY + run engine
│   ├── mcp-server/     # @puppet-master/mcp (stdio launcher + Rust exe)
│   ├── cli/            # puppet-master bin (gui, mcp, worker, watch)
│   └── shared/         # Zod schemas, agent types, port-file helpers
├── docs/wiki/          # Product + development handbook
├── docs/orchestrator/  # MCP playbooks
└── scripts/            # MCP rebuild, bundles, live regression
```

The HTTP bridge is Rust inside `packages/app/src-tauri`. There is no `packages/bridge`.

---

## Uninstall

| Install type | How to remove |
|--------------|---------------|
| **Windows installer** | Settings → Apps → Puppet Master → Uninstall, or in-app **Settings → Advanced → Uninstall** |
| **macOS** | In-app uninstall, or drag **Puppet Master.app** to Trash |
| **Dev build** (`npm run dev`) | Stop the app and delete the repo folder |

App data may remain in the folders above. Delete those for a clean removal.

On launch, the desktop app checks [GitHub Releases](https://github.com/Potato-dev-inc/puppet-master/releases) and shows a dismissible update banner.

---

## Build and test

```bash
npm test
npm run typecheck

cd packages/app/src-tauri && cargo test --lib

npm run build:rust          # desktop installers
SKIP_RELEASE_UPLOAD=1 npm run build:rust   # build without GitHub upload
```

Windows produces NSIS (`.exe`) and WiX (`.msi`); macOS produces `.dmg`. Artifacts: `packages/app/src-tauri/target/release/bundle/`. Upload only: `npm run publish:release-assets` ([GitHub CLI](https://cli.github.com/) required).

Day-to-day rebuild, MCP exe swap, and `SCHEMA_DRIFT`: [docs/wiki/developing.md](docs/wiki/developing.md).

---

## License

MIT — see [LICENSE](LICENSE).
