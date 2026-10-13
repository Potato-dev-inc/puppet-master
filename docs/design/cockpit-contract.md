# Cockpit v1 — implementation contract

Branch `feat/ui-monochrome` (worktree `D:\puppet-master-ui`). Visual reference: `mockups/ui-proposal/next/`
(v3) and `mockups/ui-proposal/STYLE.md`. This file is the interface between the Rust and the React work. If you
must deviate, update this file in the same change and say so in your report.

## Product decisions (from the user)
- **Two modes, global, persisted:**
  - `orchestration` (default): every pane is shown in one **cockpit** window.
  - `individual`: every pane gets its own pop-out window (`detached-pane-<id>`), as today.
- **Exactly one owner window per pane, decided by mode.** Only the owner may resize the PTY, which avoids the ConPTY
  garble from two viewers with different grids. The owner is `cockpit` in orchestration mode and
  `detached-pane-<id>` in individual mode.
- **States:** only the 4 real `PaneStatus` values (`running | waiting_input | idle | error`) plus `exited`. Do not
  invent Thinking/Writing/Tool. UI grouping: Needs you = waiting_input + error, Working = running, Idle = idle.
- **Spinner:** ONE unified dot-matrix spinner for `running`. Static dot patterns for waiting_input, idle, error
  and exited/done.
- **Features:** expose everything Puppet Master supports today; invent nothing the backend can't do. The phone
  composer and Steer are future work and not part of this.

## Rust → UI surface (Tauri commands; all errors as `String`)
| Command | Args | Returns | Notes |
|---|---|---|---|
| `get_ui_mode` | – | `"orchestration" \| "individual"` | from settings.json `ui_mode`, default orchestration |
| `set_ui_mode` | `mode` | `()` | Persists and emits `ui://mode-changed {mode}`. → individual: open a pop-out for every live pane, then hide the cockpit. → orchestration: destroy all `detached-pane-*` windows **without killing their panes**, then show the cockpit. |
| `open_cockpit` | `paneId?: string` | `()` | Show, unminimize and focus the cockpit; with `paneId`, also emit `cockpit://select {pane_id}` |
| `open_settings` | `tab?: string` | `()` | Show or create the Settings window (focused), optionally on a tab |
| `pane_prompt` | `paneId` | `PanePrompt \| null` | Wraps `pane_prompt::detect` / `required_action` |
| `answer_pane_prompt` | `paneId, promptId, choice` | `()` | Same validation as `POST /agents/answer` (stale and replay guards; `allow_all` keeps its existing rules) |
| `interrupt_pane` | `paneId` | `()` | Writes `\x03` |
| `new_pane` | `agentId` | `string` (pane id) | Same path as tray "New pane" (claims warm standby first). In orchestration mode: no pop-out, emit `cockpit://select`. Emits spawn stages (below). |
| `pane_run_info` | `paneId` | `{ handle, status, receipt } \| null` | `receipt` = `RunReceipt` as JSON, or null; null for panes without a run |
| `pane_transcript` | `paneId` | `{ chunks: {stream:string,text:string}[] } \| null` | Capped like the bridge route |

`PanePrompt = { promptId: string, kind: string, text: string, summary?: string, choices: {id:string,label:string}[] }`
(serde `rename_all = "camelCase"`).

### Events
- `ui://mode-changed` `{ mode }`
- `cockpit://select` `{ pane_id }`
- `pty://prompt` `{ pane_id, prompt: PanePrompt | null }`: emitted when a pane enters or leaves `waiting_input`,
  or when the detected prompt changes.
- `pane://spawn-stage` `{ agent_id, pane_id: string|null, stage: "claiming"|"spawning"|"attached"|"ready"|"failed", warm: bool, message?: string }`
- Existing and unchanged: `pty://status`, `pty://exit`, `pty://panes-changed`, `pty://resized`, `terminal-data`,
  `standby://ready-changed`.

### Ownership enforcement (Rust)
- `resize_pane` inspects the calling window label. When the caller is not the owner for the current mode, return
  `Ok` without resizing (log at debug). Phone viewer leases keep working exactly as today.
- Orchestration mode:
  - `present_spawned_pane` (bridge/pane_util.rs), tray `new_pane` and warm-standby claims must not create or show
    pop-out windows. Destroy any hidden warm window for a claimed pane without killing the pane.
  - The pane just appears in `pty://panes-changed`.
  - Agent-spawned panes must not steal focus: no `cockpit://select` unless the user initiated the spawn.
- Window wiring:
  - The cockpit window has label `cockpit` and URL `index.html?cockpit=1`, about 1280×820, min 900×560.
  - Closing the cockpit hides it (panes keep running).
  - Add `cockpit` to capabilities.
- Tray:
  - Left-click opens the cockpit in orchestration mode and Settings in individual mode.
  - Add an "Open cockpit" item.
  - Add a "Mode ▸ Orchestration / Individual" check item.
  - Second launch follows the same rule as left-click.

## UI (React)
- Route: `index.html?cockpit=1` → `CockpitApp`.
- Layout: a 240px sidebar (agents grouped by state, "+" new agent, footer with mode switch, theme toggle and
  bridge line), then the main area.
- Main area with nothing selected: an **Inbox** of panes in `waiting_input`. Parsed prompts get answer buttons;
  unparsed ones get "Open terminal".
- Main area with a pane selected:
  - Header: name, project, `DotSpinner` + state text, cols×rows, and a ⋯ menu: Reflow, Copy selection,
    Interrupt ⌃C, Close (confirm when `running`).
  - Tabs: Terminal · Transcript · Receipt. Transcript and Receipt show an honest empty state when
    `pane_run_info` is null.
- **Only the selected pane's terminal is mounted** (`TerminalViewport`, owner). Switching remounts. Never mount
  the same pane twice in a window.
- ⌘K / Ctrl+K palette: switch to a pane, new agent (warm-ready labelled), interrupt, open settings, switch mode,
  toggle theme.
- Spawn view: a 5×5 `DotSpinner` with a log driven by `pane://spawn-stage` events (no fake lines).
- Styling: monochrome tokens from STYLE.md, scoped to the cockpit stylesheet; Inter, JetBrains Mono and Doto,
  self-hosted. Doto only for the inbox count.

## Implementation notes (Rust, as built)
Deviations and additions, none of which change the table above:
- `index.html?cockpit=1` gets an extra optional `&select=<pane id>` when the cockpit window is *created* by a
  "show this pane" request (tray pane click, new pane from the tray). The webview may not be listening yet when
  `cockpit://select` fires, so the React side should also read `select` on mount. The event is still emitted
  (after 1.5 s for a freshly created window).
- The `--worker` host window (label `main`) always counts as owner in `resize_pane`, in both modes (it is a
  single-terminal window outside the cockpit model). `settings` and any non-owner label are ignored.
- Persistence goes through the store plugin (`settings.json`, key `settings`, field `ui_mode`) so the frontend
  `LazyStore` sees it; React must change the mode with `set_ui_mode`, never by writing the settings key (Rust keeps
  the live mode in memory).
- `set_ui_mode` to the current mode emits `ui://mode-changed` but moves no windows.
- `answer_pane_prompt` never opts into broad approvals (`allow_broad=false`), so `allow_all` is refused as on
  `/agents/answer`; `pane_prompt` / `pty://prompt` choices are the non-broad ones. Errors read `CODE: message`.
- `PanePrompt.choices[].label` is the choice id prettified (`allow_once` -> `Allow once`).
- `pane://spawn-stage`: `claiming` (pane_id null) -> warm: `attached`+`ready` (warm true); cold: `spawning` ->
  `attached` -> `ready` on the pane's first output or 1.5 s after attach, whichever is first. `ready` never waits
  for idle (Claude/Codex stay "running" while starting). `failed` only when the spawn errored or the pane vanished
  from the registry right after attaching; a pane that is alive and listed is never reported failed. The tray emits
  the same events. `new_pane` returns the pane id as soon as the pane is created (the readiness watch is a
  separate thread).
- Orchestration mode creates no warm pop-out windows at all; claims also destroy any hidden one.
- The `detach` bridge route in orchestration shows the pane in the cockpit (no focus).
- `pty://prompt` is emitted from the PTY reader on screen changes; `null` prompt is also sent when entering
  `waiting_input` without a parsable prompt.
