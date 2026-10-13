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
| [`docs/FEATURES.md`](FEATURES.md) | full user-facing feature list with where-it-lives and status |
| [`docs/TEST-PLAN.md`](TEST-PLAN.md) | automated suites (commands, expected counts) + manual/live test cases, known flaky tests and gaps |
| `docs/specs/agent-definitions/` | declarative agent spec: `SPEC.md`, `DEFINITION-FORMAT.md`, `AGENT-CHOICE.md`, `PLAN.md`, `REPORT.md` (+ "After T37"), `phases/` |

---

## 1. Architecture overview

Puppet Master is a **Tauri 2 desktop app** (Rust backend in `packages/app/src-tauri`, React/Vite UI in
`packages/app/src`) that hosts real PTY panes (`portable-pty` + `vt100`) for agent CLIs (OpenCode,
Cursor, Claude Code, Codex, plain shells) and exposes them to orchestrating agents. The Rust
library crate (`puppet_master_app_lib`) embeds a **loopback HTTP bridge** (`bridge/`, default
`127.0.0.1:17321`, scans upward from `PORT_MIN`; the chosen port is written to a port file in
the app data dir, `app_paths.rs`) that shares the same `PaneRegistry` as the GUI. External
orchestrators reach it through the **stdio MCP shim** `puppet-master-mcp`
(`src/bin/puppet-master-mcp.rs` + `src/bin/puppet_master_mcp/`, compiled from the same crate, shipped as
`packages/mcp-server/dist/puppet-master-mcp.exe` and staged to `resources/bin/`). The shim owns no
state: it advertises the tool catalog from `tool_registry/`, maps every tool call to an HTTP
request (method/path come from the `ToolDefinition`), adds an `X-Puppet-Master-Session` header and
relays the JSON reply. A small **JS CLI** (`packages/cli`, `puppet-master watch ...`) long-polls
the bridge so an orchestrator's shell can block on a background run and wake when it finishes.
Runs are persisted as **operations** (`operations.rs` hub + `operations/`): revisioned JSON snapshots
under `<project>/.puppet-master/operations/` that survive restarts. Backends are
`opencode_native` (OpenCode HTTP API + attached TUI, `opencode/`), and screen-scraped/headless CLIs
(`cursor_agent`, `claude`, `codex`, shells), all isolated behind the `TuiProfile` trait in `tui/`.

### How a `run_agent` call flows

1. **MCP tool** `run_agent` (declared in `tool_registry/catalog/agents.rs`, POST `/agents/run`). The shim
   (`bin/puppet_master_mcp/call.rs::call_tool` -> `dispatch/agents.rs`) mints an `idempotency_key` when
   missing (`recovery.rs`), injects the session id (`session.rs`), and bounds the HTTP wait
   (`DEFAULT_AGENT_WAIT_MS`).
2. **Bridge**: `bridge/http.rs::handle_connection` -> `bridge/router.rs::route`. `route` first runs
   `mobile_pairing::authorize_bridge_request` (`mobile_pairing/auth.rs`), then `mcp_sessions::authorize`
   (`mcp_sessions/authorize.rs`, session/mode/ownership gate), then the route groups in `bridge/routes/`;
   `/agents/*` goes to `agent_runs::handle_request` (`agent_runs/routes.rs`).
3. **Runtime**: `routes.rs` -> `runtime::run_agent`, re-exported from
   `agent_runs/headless/entry.rs::run_agent`. It validates (`INVALID_TASK`, `INVALID_TIMEOUT`,
   `READ_ONLY_UNSUPPORTED`, `AGENT_NOT_DISPATCHABLE` via `tui::profile_for(..).features()`), replays an
   identical retry by `idempotency_key` **before** worker selection, then `persist::decide_worker`
   either adopts an existing worker (-> `runtime/followup.rs::send_agent`, a follow-up turn) or creates a new
   handle. `persist::stable_handle(project, key)` derives the handle from the key so retries match.
4. **Operation created**: `operations/create.rs::create_operation` persists the snapshot (status `queued`)
   *before* any dispatch, enforcing idempotency (`IDEMPOTENCY_KEY_CONFLICT`) and one turn index per
   handle (`AGENT_TURN_EXISTS`). Locks (`acquire_operation_locks` in `bridge/locks.rs`) are taken here.
5. **Pane spawn / delivery**:
   - Headless (`headless=true`, Claude/Codex/Cursor): `headless/process.rs::execute_headless` thread
     spawns the CLI built by `tui::headless_launch` (profile `headless_command`) inside
     `operations/dispatch.rs::run_operation_dispatch`; stdout is read by reader threads
     (`headless/output.rs::spawn_reader`) into `agent_runs/transcript.rs`; the profile's `extract_result` /
     `extract_provider_session` parse the output (`headless/finish.rs`).
   - Visible TUI pane: the bridge spawns/reuses a pane (`bridge/pane_util.rs::spawn_pane_with_timeout`,
     `register_spawned_pane`, `pane_ready_for_dispatch`; queued dispatch in `bridge/dispatch.rs`),
     waits for readiness, then types the prompt
     (`agent_runs/delivery.rs::dispatch_prompt`; for profiles with `verifies_prompt_delivery`,
     `deliver_prompt` retypes/verifies via `DeliveryIo`/`PaneIo`).
   - `opencode_native`: `opencode/native/` spawns `opencode serve`, `opencode/client/` sends via
     REST, `opencode/delivery.rs` confirms the user message landed (`PROMPT_NOT_DELIVERED`).
6. **Supervisor / completion**: `headless/supervise.rs::supervise_existing` loops per run. Visible TUI turns go
   through `agent_runs/completion.rs::observe_tui_turn` (deterministic decision from pane status +
   screen), native runs through `agent_runs/native.rs` + `opencode/`. On completion the supervisor
   extracts the reply (`clean_extracted_result`, `widen_reply` for scrollback), evaluates `checks`
   (`agent_runs/checks.rs`), and calls `operations/finish.rs::mark_operation_finished` with a `ResultCapture`
   (`Authoritative` | `Inferred` | `Missing`). Failures use `mark_operation_state(Failed, error)`.
   `pane_close.rs::maybe_dispose_after_terminal` decides whether the run-spawned pane is closed.
7. **Operation snapshot published**: `bridge/delegation.rs::publish_operation` pushes SSE (`bridge/sse.rs`) + bumps waiters
   (`pane_wait_notify::bump_waiters`, `operations/store.rs` change condvar).
8. **Return**: `wait_for_run` (`headless/view.rs`) blocks up to `wait_ms` then builds an `AgentRunView`
   (`runtime/view.rs`): `status`, `result`, `result_capture`, `receipt` (`RunReceipt`, `outcome`; `runtime/receipt.rs`),
   `warnings`, `prompt`/`required_action` if blocked, and a `watch_command` (`watch_command.rs`)
   when still running.
9. **Wait / watch**: `wait_agents` -> `routes/wait.rs::wait_agents` (POST `/agents/wait`);
   `wait_for_operation` -> `operations/wait.rs::wait_for_operation(s)`. The JS CLI `watch` calls the bridge
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
    FEATURES.md TEST-PLAN.md  feature list; what to test and how
    specs/agent-definitions/  declarative agents spec, phases, REPORT.md
    perf/                   latency baseline (perf-latency.mjs output)
    wiki/                   developer/agent handbook (overview, architecture, developing, extending)
  packages/
    app/                    Tauri app
      src/                  React UI (App.tsx = LEGACY main window, unreachable since PF3; SettingsApp.tsx + lib/settings-entry.ts = Settings window route `?settings=1&tab=`; PwaApp.tsx mobile PWA (agent tabs, components/mobile/, hooks/usePaneViewportLease.ts phone-sized panes, lib/mobile-agent-tabs.ts), PwaLegacyApp.tsx old orchestrator PWA kept at `?pwa=legacy`, TerminalApp.tsx worker window,
                            WorkerHostBootstrap.tsx; components/ (+ settings/{sections,blocks}/, sidebar/,
                            coordination/, pwa/ tabs), hooks/ (usePaneRegistry, useBridge*), lib/ (bridge.ts +
                            bridge-*.ts split, tauri.ts, mcp-tools.ts, mcp-config.ts, pane-state.ts, ...),
                            terminal/ (canvas terminal, batchers), styles/ (per-area CSS))
      src-tauri/
        Cargo.toml          crate puppet_master_app, lib puppet_master_app_lib, bin puppet-master-mcp
        build.rs            stamps PM_BUILD_ID, then tauri_build
        tauri.conf.json     bundle resources, beforeDevCommand (scripts/vite-dev.mjs)
        tauri.dev.local.json  legacy dev main-window override (the main window no longer exists); machine-specific
        pm-dev.local.json     gitignored dev window anchor {x,y} (secondary monitor); with-cargo-path.mjs exports it as PM_WINDOW_ANCHOR
        tests/agent-definitions/kilo.toml  proof definition copy (bundled set is empty; install into <app data>/agents/)
        capabilities/ icons/ gen/ (gen is gitignored)
        resources/          bin/ (cloudflared, staged puppet-master-mcp), mcp-stdio.bundle.cjs,
                            puppet-master-cli.bundle.mjs (CLI bundle), pwa-dist/ (staged PWA)
        src/                Rust sources (section 3)
    cli/                    `puppet-master` CLI (src/index.ts: gui, mcp, worker, watch, version;
                            watch.ts + watch-args.ts = the watch command; launch-worker.ts)
    mcp-server/             MCP packaging: src/index.ts (legacy TS MCP entry: startup mode + handlers;
                            bridge-client / catalog / tool-router / registry-call / timeouts / errors /
                            types / util alongside, all `src/*.ts`), src/launcher.ts (picks the Rust binary),
                            dist/ holds the built exe
    shared/                 TS types/zod shared by app + cli + mcp (protocol.ts, agents.ts,
                            bridge-port.ts, agent-contexts.ts, mobile-pairing.ts, schemas.ts)
  scripts/
    build-rust-mcp.mjs      cargo build of puppet-master-mcp -> packages/mcp-server/dist
    bundle-mcp.mjs          stages MCP exe + esbuilds CLI bundle and mcp-stdio bundle into resources/
    bundle-cloudflared.mjs, stage-pwa-dist.mjs, ensure-tauri-resources.mjs, ensure-workspace-built.mjs
    with-cargo-path.mjs / cargo-with-path.mjs   run cargo/tauri with ~/.cargo/bin on PATH (`npm run dev` goes through with-cargo-path.mjs)
    check-window-monitor.ps1  lists visible puppet_master_app windows, exits 1 if any is on the primary monitor (run after live visible-pane tests)
    run-tauri-dev.ps1, vite-dev.mjs, dev-tunnel.mjs   dev server helpers
    test-mcp-operations.mjs mock-bridge stdio smoke test; test-mcp-package.mjs packaging test;
    test-bridge.ps1 test-e2e.ps1 test-mcp.ps1   PowerShell live checks
    pm-regression.mjs       LIVE regression + benchmark suite against the running app (entry point);
                            modules in pm-regression/: context.mjs (args, rpc), scenarios.mjs,
                            bench.mjs, bench-fixture.mjs, bench-stats.mjs, bench-tasks.mjs,
                            readonly-check.mjs (live read-only enforcement parity: claude/kilo + a write control)
    perf-latency.mjs        delegation latency measurement (PONG x backend x visible/headless, cold + follow-ups;
                            stage_log + 40ms pane-screen polling); perf-summarize.mjs / perf-frames.mjs turn the
                            raw JSON into the breakdown table in docs/perf/latency-baseline.md
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
| `bridge/` (`mod.rs` + submodules) | Embedded HTTP server (was one ~4k-line `bridge.rs`). `mod.rs`: `start_embedded_bridge`, doc and re-exports. `http.rs`: `handle_connection`, request parsing, JSON responses. `router.rs`: `route` (authorisation, `/health` via the `Health` struct = `version`, `catalog_version`, `build_id`, `tool_count`; body parsing; dispatch to the route groups). `routes/` = one `handle` per group: `panes.rs` (`/panes`), `operations.rs` (`/operations/...`), `mcp.rs` (`/mcp/{tools,session,mode,...}` + `/agents` lease/handoff routes), `tasks.rs` (`/tasks`), `locks.rs` (`/locks`), `opencode.rs` (`/opencode/...`), `wait.rs` (`/panes/wait*`), `session.rs` (goal, roles, digests), `workspace.rs` (`/workspace/state`, audit, events replay), `shell.rs` (`/shell/exec`), `pairing.rs` (`/pair`). `sse.rs`: `handle_sse`, `push_*_sse` (every payload is also recorded in `event_log.rs`). `event_log.rs`: numbered, capped log of SSE payloads behind `GET /events/poll?cursor=&wait_ms=` (long-poll for phones on tunnels that do not pass SSE; `gap` when events were dropped). `notify_pane_resized` (in `mod.rs`): pane-resize SSE + `pty://panes-changed` for desktop windows. `delegation.rs`: `delegate_operation_http`, `cancel_operation_http`, `publish_operation`. `dispatch.rs`: `dispatch_existing_operation` (background dispatch: pick/spawn pane, wait, hand to the run engine). `pane_util.rs`: `spawn_pane_with_timeout`, `register_spawned_pane`, `pane_ready_for_dispatch`. `locks.rs`: `acquire_operation_locks`, `release_operation_locks`, `parse_operation_lock`, `recover_interrupted_operations_on_startup`. `errors.rs`: `operation_http_error` maps `OperationError` -> HTTP status + body (+ `enrich_handle_error`). `handoff.rs`: take_over/release/transfer helpers. `approvals.rs`: manual-approval detection for fresh workers. `observe.rs`: reacts to pane status changes. `state.rs`: process-wide app handle/registry. `store.rs`: event-log access / read-model rebuilds. `projects.rs`: project-path resolution. `tool_names.rs`: route -> MCP tool name for the event log. Tests: `bridge/tests.rs`. `mcp_status.rs` probes `/health` for the UI. |
| `tool_registry/` | THE MCP tool catalog. `types.rs`: `ToolDefinition` (name, description, method, path, input/output schema, `ToolSafety`, visibility), `McpMode`-related wire types. `catalog/mod.rs`: `tools()` (assembled from per-family modules `catalog/{agents,context,opencode_reply,opencode_status,operations,orchestrator,panes,tasks,waits}.rs`), `catalog_version()` (hash of names/descriptions/method/path/schemas; `<count>-<hash>`), `mcp_instructions()` (server instruction text), `external_mcp_tool_count()`. `mode.rs`: `tools_for_mode`, `tool_visible_in_mode` (agent/shell/both lists). `schema.rs`: JSON-schema helpers; `extras.rs`: MCP resources and prompts; tests in `tests.rs` (pins `catalog_version`). |
| `bin/puppet-master-mcp.rs` + `bin/puppet_master_mcp/` | stdio JSON-RPC shim. `stdio.rs`: request loop, `initialize`, tools/resources/prompts listing. `call.rs`: `call_tool` (mode gating, argument shaping, result wrapping), which delegates to `dispatch/{agents,coordination,opencode,operations,panes}.rs` (one bridge request per tool). `progress.rs`: long-poll waits streaming progress; `recovery.rs`: idempotency keys + `WAIT_TIMEOUT` payloads; `session.rs`: session id (`session_id_from`, `PUPPET_MASTER_CLIENT_ID`) and starting mode; `endpoint.rs`: port-file discovery (`BRIDGE_UNAVAILABLE`); `bridge_http.rs`: HTTP client + `SCHEMA_DRIFT` check (shim catalog vs `/health.catalog_version`) + error normalisation; `args.rs`, `shell_hint.rs`; tests in `tests.rs`. |
| `mcp_sessions.rs` + `mcp_sessions/` | Per-connection state and ownership. `state.rs`: `Session`, `PaneControl` leases, process-wide store; `identity.rs`: connection identity, catalog mode (`McpMode`), reconnect / `attach_token`; `authorize.rs`: `authorize` (route-level gate); `access.rs`: `check_run_access/read_access/wait_access`; `registration.rs`: `register_owned_pane/agent`; `leases.rs`: `take_over_*`, `release_*`, `transfer_agent`, `attach_coordinator`; `handles.rs`: `resolve_pane_id`, `enrich_handle_error*` (turns a pane id used as handle into a helpful `AGENT_NOT_FOUND`); `persist.rs`: session state across bridge restarts, written by a background `coalesce.rs` writer (the guard publishes a clone; never serialize/write under the global lock). Plan: `docs/plans/wp7-ownership-lifecycle.md`. |
| `mcp_hints.rs` | Next-step hints appended to mutating tool responses. |
| `mcp_install/` (`mod.rs`, `claude.rs`, `codex.rs`, `opencode.rs`), `mcp_runtime.rs` | Register/verify the MCP server in CLI hosts' configs (project and global scope, one file per host); resolve how hosts launch the stdio server. |
| `mobile_pairing/` (`mod.rs`, `auth.rs`), `mobile_tunnel.rs`, `pwa_server.rs` | Ed25519 pairing + bearer tokens for the phone PWA; cloudflared tunnel; loopback PWA server proxying `/bridge/*`. `mobile_pairing/auth.rs::authorize_bridge_request` is the first gate in `bridge::route`. |
| `commands/` | Tauri `invoke` commands (thin wrappers over `PaneRegistry`; registered in `lib.rs`'s `invoke_handler`): `panes.rs`, `mcp.rs` (install/uninstall MCP configs), `agent_models.rs`, `opencode_keys.rs`, `coordination.rs`, `app.rs`; `AppState` in `mod.rs`. |
| `settings_store.rs`, `app_paths.rs`, `app_lifecycle.rs`, `platform.rs`, `shell_env.rs`, `env_probe.rs`, `project_path.rs`, `startup_timing.rs`, `worker_launch.rs` | Settings mirror; app data dir (`%APPDATA%\com.puppetmaster.app`, port file); install/uninstall metadata; OS helpers; login-shell PATH for GUI launches (`shell_env.rs` also scrubs the launching Claude Code session's `CLAUDECODE`/`CLAUDE_CODE_*` markers so panes start as top-level sessions); tool presence probe for `bridge_health.env`; writable project dir resolution; run-start stage timing; standalone worker window launch (`PUPPET_MASTER_WORKER*` env). |

### 3.2 `agent_runs/` (the run engine)

`agent_runs.rs` is the module root and public surface (`handle_request`, `AgentRunRequest`,
`AgentRunView`, `supervise_existing`, `stable_prompt_id`, `resolve_worker_name`, ...). Three of its
children are themselves directories: `headless/`, `runtime/`, `routes/`.

| File | Purpose / key items |
|---|---|
| `routes.rs` + `routes/` | HTTP dispatch for `/agents/*` (`handle_request`): `run`, `quick`, `wait`, `send`, `followup`, `answer`, `cancel`, `close`, `steer-receipt`, `GET /agents`, `GET /agents/{handle}`, `.../transcript`. `routes/wire.rs` request bodies + query helpers; `list.rs` (`list_agents`, `AgentListFilter`, `visible_runs`); `wait.rs` (`wait_agents`, transcript); `answer.rs` (`/agents/answer`); `handles.rs` (handle/name/pane resolution, `run_for_pane`, `latest_handle_for_pane`). |
| `runtime.rs` + `runtime/` | `request.rs`: `AgentRunRequest`; `view.rs`: `AgentRunView`; `receipt.rs`: `RunReceipt` (+`build_receipt`, `receipt_outcome`), `RunWarning` (+`run_warnings`), `RESULT_TRUNCATED` const; `followup.rs`: `send_agent` (next turn on a handle); `queue.rs`: follow-up queued behind its predecessor (`queue_followup`); `steer.rs`: `send_message` (live steer / restart / queue), `SteerView`; `control.rs`: `cancel_agent`, `interrupt_agent`, `close_agent`, `inspect_agent`; `access.rs`: handle -> snapshot with ownership gate, transcript reads; `present.rs`: live model/pane fields on a view. |
| `headless.rs` + `headless/` | `entry.rs`: `run_agent` (entry: validate, replay by idempotency key, pick worker, create operation); `bind.rs`: `bind_existing_worker` (adopt a persisted run or live pane, `WORKSPACE_MISMATCH`); `process.rs`: `execute_headless` (spawn CLI, timeout), `fail_start`; `output.rs`: `spawn_reader` stdout/stderr into the transcript; `finish.rs`: `finish_from_output`; `control.rs`: live children registry and stop; `supervise.rs`: `supervise_existing` (per-turn supervisor loop for visible/native panes); `pane_turn.rs`: `auto_approve_allowed_command`, `redeliver_prompt`, `record_completion_model`, `read_tui_screen`, `not_captured_diagnostics`; `view.rs`: `wait_for_run`, presenting snapshots. Many failure codes originate here. |
| `completion.rs` + `completion/clean.rs` | `TuiTurnWatch`, `observe_tui_turn` -> `TuiTurnDecision` (Done/Running/AmbiguousIdle/RetrySubmit/...), echo handling (`task_echo_on_screen`), `widen_reply` (longer-than-screen replies from scrollback), `completion_model`; `completion/clean.rs`: `clean_extracted_result`, `is_tui_chrome_line`. |
| `delivery.rs` | Verified prompt delivery: `DeliveryIo` trait, `Delivery`, `deliver_prompt`, `prompt_visible_in_input`, `deliver_prompt_traced` (trace lines go to the operation transcript `delivery` stream), `turn_started` (busy only counts if the screen changed after Enter), `needs_redelivery`, `PaneIo`, `dispatch_prompt` (appends the `complete_task` instruction for workers with MCP tools). |
| `checks.rs` | Acceptance checks evaluated on the real filesystem: `Check` enum (file/reply predicates, `command_exits`), `validate`, `evaluate`, `evaluate_with_commands`, `failure_summary`. `command_exits` runs an argv with timeout, never a shell string. |
| `messaging.rs` | Steering: `DeliveryMode`, `Disposition` (applied/next_action), `MessageReceipt`, `steer_target`, `decide_delivery`, `send_message`, `interrupt_action`, `pending_texts`. A steer receipt is NOT a result. |
| `persist.rs` + `persist/` | Worker-handle adoption and continuity: `worker_key`, `stable_handle` (key -> deterministic uuid), `decide_worker` (`WorkerBind::{Create,Adopt}`), `find_worker`, `resolve_worker_name`, `resolve_model_choice` (`ModelChoice`), `resolve_continuity`, `followup_policy`, `FollowupOpts`; `persist/prompt.rs`: `TurnPrompt` and conversation facts; `persist/inspect.rs`: extra `inspect_agent` fields. |
| `answer_guard.rs` | Pre-keystroke validation for `answer_prompt` on a handle: `resolve_target` (project/ownership/ambiguity -> `PROJECT_MISMATCH`/`AMBIGUOUS_HANDLE`/`AUTHORIZATION_DENIED`), replay ledger (`replay_receipt`, `record_applied`, `REPLAY_WINDOW_MS`), `finish`. |
| `approval_policy.rs` | Opt-in `allowed_commands` for visible Cursor panes: `validate`, `approved_command`, `approved_program`, `decide`. Allow-once only, never allow-always. |
| `pane_close.rs` | Central pane-disposal policy: `spawned_by_run`, `maybe_dispose_after_terminal`, `maybe_dispose_on_dispatch_failure`, `dispose_explicit_close`, `stop_worker_control`, `PANE_CLOSED_AFTER_RUN` errors. Plan: `docs/plans/wp9-pane-close.md`. |
| `capabilities.rs` | Honest per-backend `BackendCapabilities` + `NOTE_*` strings, `read_only_supported[_for]`. |
| `native.rs` | OpenCode native supervision: `stable_prompt_id`, `server_unreachable_error` (`OPENCODE_SERVER_UNREACHABLE`), `handle_native_read_failure`, `supervise_native_observation`. |
| `recovery.rs` | `on_native_pane_replaced` rebinds a worker when its native process is replaced under the same pane id (`SESSION_REPLACED`). |
| `quick.rs` | `quick_ask`: one-shot headless read-only fresh-context question; `select_backend`, `build_run_request`, `project`. |
| `transcript.rs` | Bounded per-operation transcript files: `append`, `read`, `read_turns`, `TranscriptPage`. |
| tests | `agent_runs/tests/` (topic files: `checks`, `claims`, `continuity`, `followups`, `interrupt`, `native_completion`, `native_stop`, `receipts`, `request_fields`, `steering`, `tui_turn`, `views`; loaded by `tests.rs`), `cursor_capture_tests.rs`, `cursor_delivery_tests.rs`, `delivery_contract_tests.rs`, `routes/tests.rs`, `answer_guard/tests.rs`, `approval_policy/tests.rs`, `checks/tests.rs`. |

### 3.3 `operations.rs` + `operations/`

`operations.rs` is a thin module root that re-exports the public surface of the submodules below.

| File | Purpose / key items |
|---|---|
| `operations/model.rs` | Types: `OperationStatus` (queued, running, completed, failed, cancelled, ...; `terminal()`), `StateSource`, `DelegateWorkRequest`, `OperationSnapshot`, `OperationError { code, message, recoverable, context }` (+`OperationError::new`), `OperationWarning`, `ContextPolicy`, `ContextContinuity`, `WorkerPersist`, `ResultCapture`, `AcceptanceStatus`, wait result/cursor types. |
| `operations/store.rs` | Atomic JSON persistence under `<project>/.puppet-master/operations/` (`atomic_json`, `replace_file`), project/task indexes (`operation-project-index.json`, `operation-task-index.json`), change condvar for waiters, `DELEGATE_LOCK`/`UPDATE_LOCK`. Error `INVALID_OPERATION_ID`, `OPERATION_NOT_FOUND`. |
| `operations/create.rs` | `create_operation` (validation, idempotency-key replay, queued snapshot), `delegate_work`. |
| `operations/dispatch.rs` | `run_operation_dispatch` (claim, in-flight guard), `dispatch_in_flight`, `prompt_dispatched`, `reserve_pane`, `checkpoint_operation`, `dispatch_input_if_active`, `save_operation_snapshot`. |
| `operations/startup.rs` | Startup readiness handshake: `mark_operation_startup_wait`, `mark_operation_startup_settling`, `resume_operation_dispatch[_when_ready]`. |
| `operations/transitions.rs` | `mark_operation_state`, `mark_operation_stage`, `mark_operation_observation` (pane output must not leave `starting` while stage is `dispatching`), `mark_operation_waiting_input`, `mark_operation_settled_unverified`, `add_warning`. |
| `operations/finish.rs` | `mark_operation_finished` (the only place evidence + `ResultCapture` are validated), `mark_operation_completed[_empty]`, `mark_task_completed`, `mark_turn_interrupted`; checks/acceptance evaluation. |
| `operations/cancel.rs` | `cancel_operation[_with]` and the `CANCELLATION_*` guards. |
| `operations/query.rs` | `get_operation`, project listings, `find_by_idempotency_key`, `resolve_agent_run`, `agent_runs_by_project`, `turns_for_handle`. |
| `operations/wait.rs` | `wait_for_operation(s)`, `completion_wait_until()` (default wait set). |
| `operations/recovery.rs`, `worker.rs`, `view.rs` | Startup recovery of operations left active by a previous process; worker/provider-session metadata; pure shaping helpers (context policy, trimmed output). |
| tests | `operations/tests/` (`acceptance`, `dispatch_race`, `idempotency`, `lifecycle`, `queries`, `recovery`, `waits`), and `operations_contract_tests/` (cfg(test), declared in `lib.rs`: `cancellation`, `completion`, `idempotency`, `locks`, `panes`, `recovery`, `waits`; shared fixtures in `mod.rs`). |

### 3.4 `tui/` (per-agent profiles)

See `tui/README.md` for the full layout; summary: `profile.rs` = the `TuiProfile` trait (~30 methods with
generic defaults), `types.rs` = `Features`/`Kind`/`RunOptionSupport`, `LaunchSpec`/`HeadlessLaunch` and
the other shared data types, `registry.rs` = the runtime `Registry` (an `ArcSwap` snapshot of `Arc<dyn TuiProfile>`, seeded with the nine code profiles in `read_only_backends` order; `registry()`, `snapshot()`, `profile_for_arc()`, `swap()`), `mod.rs` = free-function dispatchers over it (`profile_for` -> `Arc<dyn TuiProfile>`, `require_registered` -> `UnknownAgentType` / HTTP 400 `UNKNOWN_AGENT_TYPE`,
`headless_launch`, `is_chrome_line`, `tidy_reply`, `strip_dialogs`, `read_only_backends`,
`program_on_path`). The UI reads the same registry through the `agents_list` Tauri command (same body as `GET /agents/types`) and `packages/app/src/lib/agentRegistry.ts` (compiled nine-agent fallback offline); the spawn menus, pane titles, standby labels and `ansi.ts::isTuiAgent` consume it. `generic.rs` = default prompt handling + fallback profile. `conformance.rs` = rules
every registered profile must pass.
Declarative agents (`tui/declarative/`, spec in `docs/specs/agent-definitions/`): `schema.rs`/`validate.rs` (TOML -> `Definition`, path-carrying errors), `template.rs` (argv/stdin rendering, `render_tui`), `resolve.rs` (`resolve_binary`: PATH scan excluding cwd, npm shim -> native exe), `select.rs` + `output.rs` (selectors, `OutputParser`), `screen.rs` (`TuiEngine` for `[tui]` tables), `profile.rs` (`DeclarativeProfile`, one `TuiProfile` per definition), `sources.rs`/`loader.rs` (scan, REG-8 precedence, `build`/`reload_global`, `Entry` rows), `fingerprint.rs` + `approvals.rs` (APR fingerprints, `approvals.json`), `service.rs` (list/validate/reload/approve/revoke/consent, pure over dirs, `*_global` wrappers). HTTP: `bridge/routes/definitions.rs` (`/agents/definitions[/reload|validate|approve|revoke|consent]`; mutations are local-app-only, MCP sessions get 403). Tauri: `commands/agents.rs` (`agents_definitions_list/reload/validate`, `agents_definition_approve/revoke/preview`, `agents_definitions_consent`). UI: Settings > Agents = `components/settings/tabs/AgentsTab.tsx` + `lib/agentDefinitions.ts`. Spawn gate: `TuiProfile::spawn_gate` / `registry::admit` re-checks fingerprint, approval and the pinned binary before every run. The bundled proof agent is `kilo.toml` (installed in `<app data>/agents/`). Project and extra sources load only with per-path consent (`sources.rs` `SourceSettings`, stored in `agent-definitions.json`; `service.rs` consent/source/extra/project_scan; consent is not approval). `watch.rs` is the hot-reload watcher (notify, 150 ms debounce; a still-valid previous registry is kept if an active file turns invalid; emits `agent_definitions_changed` over SSE + Tauri; running panes and runs are never touched, the spawn gate refuses the next run until re-approval). `mcp_install/declarative.rs` is the `[mcp_host]` writer (JSON/TOML/YAML key-path set, `.bak`, home/project-only paths, idempotent; library-only, no route yet). Sharing: `loader::{export_definition, import_definition}` + Tauri `agents_export_definition` / `agents_import_definition` (import lands UNAPPROVED in the user dir, no network).
**Added on this branch (declarative agents, windows, resilience), by file**
- `tui/registry.rs`: runtime `Registry`; `set_declarative`/`code_ids`, `admit` (spawn gate), `test_serial()` (tests that swap the global registry must take it).
- `tui/declarative/`: `schema.rs`, `validate.rs` (interpreter denylist, prompt keys, `[mcp_host]` check), `template.rs` (argv/stdin/`render_tui`), `resolve.rs`, `select.rs`, `output.rs`, `profile.rs` (`DeclarativeProfile`), `loader.rs` (`build`, `reload`, `export/import_definition`, `write_generic`), `sources.rs` (`SourceSettings`, consent), `fingerprint.rs`, `approvals.rs`, `service.rs`, `screen.rs` (`TuiEngine`), `check.rs` (fixture runner for `*.expect.toml`), `watch.rs` (hot reload), `fixtures/` (kilo captures, `definitions{,-bad}`), `tests/` (per module, plus `conformance.rs`, `parity_claude.rs`, `consent.rs`, `expect.rs`).
- `agent_definitions_recorder/` (`mod.rs` start/status/stop per pane, `snapshots.rs`, `draft.rs` generator, `redact.rs`, `*_tests.rs`, `fixtures/`): the Rec button; drafts go to `<agents dir>/drafts/`, never loaded or approved. Tauri `recorder_start/status/stop` in `commands/agents.rs`.
- `bridge/routes/definitions.rs`: `/agents/definitions[/reload|validate|approve|revoke|consent|...]`; mutations local-app-only.
- `bridge/port_file.rs`: bridge port file (`write_owned`, remove on exit only if it still names our port). The MCP client (`bin/puppet_master_mcp/`) probes 17321-17330 when the file's port is dead.
- `agent_runs/headless/fallback.rs`: visible-first fallback. Runs default to `headless=false` + `placement=window`; `plan` decides `no_visible_mode` / `visible_failed` (pre-turn codes only), `convert` re-queues the same operation headless (`operations::convert_to_headless_fallback`, kills the failed pane), `start_headless` launches the child; hooked in `headless/entry.rs` (no visible mode), `bridge/dispatch.rs` (dispatch failure) and `headless/supervise.rs` (`PROMPT_NOT_SUBMITTED`). Recorded as `WorkerPersist.fallback`, `RunReceipt.fallback`, `AgentRunView.fallback`.
- `tray.rs`: tray icon + menu (`build_model` -> New pane submenu from registered agents, Panes, Settings..., Quit; `parse_command`; left-click opens Settings). The pure model is unit-tested.
- `app_windows.rs`: Rust-created windows: Settings window (`?settings=1&tab=agents`), pane windows (no main webview needed), anchor resolution (`PM_WINDOW_ANCHOR` > `settings.json` `pane_window_position` > primary monitor), cascade offsets with wrap. `settings_store.rs` keeps `pane_window_position` in the saved-settings load and strips it from the public mirror.
- `pane_windows.rs`: placement is always a window since PF3 (`placement` auto|grid|window is accepted on `run_agent`/`delegate_work`/`spawn_agent` and ignored; `decide_placement(requested, main_window_usable)` is legacy); closing a pane window kills exactly its pane; `window_label` (one pop-out per pane), `orphaned_labels`, `close_is_forced`, `on_close_requested`, `schedule_sweep` (escalates if destroy did not take), `notify_pane_removed`.
- `pty/registry/spawn.rs`: exit watcher (a dead pane is detected without waiting for ConPTY EOF).
- `lib.rs`: `tauri_plugin_single_instance` guard (second launch opens the Settings window, handles `--new-pane <agent>`, and exits); starts the tray; no startup webview (`tauri.conf.json` `windows = []`; the `--worker` host still creates label `main` from Rust).
- `mcp_install/declarative.rs`: `[mcp_host]` writer (library-only).
- UI: `lib/agentRegistry.ts` (registry + `onAgentDefinitionsChanged`, nine-agent offline fallback), `lib/agentDefinitions.ts` (injectable api, `tauriRecorderApi`), `components/settings/tabs/AgentsTab.tsx` (rows, review/approve, check, import/export, drafts, sources/consent), `components/AgentsOverviewCard.tsx` (on `HomeDashboard.tsx`), `components/TerminalPane.tsx` (Rec button), `lib/detached-pane-window.ts` + `lib/detached-close.ts` (pop-out open dedupe, close on pane exit); spawn menus read the registry (headless-only/experimental greyed with reason).
- Docs: [`FEATURES.md`](FEATURES.md), [`TEST-PLAN.md`](TEST-PLAN.md), `docs/specs/agent-definitions/`.
Per agent: `cursor/` (`mod.rs`, `launch.rs`, `headless.rs`, `screen.rs`, `chrome/` (`mod.rs`, `tests.rs`),
`approval.rs`, `fixtures/`), `claude/` (`mod.rs`, `headless.rs`), `codex/` (`mod.rs`, `headless.rs`),
`opencode/` (`mod.rs`, `screen.rs`, `read_only.rs`; covers `opencode_native` TUI-facing part),
`shell/` (`mod.rs`, `prompt.rs`).

### 3.5 Panes, prompts, waiting

| File | Purpose / key items |
|---|---|
| `pty/registry/` | `PaneRegistry` (`mod.rs`: owns live PTY sessions + per-pane `PaneClassifier`, `registry_read_full_text`); `spawn.rs` (spawn PTY, reader + idle threads, pane events; output that changes no visible cell neither reclassifies nor postpones idle); `idle.rs` (`IdleClock`, `idle_window`: a pane reads `idle` once its SCREEN held still for the profile's `idle_after`, or `quiet_idle_after` while `busy_marker` says no work is showing; polled every 100ms); `pane_io.rs` (write/read/resize, `is_pty_control_input`); `command.rs` (`CommandBuilder` per agent type, Windows PowerShell hosting for `cursor-agent.ps1`). `pty/viewer_size.rs` = phone-sized panes: per-viewer size leases (`POST /panes/{id}/viewport`, smallest grid over the viewers, 15 s TTL, sweeper restores the desktop size; desktop resizes during a lease are remembered; `PaneInfo.viewer_held`); `pty/agents.rs` = `AgentType` enum + per-OS launch commands; `pty/status.rs` = `PaneStatus`, `classify_pane_state`, `PaneClassifier`; `pty/keys.rs` named keys (`press_key`); `pty/ansi.rs`, `pty/scrollback.rs`, `pty/proc_tree.rs`; `pty/mod.rs` re-exports. |
| `pane_prompt/` | Structured approval prompt blocking a pane: `detect`, `detect_in_screen`, `dispatch_blocker` (`PANE_BLOCKED_BY_PROMPT`), `prompt_id_for` (the id handed to callers; command-based), `required_action[_from_prompt]`, `answer` (`STALE_PROMPT`, `PROMPT_REPLY_FAILED`, `INVALID_PROMPT_CHOICE`), `reply_key`. |
| `pane_wait/` + `pane_wait_notify.rs` | Long-poll waits. `mod.rs`: `wait_for_panes`, `WaitUntil`, `WaitForPanesRequest/Result`; `check.rs`: `check_pane`, settled/state/model/key-swap conditions and hints (`agent_hint_for_reason`); `model_task.rs`: `wait_for_model`, `wait_for_task`; `worker.rs`: `wait_for_worker`, spawn-readiness checks (`worker_settled_reason` lives in `check.rs`). `pane_wait_notify.rs`: `bump_waiters`, `wait_for_change[_ms]`. |
| `pane_windows.rs` | Pop-out pane windows: placement decision (`decide_placement`: grid vs window), one window per pane (`window_label`), destroys windows when their pane leaves the registry (`orphaned_labels`, `schedule_sweep` with escalation, `close_is_forced`). |
| `shell_exec.rs` (+`shell_exec/command.rs`, `tests.rs`) | `shell_exec`: run a command in an explicitly selected shell pane (`SHELL_PANE_REQUIRED`, `COMMAND_TIMEOUT`, `PANE_NOT_READY`). |
| `agent_adapters/{claude,codex,opencode,bash}.rs` | Older per-agent `signals()` priors for the pane-state classifier (separate from `tui/`). |
| `projections.rs` (+`projections/summary.rs`, `tests.rs`), `event_log.rs` (+`event_log/recent.rs`, `tests.rs`; `find_last_entry` scans the log from its end, so a pane's last event never parses the whole file), `events.rs`, `actors.rs` | Event stream (`SystemEvent`), append-only event log per project (recent-event queries in `event_log/recent.rs`), read models (`WorkspaceAttention`, `GET /workspace/state`; audit summaries in `projections/summary.rs`), actor/role bookkeeping. |
| `session_context.rs`, `context_pack.rs`, `agent_contexts/` (`mod.rs`, `profiles.rs`, `inspect.rs`, `tests.rs`), `project_ir.rs` | Session context store, context packs, agent context profiles/labels (`AgentContextProfile` in `profiles.rs`; model detection/inspection in `inspect.rs`), librarian project IR reader. |

### 3.5b `standby/` (opt-in warm worker pool)

Off by default. Per backend and kind (visible pane / headless process) a worker for a recently used project is
started ahead of time; `run_agent` claims it instead of spawning. Settings + history live in
`<app data>/standby-pool.json` (hand edits are applied within a second); UI: Settings > Preload
(`PreloadTab.tsx`, `lib/standby.ts`, Tauri commands `standby_get_status` / `standby_set_config`). **Preload** = `config.rs::PreloadConfig` (master + per-provider count 0-2): the pool warms those providers in `standby::default_project` (settings `project_path`, not the registry's volatile project) without waiting for a run, creates the pane's hidden window in `host.rs` (`app_windows::open_warm_pane_window`, `?warm=1`), and a claim (`tray::new_pane` or `bridge/dispatch.rs`) reveals the pane and `open_pane_window` places + shows that window; `pane_windows.rs` sweeps use `list_all()` so hidden warm windows survive.

| File | Purpose / key items |
|---|---|
| `standby/pool.rs` | State machine, no I/O: `Pool` (`note_run`, `claim` -> `ClaimOutcome`, `reconcile`, `set_config`, `shutdown`, `status`), `Host` trait (spawn / check / kill), `Claim` guard (dropped uncommitted = worker killed, so a cancelled claim leaks nothing). Rules: one per `PoolKey`, top N recent projects inside X hours, global cap, claimable after two ready observations, idle expiry leaves the key cold until its next run, dialog / dead / startup-timeout = discard + backoff (cold after 3). |
| `standby/host.rs` | `LiveHost`: hidden PTY panes (`pty/hidden.rs`, `registry_spawn_pane` under a pre-hidden id) and waiting headless processes; health = profile `standby_screen` + pane status idle. |
| `standby/key.rs`, `config.rs`, `mod.rs` | `PoolKey` (agent, canonical project, model, read_only, allow_shell, headless), settings/history file, global API (`init`, `claim_visible`, `claim_headless`, `note_run`, `status`, `shutdown`). Tests: `standby/tests.rs`. |
| `pty/hidden.rs` | Hidden-pane set; `PaneRegistry::list()` skips them (`list_all()` does not), so UI, `/panes`, workspace state and run adoption never see a standby until `reveal`. |
| `tui/<agent>/` | `TuiProfile::{standby_support, standby_screen, headless_standby_command, headless_standby_input}`: Cursor (visible only: headless takes its prompt as an argument), Claude (visible + `claude -p --input-format stream-json`). |
| integration | `bridge/dispatch.rs::assign_pane` (claim before spawning; `standby_claimed` stage; claimed pane = run-created, so `keep_pane=false` disposes it), `agent_runs/headless/process.rs` (`hand_task_to_standby`, falls back to a normal spawn), `operations::mark_standby_claimed`, `worker.standby_used` -> `receipt.standby_used` / view `standby_used`, `GET /standby`, `/health.standby` counts. Visible standbys never answer a trust dialog: an untrusted folder is simply not pooled. |

### 3.6 `opencode/` (native HTTP backend, not screen scraping)

`client/` (`mod.rs` REST client v1 API, `models.rs` session/message reads, `interactions.rs` permission and
question requests/replies), `native/` (`mod.rs` spawn `opencode serve` + session + attach TUI, `process.rs`
serve handle / free port / PTY reader, `reattach.rs` re-attach TUI to a running server),
`delivery.rs` delivery ack (`PROMPT_NOT_DELIVERED`), `messages.rs` structured messages
(`read_opencode_messages`), `status.rs` compact worker status, `watch.rs` SSE push of permission
and server-health changes, `keys.rs` API key profiles/rotation (never exposed through MCP),
`quota.rs` rate limit / key-swap events, `link.rs` `OpenCodeLink` (native pane <-> serve process link) and worker event emit. TUI-facing parsing lives in `tui/opencode/`.

### 3.7 Watch

`watch_command.rs`: builds the `watch_command` string returned for running turns
(`format_watch_command*`, `reformat_for_shell`, `ShellHint`). `find_cli_entry` searches
`PUPPET_MASTER_CLI`, the resource dir, ancestors of the exe, then `packages/cli/dist/index.js`;
`reformat_for_shell` (used by the MCP shim, which cannot resolve the bundle itself) keeps the entry path the app already chose; `stable_copy` copies the self-contained `puppet-master-cli.bundle.mjs` into
`%APPDATA%\com.puppetmaster.app\cli` so the command survives cargo clean/rebuilds.
JS side: `packages/cli/src/watch.ts`, `watch-args.ts` (`validateWatchFlags`, `WATCH_USAGE`),
`index.ts` (dispatch; unknown watch flag -> exit 4 + usage).

---

## 4. "Where do I change X?"

| Task | Start here | Also touch |
|---|---|---|
| Add an MCP tool | `tool_registry/catalog/<family>.rs` (new `ToolDefinition`; add its name to the right list in `tool_registry/mode.rs::tool_visible_in_mode`); bridge route in `bridge/routes/<group>.rs` (dispatched from `bridge/router.rs::route`) or `agent_runs/routes.rs::handle_request` | `bin/puppet_master_mcp/call.rs` + `dispatch/<family>.rs` only if the shim must shape args/replies; `mcp_sessions/authorize.rs::authorize` if ownership applies; `packages/app/src/lib/mcp-tools.ts`; bump is automatic (`catalog_version`); rebuild MCP exe (section 6) |
| Change a tool description / schema | `tool_registry/catalog/<family>.rs` (description + `input_schema`/`output_schema`); pinned `catalog_version` in `tool_registry/tests.rs` | changes `catalog_version`, so MCP exe AND app must both be rebuilt or `SCHEMA_DRIFT` shows in `bridge_health` |
| Change the server instruction text | `tool_registry/catalog/mod.rs::mcp_instructions` | `/mcp/instructions` route in `bridge/routes/mcp.rs` |
| Add an agent / TUI profile | `tui/README.md` checklist: `pty/agents.rs` (`AgentType`), `packages/shared/src/agents.ts`, `agent_contexts/profiles.rs`, new `tui/<name>/mod.rs`, register in `tui/registry.rs::Registry::seeded()` (order is `read_only_backends` order), fixtures + `tui/conformance.rs` runs automatically | `agent_adapters/` signals if the pane classifier needs priors |
| Add an agent without code (TOML definition) | one file in `<app data>/agents/<id>.toml` (format: `docs/specs/agent-definitions/DEFINITION-FORMAT.md`; worked example `kilo.toml`); validate with `POST /agents/definitions/validate`; approve in Settings > Agents | `tui/declarative/` (schema, validate, loader, approvals, service, watch), `bridge/routes/definitions.rs`, fixtures in `tui/declarative/fixtures/<name>/`; a receipt records `agent_definition` + `run_features` (what the profile held at spawn could enforce) |
| Fix result extraction for an agent | Headless: `tui/<agent>/headless.rs` / `extract_result`. Visible pane: `tui/<agent>/screen.rs`, `chrome/mod.rs` (`is_chrome_line`, `is_tool_activity_line`, `tidy_reply`) and generic `agent_runs/completion/clean.rs::clean_extracted_result` / `completion.rs::widen_reply` | tests: `agent_runs/cursor_capture_tests.rs`, fixtures in `tui/cursor/fixtures/` |
| Change completion detection (visible TUI) | `agent_runs/completion.rs::observe_tui_turn`; supervisor wiring in `headless/supervise.rs::supervise_existing`; how soon a finished pane reads idle: profile `idle_after` / `quiet_idle_after` / `busy_marker` (`tui/<agent>/screen.rs`) + `pty/registry/idle.rs` | `pty/status.rs` classifier; profile `classify_screen` |
| Change completion for headless | `headless/process.rs::execute_headless`, `headless/finish.rs` + profile `headless_result_ready`/`extract_result` | |
| Prompt delivery / "Enter not sent" bugs (Claude takes text+Enter in one burst for a paste: it needs the verified protocol) | `agent_runs/delivery.rs` (80ms polls, same 2s/3s/1.5s budgets); profile `verifies_prompt_delivery`, `prompt_visible`, `clear_input_sequence` | `pty/registry/pane_io.rs::is_pty_control_input` (newline handling) |
| Follow-up prompt never typed, RESULT_NOT_CAPTURED with no echo | `operations/transitions.rs::mark_operation_observation` (pane output must not leave `starting` while stage is `dispatching`), `operations/dispatch.rs` `prompt_dispatched`/`dispatch_in_flight`; supervisor repair in `agent_runs/headless/pane_turn.rs::redeliver_prompt` | stage log lacks `dispatched_waiting_for_task_completion` = nothing was typed |
| allowed_commands ignored a wrapped Cursor approval | `tui/cursor/approval.rs::extract` (rows cut at pane width are glued, other breaks stay newlines) | `agent_runs/approval_policy.rs` |
| Approval / trust prompts | Detection + key: `tui/<agent>/screen.rs` (`detect_prompt`, `prompt_reply`); structure/id: `pane_prompt/`; answer path: `agent_runs/answer_guard.rs`, `routes/answer.rs` (`/agents/answer`); auto-approve: `approval_policy.rs` | MCP tool `answer_prompt` in `tool_registry/catalog/agents.rs`; OpenCode permission API in `opencode/` |
| Idempotency / handles | `operations/create.rs::create_operation` (key conflict), `agent_runs/persist.rs` (`stable_handle`, `decide_worker`), replay branch at top of `headless/entry.rs::run_agent` | `operations/store.rs` indexes; `bridge/routes/operations.rs` for `by-key`/`by-handle` lookups |
| Ownership / leases / take_over | `mcp_sessions/` (`authorize.rs`, `access.rs` `check_run_*`, `leases.rs` `take_over_*`/`attach_coordinator`) | `bridge/handoff.rs` + `bridge/routes/mcp.rs` handoff routes (`/agents/take-over`, `release`, `transfer`, `attach`), `docs/plans/wp7-ownership-lifecycle.md` |
| Resource locks | `bridge/locks.rs` `acquire_operation_locks`/`release_operation_locks`/`parse_operation_lock` (`RESOURCE_LOCKED`, `INVALID_LOCK`); routes `bridge/routes/locks.rs` | tools `acquire_resource_lock`/`release_resource_lock` |
| Pane closing after a run | `agent_runs/pane_close.rs`; `keep_pane` handling in `runtime/` and `headless/` | `pane_windows.rs` for window teardown |
| Acceptance checks | `agent_runs/checks.rs` (`Check` enum + `evaluate`) | schema in `tool_registry/catalog/agents.rs` (`checks` property) |
| Watch CLI behaviour | `packages/cli/src/watch.ts`, `watch-args.ts`; `watch_command.rs` for the printed command | tests `watch.test.ts`, `watch-args.test.ts`; redeploy bundle (section 6) |
| Error codes / messages | grep the code (section 7); `OperationError::new(code, message, recoverable)` in `operations/model.rs`; HTTP mapping in `bridge/errors.rs::operation_http_error` | |
| `build_id` / `catalog_version` | `build.rs` (build id); `tool_registry/catalog/mod.rs::catalog_version`; surfaced in `bridge/router.rs::Health` and MCP `bridge_health` | `bin/puppet_master_mcp/bridge_http.rs` (`SCHEMA_DRIFT`) |
| Warnings on a finished run | `runtime/receipt.rs::run_warnings` + `RunWarning`; profile `run_warnings`/`rejected_call_warning`; `operations/transitions.rs::add_warning` | list of codes in the `tool_registry/catalog/` output schema |
| Receipt outcome wording | `runtime/receipt.rs::build_receipt`, `receipt_outcome` | |
| Steering / interrupt / follow-up | `agent_runs/messaging.rs`; `runtime/followup.rs::send_agent`, `runtime/steer.rs::send_message` | `/agents/send`, `/agents/followup` in `routes.rs` |
| OpenCode native behaviour | `opencode/*`, `agent_runs/native.rs`, `tui/opencode/` | `docs/orchestrator/opencode-native-worker.md` |
| Pane state labels (idle/blocked/...) | `pty/status.rs` + profile `classify_screen` | `lib/pane-state.ts`, `shared/src/protocol.ts` (`PaneActivityStateSchema`) |
| Wait semantics (`wait_agents`, `wait_for_panes`) | `routes/wait.rs::wait_agents`, `operations/wait.rs::wait_for_operation(s)`, `pane_wait/` | `completion_wait_until()` default set |
| Warm standby pool (what is kept warm, claim rules, why a key is held back) | `standby/pool.rs` (rules), `standby/host.rs` (live spawn/health), `GET /standby` for state | profile `standby_screen` / `headless_standby_*` in `tui/<agent>/`; claim sites `bridge/dispatch.rs::assign_pane`, `headless/process.rs`; `scripts/perf-latency.mjs --fixed-dir --cold-gap` to measure |
| Delegation latency (measure first: `scripts/perf-latency.mjs`, results in `docs/perf/latency-baseline.md`) | per-stage: `operations/store.rs::bump` (stage_log), `startup_timing.rs`, `bridge/dispatch.rs`, `agent_runs/delivery.rs`, `pty/registry/idle.rs` | whole-file scans on hot paths (`event_log::find_last_entry`, `agent_runs/headless/bind.rs::worker_lookup_key`), `mcp_sessions/state.rs` writer |
| Windows-specific spawn quirks | `pty/registry/command.rs`, `pty/agents.rs`, `project_path.rs`, `shell_env.rs` | |
| UI | `packages/app/src` (components/, hooks/, lib/); settings tabs in `components/settings/tabs/` (shown in the Settings window); `App.tsx`, `TerminalGrid.tsx`, `HomeDashboard.tsx` and the orchestrator sidebar `components/sidebar/` are **unreachable legacy** since PF3; styles in `styles/` | Tauri command called from the UI: `lib/tauri.ts` + `commands/` + `lib.rs` `invoke_handler` |
| Pane window creation, position, close-kills-pane, lifecycle | `app_windows.rs` (anchor, cascade, creation), `pane_windows.rs` (`schedule_sweep`, close handling), `lib/detached-close.ts` | `placement` in `tool_registry/catalog/agents.rs` (ignored); legacy `lib/detached-pane-window.ts`, `App.tsx` `pane://detach` |
| Tray menu / Settings window / second launch / `--new-pane` | `tray.rs`, `app_windows.rs`, `lib.rs` single-instance callback; UI `SettingsApp.tsx`, `lib/settings-entry.ts` | `components/settings/tabs/*Tab.tsx` |
| Visible-first fallback to headless | `agent_runs/headless/fallback.rs` (`plan`, `convert`, `start_headless`) | hooks in `headless/entry.rs`, `bridge/dispatch.rs`, `headless/supervise.rs` |
| Dev window position / focus | `packages/app/src-tauri/pm-dev.local.json` (anchor) read by `scripts/with-cargo-path.mjs` -> `PM_WINDOW_ANCHOR`; `app_windows.rs` | verify with `scripts/check-window-monitor.ps1` |
| Single instance / bridge port discovery | `lib.rs` (single-instance plugin), `bridge/port_file.rs`, MCP client port probing in `bin/puppet_master_mcp/` | |
| Settings > Agents UI / Rec button | `components/settings/tabs/AgentsTab.tsx`, `lib/agentDefinitions.ts`, `components/TerminalPane.tsx` | `agent_definitions_recorder/`, `commands/agents.rs` |
| Agent registry in the UI (tray New pane, Settings; legacy spawn menu/Overview card) | `lib/agentRegistry.ts`, `tray.rs` (legacy: `components/AgentsOverviewCard.tsx`) | `GET /agents/types` |
| Add a Tauri command | `commands/<area>.rs` + register in `lib.rs` `invoke_handler` | `packages/app/src/lib/tauri.ts` wrapper |

---

## 5. Invariants and conventions

- **Generic code never matches agent-type strings.** Outside `tui/` call `tui::profile_for(agent_type)`
  and use a `TuiProfile` method or `Features` flag; add a trait method with a generic default if one
  is missing (`tui/README.md`). Known leftovers still keyed by `AgentType`: `pty/agents.rs`,
  `pty/registry/command.rs` (PowerShell hosting), `agent_contexts/`, and `opencode_native` branches in
  `bridge/`, `agent_runs/*`, `pty/registry/`. `headless/entry.rs::run_agent` special-cases
  `opencode_native` (forces `headless=false`).
- **`result_capture` provenance** (`operations/model.rs::ResultCapture`): `Authoritative` only when
  native structured evidence exists (`mark_operation_completed(.., verified=true)`, a CLI result
  event, OpenCode API message). Screen-scraped TUI replies are `Inferred` (`completion.rs`,
  `TuiTurnDecision` carries `capture`); no reply text is `Missing`. `mark_operation_finished`
  rejects non-`Missing` captures with empty evidence. An `Inferred` terminal result never reports
  `verified:true` (`runtime/receipt.rs::build_receipt`) and adds `PROVENANCE_INFERRED` (Cursor profile).
  Idle never completes a task by itself ("Pane lifecycle and task lifecycle are separate",
  `operations.rs` module doc): see `AmbiguousIdle` -> `RESULT_NOT_CAPTURED` failure (when no checks)
  or `Missing` completion (checks decide).
- **Visible Cursor result = the final message only.** `agent_runs/completion.rs::tui_turn_evidence` cuts the
  screen at the last tool row (`after_last_tool_block`, run on raw text before chrome rows are
  dropped), so tool headers/output, diffs and the commentary above them never reach `result`. The
  Cursor profile's `strip_dialogs` pipeline (`tui/cursor/chrome/mod.rs`) first rejoins hard-wrapped
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
  `resolved_model` sources, best first: the CLI's own output (`TuiProfile::extract_model`, Claude `modelUsage`; recorded in
  `headless/finish.rs`), the live TUI/OpenCode session (persisted by `native.rs` BEFORE the terminal revision so a woken
  waiter never reads null), then the CLI's configured default (`default_model`: Cursor `about`, Codex `config.toml`) flagged
  `model_unconfirmed` -> `MODEL_UNCONFIRMED`. Completed with none of these -> `MODEL_UNRESOLVED` (`runtime/receipt.rs::run_warnings`).
- **Idempotency semantics**: `run_agent`/`followup_task` mint a key if omitted (shim). An identical
  retry (same key, same agent_type, task contained, turn_index 0) replays the existing run *before*
  worker selection (`headless/entry.rs::run_agent`); the handle for a keyless-worker create is derived from
  the key (`persist::stable_handle`). A same key with a different request -> `IDEMPOTENCY_KEY_CONFLICT`
  (`operations/create.rs::create_operation`). After `WAIT_TIMEOUT`, reuse the key. A failed dispatch is durable
  and its key cannot be retried (`operations/tests/lifecycle.rs::dispatch_failure_is_durable_and_key_cannot_retry_it`);
  one turn index per handle (`AGENT_TURN_EXISTS`).
- **Persist before dispatch**: `create_operation` writes the snapshot before any side effect; a
  dispatch panic is recorded (`DISPATCH_PANICKED`); startup recovery marks interrupted operations
  (`bridge/locks.rs::recover_interrupted_operations_on_startup` -> `operations/recovery.rs`, `OPERATION_INTERRUPTED`).
- **`status: interrupted` is the intentional view status for a cancelled turn** (tests assert it);
  do not rename it. `receipt.outcome` values: passed_checks, completed_unchecked, failed_checks,
  failed, cancelled, needs_input, in_progress.
- **Ownership** (`mcp_sessions/`): every `/agents/*` and mutating call needs a session id
  (`SESSION_REQUIRED`). The creating session is owner+controller of the handle/pane. Mutating a
  handle you don't control -> `AUTHORIZATION_DENIED` with `take_over(grant=true)` guidance. Reads and
  waits are allowed on unregistered handles, released leases and stale controllers
  (`check_run_read_access`, `check_run_wait_access`); waits never change leases. A reconnecting MCP
  can reclaim via `attach_agents(attach_token)`; `PUPPET_MASTER_CLIENT_ID` gives a stable session id.
  Shell panes are only writable via `shell_exec` in shell/both mode (`MODE_MISMATCH`).
- **`answer_prompt` pre-keystroke validation**: the caller must name the prompt id it read
  (`pane_prompt/mod.rs::prompt_id_for`, command-based, NOT a hash of the whole screen); the guard
  (`answer_guard.rs::resolve_target`) resolves project/ownership/ambiguity first, then
  `pane_prompt::answer` re-detects the live prompt and compares ids (`STALE_PROMPT`) and checks the
  choice (`INVALID_PROMPT_CHOICE`, broad approvals need opt-in `BROAD_APPROVAL_REQUIRES_OPT_IN`).
  A repeat of the same id within `REPLAY_WINDOW_MS` returns the earlier receipt and never presses the
  key twice. Never type into a pane while a dialog is up (Cursor dialogs act on hotkeys).
  Answers are serialized (`answer_guard::serialize_answers`) so concurrent duplicates press once, and
  a pane-only answer (`pane_id`, no handle) on a pane that belongs to a run is routed through that run
  (`routes/handles.rs::run_for_pane`) so it shares the same ledger. Run-spawned agent panes still need
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
- **Concurrency tests must not rely on sleeps for overlap**: use a rendezvous with a bounded wait
  (see `operations_contract_tests/panes.rs`) and generous channel timeouts (30s) so a loaded CI box or a
  concurrent dev-app rebuild cannot flake them.
- **Cursor specifics**: TUI trust/approval dialogs use hotkeys; headless runs need
  `trust_workspace`; `allow_shell` is headless-only and never with `read_only` (`opencode_native` accepts it as a no-op: its bash tool already runs without opt-in, `RunOptionSupport.shell_default_on`, validated by `headless/entry.rs::allow_shell_applies`); `allowed_commands`
  is visible-pane-only (`RunOptionSupport`).
- **Phone event stream**: Cloudflare quick tunnels (`*.trycloudflare.com`) never pass server-sent
  events through, so remote phones must use `GET /events/poll` (`lib/bridge-sse.ts` picks it for any
  non-loopback host). A phone that re-fetches the screen (remount) hides a dead stream: test live
  output by typing from the PC with nothing else happening.
- **Phone-sized panes resize in place**: a mirror with `onFitGrid` keeps one terminal session across
  sizes (remounting removed the focused input and closed the keyboard), keeps its stream
  (`updatePaneDimensions(..., keepStream)`), sizes from cell metrics, and has no local echo.
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
# note: `cargo test` needs packages/app/dist to exist (tauri embeds it): run `npx vite build` in packages/app once

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

### MCP exe rebuild (do this after ANY change to `tool_registry/`, `bin/puppet-master-mcp.rs`, `bin/puppet_master_mcp/`, or bridge code the shim depends on)

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
noteworthy. Grep the quoted code for the exact site. Codes shown in `bridge/` are HTTP-layer
validation unless noted.

**Request validation**
- `INVALID_ARGUMENT` - malformed/unknown argument - `agent_runs/{headless,runtime,routes}/`, `agent_runs/quick.rs`, `bridge/`, `mcp_sessions/`, `bin/puppet_master_mcp/`.
- `INVALID_JSON` - body not JSON - `agent_runs/routes.rs` (`bridge/` returns `INVALID_ARGUMENT` for the same).
- `INVALID_TASK` - empty task/message - `agent_runs/{headless,runtime,routes}/`, `messaging.rs`, `quick.rs`, `operations/create.rs`.
- `INVALID_TIMEOUT` - `timeout_ms` (1..86400000) or `wait_ms` (0..300000) out of range - `agent_runs/{headless,routes}/`, `operations/`.
- `INVALID_WAIT_MODE` - bad `until`/wait mode - `routes.rs`.
- `INVALID_AGENT_TYPE` - unknown agent type - `operations/create.rs`, `bridge/`.
- `INVALID_AGENT_HANDLE(S)` - malformed handle(s) - `agent_runs/routes/`, `operations/`.
- `INVALID_PROJECT_PATH` / `PROJECT_PATH_REQUIRED` - bad/missing project dir - `agent_runs/headless/`, `operations/`, `bridge/`.
- `INVALID_IDEMPOTENCY_KEY`, `INVALID_LOCK`, `INVALID_ACCEPTANCE_CRITERIA`, `INVALID_OPERATION_ID`, `INVALID_OPERATION_RESULT`, `INVALID_CWD`, `INVALID_COMMAND` - field validation - `operations/`, `bridge/`, `shell_exec.rs`.
- `AGENT_ID_REQUIRED`, `PANE_ID_REQUIRED`, `PANE_REQUIRED`, `SESSION_REQUIRED` - missing identifiers; `SESSION_REQUIRED` = no MCP session header (`mcp_sessions/authorize.rs`, `bridge/`).

**Operations and idempotency**
- `OPERATION_NOT_FOUND` - unknown operation id - `operations/`, `bridge/`, `answer_guard.rs`.
- `IDEMPOTENCY_KEY_CONFLICT` - same key, different request - `operations/create.rs`, `bridge/`.
- `AGENT_TURN_EXISTS` - turn index already claimed for the handle - `operations/create.rs`.
- `OPERATION_ALREADY_DISPATCHED`, `OPERATION_NOT_DISPATCHING`, `OPERATION_NOT_READY_TO_RESUME`, `INVALID_STATE_TRANSITION` - state machine guards - `operations/`, `bridge/`.
- `DISPATCH_FAILED`, `DISPATCH_CANCELLED`, `DISPATCH_PANICKED`, `OPERATION_CANCELLED`, `OPERATION_INTERRUPTED` - dispatch/cancel/restart outcomes - `operations/`, `bridge/`.
- `CANCELLATION_IN_PROGRESS`, `CANCELLATION_REQUIRES_WORKER_CONTROL`, `CANCEL_FAILED` - `operations/cancel.rs`, `bridge/`, `agent_runs/pane_close.rs`.
- `COMPLETION_EVIDENCE_REQUIRED` - completion without evidence - `operations/finish.rs`, `bridge/`.
- `ACCEPTANCE_FAILED` - checks/acceptance criteria failed after the worker finished - `agent_runs/runtime/`, `operations/finish.rs`.
- `STORAGE_ERROR`, `SERIALIZATION_ERROR`, `EVENT_WRITE_FAILED` - persistence/serialization - `operations/`, `messaging.rs`, `agent_runs/routes/`, `agent_runs/runtime/`, `bridge/`.

**Handles, workers, ownership**
- `AGENT_NOT_FOUND` - handle unknown (also the "you passed a pane id" help text) - `persist.rs`, `agent_runs/routes/`, `mcp_sessions/handles.rs`, `bridge/`.
- `AMBIGUOUS_HANDLE` - same handle in several projects, no `project_path` - `answer_guard.rs`.
- `AMBIGUOUS_WORKER_NAME` / `AMBIGUOUS_PANE_ID` - name/prefix matches multiple - `persist.rs`, `agent_runs/routes/`, `mcp_sessions/`.
- `PROJECT_MISMATCH` - explicit `project_path` is not the handle's project - `answer_guard.rs`.
- `WORKSPACE_MISMATCH` - worker lives in a different workspace than supplied - `agent_runs/headless/bind.rs`, `bridge/`.
- `AGENT_TYPE_MISMATCH` - reused worker is another agent type - `agent_runs/headless/`.
- `WORKER_NOT_FOUND`, `WORKER_UNHEALTHY`, `WORKER_STOP_FAILED`, `WORKER_EXIT_FAILED`, `WORKER_WAIT_FAILED`, `WORKER_MCP_TOOLS_UNAVAILABLE` - worker lifecycle - `agent_runs/headless/`, `agent_runs/runtime/`, `bridge/`.
- `AUTHORIZATION_DENIED` - session doesn't control the handle/pane (try `take_over`) - `mcp_sessions/`, `answer_guard.rs`, `agent_runs/runtime/`, `bridge/`, `bin/puppet_master_mcp/`.
- `MODE_MISMATCH` - tool not available in the session's agent/shell mode - `mcp_sessions/`, `bridge/`, shim.
- `SESSION_REPLACED` - native pane replaced under the same id - `recovery.rs`.
- `RESOURCE_LOCKED`, `LOCK_RELEASE_FAILED` - `bridge/locks.rs`.
- `TASK_NOT_FOUND`, `TASK_BLOCKED`, `TASK_OWNER_MISMATCH` - task board - `bridge/routes/tasks.rs`.

**Capability / dispatch refusals**
- `AGENT_NOT_DISPATCHABLE` - profile `features().dispatchable` false - `agent_runs/headless/entry.rs`, `bridge/`.
- `READ_ONLY_UNSUPPORTED` - backend can't enforce `read_only` - `agent_runs/headless/entry.rs`, `operations/create.rs`, `bridge/`.
- `BACKEND_UNAVAILABLE`, `QUICK_ASK_FAILED` - `quick_ask` - `quick.rs`.
- `MODEL_UNRESOLVED` (warning), `DELIVERY_UNSUPPORTED`, `DELIVERY_FAILED`, `INTERRUPT_FAILED` - `agent_runs/runtime/`.
- `SHELL_COMMAND_REJECTED` (warning) - agent tried a shell command in a read-only/no-shell run - `agent_runs/runtime/receipt.rs`, `tui/cursor/mod.rs`.
- `MODEL_UNCONFIRMED`, `PROVENANCE_INFERRED` (warnings) - `tui/cursor/mod.rs`.

**Pane / delivery / result capture**
- `PANE_NOT_FOUND`, `PANE_UNAVAILABLE`, `PANE_NOT_READY`, `PANE_BUSY`, `PANE_SPAWN_FAILED`, `PANE_READINESS_FAILED`, `PANE_RESERVATION_LOST`, `PANE_ERROR`, `PANE_IO_ERROR` - pane lifecycle - `bridge/`, `operations/`, `shell_exec.rs`, `agent_runs/routes/`, `mcp_sessions/`, `pane_prompt/`.
- `PANE_GONE`, `PANE_CLOSED_AFTER_RUN`, `PANE_CLEANUP_FAILED` - `pane_close.rs`.
- `PANE_BLOCKED_BY_PROMPT` - an approval/trust dialog is up; dispatch refused (`pane_prompt::dispatch_blocker`).
- `PROMPT_NOT_SUBMITTED` - typed prompt never started a turn - `agent_runs/headless/`, `native.rs`, `bridge/`.
- `PROMPT_NOT_DELIVERED` - OpenCode API ack failed after one resend - `opencode/delivery.rs` (recoverable).
- `STALE_PROMPT`, `PROMPT_NOT_PENDING`, `PROMPT_UNAVAILABLE`, `INVALID_PROMPT_CHOICE`, `PROMPT_REPLY_FAILED`, `BROAD_APPROVAL_REQUIRES_OPT_IN` - `answer_prompt` path - `pane_prompt/`, `agent_runs/routes/`.
- `STEER_RECEIPT_NOT_FOUND` - `agent_runs/routes/`.
- `RESULT_NOT_CAPTURED` - pane idle but no reply text recoverable (recoverable) - `agent_runs/headless/`.
- `NO_REPLY_TEXT` - warning: a visible turn with checks finished with no reply text on the pane - `runtime/receipt.rs` const, added in `headless/`.
- `RESULT_TRUNCATED` - warning: reply longer than the screen and its start not in scrollback - `runtime/receipt.rs` const, added in `headless/`.
- `UNVERIFIED_RESULT` - `agent_runs/headless/` (headless result not backed by authoritative evidence).
- `LAUNCH_FAILED`, `THREAD_START_FAILED`, `TIMEOUT`, `TRANSCRIPT_READ_FAILED` - headless launch/run - `agent_runs/headless/`, `agent_runs/runtime/`.
- `PANE_*`/`SHELL_PANE_REQUIRED`, `COMMAND_TIMEOUT` - `shell_exec.rs`.

**OpenCode native**
- `OPENCODE_SERVER_UNREACHABLE` - `opencode serve` unreachable past the grace period; cancel/close still finalise runs - `agent_runs/native.rs`, `runtime.rs`.
- `NATIVE_AGENT_ERROR`, `NATIVE_STATUS_UNAVAILABLE`, `APPROVAL_WAIT_FAILED` - `native.rs`, `bridge/`.

**Shim / transport** (`bin/puppet_master_mcp/`)
- `WAIT_TIMEOUT` - foreground wait ended with the turn still running; response includes `watch_command`; retry with the same idempotency key.
- `BRIDGE_UNAVAILABLE` - no bridge/port file; `BRIDGE_ERROR` - bridge returned an unparseable/failed reply; `SCHEMA_DRIFT` - shim vs bridge catalog mismatch; `REQUEST_CANCELLED`, `RATE_LIMITED`, `NOT_FOUND`, `CONFLICT` - HTTP-status-derived fallbacks.
- `STATE_UNAVAILABLE` - read models unavailable - `bridge/`.

---

## 8. Not mapped confidently / known gaps

- Memory notes mention a detached daemon (`src/bin/puppet-master-daemon.rs`, `pty/event_sink.rs`,
  `bridge_host.rs`, `daemon_runtime.rs`, `docs/detached-terminals-plan.md`); none of these exist on
  this branch (`Cargo.toml` has only the `puppet-master-mcp` bin). Treat that work as not present here.
- The exact route table (`bridge/router.rs::route` plus the `handle` of each `bridge/routes/*.rs`) is
  partly pattern-matched by segment slices; the list in section 3.1 is a summary, grep `segments ==`
  for the authoritative set.
- `agent_adapters/*` vs `tui/*` overlap in intent (both feed pane-state hints); `agent_adapters` is
  the older classifier-prior path.
- Frontend (`packages/app/src`) is only sketched; it mirrors Rust enums in `lib/tauri.ts` and
  `packages/shared/src/protocol.ts`.
- Declarative agents: env/stdin launches, `[mcp_host]` route/UI, `extract_model` parity and a recorder bridge route are not implemented (see `FEATURES.md` section 8 and `TEST-PLAN.md` section 4).
