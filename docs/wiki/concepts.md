# Concepts

Shared vocabulary for people and agents. Short definitions: [glossary.md](glossary.md).

## Orchestrator and workers

The **orchestrator** (you, a Cursor agent, the sidebar LLM) plans, delegates, waits, and unblocks. It should not implement the change in the target repo itself.

A **worker** is a managed agent turn: a handle bound to a pane running Claude, Codex, OpenCode, Cursor Agent, or a shell. Workers edit files and run tests. Results come back on the operation snapshot (`status`, `result`, `result_capture`, `receipt`).

## Pane vs handle

| | Pane | Agent handle |
|---|------|----------------|
| Identifies | A PTY session in `PaneRegistry` | A worker / operation sequence |
| You see | `pane_id` | UUID `handle` |
| Tools | `list_panes`, `write_terminal_input`, `press_key` | `run_agent`, `wait_agents`, `inspect_agent` |
| Lifetime | Until killed / closed | Until `close_agent`; may outlive a single turn |

Passing a pane id where a handle is required returns `AGENT_NOT_FOUND` with a hint. `run_agent` with the same `name` / `handle` **adopts** the worker unless context is `fresh`.

## Operations

An **operation** is the durable record of one turn. Created **before** dispatch (`create_operation`). Snapshots live at `<project>/.puppet-master/operations/<id>.json`.

Statuses include `queued`, `running`, `completed`, `failed`, `cancelled`, and `interrupted`. `interrupted` is the intentional view status for a cancelled turn — do not rename it.

**Idempotency:** `run_agent` / `followup_task` mint a key if omitted. An identical retry (same key, same agent type, task contained, turn index 0) replays the existing run **before** worker selection. Same key, different request → `IDEMPOTENCY_KEY_CONFLICT`. A failed dispatch is durable; that key cannot be retried. After `WAIT_TIMEOUT`, reuse the key.

## MCP session, mode, ownership

Each stdio MCP process has a session id (`X-Puppet-Master-Session`). `PUPPET_MASTER_CLIENT_ID` makes it stable across reconnects.

**Mode** filters the tool catalog: `agent` (default), `shell`, `both`. `set_mode` emits `tools/list_changed`. Hosts that ignore that notification should start with `--mode` or `PUPPET_MASTER_MODE`. Wrong catalog → `MODE_MISMATCH`.

**Ownership:** the creating session owns the handle and pane. Mutating someone else's handle → `AUTHORIZATION_DENIED` (use `take_over` with `grant: true`). Reads and waits are allowed without the lease. `release` drops pane control but keeps the worker lease; `release_lease` drops the lease; `transfer_agent` moves ownership. Reconnect with `attach_agents(attach_token)`.

Waits never change leases.

## Result capture (provenance)

`result_capture` says **how** the reply was obtained:

| Value | When |
|-------|------|
| `Authoritative` | Structured evidence: OpenCode API message, CLI result event, verified native completion |
| `Inferred` | Screen-scraped TUI text |
| `Missing` | No reply text (idle without new output, or checks decide) |

`mark_operation_finished` rejects a non-`Missing` capture with empty evidence. An `Inferred` result never reports `verified: true`. Idle alone never completes a task — pane lifecycle and task lifecycle are separate.

**Warnings** (`RESULT_TRUNCATED`, `PROVENANCE_INFERRED`, `MODEL_UNRESOLVED`, …) never change status. They are caveats on the receipt.

Receipt `outcome`: `passed_checks`, `completed_unchecked`, `failed_checks`, `failed`, `cancelled`, `needs_input`, `in_progress`.

## TUI profiles

Anything true of **one** agent CLI lives in `tui/<name>/`. Generic code (`pty`, `agent_runs`, `bridge`, …) **never matches agent-type strings**. Call `tui::profile_for(agent_type)` and use a trait method or `Features` flag.

Known leftovers still keyed by `AgentType`: `pty/agents.rs`, Windows PowerShell hosting in `pty/registry.rs`, `agent_contexts.rs`, and `opencode_native` protocol branches (that backend is HTTP, not a scraped TUI).

Checklist: [tui/README.md](../../packages/app/src-tauri/src/tui/README.md).

## Acceptance checks

`run_agent` can pass `checks` (file/reply predicates, `command_exits`). `command_exits` runs an **argv** with a timeout, never a shell string. Failures become `ACCEPTANCE_FAILED`.

## Prompts and `answer_prompt`

Approval / trust dialogs block dispatch (`PANE_BLOCKED_BY_PROMPT`). Answer with `answer_prompt`, naming the **prompt id** you read (command-based, not a hash of the whole screen). Stale id → `STALE_PROMPT`. Same id inside the replay window returns the earlier receipt and does not press twice.

OpenCode yes/no is an **API** question: `answer_prompt` / `reply_opencode_question`, never `press_key` into the TUI compose box.

Never type into a pane while a Cursor dialog is up — those dialogs listen for hotkeys.

## Steering vs follow-up

- `send_message` — steer a **live** turn (or queue if live delivery is unsupported). Returns a steer **receipt**, not a result.
- `followup_task` — next turn on the same handle after the current one finishes.
- `interrupt_agent` — stop the current turn, keep the worker (`keep_pane` is forced).

Cursor headless can only be steered by restart (from turn 2). Visible Cursor panes are queue-only.
