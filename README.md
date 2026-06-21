# Puppet Master

**Multi-agent terminal orchestrator.** Spawn real PTY sessions for Claude Code, Codex CLI, OpenCode, Cursor, PowerShell, and Bash — then drive them from a Tauri desktop app, a mobile PWA, or any external MCP host (Cursor, Claude Desktop, Codex).

Puppet Master is the coordination layer: it behaves like a senior engineer at the keyboard, breaking work into tasks, assigning worker panes, enforcing resource locks, handing off compact context packs, and watching panes for prompts or blockers. Worker agents do the coding, shell work, tests, and file inspection.

One HTTP bridge. One tool surface. Every client talks to the same panes and the same project-local coordination state.

---

## What you get

| Surface | Role |
|---------|------|
| **Desktop app** | Scrollable terminal grid + Puppet Master chat sidebar |
| **Mobile PWA** | Mirror panes over the bridge; orchestrate from your phone |
| **`@puppet-master/mcp`** | Stdio MCP package for Cursor / Claude Desktop / Codex |
| **Built-in LLM chat** | Sidebar loop (Anthropic / OpenAI / OpenRouter) calling the same bridge tools |
| **Coordination kernel** | Project-scoped tasks, locks, audit log, context packs, and pane timeline replay |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Puppet Master desktop (Tauri)                       │
│  ┌─────────────────────────────┐  ┌──────────────────────────────────────┐  │
│  │  Terminal grid (xterm.js)   │  │  Puppet Master sidebar               │  │
│  │  ┌──────┐ ┌──────┐ ┌──────┐ │  │  • LLM orchestrator (API or CLI)     │  │
│  │  │Claude│ │ Codex│ │ Bash │ │  │  • MCP activity log (SSE)            │  │
│  │  └──────┘ └──────┘ └──────┘ │  │  • Tasks / locks / context packs     │  │
│  └─────────────────────────────┘  └──────────────────────────────────────┘  │
│                              │ Tauri commands / events                      │
│  ┌───────────────────────────▼──────────────────────────────────────────┐   │
│  │ Rust PaneRegistry + coordination event log                           │   │
│  │   spawn · write · read · resize · kill · task/lock projections       │   │
│  └───────────────────────────┬──────────────────────────────────────────┘   │
└──────────────────────────────┼──────────────────────────────────────────────┘
                               │ embedded on startup
┌──────────────────────────────▼────────────────────────────────────────────────┐
│ Local HTTP bridge  (Rust, stdlib TCP)          127.0.0.1:17321–17399          │
│   GET  /health · /panes · /tasks · /locks · /audit · /agent-contexts          │
│   POST /panes · /panes/:id/input · /tasks · /locks · /context-packs           │
│   SSE  /events  (pane, status, settings, chat, terminal, tool activity)       │
│   writes puppet-master.bridge.port so MCP clients auto-discover the port      │
└──────────────┬───────────────────────────────┬────────────────────────────────┘
               │                               │
    ┌──────────▼──────────┐         ┌──────────▼──────────────────────────────┐
    │ Mobile PWA          │         │ External MCP hosts                      │
    │ (Vite, mirror mode) │         │ Cursor · Claude Desktop · Codex CLI     │
    │ same-origin /bridge │         │   └─► @puppet-master/mcp (stdio)        │
    │ proxy or tunnel URL │         │         └─► HTTP ──► bridge             │
    └─────────────────────┘         └─────────────────────────────────────────┘
```

### Orchestration paths

Every path hits the **same** bridge HTTP API — no duplicate logic.

```
                    ┌─────────────────────────────────────┐
                    │         HTTP bridge (:17321)        │
                    │ list · spawn · read · write · tasks │
                    │ locks · context · keypress · kill   │
                    └────────▲───────────────▲────────────┘
                             │               │
           ┌─────────────────┘               └─────────────────┐
           │                                                   │
┌──────────▼──────────┐                            ┌───────────▼───────────┐
│  Sidebar API loop   │                            │  External MCP client  │
│  or CLI orchestrator│                            │  (Cursor, etc.)       │
│                     │                            │                       │
│  User prompt        │                            │  Agent decides        │
│    → LLM API        │                            │    → MCP tool calls   │
│    → tool_use       │                            │    → stdio MCP pkg    │
│    → bridge HTTP    │                            │    → bridge HTTP      │
│    → PTY panes      │                            │    → PTY panes        │
└─────────────────────┘                            └───────────────────────┘

CLI orchestrator panes use the same MCP bridge as external hosts. See [ROUTING.md](ROUTING.md).
```

### Mobile mirror mode

The PWA does not own PTY processes. It mirrors desktop panes over the bridge.

```
 Phone keyboard                Bridge / desktop PTY
 ─────────────                 ───────────────────

 xterm hidden textarea
       │
       ├─ keystrokes ──────────► POST /panes/:id/input ──► real shell
       │
       └─ local echo (mirror)     PTY output
                                         │
 SSE /events ◄───────────────────────────┘
       │
       └─► xterm display (deduped echo)
```

Desktop keeps PTY dimensions; mobile sends input and renders output without resizing the remote session.

---

## Quick start

```bash
# from repo root
npm install
npm run dev          # alias for: npm run tauri dev
```

That launches the full stack: shared build, Vite/PWA, Tauri, and the embedded bridge.

### Uninstall

| Install type | How to remove |
|--------------|---------------|
| **Windows installer** (`Puppet Master_*_x64-setup.exe`) | **Settings → Advanced → Uninstall Puppet Master**, or Windows **Settings → Apps → Installed apps → Puppet Master → Uninstall** |
| **macOS** (`.app` in Applications) | **Settings → Advanced → Uninstall**, or drag **Puppet Master.app** to Trash |
| **Dev build** (`npm run tauri dev`) | Stop the app and delete the repo folder — there is no system uninstaller |

App data (settings, bridge port file, mobile pairing) may remain after uninstall:

- Windows: `%APPDATA%\com.puppetmaster.app`
- macOS: `~/Library/Application Support/com.puppetmaster.app`

Delete that folder if you want a clean removal.

On launch, the desktop app checks [GitHub Releases](https://github.com/Potato-dev-inc/puppet-master/releases) for a newer version and shows a dismissible banner when an update is available.

**External MCP only** (Cursor / Claude Desktop while the GUI is running):

```bash
npm run mcp
```

Register `@puppet-master/mcp` in your host — see [MCP_HOSTS.md](MCP_HOSTS.md).
Local agent/editor config is ignored; see [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Prerequisites

| Tool | Windows | macOS | Linux |
|------|---------|-------|-------|
| Node | 22.x | 22.x | 22.x |
| Rust | 1.96+ (`rustup`) | 1.96+ | 1.96+ |
| Build tools | MSVC 14.51+, Windows SDK 10.0.26100+ | Xcode CLT (`xcode-select --install`) | `build-essential` |

Agent presets resolve binaries for your OS (`claude.exe` vs `claude`, etc.). Put Cursor, Codex, OpenCode, and Claude Code on `PATH` for preset spawns to work.

---

## Monorepo layout

```
puppet-master/
├── packages/
│   ├── shared/         # Zod schemas, agent presets, pane types, port-file reader
│   ├── bridge/         # Legacy bridge package and shared bridge protocol helpers
│   ├── mcp-server/     # @puppet-master/mcp — stdio MCP for external hosts
│   ├── cli/            # puppet-master bin (launches the GUI)
│   └── app/            # Tauri 2 + React + PWA
│       ├── src/        # UI, terminal layer, orchestrator loops
│       └── src-tauri/  # Rust PTY manager, HTTP bridge, coordination kernel
├── ROUTING.md          # Sidebar orchestration: API vs CLI backends
├── MCP_HOSTS.md        # Cursor / Claude Desktop / Codex setup
├── docs/design/        # Static design prototypes
└── scripts/            # Bridge + MCP smoke tests (PowerShell)
```

---

## Coordination model

Puppet Master separates coordination from implementation:

- **Orchestrator**: acts as the senior engineer. It creates tasks, claims or assigns ownership, builds context packs, sends prompts/keys to worker panes, monitors status, and summarizes evidence. It must not directly edit project files or run project tests itself.
- **Workers**: Claude Code, Codex, OpenCode, Bash, PowerShell, or Cursor panes. Workers inspect files, write code, run commands, and report results back through the terminal.
- **Tasks**: durable work items rebuilt from the project event log. Tasks can be claimed, completed, blocked, assigned reviewers, and given leases.
- **Locks**: exclusive ownership records for files, directories, commands, ports, branches, or panes. Locks prevent two workers from editing or controlling the same resource at once.
- **Context packs**: compact handoff prompts built from the selected task, current locks, manager instructions, constraints, and evidence requirements. They are generated on demand and can be cleared from the UI.
- **Audit log**: every task, lock, tool, pane, and observation event is append-only and replayed into read models.

Coordination state is scoped per project. When a project folder is selected, tasks, locks, audit entries, and pane timeline events are written to:

```text
<project>/.puppet-master/events.jsonl
```

The folder is ignored by this repository and should generally stay out of source control.

---

## MCP tools

Whether you call from the built-in sidebar or an external host, the tool surface is identical.

**Recommended flow for external orchestrators:** `bridge_health` → `create_task` → `acquire_resource_lock` → `build_context_pack` → `spawn_agent` or `list_panes` → delegate with `write_terminal_input` → monitor with `read_terminal_buffer` → `complete_task` with evidence.

| Tool | What it does |
|------|--------------|
| `bridge_health` | Confirm the local HTTP bridge is reachable |
| `list_panes` | Live panes: id, agent type, pid, status, cwd, size |
| `list_agent_contexts` | Supported agents with strengths and routing hints |
| `read_agent_context` | Agent profile or live pane context + buffer preview |
| `inspect_agent_model` | Parse recent output for active model signal |
| `spawn_agent` | New pane — `claude`, `codex`, `opencode`, `powershell`, `bash`, `cursor` |
| `read_terminal_buffer` | Scrollback (last N lines, default 200) |
| `write_terminal_input` | Send text as if typed (appends Enter by default) |
| `press_key` | Send named TUI keys such as `enter`, arrows, `escape`, `y`, `n`, or `ctrl+c` |
| `create_task` | Create a coordination task before delegating implementation work |
| `claim_task` | Claim or renew a task lease for a worker |
| `report_task_status` | Mark delegated work as in progress, blocked, or otherwise updated |
| `complete_task` | Complete a task with evidence from the worker |
| `list_tasks` | Rebuild and list project-local task projections |
| `acquire_resource_lock` | Claim exclusive ownership of a file, directory, command, port, branch, or pane |
| `release_resource_lock` | Release a lock owned by a worker |
| `build_context_pack` | Build a compact handoff prompt from task, locks, constraints, and scrollback |
| `kill_pane_process` | Terminate pane and child process |

---

## Pane status

Each pane gets a live status from recent output. The header LED reflects it.

| Status | When |
|--------|------|
| `running` | Child alive + output in last 5s |
| `waiting_input` | Output matches prompt heuristics (`y/n`, `continue?`, …) |
| `idle` | Child alive, quiet for 5s |
| `error` | Child exited or read error |

---

## Design notes

### Why a local bridge between PTY and MCP?

Tauri owns OS processes. External MCP clients speak **stdio JSON-RPC**. The embedded bridge is a thin local HTTP API on `127.0.0.1` so the GUI, mobile PWA, CLI orchestrator panes, and `@puppet-master/mcp` share one control plane. When the GUI is not running, the MCP package fails fast instead of hanging.

### Scrollback safety

Rust accumulates all PTY output in a bounded deque (10k lines) alongside xterm scrollback. MCP `read_terminal_buffer` works even when the pane is off-screen or the GUI is hidden — clients can read history from before they attached.

### Project-local memory

Tasks, locks, audit entries, and pane timeline events are event-sourced from `<project>/.puppet-master/events.jsonl`. Switching the project folder switches the coordination board, so different repos do not share locks or task history.

### Permission and action prompts

The desktop harness watches live pane events and classifies terminal output. Routine permission prompts can be auto-approved without waking the orchestrator. Ambiguous menus, substantive worker questions, terminal errors, and completion reports wake the orchestrator so it can choose the next keypress or prompt.

### Dual orchestration

- **In-app sidebar** — see agents, interject manually, stream tool log.
- **External MCP** — let Cursor / Claude Desktop orchestrate your panes directly.
- **CLI orchestrator pane** — run Claude, Codex, or OpenCode as the manager while it controls workers through MCP.
- **Mobile PWA** — monitor and steer from a phone via tunnel or same-origin proxy.

Details on sidebar routing and backend selection: [ROUTING.md](ROUTING.md).

---

## Smoke testing

```bash
npm run build

# terminal 1 — bridge only
npm run bridge

# terminal 2 — HTTP
curl http://127.0.0.1:17321/health
curl -X POST http://127.0.0.1:17321/panes \
  -H 'Content-Type: application/json' -d '{"agent_type":"bash"}'

# terminal 3 — MCP stdio
npm run mcp
```

Or run `scripts/test-bridge.ps1` and `scripts/test-mcp.ps1`.


---

## License

MIT — see [LICENSE](LICENSE).
