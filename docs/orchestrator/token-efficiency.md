# Token efficiency for orchestrators

Why Puppet Master orchestration is cheaper than polling terminals or spawning Cursor subagents.

## Orchestrator vs worker context

| Layer | Holds | Cost |
|-------|--------|------|
| **Orchestrator** (you, via MCP) | Goals, pane ids, compact status | Keep small |
| **Worker** (`opencode_native`) | Code exploration, tool runs, long output | Isolated in OpenCode session |

Delegate implementation; don’t re-import worker scrollback into every turn.

## Expensive patterns

| Pattern | Why it hurts |
|---------|----------------|
| Loop `read_terminal_buffer` | Multi-KB TUI chrome per call |
| Loop `list_panes` | No new information; burns turns |
| `press_key` retries on API prompts | Failed attempts + debug reads |
| Cursor `Task` subagents for long work | Fresh context, re-explore repo, duplicate rules |
| Ignoring `suggested_wait` | Extra “what’s the state?” reasoning |

## Cheap patterns

| Pattern | Why it helps |
|---------|----------------|
| `wait_for_worker` once | Blocks server-side; one result |
| `read_opencode_messages` once | JSON: question + last line of assistant text |
| `reply_opencode_question` once | Small mutation; clears blocker |
| Reuse panes | No re-spawn / re-orientation |
| `read_pane_digest` / session context | Summaries instead of history (when populated) |

## Rough comparison

| Approach | Orchestrator tokens | Worker tokens |
|----------|---------------------|---------------|
| Poll buffer 20× | High | — |
| Wait + structured read + reply | Low | Normal |
| 3× Cursor subagents in parallel | High (3× spawn + merge) | — |
| 1 orchestrator + 1 opencode worker | Low | Separate (often cheaper model) |

Workers can run **cheaper models** (e.g. Luna/Haiku) while the orchestrator uses a stronger model only for coordination.

## Rule of thumb

> If you’re reading the terminal more than once per orchestrator decision, you’re probably doing it wrong.

Use wait → read structured state → act → wait again.
