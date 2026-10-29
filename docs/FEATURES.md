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
| Crews (team templates): "Save this team" in the session rail's Helpers card turns a pane and its helpers into a crew (agent per role, first task with the folder as `{folder}`, redacted; edited in a sheet), saved as `<app data>/crews/<slug>.toml`; "Start crew…" in the palette starts a crew in a folder, lead first, each helper under its parent's pane, stopping at the first failure; any registered agent, wizard-added ones included; trust and permission questions of started panes are never answered | `crews/`, `commands/crews.rs`; UI `components/cockpit/crews/`, `lib/crews-api.ts` | experimental |
| Live Room: share the chat of 1 to 6 panes as a read-only link, end-to-end encrypted through a relay, secrets and folders hidden, stops by time, idle, Stop, panic or app quit; relays: the axum binary and the Cloudflare Worker | `live_room/`, `commands/live_room.rs`, `relay/viewer/`, `relay/worker/src/room.ts`; UI `components/cockpit/live/`, `lib/live-room.ts`; doc [`features/live-room.md`](features/live-room.md) | experimental (no live run on a relay yet) |
| Home dashboard with an Agents overview card | UI `components/HomeDashboard.tsx`, `AgentsOverviewCard.tsx` | **obsolete** (Overview unreachable) |

## 1a. Point and Fix (preview, pick, send)

Point and Fix lets you see your app inside Puppet Master, point at the part that is wrong, and say what should change. The message goes to the agent you choose. Where it lives: `point/` (Rust, see [`REPO_MAP.md`](REPO_MAP.md) 3.8), the Point box window (`components/point/`), and the preview windows (`preview-*`, no Tauri access for the page).

**Status: being built.** The design is in the feature spec; the pieces land on separate branches (`feat/point-*`) and are not on `main` yet. Nothing in this table is verified live until the live check in [`TEST-PLAN.md`](TEST-PLAN.md) section 2.8 is done.

| Feature | Where | Status |
|---|---|---|
| Open a dev server that the pane printed (Preview chip, a job's port chip, or the palette) in its own window; only `localhost`, `127.0.0.1` and `[::1]`; never Puppet Master's own ports | `point/origin.rs`, `commands/point.rs` (`point_open_preview`), `pane_preview.rs`, `pane_jobs/` | being built |
| Point (Alt+P): hover shows the element name, click picks it; selector, text, styles, component and source file when the dev build shows them (React, Svelte, Vue, Astro, data attributes) | `point/picker.js`, `point/pick.rs`, `point/source.rs` | being built |
| Screenshot of the picked element, cropped natively; saved in `<project>/.puppet-master/shots/` (git-excluded, newest 50 kept) | `point/capture.rs`, `point/shot.rs` | being built; **Windows only** in v1 (macOS and Linux get the text facts with no picture) |
| Point box: a cropped picture, "What should change?", up to 3 picks, Enter sends, Esc hides | `point/box_window.rs`, `components/point/PointBox*.tsx` | being built |
| Send to any agent pane through the pane outbox (waits until the agent is free; refused when the outbox refuses); the image is passed as a file path | `point/message.rs`, `point/target.rs`, `pane_outbox/` | being built |
| Edit in Chat (text goes into that pane's Chat composer as a draft) and Copy (for protocol panes and panes an agent run controls) | `point/session.rs`, `lib/composerDraft.ts`, `ChatComposer.tsx` | being built |
| Blocked links: any link or new window that leaves the local app is stopped, with a notice; **Open in browser** only on your click | `point/origin.rs` (`nav`), `point/window.rs` | being built |
| Privacy: the page never gets Tauri access; form values and password text are never read; page text goes through redaction; the page's words are marked as data in the message | `point/picker.js`, `point/pick.rs`, `point/message.rs`, `capabilities/default.json` | being built (tests pin each rule) |
| Not in v1: before/after strip, phone Point, image input per agent profile, source maps, preview inside the cockpit, macOS/Linux screenshots | spec section 3.9 | planned (v2) |

## 1b. Text your agents (Telegram) and Clip It

Limits and how to test: the feature pages.

| Feature | Where | Status |
|---|---|---|
| Text your agents (Telegram): pair one private chat, see what needs you, answer low-risk questions, send text to agents. Off by default. Telegram's servers can read the message text. | `channels/`, `commands/channels.rs`, `bridge/routes/channels.rs` ([`features/telegram.md`](features/telegram.md)) | **early**: the engine and the commands are in; the Settings block and the phone row are not on any screen yet; not run live with a real bot |
| Clip It: a 20 to 28 second video of one finished turn, made on the PC. Secrets and folder paths are hidden before drawing. You preview it, then save it to a file you pick. Nothing is uploaded. | `clip/`, `commands/clip.rs`, `lib/clip*.ts`, `components/chat/ClipSheet.tsx` ([`features/clip-it.md`](features/clip-it.md)) | **experimental**: unit tests and fakes only; no live run yet |

## 1c. Search across chats and old chats (Bring All My Chats)

Full page: [`features/bring-all-my-chats.md`](features/bring-all-my-chats.md).

| Feature | Where | Status |
|---|---|---|
| Ctrl+Shift+F search across chats that were open while the app ran (FTS5, redacted, newest 50 000 items) | `chat_index/`, `commands/session_chat.rs` (`chat_search`), `components/cockpit/Palette.tsx` | stable |
| "Bring my old chats": opt-in import of the chats each agent kept on this PC (Find my chats, per-agent switches, time window, progress, Stop, Look again) | `chat_index/import/`, `commands/chat_import.rs`, `components/history/BringChatsPanel.tsx` | experimental (fixtures only; no live run on this branch) |
| Claude Code old chats (`~/.claude/projects`, `CLAUDE_CONFIG_DIR`); sidechains skipped | `tui/claude/history.rs`, `session_chat/history/file.rs` | experimental |
| Codex old chats (`~/.codex/sessions`, `CODEX_HOME`; plain file preferred over `.zst`) | `tui/codex/history.rs` | experimental |
| OpenCode old chats from its SQLite database (read-only; parent sessions only) | `tui/opencode/history.rs` | experimental; table names from the installed OpenCode 1.19.0 binary, not a live database |
| TOML agents bring old chats with `[session_chat.history]` (off by default until switched on) | `session_chat/declared/history*.rs`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md` 6.1 | experimental |
| Old chat hits in search carry the "Old chat" tag; choosing one opens a read-only view at the message | `components/cockpit/ChatHits.tsx`, `components/history/ImportedChatSheet.tsx` | experimental |
| "Continue in a pane" for an old chat: starts the agent with its own resume option (Claude, Codex, OpenCode, TOML agents that declare one); nothing is typed | `session_chat/history/open.rs` (`resume_plan`), `components/history/ImportHost.tsx` | experimental |
| Remove imported chats (Settings > General and the panel); "Clear search index" removes them too | `commands/chat_import.rs` (`chat_import_clear`), `components/settings/blocks/BringChatsSettings.tsx` | stable |
| Old chats are never in the companion's recall, MCP, team or bridge answers (companion uses `search_in`) | `chat_index/store.rs` (`search_in`), guard tests in `session_chat/history/guard.rs` | stable |
| Phone search (`GET /chats/search`) never shows old chats (`search_live`, `Reach::LiveOnly`) | `bridge/routes/chats.rs`, `chat_index/store.rs` | done |
| First-start card "Read my old chats on this PC" | branch `feat/bc-card` (BC-13), not merged into this branch | **gap** (not in this branch) |

## 1d. Scores on your code

Limits and how to test: the feature page.

| Feature | Where | Status |
|---|---|---|
| Scores on your code: which agent works best in one project, from this PC's own history. Plain counts per agent and account (turns, ended OK, checks green, merged, undone, per merged change, asked you). Small samples show `-` or "early"; no ranking, no overall score. Undone stays empty until Undo writes the undo log. | `scores/`, `commands/scores.rs`, `components/cockpit/AgentScores.tsx`, `lib/agent-scores.ts` ([`features/agent-scores.md`](features/agent-scores.md)) | **experimental**: unit tests only; no live run yet |
| Pick for me: the first row of the "+" menu suggests one agent for this project, from your own Scores (last 90 days), and says why in one plain sentence. Says "too early" and uses your usual agent when there is little history. Nothing starts until you press Start; nothing you type is read or sent; nothing is stored. | `scores/pick/`, `commands/scores.rs` (`pick_agent`), `components/cockpit/NewAgentMenu.tsx`, `lib/pick-for-me.ts` ([`features/pick-for-me.md`](features/pick-for-me.md)) | **experimental**: unit tests only; no live run yet; desktop app only |

## 1e. Night Shift (Preview)

Full page: [`features/night-shift.md`](features/night-shift.md). Off by default (Settings > General > Labs > Night Shift).

| Feature | Where | Status |
|---|---|---|
| Tonight sheet: up to 10 tasks (project, words, agent, the project's declared tests, turns, minutes), the plans to use (one login per company), a stop time, start now or at a time, 1 or 2 at once, keep the computer awake (asked once) | `components/cockpit/night/` (`TonightSheet`, `TestsPopover`, `PlansPicker`), `commands/night.rs` + `commands/night/start.rs` | experimental |
| The runner: each task is a mission in its own separate copy (or one turn when no tests are declared); one task per plan and per project; no new try in the last 25 minutes; a usage limit waits 3 minutes, then the task goes once to a plan of another company or waits for the reset; a stop time cuts running tasks off and keeps their copies | `night_shift/` (`runner`, `plan`, `io_live`, `checks`) | experimental: runs end to end with fake agents (`tests/fake_tui_night.rs`); no live night with real agents yet |
| Never answers a trust, permission or sign-in screen and never merges: a gate ends the try as `Needs a look`, the pane stays open; Keep and Throw away are desktop commands only | `night_shift/` (no write path into a pane), `night_shift/review.rs` | experimental (flight-log test: no `gate_answer`) |
| Sidebar chip: running, waiting, scheduled, needs you, finished; Stop | `components/cockpit/night/NightChip.tsx` | experimental |
| Morning page: summary line, groups (ready, needs a look, didn't run), per-task detail, Keep (merge into the project's branch), Throw away, Keep all that pass, history of 14 nights, save as a redacted page | `components/cockpit/night/MorningPage.tsx`, `night_shift/review.rs`, `night_shift/page.rs` | experimental |
| Quiet night and one morning push with counts only (never task text, names or paths); skipped when the Morning page was opened first | `night_shift/quiet.rs`, `push/night.rs` | experimental |
| Phone card: the redacted summary and one row per task from `GET /night`; Stop (needs `control`, asks once more) through `POST /night/stop`; no Keep or Throw away | `bridge/routes/night.rs`, `components/mobile/NightSummary.tsx` | experimental |

## 1f. Labs (off by default; Settings > Labs)

| Feature | Where | Status |
|---|---|---|
| Assign an issue: a GitHub label from you or someone on your list starts an agent in a separate copy, after you say yes. The agent cannot push. Puppet Master opens a draft PR (`gh`, your own sign-in) and never merges. Held runs and risky changes always ask. ([`features/assign-an-issue.md`](features/assign-an-issue.md)) | `issue_intake/`, `commands/issue_intake.rs`, `pty/env_guard.rs`, `worktrees/harden.rs`, `components/settings/blocks/AssignIssueSettings.tsx`, `components/cockpit/IssueCard.tsx` | **experimental**: unit tests with a fake GitHub and a fake world; no run against real GitHub yet |

## 1g. Agent Store ("More agents", Windows only)

Full page: [`features/agent-store.md`](features/agent-store.md). Entry points: "+" menu > More agents, Set up agents > Browse more agents, Settings > Agents > Browse more agents, palette > Open Agent Store. Not run live yet.

| Feature | Where | Status |
|---|---|---|
| Grid of 19 catalog agents: vendor, license, sign-in, tier chip (Tested or Basic support), search, five filter chips, one main button per card that opens the detail sheet | `components/store/` (`StoreHost`, `AgentStore`, `AgentStoreCard`, `AgentDetail`), `lib/agent-store-model.ts`, `styles/agent-store.css`, `CockpitApp.tsx` (`view === 'store'`) | experimental |
| Catalog as data: one folder per agent under `catalog/agents/`, embedded in the app; catalog check (11 rules) as a cargo test; generator `scripts/gen-store-catalog.mjs` | `packages/app/src-tauri/catalog/`, `agent_store/catalog.rs`, `agent_store/check.rs`, `agent_store/embedded.rs` | experimental |
| Install: vendor command shown first, confirmed, run in a visible PowerShell window; winget, npm, uv, then the vendor script last with a trust tick; then PATH refresh, health check, record, Add | `agent_install/` (`plan_for` store fallback), `components/store/useInstallChain.ts`, `components/cockpit/InstallSheet.tsx` | experimental |
| Add to Puppet Master: writes the shipped definition, approves it as a Store approval ("Added from the Agent Store"); refuses a missing program, an unconfirmed short name, an existing file | `agent_store/add.rs`, `tui/declarative/service.rs` (`approve_as_store`) | experimental |
| Sign in: Open starts the agent in its own pane; the Store never types into a pane or reads a credential | `components/store/AgentDetail.tsx` | experimental |
| Check it works: the wizard's real run as a report, with a cost sentence and a Start button; keeps numbers and version only; approves nothing | `lib/agentStoreCheck.ts`, `components/store/StoreCheck.tsx`, `lib/agentWizardAuto.ts` (`dryRun`) | experimental |
| Remove from Puppet Master (the Store's own file only; recoverable from Settings > Agents) and Uninstall the program (only for a program the Store installed, through winget, npm or uv) | `agent_store/add.rs` (`remove_in`), `agent_store/uninstall.rs`, `agent_install/plan.rs` (`plan_uninstall`) | experimental |
| Store record `<app data>/agent-store.json`: ids, install method and package, versions, dates, counts only | `agent_store/record.rs` | experimental |
| Desktop only: nine Tauri commands; no bridge route, no MCP tool (grep test) | `commands/agent_store.rs` | experimental |
| Publisher check for npm and PyPI names (network, maintainer, before a release) | `scripts/store-verify-publishers.mjs` | experimental |
| Known gap: built-in cards (Claude Code, Codex, Cursor Agent, OpenCode) show the vendor name as the title; the four also show the Tested chip with `windows_tested = false` | `agent_store/catalog.rs` (`label`), `catalog/agents/{claude,codex,cursor_agent,opencode}/entry.toml` | gap |
| Not in the catalog: Amp, Grok CLI, Codebuff (vendor facts not confirmed), Antigravity CLI (no confirmed install source) | `catalog/agents/` | gap |

## 1h. Focus Time (Preview; UI word "Focus")

Full page: [`features/focus-time.md`](features/focus-time.md). Press Focus for 25, 45 or 90 minutes (45 by default) and
agents keep working. Pings that can wait are held; cards that block an agent still show, and reach the phone and Telegram
without a buzz. One summary at the end. A person starts it; no agent can.

| Feature | Where | Status |
|---|---|---|
| Focus note at the top of the Inbox: chips, `Focus · 31 min left`, `+15 min`, `End Focus` | `components/cockpit/FocusNote.tsx`, mounted in `Inbox.tsx` | experimental (live check not run) |
| Sidebar pill while on (`Focus 31m`, menu with `+15 min` and `End Focus`); palette rows; tray `Focus` submenu and tooltip | `FocusPill.tsx`, `palette-focus.ts`, `tray.rs` | experimental |
| Three tiers: Hold (run ends, companion replies, the Night Shift push), Quiet (blocking cards: trust, questions, limits, medium and low approvals; phone and Telegram without a buzz), Through (stuck runs, budget, high-risk approval, consent, tamper, answers) | `attention/focus/policy.rs` | experimental |
| One reminder after 20 minutes for a quiet card still open (counts only; only while the quiet switch is on) | `attention/focus/nudge.rs`, `scan.rs` | experimental |
| End summary: one desktop card `While you focused`, one phone push `Focus is over` (counts only), one Telegram note; an empty session shows nothing | `attention/focus/summary.rs`, `turn_summary/`, `push/focus.rs`, `channels/notify.rs` | experimental |
| Phone bar at the top of the All view: start, `End`, the end summary with `Got it`; the phone reads counts only | `components/mobile/FocusBar.tsx`, `bridge/routes/focus.rs` (`GET /focus`, `POST /focus/start|extend|end`) | experimental |
| Settings > General > Focus Time: default length and the quiet switch | `components/settings/blocks/FocusSettings.tsx`, `commands/focus.rs` | experimental |
| Restart: resumes inside the window; the summary waits after the end; a damaged state file means Focus off | `attention/focus/live.rs`, `store.rs` | experimental |

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

Old chats (section 1c): the first-start card is not on this branch yet.
Tray icon/menu clicking not verified by a human (model unit-tested, tray creation logged); `visible_failed` fallback proven by unit tests only, not live; pm-regression `cursor_agent timeout` can fail `WORKER_STOP_FAILED` (known flaky headless process-tree kill). Older: Env/stdin in declarative launches; `[mcp_host]` has no route; SPEC Q6 cost/concurrency note and Q7 second proof agent unscheduled; T29 skipped; `extract_model` parity; recorder has no bridge route; visible declarative results are inferred; remote/tunnel behaviour only tested with headers on loopback.
