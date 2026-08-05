# Orchestrator quickstart

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
