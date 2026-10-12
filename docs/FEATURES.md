# Puppet Master feature list

User-facing features as they exist on branch `refactor/tui-profiles` (verified against the code by grep/read on
2026-10-12; "unverified" marks anything not confirmed in code or a live run). Status: **stable** (tested, used),
**experimental** (works, flagged in UI/receipts), **gap** (known limitation). Orientation map: [`REPO_MAP.md`](REPO_MAP.md).
What to test: [`TEST-PLAN.md`](TEST-PLAN.md). Format spec: [`specs/agent-definitions/`](specs/agent-definitions/).

Paths are relative to `packages/app/src-tauri/src` (Rust) or `packages/app/src` (UI) unless they start with `docs/`, `scripts/`, `packages/`.

## 1. Desktop app and workspace

| Feature | Where | Status |
|---|---|---|
| Tauri 2 desktop app hosting real PTY panes (portable-pty + vt100) in a terminal grid | `pty/`, UI `components/TerminalGrid.tsx`, `TerminalPane.tsx` | stable |
| Loopback HTTP bridge (default 127.0.0.1:17321, scans upward) shared by GUI and MCP | `bridge/` | stable |
| Bridge port file; removed on exit only if it still names our port | `bridge/port_file.rs` | stable |
| Single-instance guard: a second launch focuses the existing main window and exits | `lib.rs` (`tauri_plugin_single_instance`) | stable (live test in TEST-PLAN) |
| Settings tabs: General, Appearance, Agents, Orchestrator, MCP, Mobile, Session, Storage, API, Advanced, Performance, Developer | `components/settings/tabs/*Tab.tsx` | stable |
| Pane exit is detected by an exit watcher as well as ConPTY EOF, so a dead pane closes at once | `pty/registry/spawn.rs` (exit watcher) | stable |
| Detached pane pop-out windows: one per pane (concurrent opens deduped per label), unfocused, opened near the main window, close when the pane exits; backend honours X on an orphan window and sweeps stragglers | `pane_windows.rs`, UI `lib/detached-pane-window.ts`, `lib/detached-close.ts`, `TerminalApp.tsx`, `App.tsx` (`pane://detach` listener) | stable |
| Dev window opens on the secondary (left) monitor, unfocused (`tauri.dev.local.json`: x=-1660, y=60, focus=false) | `packages/app/src-tauri/tauri.dev.local.json`, `scripts/run-tauri-dev.ps1` | stable (machine-specific coordinates) |
| Mobile PWA shell and pairing/tunnel (cloudflared) | UI `PwaApp.tsx`, `components/pwa/`, `MobilePairingPanel.tsx`; `mobile_pairing/`, `mobile_tunnel.rs`, `pwa_server.rs` | present; not re-verified on this branch |
| Home dashboard with an Agents overview card | UI `components/HomeDashboard.tsx`, `AgentsOverviewCard.tsx` | stable |

## 2. Orchestrator sidebar and coordination

| Feature | Where | Status |
|---|---|---|
| Orchestrator backend selection (API vs CLI), OpenCode key profiles a/b | UI `OrchestratorTab.tsx`, `OrchestratorBackendSelect.tsx`; `opencode/` | stable (see `ROUTING.md`) |
| Coordination panel: tasks, locks, pane roles, digests, session context | UI `components/CoordinationPanel.tsx`; `session_context.rs`, `bridge/routes/tasks.rs`, `locks.rs` | stable |
| Operations hub: revisioned, restart-surviving run snapshots under `<project>/.puppet-master/operations/` | `operations.rs`, `operations/` | stable |
| Standby pool (opt-in warm workers: hidden visible Cursor/Claude panes, headless Claude stdin stream); bridge-owned, polling policy via `read/update_orchestrator_state` | `standby/` (`pool.rs`, `config.rs`, `host.rs`), `bridge/routes/standby.rs` | experimental (off by default; Cursor headless cannot be pre-launched) |

## 3. MCP server and tools

The stdio shim `puppet-master-mcp` (`bin/puppet-master-mcp.rs`, `bin/puppet_master_mcp/`) advertises the catalog in `tool_registry/catalog/` and
maps each tool to a bridge route; `catalog_version` is pinned (`66-4cd8e0886d0f544a`). `set_mode` limits a connection to agent, shell or both.
Port discovery: the shim reads the port file and, if that port is dead, probes 17321-17330.

**Agent mode (`catalog/agents.rs`)**
- `run_agent` - start or adopt a worker with a task, returns a handle; options `headless`, `read_only`, `keep_pane`, `checks`, `locks`, `model`, `context_policy`, and **`placement` = `auto|grid|window`**: visible panes open in the main window's left grid by default; `window` forces one pop-out; auto/grid fall back to a pop-out when the main window is closed/hidden; ignored when headless. `placement` is also on `delegate_work` and `spawn_agent`.
- `quick_ask` - one-shot read-only headless question, answer plus receipt.
- `wait_agents` - wait on handles (completed/failed/cancelled/needs_input). `send_message` - steer the current turn. `followup_task` - next turn on the same worker/conversation. `send_agent` - compat wrapper for followup_task.
- `answer_prompt` - answer a blocking approval prompt (handle or pane_id). `interrupt_agent` / `cancel_agent` - stop the turn, keep the handle. `close_agent` - end the session.
- `list_agents` / `list_workers` - adoptable workers and live UI panes. `inspect_agent`, `agent_transcript` - status/context and typed transcript.
- `take_over` / `release` - temporary manual pane control. `shell_exec` - run a command in the project dir.
- `session_identity`, `attach_agents`, `release_lease`, `transfer_agent` - coordinator identity, reconnect, lease and ownership.
- `set_mode` - pick the catalog (agent/shell/both).

**Delegation and operations (`catalog/operations.rs`)**: `delegate_task` (validate + render prompt), `delegate_work` (async operation), `get_operation`, `wait_for_operation`, `cancel_operation`.

**Panes (`catalog/panes.rs`)**: `list_panes`, `bridge_health` (also env probe + schema drift), `list_agent_contexts`, `read_agent_context`, `inspect_agent_model`, `switch_agent_model` (opencode_native), `spawn_agent` (empty pane), `read_terminal_buffer`, `write_terminal_input`, `press_key`, `kill_pane_process`.

**Waits (`catalog/waits.rs`)**: `wait_for_panes`, `wait_for_model`, `wait_for_task`, `wait_for_worker`, `read_recent_events`.

**Tasks and locks (`catalog/tasks.rs`)**: `create_task`, `claim_task`, `report_task_status`, `complete_task`, `list_tasks`, `acquire_resource_lock`, `release_resource_lock`.

**Context (`catalog/context.rs`)**: `read_librarian_prompt`, `read_project_ir_status`, `build_context_pack`, `read_session_context`, `update_session_context`, `set_pane_role`, `read_pane_digest`, `update_pane_digest`.

**OpenCode (`catalog/opencode_reply.rs`, `opencode_status.rs`)**: `reply_opencode_permission`, `reply_opencode_question`, `read_opencode_key_status`, `rotate_opencode_key`, `read_opencode_worker_status`, `read_opencode_messages`.

**Orchestrator state (`catalog/orchestrator.rs`)**: `read_orchestrator_state`, `update_orchestrator_state` (standby polling).

`agent_type` in every tool accepts any registered id (pattern `^[a-z][a-z0-9_]{1,31}$`); an unregistered id gives HTTP 400 `UNKNOWN_AGENT_TYPE` with `context.valid`.
Status: stable. The `puppet-master watch` CLI (`packages/cli`) blocks a shell on a background run.

## 4. Agents

### 4.1 Built-in code profiles (nine)
`tui/registry.rs` seeds them; each implements `TuiProfile` (`tui/profile.rs`): `claude`, `codex`, `opencode`, `opencode_native`, `cursor_agent`, `cursor` (IDE), `cmd`, `powershell`, `bash`. Per-agent code in `tui/{claude,codex,opencode,cursor,shell}/`; generic fallback `tui/generic.rs`; rules every profile must pass in `tui/conformance.rs`. Status: stable. Generic code never matches agent id strings (see `tui/README.md`).

### 4.2 Declarative TOML agents (`tui/declarative/`)

| Feature | Where | Status |
|---|---|---|
| Definition schema + validator (path-carrying errors, interpreter denylist, prompt-key check, `[mcp_host]` check) | `schema.rs`, `validate.rs` | stable |
| Argv/stdin templating: the task is always one argv element (injection-proof) | `template.rs` | stable; env set/unset and stdin-task definitions are not wired (LaunchSpec has no env/stdin channel) = gap |
| Binary resolution (PATH scan excluding cwd, npm shim to native exe, pinned binary) | `resolve.rs` | stable |
| Selectors + output parsers (JSON lines, result extraction) | `select.rs`, `output.rs` | stable; `extract_model` parity with code profiles is a gap |
| `[tui]` screen engine for visible runs (state tables, delivery, `[tui.layout]` sidebar crop) | `screen.rs` | experimental (visible results are screen-inferred and flagged) |
| `DeclarativeProfile`: one `TuiProfile` per definition (strings interned via `Box::leak`) | `profile.rs` | stable (small leak per edit) |
| Loader, source precedence (project > extra > user > bundled), `sha256:` fingerprints | `loader.rs`, `sources.rs`, `fingerprint.rs` | stable |
| Approvals store (fingerprint + pinned binary + argv snapshot); states `needs_approval` / `approval_stale` / `active` | `approvals.rs`, `service.rs` | stable |
| Spawn gate: re-checks fingerprint, approval, binary before every run | `registry::admit`, `TuiProfile::spawn_gate` | stable |
| Fixture checks (`*.expect.toml`) | `check.rs`, `tests/expect.rs` | stable |
| Hot-reload watcher (notify, 150 ms debounce, keeps the previous registry if an active file turns invalid, SSE + Tauri `agent_definitions_changed`) | `watch.rs` | stable (gap: an invalid active file skips the whole refresh, retried) |
| Zero-config generic agents (`kind = "generic"`, experimental flag) | `loader.rs` `write_generic`, Tauri `agents_add_generic` | experimental |
| Export/import one definition (stored zip; imports land UNAPPROVED; no network) | `loader::{export,import}_definition`, Tauri `agents_export_definition` / `agents_import_definition` | stable |
| Recorder: Rec button in a pane header records a session and generates a redacted draft in `<agents dir>/drafts/` (drafts are never loaded or approved) | `agent_definitions_recorder/` (`mod.rs`, `snapshots.rs`, `draft.rs`, `redact.rs`), Tauri `recorder_*` in `commands/agents.rs`, UI `TerminalPane.tsx` | experimental (no bridge route; chrome candidates noisy) |
| `[mcp_host]` writer (JSON/TOML/YAML key set, `.bak`, home/project paths only, idempotent, approved-only) | `mcp_install/declarative.rs` | library-only, no route/UI = gap |
| Bundled definitions | `loader.rs` (`bundled` is empty) | none shipped; `packages/app/src-tauri/tests/agent-definitions/kilo.toml` is the proof copy installed into the user dir |

**Kilo** (`kilo.toml`, kilo CLI 7.x, headless + visible) is the proof agent. **Kiro** (`kiro.toml`, kiro-cli stream-json, headless, needs `kiro-cli login`) exists as a user-dir definition on the dev machine (`<app data>/agents/`); it is not committed in the repo, and was not run live in this documentation pass (unverified).

### 4.3 Settings > Agents (`components/settings/tabs/AgentsTab.tsx`, `lib/agentDefinitions.ts`)
Definition rows with state (needs approval / active / approval stale / invalid) and an **experimental badge**; Review & approve dialog (rendered argv, binary, warnings, diff vs approved); Revoke; **Check with fixtures** (Kilo: 9 fixtures); Add as generic agent; Import / Export; Rec drafts list; Sources list with per-path consent switches (project folders, extra directories) and a consent banner; toasts including the no-restart "registry updated" notice. Live reload without restart via `lib/agentRegistry.ts` (`onAgentDefinitionsChanged`). Status: stable except as noted.

### 4.4 Spawn menu and Overview
The New-session spawn menu is built from the registry (`lib/agentRegistry.ts`); headless-only and experimental agents are listed but disabled with a reason. The Overview Agents card (`AgentsOverviewCard.tsx`) lists registered agents and updates live. Status: stable.

## 5. Run engine (`agent_runs/`)

| Feature | Where | Status |
|---|---|---|
| Headless (default) vs visible (`headless=false`) runs; the visible pane lands in the main-window grid (wide tile for run panes) | `agent_runs/headless/entry.rs`, `pane_windows::decide_placement` | stable |
| Receipts: `agent_definition` {id, fingerprint, binary}, `read_only` (enforced/...), `experimental`, authoritative vs inferred result | `agent_runs/`, `RunReceipt` | stable |
| `read_only=true` per profile capability (`READ_ONLY_UNSUPPORTED` otherwise) | `tui` `features()` | stable |
| `followup_task` / resume on the same provider session (`continuity: resume`) | `agent_runs/runtime/followup.rs` | stable |
| `answer_prompt` for approval prompts in visible panes | `bridge/routes`, `pane_prompt/` | stable |
| `keep_pane` (default false: a run-created pane closes after the run; `PANE_CLOSED_AFTER_RUN`) | `agent_runs/` | stable |
| In-flight runs keep the profile they started under; editing a definition mid-run does not lose the result | `agent_runs` + `registry` (T36b) | stable |
| Refusals `AGENT_DEF_*` (e.g. `AGENT_DEF_APPROVAL_STALE`), `UNKNOWN_AGENT_TYPE`, `AGENT_REPORTED_ERROR`, `OUTPUT_UNPARSEABLE` | `registry::admit`, `headless/finish.rs`, `tui::require_registered` | stable |
| Visible prompt delivery waits up to 20 s for a slow TUI to draw; blank screens do not burn typing attempts | `pane_prompt/` | stable |
| Idempotency keys, stable handles per key, acceptance `checks`, resource `locks` | `operations/`, `persist` | stable |

## 6. Security
- Definition mutations (approve, revoke, consent, import, reload, add generic) are local-app-only: loopback, no `X-PM-Proxied`/bearer/forwarding headers; an MCP session gets 403 `AUTHORIZATION_DENIED`; tunnel/proxy gets 401/403 (`bridge/routes/definitions.rs`, `mcp_sessions/authorize.rs`).
- Per-path consent for project and extra directories (off by default; consent is not approval): `sources.rs`, `agent-definitions.json`.
- Interpreter denylist (python, cmd, etc. rejected as the binary): `validate.rs`. Argv templating cannot be injected: `template.rs`.
- Approval pins fingerprint and binary; changing either stales the approval.

## 7. Scripts
`scripts/pm-regression.mjs` (+`scripts/pm-regression/`: live regression + bench, 28 scenario runs), `scripts/pm-regression/readonly-check.mjs` (read-only enforcement parity), `scripts/check-window-monitor.ps1` (fails if any Puppet Master window is on the primary monitor), `scripts/perf-latency.mjs` / `perf-summarize.mjs` / `perf-frames.mjs` / `perf-compare.mjs` (latency baseline, `docs/perf/`), `scripts/with-cargo-path.mjs` (cargo on PATH for `tauri dev`), `scripts/build-rust-mcp.mjs`, `scripts/test-mcp-operations.mjs`, `scripts/test-mcp-package.mjs`.

## 8. Known gaps (summary)
Env/stdin in declarative launches; `[mcp_host]` has no route; SPEC Q6 cost/concurrency note and Q7 second proof agent unscheduled; T29 skipped; `extract_model` parity; recorder has no bridge route; visible declarative results are inferred; remote/tunnel behaviour only tested with headers on loopback.
