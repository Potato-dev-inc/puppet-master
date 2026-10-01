# Commands

Copy-paste surface for **starting Puppet Master** and **spawning workers**. A host (desktop or `npm run worker`) must be running before MCP tools work.

Agent tools use **handles**. Pane tools use **pane ids**. Do not mix them.

## 1. Start a host

```bash
npm run worker                         # standalone OpenCode API window (default)
npm run worker cursor_agent            # Cursor Agent CLI window
npm run worker cursor_agent --new      # extra pane on the already-running host
npm run worker opencode_native --new
npm run worker powershell --new
npm run dev                            # full desktop grid
npm run mcp                            # stdio MCP (host must already be up)
npm run build:mcp                      # rebuild MCP binary, then reconnect the host
```

Same thing via CLI:

```bash
npx puppet-master                      # GUI
npx puppet-master --project PATH
npx puppet-master worker [AGENT]
npx puppet-master worker cursor_agent --new
npx puppet-master worker --agent-type cursor_agent --cwd PATH --new
npx puppet-master mcp
npx puppet-master version
```

`--new` / `--force-new` always create another pane instead of reusing one. Optional: `--cwd PATH`, `--pane-id ID`, `--cols N`, `--rows N`.

**CLI agent types:** `opencode_native` (default), `opencode`, `claude`, `codex`, `cursor_agent`, `cursor` (IDE), `powershell`, `cmd`, `bash`.

If the host is already up, `worker --new` attaches a pane to that process instead of starting a second app.

## 2. Spawn a persistent worker (preferred)

MCP starts in **agent mode**. This is the Luna-style loop: assign work, keep a handle, wait only when the previous call is still pending.

```text
list_agents / list_workers
  → workers[].worker_id (UI panes and managed runs)

run_agent
  worker_id: "<id>"                    # adopt that pane; directory is bound automatically
  task: "Find why the connection fails."
  name: "investigator"                 # optional; reuse this name later
  agent_type: "cursor_agent"           # claude | codex | cursor_agent | opencode | opencode_native
  headless: true                       # default; set false for a visible TUI
  keep_pane: true                      # leave the terminal up for inspection
  background: true                     # return the handle immediately (else waits up to wait_ms, default 30s)
  context_mode: "packet"               # fresh | packet | parent_summary | resume | selected_history
  model: "..."                         # requested; inspect shows requested vs resolved
  timeout_ms: 900000                   # turn timeout (default 15 min)

→ handle + current status/result
```

Adopt an existing idle worker (same collaborator, new turn) unless `context_mode` is `fresh`:

```text
run_agent
  name: "investigator"
  task: "Propose a fix using your findings."
```

Or pass `handle` / `agent_run_id` instead of `name`.

Other `run_agent` fields: `worker_id` (from `list_workers`; uses that pane's directory), `project_path`, `read_only`, `pane_id`, `role`, `scope`, `reasoning`, `acceptance_criteria`, `idempotency_key`, `allow_broad`, `selected_history`.

### After you have a handle

```text
followup_task
  handle: "<handle>"
  task: "Propose a fix using your findings."
  # waits up to 120s and returns status/result; wait_agents only if still queued/running

wait_agents
  handles: ["<handle>"]
  after_cursor: 0                      # then pass next_cursor from the last wait
  timeout_ms: 120000
  until: ["completed", "failed", "cancelled", "waiting_input"]
  mode: "any"                          # any | all
  # without until, a progress revision can wake you before the final answer

send_message
  handle: "<handle>"
  message: "Also check reconnect behavior."
  delivery: "live_or_queue"            # live_or_queue | live | queue
  idempotency_key: "steer-1"           # retries return the same receipt

interrupt_agent { handle }             # stop this turn; worker stays
inspect_agent { handle }               # status, capabilities, requested vs resolved model
agent_transcript { handle, after: 0 }
close_agent { handle }                 # dispose; this can kill a pane this run created
list_agents / list_workers { project_path? }
  # returns workers[] with worker_id, backend, workspace, status, grant_required, read_only_supported
  # includes UI-created panes, not just run_agent handles

```

`send_agent` = `followup_task`. `cancel_agent` = `interrupt_agent`.

**Do not** poll `list_panes` or `read_terminal_buffer` for status.

## 3. Spawn a pane only

Opens a visible terminal. Does **not** start a worker handle or send a task.

MCP `spawn_agent` catalog is only:

```text
spawn_agent
  agent_type: "opencode_native" | "powershell"
  cwd: "C:\\path\\to\\project"
  force_new: true                      # required for a second pane of the same type
  cols: 120
  rows: 30
  pane_id: "optional-stable-id"
```

Then wait with the returned `suggested_wait` (usually `wait_for_panes`).

For **Cursor / Claude / Codex / OpenCode TUI panes**, use the CLI in §1 (`npm run worker cursor_agent --new`), or `run_agent` with `headless: false` / `keep_pane: true`.

Need shell mode for pane tools:

```text
set_mode { mode: "shell" }             # or "both"
list_panes
bridge_health
```

Host launch with a fixed catalog: `--mode both` or `PUPPET_MASTER_MODE=both`.

## 4. Drive a pane (low-level)

Only after `set_mode` to `shell` or `both`. Prefer §2 unless you need the TUI itself.

```text
write_terminal_input { pane_id, text, append_newline: true }
wait_for_worker { pane_id, timeout_ms: 120000 }
wait_for_panes { pane_ids: ["..."], until: ["idle", "permission", "model_ready"] }

read_opencode_messages { pane_id }           # opencode_native output
read_opencode_worker_status { pane_id }
reply_opencode_question { pane_id, answer: "Yes", request_id? }
reply_opencode_permission { pane_id, request_id, reply: "once"|"always"|"deny" }
switch_agent_model { pane_id, model_id: "glm-5.2" }

kill_pane_process { pane_id }
shell_exec { command: "git status", cwd? }
```

`wait_for_panes` `until`: `idle`, `waiting_input`, `error`, `gone`, `permission`, `unhealthy`, `key_swap_required`, `key_swap`, `key_rotated`, `rate_limited`, `model_ready`, `tui_ready`, `task_completed`, `task_blocked`, `output_match`.

## 5. Ownership after reconnect

```text
session_identity                       # connection_id, coordinator_id, attach_token
attach_agents { attach_token }         # reclaim workers after MCP reconnect
take_over { handle | pane_id, grant? } # temporary control; grant=true for panes you did not start
release { handle | pane_id }           # return pane control; keep the worker lease
release_lease { handle }               # drop the lease
transfer_agent { handle, to_session_id }
```

A coordinator **name** is not a credential. Attach tokens are in-memory; they die with the desktop process.

## 6. What each spawn actually supports

| Spawn | Live steer (`send_message`) | Session resume | Visible pane | Interrupt |
|-------|-----------------------------|----------------|--------------|-----------|
| `run_agent` `opencode_native` | yes | yes | if pane exists | graceful |
| `run_agent` `cursor_agent` TUI | queued / unsupported | reconstructed summary | yes | Ctrl+C |
| `run_agent` `cursor_agent` headless | queued / unsupported | reconstructed summary | no | stop child |
| `run_agent` `opencode` / `claude` / `codex` TUI | queued / unsupported | reconstructed summary | yes | Ctrl+C |
| `spawn_agent` / `npm run worker` | not a worker handle | n/a | yes | pane kill |

`inspect_agent` returns the contract (`live_messages`, `session_resume`, `queued_followups`, `graceful_interrupt`, `visible_terminal`, `notes`). Cursor TUI is version-specific screen heuristics, not Luna parity.

## 7. Typical sequences

**Cursor or OpenCode as a reusable collaborator**

```text
list_workers
run_agent { worker_id: "<id>", task: "...", keep_pane: true }
followup_task { handle, task: "Propose a fix." }        # wait_agents only if still running
send_message { handle, message: "Also check X." }      # may queue on Cursor TUI
interrupt_agent { handle }                             # optional
close_agent { handle }                                 # when done
```

**Visible Cursor window, then attach MCP later**

```bash
npm run worker cursor_agent --new
```

```text
list_workers
run_agent { worker_id: "<id>", task: "..." }
```

**OpenCode API pane, old path**

```text
set_mode { mode: "both" }
spawn_agent { agent_type: "opencode_native", cwd: "C:\\path\\to\\project" }
→ suggested_wait → wait_for_panes
write_terminal_input { pane_id, text: "..." }
wait_for_worker { pane_id }
read_opencode_messages { pane_id }
```

## 8. If agent tools are missing

```bash
npm run build:mcp
```

Reconnect the Puppet Master MCP server in the host. Catalog is local; a stale process will not show `run_agent`.
