# UI inventory (for the redesign)

Snapshot of every user-facing UI surface as of 2026-10-14 (branch `refactor/tui-profiles`), compiled from four
code-reading passes. Nothing here was verified by running the app unless noted. Tags: **(legacy)** = code
exists but the shipped app cannot reach it; **(dev-only)** = only via a hand-typed URL or the Vite dev server;
**(dead)** = code that nothing triggers; **planned** = shown in UI with a red "Planned / Not implemented" badge.

Paths are relative to `packages/app/src/` (UI) or `packages/app/src-tauri/src/` (Rust) unless given in full.

---

## 0. Surface map

| Surface | Reachable? | Entry | Root files |
|---|---|---|---|
| System tray | yes | app start (no window opens) | `tray.rs` |
| Settings window | yes | tray left-click / Settings… / second launch | `SettingsApp.tsx`, `components/SettingsPanel.tsx`, `components/settings/**` |
| Pane windows (one per agent) | yes | tray New pane, `run_agent`, CLI detach, phone spawn | `TerminalApp.tsx` (`detached=1`), `components/TerminalViewport.tsx` |
| Warm (Preload) pane windows | yes, hidden until claimed | Preload | `app_windows.rs` |
| Worker host window | yes, CLI only | `puppet-master worker …` | `WorkerHostBootstrap.tsx`, `worker_launch.rs` |
| Phone PWA | yes | `/pair/CODE` link, phone browser | `PwaApp.tsx`, `components/mobile/*`, `terminal/*` |
| Legacy phone PWA | yes, by link | `?pwa=legacy` / menu link | `PwaLegacyApp.tsx`, `components/pwa/*` |
| Command Console | dev-only | `index.html?terminal=1` | `TerminalApp.tsx` (non-detached) |
| Main window / workspace / orchestrator sidebar | **legacy** | `App.tsx` (no window ever loads it) | `App.tsx`, `HomeDashboard`, `TerminalGrid`, `TerminalPane`, `PuppetMasterSidebar`, `sidebar/*`, `CoordinationPanel` |

`tauri.conf.json` has `windows: []`; Rust only creates `settings`, `detached-pane-<id>` and (worker mode) `main`.

---

## 1. Desktop

### 1.1 System tray (`tray.rs`)
- **Tray icon** "Puppet Master"; **left-click** opens/focuses Settings; menu on **right-click** only.
- **New pane ▸** one item per registered agent; headless-only greyed "(headless only)"; empty "(no agents)"; **"(ready)"** suffix when a Preload worker is warm (reveals the pre-built window instead of spawning).
- **Panes ▸** open panes as `agentid · id8`; click shows/unminimizes the window **without focusing it**.
- **Settings…**, **Quit Puppet Master** (immediate `exit(0)`, kills every pane, no confirmation).
- Menu rebuilds on pane / definition / standby changes. New-pane failures are only logged.

### 1.2 App lifecycle & windows (`lib.rs`, `app_windows.rs`, `pane_windows.rs`)
- **Normal start** opens the primary window (cockpit or Settings); a sign-in start (`--background`) stays tray-only; no first-run hint. **Second launch** opens Settings; `--new-pane <agent>` opens just that pane.
- Closing the last window does not quit.
- **Settings window** 1100×760 (min 720×480), opens on the **Agents** tab, X only hides it.
- **Pane window**: title `agentid · id8` (never changes), size from grid (cols×7.25, rows×16), unfocused when an agent opens it, focused when the user asked; placement by anchor (`PM_WINDOW_ANCHOR` env → `settings.json pane_window_position` (no UI) → primary monitor) + 28 px cascade.
- **Closing a pane window kills the pane** (no confirmation, even mid-run). Window auto-closes 1.5 s after its pane exits/errs.
- Windows follow their panes (sweeps destroy orphans).

### 1.3 Pane window contents (`TerminalApp.tsx` + `TerminalViewport.tsx`, `chrome={false}`)
- **Bare full-window terminal**: no in-window title bar, status, label, pid or size.
- **Pre-size step**: resized to 120×32 and blank for two frames before mounting.
- **Scrollback replay** of up to 10,000 raw lines on open.
- **Reflow nudge ↻**: 18 px, 45 % opacity, bottom-right; refit + rows−1/+1 resize so TUIs redraw.
- **"Sized for your phone" overlay** while a phone holds the pane: "Click here or switch to this window to use it on this PC." Click / focus / keypress reclaims the size. While held, the grid follows the phone's PTY size in stream order.
- **"No terminal pane selected."** dashed placeholder (no pane yet / 1.5 s before close).
- **↙ Move pane back to app** (dead).

### 1.4 Terminal UX (desktop) (`terminal/terminal-session.ts`, `theme.ts`, `xterm-textarea.ts`)
- xterm.js, Cascadia Code/Consolas 12 px, blinking cursor, 5000-line scrollback, Unicode 11, ConPTY mode.
- **Monochrome theme** from `--pm-*` tokens (red→err (white in dark), green/magenta→fg, blue/cyan→muted).
- Auto-fit on window resize; PTY resize only when cols/rows change.
- **Paste** Ctrl/Cmd+V (clipboard API + paste-event fallback; xterm textarea overlaid for WebView2 IME/paste).
- **Copy**: no custom copy, no context menu; Ctrl+C goes to the PTY.
- **Links**: web + `file:///` clickable via `window.open` (not the app's `open_external_url`).
- Pointer-down focuses; mouse wheel scrolls; **view jumps to bottom on every keystroke and output chunk**.
- Input sent in 4 ms batches. No search, no zoom/font-size, no app shortcuts beyond paste.

### 1.5 Worker host window (CLI `puppet-master worker`)
- Window "Puppet Master" 1000×700; startup "Starting worker…" (sets project, kills same-type siblings, spawns/reuses pane, resizes, titles, focuses).
- Error screen; **X kills the pane and quits the app**.
- **API key tray ˄** (opencode_native only) → **OpenCode API keys panel**: Key A/B password fields, Save keys, Refresh, Capture A/B from disk, Toggle / Use A / Use B, active profile, rotate hint.
- Worker exit notice hard-coded to `npm run worker opencode_native` and never shown in the detached window.

### 1.6 Command Console (dev-only, `index.html?terminal=1`)
- Terminal + 360 px sidebar ("PTY Terminal / Command Console", API online pill), pane picker, agent picker, New/Kill, input box with "Send Enter after input", Read Buffer, Snapshot, HTTP API curl snippet.

### 1.7 Legacy main window (`App.tsx`, unreachable)
- Boot screen "Starting control room"; Home dashboard (rail, hero, metric cards, quick actions, active panes, saved defaults); Agents overview card.
- Workspace header (project picker, New session, Clear, Restart (only kills), Kill All); terminal grid (1–2 cols, drag reorder, resize, settle animation).
- **Grid pane header** with agent-switch dropdown, **Rec / Stop rec** (the recorder's only entry point), pop-out ↗, reflow ↻, kill ✕.
- Orchestrator sidebar (bridge dot, New chat, theme toggle, Backend/Model bar, Agent tab = orchestrator terminal or API chat with Interrupt, Tools tab = Coordination panel (tasks/locks/audit/session/context/agents) + MCP log). Auto-approve/wake of worker panes.

### 1.8 Cross-surface toast
- **Update available**: "New version available — vX is ready (you have vY)", **Download & install** (only opens the release page), Dismiss. Rendered in the Settings window only.

---

## 2. Settings window

**Chrome**:
- ← Back (hides, no save).
- Header copy.
- **Search** (filters tab labels only).
- Left nav of **22 tabs**, with red "Planned" pills on placeholder tabs.
- Footer: "Saved", **Done** (hides), **Save changes**.
- **Planned badge**: "Planned" when compact, "Not implemented" in full form; planned blocks are red-bordered, 60 % opacity, non-interactive.

| Tab | Implemented controls | Planned controls |
|---|---|---|
| **General** | Settings file path; **Default project folder** (text + **Browse**); **Theme** Dark/Light (on Save); app version, **Check for updates**, Download & install | Auto-save workspace, Semantic index, Auto session summaries, Reduce motion |
| **Appearance** | **Sidebar width** presets + custom (legacy sidebar only) | Default grid columns, Design language |
| **Sessions** | **Default orchestrator** (API / Claude / Codex / OpenCode CLI) | Send initial prompt, Keep session history |
| **Orchestrator** | Backend (duplicate of Sessions); **Librarian indexer**; **OpenCode default model** (provider id, model id); **Default provider & model**; **Custom models** list (Remove / Add) | Cost-aware model router |
| **API keys** | Anthropic / OpenAI / OpenRouter keys (on Save); **OpenCode keys A/B** with own Save + Refresh status; **Key automation** mode; Restart pane after rotate | GitHub token, Linear key, Validate keys on save |
| **MCP & bridge** | **Runtime status** + Recheck & repair; Bridge endpoint (read-only); **Install** in project / globally with results; **Uninstall** project / global (confirm); Claude Desktop JSON + Codex TOML snippets with Copy | Remote runners |
| **Mobile PWA** | **Mobile input buffer (ms)**; **Keyboard resizes the agent**; **Show mobile input box**; pairing panel: **Public PWA URL**, **Local dev server port**, info lines, **QR**, pairing code + expiry, **Refresh QR**, **Paired devices** + Revoke | Require pairing approval |
| **Preload** | Master switch; **Agents to keep warm** (checkbox + keep 1–2; "(experimental)" / "(not a terminal agent)" / "(no warm mode yet)"); **Idle timeout**; live **Status** (warm agents, closes-in, held back, errors) | – |
| **Agents** | Project consent banner (Allow / Not now); Definitions: **Reload**, **Import .toml**, **Add as generic agent**, fixtures folder; rows with source/state/experimental badges, errors, **Check with fixtures** (PASS/FAIL, Create folder), **Export**, **Review and approve** (modal with command preview + diff + warnings), **Revoke**; **Drafts** (Refresh, Validate); **Sources** (switches), **Consented projects** (Revoke consent), **Extra directories** (consent switch, Remove, Add) | – |
| **Storage** | Workspace session metrics (scope, tasks, locks, paths, events) + Refresh | Require locks for edits, Archive sessions; ("Project-local tasks and locks" switch looks live but does nothing) |
| **Developer** | Bridge endpoint, Settings file (duplicates); **Use bundled Rust MCP**; Bridge health sample (static, unlabeled) | Log MCP tool calls, Protocol inspector |
| **Advanced** | **Uninstall Puppet Master** (confirm; installed builds only); **Export settings JSON** (keys masked) | Scrollback limit, Spawn timeout, Telemetry |
| **Placeholders (all planned)** | – | Security, Notifications, Backup & sync, Agent marketplace, Rules & guardrails, Cost controls, Team workspace, Automation, Observability, Plugins (mostly fake list data) |

**Cross-cutting behaviour**:
- Two save models. Most fields wait for **Save changes**; Browse, OpenCode keys, MCP actions, Refresh QR, Revoke, Preload and Agents apply instantly.
- No unsaved-changes guard.
- Hidden-not-destroyed window means reopening shows stale state; settings load once.
- "Saved" never clears.
- Validation is by clamping while typing.
- Native confirm dialogs only for MCP uninstall and app uninstall.

---

## 3. Phone PWA (current, `PwaApp.tsx`)

### 3.1 Install, routing, pairing
- Phone UI for `?pwa`, standalone mode, or Android/iPhone/iPad user agent.
- Manifest "Puppet Master"/"PM", dark, 192/512 icons; iOS meta tags; workbox SW auto-updates silently (no install or update UI).
- **Pairing link** `/pair/CODE`: auto-pair with stages ("Connecting to desktop…" → "Signing in…"), reuses an existing valid pairing, errors with hints, Try again / Back.
- **Setup screen**:
  - Context hint and dev tunnel card (dev only).
  - Pair / Manual URL toggle.
  - "Paired as …" card with Reconnect and Unpair (Unpair has no confirmation).
  - 3-step instructions.
  - Advanced: paste a code, or the legacy JSON QR.
  - Manual URL + Connect.
- Codes are minted by the desktop QR or `npm run -s pair-link`.

### 3.2 Shell & connection
- Shell fits the visible area above the keyboard (Android `interactive-widget=resizes-content`).
- Active tab is remembered.
- 5 s health/pane resync; `resync` events rebuild the open terminal.
- Remote: WebSocket `/events/ws` (token hex-encoded in subprotocol) for events and input; falls back to polling and retries the socket every 15 s.
- Only the open agent is streamed.

### 3.3 Tab bar
- **All** tab with connection dot and an amber attention badge.
- One tab per agent: status dot (working pulse / needs you / error / idle), raw `agent_type` label numbered when duplicated, horizontal scroll.
- **+** New agent; **⋯** Menu. Safe-area padding.

### 3.4 All overview
- Connecting state.
- Empty state with "Open an agent".
- Agent cards: dot, label, project, status, last 2 meaningful lines, "last output …"; amber border when the agent needs you. Previews poll every 4 s.
- Tap a card to open the agent (this path doesn't nudge).

### 3.5 Agent tab (terminal)
**Header and lifecycle**
- Header: dot, label, `cols×rows · pid`.
- "Loading agent…" placeholder.
- Session survives resizes; replays the screen at the current size (`/screen`).
- No local echo; tappable links.

**Size ownership**
- The phone measures its fit grid, saved in localStorage.
- **Claims the pane size** on tab open, app resume, any touch, or typing; renews every 5 s.
- **Sticky**: the size stays after the phone leaves, until the PC window reclaims it on click or focus. With several phones, the smallest wins.
- **Spawn at phone size** with an immediate claim.

**Nudge**
- After a tab tap, app resume or terminal tap: in-place redraw from the PC screen (no PTY resize), 350 ms settle, at most once per 1.5 s.
- Skipped while scrolled up; typing, Keys, input and ↓ never nudge.

**Input: visible box (default)**
- 44 px "Type a command…" box.
- Buffered typing turned into keystroke deltas (IME, autocorrect and CJK safe).
- Enter sends; Backspace on an empty box sends DEL; hardware Ctrl/arrows forwarded.
- Long-press (450 ms) focuses the box.

**Input: hidden mode**
- A tap (no movement) toggles the keyboard into an invisible field; a swipe scrolls.
- Re-shows a dismissed keyboard; detects keyboard close.

**Keys panel**
- 44 px frosted-glass toggle (keyboard glyph ↔ ×) opens a glass card:
  - **Control**: esc, tab, ⇤ mode, ⌃C, ⌃D, ⌃L, ⌃ latch
  - **Navigate**: Home ↑ End / ← ↓ →
  - **Edit**: ⌫, ⌦, ↵
- Acts on pointerdown (keyboard stays up); haptic tick.
- Auto-repeat (400/60 ms); Ctrl latch; closes on tap outside or Esc; reduced-motion aware.

**Touch scroll**
- 8 px threshold, momentum.
- TUIs with mouse tracking get wheel reports; otherwise xterm scrollback scrolls.
- **↓ jump-to-bottom** (36 px). A swipe cancels long-press and taps.

**Keyboard crop / view**
- Keyboard doesn't resize the agent by default: the view is **cropped**.
- **Cursor-anchored**: the typing line plus 2 rows is kept visible.
- Swipe pans the cropped top first, then scrolls; typing resets the pan.
- A grid wider than the phone is scaled to fit the width.
- Option: "Keyboard resizes the agent".

### 3.6 Sheets
- **New agent**: 2-column agent grid; visible-capable agents first; disabled ones show their reason; experimental tag.
  - Loading, Starting and error states. Spawn opens the new tab.
  - Closes via backdrop or ×; no swipe-down.
- **Menu**:
  - Connection row with **Change**.
  - **Connection stats line**, e.g. `Socket · RTT … · typing echo … · reconnects …` or `Polling (slow) · retrying socket…`.
  - **Close <agent>** (native confirm).
  - Settings: **Input box under the terminal**, **Keyboard resizes the agent**, **Typing buffer (ms)**.
  - "Classic mobile view (orchestrator)" link.

### 3.7 Legacy PWA (`?pwa=legacy`)
- Floating ⋯ menu; Orchestrator / Panes switch; backend + model selects.
- Orchestrator terminal mirror or API chat with MCP log.
- Panes list; pane select / Refresh / Close (no confirmation) / spawn.
- Buffer setting. Mirror-mode terminal with local echo and scale-to-fit.
- No link back to the new UI.

---

## 4. Design system

### 4.1 Tokens
- **Theme switch**: `lib/settings.ts applyTheme` sets `html[data-theme]` (dark default). `styles/theme.css` (38 lines) holds `:root` (dark) and `:root[data-theme='light']`. No `prefers-color-scheme`. The terminal re-themes via a MutationObserver (`terminal/theme.ts`).
- **Colours**: RGB triplets used as `rgb(var(--pm-x) / a)`. The palette is fully **monochrome**; ok, warn and err are all greys:

  | Token | Dark | Light |
  |---|---|---|
  | `--pm-bg` | 10 10 10 | 250 250 250 |
  | `--pm-panel` | 18 18 18 | 242 242 242 |
  | `--pm-raised` | 25 25 25 | 255 255 255 |
  | `--pm-border` | 45 45 45 | 212 212 212 |
  | `--pm-accent` | 245 245 245 | 12 12 12 |
  | `--pm-text` | 232 232 232 | 18 18 18 |
  | `--pm-muted` | 150 150 150 | 96 96 96 |
  | `--pm-ok` | 214 214 214 | 56 56 56 |
  | `--pm-warn` | 180 180 180 | 86 86 86 |
  | `--pm-err` | 255 255 255 | 0 0 0 |

- **Tailwind** (3.4, no plugins) extends only `pm.*` colours: no fonts, radii, shadows, spacing or z.
  - Top classes: `text-pm-muted` 180, `border-pm-border` 124, `bg-pm-bg` 52.
- **Other CSS variables**:
  - Keys-panel glass vars are literal RGB, with dark detected via `:not([data-theme='light'])`.
  - Layout vars are set from JS: visual viewport, keyboard inset, keyboard panel height, input bar height.
- **Typography**:
  - No UI font declared, so the system sans from preflight is used.
  - Terminal: `"Cascadia Code", Consolas, …` 12 px. A second mono stack `ui-monospace, SFMono-Regular, …` appears 7× in legacy CSS.
  - Sizes: `text-xs` 150, `text-sm` 61, arbitrary `text-[10px]` 47 (+ `[11px]`, `[9px]`). CSS uses 13 distinct px sizes.
- **Radius**: Tailwind `rounded` 68, `-lg` 51, `-full` 25, `-xl` 13; CSS has 13 distinct values (4 to 20 px, 50 %, 999 px).
- **Shadows**: a few Tailwind shadows, 4 different black CSS drop shadows, accent focus rings.
- **Breakpoints**: 560/760/900/980/1100 px (mostly legacy `home.css`), plus 4 reduced-motion blocks.
- **Z-index**: no scale. CSS uses 0/1/15/16/20/30/35/40; Tailwind `z-10/20/50` and `z-[80]`.

### 4.2 Components
- **The only primitive library is `components/settings/settings-ui.tsx`**, used only inside Settings:
  - SettingsSection 22, SettingBlock 36, SettingToggle 34, FieldInput 20, FieldSelect 9, InfoCard 7, CodeBlock 3, PlannedBadge, MockListCard 6.
- **Buttons**: no component. 114 `<button className>` produce **88 distinct recipes**; the most common recipe appears 4×.
- **Form controls outside Settings**: 14 input recipes, 4 select, 3 textarea.
- **Status dots**: no component. **6 copy-pasted `STATUS_COLOR` maps**, plus `STATUS` in `lib/mobile-agent-tabs.ts`, with differing dot sizes. The bridge-ready dot is repeated 3×.
- **Tabs**: four ad-hoc implementations (phone tab bar, Settings nav, legacy sidebar, legacy coordination).
- **Sheets**: `MobileSheet.tsx` exports two concrete sheets, not a generic sheet.
- **Icons**: no icon library. Unicode glyphs (`× ✕ ⋯ ↻ ↗ → ← ⌃ ❯ ● ✓ ⚙ ■`) appear in ~16 files; inline SVG only in the Keys panel; a CSS border spinner.
- **Two paradigms**:
  - BEM CSS with `var(--pm-*)`: terminal and worker windows, e.g. `.pm-terminal-control-button`.
  - Tailwind `pm-*` utilities: Settings, PWA, pane headers.

### 4.3 CSS files (`styles/index.css` import order; ~2,449 lines total)

| File | Lines | Status |
|---|---|---|
| tailwind.css | 5 | directives |
| theme.css | 38 | live (tokens) |
| layout.css | 44 | live (PWA shell) |
| terminal.css | 558 | live |
| terminal-app.css | 368 | live |
| worker-auth.css | 123 | live |
| boot.css | 359 | **legacy-only** |
| home.css | 652 | **legacy-only** |
| coordination.css | 259 | **legacy-only** |
| pane-settle.css | 32 | **legacy-only** |

- About **1,300 of ~2,400 custom CSS lines serve only unreachable UI**.
- Dead CSS variables:
  - `useTerminalAuthorityCss` sets `--pm-terminal-max-width`, `--pm-pty-authority-width` and `--pm-mobile-orchestrator-active`; no CSS reads them.
  - `--pm-keyboard-inset` is never read.

### 4.4 Design-system inconsistencies
- **Status colour lives outside the tokens**: ~108 raw `red/amber/emerald-*` classes in 20 files (AgentsTab 37, McpTab 18, settings-ui 7, …), because `pm-ok/warn/err` are grey.
- **Undefined classes**: `text-pm-amber-400` (`MobilePairingPanel.tsx:126`), `pm-boot-step-text` (legacy).
- **Dark-only colours that won't flip in light theme**:
  - `text-zinc-100/200/300` (~30 uses) across PwaApp, PwaLegacyApp, SetupScreen, mobile components and the QR scanner.
  - `bg-black/50|90`, `text-white`, `text-black`.
- **Hard-coded colours**:
  - CSS: `terminal.css:174-175` (`rgba(0,0,0,.6)`, `#fff`).
  - TS: QR colours (`MobilePairingPanel.tsx:66`); `canvas-terminal.ts` coloured ANSI vs the monochrome CSS theme.
  - HTML: `index.html theme-color #0f0f0f` ≠ `--pm-bg` and ignores light mode.
- **Fonts**: two mono stacks; the terminal stack is duplicated in `terminal.css:518`; no UI font.
- **No shared scales** for font size, radius or z-index. CSS z 40 and Tailwind `z-50` overlap in role.
- **Duplicated logic**: 6 status maps, 88 button recipes; Settings primitives are not reused in the PWA or terminal windows.

---

## 5. Issues worth fixing regardless of redesign (bugs)

1. **Settings › General: typing in "Default project folder" has no visible effect.** Only Browse works. (`components/settings/tabs/GeneralTab.tsx:30`)
2. **Settings › API keys copy claims "secure OS storage"**, but provider keys are stored in plain `settings.json`. (`tabs/ApiTab.tsx:69`, `lib/settings.ts`)
3. **Settings › Storage: "Project-local tasks and locks" switch** is marked implemented but has an empty `onChange`. (`tabs/StorageTab.tsx:61`)
4. **Settings › Mobile pairing warning class `text-pm-amber-400` is undefined**, so the warning has no colour. (`components/MobilePairingPanel.tsx`)
5. **Settings reopen shows stale state.** The window is hidden, not destroyed, and settings load once. There is no unsaved-changes guard. (`SettingsPanel.tsx`)
6. **Clamp-while-typing makes number fields awkward.** Typing "1" becomes 50 (buffer); the first port digit becomes 1024. (`tabs/MobileTab.tsx:14`, `MobilePairingPanel.tsx:153`, phone `MobileSheet.tsx setDelay`)
7. **Phone: the Ctrl latch survives closing the Keys panel**, so a later letter becomes a control code with no visible cue. (`terminal/mobile-input-guard.ts closeSpecialPanel`)
8. **Phone: ↓ jump-to-bottom sits under the Keys toggle** (same right/bottom offsets). (`styles/terminal.css` ~165–234)
9. **Desktop: the terminal yanks to the bottom on every output chunk**, so you can't read history while an agent prints. (`terminal-session.ts scrollToCursor`)
10. **Desktop: theme changes don't reach open pane windows.** `TerminalApp` never loads settings. (`lib/settings.ts applyTheme`, `TerminalApp.tsx`)
11. **Desktop: tray › Panes › click doesn't focus the window.** (`tray.rs FocusPane`, `open_pane_window(..., false)`)
12. **Update banner "Download & install" is white on near-white** in dark mode, and it only opens a page. (`UpdateAvailableBanner.tsx`)
13. **MCP tab: config snippets always show `npx`** even when Rust MCP is selected; the toggle label and the install can disagree until saved. (`tabs/McpTab.tsx:17-18,39,180`)
14. **Librarian indexer can't be cleared** (it snaps back to "opencode"). (`tabs/OrchestratorTab.tsx:37`)
15. **Docs are stale.** The Rec button and "last viewer restores the desktop size" are no longer true. (`docs/FEATURES.md` §4.2 and line 42, `docs/REPO_MAP.md`)

## 6. Redesign themes (rough edges grouped)

- **Identity & status on desktop.** Pane windows show no agent name, status or "needs you". OS titles are raw ids. Status colours are near-identical greys (ok 214 / warn 180 / muted 150). Tray mixes friendly labels with raw ids.
- **Destructive actions without confirmation.** Closing a pane window kills a running agent. Tray Quit kills everything. Revoke (devices, definitions, consent) and phone Unpair act at once. Legacy Close has no confirmation. Meanwhile the phone uses native `confirm()`/`alert()`.
- **Terminal colour.** Monochrome ANSI mapping (red/green → white/grey) hurts diffs. Dark selection highlight is nearly invisible.
- **Settings information architecture.**
  - 22 tabs, 10 of them fake placeholders with realistic mock data.
  - Duplicated settings: orchestrator backend ×2, settings file ×2, bridge endpoint ×2, cost routing ×2, inspector ×2, approval ×2.
  - Settings that only affect unreachable legacy UI: sidebar width, much of Orchestrator.
  - Opens on Agents (9th item).
  - Footer Save shows on instant-save tabs.
  - Search covers tab names only.
- **Inconsistent components.** Agents and Preload tabs use their own switch, button and input styles and native checkboxes. The mobile pairing panel uses raw inputs. Raw `red-400`/`emerald-400`/`amber-400` sit next to `pm-err`/`pm-ok` tokens. Glyph icons (↻ ˄ ↙ ↓ ⋯) are mixed with one SVG (Keys).
- **Copy & naming.**
  - "PC" vs "desktop".
  - Raw `agent_type` ids vs friendly labels.
  - Developer copy in user screens: "Is dev still running? /bridge/health", "Legacy JSON QR", "RTT / typing echo".
  - "Planned" vs "Not implemented".
  - "Download & install" only opens a page.
  - Agent-specific copy in generic settings ("Claude reprints").
- **First-run & feedback.**
  - Start with no hint.
  - Silent failures (tray New pane).
  - Phone offline state is only a small dot.
  - No PWA update notice.
  - Pane windows close 1.5 s after exit, hiding the final output.
- **Phone discoverability.**
  - Closing an agent only from ⋯.
  - Long-press to focus is hidden.
  - Hidden-input mode has no hint.
  - The Keys glyph suggests the OS keyboard.
  - The dot means connection on All but status on agent tabs.
  - Sheets have no grab handle or swipe-down.
  - The phone shows `cols×rows · pid` to end users.
- **Dead and legacy weight.**
  - The whole `App.tsx` tree.
  - The recorder entry point.
  - The reattach button.
  - The pane detach/reattach event listeners.
  - The legacy PWA (still linked from the menu).
  - The Command Console (dev-only).
  - Decide what to keep (e.g. Rec, Coordination panel) and give it a reachable home.
