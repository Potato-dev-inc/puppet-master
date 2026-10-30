# Panes-first: product direction and phases

> Update 2026-10-30: the "no main window at startup" rule below is superseded. A normal start now opens the primary window like any app; a sign-in start (`--background`), a `--worker` host and a `--no-window` start stay tray-only. See docs/FEATURES.md section 0.

Decided by the user on 2026-10-12.

## Decisions

1. **Individual pane windows are the product.** Every agent run opens its own pop-out pane window (one OS window per pane). There is **no main window**: the grid, the Overview page and the in-app orchestrator sidebar are removed from the user's path.
2. **Tray app.** Puppet Master lives in the Windows tray ("show hidden icons"). The tray icon's menu opens a **Settings window** (Agents: import / approve / revoke / check with fixtures / add generic / drafts; sources & consent; standby; MCP setup; appearance), lists open panes, starts a new pane, and quits. Same process and install as today.
3. **Visible first, headless as backup.** `run_agent` / `delegate_work` / `spawn_agent` default to a visible pane in its own window. A run falls back to headless automatically when (a) the agent has no visible mode (e.g. Kiro, headless-only definition), or (b) the visible pane fails to launch or the prompt cannot be delivered (`PROMPT_NOT_SUBMITTED`, launch error). A caller may still pass `headless: true` explicitly. Every fallback is recorded in the receipt (`fallback: {from: "visible", reason}`) and surfaced in the result.

## Constraints

- Windows stay on the user's secondary monitor in dev (`tauri.dev.local.json`); pane windows never steal focus when opened by an agent.
- Single-instance guard stays: a second launch opens the Settings window.
- The bridge, MCP server, registry, definitions, receipts and security model are unchanged.
- Pop-out creation must not depend on a main webview: pane windows are created by the Rust side (no `App.tsx` listener needed).
- Closing a pane window closes that pane (no "reattach to grid" any more).

## Phases

| Phase | Goal | Done when |
|---|---|---|
| PF1 | Visible-first defaults + headless fallback | Tool schemas default `headless=false`, placement default `window`; fallback rules (a)/(b) implemented with `fallback` in receipts; catalog_version bumped + pinned; dist MCP exe rebuilt; unit tests; live: kilo run without `headless` opens a pop-out, kiro run without `headless` falls back to headless with `fallback.reason = "no_visible_mode"`. |
| PF2 | Tray icon + Settings window | Tray icon with menu (New pane ▸ visible agents, Panes ▸ focus, Settings, Quit); Settings window reuses the existing Settings UI (Agents tab first); second launch opens Settings; tests; live check. |
| PF3 | Remove the main window | App starts tray-only (no main window); pane windows created from Rust; orchestrator sidebar pane no longer spawned at startup; grid/Overview unreachable; closing a pane window closes the pane; pm-regression and vitest/cargo green (tests updated/removed for grid/main); live: Codex run opens a pop-out, closes it at the end, no main window ever appears. |
| PF4 | Docs + demo | FEATURES.md, TEST-PLAN.md, REPO_MAP.md updated; new demo video. |

## Status (2026-10-12)

PF1-PF3 implemented; PF4 docs are in the commit named below. Commits on `refactor/tui-profiles`:

| Phase | Commit | Summary |
|---|---|---|
| PF1 | `b687905` (docs `ee71274`) | Visible-first defaults (`headless=false`, window), headless fallback `no_visible_mode` / `visible_failed`, `fallback` in view + receipt, catalog `66-46d53ccddbd2b4dc`, dist MCP exe swapped |
| PF2 | `27760a6` (kiro fixtures), `199355a` | `tray.rs` tray icon + menu model, `app_windows.rs` Settings window, second launch opens Settings, `--new-pane <agent>` |
| PF3 | `7c0a665` | Main window removed (tray-only), pane windows from Rust, closing a pane window kills the pane, `PM_WINDOW_ANCHOR` / `pane_window_position` anchor + cascade |
| PF4 | docs commit (`git log -- docs/specs/panes-first/SPEC.md`) | FEATURES, TEST-PLAN, REPO_MAP, this Status |

Tests at PF3: cargo lib 927 passed / 2 ignored, MCP bin 30, vitest 239, pm-regression 27/28 (`cursor_agent timeout` `WORKER_STOP_FAILED`, known flaky; passed on rerun).

Verified live: a kilo run with no `headless` opens an unfocused pop-out on the secondary monitor and closes it after the run; Kiro without `headless` falls back with `no_visible_mode`; a Codex external `run_agent` (result `pf3-ok`, `fallback: null`, no main window ever); app start shows no window and `GET /panes` = `[]`; second launch opens Settings; `--new-pane kilo` opens cascaded windows; WM_CLOSE on a pane window removes the pane; `check-window-monitor.ps1` OK throughout.

Not verified: a human clicking the tray icon/menu entries; the `visible_failed` fallback live (unit tests only). The demo video (second half of PF4) is separate from the docs commit.

## Mobile follow-up (2026-10-13)

The old phone UI waited for an orchestrator pane that panes-first no longer starts. It is kept at `?pwa=legacy`; the default phone UI shows the agent panes as tabs. Details and test steps: docs/FEATURES.md, docs/TEST-PLAN.md 2.7.

| Commit | Summary |
|---|---|
| `b6282a1` | Agent tabs (All overview + one tab per pane), legacy PWA kept, `GET /agents/types` for the phone |
| `2745820` | Phone-sized panes: the phone leases the PTY size (`POST /panes/{id}/viewport`) |
| `1d49436`, `486ad4d` | Keyboard shortens the view (shell follows the visual viewport); no double keyboard subtraction |
| `9fbab46` | Revert of two attempts that were redone step by step (`b1094c2`, `158d668`) |
| `33c3746` | Long-poll `/events/poll` (quick tunnels never pass SSE), resize in place (keyboard survives), no local echo, Android keyboard close detected, PC "Sized for your phone" notice |
| `5ba56a5` | Several phones on one pane share the smallest grid |

## Backlog (planned later, not started)

- **Bind panes together** (requested 2026-10-12): let the user link existing pane windows into a group (e.g. move/arrange/close together, or route one pane's output to another). Design to be planned with the user before any work.
- **Preload** (implemented, see docs/FEATURES.md): Settings > Preload keeps warm agents + hidden pane windows per selected provider so opening a pane is instant; replaces the opt-in standby pool UI. Warm `--new-pane` is idle in ~0.3 s vs 2.7-4.8 s cold.
