# Live preview

Branch `feat/approval-cards`, feature 2 (separate commits after the approval cards).

## What and why
Vibecoders (persona B) live in the preview, not the terminal. When an agent starts a dev server, the pane shows a quiet "Preview :5173" chip that opens it in the browser, the sidebar row gets a small globe, and the command palette gets "Open preview: <agent> :5173".

Agent-agnostic: detection reads only the visible screen tail (`http(s)://localhost|127.0.0.1|[::1]|0.0.0.0:PORT/path`, plus private-LAN addresses such as Vite's "Network: http://192.168.x.x:5173"). No agent name is matched anywhere.

## Design
- `src-tauri/src/pane_preview.rs`: `detect_urls` (regex; one entry per port; loopback preferred, the LAN address kept as `networkUrl`; `0.0.0.0` shown as `localhost`; public hosts are ignored on purpose, so a docs link in the output is never offered as a preview), `observe` (merge into the pane's list: new ports to the front, known ports keep their place, max 3, emit only when the list changed), `retain_alive` (drops panes that are gone or whose process exited and emits an empty list), `snapshot`.
- Throttle reuse, nothing in the PTY reader: `pane_activity::tick_with` calls `pane_preview::observe(id, tail)` right after it decided to recompute a pane (dirty, and its screen hash changed). So the regex runs on a 24-line tail at most once per second per pane. `pane_activity::init` installs the sink (Tauri event `pty://preview {pane_id, urls}` and SSE `pane-preview`) and runs `retain_alive` each tick.
- Channels: Tauri command `pane_previews` (current map), event `pty://preview`, SSE `pane-preview`, and the read-only bridge route `GET /previews` (`bridge/routes/previews.rs`, so a phone that connects later can read the current list). MCP catalog untouched.
- Cockpit: `lib/pane-preview.ts` (`usePanePreviews`, `openPreview` = existing `open_external_url`, which only accepts http/https), `CockpitApi.previews`, chip in `PaneView` header, globe in `Sidebar` rows, palette items, `styles/live-preview.css`. Monochrome.
- Phone: `components/mobile/PreviewNote.tsx` shows `Preview <url> · Open on your PC` under the tab bar of the selected agent (`PwaApp.tsx`, fed by `GET /previews` and the `pane-preview` SSE event).
- OSS: none needed (`regex` already a dependency).

### Why the phone does not get a proxy
The tunnel (ngrok or similar) is the only door from the internet to this computer. Forwarding a dev-server port through it would publish whatever the agent is running (often unauthenticated, with debug endpoints, source maps, `.env`-reading middleware) to anyone who learns the tunnel URL, and the pairing token protects the bridge, not an arbitrary dev server. The phone therefore only shows the address, labelled "Open on your PC". A safe remote preview would need its own scoped, short-lived, authenticated reverse proxy; that is a separate design.

## Evidence
Run in `D:\pm-approval-cards`, 2026-10-14.
- `cargo test --lib` (`CARGO_BUILD_JOBS=3`): `1063 passed; 1 failed; 2 ignored` (the failure is the known `approve_requires_app_session`).
- New Rust tests, `pane_preview::tests` (11): `vite_prints_local_and_network_as_one_preview`, `next_and_cra_keep_the_local_url_and_the_lan_one`, `django_flask_uvicorn_and_astro_are_found_and_loopback_is_normalised` (Django, Flask, Uvicorn, Astro, IPv6 Vite), `public_urls_and_non_http_text_are_ignored`, `trailing_punctuation_and_paths_are_handled`, `ports_are_validated`, `a_pane_keeps_its_three_most_recent_distinct_servers` (dedupe + cap), `seeing_the_same_server_again_is_silent_and_does_not_reshuffle` (50 repeats, no event), `output_without_a_url_changes_nothing`, `urls_expire_when_the_pane_exits_and_listeners_hear_it` (expiry), `the_activity_ticker_feeds_the_detector_once_per_changed_screen` (5,000 `note_output` calls, one tick). Route: `bridge::tests::previews_route_returns_the_detected_dev_servers_per_pane` (GET /previews -> 200 `{"previews":{"route-pane":[{"url":"http://localhost:5173/","port":5173,"networkUrl":"http://192.168.1.9:5173/"}]}}`; POST -> no route). The fixtures are written to match each tool's printed format, not captured from live runs.
- `npm test --workspace=@puppet-master/app`: `Test Files 51 passed (51)`, `Tests 401 passed (401)`; new `LivePreview.test.tsx` (5): helpers, sidebar globe only on panes with a preview, header chip shows "Preview :5173" and calls `open_external_url`, no chip without a preview, palette "Open preview: Agent A :5173" opens it.
- `npx tsc --noEmit -p packages/app`: clean.
- Fails before, passes after (mutation): with the trailing-punctuation trim reduced to `!`, `pane_preview::tests::trailing_punctuation_and_paths_are_handled` -> `FAILED` (`10 passed; 1 failed`); restored, all pass.
- Screenshot: `D:\pm-night\evidence\live-preview\live-preview.png` (harness scenario `live-preview`: Codex pane with the "Preview :5173" chip in the header and the globe on its sidebar row). Viewed. Mock URLs are fixtures.

## Limits
- Detection sees only the last 24 non-empty screen lines at tick time; a banner that scrolls away within one second of output is missed. A URL stays offered until the pane's process exits even if the server stopped.
- Only the newest URL gets the chip; the other (max 3) appear in the palette.
- Wrapped URLs (broken across two terminal lines) are cut at the line end.
- The phone note has no harness screenshot (the harness covers the cockpit and settings windows only) and was not tried on a real phone.
