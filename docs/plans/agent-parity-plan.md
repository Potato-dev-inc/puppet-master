# Plan: native-subagent parity for existing Puppet Master workers

Source: consolidated handoff (Oct 1, 2026) + `puppet-master-feedback.md` (chronological evidence; the handoff wins on conflicts).
Code survey: Oct 2, 2026. Line numbers are approximate; re-read before editing. All paths are under `packages/app/src-tauri/src/` unless noted.

## Goal

Using an already-open worker should feel like a native subagent: give it work, steer it, get its answer, and continue the same conversation through one handle, without dealing with transport or process details.

## Ground rules for implementing agents

- **Reproduce before patching.** Every work package (WP) with a "Repro" step must have a failing test or a recorded manual repro before you change code. If it doesn't reproduce, write that down in the WP's status and move on.
- Keep what already works: discovery of the UI OpenCode worker, handle-bound `project_path`, `run_agent(pane_id)` adoption, same-session follow-ups, and the 120 s follow-up wait.
- Use TDD with unit tests for state transitions and fault injection. Add one end-to-end test for the ordinary conversation path.
- Don't add an umbrella or facade tool unless docs alone can't remove the decision (see WP1).
- Don't claim something is fixed because normal work succeeded. Only a passing failure-path test counts.
- Never close the user's pane as cleanup.

## Suggested order and parallelism

| Order | WP | Can run in parallel with |
|---|---|---|
| 0 | **WP0 run_agent blockers (do first, small)** | none; WP1 depends on it |
| 1 | WP2 timeout and idempotency (highest reliability) | WP4, WP6a |
| 2 | WP3 completion-oriented wait | WP4 |
| 3 | WP1 docs and happy path (after WP2 and WP3 settle names and semantics) | WP5 |
| 4 | WP4 Windows stop | WP2, WP3 |
| 5 | WP5 session rotation | WP1 |
| 6 | WP6 contract consistency (6a catalog tests can start early) | everything |
| last | Final validation | none |

Shared hot files: `bin/puppet-master-mcp.rs`, `agent_runs/runtime.rs`, `operations.rs`, `tool_registry.rs`. Agents working in parallel must coordinate on these files or rebase often.

---

## WP0: run_agent blockers (live trial, Oct 2 2026)

A user trial on a fresh UI OpenCode pane found two bugs. `delegate_work` on the same pane completed in about 11 s, and `acceptance_status` correctly stayed `not_checked`. But `run_agent(pane_id)` is unusable, which breaks the WP1 happy path.

### A. `duplicate field context_policy at line 1 column 111`
- Cause: `AgentRunRequest.context_policy` has `#[serde(default, alias = "context_mode")]` (`agent_runs/runtime.rs` ~89–90). The same pattern is on `routes.rs` ~211–212. When the JSON contains **both** keys, serde fails. The `run_agent` input schema (`tool_registry.rs` ~258) advertises only `context_mode`, with default `"packet"`. Callers still send `context_policy` too, because `inspect_agent` returns and describes `context_policy` (~339, ~192) and `docs/orchestrator/agent-shell-modes.md` ~22 mentions both. Some hosts may also inject the `context_mode` schema default.
- Nothing in the MCP binary or Node server merges the keys; both forward args as-is. `delegate_work` works because `DelegateWorkRequest` has no alias.
- There is an existing test that *expects* the error: `agent_runs/tests.rs` ~59–66. Change it.
- Fix: accept both keys. If they are equal, use the value. If they differ, return a clear `INVALID_ARGUMENT` that names both keys. Apply this to every struct that uses the alias: `runtime.rs` 89 and `routes.rs` 211, plus followup's `context_mode` (~279). Use a small pre-deserialize normalizer in `routes.rs` `parse` (~248), or a custom `deserialize_with`. Pick one name for the public schema (`context_policy`, so it matches the output) and keep the other as an input alias.

### B. `pane control requires take_over with grant=true` without a prior take_over
- Cause: `mcp_sessions.rs` `authorize` (~674–685) runs in `bridge.rs` ~876–889 **before** `agent_runs::handle_request` (~934). Any mutating tool with `body.pane_id` that isn't in the session ledger gets denied. UI panes only enter the ledger through `take_over_pane`, which `bind_existing_worker` (`headless.rs` ~382–385, ~423–425) would call. That call never runs, so the internal adopt-and-grant is dead code for pane_id calls. Only `take_over` is exempt (~676). `delegate_work` without `pane_id` skips the check.
- Fix: let `/agents/run` with a live registry pane that's not in the ledger pass `authorize`, and let `bind_existing_worker` do `take_over_pane(.., true)`. Keep the denial when **another** session owns the pane. Make the error message distinguish "owned by another coordinator" from "not yet adopted".
- Test: add to `mcp_sessions.rs` `mod tests`. Call `authorize(Some(session), "POST", "/agents/run", {task, pane_id})` with the pane in the registry but not the ledger, and expect Ok. Add the same call with another session owning the pane, and expect denied.

### C. Pane IDs change after app restart (expected, document it)
- `pty/registry.rs` ~293 creates a `Uuid::new_v4()` per spawn. The registry and the MCP session ledger are in memory only.
- Stable identities are the run **handle** and `list_agents` `worker_id`. `bind_existing_worker` can re-resolve persisted runs by `worker_id`, `handle`, or `name` (`persist.rs` ~108–117, `headless.rs` ~362–387).
- WP1 docs: say "after an app restart, call `list_agents` again; pane ids aren't stable, handles are." Optional (YAGNI unless asked): bind the new pane to an old handle when the UI respawns the same slot.

### Acceptance
On a fresh connection with no `take_over`, `run_agent({pane_id, task})` succeeds. It also succeeds when the call includes both `context_mode` and `context_policy` with equal values. Add a stdio round-trip test in `bin/puppet_master_mcp/tests.rs` if one is feasible. No MCP-binary-level test for `run_agent` + `pane_id` exists today.

Untested in the trial: `send_message` and `inspect_agent` on a `delegate_work` run. Retest them after WP0.

---

## WP2: trustworthy pending results and timeout recovery

### Current state
- `bin/puppet-master-mcp.rs` `call_tool_cancellable` (~234–265): `run_agent` and `followup_task` call `wait_timeout_recovery` on a bridge error. `send_message` (~239–246) has **no recovery**.
- `wait_timeout_recovery` (~879–901) looks for an identity in `handle`, `worker_id`, or `name` only. **A request with only `pane_id` returns the raw socket error** (10060), even though the turn may already exist. It returns `WAIT_TIMEOUT` with `recoverable`, `handle`, and `suggestion: wait_agents`, but no `operation_id` or `turn_id`.
- `is_bridge_timeout` (~905–911) matches on error text.
- Socket budget (`bridge_read_timeout_secs` ~843–873) is `wait_ms` plus 15 s, capped at 330 s. Defaults are 30 s for run and 120 s for followup.
- Idempotency:
  - `run_agent` accepts an optional `idempotency_key`. Without one, it defaults to a new UUID (`agent_runs/headless.rs` ~194–197). `operations.rs` `create_operation` (~93–170) returns the existing op when the key and fingerprint match.
  - **`followup_task` always mints `Uuid::new_v4()`** (`agent_runs/runtime.rs` ~646, and `enqueue_followup` ~744). `SendAgentRequest.idempotency_key` is ignored. A retry starts another turn (`turn_index + 1`, ~663/701).
- The operation is persisted as `Queued` before dispatch, but the sync HTTP response only arrives after the wait. `background: true` returns immediately (`headless.rs` ~222–226).

### Changes
1. **Caller-supplied idempotency on followup.** Thread `SendAgentRequest.idempotency_key` into `send_agent` and `enqueue_followup`. If the caller didn't supply one, the MCP binary generates a key per logical call and reuses it on its own retry or recovery. Same key returns the same operation; same key with a different fingerprint returns an error.
2. **Recovery for the pane_id-only path.** Before the long wait, the MCP binary needs to learn the handle and operation identity. Pick one option after reading the code:
   - (a) The MCP binary always sends an idempotency key. On timeout, it looks up the operation by key through a new cheap bridge GET, such as `/operations/by-key`.
   - (b) The run and followup routes create the operation, then wait, so identity is fixed before the wait starts. On timeout, the binary resolves it by key.
   - Prefer (a). It works for every entry identity, including pane_id-only.
3. **Distinguish four outcomes** in the recovery payload, using a structured `outcome` field rather than text:
   - `pending`: the operation exists with status queued, starting, running, or waiting_input.
   - `completed_delivery_failed`: the operation is terminal. Return its result inline so no extra call is needed.
   - `unknown`: the bridge is unreachable and no operation was found by key. Tell the caller to retry with the same key.
   - `not_dispatched`: the op is missing or failed with `DISPATCH_FAILED` or a validation error. Never report this as running.
   - Always include `handle`, `operation_id`, `turn_id`, and `next_action` (`wait_agents` with `handles` and `until`).
4. Add equivalent recovery to `send_message`. Steering already has idempotent receipts (`messaging.rs` ~186–192), so look the receipt up by key.
5. Never infer "running" from a transport timeout alone. The outcome must come from operation state.

### Tests (fault injection)
- The worker delays beyond the foreground budget. Expect `pending` with an operation_id, then exactly one result from `wait_agents`.
- The bridge drops the connection after dispatch but before reply. A retry with the same key must not create a new operation. Assert the turn count is unchanged.
- Failure before dispatch must return `not_dispatched`, not `pending`.
- Run each case for pane_id-only `run_agent` and for handle-based `followup_task`.
- After a timeout, a follow-up on the same handle keeps the provider session.

Existing test to extend: `bin/puppet_master_mcp/tests.rs` `wait_timeout_keeps_the_followup_handle` (~229–244).

---

## WP3: easy answer collection

### Current state
- `wait_agents` route is `agent_runs/routes.rs` ~454–508, which calls `operations::wait_for_operations`. With no `until`, it wakes on **any revision bump or a terminal state** (`operations.rs` ~1343–1348), so progress can look like completion. Wake reasons are `revision_changed`, `matched_state`, `terminal`, and `timeout`.
- It returns full snapshots every time. The cursor controls when the wait wakes, not what it returns, so old `result`s are re-sent.
- **Default mismatch:** the tool schema says `timeout_ms` defaults to 120000 (`tool_registry.rs` ~244), but the MCP binary uses 30 s plus 20 s grace when the field is omitted (`bin/puppet-master-mcp.rs` ~798–801). The route default is 120 s (`routes.rs` ~215).
- `until` accepts `waiting_input`, and `needs_input` is an alias (`operations/model.rs` ~10–11). Views emit `needs_input`.

### Changes
1. Fix the default mismatch: 120 s everywhere, from one constant.
2. Choose a completion-oriented default **without breaking progress consumers**. Recommended: when `until` is omitted, default to `["completed","failed","cancelled","waiting_input"]`, and add an explicit `until: ["progress"]` (or `wake_on_progress: true`) for the old behavior. First grep docs, tests, and the TS server for callers that rely on unfiltered progress wakes.
3. Every wait response includes `wake_reason` per handle. With the completion default, a progress revision can never be the reason.
4. Don't re-send old answers. Omit `result` when the turn and revision haven't changed since the caller's cursor, and include `result_unchanged: true`. Alternatively, return the result only for turns that finished after the cursor. Document the chosen behavior.
5. `needs_input` responses carry the prompt payload: `prompt_id`, the question, options, and worker identity. The caller should be able to call `answer_prompt` without reading the screen.
6. Automatic push of finals: check whether the host supports MCP notifications or progress for tool results. If not, **document** that `wait_agents` is the single completion path. Don't advertise push as implemented.

### Tests
- A progress revision during an active turn doesn't wake the default wait.
- A repeated wait with `next_cursor` doesn't return the old result body.
- needs_input wakes the default wait and includes an actionable prompt.
- Omitting `timeout_ms` gives a 120 s budget end to end. Test this with a short injected clock or config if possible.

---

## WP1: make the happy path obvious (docs and descriptions)

### Current state
- Initialize instructions are in `bin/puppet_master_mcp/stdio.rs` ~179–187 (tested in `tests.rs` ~68–74). The Node server (`packages/mcp-server/src/index.ts` ~378–385) sends **no** `instructions`.
- `run_agent` with a pane_id goes through `headless.rs` `bind_existing_worker` (~346–424). It adopts the pane, calls `take_over_pane(.., true)` (always grants control), and infers the backend from the pane. **This is currently unreachable for pane_id calls, because `authorize` denies them first (see WP0-B).** There is no regression test that direct adoption works without a prior `take_over`.
- `puppet-master-coordinator-guide.md` doesn't exist in the repo. Existing docs are in `docs/orchestrator/`: `README.md`, `quickstart.md`, `workflows.md`, `mcp-cheatsheet.md`, `mcp-gui.md`, `agent-shell-modes.md`, `opencode-native-worker.md`, and `token-efficiency.md`.

### Changes
1. Rewrite the initialize instructions, and the `run_agent`, `send_message`, `followup_task`, and `wait_agents` descriptions in `tool_registry.rs`, around one sequence:
   ```
   list_agents -> run_agent(pane_id, task) -> handle
   send_message(handle, correction)   # steer while active; receipt != result
   followup_task(handle, next_task)   # next turn, same conversation; no release/close between turns
   wait_agents(handles:[handle])      # when result is pending (completion default from WP3)
   ```
   State plainly:
   - A follow-up's wait is bounded and may return pending.
   - Closing the user's pane is not cleanup.
   - Pane control and the worker lease are separate. Keeping a worker for yourself is not the same as `transfer_agent`.
2. Mirror the same text in the Node server: add `instructions`, or make it fetch them from the bridge.
3. Put the same sequence at the top of `docs/orchestrator/quickstart.md`. Move takeover, backend selection, ownership handoff, and directory overrides to an "Advanced" section. Don't create a new guide file unless the user supplies `puppet-master-coordinator-guide.md`.
4. Add a regression test: `run_agent(pane_id)` on a fresh connection, with no prior `take_over`, adopts the worker, infers the backend, creates no new pane, and later calls work with the handle alone.

### Acceptance
From a fresh agent-mode connection, the coordinator finds the existing OpenCode worker and delegates by pane_id with no takeover and no backend or project_path guessing. It then steers, gets the answer, and follows up using only the handle. Zero new panes are created.

---

## WP4: reliable Windows interruption

### Current state
- `agent_runs/headless.rs` `terminate()` (~1138–1205) runs `taskkill /PID {pid} /T /F` with **stdout and stderr discarded** (~1151–1154). It only reports an error if taskkill fails **and** `try_wait` is still `None`. The benign exit race is mostly handled.
- After a failed stop, the op is set to `Failed` with stage `stop_failed` and `WORKER_STOP_FAILED` (~708–714, ~889–897). `WORKER_EXIT_FAILED` (~974–991) is a separate path: the child exits non-zero without a structured result. The third trial saw both on one turn. **Find how both happened**: possibly a stop request that wasn't from the deadline path, or a later exit handler overwriting the state.
- Native or pane cancel doesn't use taskkill. It uses `abort_session`, then `registry_kill_pane` or Ctrl+C (`bridge.rs` ~3156–3188), which produces `CANCEL_FAILED` or `INTERRUPT_FAILED` (`runtime.rs` ~357–376).

### Changes
1. Repro first: on Windows, interrupt an active headless Codex or Cursor run, plus a native OpenCode turn. Capture the final state and process tree, and check whether any descendants survive.
2. Capture taskkill's exit code and stderr into `OperationError.details`.
3. After taskkill fails, re-check whether the tree is alive (`try_wait`, plus a descendant check if one is cheap). Then pick one terminal state:
   - `cancelled` with `stopped: true`
   - `failed` / `WORKER_STOP_FAILED` with `stopped: false` or `"unknown"`
4. Make terminal state write-once. A later exit handler can add exit diagnostics but must not overwrite the stop outcome.
5. Use one interrupt response shape for headless and native: `stopped: true|false|unknown`, `code`, and `details`.

### Tests
- Unit tests with a fake process: taskkill fails while the process is still alive, taskkill fails because the process already exited, and an exit arrives after the stop is recorded (no overwrite).
- Manual Windows check: after an interrupt, `followup_task` on the same handle resumes the conversation.

---

## WP5: explicit recovery when the native session changes

### Current state
- Key rotation (`opencode/keys.rs` ~209–225, `opencode/quota.rs` ~110–129, with `restart_pane_on_rotate` defaulting to true) can call `restart_native_pane`. That calls `spawn_native_pane` with the same `pane_id` but a new PID and possibly a new session (`opencode/native.rs` ~222–246).
- When the session changes, `agent_runs/recovery.rs` `on_native_pane_replaced` (~12–105) does the following:
  - rebinds `provider_session_id`
  - expires unsettled steers (`messaging::expire_unsettled_steers`)
  - replays the running turn with `prompt_native_session`, or fails it with stage `session_replaced`
- `reattach_tui` keeps the session. `mod.rs` ~218–221 `pane_rebound_to` checks for ops still on the old session.
- Observed bug (second trial): the snapshot kept the old session and stayed running, and the new session had no messages. Suspects:
  - the `previous_session == new` comparison is skipped when the session is reused but the process changed
  - the hook doesn't run on some restart path
  - the replay prompt is lost

### Changes
1. Repro: start a long native turn, then trigger rotation or `restart_native_pane`. Record the binding, the steers, and the outcome.
2. Run `on_native_pane_replaced` (or a sibling) when the **process** changes, not only when the session id changes. Every restart path must call it.
3. Make recovery visible on the agent view: `recovery: { kind: "session_replaced"|"process_restarted", old_session, new_session, context: "replayed"|"reset" }`. Report an explicit reset whenever memory isn't carried over.
4. Expired steers return `expired`, with a reason the coordinator can see. Each pending turn must end up either replayed and finished, or failed with a concrete code.
5. Guard: ordinary reuse must never restart or recreate OpenCode. Add a test that PID and session stay the same across two follow-ups.

---

## WP6: contract consistency and concise responses

### Current state
- The source of truth is `tool_registry.rs` (`tools()` ~223+, `agent_run_schema()` ~174–198). It is served by the stdio binary (`stdio.rs` ~288–306) and by the bridge `GET /mcp/tools` (`bridge.rs` ~814, ~889).
- Duplicates:
  - Node `FALLBACK_AGENT_TOOLS` (`packages/mcp-server/src/index.ts` ~259–316), which strips `outputSchema`
  - the esbuild bundle `resources/mcp-stdio.bundle.cjs`
- **No test validates runtime responses against `outputSchema`.**
- Known drift:
  - Agent status: the schema has no `queued`, but descriptions say "queued or running". The runtime maps Queued, Starting, and Running to `running` (`runtime.rs` ~191–195).
  - `waiting_input` (in `until` and the init text) vs `needs_input` (in views).
  - The `take_over` output schema lacks `handle` and `control` (`bridge.rs` ~3141). Its input already accepts `handle` or `pane_id`; recheck the earlier rejection report.
  - The `answer_prompt` schema requires only `prompt_id` and `choice`, but the runtime needs `handle` or `pane_id` (`routes.rs` ~92–98). The TS fallback omits `pane_id`.
  - `verified: true` is set by callers with no model check (`operations.rs` ~1121–1191, `headless.rs` ~953, `native.rs` ~183).
  - `read_only_supported` is advertised only (`capabilities.rs` ~54–56). It isn't re-checked in `bridge::dispatch_existing_operation`.
  - Prompt wrapping happens in `agent_runs/persist.rs` `render_turn_prompt` (~211–257), `context_packet` (~424–441), and `headless.rs` `wrap_acceptance_criteria` (~317–323).

### Changes
- **6a (start early)**: add a contract test. Run each agent tool through the bridge on fixtures and validate the JSON against its `outputSchema` (use the `jsonschema` crate if it's already a dependency; otherwise do a minimal enum and required-field check). Fix the enum gaps it finds.
- 6b: pick one status vocabulary. Either add `queued` to the view or drop it from descriptions. Make `needs_input` canonical everywhere and keep `waiting_input` as an input alias.
- 6c: make the `answer_prompt` schema require `handle` or `pane_id` (anyOf). Fix the TS fallback, or remove it if the bridge catalog is always reachable. Ideally, the Node server should always use `/mcp/tools`.
- 6d: rename or split `verified` into `result_verified`, and `model_verified` that is true only when `resolved_model` matches the request. Unknown model stays `null`.
- 6e: return `read_only_supported` and the reason on `list_agents` per worker, before dispatch. Reject `read_only` at dispatch for panes that can't enforce it, with `READ_ONLY_UNSUPPORTED`, and don't silently downgrade to an instruction.
- 6f: never add evidence or output sections to the prompt unless the caller asked for them. Pass the caller's format and word budget through verbatim. Add a test that a plain task renders without extra sections under the default policy.
- 6g: concise defaults. A follow-up response contains the current identity, turn, state, result, and any actionable error or input, and no prior turn results or capability notes. Add a test asserting that a one-line follow-up response excludes the previous result.
- 6h: steering receipts include `turn_id`, and document each disposition (`accepted`, `queued`, `deferred`, `processed`, `expired`, `rejected`, `unsupported`). `queued` and `deferred` must not imply the current turn consumed the message.
- **Catalog refresh:** source changes need the stdio binary rebuilt and `mcp-stdio.bundle.cjs` rebundled (`scripts/bundle-mcp.mjs`), and the host (Cursor) must reload MCP. Say so in the final report.

---

## Final validation (do this last; it can't be replaced by marker tests)

1. Rebuild and rebundle, then reload the MCP host. Confirm the host-loaded `tools/list` matches `GET /mcp/tools`.
2. Reuse the user's **existing** OpenCode pane: `run_agent(pane_id)` with a useful read-only task (for example, reviewing a doc in `docs/orchestrator/`). Then send a substantive correction with `send_message`, `wait_agents` for the answer, and do one or two `followup_task` discussion turns. Leave the pane open.
3. Report PID and session continuity only if you actually checked them with `inspect_agent` and the process info.
4. Run the WP2, WP4, and WP5 failure-path tests and list pass or fail for each.

## Report template (deliver at the end)

```
Changed: <bullets per WP>
Acceptance passed: <criterion -> evidence (test name / manual run)>
Not reproduced / unverified: <failure paths still unverified>
Catalog refresh required: yes/no (+ steps)
Continuity checked: pid=<same?> session=<same?> (or "not checked")
```

## Status (Oct 2, 2026 implementation pass)

```
Changed:
- WP0: `normalize_context_policy_fields` in agent_runs/runtime.rs; routes `parse()` merges context_mode/context_policy before deserialize; removed conflicting serde aliases on AgentRunRequest and SendAgentRequest; run_agent `context_policy` primary in tool_registry schemas; mcp_sessions authorize allows run_agent(pane_id) when pane is not yet in session ledger (bind_existing_worker adopts); clearer coordinator vs adoption errors; quickstart note on unstable pane ids after restart.
- WP2–WP6: No additional code changes in this pass — the working tree already contained timeout recovery (`/operations/by-key`, structured `outcome`), followup idempotency, wait_agents completion defaults / result_unchanged, docs and Node instructions fetch, Windows taskkill stderr in terminate(), session_replaced recovery tests, and tool_registry contract field tests. Verified via `cargo test` in packages/app/src-tauri (all passing after WP0).

Acceptance passed:
- WP0-A: run_agent_request_accepts_equal_context_fields, run_agent_request_rejects_conflicting_context_fields, run_agent_request_maps_context_mode_alias
- WP0-B: mcp_sessions::tests::run_agent_with_unadopted_pane_id_passes_authorize, run_agent_with_pane_owned_by_another_session_is_denied
- WP2: puppet_master_mcp::stdio::tests::wait_timeout_keeps_the_followup_handle, operation_timeout_outcome_classifies_dispatch_and_terminal_states, agent_runs::tests::followup_idempotency_key_reuses_the_same_operation
- WP3: operations/tests and agent_runs tests for wait_agents / result_unchanged (existing suite)
- WP1: puppet_master_mcp::stdio::tests::initialize_returns_server_info; docs/orchestrator/quickstart.md happy path; tool_registry agent-mode instructions
- WP4: headless terminate captures taskkill exit_code/stderr in context (code review; no new Windows live interrupt in this pass)
- WP5: agent_runs::tests session_replaced / recovery paths (existing)
- WP6: tool_registry::operation_tools_expose_contract_fields_and_structured_output_schemas; MCP tools list outputSchema presence tests

Not reproduced / unverified:
- Final validation live collaboration (user app): run_agent → send_message → wait_agents → followup on real OpenCode pane
- WP2 fault-injection e2e with delayed worker beyond foreground budget (no dedicated test name found; logic covered by unit tests)
- WP4 manual Windows interrupt + process-tree check
- WP5 live key-rotation / restart_native_pane repro
- WP6a full runtime JSON validation against every outputSchema via jsonschema crate (not present; enum/required checks only)
- WP1 full regression: run_agent(pane_id) end-to-end through bridge with live registry pane (authorize + bind unit coverage only)

Catalog refresh required: yes
- Rebuild: `cargo build --release -p puppet_master_app --bin puppet-master-mcp` from packages/app/src-tauri (or project build script)
- Rebundle: `node scripts/bundle-mcp.mjs` from repo root
- Reload MCP host in Cursor (toggle user-puppet-master server) so tools/list matches GET /mcp/tools

Continuity checked: not checked
```

## Integration status (WP7 + WP8 + WP9, Oct 2 2026)

Parallel WP7/WP8/WP9 changes were merged in-tree without revert. Verification used `CARGO_TARGET_DIR=target-integrate` because other `cargo` processes held the default `target/debug` test binary (LNK1104 risk); no user processes were killed.

### Test counts

| Suite | Result |
|-------|--------|
| `cargo test` (lib) in `packages/app/src-tauri` | **318 passed**, 0 failed |
| `cargo test` (`puppet-master-mcp` bin) | **25 passed**, 0 failed |
| `cargo test` (`puppet_master_app` main bin) | 0 tests |
| `cargo clippy --all-targets` | Finished (warnings only; no errors) |
| `npm run typecheck` (all workspaces) | pass |
| `npm run test` (`@puppet-master/app` vitest) | **188 passed** |
| `npx vitest run` (`packages/shared`) | **13 passed** |
| `@puppet-master/cli` / `@puppet-master/mcp` | no `test` script (typecheck only) |

WP7’s earlier **54× `INVALID_PROJECT_PATH`** failures do not reproduce on the integrated tree (`prepare_project_path` + absolute paths in `create_operation` are consistent with `operations::store::root`).

### Fixes made in this integration pass

- **Docs only:** `tool_registry::mcp_instructions`, `docs/orchestrator/quickstart.md`, and `docs/plans/wp8-watcher-contract.md` — `watch_command` must run as a foreground CLI via the agent harness shell **background** mode, not detached `Start-Process` (harness would not wake).
- **No Rust/TS logic changes** required; overlapping edits in `bridge.rs`, `mcp_sessions.rs`, `pty/registry.rs`, `agent_runs/runtime.rs`, and `tool_registry.rs` already compose.

### Cross-WP interactions

| Question | Resolution |
|----------|------------|
| **(a) WP7 reclaim vs WP9 pane kept / `PANE_GONE`** | Compatible. Adopted panes are not auto-disposed (`pane_close`, `pane_created: false`, `keep_pane` forced on adopt). `take_over` with `handle` + `grant=true` works when the pane is gone (`handoff_route` skips `take_over_pane`, still calls `take_over_agent`). Followup on a missing bound pane returns **`PANE_GONE`** instead of respawning — reclaim the lease, do not expect a silent new worker. |
| **(b) WP8 read-only watch vs WP7 access tiers** | Compatible. `readonly_operation_route` allows GET/wait on operations without session ownership; `check_run_read_access` / `check_run_wait_access` gate agent `inspect` / `wait_agents` without mutating leases. `visible_runs` `owned` uses `owner_session_id` or `check_run_access` (control), not read-only visibility. |
| **(c) Chunked `wait_agents` + `watch_command` / `result_unchanged`** | Compatible. MCP stdio chunks POST `/agents/wait` (4s) and returns the bridge JSON as-is. Per-agent views come from `agent_view_after_wait` (`result_unchanged` when revision unchanged); `watch_command` is set on `AgentRunView::from` for run/followup responses and WAIT_TIMEOUT recovery JSON. |
| **(d) Single kill path + `PaneKilled.reason`** | Mostly unified: automatic/explicit run dispose uses `pane_close` → `registry_kill_pane_with_reason` → `shutdown_pane` (Windows `taskkill /T` then `child.kill()`). **Exceptions (intentional):** UI/`DELETE` pane, `kill_pane_cmd`, shell-exec spawn rollback (`bridge.rs` ~989), and `kill_all` still call `registry_kill_pane` / `shutdown_pane` directly; only policy-driven dispose always sets a reason. |
| **(e) `tool_registry` vs runtime** | `tool_registry::tests::*` and `operations_contract_tests::*` pass in lib suite; schema defaults removed for `context_policy` / `context_mode` per WP8. |
| **(f) Watch background docs for harnesses** | Documented (see fixes above). |

### Still unverified live (manual)

1. **MCP reload:** Toggle off/on **user-puppet-master** in Cursor MCP settings (or restart Cursor) so stdio loads the rebuilt binary/bundle.
2. **Reconnect:** New stdio session → `session_identity` → `attach_agents(attach_token)` or `take_over(handle, grant=true)` → `wait_agents` with `progressToken` for ~3 min (chunked progress).
3. **Adopted OpenCode pane:** `run_agent { pane_id }` → wait for terminal → confirm pane still listed (`list_agents` / UI); no replacement worker.
4. **`PANE_GONE`:** Kill adopted pane externally → `followup_task` → expect recoverable `PANE_GONE`, not a new spawn.
5. **`npx puppet-master watch <handle>`** against a running bridge with a long-running turn (CLI long-poll).
6. **Windows serve tree:** After spawned-pane auto-dispose, confirm opencode serve PIDs are gone (`taskkill /T` path).

### Binary / bundle

| Step | Status |
|------|--------|
| `cargo build --release -p puppet_master_app --bin puppet-master-mcp` | **Succeeded** (`packages/app/src-tauri/target/release/puppet-master-mcp.exe`) |
| `node scripts/bundle-mcp.mjs` | **Partial:** wrote `packages/app/src-tauri/resources/bin/puppet-master-mcp.exe` and `resources/mcp-stdio.bundle.cjs`; **could not overwrite** `packages/mcp-server/dist/puppet-master-mcp.exe` (**EPERM** — likely MCP host or another process has the file open). Reload host after closing the lock, or copy from `resources/bin/` if needed. |
