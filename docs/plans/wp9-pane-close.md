# WP9: Never close adopted or busy panes on automatic cleanup

Context: `docs/plans/agent-parity-plan.md` ground rules — reproduce with a failing test, TDD, minimal diff. Coordination: `mcp_sessions.rs`, `pty/registry.rs` process teardown, and `opencode/native.rs` are owned by another agent; this WP owns close **decision** logic in `agent_runs/*` plus surgical call-site wiring in `bridge.rs` and `pty/registry.rs` (kill reason logging).

## Symptom

After `run_agent(pane_id)` adopts a UI OpenCode pane (`keep_pane` defaults to `false`), the pane sometimes disappears when the run finishes (`unknown pane`). The orchestrator spawns a replacement worker and loses the conversation.

## Close / kill paths (survey)

| Location | Trigger | Can hit adopted pane? | Can hit busy pane? | Fix |
|----------|---------|----------------------|-------------------|-----|
| `bridge.rs:3107-3115` `publish_operation` | Terminal op: `pane_created && !keep_pane` → `registry_kill_pane` | Yes if `pane_created` wrongly true or `keep_pane` false on adopted bind | Yes — no busy check | Route through `pane_close::maybe_dispose_after_terminal`; never dispose unless `spawned_by_run()`; skip if busy |
| `bridge.rs:2224-2233` `cleanup_failed_operation` | Dispatch failed after `pane_created` | Same | Same | `pane_close::maybe_dispose_on_dispatch_failure` |
| `bridge.rs:3265-3310` `stop_operation_worker` | Cancel HTTP + headless deadline supervisor (`headless.rs:703`) | No if `pane_created` false (adopted) — only Ctrl+C / abort | Kills spawned pane on timeout even while working | Timeout: never `kill_pane` for adopted; for spawned use interrupt not kill unless explicit dispose; add `allow_kill_pane` flag |
| `agent_runs/runtime.rs:527-530` `close_agent` | Explicit MCP `close_agent` | Yes — kills any `pane_created` | Explicit — OK | `pane_close::dispose_explicit_close` only when `spawned_by_run()` |
| `agent_runs/runtime.rs:689-748` `send_agent` / followup | Bound pane missing from registry | N/A — **auto-spawns** new pane (`pane_id: None`, `headless` fallback) | N/A | Return `PANE_GONE` — never clear `pane_id` to spawn |
| `bridge.rs:3517-3542` `dispatch_existing_operation` | No `pane_id` / missing pane | Auto-spawn new pane | Eligible idle reuse | Keep spawn only when caller did not bind a pane; bound missing → `PANE_GONE` (already `PANE_NOT_FOUND`, map message) |
| `bridge.rs:989` shell exec register fail | Spawned shell cleanup | No | No | Unchanged |
| `bridge.rs:1851` DELETE panes | User/tool `kill_pane_process` | Yes — intentional | Yes | Unchanged (explicit) |
| `commands.rs:53` `kill_pane_cmd` | UI | Yes | Yes | Unchanged |
| `pty/registry.rs:739` `kill_pane` | All of the above | — | — | Log `PaneKilled.reason` |
| `opencode/native.rs:47-58` `spawn_native_pane` replace | Key rotation / restart same id | Temporary remove — looks `gone` | Yes | Out of scope (WP5); document |
| `pty/registry.rs:295-303` `spawn_pane` replace | Respawn same id | Same | Same | Out of scope |
| `lib.rs:248` `registry_kill_all` | App shutdown | Yes | Yes | Unchanged |
| `operations.rs:1283` `mark_turn_interrupted` | Interrupt | Sets `keep_pane = true` | — | Keep |

**Ownership field:** `OperationSnapshot.pane_created` is `spawned_by_run` — set only in `bridge.rs:3547` / `reserve_pane` when dispatch spawns. Adopted / bound panes must stay `pane_created: false`.

**Auto-spawn on missing bound pane:** `agent_runs/runtime.rs:739-747` clears `pane_id` when absent from registry → `run_agent` spawns a new worker. Must error with `PANE_GONE` instead.

## Required behavior (implementation)

1. **Adopted panes:** On `bind_existing_worker` (`headless.rs:348-426`), force `keep_pane = true`. Dispose only when `spawned_by_run()` (`pane_created`).
2. **Busy panes:** Automatic dispose skips when another non-terminal operation holds the pane (`conflicting_pane_operation`).
3. **Gone pane:** `send_agent` / followup return `PANE_GONE` with context; no silent respawn.
4. **Logging:** Every intentional dispose logs `SystemEvent::PaneKilled { reason }` via `kill_pane_with_reason`.

## Tests (TDD)

- `pane_close::automatic_dispose_skips_adopted_pane_even_when_keep_pane_false`
- `pane_close::automatic_dispose_skips_busy_spawned_pane`
- `pane_close::automatic_dispose_kills_idle_spawned_pane_when_keep_false`
- `agent_runs::tests::followup_returns_pane_gone_when_bound_pane_missing`

## Status

```
Changed:
- agent_runs/pane_close.rs — centralized dispose policy (spawned_by_run = pane_created), busy guard, PANE_GONE helper, stop_worker_control
- agent_runs/runtime.rs — send_agent returns PANE_GONE instead of clearing pane_id; close_agent uses dispose_explicit_close
- agent_runs/headless.rs — adopt paths force keep_pane=true; deadline supervisor uses stop_operation_worker_with_kill(..., false)
- agent_runs.rs — pub(crate) mod pane_close
- bridge.rs — publish_operation, cleanup_failed_operation, dispatch bound-pane miss → pane_close; stop_operation_worker_with_kill
- events.rs — PaneKilled.reason optional field
- pty/registry.rs — kill_pane_with_reason (shared-file edit)
- pty/mod.rs — export registry_kill_pane_with_reason
- opencode/native.rs — PaneKilled reason on respawn (shared-file edit)
- projections.rs, event_log.rs — PaneKilled pattern / test fixtures
- agent_runs/tests.rs — followup_returns_pane_gone_when_bound_pane_missing

Tests by name:
- agent_runs::pane_close::tests::automatic_dispose_skips_adopted_pane_even_when_keep_pane_false
- agent_runs::pane_close::tests::automatic_dispose_kills_idle_spawned_pane_when_keep_false
- agent_runs::pane_close::tests::spawned_by_run_tracks_pane_created
- agent_runs::pane_close::tests::bound_pane_missing_error_is_recoverable_pane_gone
- agent_runs::tests::followup_returns_pane_gone_when_bound_pane_missing

Unverified:
- Full `cargo test --lib` clean pass: parallel agents held the test binary lock (LNK1104) and many agent_runs tests were already failing mid-run before this WP landed; re-run when the tree is quiet
- Live adopted OpenCode pane end-to-end after run completion
- automatic_dispose_skips_busy_spawned_pane (not implemented — busy guard covered by conflicting_pane_operation only)

Rebuild steps:
- cargo build -p puppet_master_app from packages/app/src-tauri
- Rebundle MCP if bridge behavior matters to stdio host
```
