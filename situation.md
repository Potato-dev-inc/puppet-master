
### Project context

**Repo:** `tmux-puppet-master` — monorepo with Tauri desktop app + mobile PWA sharing tmux panes over an HTTP bridge.

**Relevant paths:**
- `packages/app/src/terminal/mobile-input-guard.ts` — mobile keyboard interception
- `packages/app/src/terminal/terminal-session.ts` — wires xterm, `MobileInputGuard`, `OutboundTypingBuffer`, `MirrorEchoFilter`, `InputBatcher`
- `packages/app/src/terminal/outbound-typing-buffer.ts` — tracks current word already sent to PTY
- `packages/app/src/terminal/word-replacement.ts` — `resolveReplacementLength`, replacement input types
- `packages/app/src/terminal/mirror-local-echo.ts` — local echo for mobile mirror mode
- `packages/app/src/terminal/mirror-echo-filter.ts` — dedupes PTY echo vs local echo
- `packages/app/src/components/BridgePaneTerminal.tsx` / `PwaApp.tsx` — mobile uses `syncPTYResize: false` (mirror mode)

**Mobile mirror mode behavior:**
- Keystrokes go: xterm hidden textarea → `terminal.onData` → local echo + `InputBatcher` → bridge → PTY
- PTY output comes back via SSE; `MirrorEchoFilter` skips duplicate echo
- Mobile does **not** resize the real PTY (desktop owns dimensions)

---

### Current bugs (still reproducing on phone)

1. **Backspace fails on mobile** — backspace works on desktop PTY but does not reliably erase on the phone display / may not reach `OutboundTypingBuffer` or PTY correctly.

2. **Word replacement doubles text** — typing `sufh` and tapping suggestion `much` produces something like `sufh much much` instead of `much`.

---

### What was already tried (partial fix — may be wrong approach)

We added:
- `OutboundTypingBuffer` to track chars sent to PTY (not xterm textarea value, which stays empty)
- `MobileInputGuard` listening to `beforeinput` in **capture phase**
- On suggestion: `flushPending()`, compute replacement length from outbound token, send `\x7f`×N + word to PTY, `terminal.input()` locally, call **`preventDefault()` + `stopImmediatePropagation()`** on success
- Bypass path in `onData` for multi-char inserts that skip `beforeinput`
- `normalizeSuggestionText()` to dedupe `much much`

**49 unit tests pass**, but **real mobile keyboards still fail**.

---

### Suspected root causes (user hypothesis — validate and fix)

Mobile virtual keyboards treat backspace and autocorrect as **text composition mutations**, not discrete key events.

1. **Backspace:** Mobile often fires `beforeinput` / `input` with `inputType: "deleteContentBackward"` instead of reliable `keydown` / `onData` with `\x7f`. Current code mostly relies on `terminal.onData`; deletion intents may never update `OutboundTypingBuffer` or PTY.

2. **Word replacement duplication:** Calling `preventDefault()` on `insertReplacementText` / suggestions **blocks the browser from updating** `.xterm-helper-textarea`. The keyboard expects native replacement in the DOM. When blocked, its internal composition buffer desyncs and it **flushes the suggestion again** (`much much`) as a recovery behavior.

**Conclusion:** `preventDefault()` on replacement events is likely **too aggressive** and is part of the problem, not the solution.

---

### Required architectural shift: “Let it happen, then clean up”

Restructure `MobileInputGuard` around how mobile browsers handle virtual keyboard mutations:

| Event | Old approach (suspected broken) | New approach |
|--------|----------------------------------|--------------|
| `deleteContentBackward` | Rely on `onData` / key events | Intercept in `beforeinput`, `preventDefault()`, explicitly send `\x7f` to PTY + update outbound buffer |
| `insertReplacementText` / `insertFromSuggestion` | `preventDefault()` in capture, synthesize replacement | **Do NOT `preventDefault()`** — let keyboard mutate textarea natively |
| After replacement | Block native path | Handle in **`input` event**: read `textarea.value`, diff vs `OutboundTypingBuffer`, send `\x7f`×N + new word to PTY, update buffer, **clear textarea in `requestAnimationFrame`** |

**Why rAF clear:** Gives the OS/keyboard time to finish its input cycle before resetting the hidden field for the next word.

---

### Implementation sketch (adapt to existing code — do not blindly copy)

```typescript
// beforeinput
if (e.inputType === 'deleteContentBackward') {
  e.preventDefault();
  handleBackspace(); // send \x7f, outboundBuffer pop
} else if (insertReplacementText | insertFromSuggestion) {
  // DO NOT preventDefault — let keyboard update textarea
}

// input
if (replacement inputType OR textarea.value.length > 1) {
  handleMobileWordReplacement();
  // incomingText = textarea.value
  // eraseLength = outboundBuffer.getToken().length
  // payload = '\x7f'.repeat(eraseLength) + incomingText
  // onInput(payload) + local xterm update + outboundBuffer commit
  // requestAnimationFrame(() => textarea.value = '')
}
```

**Integration constraints:**
- Keep working with existing `TerminalSession` wiring (`flushPending`, `sendInput`, `setMuteOnData`, `MirrorEchoFilter`, local echo via `applyMirrorLocalEcho`)
- Avoid double-sending: when handling replacement in `input`, mute `onData` / skip batcher for that mutation
- `OutboundTypingBuffer` may need `pop()` / `set()` helpers if not present
- Preserve desktop behavior — changes only for `isMobileInputDevice()` + mirror mode
- Update/add tests in `mobile-input-guard.test.ts`, `word-replacement.test.ts`, `outbound-typing-buffer.test.ts`

---

### Success criteria

On mobile PWA (iOS Safari / Chrome + system or Gboard keyboard):

1. Backspace visibly erases on phone **and** on shared PTY/desktop.
2. Type `sufh` → tap `much` → result is **`much` only** (no `sufh`, no `much much`).
3. Normal single-character typing still works with local echo and no duplicate PTY echo.
4. All existing terminal unit tests pass; add tests for `deleteContentBackward` and post-`input` replacement flow.

---

### What NOT to do

- Do not re-enable `autocorrect=off` on the textarea (suggestions must stay enabled).
- Do not resize PTY from mobile (`syncPTYResize: false` must stay).
- Do not use `preventDefault()` on `insertReplacementText` unless you have strong evidence it’s safe on iOS/Android.
- Minimize scope — focused changes to mobile input guard + wiring, not unrelated refactors.

---

### Deliverables

1. Refactored `MobileInputGuard` per the mutation-based pattern above.
2. Any small `OutboundTypingBuffer` / `terminal-session.ts` wiring updates needed.
3. Tests covering backspace + word replacement flows.
4. Brief comment in `mobile-input-guard.ts` explaining why we avoid `preventDefault` on replacement events.

---