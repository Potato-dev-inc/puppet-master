# Puppet Master wiki

In-repo handbook for **developers** and **coding agents**. Use this to understand the product, keep building it, and (from the overview below) draft or refresh the root `README.md`.

When this wiki disagrees with the code, the **code wins** — fix the wiki. File-level maps and error codes live in [REPO_MAP.md](../REPO_MAP.md), not here.

## Pages

| # | Page | Read it when you need |
|---|------|------------------------|
| 1 | [Overview](README.md) (this page) | What the product is; README source |
| 2 | [Architecture](architecture.md) | How the pieces connect |
| 3 | [Concepts](concepts.md) | Handles, panes, operations, ownership, provenance |
| 4 | [Run lifecycle](run-lifecycle.md) | What `run_agent` actually does |
| 5 | [Packages](packages.md) | Monorepo layout |
| 6 | [Developing](developing.md) | Build, test, MCP rebuild, `build_id` |
| 7 | [Extending](extending.md) | Add a tool, add an agent, where to change X |
| 8 | [Glossary](glossary.md) | Shared vocabulary |

Related, not duplicated:

- [docs/orchestrator/](../orchestrator/README.md) — playbooks for **using** Puppet Master as an MCP orchestrator
- [packages/app/src-tauri/src/tui/README.md](../../packages/app/src-tauri/src/tui/README.md) — add-a-TUI checklist
- [MCP_HOSTS.md](../../MCP_HOSTS.md) — register the MCP server with Cursor / Claude / Codex
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — local tool config is not committed

---

## Overview (README seed)

Root [`README.md`](../../README.md) is the GitHub landing page derived from this section. Keep product facts in sync here; keep file maps in REPO_MAP.

### What it is

Puppet Master is a **multi-agent terminal orchestrator**. It hosts real PTY panes for agent CLIs (OpenCode, Cursor Agent, Claude Code, Codex, shells) and exposes one local control plane so a desktop app, a mobile PWA, and any MCP host can drive the same workers.

It is the **coordination layer**, not the coder. An orchestrator (sidebar LLM, CLI manager, or external MCP agent) breaks work into tasks, assigns workers, waits for results, and unblocks prompts. Workers inspect files, write code, and run tests inside their panes.

### Surfaces

| Surface | Role |
|---------|------|
| Desktop app (Tauri 2 + React) | Terminal grid + orchestrator sidebar |
| Mobile PWA | Mirrors desktop panes over the bridge; does not own PTYs |
| `@puppet-master/mcp` | Stdio MCP shim for Cursor, Claude Desktop, Codex, … |
| `puppet-master` CLI | Launch GUI / worker window; `watch` long-polls a running operation |

One HTTP bridge. One tool catalog. Every client talks to the same panes and the same project-local state.

### Control plane (one paragraph)

The Tauri process embeds a loopback HTTP bridge (`127.0.0.1`, default port `17321`, scans upward). The MCP package is a **stateless stdio shim**: it maps tool calls to that HTTP API. Runs persist as JSON **operations** under `<project>/.puppet-master/operations/`. Backends are either `opencode_native` (OpenCode HTTP API + attached TUI) or screen-scraped / headless CLIs behind a `TuiProfile`.

```
MCP host / sidebar / mobile / CLI watch
        │
        ▼
embedded HTTP bridge  (bridge.rs)
        │
        ├── PaneRegistry     real PTY panes
        ├── agent_runs       run_agent engine
        ├── operations       durable snapshots
        └── mcp_sessions     mode + ownership leases
```

### Quick start

```bash
npm install
npm run dev          # Tauri desktop + Vite + embedded bridge
```

Prerequisites: Node 22, Rust 1.96+, platform build tools (MSVC on Windows, Xcode CLT on macOS, `build-essential` on Linux). Put agent CLIs on `PATH` for preset spawns.

External MCP while the GUI is running: register `@puppet-master/mcp` — see [MCP_HOSTS.md](../../MCP_HOSTS.md). The GUI must be up; the shim does not start a headless daemon.

### Orchestrator vs worker

- **Orchestrator** — creates tasks, delegates with `run_agent`, waits (`wait_agents` / `watch`), answers prompts, does **not** implement the change itself.
- **Worker** — a pane running Claude / Codex / OpenCode / Cursor / a shell. It edits the project and reports back through the run result.

New MCP connections start in **agent mode** (`run_agent`, `wait_agents`, …). Terminal/PTY tools need `set_mode({ "mode": "shell" })` or `"both"`. Details: [agent-shell-modes.md](../orchestrator/agent-shell-modes.md).

### License

MIT — see [LICENSE](../../LICENSE).
