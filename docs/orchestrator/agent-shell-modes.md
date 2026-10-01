# MCP agent and shell modes

The Puppet Master MCP server exposes a per-connection tool catalog. Each stdio server process creates a random session ID and sends it to the local bridge in the `X-Puppet-Master-Session` header. TypeScript MCP clients use the same contract. The bridge keeps each session's selected mode and pane/agent ownership in memory; different MCP connections do not share mode or control state.

New connections start in `agent` mode. Call `set_mode` to choose `agent`, `shell`, or `both`. The server filters `tools/list` and `/mcp/tools` for that session. After a successful change, it sends `notifications/tools/list_changed` so the host can refresh discovery. Calls rejected by the selected catalog return `MODE_MISMATCH` with a `context.switch_with.mode` hint.

Some hosts read the tool list once when they connect and ignore `notifications/tools/list_changed`, so a `set_mode` call never changes what they can call. For those hosts, choose the starting catalog when the server launches: pass `--mode <agent|shell|both>` or set `PUPPET_MASTER_MODE`. The argument wins over the environment variable, an invalid value is logged and ignored, and `set_mode` still works afterward. The starting mode applies to that connection only. If the bridge is not running yet, the server tells it the mode on the first tool call.

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "C:/path/to/puppet-master-mcp.exe",
      "args": ["--mode", "both"]
    }
  }
}
```

`agent` mode exposes worker controls: `set_mode`, `run_agent`, `wait_agents`, `send_message`, `followup_task`, `interrupt_agent`, `inspect_agent`, `agent_transcript`, `close_agent`, plus `list_agents` / `list_workers` (UI panes and managed runs), compatibility wrappers `send_agent`, `cancel_agent`, `answer_prompt`, and `take_over`. `shell` mode exposes `set_mode`, the terminal and pane tools, `shell_exec`, `release`, and `take_over`. `both` exposes the full MCP catalog.

Agent tools use worker handles, not pane ids. `run_agent` starts or adopts a worker (same `name`/`handle` reuses the collaborator unless `context_mode`/`context_policy` is `fresh`) and waits up to `wait_ms` (30 seconds by default, at most 300 seconds) before returning the current state; set `background: true` to return immediately. Default context is a compact packet; inspect reports `context_policy` and `context_continuity` (`resume` | `reconstructed_summary` | `none`). Requested model/reasoning are returned next to the resolved values — a remap is never silent. The MCP connection UUID is a transport id: `session_identity` returns the coordinator id and `attach_token` so a replacement connection can `attach_agents` after reconnect. `release` returns temporary pane control without dropping the worker lease; `release_lease` drops the lease; `transfer_agent` moves ownership to a live `to_session_id`. A coordinator name is not a credential. The operation itself defaults to a 15-minute timeout. `wait_agents` takes `handles`, optional `after_cursor` / `after_revisions`, status filters in `until`, a timeout, and `mode: "any" | "all"`. A completed wait includes `result`, `result_capture` (`authoritative` | `inferred` | `missing`), and `acceptance_status` (`not_checked` unless checks ran). `send_message` steers a live worker; `followup_task` starts the next turn on the same handle. `interrupt_agent` stops the current turn and keeps the worker. `close_agent` ends the worker session. `send_agent` / `cancel_agent` remain wrappers.

`read_terminal_buffer` accepts `view: "screen" | "scrollback"`. Omitting `view` returns the current screen for agent/TUI panes and historical scrollback for shell panes.

Results include `status`, `result`, `verified`, `prompt`, `error`, `pane_id`, `duration_ms`, `revision`, and `stage`. Status is `running`, `needs_input`, `completed`, `failed`, `cancelled`, or `timeout`. A bounded foreground wait can return a still-running handle. Follow-up tasks persist as queued turns behind active work. Native permission choices accept `allow_once` or `deny`; broad permission also requires `allow_broad: true`.

A TUI turn completes when the pane returns to input with new non-chrome output versus the **pre-dispatch** screen, even if a busy state was never observed. Idle with no new output finishes the turn as `result_capture: missing` after a short reconcile window instead of running until timeout. Permission prompts are waiting_input, not completion. Native OpenCode completion requires a fresh assistant message with `finish: "stop"` and no pending question. Headless CLI completion requires structured output and a successful process exit. Truncating a transcript never drops the stored turn result.

`shell_exec` accepts a command, optional working directory, and optional shell pane. Without a pane it creates a shell for the project. It waits for a unique completion marker and returns output, an integer exit code, duration, and working directory. A timeout interrupts the selected shell and reports partial output. Cursor's CLI is `cursor_agent`; `cursor` continues to launch the IDE.

New panes and operations are registered to the creating connection before the worker receives its prompt. An agent handle or managed pane cannot be taken from an active connection by guessing its ID or setting `grant: true`. `take_over` returns the current rendered screen and prompt. A preexisting pane that is not registered to an MCP session requires `pane_id` and `grant: true`. Shell mode can read an agent pane, but it cannot write to it until the connection explicitly takes it over. `release` returns control to the original agent owner.

The bridge enforces both tool mode and session ownership on every MCP-tagged route. Calls without a session header keep legacy pane and operation routes available, but they cannot use the new agent APIs or mutate panes registered to an MCP session. Host-managed schema discovery remains outside Puppet Master's control; the MCP server publishes its filtered catalog and change notifications.

## Verification

Validation passed for the Rust library and MCP tests, stdio mock-bridge smoke,
shared and app tests, TypeScript checks, and the Qt console's 66 tests. The console
has 86% coverage of production Python code. The existing OpenCode executable-path
test was skipped because `opencode.exe` is absent locally. Rust coverage was not
measured because `cargo-llvm-cov` is not installed. Tests do not dispatch paid agents.

The already-running desktop and MCP executables are locked by Windows. Updated
builds are available as `target/debug/puppet_master_app_updated.exe` and
`target/debug/puppet-master-mcp-updated.exe`. Close the current desktop when its
workers can stop, launch the updated desktop, then reconnect MCP clients. The
button console selects the newest local MCP executable automatically. Other hosts
can point at the updated MCP executable until their usual executable can be rebuilt.

The session contract has focused Rust tests for catalog isolation, invalid modes, mode mismatch hints, cross-session handle access, pane registration conflicts, explicit takeover, release, and shell-pane access. The MCP host should exercise discovery, switch each mode, observe `tools/list_changed`, verify that tools disappear from the catalog, and confirm that a guessed direct HTTP route is rejected with `MODE_MISMATCH`.

## Deferred work

Phase 5 remains deferred: optional worktree isolation and enforced file/resource locks are postponed until after one week of validating Phases 0–4.
