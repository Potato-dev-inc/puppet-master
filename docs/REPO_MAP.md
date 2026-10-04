# Puppet Master repository map

Orientation for coding agents. For product concepts and how to keep developing, start at
[`docs/wiki/`](wiki/README.md). Read this file before exploring the tree. Paths are relative to the repo root;
functions are cited by name (line numbers rot). When something here disagrees with the code, the
code wins: fix this file.

Existing docs this file links to rather than repeats:

| Doc | Covers |
|---|---|
| `docs/wiki/` | product + development handbook (README seed, architecture, how to keep building) |
| `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md` | product overview, contribution flow |
| `MCP_HOSTS.md` | registering the MCP server with Cursor/Claude/Codex hosts, tool reference |
| `ROUTING.md`, `ROADMAP.md` | API-vs-CLI orchestrator backends, planned work |
| `docs/orchestrator/*.md` | playbooks for orchestrators (quickstart, cheatsheet, workflows, `opencode-native-worker.md`, `benchmark.md`) |
| `docs/plans/wp7-ownership-lifecycle.md`, `wp8-watcher-contract.md`, `wp9-pane-close.md`, `agent-parity-plan.md` | design notes behind ownership/leases, the watch contract, pane close policy |
| `packages/app/src-tauri/src/tui/README.md` | TUI profile folder layout, add-a-TUI checklist, symptom-to-file table |
| `packages/mcp-server/README.md` | MCP package/launcher |

---

## 1. Architecture overview

Puppet Master is a **Tauri 2 desktop app** (Rust backend in `packages/app/src-tauri`, React/Vite UI in
`packages/app/src`) that hosts real PTY panes (`portable-pty` + `vt100`) for agent CLIs (OpenCode,
Cursor, Claude Code, Codex, plain shells) and exposes them to orchestrating agents. The Rust
library crate (`puppet_master_app_lib`) embeds a **loopback HTTP bridge** (`bridge.rs`, default
`127.0.0.1:17321`, scans upward from `PORT_MIN`; the chosen port is written to a port file in
the app data dir, `app_paths.rs`) that shares the same `PaneRegistry` as the GUI. External
orchestrators reach it through the **stdio MCP shim** `puppet-master-mcp`
(`src/bin/puppet-master-mcp.rs`, compiled from the same crate, shipped as
`packages/mcp-server/dist/puppet-master-mcp.exe` and staged to `resources/bin/`). The shim owns no
state: it advertises the tool catalog from `tool_registry.rs`, maps every tool call to an HTTP
request (method/path come from the `ToolDefinition`), adds an `X-Puppet-Master-Session` header and
relays the JSON reply. A small **JS CLI** (`packages/cli`, `puppet-master watch ...`) long-polls
the bridge so an orchestrator's shell can block on a background run and wake when it finishes.
Runs are persisted as **operations** (`operations.rs` + `operations/`): revisioned JSON snapshots
under `<project>/.puppet-master/operations/` that survive restarts. Backends are
`opencode_native` (OpenCode HTTP API + attached TUI, `opencode/`), and screen-scraped/headless CLIs
(`cursor_agent`, `claude`, `codex`, shells), all isolated behind the `TuiProfile` trait in `tui/`.

### How a `run_agent` call flows

1. **MCP tool** `run_agent` (declared in `tool_registry.rs::tools`, POST `/agents/run`). The shim
   (`bin/puppet-master-mcp.rs`, `call_tool` match arm `"run_agent"`) mints an `idempotency_key` when
   missing, injects the session id, and bounds the HTTP wait (`DEFAULT_AGENT_WAIT_MS`).
2. **Bridge**: `bridge.rs::handle_connection` -> `route`. `route` first runs
   `mobile_pairing::authorize_bridge_request`, then `mcp_sessions::authorize` (session/mode/ownership
   gate), then delegates `/agents/*` to `agent_runs::handle_request` (`agent_runs/routes.rs`).
3. **Runtime**: `routes.rs` -> `runtime::run_agent` which is re-exported from
   `agent_runs/headless.rs::run_agent`. It validates (`INVALID_TASK`, `INVALID_TIMEOUT`,
   `READ_ONLY_UNSUPPORTED`, `AGENT_NOT_DISPATCHABLE` via `tui::profile_for(..).features()`), replays an
   identical retry by `idempotency_key` **before** worker selection, then `persist::decide_worker`
   either adopts an existing worker (-> `runtime::send_agent`, a follow-up turn) or creates a new
   handle. `persist::stable_handle(project, key)` derives the handle from the key so retries match.
4. **Operation created**: `operations::create_operation` persists the snapshot (status `queued`)
   *before* any dispatch, enforcing idempotency (`IDEMPOTENCY_KEY_CONFLICT`) and one turn index per
   handle (`AGENT_TURN_EXISTS`). Locks (`acquire_operation_locks` in `bridge.rs`) are taken here.
5. **Pane spawn / delivery**:
   - Headless (`headless=true`, Claude/Codex/Cursor): `execute_headless` thread spawns the CLI built
     by `tui::headless_command` (profile `headless_command`) inside
     `operations::run_operation_dispatch`; stdout is read by reader threads
     (`spawn_reader`) into `agent_runs/transcript.rs`; the profile's `extract_result` /
     `extract_provider_session` parse the output.
   - Visible TUI pane: the bridge spawns/reuses a pane (`bridge.rs::spawn_pane_with_timeout`,
     `register_spawned_pane`, `pane_ready_for_dispatch`), waits for readiness, then types the prompt
     (`agent_runs/delivery.rs::dispatch_prompt`; for profiles with `verifies_prompt_delivery`,
     `deliver_prompt` retypes/verifies via `DeliveryIo`/`PaneIo`).
   - `opencode_native`: `opencode/native.rs` spawns `opencode serve`, `opencode/client.rs` sends via
     REST, `opencode/delivery.rs` confirms the user message landed (`PROMPT_NOT_DELIVERED`).
6. **Supervisor / completion**: `headless.rs::supervise_existing` loops per run. Visible TUI turns go
   through `agent_runs/completion.rs::observe_tui_turn` (deterministic decision from pane status +
   screen), native runs through `agent_runs/native.rs` + `opencode/`. On completion the supervisor
   extracts the reply (`clean_extracted_result`, `widen_reply` for scrollback), evaluates `checks`
   (`agent_runs/checks.rs`), and calls `operations::mark_operation_finished` with a `ResultCapture`
   (`Authoritative` | `Inferred` | `Missing`). Failures use `mark_operation_state(Failed, error)`.
   `pane_close.rs::maybe_dispose_after_terminal` decides whether the run-spawned pane is closed.
7. **Operation snapshot published**: `bridge.rs::publish_operation` pushes SSE + bumps waiters
   (`pane_wait_notify::bump_waiters`, `operations/store.rs` change condvar).
8. **Return**: `wait_for_run` (in `headless.rs`) blocks up to `wait_ms` then builds an `AgentRunView`
   (`runtime.rs`): `status`, `result`, `result_capture`, `receipt` (`RunReceipt`, `outcome`),
   `warnings`, `prompt`/`required_action` if blocked, and a `watch_command` (`watch_command.rs`)
   when still running.
9. **Wait / watch**: `wait_agents` -> `routes.rs::wait_agents` (POST `/agents/wait`);
   `wait_for_operation` -> `operations::wait_for_operation(s)`. The JS CLI `watch` calls the bridge
   operation wait route repeatedly (`packages/cli/src/watch.ts`).

---

## 2. Top-level layout

```
puppet-master/
  package.json              npm workspaces root (packages/*); scripts: build, test, typecheck, test:mcp-*
  README.md CHANGELOG.md CONTRIBUTING.md MCP_HOSTS.md ROUTING.md ROADMAP.md rust-migration-plan.md
  commands.md puppet-master-feedback.md   scratch/feedback notes (not authoritative)
  puppet-master.dev.json    dev tunnel info written by scripts (localUrl, tunnelUrl); machine-local
  tsconfig.base.json
  docs/
    orchestrator/           orchestrator-facing guides (see table above)
    plans/                  design notes (wp7, wp8, wp9, agent-parity)
    design/                 static HTML demo
    mobile-terminal-bugs-and-fixes.md
    REPO_MAP.md             this file
    wiki/                   developer/agent handbook (overview, architecture, developing, extending)
  packages/
    app/                    Tauri app
      src/                  React UI (App.tsx, PwaApp.tsx mobile PWA, TerminalApp.tsx worker window,
                            components/, hooks/ (usePaneRegistry, useBridge*), lib/ (bridge.ts,
                            tauri.ts, mcp-tools.ts, pane-state.ts, ...), terminal/)
      src-tauri/
        Cargo.toml          crate puppet_master_app, lib puppet_master_app_lib, bin puppet-master-mcp
        build.rs            stamps PM_BUILD_ID, then tauri_build
        tauri.conf.json     bundle resources, beforeDevCommand (scripts/vite-dev.mjs)
        capabilities/ icons/ gen/ (gen is gitignored)
        resources/          bin/ (cloudflared, staged puppet-master-mcp), mcp-stdio.bundle.cjs,
                            puppet-master-cli.bundle.mjs (CLI bundle), pwa-dist/ (staged PWA)
        src/                Rust sources (section 3)
    cli/                    `puppet-master` CLI (src/index.ts: gui, mcp, worker, watch, version;
                            watch.ts + watch-args.ts = the watch command; launch-worker.ts)
    mcp-server/             MCP packaging: src/index.ts (legacy TS MCP entry: startup mode + handlers;
                            bridge-client / catalog / tool-router / registry-call / timeouts / errors /
                            types / util alongside), src/launcher.ts (picks the Rust binary),
                            dist/ holds the built exe
    shared/                 TS types/zod shared by app + cli + mcp (protocol.ts, agents.ts,
                            bridge-port.ts, agent-contexts.ts, mobile-pairing.ts, schemas.ts)
  scripts/
    build-rust-mcp.mjs      cargo build of puppet-master-mcp -> packages/mcp-server/dist
    bundle-mcp.mjs          stages MCP exe + esbuilds CLI bundle and mcp-stdio bundle into resources/
    bundle-cloudflared.mjs, stage-pwa-dist.mjs, ensure-tauri-resources.mjs, ensure-workspace-built.mjs
    with-cargo-path.mjs / cargo-with-path.mjs   run cargo/tauri with ~/.cargo/bin on PATH
    run-tauri-dev.ps1, vite-dev.mjs, dev-tunnel.mjs   dev server helpers
    test-mcp-operations.mjs mock-bridge stdio smoke test; test-mcp-package.mjs packaging test;
    test-bridge.ps1 test-e2e.ps1 test-mcp.ps1   PowerShell live checks
    pm-regression.mjs       LIVE regression + benchmark suite against the running app (entry point);
                            modules in pm-regression/: context (args, rpc), scenarios, bench*.mjs
    spawn-close-agent.py    live pane spawn/close probe
    project-indexer.py, librarian-prompt.md   librarian project IR
    publish-github-release-assets.mjs, count-loc.*   release/LOC helpers
    launch-mcp-gui.ps1, puppet-master-mcp-gui.py, qt_mcp/   old/aux GUI launchers
                            (qt_mcp/: gui.py MainWindow; discovery, operations, dialogs, style helpers)
```

Gitignored build outputs: `dist/`, `target/`, `gen/`, `node_modules/`. `.claude/worktrees/` holds
agent worktrees (ignore).

---

## 3. Rust module index (`packages/app/src-tauri/src`)

`lib.rs` declares every module, registers Tauri commands (`commands::*`) and starts the bridge.
`main.rs` just calls `run()`. Modules marked `pub` are used by the MCP binary.

### 3.1 Bridge and routes

| File | Purpose / key items |
|---|---|
| `bridge.rs` (~4k lines) | Embedded HTTP server. `start_embedded_bridge`, `handle_connection`, `route` (the route table: `/health`, `/mcp/{tools,session,mode,...}`, `/panes/...`, `/operations/...`, `/agents/...`, `/tasks`, `/locks`, `/opencode/...`, `/events`, `/workspace/state`, `/shell/exec`, ...). SSE: `handle_sse`, `push_*_sse`, `publish_operation`. Operation plumbing: `delegate_operation_http`, `cancel_operation_http`, `dispatch_existing_operation`, `acquire_operation_locks`, `release_operation_locks`, `recover_interrupted_operations_on_startup`. Errors: `operation_http_error` maps `OperationError` -> HTTP status + body. `Health` struct = `/health` (`version`, `catalog_version`, `build_id`, `tool_count`). `mcp_status.rs` probes it for the UI. |
| `tool_registry.rs` | THE MCP tool catalog. `ToolDefinition` (name, description, method, path, input/output schema, `ToolSafety`, visibility), `tools()`, `tools_for_mode(McpMode)`, `tool_visible_in_mode` (agent/shell/both lists), `mcp_instructions()` (server instruction text), `catalog_version()` (hash of names/descriptions/method/path/schemas; `<count>-<hash>`), `external_mcp_tool_count()`. |
| `bin/puppet-master-mcp.rs` | stdio JSON-RPC shim: initialize/tools/list/tools/call, bridge discovery (port file), session id (`session_id_from`, `PUPPET_MASTER_CLIENT_ID`), `call_tool` arms with shim-side shaping (`run_agent`, `wait_agents`, ...), `WAIT_TIMEOUT` handling, `SCHEMA_DRIFT` check (shim catalog vs `/health.catalog_version`), `BRIDGE_UNAVAILABLE`. Helpers in `bin/puppet_master_mcp/stdio.rs`; tests in `bin/puppet_master_mcp/tests.rs`. |
| `mcp_sessions.rs` | Per-connection state and ownership: `Session`, `McpMode`, `PaneControl` leases, coordinators + `attach_token`. `authorize` (route-level gate), `check_run_access/read_access/wait_access`, `register_owned_pane/agent`, `take_over_*`, `release_*`, `transfer_agent`, `attach_coordinator`, `resolve_pane_id`, `enrich_handle_error*` (turns a pane id used as handle into a helpful `AGENT_NOT_FOUND`). Plan: `docs/plans/wp7-ownership-lifecycle.md`. |
| `mcp_hints.rs` | Next-step hints appended to mutating tool responses. |
| `mcp_install.rs`, `mcp_runtime.rs` | Register/verify the MCP server in CLI hosts' configs; resolve how hosts launch the stdio server. |
| `mobile_pairing.rs`, `mobile_tunnel.rs`, `pwa_server.rs` | Ed25519 pairing + bearer tokens for the phone PWA; cloudflared tunnel; loopback PWA server proxying `/bridge/*`. `authorize_bridge_request` is the first gate in `bridge::route`. |
| `commands.rs` | Tauri `invoke` commands (thin wrappers over `PaneRegistry`); `AppState`. |
| `settings_store.rs`, `app_paths.rs`, `app_lifecycle.rs`, `platform.rs`, `shell_env.rs`, `env_probe.rs`, `project_path.rs`, `startup_timing.rs`, `worker_launch.rs` | Settings mirror; app data dir (`%APPDATA%\com.puppetmaster.app`, port file); install/uninstall metadata; OS helpers; login-shell PATH for GUI launches; tool presence probe for `bridge_health.env`; writable project dir resolution; run-start stage timing; standalone worker window launch (`PUPPET_MASTER_WORKER*` env). |

### 3.2 `agent_runs/` (the run engine)

`agent_runs.rs` is the module root and public surface (`handle_request`, `AgentRunRequest`,
`AgentRunView`, `supervise_existing`, `stable_prompt_id`, `resolve_worker_name`, ...).

| File | Purpose / key items |
|---|---|
| `routes.rs` | HTTP dispatch for `/agents/*`: `run`, `quick`, `wait`, `send`, `followup`, `answer`, `cancel`, `close`, `steer-receipt`, `GET /agents`, `GET /agents/{handle}`, `.../transcript`. `wait_agents`, `list_agents` (+`AgentListFilter`), `visible_runs`, `latest_handle_for_pane`. |
| `runtime.rs` | Request/response types: `AgentRunRequest`, `AgentRunView`, `RunReceipt` (+`build_receipt`, `receipt_outcome`), `RunWarning` (+`run_warnings`), `SteerView`; `send_agent`/follow-up turns, `cancel`, `answer_prompt` entry, context-policy normalisation, `RESULT_TRUNCATED` const. |
| `headless.rs` (largest) | `run_agent` (entry), `bind_existing_worker`, `execute_headless` (spawn CLI, readers, timeout), `wait_for_run`, `supervise_existing` (the per-turn supervisor loop for visible/native panes), `auto_approve_allowed_command`, `record_completion_model`, workspace checks (`WORKSPACE_MISMATCH`). Also home of many failure codes. |
| `completion.rs` | `TuiTurnWatch`, `observe_tui_turn` -> `TuiTurnDecision` (Done/Running/AmbiguousIdle/RetrySubmit/...), echo handling (`task_echo_on_screen`), `widen_reply` (longer-than-screen replies from scrollback), `clean_extracted_result`, `is_tui_chrome_line`, `completion_model`. |
| `delivery.rs` | Verified prompt delivery: `DeliveryIo` trait, `Delivery`, `deliver_prompt`, `prompt_visible_in_input`, `deliver_prompt_traced` (trace lines go to the operation transcript `delivery` stream), `turn_started` (busy only counts if the screen changed after Enter), `needs_redelivery`, `PaneIo`, `dispatch_prompt` (appends the `complete_task` instruction for workers with MCP tools). |
| `checks.rs` | Acceptance checks evaluated on the real filesystem: `Check` enum (file/reply predicates, `command_exits`), `validate`, `evaluate`, `evaluate_with_commands`, `failure_summary`. `command_exits` runs an argv with timeout, never a shell string. |
| `messaging.rs` | Steering: `DeliveryMode`, `Disposition` (applied/next_action), `MessageReceipt`, `steer_target`, `decide_delivery`, `send_message`, `interrupt_action`, `pending_texts`. A steer receipt is NOT a result. |
| `persist.rs` | Worker-handle adoption and continuity: `worker_key`, `stable_handle` (key -> deterministic uuid), `decide_worker` (`WorkerBind::{Create,Adopt}`), `find_worker`, `resolve_worker_name`, `resolve_model_choice` (`ModelChoice`), `resolve_continuity`, `followup_policy`, `FollowupOpts`, `TurnPrompt`. |
| `answer_guard.rs` | Pre-keystroke validation for `answer_prompt` on a handle: `resolve_target` (project/ownership/ambiguity -> `PROJECT_MISMATCH`/`AMBIGUOUS_HANDLE`/`AUTHORIZATION_DENIED`), replay ledger (`replay_receipt`, `record_applied`, `REPLAY_WINDOW_MS`), `finish`. |
| `approval_policy.rs` | Opt-in `allowed_commands` for visible Cursor panes: `validate`, `approved_command`, `approved_program`, `decide`. Allow-once only, never allow-always. |
| `pane_close.rs` | Central pane-disposal policy: `spawned_by_run`, `maybe_dispose_after_terminal`, `maybe_dispose_on_dispatch_failure`, `dispose_explicit_close`, `stop_worker_control`, `PANE_CLOSED_AFTER_RUN` errors. Plan: `docs/plans/wp9-pane-close.md`. |
| `capabilities.rs` | Honest per-backend `BackendCapabilities` + `NOTE_*` strings, `read_only_supported[_for]`. |
| `native.rs` | OpenCode native supervision: `stable_prompt_id`, `server_unreachable_error` (`OPENCODE_SERVER_UNREACHABLE`), `handle_native_read_failure`, `supervise_native_observation`. |
| `recovery.rs` | `on_native_pane_replaced` rebinds a worker when its native process is replaced under the same pane id (`SESSION_REPLACED`). |
| `quick.rs` | `quick_ask`: one-shot headless read-only fresh-context question; `select_backend`, `build_run_request`, `project`. |
| `transcript.rs` | Bounded per-operation transcript files: `append`, `read`, `read_turns`, `TranscriptPage`. |
| tests | `tests.rs` (main, ~2.8k lines), `cursor_capture_tests.rs`, `cursor_delivery_tests.rs`, `delivery_contract_tests.rs`; module-level `#[cfg(test)]` in several files. |

### 3.3 `operations.rs` + `operations/`

| File | Purpose / key items |
|---|---|
| `operations/model.rs` | Types: `OperationStatus` (queued, running, completed, failed, cancelled, ...; `terminal()`), `StateSource`, `DelegateWorkRequest`, `OperationSnapshot`, `OperationError { code, message, recoverable, context }` (+`OperationError::new`), `OperationWarning`, `ContextPolicy`, `ContextContinuity`, `WorkerPersist`, `ResultCapture`, `AcceptanceStatus`, wait result/cursor types. |
| `operations/store.rs` | Atomic JSON persistence under `<project>/.puppet-master/operations/` (`atomic_json`, `replace_file`), project/task indexes (`operation-project-index.json`, `operation-task-index.json`), change condvar for waiters, `DELEGATE_LOCK`/`UPDATE_LOCK`. Error `INVALID_OPERATION_ID`, `OPERATION_NOT_FOUND`. |
| `operations.rs` | State machine and queries: `create_operation`, `run_operation_dispatch`, `dispatch_input_if_active`, `mark_operation_state`, `mark_operation_finished` (the only place evidence + `ResultCapture` are validated), `mark_operation_completed`, `add_warning`, cancellation (`CANCELLATION_*`), `get_operation`, `wait_for_operation(s)`, `find_by_idempotency_key`, `resolve_agent_run`, `agent_runs_by_project`, `turns_for_handle`, `completion_wait_until()` (default wait set). |
| tests | `operations/tests.rs`, `operations_contract_tests.rs` (cfg(test), declared in `lib.rs`). |

### 3.4 `tui/` (per-agent profiles)

See `tui/README.md` for the full layout; summary: `mod.rs` = `TuiProfile` trait (~30 methods with
generic defaults), `Features`/`Kind`/`RunOptionSupport`, `LaunchSpec`/`HeadlessLaunch`, registry
`PROFILES`, free-function dispatchers (`profile_for`, `headless_launch`, `is_chrome_line`,
`tidy_reply`, `strip_dialogs`, `read_only_backends`, `program_on_path`). `generic.rs` = default
prompt handling + fallback profile. `conformance.rs` = rules every registered profile must pass.
Per agent: `cursor/` (`launch.rs`, `headless.rs`, `screen.rs`, `chrome.rs`, `approval.rs`, `fixtures/`),
`claude/` (`mod.rs`, `headless.rs`), `codex/` (`mod.rs`, `headless.rs`), `opencode/` (`mod.rs`,
`screen.rs`, `read_only.rs`; covers `opencode_native` TUI-facing part), `shell/` (`mod.rs`, `prompt.rs`).

### 3.5 Panes, prompts, waiting

| File | Purpose / key items |
|---|---|
| `pty/registry.rs` | `PaneRegistry`: spawn/kill/resize/write, reader + idle threads, per-pane `PaneClassifier`, `registry_read_full_text`, Windows PowerShell hosting for `cursor-agent.ps1`, `is_pty_control_input`. `pty/agents.rs` = `AgentType` enum + per-OS launch commands; `pty/status.rs` = `PaneStatus`, `classify_pane_state`, `PaneClassifier`; `pty/keys.rs` named keys (`press_key`); `pty/ansi.rs`, `pty/scrollback.rs`, `pty/proc_tree.rs`; `pty/mod.rs` re-exports. |
| `pane_prompt.rs` | Structured approval prompt blocking a pane: `detect`, `detect_in_screen`, `dispatch_blocker` (`PANE_BLOCKED_BY_PROMPT`), `prompt_id_for` (the id handed to callers; command-based), `required_action[_from_prompt]`, `answer` (`STALE_PROMPT`, `PROMPT_REPLY_FAILED`, `INVALID_PROMPT_CHOICE`), `reply_key`. |
| `pane_wait.rs` | Long-poll `wait_for_panes` / `wait_for_model` / `wait_for_task` / `wait_for_worker`: `WaitUntil`, `WaitForPanesRequest/Result`, `check_pane`, `worker_settled_reason`, `agent_hint_for_reason`. `pane_wait_notify.rs`: `bump_waiters`, `wait_for_change[_ms]`. |
| `pane_windows.rs` | Destroys detached pane windows when their pane leaves the registry (`orphaned_labels`, `schedule_sweep`). |
| `shell_exec.rs` (+`shell_exec/command.rs`, `tests.rs`) | `shell_exec`: run a command in an explicitly selected shell pane (`SHELL_PANE_REQUIRED`, `COMMAND_TIMEOUT`, `PANE_NOT_READY`). |
| `agent_adapters/{claude,codex,opencode,bash}.rs` | Older per-agent `signals()` priors for the pane-state classifier (separate from `tui/`). |
| `projections.rs`, `event_log.rs`, `events.rs`, `actors.rs` | Event stream (`SystemEvent`), append-only event log per project, read models (`WorkspaceAttention`, `GET /workspace/state`), actor/role bookkeeping. |
| `session_context.rs`, `context_pack.rs`, `agent_contexts.rs`, `project_ir.rs` | Session context store, context packs, agent context profiles/labels (`AgentContextProfile`), librarian project IR reader. |

### 3.6 `opencode/` (native HTTP backend, not screen scraping)

`client.rs` REST client (v1 API), `native.rs` spawn `opencode serve` + session + attach TUI,
`delivery.rs` delivery ack (`PROMPT_NOT_DELIVERED`), `messages.rs` structured messages
(`read_opencode_messages`), `status.rs` compact worker status, `watch.rs` SSE push of permission
and server-health changes, `keys.rs` API key profiles/rotation (never exposed through MCP),
`quota.rs` rate limit / key-swap events. TUI-facing parsing lives in `tui/opencode/`.

### 3.7 Watch

`watch_command.rs`: builds the `watch_command` string returned for running turns
(`format_watch_command*`, `reformat_for_shell`, `ShellHint`). `find_cli_entry` searches
`PUPPET_MASTER_CLI`, the resource dir, ancestors of the exe, then `packages/cli/dist/index.js`;
`stable_copy` copies the self-contained `puppet-master-cli.bundle.mjs` into
`%APPDATA%\com.puppetmaster.app\cli` so the command survives cargo clean/rebuilds.
JS side: `packages/cli/src/watch.ts`, `watch-args.ts` (`validateWatchFlags`, `WATCH_USAGE`),
`index.ts` (dispatch; unknown watch flag -> exit 4 + usage).

---

## 4. "Where do I change X?"

| Task | Start here | Also touch |
|---|---|---|
| Add an MCP tool | `tool_registry.rs::tools` (new `ToolDefinition`; add its name to the right list in `tool_visible_in_mode`); bridge route in `bridge.rs::route` or `agent_runs/routes.rs::handle_request` | `bin/puppet-master-mcp.rs::call_tool` only if the shim must shape args/replies; `mcp_sessions.rs::authorize` if ownership applies; `packages/app/src/lib/mcp-tools.ts`; bump is automatic (`catalog_version`); rebuild MCP exe (section 6) |
| Change a tool description / schema | `tool_registry.rs` (description + `input_schema`/`output_schema`) | changes `catalog_version`, so MCP exe AND app must both be rebuilt or `SCHEMA_DRIFT` shows in `bridge_health` |
| Change the server instruction text | `tool_registry.rs::mcp_instructions` | `/mcp/instructions` route in `bridge.rs` |
| Add an agent / TUI profile | `tui/README.md` checklist: `pty/agents.rs` (`AgentType`), `packages/shared/src/agents.ts`, `agent_contexts.rs`, new `tui/<name>/mod.rs`, register in `tui/mod.rs::PROFILES`, fixtures + `tui/conformance.rs` runs automatically | `agent_adapters/` signals if the pane classifier needs priors |
| Fix result extraction for an agent | Headless: `tui/<agent>/headless.rs` / `extract_result`. Visible pane: `tui/<agent>/screen.rs`, `chrome.rs` (`is_chrome_line`, `is_tool_activity_line`, `tidy_reply`) and generic `agent_runs/completion.rs::clean_extracted_result`/`widen_reply` | tests: `agent_runs/cursor_capture_tests.rs`, fixtures in `tui/cursor/fixtures/` |
| Change completion detection (visible TUI) | `agent_runs/completion.rs::observe_tui_turn`; supervisor wiring in `headless.rs::supervise_existing` | `pty/status.rs` classifier; profile `classify_screen` |
| Change completion for headless | `headless.rs::execute_headless` + profile `headless_result_ready`/`extract_result` | |
| Prompt delivery / "Enter not sent" bugs | `agent_runs/delivery.rs`; profile `verifies_prompt_delivery`, `prompt_visible`, `clear_input_sequence` | `pty/registry.rs::is_pty_control_input` (newline handling) |
| Follow-up prompt never typed, RESULT_NOT_CAPTURED with no echo | `operations.rs::mark_operation_observation` (pane output must not leave `starting` while stage is `dispatching`), `prompt_dispatched`/`dispatch_in_flight`; supervisor repair in `agent_runs/headless.rs::redeliver_prompt` | stage log lacks `dispatched_waiting_for_task_completion` = nothing was typed |
| allowed_commands ignored a wrapped Cursor approval | `tui/cursor/approval.rs::extract` (rows cut at pane width are glued, other breaks stay newlines) | `agent_runs/approval_policy.rs` |
| Approval / trust prompts | Detection + key: `tui/<agent>/screen.rs` (`detect_prompt`, `prompt_reply`); structure/id: `pane_prompt.rs`; answer path: `agent_runs/answer_guard.rs`, `routes.rs` (`/agents/answer`); auto-approve: `approval_policy.rs` | MCP tool `answer_prompt` in `tool_registry.rs`; OpenCode permission API in `opencode/` |
| Idempotency / handles | `operations.rs::create_operation` (key conflict), `agent_runs/persist.rs` (`stable_handle`, `decide_worker`), replay branch at top of `headless.rs::run_agent` | `operations/store.rs` indexes; `routes.rs` for `by-key`/`by-handle` lookups in `bridge.rs` |
| Ownership / leases / take_over | `mcp_sessions.rs` (`authorize`, `check_run_*`, `take_over_*`, `attach_coordinator`) | `bridge.rs` handoff routes (`/agents/take-over`, `release`, `transfer`, `attach`), `docs/plans/wp7-ownership-lifecycle.md` |
| Resource locks | `bridge.rs::acquire_operation_locks`/`release_operation_locks`/`parse_operation_lock` (`RESOURCE_LOCKED`, `INVALID_LOCK`) | tools `acquire_resource_lock`/`release_resource_lock` |
| Pane closing after a run | `agent_runs/pane_close.rs`; `keep_pane` handling in `runtime.rs`/`headless.rs` | `pane_windows.rs` for window teardown |
| Acceptance checks | `agent_runs/checks.rs` (`Check` enum + `evaluate`) | schema in `tool_registry.rs` (`checks` property) |
| Watch CLI behaviour | `packages/cli/src/watch.ts`, `watch-args.ts`; `watch_command.rs` for the printed command | tests `watch.test.ts`, `watch-args.test.ts`; redeploy bundle (section 6) |
| Error codes / messages | grep the code (section 7); `OperationError::new(code, message, recoverable)` in `operations/model.rs`; HTTP mapping in `bridge.rs::operation_http_error` | |
| `build_id` / `catalog_version` | `build.rs` (build id); `tool_registry.rs::catalog_version`; surfaced in `bridge.rs::Health` and MCP `bridge_health` | `bin/puppet-master-mcp.rs` (`shim_catalog_version`, `SCHEMA_DRIFT`) |
| Warnings on a finished run | `runtime.rs::run_warnings` + `RunWarning`; profile `run_warnings`/`rejected_call_warning`; `operations::add_warning` | list of codes in `tool_registry.rs` output schema |
| Receipt outcome wording | `runtime.rs::build_receipt`, `receipt_outcome` | |
| Steering / interrupt / follow-up | `agent_runs/messaging.rs`; `runtime.rs::send_agent` | `/agents/send`, `/agents/followup` in `routes.rs` |
| OpenCode native behaviour | `opencode/*`, `agent_runs/native.rs`, `tui/opencode/` | `docs/orchestrator/opencode-native-worker.md` |
| Pane state labels (idle/blocked/...) | `pty/status.rs` + profile `classify_screen` | `lib/pane-state.ts`, `shared/src/protocol.ts` (`PaneActivityStateSchema`) |
| Wait semantics (`wait_agents`, `wait_for_panes`) | `routes.rs::wait_agents`, `operations::wait_for_operation(s)`, `pane_wait.rs` | `completion_wait_until()` default set |
| Windows-specific spawn quirks | `pty/registry.rs`, `pty/agents.rs`, `project_path.rs`, `shell_env.rs` | |
| UI | `packages/app/src` (components/, hooks/, lib/) | |

---

## 5. Invariants and conventions

- **Generic code never matches agent-type strings.** Outside `tui/` call `tui::profile_for(agent_type)`
  and use a `TuiProfile` method or `Features` flag; add a trait method with a generic default if one
  is missing (`tui/README.md`). Known leftovers still keyed by `AgentType`: `pty/agents.rs`,
  `pty/registry.rs` (PowerShell hosting), `agent_contexts.rs`, and `opencode_native` branches in
  `bridge.rs`, `agent_runs/*`, `pty/registry.rs`. `headless.rs::run_agent` special-cases
  `opencode_native` (forces `headless=false`).
- **`result_capture` provenance** (`operations/model.rs::ResultCapture`): `Authoritative` only when
  native structured evidence exists (`mark_operation_completed(.., verified=true)`, a CLI result
  event, OpenCode API message). Screen-scraped TUI replies are `Inferred` (`completion.rs`,
  `TuiTurnDecision` carries `capture`); no reply text is `Missing`. `mark_operation_finished`
  rejects non-`Missing` captures with empty evidence. An `Inferred` terminal result never reports
  `verified:true` (`runtime.rs::build_receipt`) and adds `PROVENANCE_INFERRED` (Cursor profile).
  Idle never completes a task by itself ("Pane lifecycle and task lifecycle are separate",
  `operations.rs` module doc): see `AmbiguousIdle` -> `RESULT_NOT_CAPTURED` failure (when no checks)
  or `Missing` completion (checks decide).
- **Visible Cursor result = the final message only.** `completion.rs::tui_turn_evidence` cuts the
  screen at the last tool row (`after_last_tool_block`, run on raw text before chrome rows are
  dropped), so tool headers/output, diffs and the commentary above them never reach `result`. The
  Cursor profile's `strip_dialogs` pipeline (`tui/cursor/chrome.rs`) first rejoins hard-wrapped
  `$ cmd` headers (`join_wrapped_shell_headers`) and drops the `Found N matches` row under a
  search header (`strip_search_results`). A `$ cmd` row counts as a shell header only with a
  trailing duration or `Waiting for approval` (a command quoted in the answer is kept). A reply
  that is a slice of a one-line prompt echo is kept (`keep_last_copy_of_repeated_echoes`).
  Cursor itself sometimes answers a bare-data follow-up (`[]`, `{}`) or `café ✓` with an EMPTY
  assistant message (its chat store `~/.cursor/chats/*/*/store.db` has no text part; headless does
  the same intermittently): such a turn ends with no result, the `NO_REPLY_TEXT` warning and pane
  evidence in `agent_transcript`; nothing is inferred.
- **Warnings never change status** (`RESULT_TRUNCATED`, `NO_REPLY_TEXT`, `SHELL_COMMAND_REJECTED`,
  `PROVENANCE_INFERRED`, `MODEL_UNRESOLVED`, `MODEL_UNCONFIRMED`); they are caveats on the receipt.
- **Idempotency semantics**: `run_agent`/`followup_task` mint a key if omitted (shim). An identical
  retry (same key, same agent_type, task contained, turn_index 0) replays the existing run *before*
  worker selection (`headless.rs::run_agent`); the handle for a keyless-worker create is derived from
  the key (`persist::stable_handle`). A same key with a different request -> `IDEMPOTENCY_KEY_CONFLICT`
  (`operations::create_operation`). After `WAIT_TIMEOUT`, reuse the key. A failed dispatch is durable
  and its key cannot be retried (`operations/tests.rs::dispatch_failure_is_durable_and_key_cannot_retry_it`);
  one turn index per handle (`AGENT_TURN_EXISTS`).
- **Persist before dispatch**: `create_operation` writes the snapshot before any side effect; a
  dispatch panic is recorded (`DISPATCH_PANICKED`); startup recovery marks interrupted operations
  (`recover_interrupted_operations_on_startup`, `OPERATION_INTERRUPTED`).
- **`status: interrupted` is the intentional view status for a cancelled turn** (tests assert it);
  do not rename it. `receipt.outcome` values: passed_checks, completed_unchecked, failed_checks,
  failed, cancelled, needs_input, in_progress.
- **Ownership** (`mcp_sessions.rs`): every `/agents/*` and mutating call needs a session id
  (`SESSION_REQUIRED`). The creating session is owner+controller of the handle/pane. Mutating a
  handle you don't control -> `AUTHORIZATION_DENIED` with `take_over(grant=true)` guidance. Reads and
  waits are allowed on unregistered handles, released leases and stale controllers
  (`check_run_read_access`, `check_run_wait_access`); waits never change leases. A reconnecting MCP
  can reclaim via `attach_agents(attach_token)`; `PUPPET_MASTER_CLIENT_ID` gives a stable session id.
  Shell panes are only writable via `shell_exec` in shell/both mode (`MODE_MISMATCH`).
- **`answer_prompt` pre-keystroke validation**: the caller must name the prompt id it read
  (`pane_prompt.rs::prompt_id_for`, command-based, NOT a hash of the whole screen); the guard
  (`answer_guard.rs::resolve_target`) resolves project/ownership/ambiguity first, then
  `pane_prompt::answer` re-detects the live prompt and compares ids (`STALE_PROMPT`) and checks the
  choice (`INVALID_PROMPT_CHOICE`, broad approvals need opt-in `BROAD_APPROVAL_REQUIRES_OPT_IN`).
  A repeat of the same id within `REPLAY_WINDOW_MS` returns the earlier receipt and never presses the
  key twice. Never type into a pane while a dialog is up (Cursor dialogs act on hotkeys).
  Answers are serialized (`answer_guard::serialize_answers`) so concurrent duplicates press once, and
  a pane-only answer (`pane_id`, no handle) on a pane that belongs to a run is routed through that run
  (`routes.rs::run_for_pane`) so it shares the same ledger. Run-spawned agent panes still need
  `take_over` before a pane-only answer is authorised.
- **Native (OpenCode) completion is decided from message order**, never from a baseline re-read
  after a steer: `native.rs::native_completion_evidence` requires the newest assistant message (not
  in the dispatch baseline, finish=stop) to come after the latest user message. A live steer
  registers a `SteerGuard` and the supervisor completes only via `unless_steer_in_flight` (which
  re-reads the session under the lock), so a steer is never accepted-then-ignored.
- **OpenCode yes/no goes through `answer_prompt`, not `press_key`.** Multi-line prompts to
  `opencode_native` must not be typed as raw newlines (see `is_pty_control_input`).
- **Pane close policy**: a run closes a pane only if it spawned it and `keep_pane` is not requested
  (`pane_close.rs`); never close the user's pane as cleanup. An interrupt forces keep_pane, so
  `cancel_agent` records `keep_pane_requested` to still release a run-spawned pane.
- **Tests that touch Tauri window APIs** crash the test binary (`STATUS_ENTRYPOINT_NOT_FOUND`):
  cfg(test)-gate such lib code (see `pane_windows.rs` `schedule_sweep` stub).
- **Handle names in tests must be unique**: `operations::resolve_agent_run` searches a process-wide
  index of all test projects.
- **Cursor specifics**: TUI trust/approval dialogs use hotkeys; headless runs need
  `trust_workspace`; `allow_shell` is headless-only and never with `read_only`; `allowed_commands`
  is visible-pane-only (`RunOptionSupport`).
- **Build id**: every release build re-stamps `PM_BUILD_ID`; hand-offs for live retest must carry a
  fresh `build_id` (commit first so the sha is current).

---

## 6. Build, test, deploy

All commands from the repo root unless noted. Windows is the primary dev platform; cargo lives in
`%USERPROFILE%\.cargo\bin` (the `scripts/with-cargo-path.mjs` wrapper adds it to PATH).

### Tests

```
# Rust unit + contract tests (the main suite)
cd packages/app/src-tauri && cargo test --lib
cd packages/app/src-tauri && cargo test --lib tui::            # profile + conformance tests only
cd packages/app/src-tauri && cargo test --lib agent_runs::     # run-engine tests
cd packages/app/src-tauri && cargo test --bin puppet-master-mcp  # shim tests (bin/puppet_master_mcp/tests.rs)

# JS
npm test                           # runs `test` in all workspaces (cli: tsx --test; app: vitest run)
npm test --workspace=@puppet-master/cli     # watch.test.ts, watch-args.test.ts  (or: cd packages/cli && npm test)
npm run typecheck                  # tsc --noEmit in workspaces

# MCP shim against a mock bridge (never launches a worker)
npm run test:mcp-operations        # builds the debug exe unless PUPPET_MASTER_TEST_MCP_BINARY is set
PUPPET_MASTER_TEST_MCP_BINARY=<path-to-puppet-master-mcp.exe> node scripts/test-mcp-operations.mjs
  # PowerShell: $env:PUPPET_MASTER_TEST_MCP_BINARY = "..."; node scripts/test-mcp-operations.mjs
npm run test:mcp-package           # packaging smoke test (builds, spawns exe against a mock bridge)
```

### Live regression (`scripts/pm-regression.mjs`)

Drives the REAL MCP binary against the RUNNING app and real backends (spawns agents, costs money/time).
It has **no `--help`**: flags are parsed by a hand-rolled loop (`--name value` pairs; a bare flag is
`true`), and usage lives only in the file's header comment.

```
node scripts/pm-regression.mjs [--backends opencode_native,cursor_agent] [--repeat 3]
     [--only pong,write,failing_check,visible_close] [--binary path] [--project dir] [--keep]
# benchmark mode
node scripts/pm-regression.mjs --bench [--backends ...] [--tasks small,multi|ids] [--repeat 5]
     [--bench-out results.json] [--max-p95 120] [--task-timeout 600] [--dry-run]
```

`--dry-run` implies the bench plan and never spawns anything. Scenario names (`--only`) are defined
in the script (recovery scenarios: duplicate, cancel, timeout, reconnect; also `multiline`). Don't
edit `src-tauri` while a live run is in progress (the dev app restarts and corrupts the run). Other
live helpers: `scripts/spawn-close-agent.py`, `scripts/test-bridge.ps1`, `scripts/test-e2e.ps1`.

### Dev app

`npm run dev` -> `tauri dev` (via `scripts/with-cargo-path.mjs`); `beforeDevCommand` starts Vite
(`scripts/vite-dev.mjs`, port 1420). **Tauri dev rebuilds and restarts the app on every
`src-tauri` edit**, which kills the live bridge for about a minute; poll `GET /health` (`build_id`)
to know it is back. The dev app may run as a "worker host". Debug builds re-stamp the build id only on
source/git-ref changes (set `PM_FORCE_NEW_BUILD_ID=1` to force); release builds always re-stamp.

### MCP exe rebuild (do this after ANY change to `tool_registry.rs`, `bin/puppet-master-mcp.rs`, or bridge code the shim depends on)

`node scripts/build-rust-mcp.mjs` (or `npm run build:rust --workspace=@puppet-master/mcp`) builds
`target/release` and copies to `packages/mcp-server/dist/puppet-master-mcp.exe`, but fails with
"Access is denied" while other sessions run the exe. So, instead:

```
# separate target dir so running exes are not locked
CARGO_TARGET_DIR=<scratch>/mcp-target cargo build --release --bin puppet-master-mcp   (in packages/app/src-tauri)
# swap: Windows allows renaming a running exe
mv packages/mcp-server/dist/puppet-master-mcp.exe packages/mcp-server/dist/puppet-master-mcp.old.exe
cp <scratch>/mcp-target/release/puppet-master-mcp.exe packages/mcp-server/dist/puppet-master-mcp.exe
```

Never kill other MCP processes. Then tell the user to run `/mcp` to reconnect. The user's host config
points the `puppet-master` server at `packages/mcp-server/dist/puppet-master-mcp.exe`.

### CLI bundle (watch) deploy

`node scripts/bundle-mcp.mjs` also esbuilds `packages/cli/src/index.ts` (ESM, node22) into
`packages/app/src-tauri/resources/puppet-master-cli.bundle.mjs` and the MCP stdio bundle
`mcp-stdio.bundle.cjs`, and stages the MCP exe into `resources/bin/`. The app copies the CLI bundle
to `%APPDATA%\com.puppetmaster.app\cli\puppet-master-cli.bundle.mjs` on demand
(`watch_command.rs::stable_copy`, only when size/mtime differ) the next time it prints a
`watch_command`. To deploy a CLI change by hand: run `bundle-mcp.mjs` (or just esbuild the CLI) and
copy the bundle over the `%APPDATA%` copy (stable copy is what running watchers use; a watcher that
is already running keeps the old code). `PUPPET_MASTER_CLI` overrides the entry.

### `build_id` and `catalog_version`

- `build_id` = `<short git sha>[-dirty]@<unix seconds>` from `build.rs` (`PM_BUILD_ID`), embedded in the
  *lib* and reported by `GET /health` and `bridge_health`; it is NOT in the MCP exe. Verify from
  `target/release/build/puppet_master_app-*/output`.
- `catalog_version` = `<tool count>-<16 hex>` hash of the tool catalog (`tool_registry::catalog_version`).
  `bridge_health` reports `shim_catalog_version` vs the bridge's; mismatch sets `schema_drift` /
  `SCHEMA_DRIFT` -> rebuild whichever side is stale.
- Commit finished work (so the sha is current), rebuild, and give the user the expected sha; they
  must restart the app to pick up the new build.

---

## 7. Error code glossary

Format: `CODE` - meaning - raised in. "Recoverable" is the `OperationError.recoverable` flag when
noteworthy. Grep the quoted code for the exact site. Codes shown in `bridge.rs` are HTTP-layer
validation unless noted.

**Request validation**
- `INVALID_ARGUMENT` - malformed/unknown argument - `agent_runs/{headless,quick,routes,runtime}.rs`, `bridge.rs`, `mcp_sessions.rs`, `bin/puppet-master-mcp.rs`, `bin/puppet_master_mcp/stdio.rs`.
- `INVALID_JSON` - body not JSON - `agent_runs/routes.rs` (`bridge.rs` returns `INVALID_ARGUMENT` for the same).
- `INVALID_TASK` - empty task/message - `headless.rs`, `routes.rs`, `runtime.rs`, `messaging.rs`, `quick.rs`, `operations.rs`.
- `INVALID_TIMEOUT` - `timeout_ms` (1..86400000) or `wait_ms` (0..300000) out of range - `headless.rs`, `routes.rs`, `operations.rs`.
- `INVALID_WAIT_MODE` - bad `until`/wait mode - `routes.rs`.
- `INVALID_AGENT_TYPE` - unknown agent type - `operations.rs`, `bridge.rs`.
- `INVALID_AGENT_HANDLE(S)` - malformed handle(s) - `routes.rs`, `operations.rs`.
- `INVALID_PROJECT_PATH` / `PROJECT_PATH_REQUIRED` - bad/missing project dir - `headless.rs`, `operations.rs`, `operations/store.rs`, `bridge.rs`.
- `INVALID_IDEMPOTENCY_KEY`, `INVALID_LOCK`, `INVALID_ACCEPTANCE_CRITERIA`, `INVALID_OPERATION_ID`, `INVALID_OPERATION_RESULT`, `INVALID_CWD`, `INVALID_COMMAND` - field validation - `operations.rs`/`bridge.rs`/`operations/store.rs`/`shell_exec.rs`.
- `AGENT_ID_REQUIRED`, `PANE_ID_REQUIRED`, `PANE_REQUIRED`, `SESSION_REQUIRED` - missing identifiers; `SESSION_REQUIRED` = no MCP session header (`mcp_sessions.rs::authorize`, `bridge.rs`).

**Operations and idempotency**
- `OPERATION_NOT_FOUND` - unknown operation id - `operations.rs`, `operations/store.rs`, `bridge.rs`, `answer_guard.rs`.
- `IDEMPOTENCY_KEY_CONFLICT` - same key, different request - `operations.rs`, `bridge.rs`.
- `AGENT_TURN_EXISTS` - turn index already claimed for the handle - `operations.rs`.
- `OPERATION_ALREADY_DISPATCHED`, `OPERATION_NOT_DISPATCHING`, `OPERATION_NOT_READY_TO_RESUME`, `INVALID_STATE_TRANSITION` - state machine guards - `operations.rs`, `bridge.rs`.
- `DISPATCH_FAILED`, `DISPATCH_CANCELLED`, `DISPATCH_PANICKED`, `OPERATION_CANCELLED`, `OPERATION_INTERRUPTED` - dispatch/cancel/restart outcomes - `operations.rs`, `bridge.rs`.
- `CANCELLATION_IN_PROGRESS`, `CANCELLATION_REQUIRES_WORKER_CONTROL`, `CANCEL_FAILED` - `operations.rs`, `bridge.rs`, `agent_runs/pane_close.rs`.
- `COMPLETION_EVIDENCE_REQUIRED` - completion without evidence - `operations.rs`, `bridge.rs`.
- `ACCEPTANCE_FAILED` - checks/acceptance criteria failed after the worker finished - `agent_runs/runtime.rs`, `operations.rs`.
- `STORAGE_ERROR`, `SERIALIZATION_ERROR`, `EVENT_WRITE_FAILED` - persistence/serialization - `operations/model.rs`, `messaging.rs`, `routes.rs`, `runtime.rs`, `bridge.rs`.

**Handles, workers, ownership**
- `AGENT_NOT_FOUND` - handle unknown (also the "you passed a pane id" help text) - `persist.rs`, `routes.rs`, `mcp_sessions.rs::agent_not_found`, `bridge.rs`.
- `AMBIGUOUS_HANDLE` - same handle in several projects, no `project_path` - `answer_guard.rs`.
- `AMBIGUOUS_WORKER_NAME` / `AMBIGUOUS_PANE_ID` - name/prefix matches multiple - `persist.rs`, `routes.rs`, `mcp_sessions.rs`.
- `PROJECT_MISMATCH` - explicit `project_path` is not the handle's project - `answer_guard.rs`.
- `WORKSPACE_MISMATCH` - worker lives in a different workspace than supplied - `headless.rs`, `bridge.rs`.
- `AGENT_TYPE_MISMATCH` - reused worker is another agent type - `headless.rs`.
- `WORKER_NOT_FOUND`, `WORKER_UNHEALTHY`, `WORKER_STOP_FAILED`, `WORKER_EXIT_FAILED`, `WORKER_WAIT_FAILED`, `WORKER_MCP_TOOLS_UNAVAILABLE` - worker lifecycle - `headless.rs`, `runtime.rs`, `bridge.rs`.
- `AUTHORIZATION_DENIED` - session doesn't control the handle/pane (try `take_over`) - `mcp_sessions.rs`, `answer_guard.rs`, `runtime.rs`, `bridge.rs`, `bin/puppet-master-mcp.rs`.
- `MODE_MISMATCH` - tool not available in the session's agent/shell mode - `mcp_sessions.rs`, `bridge.rs`, shim.
- `SESSION_REPLACED` - native pane replaced under the same id - `recovery.rs`.
- `RESOURCE_LOCKED`, `LOCK_RELEASE_FAILED` - `bridge.rs`.
- `TASK_NOT_FOUND`, `TASK_BLOCKED`, `TASK_OWNER_MISMATCH` - task board - `bridge.rs`.

**Capability / dispatch refusals**
- `AGENT_NOT_DISPATCHABLE` - profile `features().dispatchable` false - `headless.rs`, `bridge.rs`.
- `READ_ONLY_UNSUPPORTED` - backend can't enforce `read_only` - `headless.rs`, `operations.rs`, `bridge.rs`.
- `BACKEND_UNAVAILABLE`, `QUICK_ASK_FAILED` - `quick_ask` - `quick.rs`.
- `MODEL_UNRESOLVED` (warning), `DELIVERY_UNSUPPORTED`, `DELIVERY_FAILED`, `INTERRUPT_FAILED` - `runtime.rs`.
- `SHELL_COMMAND_REJECTED` (warning) - agent tried a shell command in a read-only/no-shell run - `runtime.rs`, `tui/cursor/mod.rs`.
- `MODEL_UNCONFIRMED`, `PROVENANCE_INFERRED` (warnings) - `tui/cursor/mod.rs`.

**Pane / delivery / result capture**
- `PANE_NOT_FOUND`, `PANE_UNAVAILABLE`, `PANE_NOT_READY`, `PANE_BUSY`, `PANE_SPAWN_FAILED`, `PANE_READINESS_FAILED`, `PANE_RESERVATION_LOST`, `PANE_ERROR`, `PANE_IO_ERROR` - pane lifecycle - `bridge.rs`, `operations.rs`, `shell_exec.rs`, `routes.rs`, `mcp_sessions.rs`, `pane_prompt.rs`.
- `PANE_GONE`, `PANE_CLOSED_AFTER_RUN`, `PANE_CLEANUP_FAILED` - `pane_close.rs`.
- `PANE_BLOCKED_BY_PROMPT` - an approval/trust dialog is up; dispatch refused (`pane_prompt::dispatch_blocker`).
- `PROMPT_NOT_SUBMITTED` - typed prompt never started a turn - `headless.rs`, `native.rs`, `bridge.rs`.
- `PROMPT_NOT_DELIVERED` - OpenCode API ack failed after one resend - `opencode/delivery.rs` (recoverable).
- `STALE_PROMPT`, `PROMPT_NOT_PENDING`, `PROMPT_UNAVAILABLE`, `INVALID_PROMPT_CHOICE`, `PROMPT_REPLY_FAILED`, `BROAD_APPROVAL_REQUIRES_OPT_IN` - `answer_prompt` path - `pane_prompt.rs`, `routes.rs`.
- `STEER_RECEIPT_NOT_FOUND` - `routes.rs`.
- `RESULT_NOT_CAPTURED` - pane idle but no reply text recoverable (recoverable) - `headless.rs`.
- `NO_REPLY_TEXT` - warning: a visible turn with checks finished with no reply text on the pane - `runtime.rs` const, added in `headless.rs`.
- `RESULT_TRUNCATED` - warning: reply longer than the screen and its start not in scrollback - `runtime.rs` const, added in `headless.rs`.
- `UNVERIFIED_RESULT` - `headless.rs` (headless result not backed by authoritative evidence).
- `LAUNCH_FAILED`, `THREAD_START_FAILED`, `TIMEOUT`, `TRANSCRIPT_READ_FAILED` - headless launch/run - `headless.rs`, `runtime.rs`.
- `PANE_*`/`SHELL_PANE_REQUIRED`, `COMMAND_TIMEOUT` - `shell_exec.rs`.

**OpenCode native**
- `OPENCODE_SERVER_UNREACHABLE` - `opencode serve` unreachable past the grace period; cancel/close still finalise runs - `agent_runs/native.rs`, `runtime.rs`.
- `NATIVE_AGENT_ERROR`, `NATIVE_STATUS_UNAVAILABLE`, `APPROVAL_WAIT_FAILED` - `native.rs`, `bridge.rs`.

**Shim / transport** (`bin/puppet-master-mcp.rs`)
- `WAIT_TIMEOUT` - foreground wait ended with the turn still running; response includes `watch_command`; retry with the same idempotency key.
- `BRIDGE_UNAVAILABLE` - no bridge/port file; `BRIDGE_ERROR` - bridge returned an unparseable/failed reply; `SCHEMA_DRIFT` - shim vs bridge catalog mismatch; `REQUEST_CANCELLED`, `RATE_LIMITED`, `NOT_FOUND`, `CONFLICT` - HTTP-status-derived fallbacks.
- `STATE_UNAVAILABLE` - read models unavailable - `bridge.rs`.

---

## 8. Not mapped confidently / known gaps

- Memory notes mention a detached daemon (`src/bin/puppet-master-daemon.rs`, `pty/event_sink.rs`,
  `bridge_host.rs`, `daemon_runtime.rs`, `docs/detached-terminals-plan.md`); none of these exist on
  this branch (`Cargo.toml` has only the `puppet-master-mcp` bin). Treat that work as not present here.
- The exact route table in `bridge.rs::route` is long and partly pattern-matched by segment slices;
  the list in section 3.1 is a summary, grep `segments ==` for the authoritative set.
- `agent_adapters/*` vs `tui/*` overlap in intent (both feed pane-state hints); `agent_adapters` is
  the older classifier-prior path.
- Frontend (`packages/app/src`) is only sketched; it mirrors Rust enums in `lib/tauri.ts` and
  `packages/shared/src/protocol.ts`.
