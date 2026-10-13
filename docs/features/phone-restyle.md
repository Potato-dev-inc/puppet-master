# Phone PWA restyle (branch `feat/phone-restyle`)

Cut from `integration/night` (`5bdcfd3`). Commits on top: `84bdf1c` (restyle), `747080f` (harness), `343dee9` (tests), then this doc.
No Rust, no MCP catalog change.

## 1. What and why

The phone PWA (`PwaApp.tsx`) now has the monochrome iOS look of `mockups/ui-proposal/phone.html` (v3) and `STYLE.md`
(grayscale only, hairlines, 44px rows, no coloured badges). Personas: B (vibecoders) and C (non-technical) who watch and
answer their agents from the couch. Nothing is agent-specific: the list shows whatever `PaneInfo` the PC reports, and a
pane's title falls back to its agent id.

What changed on screen:
- **Agents list**: 34px large title, a Doto "02 waiting" counter, rows grouped **Needs you / Working / Idle**
  (`groupPanes` in `lib/mobile-agent-tabs.ts`; "Needs you" holds waiting and error panes). Rows use the unified dot glyph
  (`DotSpinner`, the same component as the cockpit), title, project, last output line, "3m ago", chevron. Needs-you glyphs
  pulse as a whole (CSS only). Empty state: idle 5x5 glyph on a dot-grid texture.
- **Top bar**: pill tabs (open tab filled), pane titles on the tabs, state glyph per tab, a white count badge for "needs you",
  44px `+` and `...` buttons.
- **Sheets**: grabber, centred title, "Done", 38px top radius, float material. **Swipe down** on the grabber/title bar closes
  (past 110px or a fling); scrim tap, Escape and Done also close. New agent = inset list (disabled agents say why).
  Menu = grouped lists with real iOS switches on the existing checkboxes.
- **Confirm**: "Stop all agents" and "Close <agent>" use an iOS action sheet (message, one action, Cancel) instead of
  `window.confirm`; results show in a toast instead of `window.alert`.
- **Kept and restyled from tonight**: ask_user question cards (primary first answer, 44px buttons and field), the away digest
  card, the Notifications row and iOS "Add to Home Screen" hint (same `lib/push-client.ts` state machine), the live-preview
  "Open on your PC" note, pane titles on tabs.
- **Loop guard on the phone**: the bridge already sent `event: loop-signal` (`bridge/sse.rs::push_loop_signal_sse`) but the
  phone ignored it. `bridge-sse.ts` now parses it (`BridgeEvent` `loop-signal`); `PwaApp` keeps a pane-to-signal map and shows a
  quiet "Looks stuck: the same error 4 times" note under the bar on that agent's tab and as the row's subtitle in the list.
  It clears when the signal arrives as `null`.
- **Keys panel and jump-to-bottom button: CSS only** (`terminal.css`): float material, hairline, 44px caps, sentence-case
  captions (the uppercase tracked titles are gone), latched Ctrl = the one inverted (primary) key, jump button 36px to 44px.

## 2. Design

| File | Change |
|---|---|
| `styles/phone.css` (new) | tokens scoped under `.ph` (dark default, `:root[data-theme=light]`; follows `data-theme` only, like the terminal), all `ph-*` classes. Element resets use `:where()` so they never out-rank the Keys panel classes. Reduced motion: no animation. |
| `components/mobile/AgentTabBar.tsx`, `AgentsOverview.tsx`, `MobileSheet.tsx` | restyled (lead); `Sheet` exported, `ConfirmSheet` added |
| `QuestionsList.tsx`, `DigestCard.tsx`, `NotificationsToggle.tsx`, `PreviewNote.tsx` | class-only restyles done by Haiku subagents and reviewed; DigestCard gained a `.ph-clamp` span |
| `components/mobile/StuckNote.tsx` (new) | the stuck note |
| `lib/mobile-agent-tabs.ts` | `glyphStatus`, `groupPanes`, `waitingCount` (pure) |
| `lib/bridge-sse.ts`, `lib/bridge-types.ts` | `loop-signal` event and `LoopSignalInfo` |
| `PwaApp.tsx` | `ph` class on the shells, `stuck` map and `loop-signal` handling, toast (replaces two `window.alert`s), StuckNote mount, `stuck` prop. Nothing in the lease, nudge or input code. |
| `dev/harness.tsx`, `dev/types.ts`, `dev/mock-bridge.ts`, `dev/phone-fixtures.ts`, `dev/scenarios/phone-*.ts` | harness `phone` route |
| `scripts/evidence/screenshot.mjs` | per-scenario window size (see below) |

OSS: no new dependency. Doto is already bundled (`@fontsource/doto`), `DotSpinner` is the cockpit's.

### Harness: phone route
`route: 'phone'` renders the real `PwaApp`. `dev/mock-bridge.ts` replaces `fetch` for the fake bridge origin
(`http://127.0.0.1:17321/bridge`) and `EventSource`, answering `/health`, `/panes`, `/previews`, `/settings`, `/questions`,
`/digest`, `/agents/types`, `/panes/stop-all` and `/panes/:id/{buffer,screen,raw,viewport}` from `dev/phone-fixtures.ts`.
The network stays blocked for everything else. Scenario options: `phone.activeTab`, `phone.questions` / `phone.digest: false`,
`phone.userAgent` (an iPhone UA for the Home Screen hint, used by `phone-menu`), `phone.events` (bridge events delivered
once, e.g. `loop-signal`), `phonePanes`.

**Window size per scenario**: a `window: { width, height }` literal in the scenario file (the script reads it from the text);
`route: 'phone'` defaults to 390x844; others stay 1440x900. Edge cannot open a window narrower than 500px (a first attempt
produced a 500px layout cropped to 390), so scenarios narrower than 500px are driven over the DevTools protocol
(`Emulation.setDeviceMetricsOverride`, `Page.captureScreenshot`) and shot when the harness sets `window.__harnessReady`
(after the scenario's `after()` finished). Wide scenarios keep the old virtual-time path unchanged. Still headless, own
temp profile, killed on exit.

## 3. Evidence

PNGs (390x844): `D:\pm-night\evidence\phone-restyle\`
- `phone-agents.png`: large title and Doto counter, question card, away digest, start of the grouped list
- `phone-agents-list.png`: the same list scrolled: Needs you (powershell error, Claude waiting), Working, Idle
- `phone-terminal.png`: agent tab with the real xterm on the mocked 44x30 screen, the loop-guard stuck note, the Keys panel open, the input box
- `phone-new-agent.png`: the sheet (Experimental tag, "Not installed" disabled row)
- `phone-menu.png`: Notifications with the iOS hint, Terminal switches, Stop all agents
- `phone-confirm.png`: the action sheet over the menu

Command: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\phone-restyle phone-agents phone-agents-list phone-terminal phone-new-agent phone-menu phone-confirm`
(all six `ok`). I looked at every PNG; fixes made from them: Edge's 500px minimum window (CDP path), the mock bridge URL
needing the `/bridge` suffix, the digest line clamp cutting text (clamp moved to an inner span).

```
npx tsc --noEmit -p packages/app             -> exit 0, no output
npx vitest run (packages/app)                -> Test Files 82 passed (82), Tests 609 passed (609)
npx vitest run src/components/mobile src/lib/mobile-agent-tabs.groups.test.ts src/lib/bridge-sse.loop-signal.test.ts
                                             -> Test Files 9 passed (9), Tests 47 passed (47)
npm run build --workspace=@puppet-master/app -> vite build ok (PWA injectManifest, dist/sw.js)
```
The build also rewrote `src-tauri/resources/mcp-stdio.bundle.cjs` and `puppet-master-cli.bundle.mjs`; both were reverted with
`git checkout`. The npm build script also runs a Rust release build of the MCP binary; I did not set `CARGO_BUILD_JOBS` for
that one run (1m21s on a warm target dir).

New tests (all pass): `mobile-agent-tabs.groups.test.ts` (7), `bridge-sse.loop-signal.test.ts` (2), `Notes.test.tsx` (2),
`AgentTabBar.test.tsx` (6), `AgentsOverview.test.tsx` (8), `MobileSheet.test.tsx` (7: confirm sheet for Stop all and Close,
Escape, scrim, grabber, swipe past and under the threshold, New agent rows). Existing phone tests (DigestCard 11,
NotificationsToggle 2, QuestionsList 2) are green and unchanged. `MobileSheet.test.tsx` itself was rewritten because
"Stop all agents" now confirms in an action sheet instead of `window.confirm`.

**Fails before, passes after**:
- `mobile-agent-tabs.groups.test.ts` against the old `lib/mobile-agent-tabs.ts` (`git show 5bdcfd3:...` copied in):
  `7 failed ... TypeError: (0 , glyphStatus) is not a function`; with the new file `Tests 7 passed (7)`.
- The new "asks in an action sheet" test would fail on the old MenuSheet (it has no `[role="alertdialog"]`).

### No-logic-change check for the protected files
```
git diff 5bdcfd3 HEAD --stat -- packages/app/src/components/BridgePaneTerminal.tsx packages/app/src/terminal \
  packages/app/src/hooks/usePaneViewportLease.ts packages/app/src/hooks/useVisualViewport.ts \
  packages/app/src/lib/nudge-scheduler.ts packages/app/src/hooks/useBridgePaneRegistry.ts packages/app/src/hooks/useTerminalSession.ts
-> (empty: zero lines changed)
git diff 5bdcfd3 HEAD -U0 -- packages/app/src/PwaApp.tsx | grep -E '^[+-]' | grep -E 'claimPaneSize|NudgeScheduler|requestNudge|onAgentPointer|fitGrid|usePaneViewportLease|leasedTransport|refreshSignal|BridgePaneTerminal|socketInput'
-> (empty)
```
`terminal/mobile-input-guard.ts` (Keys panel markup and handlers) is byte-identical. The only protected-area edits are the
declarations in `styles/terminal.css` (colours, radii, shadows, sizes, transition timings, caption casing) for the existing
`terminal-mobile-special-*` and `terminal-scroll-bottom` classes. The full `PwaApp.tsx` diff is: imports, `ph` class names,
the `stuck` and `toast` state, the `loop-signal` branch in the event handler, the toast effect, two `alert` to toast swaps,
the StuckNote mount and the `stuck` prop.

## 4. Overlap with other branches (file level)
- `PwaApp.tsx`, `components/mobile/*`, `lib/mobile-agent-tabs.ts`, `lib/bridge-sse.ts`, `lib/bridge-types.ts`: any other branch
  touching the phone.
- `styles/terminal.css`: the Keys panel and jump button blocks only.
- `dev/harness.tsx`, `dev/types.ts` (route union, `window`, `phone`, `phonePanes`), `scripts/evidence/screenshot.mjs`:
  shared evidence tooling, additive. A branch adding its own harness route will conflict textually in `bootHarness`.
- `styles/index.css` is not touched (`phone.css` is imported from `PwaApp.tsx`).

## 5. Limits / follow-ups
- The mock phone has no push service worker; the iOS hint is shown by spoofing the user agent in the scenario. The enabled and
  blocked toggle states are covered by `NotificationsToggle.test.tsx`, not by screenshots.
- The BridgePaneTerminal header row ("Fix login redirect 44x30, pid 4120") and the input box keep their current markup (off
  limits); they inherit the dark tokens but are not part of the v3 look. A follow-up with the terminal owner can restyle them.
- The Needs-you preview line for the Claude pane shows the TUI's last line ("3. No, tell Claude ..."), as the real
  `previewLines` heuristic would.
- Light theme is implemented through `[data-theme=light]` but has no screenshot; the phone does not set that attribute today.
- The v3 mock's agent page uses a composer and a Terminal/Receipt switch; those are cockpit features and were out of scope.
- Swipe-down works from the grabber and title bar only (not from the scrolled body), matching the mock.
- Sheets animate with CSS keyframes; the harness lands animations on their last frame.
