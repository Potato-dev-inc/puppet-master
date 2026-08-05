# Orchestrator workflows

Copy-paste flows for common tasks. All steps are MCP tools unless noted.

## A. Send a prompt and get the answer

```text
1. list_panes
2. write_terminal_input { pane_id, text: "<task>" }
3. wait_for_worker { pane_id }
4. read_opencode_messages { pane_id }
5. Summarize last_assistant_text for the user
```

## B. Yes / no confirmation (opencode_native)

```text
1. write_terminal_input {
     pane_id,
     text: "Ask me one yes/no question only: Should I proceed with the next action? Do not run any tools until I answer."
   }
2. wait_for_worker { pane_id }          # → tui_prompt
3. read_opencode_messages { pane_id }   # → pending_question.request_id
4. reply_opencode_question { pane_id, answer: "No", request_id }
5. wait_for_worker { pane_id }          # → idle
6. read_opencode_messages { pane_id }   # confirm pending_question gone
```

Do **not** use `press_key` for step 4.

## C. Permission prompt

```text
1. wait_for_worker { pane_id }                    # → permission
2. read_opencode_worker_status { pane_id }        # pending_permission_ids
3. reply_opencode_permission { pane_id, request_id, reply: "once" }
4. wait_for_worker { pane_id }
```

## D. Switch model then delegate

```text
1. switch_agent_model { pane_id, model_id: "glm-5.2", model_provider: "opencode-go" }
2. wait_for_model { pane_id, match: { provider_id, model_id } }
3. inspect_agent_model { pane_id }
4. write_terminal_input { pane_id, text: "..." }
5. wait_for_worker { pane_id }
```

## E. New worker for a repo path

```text
1. list_panes   # confirm none match cwd + agent_type
2. spawn_agent { agent_type: "opencode_native", cwd: "C:\\path\\to\\project" }
3. wait_for_worker { pane_id from spawn }
4. write_terminal_input { ... }
```

## F. Multi-step task with session memory

```text
1. update_session_context { goal: "...", notes: [...] }
2. set_pane_role { pane_id, role: "implementer" }
3. delegate_task { pane_id, intent, acceptance_criteria }
4. write_terminal_input { pane_id, text: <rendered from delegate_task> }
5. wait_for_worker → read_opencode_messages
6. update_pane_digest { pane_id, summary: "..." }
```

## G. When something looks stuck

```text
1. read_opencode_worker_status { pane_id }   # serve_healthy? permissions?
2. read_opencode_messages { pane_id }        # pending_question?
3. read_terminal_buffer { pane_id, lines: 40 }  # once, for human evidence only
```

If `pending_question` exists but TUI looks idle → use `reply_opencode_question`, not keyboard input.
