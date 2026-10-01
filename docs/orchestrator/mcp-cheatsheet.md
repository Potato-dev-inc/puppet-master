# MCP tool cheatsheet

New connections start in **agent mode**. Call `run_agent` with a task; it waits
up to 30 seconds by default. Use `background: true` for an immediate handle.
Then use `wait_agents`, `send_message` or `followup_task`, `interrupt_agent`,
`inspect_agent`, `agent_transcript`, or `close_agent`. `send_agent` and
`cancel_agent` remain compatibility wrappers. `take_over` grants explicit pane
control. `spawn_agent` still only opens a pane.

Use `set_mode({"mode":"shell"})` or `set_mode({"mode":"both"})` for the
terminal and legacy operation tools below. See [the mode contract](agent-shell-modes.md).

## Always first

| Tool | Args | Notes |
|------|------|-------|
| `bridge_health` | — | Confirms bridge + `tool_count` |
| `list_panes` | — | Reuse before `spawn_agent` |

## Delegate & wait

| Tool | When |
|------|------|
| `run_agent` | Start/adopt a worker; `name`/`handle` reuse an idle worker unless `context_mode=fresh` |
| `wait_agents` | Wait for turn result (`status`, `result`, `result_capture`, `acceptance_status`, `next_cursor`) |
| `send_message` | Steer the current turn; receipt has `message_id` + `disposition` |
| `followup_task` | New task on the same worker |
| `interrupt_agent` | Stop the current turn; worker remains |
| `inspect_agent` | Status, task, `context_policy`, requested vs resolved model |
| `agent_transcript` | Typed events with cursor |
| `close_agent` | Dispose the worker session |
| `session_identity` | Connection UUID vs coordinator id + attach token |
| `attach_agents` | Reconnect: new connection UUID, same coordinator token |
| `release` | Return temporary pane control; keeps the worker lease |
| `release_lease` | Drop the worker lease (not a transfer) |
| `transfer_agent` | Move ownership to a live `to_session_id` |
| `delegate_work` | Lower-level operation create (shell/both mode) |
| `wait_for_operation` | Wait on returned `operation_id` and `revision`; continue if nonterminal |
| `get_operation` | Read durable operation state when needed |
| `cancel_operation` | Request cancellation; running cancellation aborts a native session or interrupts the assigned pane |
| `delegate_task` | Structured handoff (validate + render prompt only; does not dispatch) |
| `spawn_agent` | New pane only when none fits |
| `wait_for_worker` | **Default wait** after mutations (`settled` + errors) |
| `wait_for_panes` | Custom `until`: `idle`, `permission`, `model_ready`, `tui_ready`, `task_completed`, … |
| `wait_for_model` | After `switch_agent_model` |
| `wait_for_task` | After task delegation with `task_id` |

## `opencode_native` status & output

| Tool | When |
|------|------|
| `read_opencode_messages` | Model text, `pending_question`, `request_id` |
| `read_opencode_worker_status` | Health, permissions, session id |
| `inspect_agent_model` | Session model (API truth) |
| `reply_opencode_question` | Yes/No API menus — **not** `press_key` |
| `reply_opencode_permission` | API permission prompts |
| `switch_agent_model` | Change worker model + sync TUI |
| `read_opencode_key_status` | Key profile a/b (no secrets) |
| `rotate_opencode_key` | Rate-limit recovery |

## Coordination (longer runs)

| Tool | When |
|------|------|
| `read_session_context` / `update_session_context` | Shared goal, roles, timeline |
| `set_pane_role` | `implementer`, `reviewer`, `shell`, … |
| `read_pane_digest` / `update_pane_digest` | Short summary without scrollback |
| `create_task` / `claim_task` / `complete_task` | Task board |
| `acquire_resource_lock` / `release_resource_lock` | Avoid conflicting edits |
| `build_context_pack` | Pack context for worker |

## Debug only (do not loop)

| Tool | When |
|------|------|
| `read_terminal_buffer` | Verbatim TUI evidence after a wait |
| `read_recent_events` | Event log tail |
| `press_key` | Rare TUI-only interactions — **not** OpenCode API questions |

## `wait_for_panes` `until` values

`idle`, `waiting_input`, `error`, `gone`, `permission`, `unhealthy`, `key_swap_required`, `key_swap`, `key_rotated`, `rate_limited`, `model_ready`, `tui_ready`, `task_completed`, `task_blocked`, `output_match`

`wait_for_worker` ≈ `until: ["settled", "error"]` (includes API yes/no → `tui_prompt`).

Operation status is `queued`, `starting`, `running`, `waiting_input`, `cancelling`, `completed`, `failed`, or `cancelled`; source is `native` or `inferred`. `progress_pct` is null without authoritative progress. Idle panes do not imply task completion. Only explicit result evidence supports `completed`. If startup encounters an approval prompt, `delegate_work` has already returned an operation ID; that operation waits in `waiting_input` with its pane preserved. A human clears the prompt, then the bridge observes the pane state and resumes dispatch. No approval is automatic. A timeout or revision change is not success. With an MCP progress token, revision changes produce progress notifications with the latest snapshot. Request cancellation wakes a wait call; operation cancellation aborts a native session or interrupts the assigned pane. Prefer operation waits and snapshots over terminal scraping. Tool search/discovery can be limited by an MCP host: if a tool is absent, refresh/reconnect the Puppet Master server and inspect the host's available tools before falling back.

## Decision tree

```text
After delegate_work
  → wait_for_operation(operation_id, after_revision)
  → reason?
       matched_state / revision_changed → inspect snapshot; wait again if nonterminal
       terminal                     → report completed only with result evidence
       timeout                      → wait again or inspect once; do not infer success

After write_terminal_input (manual pane workflow)
  → wait_for_worker
  → reason?
       idle        → read_opencode_messages
       tui_prompt  → read_opencode_messages → reply_opencode_question
       permission  → reply_opencode_permission
       timeout     → read_opencode_worker_status once
```
