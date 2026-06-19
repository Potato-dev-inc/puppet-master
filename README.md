# Puppet Master

Multi-agent terminal orchestrator. Spawn multiple AI coding agents — **Claude Code, Codex CLI, OpenCode CLI**, plus generic shells (PowerShell, Bash) — as real PTY terminals in a Tauri desktop app, and orchestrate them via MCP from any external host.

- **Built-in LLM chat** in the right sidebar drives the same agents using your Anthropic or OpenAI key.
- **`@puppet-master/mcp`** is a publishable stdio MCP server that registers in **Cursor**, **Claude Desktop**, and **Codex CLI** and gives them pane control plus agent-context/model-inspection tools.

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│ Tauri desktop app  (React + Vite + xterm.js + Rust PTY manager)    │
│  ┌───────────────────────────┐  ┌─────────────────────────────────┐│
│  │  scrollable 2-col grid    │  │  Puppet Master chat (LLM)       ││
│  │  ┌─────┐ ┌─────┐          │  │  - Claude / OpenAI              ││
│  │  │ P1  │ │ P2  │          │  │  - MCP log feed                 ││
│  │  └─────┘ └─────┘          │  └─────────────────────────────────┘│
│  └───────────────────────────┘                                     │
│         │ Tauri events / commands                                  │
│  ┌──────▼──────────────────┐                                       │
│  │ Rust PaneRegistry       │ ← portable-pty (ConPTY / POSIX PTY)   │
│  │  spawn / write / read   │                                       │
│  │  / kill / resize        │                                       │
│  └─────────────────────────┘                                       │
└─────────┬──────────────────────────────────────────────────────────┘
          │ spawns on startup
┌─────────▼──────────────┐
│ Local HTTP bridge      │  http://127.0.0.1:17321–17399
│ (Node, stdlib http)    │  - /panes /panes/:id/buffer /panes/:id/input
│  + SSE /events         │  - /agent-contexts /panes/:id/model /health
└─────────┬──────────────┘
          │ same pane + agent-context HTTP API
┌─────────▼─────────────────────────────────────────────────────────┐
│ External MCP clients (Cursor, Claude Desktop, Codex)              │
│   via `@puppet-master/mcp` stdio package  →  HTTP  →  bridge      │
└───────────────────────────────────────────────────────────────────┘
```

## Prerequisites

| Tool | Windows | macOS | Linux |
|------|---------|-------|-------|
| Node | 22.x | 22.x | 22.x |
| Rust | 1.96+ (`rustup`) | 1.96+ (`rustup`) | 1.96+ (`rustup`) |
| Build tools | MSVC 14.51+, Windows SDK 10.0.26100+ (VS Build Tools → "Desktop development with C++") | Xcode Command Line Tools (`xcode-select --install`) | `build-essential` (or distro equivalent) |

Agent presets resolve commands automatically for your OS (e.g. `claude.exe` on Windows, `claude` on macOS/Linux). Cursor / Codex / OpenCode / Claude Code CLIs must be on `PATH` for the presets to spawn them.

## Quick start

```bash
# from the repo root
npm install

# launch the full dev stack (shared build + vite/PWA + tauri + embedded bridge)
npm run tauri dev
# same as:
npm run dev

# external MCP stdio server (only needed for Cursor / Claude Desktop outside the app):
npm run mcp
```

## Layout

```
puppet-master/
├── packages/
│   ├── shared/         # zod schemas, agent presets, pane types, port-file reader
│   ├── cli/            # `puppet-master` bin entry
│   ├── bridge/         # local HTTP/WS daemon (shared protocol)
│   ├── mcp-server/     # @puppet-master/mcp — stdio MCP for external hosts
│   └── app/            # Tauri 2 + React frontend
│       ├── src/        # UI (WorkspaceHeader, TerminalGrid, TerminalPane, PuppetMasterSidebar, SettingsPanel)
│       └── src-tauri/  # Rust PTY manager + bridge lifecycle
└── scripts/            # smoke-test PowerShell scripts
```

## MCP tools

Every agent pane is reachable through the same tool surface, whether you call it from the built-in Puppet Master LLM or from an external MCP host.

External orchestrators such as Cursor should start each task with `bridge_health`
and `list_panes`, then reuse live panes where possible. Before delegating to a
pane, read its context with `read_agent_context`; when choosing between panes,
use `inspect_agent_model` to compare model hints and routing score.

| Tool | What it does |
|------|--------------|
| `bridge_health` | Confirm the local HTTP bridge is reachable |
| `list_panes` | List live panes (id, agent type, pid, status, cwd, size) |
| `list_agent_contexts` | List supported agents with strengths, smartness score, and planned sidebar actions |
| `read_agent_context` | Read an agent type profile or a live pane context with recent buffer preview |
| `inspect_agent_model` | Inspect recent terminal output for the active model signal |
| `spawn_agent` | Spawn a new pane — pick from `claude`, `codex`, `opencode`, `powershell`, `bash`, `cursor` |
| `read_terminal_buffer` | Read recent scrollback (last N lines, default 200) |
| `write_terminal_input` | Send text to a pane as if typed. Default appends Enter |
| `kill_pane_process` | Terminate a pane and its child process |

External hosts see exactly the same surface — see [MCP_HOSTS.md](MCP_HOSTS.md) for Cursor / Claude Desktop / Codex configuration.

## Status heuristics

Each pane has a status derived from recent output:

| Status | When |
|--------|------|
| `running` | Child alive + recent output (last 5s) |
| `waiting_input` | Recent output matches prompt regex (`y/n`, `press enter`, `continue?`, …) |
| `idle` | Child alive, no output in 5s |
| `error` | Child exited or read error |

The LED in the pane header bar reflects this in real time.

## Architecture details

### Why a Node HTTP bridge between the GUI and external MCP?

- Tauri's PTY manager owns the OS processes; external MCP clients (Cursor / Claude Desktop) speak **stdio JSON-RPC**.
- The Node bridge is a thin shim that exposes the pane and agent-context API over HTTP on `127.0.0.1`.
- The bridge writes a port file (`puppet-master.bridge.port`) on startup so the MCP package can find it.
- When the GUI is **not** running, `@puppet-master/mcp` returns a clear error rather than hanging.

### Why dual orchestration?

- **Built-in Puppet Master**: keeps the user in the GUI; you can see the agents' state and interject manually.
- **External MCP**: lets you point Cursor / Claude Desktop / Codex at your panes and have **them** orchestrate.
- Both call the **same** bridge HTTP API — no duplicate logic, no chance of drift.

### Frontend roadmap

- The current frontend remains React + Vite because it is the simplest fit for Tauri's desktop webview build.
- A Next.js migration is possible, but should be done as a focused pass using static export (`output: 'export'`) or a Tauri-compatible custom dev URL. It will require replacing `vite.config.ts`, updating Tauri `beforeDevCommand` / `beforeBuildCommand`, and checking asset routing inside the desktop bundle.
- The first design pass now uses monochrome light/dark CSS tokens, a resizable sidebar, independently resizable terminal panes, and a sidebar roadmap for direct Codex / Claude Code / OpenCode orchestration.

### Scrollback safety

The Rust PTY manager accumulates **all** output in a bounded `VecDeque<String>` (10k lines cap) inside the registry, in addition to xterm.js's own scrollback. This means:

- The MCP `read_terminal_buffer` tool works even if the GUI is hidden / scrolled away.
- MCP clients can read what happened **before** they attached.
- xterm never loses data when panes scroll off-screen.

## Publishing (phase 2)

```bash
# 1. publish the MCP package
cd packages/mcp-server
npm version patch
npm login
npm publish --access public

# 2. publish the CLI (which spawns the GUI via npx)
cd ../cli
npm version patch
npm login
npm publish --access public

# 3. build the Tauri bundle (.msi)
cd ../..
npm run build:rust
# Output: packages/app/src-tauri/target/release/bundle/msi/*.msi
```

## Smoke testing the bridge / MCP layer

```bash
# build everything
npm run build

# in one terminal — start the bridge
npm run bridge

# in another terminal — exercise the HTTP API
curl http://127.0.0.1:17321/health
curl -X POST http://127.0.0.1:17321/panes -H 'Content-Type: application/json' \
  -d '{"agent_type":"powershell"}'
curl http://127.0.0.1:17321/panes

# in another terminal — exercise the MCP layer
npm run mcp
# then in another:
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' | npm run -s mcp
```

Or use the included PowerShell scripts:

```powershell
pwsh -File scripts/test-bridge.ps1   # HTTP API round-trip
pwsh -File scripts/test-mcp.ps1      # MCP JSON-RPC round-trip
```

## License

MIT — see [LICENSE](LICENSE).
