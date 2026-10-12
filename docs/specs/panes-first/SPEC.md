# Panes-first: product direction and phases

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
