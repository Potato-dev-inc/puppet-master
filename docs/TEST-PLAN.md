# Puppet Master test plan

A practical checklist a human or agent can run. Features under test: [`FEATURES.md`](FEATURES.md). Map: [`REPO_MAP.md`](REPO_MAP.md).
Counts are the last recorded values (T37 and later work on `refactor/tui-profiles`); treat them as floors, not exact pins.

Rules for live tests: the dev app runs in the "puppet-master dev app" terminal tab; do not start/stop `npm run dev`. Windows must stay on the
secondary (left) monitor: after any visible-pane test run
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-window-monitor.ps1` and expect `OK: no Puppet Master window on the primary monitor`.
Editing files under `packages/app/src-tauri` restarts the dev app (~1 min; poll `http://127.0.0.1:17321/health`).

## 1. Automated suites

| Suite | Command (from repo root unless noted) | Expected |
|---|---|---|
| Rust lib | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\Ren-pc\cargo-target-shared'; cargo test --lib` | about 907+ passed, 0 failed, 2 ignored (893 at T37, more since) |
| Rust: TUI only | `cargo test --lib tui::` (same dir/env) | about 252+ passed, 1 ignored |
| Rust: tool registry | `cargo test --lib tool_registry` | about 22 passed (includes the pinned `catalog_version`) |
| Rust: standby | `cargo test --lib standby` | about 36 passed |
| MCP shim bin | `cargo test --bin puppet-master-mcp` (same dir/env) | about 29 passed |
| App unit tests | `cd packages/app; npx vitest run` | about 235+ tests (227 at T37), all files pass |
| Typecheck | `npm run typecheck` (workspaces) | clean for shared, app, mcp-server |
| CLI tests | `npm test --workspace=@puppet-master/cli` | pass |
| MCP vs mock bridge | `npm run test:mcp-operations` / `npm run test:mcp-package` | pass (not run at T37) |
| Live regression | `node scripts/pm-regression.mjs` (needs the running app; spawns real agents) | `28/28 scenario runs passed; 0 failed assertion(s)` |
| Read-only parity | `node scripts/pm-regression/readonly-check.mjs` | claude and kilo PASS; the control (a write) is detected |
| Window monitor | `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-window-monitor.ps1` | exit 0, `OK: no Puppet Master window on the primary monitor` |
| Perf (optional) | `node scripts/perf-latency.mjs` then `perf-summarize.mjs` | see `docs/perf/latency-baseline.md` |

`cargo test` needs `packages/app/dist` (run `npx vite build` in `packages/app` once). After any change to `tool_registry/`, the shim or bridge code it
depends on, rebuild the dist MCP exe with a separate `CARGO_TARGET_DIR` and rename-swap (REPO_MAP section 6).

## 2. Manual / live cases

Setup: dev app running, bridge healthy (`GET /health` gives `build_id`), `kilo` installed and its definition approved. "Bridge" = `http://127.0.0.1:17321`.

### 2.1 Agent definitions (Settings > Agents)
1. **Import to active (Kiro).** Import `kiro.toml` (Import button). Expect: row appears at once, state "Needs approval", no restart. Open Review: argv shown (`kiro-cli chat --output-format stream-json ...`), binary path, warnings, diff. Approve. Expect: state Active; toast says the registry updated, no restart needed. File > New session menu lists Kiro greyed (headless-only) with a reason; the Overview Agents card lists it. Both update without restart.
2. **Check with fixtures (Kilo).** Press Check on kilo. Expect `9/9 PASS`.
3. **No fixtures folder.** Check a definition that has no fixtures folder. Expect a neutral "no captures yet" message, not an error (this message was being fixed when this plan was written; verify it).
4. **Revoke.** Revoke kilo. Expect state Needs approval and `run_agent(agent_type=kilo)` refused with an `AGENT_DEF_*` message. Re-approve restores Active.
5. **Add as generic agent.** Add a new generic agent for an installed CLI. Expect the experimental badge and a "needs approval" flow.
6. **Rec button / drafts.** Open a pane, press Rec in its header, interact, stop. Expect a draft under Drafts (file in `<app data>/agents/drafts/`), never auto-loaded or approved. Validate the draft (Validate/Review) and copy it to the agents folder to approve.
7. **Export / import round trip.** Export kilo (zip), import it under a different folder or after deleting a copy. Expect it lands Needs approval (never auto-approved).
8. **Hot reload.** Edit a label in `<app data>/agents/kilo.toml` while the app runs. Expect the row refreshes within about a second, no restart (label-only edits keep the fingerprint). Edit argv: state turns "Approval stale".
9. **Invalid edit.** Break the TOML of an Active file. Expect the previous registry kept (agent still listed), the row reported invalid when it settles; fixing it recovers.
10. **Sources and consent.** Settings > Agents > Sources: project and extra dirs are OFF by default. Put a definition in `<project>/.puppet-master/agents/`; expect a consent banner for that project; nothing loads until consent is given, and even then it needs approval. Extra dir: add a dir, expect it needs its own consent.

### 2.2 Runs and receipts
1. **Headless run.** `run_agent(agent_type=kilo, project_path, task="reply ack")` (default headless). Expect result text, receipt with `agent_definition` {id, fingerprint, binary}, `read_only` status, no window opens.
2. **read_only.** Same with `read_only=true`; a task that writes a file must not write. Run `node scripts/pm-regression/readonly-check.mjs`.
3. **followup_task / resume.** `followup_task(handle, "what was the word?")`: same provider session id, `continuity: resume`.
4. **Visible run lands in the grid.** `run_agent(..., headless=false, keep_pane=true)` through MCP. Expect the pane in the main window's left grid, no separate window; result returned; close it with `close_agent`. Then run the window-monitor script.
5. **placement=window.** Same with `placement="window"`. Expect exactly one pop-out window (unfocused, near the main window, on the secondary monitor) that closes within 3 s after the run ends (without `keep_pane`). Expect no second window for the same pane.
6. **answer_prompt.** In a visible kilo run, make it write outside the project. Expect a `command_approval` prompt surfaced via `wait_agents` (`needs_input`); `answer_prompt(allow_once)` lets the turn finish.
7. **Mid-run edit.** Start a long kilo run, edit `kilo.toml` argv during the run. Expect the in-flight run completes with its result; the next `run_agent` is refused with `AGENT_DEF_APPROVAL_STALE`; restoring the file (or re-approving) makes it Active again.
8. **UNKNOWN_AGENT_TYPE.** `run_agent(agent_type="nonesuch")`: HTTP 400 `UNKNOWN_AGENT_TYPE` with `context.valid` listing registered ids (not `AGENT_NOT_DISPATCHABLE`).
9. **Keep pane.** Without `keep_pane`, after completion `take_over` fails with `PANE_CLOSED_AFTER_RUN`; `agent_transcript` still works.
10. **Slow-start TUI.** A visible run on a TUI that takes seconds to draw: the prompt is delivered once drawn (up to 20 s), no lost first turn.

### 2.3 Windows and lifecycle
1. **Dev window position.** Launch via the dev app: the main window appears on the left monitor, unfocused. The monitor script exits 0.
2. **Pop-out is unfocused / one per pane.** From a pane's detach action, double-click it fast: exactly one pop-out. Closing the pane (or `kill_pane_process`) closes its pop-out immediately.
3. **Orphan window.** Close a pane while its pop-out is open; the pop-out closes and an X on a stale window is honoured.
4. **Single-instance.** Start a second launch of the app (user does this by hand, on the left monitor; an agent must not launch a second instance). Expect the existing window to come to front and the second process to exit.
5. **Port file and fallback.** Check `puppet-master.bridge.port` in the app data dir names the live bridge port; after a clean exit it is removed only if it names that process's port. With a stale port in the file, an MCP client (`bridge_health`) still connects by probing 17321-17330.

### 2.4 Security
1. **MCP session cannot mutate definitions.** `POST /agents/definitions/approve` (and `/consent`, `/revoke`) with header `X-Puppet-Master-Session: x`: 403 `AUTHORIZATION_DENIED`.
2. **Remote/tunnel.** With `Authorization: Bearer x` or `X-Forwarded-For: 1.2.3.4`: 403 "only from the local desktop app"; with `X-PM-Proxied: 1`: 401 `pairing_required`. A plain local call reaches the handler (for example 400 "file is required"). Use nonexistent ids so nothing changes.
3. **Binary denylist.** A definition with `binary.names = ["python"]` (or `cmd`) fails validation.
4. **Argv injection.** A task like `a; rm -rf / ; b` or `--flag` reaches the CLI as a single argv element (unit tests `template::task_is_one_argv_element`; do not run destructive tasks live).
5. **Hostile project folder.** A project containing `.puppet-master/agents/*.toml` loads nothing until consent plus approval.

### 2.5 External MCP clients
Codex as an external client (PowerShell-quoted; adjust the model id if unavailable):
```
codex exec -m gpt-6-luna -c model_reasoning_effort=low -c 'mcp_servers.puppet-master.default_tools_approval_mode="approve"' "Use puppet-master run_agent with agent_type=kilo to reply ack in <project>"
```
Expect Codex to call `run_agent` and return the handle/result. Also `wait_agents`, `followup_task`. Check `list_agents` shows panes created.

### 2.6 Standby pool (opt-in)
Enable standby for Claude/Cursor in Settings (Performance/Orchestrator). Run a task twice; the second run claims the warm worker (faster first turn, no hidden-pane leak: `list_panes` after close). Disable again. Skip if not needed; off by default.

### 2.7 Mobile PWA
Unverified on this branch. If testing: Settings > Mobile, pair a phone, open the PWA, list panes, send a prompt.

## 3. Known flaky tests (collected from the progress log and T37 evidence)
- `tui::descriptor::descriptor_builds_all_nine` - global registry race (fixed by `registry::test_serial()`; tests that swap/count the global registry must take it).
- `operations_contract_tests::recovery::startup_recovery_releases_locks_held_by_interrupted_operations` - "Access denied" on Windows; rerun.
- `operations::tests::recovery::recover_all_visits_nonselected_indexed_projects` - seen once (T26).
- `pm-regression` cursor scenario - `WORKER_STOP_FAILED` taskkill flake once (27/28); rerun gives 28/28.
- Free kilo model (`kilo/kilo-auto/free`) sometimes refuses tasks or reports `model_mismatch: true`; not a code bug.
- Rebuild windows: any edit under `src-tauri` drops the bridge for about a minute; a live run during that fails spuriously.

## 4. Known gaps / not proven live
- Clean-state Kilo approval flow (delete approvals, re-approve) was skipped at T37 (classifier denied); needs_approval -> approve and the stale-edit refusal were proven live at T36/T36b and by unit tests.
- Remote/tunnel behaviour tested with headers on loopback only (no real tunnel).
- Kiro runs, the Kiro approve flow in the UI, and Rec to approved definition were not exercised live in this documentation pass.
- Visible declarative results are screen-inferred (flagged); free models may refuse.
- Env/stdin launches, `[mcp_host]` writer UI/route, SPEC Q6/Q7, T29 (blocked), `extract_model` parity: not implemented.
- `npm run test:mcp-package` / `test:mcp-operations` were not run at T37.
- Mobile PWA and standby pool were not re-verified on this branch.
