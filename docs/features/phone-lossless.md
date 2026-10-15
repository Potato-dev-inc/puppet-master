# Phone terminal: lossless snapshot to live handoff

Branch `feat/phone-lossless` (cut from `integration/night` 18c3d0f; backup `backup/feat-phone-lossless-0618`).
Status: built and tested. Not exercised against a real phone in this session (no windows or GUI were launched).

## 1. What and why

**Complaint.** "The chunk eating is still a thing, like a part of a thing may be cut off." Terminal output goes
missing: the start of a line is on screen, the rest is not.

**Personas.** A and B use the phone PWA to watch and steer agents away from the desk. C never sees this; the
terminal just has to be right.

**Already fixed (commit b369250).** The DESKTOP handoff: the PTY reader numbers every chunk with its byte offset
(`pty/stream_pos.rs`), `read_pane_screen_replay` returns the offset the snapshot was taken at (under the emulator
lock), `pty://resized` carries an offset, and `PaneStreamManager` writes only bytes at or after the snapshot offset,
in order with resizes.

**The remaining gap (root cause).** The PHONE path still dropped bytes at its handoff. A phone tab joins a pane in
three steps: subscribe to the live feed (buffering), fetch `GET /panes/{id}/screen`, then go live. The live feed
(WebSocket `/events/ws`, the SSE stream, and the `/events/poll` fallback) carried `terminal` chunks with NO position,
`/screen` returned `{b64, cols, rows}` with NO position, and `pane-resize` events had none either. So the phone could
not tell which buffered chunks the snapshot already drew, and `PaneStreamManager` fell back to
`discardBacklogAndGoLive`: everything that arrived while the snapshot crossed the tunnel (tens to hundreds of
milliseconds on a Cloudflare quick tunnel, and again for every retake) was thrown away. That happens on every phone
tab open, resume, resync, nudge and size-lease change, and a part of a line that straddled the cut is exactly "a part
of a thing cut off". The wrong-grid garbling that follows a dropped resize is the same bug.

A second, related hole: the WebSocket `watch` filter. Opening a tab asks the server to start forwarding that pane's
`terminal` events at the same time as the screen is requested; if the watch message lands after the snapshot,
output in between is never forwarded at all. Offsets make that visible, so it can be healed.

**Agent-agnostic.** Nothing here knows an agent. Offsets number the raw PTY byte stream of any pane.

## 2. Design

### Wire (Rust)
All offsets use the numbering of `pty/stream_pos.rs`: bytes since the pane started.

| Message | Before | Now |
|---|---|---|
| `event: terminal` (SSE, WS frame, poll body) | `{pane_id, b64}` | `{pane_id, b64, offset}` where `offset` is where the chunk starts |
| `event: pane-resize` | `{pane_id, cols, rows}` | `{pane_id, cols, rows, offset}` where `offset` is `stream_pos::emitted` (the resize falls after that many bytes) |
| `GET /panes/{id}/screen` | `{b64, cols, rows}` | `{b64, cols, rows, offset}`; `offset` is `processed`, read under the emulator lock together with the snapshot |

`offset` is omitted when the producer does not number its stream (native OpenCode panes, which emit `terminal-data`
without `stream_pos`). WS, SSE and long-poll all push the same strings through `push_sse`, so one change covers
all three.

- `pty/stream_pos.rs`: new `StreamPos::feed`, the PTY reader's per-chunk step (emit with the start offset, advance,
  feed the emulator and store `processed` under its lock). `spawn.rs` calls it, so the production reader and the tests
  run the same code.
- `bridge/sse.rs`: `terminal_payload(.., offset)`, `push_terminal_sse(.., offset)`, `pane_resize_payload` /
  `push_pane_resize_sse(.., offset)`.
- `lib.rs`: the `terminal-data` listener forwards the event's `offset` to the bridge.
- `bridge/mod.rs::notify_pane_resized`: stamps the resize with `stream_pos::emitted`.
- `bridge/routes/panes.rs`: `screen_response` / `registry_route` (a route that needs only the registry, so it runs
  in-process in tests); `GET .../screen` returns the offset.

### Phone (TypeScript)
The phone already shares `PaneStreamManager` with the desktop (`useBridgePaneRegistry`), so the lossless rule is not
duplicated: it is the same code, now fed offsets.

- `lib/bridge-sse.ts`: parses `offset` on `terminal` and `pane-resize` (`streamOffset`, ignores anything that is not a
  finite non-negative number). `lib/bridge.ts::readScreen` returns `offset`. `lib/bridge-types.ts`: optional `offset`.
- `hooks/useBridgePaneRegistry.ts`: `ingestTerminalData(.., offset?)`, `ingestPaneResize(.., offset?)`, and an optional
  `onStreamGap` callback. `PwaApp.tsx` and `PwaLegacyApp.tsx` pass `ev.offset` through.
- `terminal/pane-stream.ts` (additive): 
  - **Handoff gap check.** Chunks buffered while the screen loaded must join the snapshot end without a hole. If the
    first one starts after the snapshot offset, output in between never reached the phone (the late `watch`), so the
    screen is fetched again (the existing retake loop, at most 3 attempts) instead of going live with a hole.
  - **Live gap detection.** `PaneStreamManager({ onGap })` reports a live chunk that starts after the end of the last
    one. `PwaApp` answers with the existing nudge (`requestNudge(true)`: redraw from the PC, never while the user
    reads history). The nudge rules and the lease code are untouched.
- Backwards compatibility: without offsets (older backend) every new branch is skipped and the code runs exactly as
  before (`discardBacklogAndGoLive`). A new phone build against an old backend, or the reverse (the extra JSON field is
  ignored), keeps today's behaviour.

### What did not change
Lease semantics, nudge timing and rules, the MCP catalog, `usePaneTunnel` (orchestrator mirror, a different stream).

### OSS
None added.

### Overlaps with other branches (file level)
`bridge/sse.rs`, `bridge/mod.rs`, `bridge/routes/panes.rs`, `lib.rs` (one listener line), `pty/registry/spawn.rs`
(reader loop), `pty/stream_pos.rs`, `terminal/pane-stream.ts`, `hooks/useBridgePaneRegistry.ts`, `PwaApp.tsx`,
`lib/bridge-sse.ts`. Hot spots: `PwaApp.tsx` (restyle branches) and `spawn.rs` (the reader).

## 3. Evidence

All commands run in `D:\pm-phone-lossless`, `CARGO_BUILD_JOBS=4`.

### Before / after: the phone test
`packages/app/src/hooks/phone-handoff.test.tsx`. A fake PC runs a real xterm emulator and speaks the bridge's wire
shapes (`terminal` / `pane-resize` SSE payloads in a WebSocket frame or a long-poll body, and `/screen`). The phone
side is production code: `makeBridgeClient`, `subscribeBridgeEventsHandle` / `subscribeBridgeEventsViaPoll`,
`useBridgePaneRegistry` + `PaneStreamManager`. The viewer's xterm is compared line by line with the PC's. Cases (each
over the WebSocket feed and the long-poll feed): a snapshot taken mid-stream with chunks in flight (one cut mid-line,
one arriving before the snapshot is taken, one just after the reply); a lease resize in flight; a resize before the
snapshot that must not be applied twice; output that never arrived between snapshot and live (late watch); the
fallback without offsets; a hole in live numbering.

Before (previous phone code, the same test file, source changes stashed):
```
 × ... over the socket feed > writes every byte after the snapshot exactly once
 × ... over the socket feed > applies a lease resize in flight in order with the output
 × ... over the socket feed > retakes the screen when output between the snapshot and going live never arrived
 × ... over the poll feed   > writes every byte after the snapshot exactly once
 × ... over the poll feed   > applies a lease resize in flight in order with the output
 × ... over the poll feed   > retakes the screen when output between the snapshot and going live never arrived
 × live stream > reports a hole in the numbering so the viewer can redraw from the PC
 ✓ does not apply a resize the snapshot already shows a second time (x2), ✓ falls back without offsets (as today)
 Tests  7 failed | 3 passed (10)
 AssertionError: expected [ 'before 0', 'before 1', ...(21) ] to deeply equal [ 'before 0', 'before 1', ...(24) ]
```
(the viewer is missing three lines: `during B`, `during C ...`, `after resize D`). The resize test fails on an extra screen
fetch (a retake that is no longer needed; each retake is another unprotected window), not on missing text, because in
the fake the retake is quick. Full logs: `D:\pm-night\phone-lossless-before.txt`, `phone-lossless-after.txt`.

After:
```
 ✓ src/hooks/phone-handoff.test.tsx (10 tests)
 Tests  10 passed (10)
```

### Rust
Specific new tests, all passing:
- `bridge::sse::tests::terminal_payload_carries_the_chunk_offset_when_the_reader_numbered_it` (and the unnumbered case)
- `bridge::sse::tests::pane_resize_payload_carries_the_stream_offset_it_falls_after`
- `bridge::sse::tests::offsets_survive_the_event_log_that_feeds_long_polling` (the poll fallback replays the same strings)
- `pty::stream_pos::tests::feed_numbers_chunks_by_their_start_and_advances_both_counters`
- `pty::stream_pos::tests::a_snapshot_offset_is_the_exact_point_the_snapshot_was_taken_at`: a reader thread feeds
  numbered chunks (boundaries mid-line) while the main thread takes `read_screen_replay_at` snapshots; each snapshot
  replayed into a fresh emulator must show exactly the first `offset` bytes of the stream. Negative control: with
  the offset read from `emitted` instead of `processed` (not under the lock) the test failed 3 of 3 runs
  ("snapshot at offset 210 does not show the first 210 bytes").
- `bridge::tests::screen_route::*`: router transcript, no app handle needed:
```
GET /panes/screen-route-pane/screen      (after the reader fed "hello\r\nwor" at 0, then "ld" at 10)
 -> 200 {"b64": "...", "cols": 10, "rows": 3, "offset": 12}      replay shows "hello\nworld"
(another chunk "!")
GET /panes/screen-route-pane/screen      -> 200 {..., "offset": 13}
GET /panes/nope/screen                   -> 404 {"error": "..."}
POST /panes/screen-route-pane/screen     -> not this route (None)
GET /panes/screen-route-unnumbered/screen -> 200 {"b64","cols","rows"}   (no "offset" key)
```

### Full checks
```
cargo test --lib            test result: ok. 1439 passed; 0 failed; 2 ignored
cargo test --bins           puppet-master-mcp: ok. 33 passed; 0 failed   |   main.rs: 0 tests
npm test --workspace=@puppet-master/app   Test Files 100 passed (100) | Tests 864 passed (864)
npx tsc --noEmit -p packages/app          (no output)
```
MCP catalog untouched (no `tool_registry/*` change). Logs: `D:\pm-night\phone-lossless-cargo-lib.txt`, `-cargo-bins.txt`.

## 4. Limits and follow-ups

- Not tried on a real phone or tunnel; the tests model the timing with gates, not wall-clock latency.
- The resize offset is `emitted` at the moment the bridge notices the resize, as on the desktop. A chunk the reader is
  emitting at that instant can land on the wrong side of it; the byte boundary of a resize inside ConPTY's pipe is not
  observable anyway.
- Live chunks are not de-duplicated by offset (only the join trims). A pane that restarted with the same id would
  otherwise have its fresh output dropped until it passed the old offset. A duplicate would need two feeds writing
  at once, which the code never does (the poll loop stops when the socket opens).
- The healing nudge is skipped while the user reads history, so a hole can stay until the next nudge or reopen.
- `usePaneTunnel` (the orchestrator mirror) has its own stream and is not part of this.
- `stream_pos` entries are never removed when a pane is killed (a few bytes per pane id; pre-existing).
