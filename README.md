# Puppet Master

**Multi-agent terminal orchestrator.** Spawn real PTY sessions for Claude Code, Codex CLI, OpenCode, Cursor, PowerShell, and Bash — then drive them from a Tauri desktop app, a mobile PWA, or any external MCP host (Cursor, Claude Desktop, Codex).

One HTTP bridge. One tool surface. Every client talks to the same panes.

---

## What you get

| Surface | Role |
|---------|------|
| **Desktop app** | Scrollable terminal grid + Puppet Master chat sidebar |
| **Mobile PWA** | Mirror panes over the bridge; orchestrate from your phone |
| **`@puppet-master/mcp`** | Stdio MCP package for Cursor / Claude Desktop / Codex |
| **Built-in LLM chat** | Sidebar loop (Anthropic / OpenAI / OpenRouter) calling the same bridge tools |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Puppet Master desktop (Tauri)                       │
│  ┌─────────────────────────────┐  ┌──────────────────────────────────────┐  │
│  │  Terminal grid (xterm.js)   │  │  Puppet Master sidebar               │  │
│  │  ┌──────┐ ┌──────┐ ┌──────┐ │  │  • LLM orchestrator (API loop)       │  │
│  │  │Claude│ │ Codex│ │ Bash │ │  │  • MCP activity log (SSE)            │  │
│  │  └──────┘ └──────┘ └──────┘ │  │  • Settings / custom models          │  │
│  └─────────────────────────────┘  └──────────────────────────────────────┘  │
│                              │ Tauri commands / events                      │
│  ┌───────────────────────────▼──────────────────────────────────────────┐ │
│  │ Rust PaneRegistry  (portable-pty — ConPTY on Windows, POSIX elsewhere) │ │
│  │   spawn · write · read · resize · kill · bounded scrollback (10k)    │ │
│  └───────────────────────────┬──────────────────────────────────────────┘ │
└──────────────────────────────┼──────────────────────────────────────────────┘
                               │ embedded on startup
┌──────────────────────────────▼──────────────────────────────────────────────┐
│ Local HTTP bridge  (Node, stdlib http)          127.0.0.1:17321–17399         │
│   GET  /health · /panes · /panes/:id/buffer · /agent-contexts                 │
│   POST /panes · /panes/:id/input                                              │
│   SSE  /events  (pane + tool activity)                                        │
│   writes puppet-master.bridge.port so MCP clients auto-discover the port      │
└──────────────┬───────────────────────────────┬────────────────────────────────┘
               │                               │
    ┌──────────▼──────────┐         ┌──────────▼──────────────────────────────┐
    │ Mobile PWA          │         │ External MCP hosts                      │
    │ (Vite, mirror mode) │         │ Cursor · Claude Desktop · Codex CLI     │
    │ same-origin /bridge │         │   └─► @puppet-master/mcp (stdio)        │
    │ proxy or tunnel URL │         │         └─► HTTP ──► bridge             │
    └─────────────────────┘         └───────────────────────────────────────────┘
```

### Orchestration paths

Every path hits the **same** bridge HTTP API — no duplicate logic.

```
                    ┌─────────────────────────────────────┐
                    │         HTTP bridge (:17321)        │
                    │  list · spawn · read · write · kill │
                    └────────▲───────────────▲──────────┘
                             │               │
           ┌─────────────────┘               └─────────────────┐
           │                                                   │
┌──────────▼──────────┐                            ┌───────────▼───────────┐
│  Sidebar API loop   │                            │  External MCP client  │
│  (shipped)          │                            │  (Cursor, etc.)       │
│                     │                            │                       │
│  User prompt        │                            │  Agent decides        │
│    → LLM API        │                            │    → MCP tool calls   │
│    → tool_use       │                            │    → stdio MCP pkg    │
│    → bridge HTTP    │                            │    → bridge HTTP      │
│    → PTY panes      │                            │    → PTY panes        │
└─────────────────────┘                            └───────────────────────┘

Planned: CLI orchestrator pane (Claude / Codex / OpenCode TUI + MCP) — see ROUTING.md
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
 SSE /events ◄──────────────────────────┘
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

**External MCP only** (Cursor / Claude Desktop while the GUI is running):

```bash
npm run mcp
```

Register `@puppet-master/mcp` in your host — see [MCP_HOSTS.md](MCP_HOSTS.md).

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
│   ├── bridge/         # Local HTTP daemon + SSE (shared protocol)
│   ├── mcp-server/     # @puppet-master/mcp — stdio MCP for external hosts
│   ├── cli/            # puppet-master bin (launches the GUI)
│   └── app/            # Tauri 2 + React + PWA
│       ├── src/        # UI, terminal layer, orchestrator loops
│       └── src-tauri/  # Rust PTY manager + bridge lifecycle
├── ROUTING.md          # Sidebar orchestration: API vs CLI backends
├── MCP_HOSTS.md        # Cursor / Claude Desktop / Codex setup
└── scripts/            # Bridge + MCP smoke tests (PowerShell)
```

---

## MCP tools

Whether you call from the built-in sidebar or an external host, the tool surface is identical.

**Recommended flow for external orchestrators:** `bridge_health` → `list_panes` → `read_agent_context` → delegate → `read_terminal_buffer` once to confirm.

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

### Why a Node bridge between PTY and MCP?

Tauri owns OS processes. External MCP clients speak **stdio JSON-RPC**. The bridge is a thin HTTP shim on `127.0.0.1` so both the GUI and `@puppet-master/mcp` share one API. When the GUI is not running, the MCP package fails fast instead of hanging.

### Scrollback safety

Rust accumulates all PTY output in a bounded deque (10k lines) alongside xterm scrollback. MCP `read_terminal_buffer` works even when the pane is off-screen or the GUI is hidden — clients can read history from before they attached.

### Dual orchestration

- **In-app sidebar** — see agents, interject manually, stream tool log.
- **External MCP** — let Cursor / Claude Desktop orchestrate your panes directly.
- **Mobile PWA** — monitor and steer from a phone via tunnel or same-origin proxy.

Details on sidebar routing (API vs planned CLI backends): [ROUTING.md](ROUTING.md).

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

## Publishing

```bash
# MCP package
cd packages/mcp-server && npm version patch && npm publish --access public

# CLI (spawns GUI via npx)
cd packages/cli && npm version patch && npm publish --access public

# Desktop bundle
npm run build:rust
# → packages/app/src-tauri/target/release/bundle/
```

---

## License

MIT — see [LICENSE](LICENSE).
