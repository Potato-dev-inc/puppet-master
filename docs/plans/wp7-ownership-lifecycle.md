# WP7: Ownership, reconnect, wait timeout, pane teardown

Live-session bugs (Oct 2, 2026). Ground rules: repro test first, minimal diff, no new deps.

## Bug 1 & 2 — Runs orphaned; handles show `owned:false` after wait timeout / reconnect

### Root cause

| Location | Issue |
|----------|--------|
| `mcp_sessions.rs:264-275` `take_over_agent` | `grant=true` still returns *"active agents cannot be taken over"* when another session holds `controller`, unlike `take_over_pane` which preempts with `grant=true` (`312-330`). Error text says use `take_over(grant=true)` but that path is dead. |
| `bridge.rs:3229-3230` `handoff_route` | Calls `check_run_access` **before** `take_over_agent`, so handle-based `take_over(grant=true)` always fails with *"does not control the agent; use take_over with grant=true"* — circular. |
| `mcp_sessions.rs:639-671` `authorize` | `inspect_agent`, `agent_transcript`, and `wait_agents` call `check_run_access` at the HTTP gate; after a new stdio session UUID (`stdio.rs:9`) without `attach_agents`, the ledger still points at the old connection → `owned:false` and all tools denied. |
| `stdio.rs:9` | New MCP process = new `session_id`; ledger is in-memory (`mcp_sessions.rs:38-44`). Recovery is `attach_coordinator` (`437-497`) but errors never mention it. |
| `agent_runs/routes.rs:357-362` | `owned` is derived from `check_run_access`, so it flips false when the ledger disagrees even though the persisted run is unchanged. |

### Fix

1. `take_over_agent`: mirror pane preemption — `controlled_by_other && grant` transfers control; improve denial when `grant=false`.
2. `handoff_route`: only `check_run_access` on **release**; on take, run `take_over_agent` with `grant`. Allow handle-only reclaim when pane is gone: `take_over_agent` only, optional empty screen.
3. Split access: `check_run_read_access` for `inspect_agent` / `agent_transcript` (persisted run visible to any session that can resolve the handle; no control required). `check_run_wait_access` for `wait_agents` (same; wait must not mutate lease).
4. `check_run_access` denial: name `attach_agents(attach_token)` from `session_identity` when coordinator matches, else `take_over(handle, grant=true)`.
5. `visible_runs` `owned`: true when `check_run_access` **or** `owner_session_id == session` **or** released lease reclaimable.

## Bug 3 — Orphan opencode processes after pane `gone`

### Root cause

| Location | Issue |
|----------|--------|
| `pty/registry.rs:734-737` `shutdown_pane` | `child.kill()` on Windows does not kill the process tree; opencode `serve` child survives (`opencode/link.rs:63-70` only kills direct child). |
| `bridge.rs:3107-3115` | Pane cleanup on terminal op uses `registry_kill_pane`; if shutdown is incomplete, serve PIDs linger. |

### Fix

- `shutdown_pane` (Windows): `taskkill /PID /T /F` on PTY child PID (same pattern as `headless.rs:1159`), then `kill_serve_if_present`.
- Unit test with mock is limited; add `#[cfg(test)]` helper assertion on Windows command path or document manual check.

## Bug 4 — `wait_agents` `timeout_ms=180000` → generic client timeout

### Root cause

| Location | Issue |
|----------|--------|
| `puppet-master-mcp.rs:244` | `wait_agents` uses single long `bridge_request_wait_agents` (`816-864`); host MCP clients often abort the JSON-RPC request (~60–120s) while the bridge would still wait. |
| `stdio.rs:137-138` | Only `wait_for_operation` uses chunked waits + `notifications/progress` (`128-207`); `wait_agents` does not. |

### Fix

- Route `wait_agents` through the same chunked loop as `wait_for_operation` (4s bridge chunks), emit progress when `progressToken` present.
- On overall deadline without terminal `until`, return structured pending (`reason: timeout`) with per-agent `next_cursor` / views — not an error; **no lease mutation**.

## Bug 5 — `wait_agents` / `inspect_agent` reject pane IDs

### Root cause

| Location | Issue |
|----------|--------|
| `agent_runs/routes.rs:510-511` | Passes raw `handles[]` to `current_agent`; pane UUIDs are not run handles. |
| `mcp_sessions.rs:756-777` | Enrichment only on AGENT_NOT_FOUND after registry lookup, not at wait/inspect entry. |

### Fix

- `normalize_run_handle(project, id)` → resolve pane id to latest non-closed `agent_run_id` for that pane, else clear error: *"{id} is a pane id; use handle {h}"*.

## Bug 6 — `switch_agent_model` requires pane takeover; `keep_pane:false` closes pane

### Root cause

| Location | Issue |
|----------|--------|
| `mcp_sessions.rs:676-709` | Pane mutations require `session_controls_pane`; run lease does not imply pane control. |
| `puppet-master-mcp.rs:351-362` | Only accepts `pane_id`. |
| `bridge.rs:3107-3108` | `!keep_pane` kills pane when run completes — document; switching on owned handle should not require separate `take_over`. |

### Fix

- `authorize`: for `switch_agent_model`, allow pane when `session_controls_pane` **or** `check_run_access` on run bound to that pane (`latest_handle_for_pane`).
- MCP binary: accept `handle` OR `pane_id`, resolve to pane.
- Document `keep_pane` in `docs/plans/wp7` status (tool_registry note only if needed).

## Tests (TDD)

- `mcp_sessions`: `take_over_agent_grant_preempts_stale_controller`; update `agent_handles_are_private` to use release before cross-session read; `take_over_by_handle_without_prior_access` (grant).
- `bridge` / `mcp_sessions`: handoff take does not require prior control.
- `agent_runs`: `normalize_run_handle_from_pane_id`; `inspect_read_without_control`.
- `puppet_master_mcp::tests`: `wait_agents_chunks_bridge_calls` (mock or inspect chunk size via helper).
- `pty/registry`: Windows tree kill behind `cfg(windows)` compile test.

## Status (Oct 2, 2026)

### Changed

- **#1–2:** `take_over_agent` preempts with `grant=true`; `handoff_route` no longer requires prior control on take; handle-only reclaim when pane is gone; `check_run_read_access` / `check_run_wait_access`; clearer errors (`attach_agents`, `take_over`); fixed mutex deadlock in `control_denied`.
- **#3:** Windows `shutdown_pane` uses `taskkill /PID /T /F` before `child.kill()`.
- **#4:** `wait_agents` MCP tool uses 4s chunked bridge waits + `notifications/progress` when `progressToken` is set (`call_wait_agents_with_progress`).
- **#5:** `resolve_run_handle` in `agent_runs/routes.rs` maps pane ids to run handles with explicit error text.
- **#6:** `switch_agent_model` accepts `handle` in MCP binary; bridge auto `take_over_pane(grant=true)` when session owns run on that pane; authorize bypass when `handle` in body owns run.
- **keep_pane:** Default `false` on `run_agent` still disposes **spawned** panes when a turn completes (`bridge` publish path). Set `keep_pane: true` to retain the pane for later `switch_agent_model` / `take_over`. Adopted UI panes are not auto-killed by `pane_close` policy.

### Tests passing (by name)

- `mcp_sessions::tests::*` — **27/27** (including `take_over_agent_grant_reclaims_without_prior_control`)
- `puppet_master_mcp::stdio::tests::*` — **25/25**
- `cargo test` full lib — **264 passed, 54 failed** (failures are predominantly `INVALID_PROJECT_PATH` / missing temp dirs in `operations::tests` and downstream `agent_runs::tests`; appears to be in-flight work on `operations.rs` project-path validation, not WP7 ownership changes)

### Unverified

- Live MCP reconnect + `wait_agents` 180s with Claude Code progress token
- Windows manual check: opencode serve tree gone after pane kill
- Full lib suite green (blocked on parallel `operations` path validation failures)

### Catalog refresh

- Rebuild: `cargo build --release -p puppet_master_app --bin puppet-master-mcp` from `packages/app/src-tauri`
- Rebundle: `node scripts/bundle-mcp.mjs` from repo root
- Reload MCP host so stdio picks up `wait_agents` chunking + `switch_agent_model` handle support
