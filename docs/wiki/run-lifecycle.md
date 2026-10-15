# Run lifecycle

What happens after an orchestrator calls `run_agent`. This is the path you debug when a turn hangs, returns empty, or closes the wrong pane.

## Happy path

```
run_agent
  → MCP shim (idempotency_key, session, bounded HTTP wait)
  → bridge authorize (pairing → session/mode/ownership)
  → agent_runs::run_agent
       validate (task, timeout, read_only, dispatchable)
       replay identical key          ← before worker selection
       persist::decide_worker        ← create handle or adopt
       operations::create_operation  ← snapshot queued, before side effects
       spawn or reuse pane / native server
       deliver prompt
       supervise until terminal
       extract result + evaluate checks
       mark_operation_finished
  → AgentRunView (status, result, result_capture, receipt, watch_command?)
```

If the foreground `wait_ms` expires, the shim returns `WAIT_TIMEOUT` plus `watch_command`. The operation **keeps running**. Retry with the same idempotency key or `wait_agents` / CLI `watch`.

## 1. Tool call

Declared in `tool_registry.rs` (`POST /agents/run`). The shim (`bin/puppet-master-mcp.rs`) mints `idempotency_key` when missing, injects the session header, and caps the HTTP wait (`DEFAULT_AGENT_WAIT_MS`).

## 2. Authorize and route

`bridge.rs::route` → `mobile_pairing::authorize_bridge_request` → `mcp_sessions::authorize` → `agent_runs/routes.rs`.

## 3. Validate and bind

`headless.rs::run_agent` (despite the name, this is the entry for visible, headless, and native). Refusals include `INVALID_TASK`, `INVALID_TIMEOUT`, `READ_ONLY_UNSUPPORTED`, `AGENT_NOT_DISPATCHABLE` (`tui::profile_for(..).features()`).

Identical retry by key replays **before** `persist::decide_worker`. New workers get `persist::stable_handle(project, key)` so retries land on the same UUID.

## 4. Persist before dispatch

`create_operation` writes `queued` first. Conflicts: `IDEMPOTENCY_KEY_CONFLICT`, `AGENT_TURN_EXISTS`. Locks are taken here (`acquire_operation_locks`). A later panic is recorded as `DISPATCH_PANICKED`. App startup recovers interrupted ops (`OPERATION_INTERRUPTED`).

## 5. Deliver work

| Backend | What runs |
|---------|-----------|
| Headless (`headless=true`, Claude / Codex / Cursor) | `tui::headless_command`; stdout → `transcript.rs`; profile `extract_result` / `extract_provider_session` |
| Visible TUI | Spawn or reuse pane, wait until ready, `delivery.rs::dispatch_prompt`. Profiles with `verifies_prompt_delivery` retype until the prompt is visible. |
| `opencode_native` | Forced visible. `opencode/native.rs` spawns `opencode serve`; REST sends the prompt; `opencode/delivery.rs` acks (`PROMPT_NOT_DELIVERED` if ack fails). |

`opencode_native` is a protocol backend, not a scraped TUI. `run_agent` special-cases it to `headless=false`.

## 6. Supervise until terminal

`supervise_existing` loops per run.

- Visible TUI: `completion.rs::observe_tui_turn` — Done / Running / AmbiguousIdle / RetrySubmit / … from pane status + screen vs **pre-dispatch** baseline. Chrome lines are not reply text. `widen_reply` pulls longer replies from scrollback.
- Native: completion from **message order** (newest assistant `finish=stop` after the latest user message, not in the dispatch baseline). A live steer holds a `SteerGuard`; the supervisor must not accept-then-ignore a steer.
- Headless: process exit + structured extract.

Idle with no new output → `RESULT_NOT_CAPTURED` (recoverable) or `Missing` completion when checks decide. Permission prompts are `waiting_input`, not done.

On finish: `clean_extracted_result`, `checks.rs`, `mark_operation_finished` with `ResultCapture`. Failures: `mark_operation_state(Failed, error)`.

## 7. Pane close

`pane_close.rs::maybe_dispose_after_terminal`: close only if **this run spawned** the pane and `keep_pane` was not requested. Never close the user's pre-existing pane as cleanup. Interrupt forces keep-pane, then `cancel_agent` still records `keep_pane_requested` so a run-spawned pane can be released.

## 8. Publish

`publish_operation` → SSE + waiters (`pane_wait_notify`, operations store condvar).

## 9. Return or watch

`wait_for_run` builds `AgentRunView`. Still running → `watch_command` (`watch_command.rs`), a `node … watch <operation_id>` line the harness runs in the background. The JS CLI (`packages/cli/src/watch.ts`) long-polls `/operations/{id}/wait`. Killing watch never cancels the run.

`wait_agents` is the MCP equivalent (`POST /agents/wait`): handles, `after_cursor` / `after_revisions`, `until` filters, `mode: any | all`.

## Blocked turns

If a trust/approval dialog is up, dispatch returns `PANE_BLOCKED_BY_PROMPT`. The view includes `prompt` / `required_action`. Answer with `answer_prompt` (prompt id you read). OpenCode API questions: [workflows.md](../orchestrator/workflows.md) § B — not `press_key`.

## After you mutate

For worker handles: `wait_agents` with the handle and last `next_cursor`. Do not poll `list_panes` or `read_terminal_buffer` in a loop. Buffer reads are evidence after a wait, not a substitute for wait. See `.cursor/rules/imported/puppet-master/mcp-wait-not-poll.mdc`.
