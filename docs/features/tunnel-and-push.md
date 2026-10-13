# Bring-your-own tunnel and actionable push notifications

Branch `feat/tunnel-push`. Two features: a stable public address for the phone, and push notifications
(with Allow / Deny) that reach the phone when the PWA is closed.

## 1. What and why

**Personas.** A (developers) have their own Cloudflare account or domain. B/C just want the phone to
keep working. A stable address means the phone pairs once: the pairing token lives in per-origin
`localStorage`, so a new `trycloudflare.com` URL at every launch forced re-pairing.

**Story.** "I put my agents to work, put the phone in my pocket, and get a notification when one needs
me. I tap Allow on the lock screen."

**Agent-agnostic.** Neither feature names an agent. Triggers use the four generic `PaneStatus` values,
the parsed prompt that each agent's profile already produces (`pane_prompt::detect`) and the agent
label from the registry (`tui::descriptors`, which includes TOML-defined agents).

## 2. Design

### Feature 1: tunnel setting

Verified facts: `mobile_tunnel.rs` only ever ran a quick tunnel; the "Public PWA URL" field was cosmetic
in Rust; only `PUPPET_MASTER_PUBLIC_URL` was honoured.

| File | Role |
|---|---|
| `src-tauri/src/tunnel_config.rs` | persisted setting + pure logic (modes, validation, plan, cloudflared args, health URL, log-line classifier) |
| `src-tauri/src/mobile_tunnel.rs` | uses the plan; `restart_mobile_tunnel`, `tunnel_status`, `test_connection`; generation counter so a dead tunnel's log readers never overwrite new state |
| `src-tauri/src/commands/tunnel.rs` | `get_tunnel_status`, `set_tunnel_config`, `restart_tunnel`, `test_tunnel_connection` |
| `src/components/settings/TunnelSection.tsx`, `tunnel-model.ts` | "Public address" section at the top of Settings > Mobile PWA |

- Modes: `quick` (default, unchanged), `named {token, hostname}`, `custom {url}`. Env var still wins
  (mode reported as `env`).
- **Secret handling.** `settings_store.rs` only whitelists `PATCHABLE_KEYS` and mirrors the result over
  SSE to every phone, so the tunnel config never goes through it. It lives in
  `<app data>/puppet-master.tunnel.json` (atomic write, 0600 on Unix), is returned to the UI only as
  `{mode, hostname, url, has_token}`, and has a hand-written `Debug` that redacts the token. The token
  reaches cloudflared via the `TUNNEL_TOKEN` env var instead of `--token <T>` (equivalent for
  `cloudflared tunnel run`, but not visible in the process list).
- Named status turns `running` on "Registered tunnel connection" and `error` on token errors or when
  cloudflared exits. Test connection fetches `<public>/bridge/health` (a public route) with a 10 s timeout and
  distinguishes "not Puppet Master" from "tunnel down" (HTTP 502/503/504/530).
- The tunnel only starts in release builds (unchanged `#[cfg(not(debug_assertions))]`); in a dev build the
  restart command says so.
- Known leftover: the old cosmetic `public_pwa_url` setting still overrides the QR origin in
  `MobilePairingPanel` when it is non-empty (existing behaviour, not touched).

### Feature 2: push

| File | Role |
|---|---|
| `src-tauri/src/push/vapid.rs` | P-256 keypair in `puppet-master.push-vapid.json` |
| `push/store.rs` | subscriptions keyed by `device_id` in `puppet-master.push.json` (own file); validation (https, no internal hosts, key shapes); dropped when the VAPID key changes |
| `push/triggers.rs` | pure: `Gate` (entered waiting_input/error, duplicate `prompt_id`, 30 s per pane, op dedupe), copy, 120-char truncation, `PromptInfo` |
| `push/hub.rs` | state: presence, Allow/Deny grants, `plan()` builds one payload per device that is not looking |
| `push/sender.rs` | encrypt + VAPID via `web-push-native`, POST via `ureq` |
| `push/routes.rs`, `bridge/routes/push.rs` | HTTP routes (pure logic + thin bridge adapter) |
| `push/mod.rs` | globals, hooks `notify_pane_status` / `notify_operation`, dispatch thread |
| `src/sw.ts`, `src/pwa/push-core.ts` | service worker + its pure logic |
| `src/lib/push-client.ts`, `components/mobile/NotificationsToggle.tsx`, `hooks/usePushPresence.ts` | toggle state machine, wiring, presence heartbeat, deep link |

**Hooks (one line each).** `bridge/sse.rs::push_pane_status_sse` -> `push::notify_pane_status`;
`push_operation_sse` (called at the end of `bridge/delegation.rs::publish_operation`) ->
`push::notify_operation`; `MobilePairingStore::revoke_device` -> `push::on_device_revoked`. Startup:
`push::init` in `lib.rs`.

**OSS.** `web-push-native` 0.4.0, MIT OR Apache-2.0, pure Rust (RustCrypto p256/aes-gcm/hkdf and
`jwt-simple` with `pure-rust`), no OpenSSL; it compiled and its tests ran on Windows here. Rejected:
`web-push` 0.11 (Apache-2.0, but its encryption backend and default HTTP client pull native
dependencies). `workbox-*` 7.4.1 (MIT) for the service worker (already transitive deps of
vite-plugin-pwa; now explicit dev dependencies).

**Routes** (bearer = a paired device; the device is whoever owns the token, never a request field):
`GET /push/key`, `POST /push/subscribe`, `DELETE /push/subscribe`, `POST /push/presence {visible}`;
and `POST /push/act {token, choice}` which is public (see below).

**Triggers.** A pane ENTERS `waiting_input` (after a 1.2 s settle; prompt read from the registry),
a pane ENTERS `error`, or a run operation reaches `completed` / `failed`. Title "<Agent label> needs you"
(or "hit an error" / "finished" / "failed"). Body: the prompt summary, else its text, truncated to 120
characters; else "Waiting for your input". Max one per pane per 30 s; the same `prompt_id` is not sent twice
while the pane keeps waiting. No raw terminal text beyond that summary.

**Suppression when the phone is looking.** The WS/SSE layer does not know visibility (and the SSE/WS
connection carries no device identity on every transport). So the PWA reports it: `POST /push/presence`
every 20 s while visible, and on show/hide. The server treats a device as looking for 45 s after the last
`visible: true`; a phone that dies stops suppressing on its own. Suppression is decided on the server, not in
the service worker, because iOS revokes subscriptions whose pushes show no notification.

**PWA.** `generateSW` -> `injectManifest` with `src/sw.ts`. Kept identical: same precache set (14 entries
including the 6 woff2 files, compared against a build of the old config), `cleanupOutdatedCaches`,
`skipWaiting` + `clientsClaim` (autoUpdate), navigation fallback to `index.html` with the same denylist.
One deliberate difference: the old `NetworkOnly` rule for `/bridge` used an anchored regex that never
matched a full URL (so it was dead code; unmatched requests go to the network anyway); the new rule matches
on the path. Tap: focus an existing window and post the pane id, or open `/?pane=<id>`; `PwaApp` switches to
that tab. The Menu sheet has the "Notifications" toggle; permission is requested first thing inside the tap.
iOS outside the home-screen app gets a plain hint and a disabled toggle.

**Allow / Deny: implemented.** Decision and justification:
- The notification payload (end-to-end encrypted to that browser) carries a **one-shot grant token** for
  exactly one prompt on one pane for one device. It is NOT the device bearer, expires after 10 minutes,
  is consumed on first use, and dies when the device is revoked.
- `POST /push/act` accepts only `allow` (mapped to `allow_once`) or `deny`; `allow_all` / broad approvals
  are impossible from a notification. It calls the existing `pane_prompt::answer`, which re-checks that the
  prompt currently on screen still has the granted `prompt_id` (else `STALE_PROMPT`).
- Buttons are offered only when the parsed prompt is a command approval (a `command` was extracted) whose
  choices include both `allow_once` and `deny`. Workspace-trust and other dialogs never get buttons (the
  project rule: trust must be accepted by the user).
- Residual risk: an accidental tap on Allow approves the shown command once. The body shows the command
  summary, and that is the same trust the phone already has (it may type into any pane).

## 3. Evidence

All commands run in `D:\pm-tunnel-push`.

```
cd packages/app/src-tauri && CARGO_BUILD_JOBS=4 cargo test --lib
  test result: FAILED. 1051 passed; 1 failed; 2 ignored   (only bridge::tests::definitions::approve_requires_app_session, known pre-existing)
cargo test --lib push::           -> test result: ok. 31 passed
cargo test --lib tunnel           -> test result: ok. 12 passed
npm test --workspace=@puppet-master/app
  Test Files  51 passed (51)   Tests  395 passed (395)
npx tsc --noEmit -p packages/app  -> clean (also: tsc --noEmit -p packages/app/tsconfig.sw.json, clean)
```

Rust tests added:
- Tunnel (`tunnel_config::tests`, `mobile_tunnel::tests`): persistence roundtrip, view/Debug never contain the
  token, token absent from the public settings mirror, origin normalisation, keep-stored-token, plan (env wins),
  cloudflared args hold no token, health URL and interpretation, `test_connection` against a live loopback server.
- VAPID key persistence: `push::vapid::tests::*` (created once, survives restart, damaged file replaced).
- Subscription store: `push::store::tests::*` (own file, per device, removal, rekey drops, pruning, SSRF validation).
- Trigger / dedupe / rate limit as pure functions: `push::triggers::tests::*` (entry edges, 30 s per pane, same
  prompt, terminal operations once, 120-char truncation by characters, copy, only command approvals actionable).
- Encryption round trip: `push::sender::tests::payload_round_trips_through_encryption` (build request, decrypt with
  the receiver key), `decrypts_the_rfc_8291_appendix_a_message` (the RFC test vector), and
  `vapid_header_is_signed_by_our_key_for_the_push_service_origin` (JWT verified, `aud` = push service origin).
- Presence/suppression, minimal payload keys, grants (single use, device-bound, expiry, revoke): `push::hub::tests::*`.
- Routes, in-process request -> response (`push::routes::tests::*`): all authed routes 401 without or with a wrong
  token; key -> subscribe -> (bad endpoint 400) -> unsubscribe; device B cannot touch device A's subscription; presence
  per device; `act` redeems once, allow/deny only. `mobile_pairing::tests::push_act_is_public_but_every_other_push_route_needs_a_bearer`.

Transcript (from `key_then_subscribe_then_unsubscribe_transcript`):
```
GET    /push/key         Bearer tok-a               -> 200 {"public_key":"B..."}
POST   /push/subscribe   Bearer tok-a, fcm endpoint -> 200 {"ok":true}
POST   /push/subscribe   Bearer tok-a, http://169.254.169.254/latest -> 400 invalid_subscription
DELETE /push/subscribe   Bearer tok-a               -> 200 {"ok":true,"removed":true}
GET    /push/key         (no bearer)                -> 401 device_token_required
```

**Fails before, passes after.** `push::tests::revoking_a_paired_device_drops_its_push_subscription` (real pairing
store + global hub). With the one-line hook in `MobilePairingStore::revoke_device` commented out:
`FAILED ... panicked at src\push\mod.rs:223:9` (the subscription survived a revoke); restored: `ok. 1 passed`.
(The other tests exercise new modules and cannot compile on the previous commit.)

Frontend tests: `TunnelSection.test.tsx` (model + component: save sends the token once and never renders it back,
test connection result), `lib/push-client.test.ts` (the toggle state machine: support ladder, iOS hint, permission
requested first, denied, dismissed, subscribe failure, disable with PC unreachable),
`components/mobile/NotificationsToggle.test.tsx` (tap on -> subscribe/send, off, blocked), `pwa/push-core.test.ts`
(payload parsing never throws, actions only with a token, click intent, deep links).

Service worker build check: `vite build` of the old and new config both precache 14 entries (6 woff2).

Screenshot: `D:\pm-night\evidence\tunnel-and-push\settings-mobile.png` (Settings > Mobile PWA, "Public address",
named tunnel online, Test connection result). Harness scenario `src/dev/scenarios/settings-mobile.ts`; mock commands
added in `dev/mock-tauri.ts`. The phone PWA is not covered by the harness (it renders only cockpit/settings), so the
Notifications toggle is evidenced by component tests only.

## 4. Limits and follow-ups

- **Not tested against a real push service, a real browser or a real Cloudflare tunnel** (no GUI/browser/network
  allowed overnight). Encryption is verified against the RFC vector and VAPID JWTs are verified locally. First real
  run should check FCM, Mozilla and Apple. The VAPID `sub` is `mailto:admin@puppet-master.example` by default
  (override `PUPPET_MASTER_VAPID_SUBJECT`); Apple may want a reachable contact.
- Rate limiting is literal: a second prompt on the same pane within 30 s produces no notification (it is not queued).
- Allow/Deny posts to `/bridge/push/act` on the PWA origin; a PWA configured with a bridge on a different origin
  would need an absolute URL in the payload.
- Named tunnel needs the user to create the tunnel and public hostname in Cloudflare (pointing at
  `http://127.0.0.1:1420`); we do not call the Cloudflare API. Token errors are detected from cloudflared logs.
- Subscriptions that the push service reports gone (404/410) are pruned; other failures are logged, not retried.
- `Cargo.lock` grew (jwt-simple pure-rust brings RustCrypto crates); expect a slower first build.
- Merge hot-spots: `lib.rs` (3 lines), `commands/mod.rs`, `bridge/routes/mod.rs`, `bridge/sse.rs`,
  `mobile_pairing/{mod,auth}.rs`, `vite.config.ts`, `tsconfig.json` (excludes `src/sw.ts`; own `tsconfig.sw.json`),
  `package.json` / `package-lock.json`, `PwaApp.tsx` (2 lines), `MobileSheet.tsx` (2 lines).
