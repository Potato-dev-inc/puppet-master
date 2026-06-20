# Mobile Terminal Bugs and Fixes

This document records several hard-won bugs encountered while building Puppet Master's mobile PWA terminal mirror — a phone that views and types into a desktop-owned PTY over a bridge tunnel — and how the current codebase addresses them.

## Architecture context

```
Desktop (owner)                    Bridge tunnel                 Mobile PWA (mirror)
─────────────────                  ─────────────                 ───────────────────
Tauri PTY + xterm.js    ──SSE──►   terminal-data events   ──►   xterm.js or CanvasTerminal
resize drives cols/rows            pane dimensions               syncPTYResize = false
                                   terminal-snapshot (orch.)     CSS scale / snapshot render
```

On desktop, the terminal **owns** the PTY: resizing the pane changes `cols × rows` and the shell reflows. On mobile, the terminal **mirrors** the same PTY without resizing it (`syncPTYResize: false`). Input travels back over the bridge; output travels forward as raw bytes (agent panes) or vt100 snapshots (orchestrator).

---

## 1. Terminal corruption and duplicate characters

Several distinct duplication/corruption paths were fixed at different layers.

### 1a. UTF-8 corruption at PTY read boundaries (backend)

**Symptom:** Garbled characters, replacement glyphs (), or broken CJK/emoji in scrollback and replay.

**Cause:** The PTY reader decoded each 4 KiB chunk to `String` with `from_utf8_lossy`. Multi-byte UTF-8 sequences split across chunk boundaries were replaced with U+FFFD, permanently corrupting stored output.

**Fix:** `Scrollback` now stores **raw bytes** per line and only decodes when producing human-readable text for MCP/LLM reads. PTY events emit raw `Vec<u8>` to the frontend renderer.

Key files:
- `packages/app/src-tauri/src/pty/scrollback.rs`
- `packages/app/src-tauri/src/pty/registry.rs` (reader loop)

### 1b. TUI redraw duplicate lines (LLM scrollback)

**Symptom:** Progress spinners and TUIs (e.g. `Working…` redrawn in place) produced repeated identical lines in scrollback text.

**Cause:** ANSI clear-line sequences (`\x1b[2K`) erase visually but leave multiple content lines in the byte stream.

**Fix:** `strip_ansi` collapses consecutive identical trimmed lines when producing LLM-readable scrollback.

Key file: `packages/app/src-tauri/src/pty/ansi.rs`

### 1c. Partial-frame flicker from small PTY chunks (frontend)

**Symptom:** Visual tearing or flicker when many small writes arrived in one frame.

**Fix:** `WriteBatcher` coalesces PTY byte chunks and flushes once per animation frame via `mergeChunks`.

Key file: `packages/app/src/terminal/write-batcher.ts`

### 1d. Duplicate typed characters on mobile mirror (echo dedup)

**Symptom:** Every keystroke on the phone appeared twice — once immediately, again when the PTY echoed the input back over the tunnel.

**Cause:** Mobile mirror viewers need low-latency local feedback, but the same bytes also arrive inbound from the remote PTY echo.

**Fix:** A two-part echo pipeline:

1. **`applyMirrorLocalEcho`** — Renders outbound keystrokes immediately in xterm (including wide-character and backspace visual erase `\b \b`).
2. **`MirrorEchoFilter`** — Tracks pending outbound text and skips matching inbound PTY echo. Also handles backspace echo patterns (`\b`, `\b \b`, `\x08 \x08`) and refuses to visually erase prompt content that only exists from remote output.

Inbound data path in `TerminalSession`:

```ts
if (this.mirrorPTY && isMobileInputDevice() && this.mirrorEchoFilter.shouldSkipInbound(data)) {
  return; // skip duplicate echo
}
```

Key files:
- `packages/app/src/terminal/mirror-local-echo.ts`
- `packages/app/src/terminal/mirror-echo-filter.ts`
- `packages/app/src/terminal/terminal-session.ts`

---

## 2. Mobile keyboard autofill and autosuggest

### Problem

Feeding keystrokes directly into xterm's hidden `<textarea>` (`terminal.onData`) does **not** engage the mobile OS keyboard stack: no autocorrect, no autocomplete bar, no suggestion chips, poor IME behavior for CJK.

Mobile browsers only expose those features to **real, visible HTML form controls**.

### Solution: `MobileInputGuard`

A dedicated command field sits beside (or above) the terminal:

- **`<textarea>` in a `<form>`** with `autocomplete="on"`, `autocorrect="on"`, `spellcheck="true"`, `inputmode="text"`.
- xterm's internal textarea is **disabled for mobile mirror** via `configureXtermTextareaForMobileMirror` (`readOnly`, `pointer-events: none`, `aria-hidden`).
- Keystrokes are **buffered** (default 250 ms, configurable in Settings → Mobile PWA) and committed to the PTY as a delta, not character-by-character.
- **`buildInputDelta(previous, next)`** computes the minimal PTY payload: shared prefix preserved, deletions as `\x7f` repeats, insertions appended.
- **Autosuggest / autocorrect** arrives as `insertReplacementText` or `insertFromSuggestion` input events. `normalizeSuggestionText` dedupes doubled suggestions (`"much much"` → `"much"`). `stripCjkImeSpaces` removes IME padding spaces between CJK characters that shells would not echo.
- **Long-press** on the terminal surface reveals/focuses the command field when the input bar is hidden.
- **Special keys grid** (Ctrl-C, arrows, Tab, etc.) for keys the soft keyboard cannot send.

Key files:
- `packages/app/src/terminal/mobile-input-guard.ts`
- `packages/app/src/terminal/word-replacement.ts`
- `packages/app/src/components/settings/settings-tab-panels.tsx` (`mobile_input_delay_ms`, `mobile_input_visible`)

### Hidden input mode

When "Show mobile input box" is off, the textarea is moved to a **keyboard sink zone** (`terminal-mobile-keyboard-sink-zone`) — a nearly invisible fixed strip at the top of the viewport that still captures the OS keyboard and autofill UI without occupying terminal space. A tap/long-press on the terminal engages it (`terminal-host--mobile-input-engaged`).

---

## 3. Scaling when mobile and desktop share one tunnel

### Problem

If the mobile viewer resized the PTY to fit the phone screen, two bad things happened:

1. **Layout reflow** — The shell/TUI on desktop reflowed to the phone's column count, breaking the desktop view.
2. **Competing resize authority** — Desktop sidebar resize and mobile viewport resize fought over `cols × rows`, causing constant remounts and garbled state.

### Solution: mirror without resizing the PTY

| Concept | Implementation |
|--------|----------------|
| Desktop owns resize | `syncPTYResize: true` on desktop; `onResize` updates the real PTY |
| Mobile never resizes PTY | `syncPTYResize: false` on PWA / bridge panes |
| Same grid, smaller presentation | `renderMode: 'mirror-same-grid'` — xterm keeps desktop `cols × rows`, `TerminalScaleController` CSS-scales the stage to fit the phone viewport |
| Fit local viewport (orchestrator alt.) | `renderMode: 'mirror-fit-local'` — fits xterm to local size without notifying PTY |
| Debounced dimension changes | `useDebouncedMirrorLayout` (300 ms) avoids remount storms when desktop resizes |
| Session remount key | `mirrorLayoutSessionKey(createdAt, cols, rows)` remounts only when PTY dimensions actually change |
| PTY authority width | `useTerminalAuthorityCss` sets `--pm-pty-authority-width` from desktop sidebar or active mobile orchestrator viewport |
| Stable width during keyboard | `useVisualViewportSync` pins `--pm-stable-layout-width` while keyboard is closed; `containerFitDimensions` uses it while keyboard is open so scale does not jump when the visual viewport narrows |

### Orchestrator: snapshot rendering instead of byte replay

For the orchestrator pane specifically, replaying raw PTY bytes on mobile was fragile — bytes reflow when desktop resizes mid-stream.

**Fix:** `useOrchestratorSnapshotMirror` renders **vt100 screen snapshots** at the phone's local viewport size via `CanvasTerminal` + `SnapshotBatcher`, with local input overlaid on the last snapshot line (`snapshotWithLocalInputOverlay`).

Key files:
- `packages/app/src/hooks/useOrchestratorSnapshotMirror.ts`
- `packages/app/src/terminal/canvas-terminal.ts`
- `packages/app/src/terminal/scaled-viewport.ts`
- `packages/app/src/hooks/useTerminalAuthorityCss.ts`
- `packages/app/src/hooks/useVisualViewport.ts`
- `packages/app/src/lib/layout-viewport.ts`

### Scale controller behavior

`TerminalScaleController`:

- Measures the full xterm canvas (`measureTerminalContent`).
- Computes uniform `scale` via `computeContainerFitScale` (never upscale past 1×).
- While keyboard is open: `ignoreHeight: true` (don't shrink further), `pushUp: true` with `maxPushUpPx` from `getKeyboardObscuredPx()` to translate content upward above the keyboard.
- Listens to `ResizeObserver`, `visualViewport` resize/scroll, and `pm:terminal-authority-changed`.

---

## 4. Bottom of terminal not visible with input box

### Problem

Adding a visible command input bar at the bottom caused the terminal's last lines to sit **behind** the input panel or the software keyboard. Users could not see what they were typing or the shell prompt.

### Solution: layered layout + viewport-aware insets

**Flex column layout** (`terminal-host--mobile-input`):

```
┌─────────────────────────┐  order: 1, flex: 1, min-height: 0
│  Terminal / scale stage │
├─────────────────────────┤  order: 2, flex: 0  (--pm-mobile-input-bar-height)
│  Command input form     │
└─────────────────────────┘
```

CSS variables:
- `--pm-mobile-keyboard-panel-height` — measured from `visualViewport` (fallback `42dvh`)
- `--pm-mobile-input-bar-height` — tracks panel height; `0px` when input is hidden

**Keyboard height sync** (`installKeyboardHeightSync` in `MobileInputGuard`):

- On focus, listens to `visualViewport` resize/scroll.
- Sets `--pm-mobile-keyboard-panel-height` to the obscured pixel height when keyboard ≥ 100 px.
- Triggers `scrollToCursor` on each update.

**Scroll pinning** (`scrollToCursor` in `TerminalSession`):

- Calls `terminal.scrollToBottom()` then force-scrolls `.xterm-viewport` to `scrollHeight` across two animation frames on mobile mirror.

**Tap zone inset:**

- `terminal-mobile-tap-zone` bottom edge is `var(--pm-mobile-input-bar-height)` so taps pass through to xterm above the input bar, not behind it.

**Scale push-up:**

- When keyboard is open, `TerminalScaleController` applies negative `translateY` so scaled terminal content shifts up by the keyboard overlap amount.

**Special keys FAB position:**

- `bottom: calc(var(--pm-mobile-input-bar-height) + 12px)` keeps the Keys button above the input bar.

Key files:
- `packages/app/src/styles/index.css` (`.terminal-host--mobile-input`, `.terminal-mobile-input-zone`, `.terminal-mobile-tap-zone`)
- `packages/app/src/terminal/mobile-input-guard.ts` (`installKeyboardHeightSync`)
- `packages/app/src/terminal/scaled-viewport.ts` (`isMobileKeyboardTransitionActive`, `computeContainerFitTransform`)

---

## Settings reference

| Setting | Default | Effect |
|---------|---------|--------|
| Mobile input buffer (ms) | 250 | Delay before buffered keystrokes commit to PTY; also debounces autosuggest |
| Show mobile input box | on | Visible command bar vs. hidden keyboard-sink mode |

---

## Key source index

| Area | Primary files |
|------|---------------|
| Echo dedup | `mirror-echo-filter.ts`, `mirror-local-echo.ts` |
| Mobile input | `mobile-input-guard.ts`, `word-replacement.ts` |
| CSS scale | `scaled-viewport.ts`, `index.css` |
| Session lifecycle | `terminal-session.ts`, `useTerminalSession.ts` |
| Bridge mirror | `BridgePaneTerminal.tsx`, `usePaneTunnel.ts` |
| Orchestrator mirror | `useOrchestratorSnapshotMirror.ts`, `canvas-terminal.ts` |
| Backend bytes | `scrollback.rs`, `registry.rs`, `ansi.rs` |
| Viewport stability | `layout-viewport.ts`, `useVisualViewport.ts` |

---

## Testing

Regression coverage lives in:

- `packages/app/src/terminal/mirror-local-echo.test.ts` — echo, wide chars, backspace guard
- `packages/app/src/terminal/mobile-input-guard.test.ts` — autosuggest, buffer, hidden input, keyboard height
- `packages/app/src/terminal/scaled-viewport.test.ts` — scale math, keyboard transition detection
- `packages/app/src/lib/layout-viewport.test.ts` — stable width during keyboard
- `packages/app/src-tauri/src/pty/ansi.rs` — TUI dedup
- `packages/app/src-tauri/src/pty/scrollback.rs` — raw byte line assembly
