# Architecture

Puppet Master is a **hub-and-spoke** desktop process. The hub is an embedded loopback HTTP bridge. Every client is a spoke that talks to the same `PaneRegistry` and the same operation store.

```
┌─────────────────────────────────────────────────────────────┐
│  Puppet Master desktop (Tauri 2)                            │
│  React UI  (packages/app/src)                               │
│  Rust lib  (packages/app/src-tauri)  ← PaneRegistry + bridge│
└──────────────────────────┬──────────────────────────────────┘
                           │ 127.0.0.1:17321–17399
           ┌───────────────┼────────────────┬─────────────────┐
           ▼               ▼                ▼                 ▼
     MCP stdio shim    Mobile PWA      Sidebar LLM        CLI `watch`
     puppet-master-mcp  /bridge proxy   (same HTTP)     long-poll wait
```

There is no second control plane. If the GUI is not running, MCP fails fast (`BRIDGE_UNAVAILABLE`).

## Layers

| Layer | Lives in | Owns |
|-------|----------|------|
| UI | `packages/app/src` | Grid, sidebar, PWA shell. Mirrors Rust types in `lib/tauri.ts` / `packages/shared`. |
| Tauri commands | `commands.rs` | Thin wrappers over `PaneRegistry` for the desktop window. |
| HTTP bridge | `bridge.rs` | Route table, SSE, session gate, operation publish. |
| Run engine | `agent_runs/` | `run_agent`, wait, steer, completion, checks. |
| Operations | `operations.rs` + `operations/` | Durable JSON snapshots, idempotency, wait condvar. |
| TUI profiles | `tui/` | Per-agent launch, screen, prompts, result extraction. |
| PTY | `pty/` | Spawn, read, write, classify pane status. |
| OpenCode native | `opencode/` | `opencode serve` + REST; not screen scraping. |
| MCP shim | `bin/puppet-master-mcp.rs` | stdio JSON-RPC → HTTP. **No state.** |
| CLI | `packages/cli` | `gui`, `mcp`, `worker`, `watch`. |

## Two APIs, same panes

Orchestrators should not mix these casually:

| Path | Mode | Use |
|------|------|-----|
| **Agent API** — `run_agent`, `wait_agents`, `send_message`, `followup_task`, … | `agent` (default) | Delegate a task to a **worker handle**. Persist an operation, wait for a result with provenance. |
| **Shell / pane API** — `spawn_agent`, `write_terminal_input`, `press_key`, `read_terminal_buffer`, `shell_exec` | `shell` or `both` | Drive a **pane id** like a keyboard. Evidence is whatever you scrape. |

`spawn_agent` only opens a pane. It does not create a worker turn. Prefer `run_agent` for implementation work.

Request flow for the agent API: MCP tool → shim (`call_tool`) → `bridge::route` → `mcp_sessions::authorize` → `agent_runs::handle_request`. Full turn: [run-lifecycle.md](run-lifecycle.md).

## Project-local state

Scoped to the selected project folder, not the Puppet Master repo:

```text
<project>/.puppet-master/
  events.jsonl              append-only coordination log
  operations/<id>.json      one snapshot per operation
  operations/<id>.transcript.jsonl
  messages/                 steer receipts
```

Gitignore it in the **target** project. Safe to delete when no run is active. Finished operations are kept; filter with `list_agents` (`active_only`, `ended_within_ms`).

App data (port file, settings, pairing) is separate:

- Windows: `%APPDATA%\com.puppetmaster.app`
- macOS: `~/Library/Application Support/com.puppetmaster.app`

The chosen bridge port is written to `puppet-master.bridge.port` in that directory so MCP clients can find it.

## Catalog and schema drift

`tool_registry.rs` is the **only** MCP tool catalog: name, description, HTTP method/path, schemas, visibility (agent / shell / both). `catalog_version` is a hash of that catalog (`<count>-<16 hex>`).

The shim embeds its own copy at compile time. If the running app and the MCP exe disagree, `bridge_health` reports `SCHEMA_DRIFT`. Rebuild both (see [developing.md](developing.md)).

## Mobile

The PWA does not spawn PTYs. It proxies `/bridge/*` to the desktop bridge (loopback or cloudflared tunnel) after Ed25519 pairing (`mobile_pairing.rs`). Desktop owns dimensions; the phone sends input and renders output.

## What this is not

A detached headless daemon (`puppet-master-daemon`, `bridge_host.rs`, …) is **not on this branch**. The MCP shim requires the desktop app. Treat memory or old plans that mention a daemon as absent until the code exists.
