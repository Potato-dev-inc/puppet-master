# MCP tool cheatsheet

Quick reference for orchestrators. Prefer **one wait, one read, one action**.

## Always first

| Tool | Args | Notes |
|------|------|-------|
| `bridge_health` | — | Confirms bridge + `tool_count` |
| `list_panes` | — | Reuse before `spawn_agent` |

## Delegate & wait

| Tool | When |
|------|------|
| `write_terminal_input` | Send prompt to worker; then call `suggested_wait` |
| `delegate_task` | Structured handoff (validate + render prompt) |
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

## Decision tree

```text
After write_terminal_input
  → wait_for_worker
  → reason?
       idle        → read_opencode_messages
       tui_prompt  → read_opencode_messages → reply_opencode_question
       permission  → reply_opencode_permission
       timeout     → read_opencode_worker_status once
```
