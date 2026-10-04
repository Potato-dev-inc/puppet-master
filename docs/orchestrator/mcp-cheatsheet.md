# MCP tool cheatsheet

New connections start in **agent mode**. For a tiny question call `quick_ask`
(below). For real work call `run_agent` with a task; it waits up to 120 seconds
(`wait_ms`, default 120000) by default. Use `background: true` for an immediate handle.
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

`bridge_health` also returns an additive `env` object so you do not have to install or probe tools:
`{"python","pytest","node","git","cargo"}`, each `{present, version, path}` (`version`/`path` are
null when absent). It is read from PATH on the MCP host with a short per-tool timeout and cached for
about a minute. `pytest` counts as present for a `pytest` executable or `python -m pytest`. Check it
before running `pip install` or bootstrapping a virtualenv.

## `watch_command` and the `shell` argument

Running turns return a `watch_command` (`node <cli> watch <operation_id> --project-path <path>`).
Paths use forward slashes and are single-quoted when they need quoting, which is valid in both
PowerShell and Git Bash (backslash paths were eaten by Git Bash). `run_agent`, `wait_agents` and
`followup_task` accept an optional `shell` (`bash` | `powershell` | `cmd`) to render it for one shell;
the env var `PUPPET_MASTER_WATCH_SHELL` sets a default.

## Steering limits

- Cursor headless runs can only be steered by a restart, and only from turn 2; visible Cursor panes
  are queue-only (no live steering).
- `send_message` can queue the message as the next turn (`delivery=queue`, or `live_or_queue` when
  live delivery is unavailable) instead of steering the current one; a finished turn needs
  `followup_task`. Check the returned `disposition` before assuming the worker saw it.

## Quick questions: `quick_ask`

`quick_ask({"task": "..."})` is the native-subagent-style call for a tiny question. It runs
`run_agent` with fixed conservative settings: headless, foreground, `read_only: true`, fresh
context, no `checks`, no shell, `keep_pane: false`, `timeout_ms: 300000`. Arguments: `task`
(required), `project_path`, `backend` (`claude` | `codex` | `cursor_agent`), `model`, `wait_ms`
(default 90000, clamped to 120000) and `read_only` (pass `false` only if the agent must write).
With no `backend`, the first installed CLI that enforces read-only is used, in order
claude, codex, cursor_agent (OpenCode is never picked). If none is on PATH the call fails with
`BACKEND_UNAVAILABLE`.

It returns `{"answer": "...", "receipt": {...}}`. If the run is still going at `wait_ms` it returns
`{"status":"running","handle":"...","hint":"wait_agents"}`; call `wait_agents` with that handle.
A failed run is an error that carries the handle and receipt in `context`.

## Run receipt

Every run view (`run_agent`, `wait_agents`, `followup_task`, `inspect_agent`, `quick_ask`) carries a
`receipt` saying what the run verified and what it did not:

| Field | Meaning |
|-------|---------|
| `backend` | Agent type that ran |
| `model` | Resolved model when the backend confirmed it, else the requested one (absent if neither) |
| `read_only` | `enforced` (requested and the backend enforces it), `unenforced` (requested, backend cannot), `off`. `opencode_native` enforces it only for a pane the run spawns itself (see [opencode-native-worker.md](opencode-native-worker.md)) |
| `checks` | `none configured` or `<passed>/<total> passed` (`0/N passed (pending)` while running) |
| `result_capture` | `authoritative`, `inferred`, `missing`, or `pending` |
| `not_verified` | Short strings, each derived from a recorded fact |
| `outcome` | The one field for "did it work" (below) |
| `build_id` | Puppet Master build that produced the run |

`not_verified` entries: `no checks configured`; `result read from terminal screen`
(`result_capture` is `inferred`); `model not confirmed` (a model was requested but not confirmed,
only once the run has finished); `read_only not enforced`; `shell commands rejected` (the
`SHELL_COMMAND_REJECTED` warning is present). An empty list means none of these apply, not that
the answer is correct. Read it before trusting `result`.

### Did it work? `receipt.outcome`

Read `receipt.outcome` instead of combining `status`, `verified` and `acceptance_status`:

| `outcome` | Meaning |
|-----------|---------|
| `passed_checks` | Completed **and** every acceptance check passed: the requested behaviour was proven |
| `completed_unchecked` | The worker finished but nothing verified it (no `checks`); the answer may still be wrong |
| `failed_checks` | A check failed (`ACCEPTANCE_FAILED`) |
| `failed` | The run failed for another reason (see `error.code`; `recoverable` says whether a retry is sensible) |
| `cancelled` | Stopped by `cancel_agent` / `interrupt_agent` (the view `status` reads `interrupted`) |
| `needs_input` | Waiting on a prompt; `prompt` holds its id, kind, `command` and `cwd` |
| `in_progress` | Still running |

A finished run's `status` is one of `completed`, `failed`, `interrupted` (cancelled), `timeout`,
`closed`. For anything that changes state, pass `checks` (for example `command_exits` running the
tests, or `file_contains`) so `passed_checks` means something. `followup_task` takes the same
`checks`, `acceptance_criteria`, `wait_ms`, `background` and `locks` as `run_agent`.

### Recovery contract

- **Retries do not duplicate.** Calling `run_agent` again with the same `idempotency_key` (and no
  worker name) replays the run already in flight and returns the same `handle` and `operation_id`.
- **Cancel releases.** `cancel_agent` stops the turn and closes a pane the run created unless it
  was started with `keep_pane=true`. `interrupt_agent` stops the turn but leaves the worker (and its
  pane) open for a resume.
- **Timeouts are explicit.** A run past `timeout_ms` ends as `status: timeout`, `error.code:
  TIMEOUT`, `recoverable: true`, with its handle still inspectable.
- **Reconnect.** Another connection cannot act on a worker it does not own
  (`AUTHORIZATION_DENIED`); it can after `attach_agents` with the owner's `attach_token`
  (see `session_identity`).
- **Approvals.** A command approval has a stable `prompt_id` tied to the exact command and cwd; it
  survives redraws and scrolling. If the command on screen changed, `answer_prompt` returns
  `STALE_PROMPT` with the `current_prompt` (id, command, cwd) so you can review and answer that one.

Each of these has a scenario in `scripts/pm-regression.mjs` (`duplicate`, `cancel`, `timeout`,
`reconnect`).

### Steering receipts

`send_message` returns a receipt whose `disposition` is `accepted`, `accepted_via_restart`,
`queued`, `processed`, `deferred`, `unsupported`, `expired` or `rejected`. It also says what that
means: `applied` (true only when the message reached the turn it was meant for) and `next_action`
in words. A `queued` message does **not** change the running turn; it expires when the turn ends, so
send it with `followup_task` if you still need it.

## Delegate & wait

| Tool | When |
|------|------|
| `quick_ask` | Tiny one-shot question; returns `{answer, receipt}` (see above) |
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
