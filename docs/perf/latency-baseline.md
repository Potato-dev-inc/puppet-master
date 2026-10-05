# Delegation latency baseline

Measured with `scripts/perf-latency.mjs` against the dev app on one Windows machine: a trivial task
(`Reply with only the word PONG. Use no tools.`) per backend, 3 cold runs (fresh sandbox folder, new handle)
each followed by 2 follow-ups on the same handle. Turn latency is wall-clock from the MCP call to the returned
result; stages come from the operation `stage_log` plus a 40ms poll of the pane screen for visible panes.
Model time is the provider's and varies run to run (Cursor Composer 2.5, DeepSeek via OpenCode Go, Claude
Sonnet 5.5, Codex), so compare the stage columns, not only totals. Regenerate:

```
node scripts/perf-latency.mjs --out <dir> --label after --start-dir 1100
node scripts/perf-compare.mjs <before-turns.json> 400 <after-turns.json> 1100
```

Raw runs (JSON/CSV, every stage mark and screen frame) are kept in the session scratchpad as
`perf-base/before-*` (baseline, build 3418637) and `perf-final2/final2-*` (build cf50920).

Before the work, visible Claude could not complete at all (RESULT_NOT_CAPTURED after 30s) and Codex needs a git
folder (`--skip-git-repo-check` is not passed), so the sandboxes for Codex are `git init`ed.

## What was slow, and what fixed it

| Finding | Evidence | Fix |
|---|---|---|
| A TUI pane read `idle` only after 5s with no output bytes. Cursor keeps emitting bytes that change no cell (19s after its input box was ready), so finished Cursor panes stayed `running`: 19s completion lag cold, 5.7s on follow-ups, 21s before the first prompt could be typed | status trace + `last_output_at` polling | idle = screen held still; profile `quiet_idle_after` (Cursor 700ms, Claude 800ms) when no spinner/stop hint shows; 100ms watcher; classifier skipped on unchanged screens |
| `worker_status` parsed the whole project `events.jsonl` (40MB here, 1.3s) on every call; the native supervisor ran it, plus two full message reads, every tick (tick 1-1.3s, completion 1.6-5s after the server finished) | `PMPERF` timing around each step | `find_last_entry` scans from the end; supervisor tick = messages + permissions + questions |
| Visible follow-ups spent 0.7s before the operation existed: `bind_existing_worker` looked a live pane id up as a persisted worker, scanning every operation of every indexed project twice | stage marks inside `run_agent` | follow-ups look up their handle; exact live pane ids skip the scan |
| Session table (1.2MB) serialized and written while holding the global lock | code + lock profile | background coalescing writer |
| Visible delivery polled every 250ms | `deliver_prompt_traced` | 80ms polls, same 2s/3s/1.5s budgets |
| Visible Claude (v2.1.287): text+Enter in one burst is a paste, Enter dropped | transcript + frames | verified delivery protocol; status rows are chrome |
| Dev app started inside Claude Code leaked `CLAUDE_CODE_CHILD_SESSION`; claude panes behaved as nested sessions | pane banner | scrub the markers at startup |

## Results (median, before -> after)

#### Median wall-clock per turn (MCP call to returned result)

| scenario | turn | before | after | saved | n (b/a) |
|---|---|---|---|---|---|
| cursor_visible | cold | 45.2s | 10.7s | 34.5s | 3/3 |
| cursor_visible | followup | 10.8s | 4.8s | 6.0s | 6/6 |
| cursor_headless | cold | 32.2s | 23.6s | 8.6s | 3/3 |
| cursor_headless | followup | 29.9s | 23.5s | 6.4s | 6/6 |
| claude_headless | cold | 5.2s | 4.1s | 1.1s | 3/3 |
| claude_headless | followup | 5.1s | 4.3s | 0.8s | 6/6 |
| codex_headless | cold | 9.1s | 8.6s | 0.6s | 3/3 |
| codex_headless | followup | 9.2s | 8.1s | 1.1s | 6/6 |
| opencode_visible | cold | 15.6s | 7.1s | 8.5s | 3/3 |
| opencode_visible | followup | 16.5s | 6.4s | 10.1s | 6/6 |
| opencode_background | cold | 28.9s | 13.6s | 15.3s | 3/3 |
| opencode_background | followup | 18.6s | 11.6s | 6.9s | 6/6 |
| claude_visible | cold | n/a | 5.6s | n/a | 0/3 |
| claude_visible | followup | n/a | 3.6s | n/a | 0/6 |

#### Stage breakdown (median ms, before -> after)

| scenario | turn | mcp_and_create | spawn | pane_spawn | tui_ready | trust_to_dispatch | delivery | model | cli_to_first_output | first_output_to_done | completion_lag | return |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cursor_visible | cold | 6 -> 5 |  | 316 -> 107 | 21463 -> 3330 | 0 -> 0 | 1070 -> 580 | 2582 -> 5710 |  |  | 18961 -> 1132 | 56 -> 48 |
| cursor_visible | followup | 696 -> 9 |  | - -> - | - -> - | 0 -> 0 | 1966 -> 840 | 1972 -> 2562 |  |  | 5661 -> 1117 | 51 -> 47 |
| cursor_headless | cold | 7 -> 5 | 70 -> 72 |  |  |  |  |  | 28478 -> 20227 | 3355 -> 3247 |  | 30 -> 24 |
| cursor_headless | followup | 9 -> 6 | 72 -> 57 |  |  |  |  |  | 26445 -> 19786 | 3430 -> 3262 |  | 32 -> 25 |
| claude_headless | cold | 7 -> 5 | 76 -> 56 |  |  |  |  |  | 3934 -> 3040 | 945 -> 945 |  | 33 -> 24 |
| claude_headless | followup | 9 -> 6 | 69 -> 55 |  |  |  |  |  | 3946 -> 3173 | 947 -> 958 |  | 32 -> 25 |
| codex_headless | cold | 6 -> 4 | 74 -> 62 |  |  |  |  |  | 436 -> 283 | 8419 -> 7033 |  | 33 -> 24 |
| codex_headless | followup | 9 -> 6 | 80 -> 57 |  |  |  |  |  | 318 -> 207 | 8652 -> 7726 |  | 32 -> 25 |
| opencode_visible | cold | 6 -> 6 |  | 2187 -> 1291 | 1205 -> 79 | 0 -> 0 | 1905 -> 1435 | - -> - |  |  | 4285 -> 49 | 1238 -> 66 |
| opencode_visible | followup | 821 -> 9 |  | - -> - | - -> - | 0 -> 0 | 5860 -> 1814 | - -> - |  |  | 3417 -> 1547 | 1317 -> 908 |
| opencode_background | cold | 7 -> 5 | - -> - |  |  |  |  |  | - -> - | - -> - |  | 1715 -> 1040 |
| opencode_background | followup | 821 -> 12 | - -> - |  |  |  |  |  | - -> - | - -> - |  | 1353 -> 862 |
| claude_visible | cold | - -> 5 |  | - -> 120 | - -> 1378 | - -> 0 | - -> 547 | - -> 2043 |  |  | - -> 1259 | - -> 38 |
| claude_visible | followup | - -> 9 |  | - -> - | - -> - | - -> 0 | - -> 689 | - -> 1598 |  |  | - -> 1201 | - -> 43 |


Columns: `mcp_and_create` = MCP call to operation snapshot created; `spawn`/`pane_spawn` = process launch /
pane registration; `tui_ready` = until dispatch-ready or the startup dialog; `trust_to_dispatch` = after the
dialog is answered until the prompt is typed; `delivery` = type + submit verification; `model` = dispatch to
reply readable on screen; `cli_to_first_output` / `first_output_to_done` (headless) = CLI start to first stdout,
then to completed; `completion_lag` = screen final to operation completed; `return` = completed to the MCP
call returning. Native OpenCode has no screen-derived `model` column: its completion lag is HTTP polling
(1.6-5s before, ~0.1-0.7s now; see commit `perf(native)`).

## Polling intervals that add latency (after)

| Where | Interval | Note |
|---|---|---|
| `pty/registry/idle.rs` watcher | 100ms (was 600ms) + idle window | window: 700/800ms quiet, 5s otherwise |
| `headless/supervise.rs` `SUPERVISE_POLL_MS` | 250ms | `STABLE_IDLE_TICKS=2` => 250-500ms after idle |
| `agent_runs/delivery.rs` | 80ms (was 250ms), clear pause 120ms | budgets unchanged |
| `bridge/pane_util.rs` `wait_for_pane_idle` | 100ms poll, 250ms settle (was 700ms) | |
| headless exit poll / result poll / exit grace | 75ms / 400ms / 3000ms | Cursor never exits after its result, so every headless Cursor turn ends via the 3s grace (left as is: killing earlier could lose the resumable session) |
| `opencode/delivery.rs` ack poll | 250ms | unchanged |
| MCP shim hop | ~4ms over the bridge's own ~10-15ms per HTTP request | |

## Not PM's: provider CLI startup

Headless Cursor spends ~20-26s before its first output (`cursor-agent --print` run by hand took 59s end to end
with `duration_ms: 7059` in its own result), headless Claude ~3s, Codex ~0.3s then model time. A warm pool cannot
remove that unless it keeps one CLI process alive across turns.

## Warm-pool estimate (startup share a pool could remove, after the optimizations)

Cold turn minus follow-up turn on the same handle, and the stage-based sum (spawn + TUI ready + dialog + settle):

| scenario | cold median | follow-up median | startup share (cold - follow-up) | stage-based (spawn + TUI ready + dialog + settle) |
|---|---|---|---|---|
| cursor_visible | 10.7s | 4.8s | 5.9s | 3.4s |
| cursor_headless | 23.6s | 23.5s | 0.1s | 0.1s |
| claude_headless | 4.1s | 4.3s | -0.2s | 0.1s |
| codex_headless | 8.6s | 8.1s | 0.5s | 0.1s |
| opencode_visible | 7.1s | 6.4s | 0.7s | 1.4s |
| opencode_background | 13.6s | 11.6s | 2.0s | 0.0s |
| claude_visible | 5.6s | 3.6s | 2.0s | 1.5s |

Reading: visible Cursor ~3.4-5.9s (pane spawn, TUI start, trust dialog; a pre-trusted pool folder removes the
dialog round trip), visible Claude ~1.5-2s, native OpenCode ~1.3-2s (serve spawn + health + session + TUI
attach), headless backends ~0.1s (process launch only; the CLI's own startup stays unless a persistent
stream-json process is pooled for Claude).

## Known leftovers

- Native OpenCode: ~0.9s between completion and the waiter returning on follow-ups (publish path).
- Every event-log `PaneOutputObserved` chunk is appended and flushed; coalescing unchanged-screen chunks would cut I/O but changes the audit stream.
- `rebuild_read_models` still parses the whole event log per task/lock/session route.
- Visible Claude reply text keeps its `●` marker.
