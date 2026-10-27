# Device capabilities, sliding expiry, local secret (log-only) and CORS (P0-8)

Branch `sec/device-caps`. Roadmap P0-8 (S2 device caps, S3 log-only local secret, S8 defaults).
Background: `IDEAS-R3-RELIABILITY.md` §3, `docs/security/2026-10-14-tunnel-auth-bypass.md`.

## What and why

Before this branch a paired phone's bearer token never expired, had no scope (pane input, `shell_exec`,
spawning, settings, all of it) and received every pane's raw terminal bytes on `/events`, `/events/ws`
and `/events/poll`. The bridge also answered `Access-Control-Allow-Origin: *`, and any local process
was trusted as the app.

- **Persona A (Maya)** keeps a full-control phone she uses daily; nothing changes for it.
- **Persona C (Priya)** can pair a phone as "Watch and answer only": it sees agents and answers their
  cards, and it cannot type into a terminal, start or close agents, run commands or change settings.
- A lost phone that nobody uses signs out by itself after 30 days; a phone in use never does.

Product constraint (founder): existing paired phones must not lose anything. They migrate to **all**
caps, and new pairings default to **Full control**.

Agent-agnostic: nothing here knows about any agent. It is all bridge routing and pairing state.

## Design

### Caps (`mobile_pairing/caps.rs`)
`watch` (terminals), `answer` (approval and question cards), `type` (terminal input), `control`
(everything else that acts). `route_need(method, segments)` puts every bridge route in one class:

| Class | Routes (examples) |
|---|---|
| any paired device | `GET /`, `/panes`, `/settings`, `/agents/types`, `/pair/me`, the three event streams, `/push/*` (own subscription) |
| watch | every other `GET` (screens, buffers, transcripts, operations, events/recent, ...; `GET /panes/{id}/chat?since=<rev>` is listed exactly, and its text is redacted), `POST /panes/{id}/viewport`, `/orchestrator/viewport`, long-poll waits (`/agents/wait`, `/operations/{id}/wait`, `/panes/wait*`) |
| watch or answer | `GET /questions` |
| answer | `POST /questions/{id}/answer|dismiss`, `/agents/answer`, OpenCode permission and question replies, the push Allow/Deny tap |
| type | `POST /panes/{id}/input|key|resize`, WebSocket `input` frames |
| control | every other write: spawn, close, stop-all, `shell_exec`, run_agent, orchestrator message, settings, accounts, `/pair/invite`, any future route |

An unknown route fails closed: a `GET` needs `watch`, anything else `control`.

### Enforcement
- `mobile_pairing::authorize_caller` (the one decision, used by `gate`, the router, SSE, WS and poll)
  returns `Caller::{Local, Public, Device{id, caps}}`. A device without the cap gets **403**
  `{"error":"not_allowed","code":"DEVICE_NOT_ALLOWED","needs":"<cap>"}`; an expired one **401**
  `device_expired`. `authorize_bridge_request` keeps its signature for existing callers.
- **Fanout per device** (`bridge/sse.rs`): every SSE/WS client is registered with a `Viewer`. A device
  without `watch` gets each payload cut down by `caps::project` (review R12 2.1): no `event: terminal`
  bytes and no `event: chat` (the orchestrator transcript); `event: operation` keeps ids, status,
  `required_action` without `screen`/`raw_prompt`, and the error's code and message, but no `task`,
  `result`, `output_baseline`, `error.context` or any field not on the allowlist
  (`OPERATION_NO_WATCH_FIELDS`), and carries `"content_hidden": "needs_watch"`; `event: loop-signal`
  loses its `evidence_lines`. Structural events (pane list, status, geometry, settings, questions
  revision, turn summary, OpenCode worker status) pass unchanged, and an event name the list does not
  know is left out (fail closed: a new event must be classified in `caps.rs`). A revoked or expired
  device's stream is dropped on the next payload. Caps are read from a lock-free mirror (`stream_caps`), so the fanout never takes the
  store lock (revoke holds it while it talks to push). `/events/poll` filters the same way
  (`event_log::poll_for`, same projection), the cursor still advances past what a viewer may not see.
- WebSocket `input` frames re-check `type` per frame (`caller_may`), so a change applies at once. The
  typing path itself (`write_input`) is untouched.
- `POST /push/act` (public, one-shot token) now also needs the `answer` cap of the device the
  notification went to.
- The local app is never limited by caps.

### Pairing state (`mobile_pairing/mod.rs`)
Each device stores `caps`, `exp` (sliding: last use + 30 days) and `last_seen`. A valid token slides
`exp`; the file is written at most hourly per device. Old files (no `caps`, no `exp`) load with all caps
and a fresh 30-day window and are written back once. The pending pairing code carries the caps chosen
on the desktop (also persisted). The store file is now written with `secure_file::write_private`
(atomic, 0600 on Unix). `set_device_caps`, `is_active_device`, `device_caps`, `list_devices` (with
`caps`, `expires_at`, `last_seen`, `expired`, `idle_expiry_secs`) and `GET /pair/me` are new.

### Local secret, log-only (`mobile_pairing/local_secret.rs`)
At bridge start 32 random bytes (hex) are written to `puppet-master.bridge.secret` beside the port file
(written first, so a client that sees the new port sees the new secret). The port file format is
unchanged, so older readers keep working. The webview gets the key over IPC (`bridge_local_secret`) and
sends `X-PM-Local-Secret` to loopback bridges only; with the key it streams events over `fetch`
(EventSource cannot send headers). The MCP shim reads the file beside the port file it found
(`endpoint.rs::local_secret_header`) and sends it on every request. A local caller without it is
**logged** (`LOCAL_SECRET_MISSING`, once per route class per 10 minutes; proxied and tunnelled requests are Device/Public callers and are never counted here), counted (`bridge_local_secret_status`)
and still served.

### CORS
No more `Access-Control-Allow-Origin: *` (`bridge/http.rs`, `bridge/sse.rs`). Only the app's own
origins are echoed (`tauri://localhost`, `http(s)://tauri.localhost`, loopback dev servers) with
`Vary: Origin`; the allowed headers now include `X-PM-Local-Secret`. The phone PWA calls `/bridge/*` on
its own origin through `pwa_server` (or the Vite proxy), so it needs no CORS.

### Event hook
`mobile_pairing::events::{subscribe, emit}`: `DevicePaired`, `DeviceRevoked`, `DeviceCapsChanged`,
`DeviceExpired`, `CapDenied`, `LocalSecretMissing`. Only logs today; for the audit log (P0-9) to hook.

### UI
- Settings › Phone › Devices (`components/settings/blocks/PairedDevices.tsx`): one card per phone with
  Watch · Answer · Use the terminal · Start tasks switches, "Active · expires if unused for 30 days" (or
  "Signed out after 30 days without use…" with Remove), and Revoke. No raw ids.
- Pair a phone (`MobilePairingPanel.tsx`): "What can this phone do?" Full control (default) / Watch
  and answer only. A new choice makes a new code. The old device list with raw ids moved out.

## Evidence
See `D:\pm-night\p0\device-caps\REPORT.md` (commands, counts, fail-before log, screenshots, the
headless-browser CORS proof).

## Overlaps with other branches (file level)
- `bridge/ws.rs` (typing-permit): one check at the top of the `input` arm; `serve` takes a `Viewer`.
- `bridge/http.rs`, `bridge/sse.rs`: CORS and stream registration; `handle_sse` takes viewer and CORS.
- `lib.rs`: three command names and one `#[cfg(test)] mod` line. `push/mod.rs`: one line.
- `mcp_shim/{endpoint,bridge_http}.rs` (mcp-catalog): one header added; no tool change.

## Limits / follow-ups
- Fixed since this branch: the PWA reads `GET /pair/me` and hides its input box for a phone without
  `type` (3c81a90); push offers no Allow/Deny to a phone without `answer` (662ba33); the JS MCP server
  and the CLI send the local secret (cf2ea04); devices without `watch` get cut-down operation and
  loop-signal events and no chat (review R12 2.1); the four `/panes/wait` routes are listed exactly
  (2.2); an open stream ends when its device expires (2.3).
- Enforcing the local secret (refusing) is Phase 1 (1-18), once every client sends it.
- `GET` routes default to `watch`; a future read route that should be open to answer-only devices must be
  listed in `route_need`. Likewise a new event name reaches devices without `watch` only once it is
  classified in `caps::project` (unknown names are left out).
- An open SSE/WS stream does not count as use: a phone that only streams, with no request for 30 days,
  is signed out (the long poll makes a request every 25 s, so the tunnel path is unaffected).
