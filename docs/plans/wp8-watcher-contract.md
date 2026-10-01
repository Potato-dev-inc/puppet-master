# WP8: Watcher CLI + MCP contract fixes

## A. `pm-watch` (CLI `watch` subcommand)

| Item | Root cause (file:line) | Fix |
|------|------------------------|-----|
| No push after MCP wait timeout | MCP tools are request/response (`agent-parity-plan.md` WP3 §6); bridge already has `POST /operations/{id}/wait` (`bridge.rs` ~1174) and `GET /events` SSE (`bridge.rs` ~1118) | Add `packages/cli` `watch` using `readBridgePort` (`packages/shared/src/bridge-port.ts`) + long-poll wait |
| Watcher blocked without MCP session | `mcp_sessions::authorize` denies managed handles without session (`mcp_sessions.rs` ~543–558) and enforces `check_run_access` on `get_operation` / `wait_for_operation` (`mcp_sessions.rs` ~639–671) | Allow read-only operation GET/wait/lookup without ownership; smallest change in `authorize` only |
| Handle vs operation_id | `resolve_agent_run` exists (`operations.rs` ~404) but no HTTP route | `GET /operations/by-handle` in `bridge.rs` next to `by-key` (~1140) |

## B. `watch_command` on tool responses

| Item | Root cause | Fix |
|------|------------|-----|
| Missing hint after run/followup | No field in `AgentRunView` (`agent_runs/runtime.rs` ~144) or timeout recovery (`bin/puppet-master-mcp.rs` ~1003) | `watch_command.rs` + attach on run/followup/delegate + recovery JSON |
| Docs | `tool_registry::mcp_instructions` (~217), tool descriptions | Document + `quickstart.md` background example |

## C. Contract bugs

### C1 — `context_mode` + schema defaults disagree

- **Cause:** `tool_registry.rs` ~258 sets `default:"packet"` on both `context_policy` and `context_mode`; hosts inject both; `normalize_context_policy_fields` (`agent_runs/runtime.rs` ~80–86) errors when `packet` vs `resume`.
- **Fix:** Remove schema defaults; normalizer prefers explicit `context_mode` when values differ; treat `packet` as unset when paired with non-packet mode.

### C2 — `resolved_model` null in run results

- **Cause:** `resolve_model_choice` (`agent_runs/persist.rs` ~129) only copies request; live pane model is filled in dispatch (`bridge.rs` ~3602) but not merged into `AgentRunView` (`runtime.rs` ~298).
- **Fix:** `live_resolved_model` from `inspect_agent_model_with_registry` when view lacks model.

### C3 — acceptance stays `not_checked`

- **Cause:** `stored_acceptance_criteria` (`headless.rs` ~310–316) stores `DEFAULT_ACCEPTANCE` when omitted; evaluator (`operations.rs` ~1469) substring-matches result — never passes for normal answers.
- **Fix:** Do not persist default criteria when caller omitted them; keep evaluator honest (`not_checked` when no criteria).

### C4 — `delegate_work` ignores `context_policy`

- **Cause:** `DelegateWorkRequest` has no field (`operations/model.rs` ~31); worker stays `ContextPolicy` default (`operations/tests.rs` ~25 `Default::default()`).
- **Fix:** Add optional `context_policy` on request; apply in `create_operation`.

### C5 — noisy `output_baseline` / `result_capture: missing` mid-run

- **Cause:** `snapshot_for_api` compacts but still exposes baseline (`operations.rs` ~1552); `AgentRunView::from` maps `None` → `"missing"` (`runtime.rs` ~257–260) for non-terminal ops.
- **Fix:** Drop `output_baseline` from API snapshots; emit `result_capture: "pending"` until terminal.

### C6 — `project_path` os error 3

- **Cause:** Relative paths fail `root()` absolute check (`operations/store.rs` ~35) or `canonicalize` on missing path (`bridge.rs` ~2595); `.puppet-master` not created before some writes.
- **Fix:** `prepare_project_path` — expand `~`, absolutize relative to cwd, verify directory, `create_dir_all` for `.puppet-master`.

## Tests (TDD)

- `normalize_context_policy_fields`: mode-only-with-injected-packet default
- `mcp_sessions`: authorize GET wait without session on managed op
- `project_path`: relative existing dir + storage dir created
- `headless`: omitted acceptance stores empty / none
- `operations`: existing acceptance tests unchanged
- CLI: `watch-command` shell quoting unit test in shared or cli

## Status

### Changed
- **A:** `packages/cli/src/watch.ts` + `watch` subcommand; bridge `GET /operations/by-handle`; `mcp_sessions::readonly_operation_route` + removed `get_operation`/`wait_for_operation` from ownership gate; `mcp_sessions::tests::readonly_operation_wait_allowed_without_session_for_managed_runs`
- **B:** `watch_command.rs`; `watch_command` on `AgentRunView`, operation HTTP JSON, `WAIT_TIMEOUT` recovery; tool catalog + `mcp_instructions` + Node MCP instructions + `quickstart.md`
- **C1:** `normalize_context_policy_fields` prefers `context_mode`, treats schema `packet` as unset; removed `default` on `context_policy`/`context_mode` in `tool_registry.rs`
- **C2:** `persist::live_resolved_model` + `AgentRunView::present`
- **C3:** omitted acceptance no longer stored; followup does not inherit criteria; evaluator unchanged (`operations.rs` `evaluate_acceptance_criteria`)
- **C4:** `DelegateWorkRequest.context_policy` wired in `create_operation`
- **C5:** `snapshot_for_api` drops `output_baseline`; non-terminal `result_capture: pending`
- **C6:** `project_path::prepare_project_path` (expand, absolutize, mkdir project + `.puppet-master`); used in `create_operation`, `run_agent`, `delegate_operation_http`
- **Extra:** `visible_runs` `owned` flag no longer treats read-only visibility as ownership (`routes.rs`)

### Tests passing (by name)
- `cargo test --lib`: **318 passed** (includes `run_agent_request_treats_schema_packet_default_as_unset`, `run_agent_request_prefers_context_mode_when_both_differ`, `delegate_work_honors_context_policy`, `followup_does_not_inherit_previous_acceptance_criteria`, `readonly_operation_wait_allowed_without_session_for_managed_runs`, `watch_command::tests::watch_command_includes_project_when_set`, `prepare_project_path_creates_storage_dir`, acceptance evaluator tests in `operations::tests`)
- `cargo test --bin puppet-master-mcp`: run locally after rebuild
- `npm run typecheck` — `@puppet-master/cli`, `@puppet-master/mcp-server`

### Unverified
- Live `npx puppet-master watch` against a running bridge (manual)
- End-to-end MCP host background watcher harness
- `node scripts/bundle-mcp.mjs` + Cursor MCP reload on this machine

### Agent harness note (integration)
Run `watch_command` as a normal foreground CLI through the harness shell tool's background mode. Detached launchers (e.g. PowerShell `Start-Process`) do not notify Cursor/Claude harnesses when the watch exits.

### Catalog refresh
1. `cargo build --release -p puppet_master_app --bin puppet-master-mcp` from `packages/app/src-tauri`
2. `node scripts/bundle-mcp.mjs` from repo root
3. Reload MCP host (toggle `user-puppet-master`) so `tools/list` matches `GET /mcp/tools`
