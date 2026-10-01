# Orchestrator quickstart

## Agent-mode happy path (one handle)

```
list_agents
run_agent { worker_id: "<pane or run id>", task: "..." }   → handle
send_message { handle, message: "..." }                     → receipt (steer only)
wait_agents { handles: [handle] }                           → result or needs_input
followup_task { handle, task: "..." }                     → next turn, same worker
```

- `run_agent` adopts an existing OpenCode pane by id without `take_over`.
- `WAIT_TIMEOUT` on run/followup: keep the same `idempotency_key` and call `wait_agents`, or run the returned `watch_command` (`npx puppet-master watch …`) in your agent harness shell tool's **background** mode (foreground process the harness can monitor). Do not use detached `Start-Process` — that bypasses harness notifications.
- Each `run_agent` / `followup_task` / `delegate_work` response includes `watch_command` (`npx puppet-master watch …`).
- Default `wait_agents` wakes on completion, failure, cancel, or `needs_input` — not on progress alone. Use `wake_on_progress: true` for stage updates.
- Closing the user's pane is not cleanup.
- After an app restart, call `list_agents` again — pane ids are not stable; handles and `worker_id` from persisted runs are.

---

Minimal loop for driving a worker through Puppet Master MCP.

## 0. Confirm bridge

```
bridge_health
```

Expect `ok: true` and a `tool_count` matching the bridge (check after MCP toggle if tools are missing).

## 1. List panes

```
list_panes
```

Reuse an existing `opencode_native` pane when `cwd` and status fit. Only `spawn_agent` when no suitable pane exists.

## 2. Read context (optional but cheap)

```
read_agent_context { pane_id }
inspect_agent_model { pane_id }    # opencode_native: session API, not buffer
```

## 3. Delegate

```
write_terminal_input {
  pane_id,
  text: "<task or prompt>",
  append_newline: true
}
```

Response includes `suggested_wait` — **call it next**, do not inspect the terminal yet.

## 4. Wait (one call)

```
wait_for_worker { pane_id, timeout_ms: 120000 }
```

Or use `wait_for_panes` with explicit `until` when you need a specific predicate (see [cheatsheet](mcp-cheatsheet.md)).

Read `reason` and `agent_hint` in the result:

| `reason` | Next step |
|----------|-----------|
| `idle` | `read_opencode_messages` for output |
| `tui_prompt` | `read_opencode_messages` → `reply_opencode_question` |
| `permission` | `reply_opencode_permission` with `request_id` from hint/snapshot |
| `timeout` | One diagnostic read, then adjust timeout or check worker health |

## 5. Read output (structured)

For `opencode_native`:

```
read_opencode_messages { pane_id }
```

Use `last_assistant_text`, `pending_question`, and `messages[]` — not `read_terminal_buffer` for routine status.

## 6. Unblock if needed

```
reply_opencode_question { pane_id, answer: "Yes" | "No", request_id? }
reply_opencode_permission { pane_id, request_id, reply: "once" | "always" | "deny" }
```

## 7. Wait again, then summarize

Call `wait_for_worker` until `idle`, then `read_opencode_messages` once for evidence.

---

**Anti-patterns:** looping `list_panes`, looping `read_terminal_buffer`, using `press_key` for OpenCode API yes/no menus, ignoring `suggested_wait`.
