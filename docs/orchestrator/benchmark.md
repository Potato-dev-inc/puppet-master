# Backend benchmark

`scripts/pm-regression.mjs --bench` compares Puppet Master backends (`opencode_native`,
`cursor_agent`, `codex`, `claude`, ...) on the same tasks, so PM can be weighed against a
native-subagent-style baseline (a small task answered in-process in a few seconds). It drives the
real MCP binary against the running app, so a live run spawns real agents and costs money and time.

## Running

```
node scripts/pm-regression.mjs --dry-run                      # plan + fixture self-check, no backend
node scripts/pm-regression.mjs --bench --repeat 5 --bench-out bench.json
node scripts/pm-regression.mjs --bench --backends opencode_native,codex --tasks small
node scripts/pm-regression.mjs --bench --tasks fix_bug,dedupe --max-p95 300
```

| Flag | Meaning |
| --- | --- |
| `--bench` | Run the benchmark instead of the regression scenarios. |
| `--backends a,b` | Backends to run. Default: those of `opencode_native`, `cursor_agent`, `codex`, `claude` whose CLI is on `PATH`. |
| `--tasks small,multi` | Groups and/or task ids (default `small,multi`). |
| `--repeat N` | Runs per (backend, task). Use 5+ for a meaningful p95. |
| `--bench-out f.json` | JSON report path (default `pm-bench-<timestamp>.json` in the cwd). |
| `--max-p95 S` | Optional budget: exit 1 if any group's p95 total exceeds S seconds. Without it only correctness fails the run. |
| `--task-timeout S` | Per-run limit before the run is cancelled (default 180 small, 600 multi). |
| `--dry-run` | Print the plan, then prove each task is sound: its checks must fail on the untouched fixture and pass after a built-in reference solution. Never starts the MCP binary. |
| `--project dir` | Use an existing folder as the sandbox (never deleted). |

Every run gets a fresh copy of a generated fixture (a small CommonJS cart/report project with
`src/*.js` and `node --test` tests), so runs never see each other's edits.

## Tasks

Small (native-style): `pong`, `create_file`, `rename_symbol` (one symbol, one file),
`read_question`. Each small task also runs a **warm** phase: a `followup_task` on the same handle
right after the cold run.

Multi-file: `rename_fn` (4 files), `add_field` (feature plus its test), `fix_bug` (seeded bug, a
test fails until fixed, tests must stay untouched), `add_module` (new module wired in), `dedupe`
(duplicated function moved to a shared module).

## What the table means

Seconds unless noted. One group is (backend, task, phase), phase = `cold` or `warm`.

- `n`: runs in the group.
- `p50`, `p95`, `min`, `max`: total wall time from the `run_agent` call (or followup) until the
  turn is terminal. p95 is interpolated, so with few runs it is close to max.
- `ttfh50`: median time until the call returned a handle (spawn and dispatch overhead).
- `pass`: runs that completed and whose machine checks pass when re-run locally by the benchmark.
- `verified`: pass runs where PM also reported acceptance passed (`verified:true`). A run where PM
  says verified but the local check disagrees is a failure, not just a lower rate.

The JSON also has per-run rows: status, handle, `started_at`/`completed_at` from `inspect_agent`
(when the backend reports them) and `server_s`, their difference, which excludes client-side
polling.

## Adding a task

Append an entry to `TASKS` in `scripts/pm-regression.mjs`:

- `id`, `size` (`small` or `multi`), `prompt`.
- `checks`: machine checks (`file_contains`, `command_exits`, `result_contains`). Prefer
  `command_exits` with `node --test` plus a behavioural `node -e` assertion over text matching.
- `setup(dir)` (optional): seed the fixture, e.g. introduce a bug.
- `solve(dir)`: a reference solution (return the reply text for result checks). Only `--dry-run`
  uses it, to prove the checks are satisfiable and not already true on the untouched fixture.
- `warm` (small tasks only): `{ task, checks, solve }` for the followup phase.

Run `--dry-run` after every change; it must end with `all fixture self-checks passed`.

## Caveat

Results are per-machine and per-moment: CPU, network, model load, the backend CLI version and the
model chosen all move the numbers. Compare backends within one run on one machine, keep `--repeat`
high enough to see the spread, and do not treat numbers from another machine as a baseline.
