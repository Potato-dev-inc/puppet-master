# Tunnel auth bypass: remote callers treated as the local app (2026-10-14)

Severity: critical (latent in the shipped proxy, live behind any forwarder pointed at the bridge).
Branch: `fix/tunnel-auth`. Fix commits: `a9f7fc3`, `9055cae` (tests `9b3a6cd`, refactor `182fa8f`).

## Backport to 0.1.5 (read this first)

The fix was written against the post-0.1.4 tree and backported to the shipped 0.1.4 code as
**0.1.5** (branch `release/0.1.5-security`). Everything below describes the original fix;
these are the differences in the backport, because 0.1.4 has a smaller bridge:

- 0.1.4 keeps the bridge in one file (`bridge.rs`) and the pairing code in one file
  (`mobile_pairing.rs`); the same functions (`read_request`, `gate`, `requires_pairing_token`)
  were added there. The rule, header list, Host and Origin checks are identical.
- 0.1.4 has no WebSocket events endpoint, no `/events/poll`, no `/pair/invite` and no
  `routes/definitions.rs`. So `pwa_server::proxy_upgrade`, the `pm-bearer` subprotocol branch of
  `gate`, the `is_local_app` change and the WebSocket and poll tests do not exist in 0.1.5. The
  `/events` SSE route and every other route already went through `authorize_bridge_request`,
  which is the function that changed.
- The `read_bridge_addr` fix (the proxy could never connect in 0.1.1 to 0.1.4) is included. Side
  effect: in 0.1.5 the `/bridge` proxy of the embedded PWA server actually reaches the bridge,
  and every proxied request is marked and needs a paired-device bearer.
- Version scope, corrected: the bridge trusted every loopback caller, with
  `Access-Control-Allow-Origin: *`, already in 0.1.0 (tag `puppet-master`, no pairing yet). So
  the "any web page in the browser can drive the bridge" route applies from 0.1.0 to 0.1.4, and
  it does not need the phone tunnel to be enabled, only the app to be running.
- Tests in 0.1.5: `src/tunnel_auth_tests.rs` has 11 end-to-end tests. Against the 0.1.4 trust
  rule they fail: 10 of 11 with the 0.1.4 proxy (which cannot connect), 8 of 11 with only the
  proxy address repaired (anonymous tunnel request answered 200 and treated as the local app).
  With the fix all 11 pass.

## Impact

The bridge (`bridge/*`, loopback HTTP on 17321-17399) decides whether a request needs a
paired-device bearer token with `mobile_pairing::requires_pairing_token(headers, peer_loopback)`.
Before the fix it returned true only when the peer was not loopback, or when the request carried
`X-PM-Proxied`. Everything else was "the desktop app": no token, and `is_local_only_caller` /
`definitions::is_local_app` true.

Any remote client that reached the bridge through a forwarder running on the same machine arrived
from `127.0.0.1` without `X-PM-Proxied`, so it was trusted as the local app. That means every
bridge route without a token: pane input and keys (`/panes/{id}/input`, `/key`), `answer_prompt`,
`shell_exec`, spawning agents, reading pane buffers, the `/events` SSE and WebSocket streams, and
`/pair/invite` (mint a pairing code for yourself and pair a device of your own, which makes the
takeover permanent). It is remote code execution on the user's machine for anyone who knows the
tunnel URL.

Two ways to get there:

1. **`pwa_server` (the release tunnel).** `proxy_bridge` and `proxy_upgrade` forwarded the client's
   headers and never added `X-PM-Proxied`. Only the Vite dev proxy (`vite-bridge-proxy.ts`) set it.
   Found by security review; confirmed by test (below).
2. **Anything else pointed at the bridge port**: ngrok, a custom domain's reverse proxy
   (`PUPPET_MASTER_PUBLIC_URL`; `normalize_bridge_segments` exists for exactly this), or a
   cloudflared started by hand with `--url http://127.0.0.1:<bridge port>`. Those forwarders add
   `X-Forwarded-For` / `Cf-Connecting-Ip` but not `X-PM-Proxied`, so they hit the same hole. Also,
   any web page open in the user's browser could `fetch("http://127.0.0.1:<port>/...")`: the
   bridge answers `Access-Control-Allow-Origin: *` and a loopback peer was always "the app" (DNS
   rebinding gave the same result).

## Affected versions

- `requires_pairing_token` with the `X-PM-Proxied`-only rule was introduced in `411048d`
  (2026-06-19, "qrcode pairing"), first released in tag **0-1-1**. Before that there was no
  pairing at all.
- `pwa_server` (and `mobile_tunnel`) was added in `db73182` (2026-06-21), first in tag **0-1-2**,
  and never added the header. It is in 0-1-2, 0-1-3 and 0-1-4-opencode-native.
- Honest caveat found while writing the regression test: in every one of those versions
  `pwa_server::read_bridge_addr` returned `http://host:port` and `TcpStream::connect` was called
  with it, which cannot resolve (`No such host is known`). So the `pwa_server` `/bridge` proxy
  never reached the bridge at all, and path 1 was latent (a dead proxy, not an open one). It
  became live the moment that bug was fixed, which the fix below had to do for the proxy to work.
  Path 2 (a forwarder pointed straight at the bridge) and the browser-page route were live in
  0-1-1 through current `refactor/tui-profiles`.
- The Vite dev flow (cloudflared/ngrok -> Vite -> bridge) was always safe: Vite sets the header.

## Fix (defence in depth, fail closed)

1. `pwa_server.rs`: both `proxy_bridge` and `proxy_upgrade` (WebSocket) drop any client-sent
   `X-PM-Proxied` (case-insensitive, whitespace tolerant, so an obs-fold line is caught too) and
   always add `X-PM-Proxied: 1`. Also fixed in the same file, because the proxy could not work
   otherwise: `read_bridge_addr` now returns `host:port`; the query string is no longer dropped
   (`/events/poll?cursor=...` would have lost its cursor).
2. `mobile_pairing/auth.rs::requires_pairing_token` is now true when the peer is not loopback, OR
   the request has any forwarder header (`X-PM-Proxied`, `Cf-Connecting-Ip`, `Cf-Connecting-Ipv6`,
   `Cf-Ray`, `Cf-Visitor`, `Cf-Ipcountry`, `Cdn-Loop`, `True-Client-Ip`, `X-Forwarded-For/Host/
   Proto/Port`, `X-Real-Ip`, `Forwarded`), OR a `Host` that is not loopback (a reverse proxy that
   forwards the public hostname, or DNS rebinding), OR an `Origin` that is not loopback (a web page
   in the user's browser). `localhost`, `*.localhost`, `127.0.0.0/8`, `::1`, and the webview's
   `tauri://localhost` / `http(s)://tauri.localhost` stay local. No `Host`/`Origin` (CLI, MCP shim,
   HTTP/1.0 tools) is local.
3. `bridge/routes/definitions.rs::is_local_app` now relies on that single function instead of its
   own, shorter header list.
4. `bridge/http.rs`: request reading and the auth step are now `read_request` and `gate`, and
   `handle_connection` runs `gate` first for every request, before any route, SSE or WebSocket
   handler (each handler still checks again).

### Audit of every user of the decision

| Site | Role | After the change |
|---|---|---|
| `bridge/http.rs` `/events`, `/events/poll`, `/events/ws` | `authorize_bridge_request` | Same function, now also behind `gate`. WS bearer via `pm-bearer.<token>` subprotocol is honoured in `gate` too. Covered by tests. |
| `bridge/router.rs::route` | `authorize_bridge_request`, then `paired_phone_typing` (token required and no MCP session) | Unchanged logic; stricter input. A forwarded caller needs a valid bearer before it gets this far. |
| `bridge/ws.rs::write_input` | `paired_phone_typing` for WS `input` frames | Same as above; only reachable after the upgrade was authorized. |
| `routes/pairing.rs::invite` | `is_local_only_caller` | Forwarded, foreign-Host and foreign-Origin callers get 403 `local_only`. |
| `routes/definitions.rs` (approve, revoke, consent, ...) | `is_local_app` | A paired phone is still refused (bearer present). Now also refused for any forwarder header. |
| `mcp_sessions::authorize` with no session | "legacy local MCP behavior" | Not a network gate; it relies on the checks above. Left as is. |

Local callers checked: the desktop webview (Origin `tauri://localhost`, Host = bridge), the Vite
dev proxy (sets the marker, i.e. remote), `packages/mcp-server` (fetch with
`X-Puppet-Master-Session`), `packages/cli` (fetch). None sends a forwarder header, a foreign Host
or a foreign Origin. A system HTTP proxy that adds `X-Forwarded-For` or similar to loopback calls
would now be asked for a token; `Via` was deliberately left out of the list for that reason.

## Tests

All in `packages/app/src-tauri` (`cargo test --lib`).

`src/tunnel_auth_tests.rs`: end to end, client -> real `pwa_server` -> the real bridge request
reader and `gate` (via `bridge::spawn_gate_listener`, on ephemeral loopback ports) with the real
pairing store holding one paired device. Only the `AppHandle`-bound route dispatch is stubbed
(a Tauri `AppHandle` cannot be built in a unit test); the stub reports `authorized`, `local_app`
(what the definitions/`/pair/invite` gate would conclude), the target, and the `X-PM-Proxied`
values it received.

- (a) no Authorization through the proxy to a pane route: 401
- (b) forged `X-PM-Proxied: 0` (several spellings) or no marker (pane input POST): 401
- (c) valid bearer through the proxy: 200, and `local_app` is false
- (d) direct loopback request, no forwarding headers: 200, `local_app` true
- (e) direct loopback with `Cf-Connecting-Ip`, `Cf-Ray`, `X-Forwarded-*`, `X-Real-Ip`, `Forwarded`,
  `X-PM-Proxied`: 401; with a bearer: 200 and not local
- WebSocket upgrade through the proxy without a token: 401 (also with a forged marker and with a
  bad `pm-bearer` subprotocol); with a good one: authorized
- `/events` and `/events/poll` through the proxy: 401 without a bearer, 200 with
- the proxy forwards exactly one `X-PM-Proxied: 1` whatever the client sent, and the query string
- foreign `Host` and web-page `Origin` on a loopback connection: 401; webview origins: 200
- `/health` stays public through the proxy

Unit tests in `mobile_pairing/tests.rs` cover the header list, Host and Origin parsing and the
local-tooling allow cases.

Before (sources of `fix/tunnel-auth` with only the fix lines undone: no marker in `pwa_server`,
`auth.rs` and `definitions.rs` from `182fa8f`): 10 of 13 fail, e.g. an anonymous tunnel request
returns `{"authorized":true,"local_app":true,...}`. Full output:
`D:\pm-night\evidence\fix-tunnel-auth\before-fix.txt`.

After: `test result: ok. 13 passed; 0 failed` (`after-fix.txt`). Full `cargo test --lib`: 1009
passed, 2 failed. Both failures also fail on the unchanged base commit:
`bridge::tests::definitions::approve_requires_app_session` (known; it asserts
`mcp_sessions::authorize(None, "GET", "/agents/types")` is an error, which the PWA's "new agent"
sheet needed to change) and `tui::declarative::tests::screen::visible_pane_launch_renders_tui_argv`
(expects `["--pure"]` and gets `[]`; unrelated to this change, looks environment-dependent).

## What to do now

1. Update to a build with this fix, then restart the app so the bridge and tunnel are rebuilt.
2. Revoke every paired device in the mobile pairing settings and pair your phone again. Tokens were never
   exposed by this bug, but an attacker could have used `/pair/invite` to add their own device,
   and a revoked list is the only way to be sure. If you cannot tell, revoke all.
3. Stop and restart the tunnel. A cloudflared quick tunnel URL is random per run, so a new URL
   makes the old one worthless; a custom domain or ngrok reserved domain should be rotated.
4. Look at pane history for commands you did not type, and at `shell_exec` use in the event log.
5. Do not point a reverse proxy or tunnel at the bridge port. Point it at the PWA server (port
   1420), which marks every request. The bridge now also refuses such setups without a token.

## Not fixed here / follow-ups

- Local trust is still "loopback and no forwarder, Host or Origin". Another local user or process
  on the same machine is trusted as the app (as before). A per-launch secret shared with the
  webview, CLI and MCP shim would close that, but changes every client.
- `packages/shared/src/mobile-pairing.ts::bridgeRequiresPairingToken` still encodes the old rule.
  It is unused outside its test.
- `Access-Control-Allow-Origin: *` on bridge responses is still sent. It is harmless for
  authorized routes now (a foreign Origin needs a token) but should be narrowed.
