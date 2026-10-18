# Puppet Master feature list

User-facing features as they exist on branch `refactor/tui-profiles` (verified against the code by grep/read on
2026-10-12; "unverified" marks anything not confirmed in code or a live run). Status: **stable** (tested, used),
**experimental** (works, flagged in UI/receipts), **gap** (known limitation). Orientation map: [`REPO_MAP.md`](REPO_MAP.md).
What to test: [`TEST-PLAN.md`](TEST-PLAN.md). Format spec: [`specs/agent-definitions/`](specs/agent-definitions/).

Paths are relative to `packages/app/src-tauri/src` (Rust) or `packages/app/src` (UI) unless they start with `docs/`, `scripts/`, `packages/`.

## 0. Panes-first product model (PF1-PF3, decided 2026-10-12; spec `specs/panes-first/SPEC.md`)

- **There is no main window.** The app is tray-only: it starts with no webview (`tauri.conf.json` `windows = []`). Everything the user sees is a **tray icon**, a **Settings window** and **one OS window per pane**.
- **Tray icon** (`tray.rs`): menu entries `New pane` (submenu of registered agents; headless-only ones disabled and marked), `Panes` (open panes), `Settings...`, `Quit`. **Left-click opens Settings.** The menu model is unit-tested; clicking the real tray by hand is unverified.
- **Settings** (Orchestration: inline in the cockpit, `components/cockpit/SettingsView.tsx`; Individual: the Settings window, `app_windows.rs`, UI `SettingsApp.tsx` via `?settings=1&tab=`): the existing Settings tabs, opening on **Agents**. A **second launch of the app opens Settings** (single-instance callback). `--new-pane <agent>` on the command line opens a pane of that agent.
- **Visible first**: `run_agent` / `delegate_work` / `spawn_agent` default to `headless=false`; each run opens its own pane window. **Headless fallback** when the agent has no visible mode (`no_visible_mode`, e.g. Kiro) or the pane fails to launch / take the prompt (`visible_failed`); recorded as `receipt.fallback = {from:"visible", to:"headless", reason}`.
- **Pane windows are created from Rust** (`app_windows.rs`, `pane_windows.rs`), unfocused when an agent opens them. Placement is anchor + cascade. Anchor priority: `PM_WINDOW_ANCHOR` env (dev; set from gitignored `packages/app/src-tauri/pm-dev.local.json` by `scripts/with-cargo-path.mjs`) > `settings.json` `pane_window_position` > primary monitor. Each new pane is offset from the previous one and wraps.
- **Closing a pane window kills that pane** (no reattach to a grid).
- **Removed from the user's path (code kept, unreachable):** main window, terminal grid, Overview/Home dashboard, in-app orchestrator sidebar, the New-session spawn menu, the `App.tsx` `pane://detach` listener. The `placement` argument is still accepted by the tools for compatibility and **ignored** (always a window).

## 1. Desktop app and workspace

Rows marked **obsolete** describe the legacy main-window UI: still in the tree, no longer reachable.

| Feature | Where | Status |
|---|---|---|
| Tauri 2 tray app hosting real PTY panes (portable-pty + vt100), one window per pane | `pty/`, UI `TerminalApp.tsx`, `TerminalPane.tsx`; `tray.rs`, `app_windows.rs` | stable |
| Terminal grid in a main window | UI `components/TerminalGrid.tsx`, `App.tsx` | **obsolete** (unreachable since PF3) |
| Loopback HTTP bridge (default 127.0.0.1:17321, scans upward) shared by GUI and MCP | `bridge/` | stable |
| Bridge port file; removed on exit only if it still names our port | `bridge/port_file.rs` | stable |
| Single-instance guard: a second launch opens the Settings window (and handles `--new-pane <agent>`) and exits | `lib.rs` (`tauri_plugin_single_instance`) | stable (live-proven PF2/PF3) |
| Settings: six sections (General, Agents, Orchestrator & Models, Keys, Bridge & MCP, Phone) plus a read-only "Coming later" page; every control autosaves through `settings_patch`; row search. See `docs/features/settings-redesign.md` | `components/SettingsPanel.tsx`, `components/settings/sections/*Section.tsx` | stable |
| Pane exit is detected by an exit watcher as well as ConPTY EOF, so a dead pane closes at once | `pty/registry/spawn.rs` (exit watcher) | stable |
| Pane windows: one per pane, created from Rust (no main webview needed), unfocused for agent runs, anchor + cascade placement, close when the pane exits; closing the window kills the pane; backend sweeps stragglers | `pane_windows.rs`, `app_windows.rs`, UI `lib/detached-close.ts`, `TerminalApp.tsx` | stable |
| Pop-out opened through the main window's `pane://detach` listener | UI `lib/detached-pane-window.ts`, `App.tsx` | **obsolete** (replaced by Rust-created windows) |
| Tray icon + menu (New pane / Panes / Settings... / Quit), left-click opens Settings | `tray.rs` | stable (menu model tested; real tray clicking not human-verified) |
| Standalone Settings window on the Agents tab | `app_windows.rs`, UI `SettingsApp.tsx`, `lib/settings-entry.ts` | stable |
| Window anchor for pane windows: `PM_WINDOW_ANCHOR` > `settings.json` `pane_window_position` > primary monitor, then cascade | `app_windows.rs`, `settings_store.rs`, `scripts/with-cargo-path.mjs` | stable |
| Dev windows (Settings, panes) open on the secondary (left) monitor via the anchor in gitignored `packages/app/src-tauri/pm-dev.local.json` -> `PM_WINDOW_ANCHOR` (`tauri.dev.local.json` is the legacy main-window override) | `scripts/with-cargo-path.mjs`, `app_windows.rs` | stable (machine-specific coordinates) |
| Mobile PWA: agent tabs ("All" overview with status, project, last output age and last screen lines; one tab per pane with the live terminal and input box; "+" opens any registered agent on the PC, menu closes the current one; no orchestrator control) | UI `PwaApp.tsx`, `components/mobile/`, `lib/mobile-agent-tabs.ts`; bridge `GET /agents/types` (sessionless read) | verified on an Android emulator through the Cloudflare tunnel and in headless Chrome as an iPhone |
| Live output to remote phones: loopback uses SSE (`/events`); any remote host long-polls `GET /events/poll?cursor=&wait_ms=` (Cloudflare quick tunnels never pass SSE through); a missed stretch (`gap`) re-reads the pane list and replays the terminal | `bridge/event_log.rs`, `bridge/http.rs`, `lib/bridge-sse.ts` (`subscribeBridgeEventsViaPoll`) | verified through the tunnel (PC typing shows on the phone in about 1 s) |
| Phone-sized panes: while a phone shows an agent's tab the pane is resized to the phone's grid and the agent redraws for it; the terminal resizes in place (keyboard and stream survive), sized from cell arithmetic; no local echo, so the phone shows exactly what the agent drew | `pty/viewer_size.rs`, bridge `POST /panes/{id}/viewport`, `hooks/usePaneViewportLease.ts`, `TerminalSession.measureFitGrid` | verified (emulator 56x56 -> 56x32 with the keyboard up, typing/erasing identical to the PC) |
| Several phones on one pane share the smallest grid (per-viewer leases, tmux-style); a leaving or silent phone (15 s) hands the pane to the others, the last one restores the desktop size | `pty/viewer_size.rs` (`Leases`), viewer id in `usePaneViewportLease.ts` | verified against the bridge (56x56 + 48x21 -> 48x21, no flipping) |
| Keyboard: the shell follows the visual viewport (tab strip stays, terminal gets shorter, input above the keyboard); Android uses `interactive-widget=resizes-content`; the input guard sees an Android keyboard close as the page getting shorter and a tap re-shows a hidden keyboard | `PwaApp.tsx` (`.pwa-shell--fit-visual`), `terminal/mobile-input-guard.ts` (`reserveKeyboardSpace: false`, `keyboardObscuredPx`) | verified on the emulator; iPhone only in emulation |
| PC pop-out while a phone holds the pane: "Sized for your phone" notice instead of a mismatched grid; desktop and phone resizes notify the windows | `PaneInfo.viewer_held`, `components/TerminalViewport.tsx`, `bridge::notify_pane_resized` | verified on screen |
| Legacy mobile PWA (orchestrator chat / Panes), kept as a backup at `?pwa=legacy` | `PwaLegacyApp.tsx`, `components/pwa/` | backup; waits for an orchestrator pane nothing starts since PF3 |
| Mobile pairing/tunnel (cloudflared quick tunnel, new URL on every app start) | `MobilePairingPanel.tsx`; `mobile_pairing/`, `mobile_tunnel.rs`, `pwa_server.rs` | in use (paired Android phone and emulator); a paired phone may type into panes a `run_agent` manages |
| Home dashboard with an Agents overview card | UI `components/HomeDashboard.tsx`, `AgentsOverviewCard.tsx` | **obsolete** (Overview unreachable) |

## 2. Orchestrator sidebar and coordination

The in-app orchestrator sidebar and Coordination panel UI are **obsolete** (main window removed); the backend (operations, tasks, locks, standby) is unchanged and used through MCP.

| Feature | Where | Status |
|---|---|---|
| Orchestrator backend selection (API vs CLI), OpenCode key profiles a/b | UI `OrchestratorTab.tsx`, `OrchestratorBackendSelect.tsx`; `opencode/` | stable (see `ROUTING.md`) |
| Coordination panel: tasks, locks, pane roles, digests, session context | UI `components/CoordinationPanel.tsx`; `session_context.rs`, `bridge/routes/tasks.rs`, `locks.rs` | stable |
| Operations hub: revisioned, restart-surviving run snapshots under `<project>/.puppet-master/operations/` | `operations.rs`, `operations/` | stable |
| **Preload** (Settings > Agents > Keep agents ready; replaces the Performance/standby UI): master switch + per-provider checklist (Claude Code, Cursor Agent, Kilo; others listed greyed until they declare a standby mode), keep 0-2 warm each in the default project folder (the "project folder" setting), idle timeout in minutes; each warm worker is the CLI at its input box in a hidden pane **plus a pre-built hidden pane window** (`warm=1`), so tray "New pane" / a visible run only shows it (tray marks warm agents "(ready)"); a replacement warms after every claim; old `standby-pool.json` visible toggles migrate into it | `standby/` (`config.rs` PreloadConfig, `pool.rs`, `host.rs`), `app_windows.rs` (`open_warm_pane_window`), `tray.rs`, `components/settings/blocks/PreloadSettings.tsx` | experimental (off by default) |
| Standby pool (the engine under Preload; also opt-in warm headless Claude stdin stream by hand-edited `backends[..].headless`); bridge-owned, polling policy via `read/update_orchestrator_state` bridge-owned, polling policy via `read/update_orchestrator_state` | `standby/` (`pool.rs`, `config.rs`, `host.rs`), `bridge/routes/standby.rs` | experimental (off by default; Cursor headless cannot be pre-launched) |

## 3. MCP server and tools

The stdio shim `puppet-master-mcp` (`bin/puppet-master-mcp.rs`, `mcp_shim/`) advertises the catalog in `tool_registry/catalog/` and
maps each tool to a bridge route; `catalog_version` is pinned (`72-ffde8e7c2c763567`, 72 tools; the four Team tools are advanced-tier). `set_mode` limits a connection to agent, shell or both.
Port discovery: the shim reads the port file and, if that port is dead, probes 17321-17330.

**Agent mode (`catalog/agents.rs`)**
- `run_agent` - start or adopt a worker with a task, returns a handle; options `headless`, `read_only`, `keep_pane`, `checks`, `locks`, `model`, `context_policy`, and **`placement` = `auto|grid|window`**: runs are visible by default (`headless=false`) and the pane opens in its own pop-out window (`placement` default `window`); `grid`/`auto` are accepted but **ignored** since PF3 (no main window, always a window); `placement` is ignored when headless. `headless=true` is the explicit opt-out and the automatic backup: a run falls back to headless once, recorded as `fallback: {from: "visible", to: "headless", reason}` on the view and the receipt, when the agent has no visible mode (`no_visible_mode`, e.g. Kiro) or the pane fails to launch / take the prompt before the turn starts (`visible_failed`; the failed pane is closed, the same operation re-queued headless). `placement` is also on `delegate_work` and `spawn_agent`.
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
| Screen-read agents (visible, no native protocol): the id of a card is the question's (a spinner or counter under it does not change it); a run's reply is what follows THIS turn's task echo minus chrome / `[tui.thinking]` / tool rows; `resolved_model` from the screen, an earlier look or the scrollback; a menu no `[[tui.prompt]]` describes becomes an `unknown_question` card ("recognised by shape", options as shown, picked by `answer_prompt`) | `pane_prompt/` (`mod.rs`, `generic_menu.rs`), `agent_runs/completion.rs`, `agent_runs/headless/supervise.rs`, `tui/declarative/screen.rs` | stable |
| Agent Wizard, screen-read extras: clean-reply, model and resize checks in the visible test; the capture measures `reprints_on_resize` and writes the flag; two permission captures (`tui_permission` file, `tui_permission_command` command, the latter checked in a second test pane) | `tui/declarative/wizard/` (`visible/capture.rs`, `visible/livetest.rs`, `features.rs`), SPEC 11.17 | stable |
| Bundled definitions | `loader.rs` (`bundled` is empty) | none shipped; `packages/app/src-tauri/tests/agent-definitions/kilo.toml` is the proof copy installed into the user dir |

**Kilo** (`kilo.toml`, kilo CLI 7.x, headless + visible) is the proof agent. **Kiro** (`kiro.toml`, kiro-cli stream-json, headless, needs `kiro-cli login`) is committed as `packages/app/src-tauri/tests/agent-definitions/kiro.toml` (install by importing it in Settings > Agents). Proven live on 2026-10-11/12: imported, reviewed and approved in the UI (customer demo video), and headless runs returned verified answers from the in-app orchestrator and from Codex as an external MCP client.

### 4.3 Settings > Agents (tray > Settings window; `components/settings/blocks/AgentDefinitions.tsx`, `lib/agentDefinitions.ts`)
Definition rows with state (needs approval / active / approval stale / invalid) and an **experimental badge**; Review & approve dialog (rendered argv, binary, warnings, diff vs approved); Revoke; **Remove** (only for agents in your own agents folder: a confirm sheet, then the file, its `<id>/` fixtures folder, `<id>.wizard.json` and the approval move to `<agents dir>/.removed/<id>-<timestamp>/`, nothing is deleted; the registry reloads at once; an Undo toast and a "Recently removed" list with Restore put it back, refusing if the file name is taken again, and the approval only returns for a byte-identical file; project and extra-folder agents show "This agent comes from <folder>" instead; `POST /agents/definitions/remove|restore`, `GET /agents/definitions/removed`, `removal.rs`); **Check with fixtures** (Kilo: 9 fixtures); Add as generic agent; Import / Export; Rec drafts list; Sources list with per-path consent switches (project folders, extra directories) and a consent banner; toasts including the no-restart "registry updated" notice. Live reload without restart via `lib/agentRegistry.ts` (`onAgentDefinitionsChanged`). Status: stable except as noted.

### 4.4 Spawn menu and Overview
The tray **New pane** submenu is built from the registry (`tray.rs`; headless-only agents disabled and marked). The legacy New-session spawn menu and the Overview Agents card (`AgentsOverviewCard.tsx`) are **obsolete** (unreachable). Status: tray stable.

## 5. Run engine (`agent_runs/`)

| Feature | Where | Status |
|---|---|---|
| Visible-first runs (`headless=false` default, pop-out window per pane) with automatic headless fallback (`fallback` in view + receipt) | `agent_runs/headless/entry.rs`, `agent_runs/headless/fallback.rs`, `operations::convert_to_headless_fallback`, `pane_windows::decide_placement` (always a window since PF3) | stable |
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
- Definition mutations (approve, revoke, remove, restore, consent, import, reload, add generic) are local-app-only: loopback, no `X-PM-Proxied`/bearer/forwarding headers; an MCP session gets 403 `AUTHORIZATION_DENIED`; tunnel/proxy gets 401/403 (`bridge/routes/definitions.rs`, `mcp_sessions/authorize.rs`).
- Per-path consent for project and extra directories (off by default; consent is not approval): `sources.rs`, `agent-definitions.json`.
- Interpreter denylist (python, cmd, etc. rejected as the binary): `validate.rs`. Argv templating cannot be injected: `template.rs`.
- Approval pins fingerprint and binary; changing either stales the approval.

## 7. Scripts
`scripts/pm-regression.mjs` (+`scripts/pm-regression/`: live regression + bench, 28 scenario runs), `scripts/pm-regression/readonly-check.mjs` (read-only enforcement parity), `scripts/check-window-monitor.ps1` (fails if any Puppet Master window is on the primary monitor), `scripts/perf-latency.mjs` / `perf-summarize.mjs` / `perf-frames.mjs` / `perf-compare.mjs` (latency baseline, `docs/perf/`), `scripts/with-cargo-path.mjs` (cargo on PATH for `tauri dev`; reads `pm-dev.local.json` into `PM_WINDOW_ANCHOR`), `scripts/build-rust-mcp.mjs`, `scripts/test-mcp-operations.mjs`, `scripts/test-mcp-package.mjs`.

## 8. Known gaps (summary)
Tray icon/menu clicking not verified by a human (model unit-tested, tray creation logged); `visible_failed` fallback proven by unit tests only, not live; pm-regression `cursor_agent timeout` can fail `WORKER_STOP_FAILED` (known flaky headless process-tree kill). Older: Env/stdin in declarative launches; `[mcp_host]` has no route; SPEC Q6 cost/concurrency note and Q7 second proof agent unscheduled; T29 skipped; `extract_model` parity; recorder has no bridge route; visible declarative results are inferred; remote/tunnel behaviour only tested with headers on loopback.
