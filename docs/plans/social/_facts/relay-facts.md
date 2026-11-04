# Relay facts (read-only survey)

Repo root: `C:\Users\Ren-pc\Desktop\work2\puppet-master`. Every fact below was read from the file named next to it. Items marked **not read** were not opened and must not be assumed.

---

## 1. `relay/` layout

| Path | Purpose |
|---|---|
| `relay/Cargo.toml` | Cargo workspace: `members = ["protocol", "server", "worker/wasm"]`, `resolver = "2"`. |
| `relay/Cargo.lock` | Lock file for the workspace. |
| `relay/protocol/` (`Cargo.toml`, `src/lib.rs`, `src/room.rs`) | Crate `pm-relay-protocol`, license **MIT**. Wire types and signature checks, no I/O. Shared by app and server. |
| `relay/server/` (`Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/rooms.rs`, `src/rooms/routes.rs`, `tests/protocol.rs`, `tests/rooms.rs`) | Crate `pm-relay` (lib `pm_relay`, bin `pm-relay`), axum + SQLite. Self-hostable binary. `publish = false`, license from `../LICENSE`. |
| `relay/worker/` (`wrangler.toml`, `package.json`, `src/*.ts`, `src/wasm/`, `wasm/`, `scripts/`, `test/`) | Cloudflare Worker, same protocol. Has its own `README.md`, `LICENSE`, `.gitignore`. |
| `relay/viewer/` (`package.json`, `src/*.ts`, `src/viewer.html`, `src/viewer.css`, `dist/viewer.html|js|css`, `scripts/build.mjs`, `test/*`) | Live Room viewer page, built output committed in `dist/`. Served by both server and Worker. Has `README.md`. |
| `relay/Dockerfile` | Two-stage image (see section 4). |
| `relay/.dockerignore` | Lines: `target`, `worker/node_modules`, `worker/.wrangler`. |
| `relay/README.md` | Main relay doc: "Pick one" table, binary run, limits, Live Rooms. |
| `relay/HOSTED-RELAY.md` | Founder checklist for a Puppet Master hosted relay (cost, names, decisions). Says nothing is deployed. |
| `relay/LICENSE` | Licence text placeholder for server (decision pending per HOSTED-RELAY.md section 8). |
| `relay/relay.sqlite`, `relay.sqlite-shm`, `relay.sqlite-wal` | Local SQLite DB files present in the folder (not opened). |
| `relay/target/` | Cargo build output (not inspected). |

### 1a. Deploy facts from `relay/README.md` and `relay/HOSTED-RELAY.md`

- Three self-host options in README "Pick one": Cloudflare Worker (`relay/worker/README.md`, "Deploy to Cloudflare" button or `npm run deploy`), Docker (`docker build -t pm-relay relay`), binary (`cargo run --release -p pm-relay -- --listen 127.0.0.1:8787 --db relay.sqlite`).
- Docker run from README: `docker run -d --restart unless-stopped -p 127.0.0.1:8787:8787 -v pm-relay-data:/data pm-relay`.
- Put TLS in front (reverse proxy or `cloudflared`). The app accepts plain `http://` only for loopback (see section 6).
- Trusted proxy flag: `--trusted-proxy <address or range>` (repeatable). Example: `--trusted-proxy 127.0.0.1 --trusted-proxy 10.0.0.0/8`.
- Health: `curl https://<address>/v1/health` must return exactly `{"ok":true,"protocol":"pm-relay/1"}` (HOSTED-RELAY.md section 5).
- Worker commands (HOSTED-RELAY.md): `cd relay/worker`, `npm ci`, `npx wrangler login`, `npm run deploy`. Optional `npm test`, `npm run test:worker`. WASM rebuild: `npm run build:wasm` (needs `wasm32-unknown-unknown` target and `wasm-bindgen-cli --version 0.2.129`).
- Never set `TEST_HOOKS` on a real deployment (HOSTED-RELAY.md section 5, wrangler.toml comment).
- Cloudflare free plan stops until 00:00 UTC when a daily limit is hit; paid plan 5 USD/month minimum (HOSTED-RELAY.md section 2). Recommendation there: Workers Paid.
- HOSTED-RELAY.md section 6: app has **no default relay**; the Relay address box in Settings › Team starts empty (`CreateForm` in `TeamSettings.tsx`) and `relay` in `team/config.rs` defaults to empty.
- Docker image was **never built** (HOSTED-RELAY.md section 10 "Not verified").
- Env vars and defaults: the Rust binary reads **no environment variables** for config (only `RUST_LOG`-style filter: `tracing_subscriber::EnvFilter::try_from_default_env()`, default `info`, in `server/src/main.rs` line 24). Configuration is flags only (section 2). The Worker reads `LIMIT_*` vars from `wrangler.toml [vars]` (section 3).
- Ports: server default listen `127.0.0.1:8787`; Docker entrypoint binds `0.0.0.0:8787`; Worker has no port (Cloudflare URL).
- Volumes: Docker `/data` (named volume `pm-relay-data` in docs); DB file `/data/relay.sqlite`.

---

## 2. `relay/server/src` (crate `pm-relay`)

### Modules

| File | Purpose |
|---|---|
| `main.rs` | Binary entry. Parses CLI flags, opens store, runs `serve` on a tokio runtime. |
| `lib.rs` (1053 lines) | Everything for teams: `Limits`, `Config`, `Cidr`, `client_addr`, `address_key`, `SeenSigs` (replay guard), `Relay` store, all team HTTP and WebSocket handlers, `router`, `serve`, `spawn_local` (tests). Has `mod rooms;`. |
| `rooms.rs` (477 lines) | Live Room in-memory state: `RING_FRAMES = 300`, `BATCH_FRAMES = 100`, `FRAME_BURST = 10.0`, `REGISTRATION_BYTES = 8 * 1024`, `RoomError`, `Rooms` (`register`, `owner_key`, `put_snapshot`, `push`, `end`, `status`, `view`, `notifier`, `sweep`, `dump`). Rooms are not written to disk. |
| `rooms/routes.rs` (301 lines) | Live Room axum routes and the viewer page. Includes `VIEWER_CSP`, and viewer HTML/JS/CSS via `include_str!("../../../viewer/dist/viewer.*")`. |
| `tests/protocol.rs` | End-to-end tests over loopback HTTP and WebSocket (20 `#[test]`). |
| `tests/rooms.rs` | Live Room tests (21 `#[test]`). |

### CLI flags (`main.rs`)

- `--listen <addr>` default `127.0.0.1:8787`.
- `--db <path>` default `relay.sqlite` (relative to working directory). `--memory` sets DB to in-memory (no file).
- `--trusted-proxy <addr or CIDR>` (repeatable).
- Live Room flags, with defaults from `--help` text: `--rooms-per-address-per-hour 10`, `--max-open-rooms 100`, `--room-frames-per-second 2`, `--room-frames-per-day 7200`, `--viewer-requests-per-minute 120`, `--seats-per-address 4`, `--room-ring-bytes 2097152`, `--global-room-bytes 268435456`.
- `-h` / `--help`. Unknown flag: exit 2.

### SQLite file path

- Chosen by `--db` (`Config.db`). Default is `relay.sqlite` in the working directory (main.rs line 27: `Config { db: Some("relay.sqlite".into()), .. }`). `--memory` gives `None` and `Connection::open_in_memory()`.
- `Relay::open(config)` in `lib.rs` line 326; sets `PRAGMA journal_mode = WAL`.
- Tables: `teams`, `state`, `envelopes`, `joins`, `devices`. Migrations for old DBs inside `open`.

### Limits (`Limits` struct, `lib.rs` lines 59-100, `Default` lines 102-128)

| Field | Default | Meaning |
|---|---|---|
| `device_per_minute` | 60 | envelopes per device per minute |
| `device_per_day` | 5,000 | envelopes per device per day |
| `team_per_minute` | 600 | envelopes per team per minute |
| `device_requests_per_minute` | 300 | all signed requests of one device |
| `joins_per_invite` | 5 | join tries per invite |
| `joins_per_team_per_day` | 200 | join requests per team per day |
| `joins_per_address_per_hour` | 10 | join requests per address per team per hour |
| `unauthenticated_per_ip_per_hour` | 30 | team registrations per address per hour |
| `max_wait_s` | 25 | long-poll cap seconds |
| `team_max_envelopes` | 20,000 | mailbox envelopes per team (over: 507) |
| `team_max_bytes` | 128 MiB | mailbox bytes per team |
| `sockets_per_device` | 4 | open WebSockets per device (over: 429) |
| `max_windows` | 500,000 | rate-limit windows in memory |
| `max_seen_sigs` | 2,000,000 | remembered request signatures |
| `rooms_per_address_per_hour` | 10 | Live Room registrations |
| `max_open_rooms` | 100 | open rooms (over: 503) |
| `room_ring_bytes` | 2 MiB | deltas per room |
| `global_room_bytes` | 256 MiB | all rooms |
| `room_frames_per_second` | 2 | owner frames refill rate (burst 10) |
| `room_frames_per_day` | 7,200 | owner frames per room per day |
| `viewer_requests_per_minute` | 120 | viewer calls per address per minute |
| `seats_per_address` | 4 | viewer seats per address per room |

Other fixed constants (`lib.rs`): `MAX_CLIENT_FRAME = 4 * 1024` (client WS message size), `BATCH = 200` (envelopes per batch), `SEEN_FOR_MS = 2 * AUTH_SKEW_MS + 1_000` (= 601,000 ms, replay memory), sweeper every 600 s (`serve`).

Protocol constants (`protocol/src/lib.rs`): `MAX_ENVELOPE_BYTES = 24 * 1024`, `MAX_JOIN_BYTES = 4 * 1024`, `MAX_STATE_BYTES = 256 * 1024`, `MAILBOX_TTL_MS = 7 days`, `JOIN_TTL_MS = 1 day`, `AUTH_SKEW_MS = 5 min`, `AUTH_HEADER = "x-pm-auth"`, `SLOTS = ["roster", "keys"]`, `PROTOCOL = "pm-relay/1"`.

Body limits per route (from `router`): register 8 KiB, acl 64 KiB, state `MAX_STATE_BYTES`, envelopes `MAX_ENVELOPE_BYTES`, joins `MAX_JOIN_BYTES`.

### HTTP routes (team side, `router` in `lib.rs` lines 924-935)

| Method and path | Auth | Request | Response |
|---|---|---|---|
| `GET /v1/health` | none | — | 200 `{"ok":true,"protocol":"pm-relay/1"}` |
| `POST /v1/teams` | none; per-address limit (30/hour) | `TeamRegistration` JSON (≤8 KiB) | 201 empty; 400 `"bad registration"` or `"the team id, root key and access list don't match"`; 409 `"team already registered"`; 429 `"slow down"` |
| `PUT /v1/teams/{team}/acl` | root-signed body (`SignedAcl`), no header | `SignedAcl` (≤64 KiB) | 204; 400 `"bad access list"`; 403 `"not signed by the team's root key"`; 409 `"older access list"` |
| `PUT /v1/teams/{team}/state/{slot}` | signed device (`x-pm-auth`) + root-signed blob | `SignedBlob` (≤256 KiB) | 204; 400 `"bad state"`; 403 `"not signed by the team's root key"`; 409 `"older state"` |
| `GET /v1/teams/{team}/state/{slot}` | signed device | — | 200 blob JSON (`application/json`); 204 if none |
| `POST /v1/teams/{team}/envelopes` | signed device | `Envelope` (≤24 KiB) | 200 `{"seq": N}`; 400 `"bad envelope"`; 403 `"the envelope is not signed by this device for this team"`; 429 `"slow down"`; 507 `"the team's mailbox is full"` |
| `GET /v1/teams/{team}/envelopes?after=N&wait=S` | signed device | query `after` (default 0), `wait` (capped at 25) | 200 `Batch` JSON; 403 `"device not on this team"` if removed during long poll |
| `GET /v1/teams/{team}/ws?after=N` | signed device (`authorize` runs before upgrade) | WebSocket upgrade | stream of `Delivered` JSON frames, or `{"reset":true,"head":N}`; 429 `"too many open connections for this device"` |
| `POST /v1/teams/{team}/joins` | none (sealed, per invite) | `JoinBox` (≤4 KiB) | 202; 400 `"bad join request"` or `"wrong team"`; 429 `"too many join requests"` |
| `GET /v1/teams/{team}/joins` | signed device | — | 200 JSON array of `JoinBox` |

Signed-request auth (`authorize`, lines 466-490): header `x-pm-auth` required, else 401 `"signed request required"`. `verify_request` failure is 401 with the reason. Device not on access list: 403 `"device not on this team"` (404 `"unknown team"` if team unknown). Replay: 401 `"request replayed"`. Seen-table full: 503 `"the relay is busy, try again shortly"`. Per-device request window: 429 `"slow down"`.

### Live Room routes (`rooms/routes.rs`, merged in `router`)

| Method and path | Auth | Notes |
|---|---|---|
| `POST /v1/rooms` | signature inside body | body ≤ `REGISTRATION_BYTES` (8 KiB); 201 `{"room": …}`; 409 if id used (tombstones included) |
| `PUT /v1/rooms/{room}/snapshot` | owner (`x-pm-auth`, room key) | one `snap` frame ≤96 KiB; 200 `RoomStatus` |
| `POST /v1/rooms/{room}/frames` | owner | one `delta` or `state` frame ≤24 KiB, `seq` must rise; 200 `RoomStatus` |
| `GET /v1/rooms/{room}/status` | owner | 200 `RoomStatus` |
| `DELETE /v1/rooms/{room}` | owner | 204; body limit 0 |
| `GET /v1/rooms/{room}/snapshot` | viewer: headers `x-pm-room-read`, `x-pm-room-seat` | 200 frame, 204 none yet, 404, 410 `{"ended": …}`, 429 `{"full": true}` |
| `GET /v1/rooms/{room}/frames?after=N&wait=S` | viewer | 200 batch up to 100 frames; `wait` capped at 25 s |
| `GET /r/`, `/r/viewer.js`, `/r/viewer.css` | none | viewer page with strict CSP (`default-src 'none'`) |

### WebSocket details (`lib.rs` lines 782-873)

- Path: `/v1/teams/{team}/ws?after=N`. Backlog sent first, then live `Delivered` items as text frames.
- Server sends `Message::Ping` every 30 s (`stream_team`, `ping` interval 30 s).
- Client messages capped at 4 KiB (`MAX_CLIENT_FRAME`); clients only listen (pings).

### Auth and tokens

- There is **no admin token and no bearer token**. Every device request is signed with Ed25519 by the device key, in header `x-pm-auth` (`auth_header` / `verify_request` in `protocol/src/lib.rs`). The signed message is built by `auth_message(method, path_and_query, ts_ms, body)`.
- Admission: device must be in the team's access list (`devices` table; `member_from`). Access list changes need a root-key signature (`SignedAcl`). Team state blobs need root signature (`SignedBlob`).
- Team id = hash of root key and salt: `team_id(root_pk: &[u8;32], salt: &[u8]) -> String`.
- Client address for per-address limits: `client_addr(peer, headers, trusted)`. IPv6 counted per /64 (`address_key`).
- Health/version: `GET /v1/health` only. No other health or version endpoints.

---

## 3. `relay/worker`

- **What it is:** the same relay as a Cloudflare Worker (`src/index.ts`) with one Durable Object per team. Same routes, status codes and messages as the server (`relay/worker/README.md`, "Differences from the server").
- **Entry file:** `src/index.ts` (`main = "src/index.ts"` in `wrangler.toml`). Worker header comment lists routes (lines 6-15).
- **Modules:** `index.ts` (front door: route sort, body size 413, signature check, per-address count, hands to team object), `team.ts` (class `Team`, 780 lines: team storage, access list, replay guard, rate limits, long poll, WebSockets, alarm), `gate.ts` (class `IpGate`: per-address counter and replay), `room.ts` (classes `Room` and `RoomIndex`), `roomproto.ts` (TS mirrors of room shape checks), `proto.ts` (imports the WASM protocol checks), `addr.ts` (address key, same rule as server `address_key`), `http.ts` (`json`, `empty`, `jsonText`, `queryU64`, `Refusal`, `refuse`), `env.ts` (`Env` interface, `DEFAULT_LIMITS`, `limitsFrom`), `wasm.d.ts`, `text-modules.d.ts`, `wasm/` (committed `pm_relay_wasm.js`, `pm_relay_wasm_bg.wasm`, `.d.ts`, `BUILD.txt`).
- **Config (`wrangler.toml`):**
  - `name = "pm-relay"`, `main = "src/index.ts"`, `compatibility_date = "2025-09-01"`.
  - Routes line commented out. Example: `routes = [{ pattern = "relay.example.com", custom_domain = true }]`.
  - Durable Objects: `TEAM` → class `Team`; `GATE` → class `IpGate`. Migration `v1`: `new_sqlite_classes = ["Team", "IpGate"]`.
  - Durable Objects: `ROOM` → class `Room`; `ROOM_INDEX` → class `RoomIndex`. Migration `v2`: `new_sqlite_classes = ["Room", "RoomIndex"]`.
  - `[[rules]] type = "Text"`, globs `**/viewer.html`, `**/viewer.js`, `**/viewer.css`.
  - `[vars]` lines are commented out. Names: `LIMIT_DEVICE_PER_MINUTE`, `LIMIT_DEVICE_PER_DAY`, `LIMIT_TEAM_PER_MINUTE`, `LIMIT_DEVICE_REQUESTS_PER_MINUTE`, `LIMIT_JOINS_PER_INVITE`, `LIMIT_JOINS_PER_TEAM_PER_DAY`, `LIMIT_JOINS_PER_ADDRESS_PER_HOUR`, `LIMIT_UNAUTH_PER_IP_PER_HOUR`, `LIMIT_SOCKETS_PER_DEVICE`, `LIMIT_TEAM_MAX_ENVELOPES`, `LIMIT_TEAM_MAX_BYTES`, `LIMIT_MAX_WAIT_S`, `LIMIT_ROOMS_PER_ADDRESS_PER_HOUR`, `LIMIT_ROOM_VIEWS_PER_MINUTE`, `LIMIT_ROOM_MAX_OPEN`. Comment: never set `TEST_HOOKS`.
  - `[observability] enabled = true`.
- **Env bindings (`env.ts`):** `TEAM`, `GATE`, `ROOM`, `ROOM_INDEX`, the `LIMIT_*` strings above, and `TEST_HOOKS` ("1" turns on `/v1/_test`).
- **Storage:** SQLite-backed Durable Objects only. Team: mailbox, access list, state slots, replay guard and rate-limit counters in the object's SQLite. Live Room: `room`, `frames`, `snapshot` tables per room object. Nothing in KV, R2 or D1.
- **Differences from server** (README "Differences from the server"): reads `CF-Connecting-IP` instead of `X-Forwarded-For`; rate limits and replay guard live in storage (cost row writes), not memory; WebSocket has no server ping (client sends text `ping`, runtime answers `pong`); long polls hold the object awake.
- **Tests:** `npm test` (`scripts/run-tests.mjs`, all), `npm run test:worker` (`--worker-only`), `npm run test:app` (`--app-only`), `npm run typecheck`, `npm run dev` (`wrangler dev`, `http://127.0.0.1:8787`). Test files: `test/proto.mjs`, `test/room.test.mjs`, `test/worker.test.mjs`.
- **Dependencies:** dev-only `wrangler ^4.0.0`, `typescript ^5.6.0`, `@cloudflare/workers-types`. Not installed globally.

---

## 4. `relay/Dockerfile`

- **Stage 1 `build`:** `FROM rust:1.96-slim-bookworm AS build`. `WORKDIR /src`. Copies `protocol`, `server`, `worker/wasm`, `worker/LICENSE`, `viewer/dist`, `Cargo.toml`, `Cargo.lock`, `LICENSE`. Runs `cargo build --release --locked -p pm-relay`.
- **Stage 2 runtime:** `FROM debian:bookworm-slim`. Installs `tini` via apt. Creates system user `pmrelay`, uid `10001`, no home, nologin shell. `mkdir /data`, `chown pmrelay:pmrelay /data`.
- `COPY --from=build /src/target/release/pm-relay /usr/local/bin/pm-relay`.
- `USER pmrelay`. `VOLUME /data`. `EXPOSE 8787`.
- `HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3` runs a bash `/dev/tcp` GET of `/v1/health` (no curl in image).
- `ENTRYPOINT ["tini", "--", "pm-relay", "--listen", "0.0.0.0:8787", "--db", "/data/relay.sqlite"]`.
- **ENV:** none set. **Build args:** none.
- Extra run options are appended after the image name (e.g. `--trusted-proxy 172.16.0.0/12`).
- Base images are not pinned by digest (comment says `@sha256:<digest>` can be added).
- **Never built** (HOSTED-RELAY.md section 10).

---

## 5. `relay/protocol/src` (crate `pm-relay-protocol`, MIT)

Constants: `PROTOCOL = "pm-relay/1"` (lib.rs); `ROOM_PROTOCOL = "pm-room/1"`, `ROOM_READ_HEADER = "x-pm-room-read"`, `ROOM_SEAT_HEADER = "x-pm-room-seat"`, `MAX_ROOM_FRAME_BYTES = 24 KiB`, `MAX_ROOM_SNAPSHOT_BYTES = 96 KiB`, `MAX_ROOM_LIFETIME_MS = 8 h`, `MAX_ROOM_VIEWERS = 50`, `SEAT_ACTIVE_MS = 40_000`, `ROOM_TOMBSTONE_MS = 1 h` (room.rs).

Functions (lib.rs): `b64`, `unb64`, `unb64_32`, `base32`, `sha256`, `hex`, `team_id(root_pk: &[u8; 32], salt: &[u8]) -> String`, `is_team_id`, `is_key_id`, `verify_sig(key_id, message, sig_b64) -> bool`, `sign(key, message) -> String`, `key_id(key) -> String`, `auth_message(method, path_and_query, ts_ms, body) -> Vec<u8>`, `auth_header(device, method, path_and_query, ts_ms, body) -> String`, `verify_request(...)`.

Wire types (lib.rs, serde):

- `Envelope { v: u8, id: String, team: String, epoch: u64, from: String, seq: u64, nonce: String, ct: String, sig: String }`.
- `Delivered { seq: u64, at_ms: u64, envelope: Envelope }`.
- `Batch { items: Vec<Delivered>, next: u64, head: Option<u64>, reset: bool }` (`head` and `reset` omitted when absent or false).
- `Resync { reset: bool, head: u64 }` (WebSocket control frame).
- `SignedAcl { team: String, version: u64, devices: Vec<String>, sig: String }`.
- `TeamRegistration { team: String, root: String, salt: String, acl: SignedAcl }`.
- `SignedBlob { team: String, slot: String, version: u64, body: String, sig: String }`.
- `JoinBox { team: String, invite: String, enc: String, ct: String, at_ms: u64 }`.

Wire types (room.rs):

- `RoomRegistration { v: u8, room: String, key: String, expires_at_ms: u64, max_viewers: u32, read_hash: String, sig: String }`.
- `RoomFrame { v: u8, room: String, seq: u64, kind: FrameKind, nonce: String, ct: String }`.
- `FrameKind` enum: `Snap`, `Delta`, `State` (serde lowercase: `snap`, `delta`, `state`).
- `RoomEnd` enum: `Stopped`, `Expired` (lowercase).
- `RoomBatch { frames: Vec<RoomFrame>, next: u64, ended: Option<RoomEnd> }` (camelCase).
- `RoomStatus { viewers: u32, expires_at_ms: u64 }` (camelCase).

Tests: 5 `#[test]` in `protocol/src/lib.rs`, 21 in `protocol/src/room.rs`.

---

## 6. App side

### `packages/app/src-tauri/src/team/transport/relay.rs` (465 lines)

- Module doc: outbound only; every request signed (`x-pm-auth`); receive by WebSocket `/ws?after=`, fallback long poll `/envelopes?after=&wait=`.
- Constants: `POLL_WAIT_S = 20`, `POLL_BEFORE_WS_RETRY = 60 s`, `WS_PING_EVERY = 45 s`, `BACKOFF = 3 s`. Modes: `MODE_OFFLINE=0`, `MODE_LIVE=1`, `MODE_POLLING=2`, `MODE_DENIED=3`.
- `pub struct RelayTransport` fields: `base`, `prefix`, `origin`, `team`, `device: SigningKey`, `agent: ureq::Agent`, `stops`, `mode`, `poll_only`.
- `pub fn normalize_relay_url(raw: &str) -> Result<String, String>` rules:
  - trims and strips trailing `/`;
  - must start `https://`, or `http://` (else error "The relay address starts with https:// (or http:// on this computer).");
  - no `?` or `#`, no whitespace or control chars;
  - authority must not contain `@`, `\`, `%`;
  - IPv6 in brackets; port digits only;
  - host non-empty;
  - `http` only if host is `localhost` or a loopback IP, else "Use https:// for a relay on another computer.".
- `RelayTransport::new(base: &str, team: &str, device: SigningKey) -> Result<Self, String>`: connect timeout 5 s; overall timeout `POLL_WAIT_S + 15` = 35 s.
- `poll_only(self)` test switch.
- Paths: `{prefix}/v1/teams/{team}…`. Poll URL `…/envelopes?after={after}&wait={wait_s}`. WebSocket URL `{ws|wss}://{host}{prefix}/v1/teams/{team}/ws?after={cursor}`.
- `fn relay_error(code: u16, body: &str) -> String` (private) error strings:
  - 403: `"{DENIED} yet"` (+ ` (relay message)`), where `DENIED` = `"The relay doesn't let this computer in"` (`team/transport/mod.rs` line 37).
  - 401: `"The relay refused this computer's signature"` (+ message).
  - 404: `"The relay doesn't know this team."`
  - 409: `"The relay already has a newer copy."`
  - 413: `"That's too large for the relay."`
  - 507: `"The team's mailbox on the relay is full. Try again later."`
  - 429: `"The relay asks to slow down. Try again in a minute."`
  - other: `"The relay said {code}"` (+ `: message`).
- Status strings given to UI (`fn status`): `"live"`, `"polling"`, `"offline"`. Denied via `lost_access()`.
- Trait impl `TeamTransport` methods: `kind`, `send`, `subscribe`, `put_state`, `get_state`, `post_join`, `joins`, `register`, `set_acl`, `status`, `lost_access`, `stop`.
- Trait `TeamTransport` lives in `team/transport/mod.rs`. `TransportError(String)` with `is_denied()`.

### `packages/app/src-tauri/src/team/config.rs` (154 lines)

- File name `team.json` (`FILE_NAME`), path `path_in(dir: &Path) -> PathBuf`.
- `pub struct TeamConfig` (`camelCase`), fields: `enabled: bool` (Labs switch), `team: String`, `name: String`, `relay: String` ("The relay address, or empty to use `folder`"), `folder: String`, `me: Member`, `device: String`, `root: String`, `salt: String`, `github: Option<String>`, `projects: Vec<String>`, `join: Option<PendingJoin>`.
- Relay URL saved in `TeamConfig.relay`. `normalized(self) -> Result<Self, String>` runs `normalize_relay_url` when non-empty.
- `has_team()`: team, root, and a relay or folder set. `is_active()`: enabled + has_team + me.name non-empty.
- `load_checked(path) -> Result<TeamConfig, String>`, `save(path, config) -> Result<(), String>` (writes `team.json.tmp` then renames).

### Team settings UI (`packages/app/src/`)

- `components/settings/blocks/TeamSettings.tsx`:
  - `export function TeamSettings({ api = teamApi, onSaved }: { api?: TeamApi; onSaved?: () => void })` (line 74). Shows connection text (`Relay · live`, `Relay · polling`, from `view.link`) when `view.transport === 'relay'`.
  - `export const RELAY_HELP_URL` (line 28) → `https://github.com/Potato-dev-inc/puppet-master/blob/main/relay/README.md`.
  - `function RelayHelp()` (no props): text "Host your own relay on Cloudflare or with Docker. A hosted relay from Puppet Master is coming."
  - `function CreateForm({ busy, onCreate }: { busy: boolean; onCreate: (input: TeamCreateInput) => Promise<boolean> })` (line 298). Has the **Relay address** field (`<span>Relay address</span>`, placeholder `https://relay.example.com`, `useFolder` toggles folder instead). Calls `onCreate({ teamName, name, relay, folder })`.
  - `function JoinForm({ busy, onJoin })` (line 350): no relay field (address comes from invite link).
- `components/settings/blocks/LiveRoomSettings.tsx`: `export function LiveRoomSettings({ api = liveRoomApi, teamRelay = '', onSaved })`. Its own **Relay address** field (aria-label "Relay address") for Live Rooms, saved on blur or Enter. Placeholder `Uses your team relay` when `teamRelay` is set.
- `components/settings/blocks/LiveRoomRow.tsx`: `export function LiveRoomRow({ onSaved }: { onSaved?: () => void })`. Looks up `useTeamRelay()`.
- Settings host: `components/settings/sections/GeneralSection.tsx` line 549: Team row `render: (env) => <TeamSettings onSaved={env.markSaved} />` (under group `Labs`).
- Live Room row line 550: `render: (env) => <LiveRoomRow onSaved={env.markSaved} />` (group `Live Room`).
- Cockpit (not settings): `components/cockpit/team/TeamSection.tsx`, `components/cockpit/live/useTeamRelay.ts`, `components/cockpit/live/GoLiveSheet.tsx`, `components/cockpit/live/LiveBar.tsx`.
- Client API: `src/lib/team.ts`: `type TeamTransport = 'folder' | 'relay'`, `TeamConfigView` (has `relay`), `TeamConfigInput`, `TeamCreateInput`, `TeamJoinInput`, `TeamApi`, `teamApi` (`configSet: (config) => call('team_config_set', { config })`).
- Existing tests: `components/settings/blocks/TeamSettings.test.tsx` (checks the Docker/Cloudflare help text), `components/settings/blocks/LiveRoomSettings.test.tsx`, `packages/e2e/tests/labs/team-forms-need-input.test.mjs`.

### Tauri commands for team (`src-tauri/src/lib.rs` `generate_handler!`)

- Registered in `lib.rs` lines 265-287: line 266 `team::commands::team_config_get` (`pub async fn team_config_get() -> Result<Value, String>`, `team/commands.rs` line 20), line 267 `team::commands::team_config_set` (`pub async fn team_config_set(config: service::ConfigInput) -> Result<Value, String>`, `team/commands.rs` line 25), plus `team_state` (265), `team_create`, `team_invite`, `team_join`, `team_leave`, `team_join_approve`, `team_join_decline`, `team_member_remove` and others up to line 287.

---

## 7. Secrets: how the app stores them

- File `packages/app/src-tauri/src/secrets.rs`:
  - `pub const SERVICE: &str = "com.puppetmaster.app";`
  - `pub const PROVIDER_SECRETS: &[&str] = &["anthropic_api_key", "openai_api_key", "openrouter_api_key"];` and `pub fn is_provider_secret(name: &str) -> bool`.
  - `pub const SIDECAR_SECRETS: &[&str] = &["deepseek_api_key", "discord_bot_token"];` and `pub fn is_sidecar_secret(name: &str) -> bool`.
  - `pub fn is_ui_secret(name: &str) -> bool` (provider or sidecar). Only names in these lists can be set/read masked/deleted from the UI.
  - `pub trait SecretBackend: Send + Sync` with `fn get(&self, name: &str) -> Result<Option<String>, String>`, `fn set(&self, name: &str, value: &str) -> Result<(), String>`, `fn delete(&self, name: &str) -> Result<(), String>`, `fn label(&self) -> &'static str`, `fn available(&self) -> bool`.
  - `pub struct KeyringBackend;` (Windows Credential Manager via `keyring`), `pub struct MemoryBackend` (`new()`, `unavailable()`).
  - `pub fn backend() -> &'static dyn SecretBackend` (line 140; test version line 146).
  - `pub fn read_secret(name: &str) -> Option<String>` (line 152, empty counts as unset).
- File `packages/app/src-tauri/src/commands/secrets.rs` Tauri commands:
  - `secret_storage_info() -> SecretStorageInfo`
  - `secret_set(name: String, value: String) -> Result<(), String>` (rejects empty value)
  - `secret_get_masked(name: String) -> Result<SecretMask, String>`
  - `secret_delete(name: String) -> Result<(), String>`
  - `secret_get_for_llm(name: String) -> Result<Option<String>, String>` (provider names only)
  - guard `fn guard(name)`: rejects names not in `is_ui_secret`.
  - Frontend: `packages/app/src/lib/secrets.ts` wraps them (`getMasked`, `set` via `invoke`).
- Team keys: `packages/app/src-tauri/src/team/keystore.rs`:
  - `pub struct KeyStore { backend: Arc<dyn SecretBackend>, prefix: String }`, prefix `team/{profile}/`.
  - `KeyStore::app() -> Self` (profile from `crate::app_paths::profile()`), `KeyStore::with_backend(backend, prefix)`.
  - Methods: `member() -> Result<SigningKey, String>`, `device_sign()`, `p2p_node()`, `device_kem() -> Result<KemSecret, String>`, `epoch_key(team, epoch) -> Result<Option<[u8;32]>, String>`, `set_epoch_key(team, epoch, key)`, `invite_code(team, invite) -> Result<Option<String>, String>`, `set_invite_code`, `forget_invite`, `forget_team(team, last_epoch)`.
  - Private helpers: `name(&self, what)`, `get32`, `set32`, `get_or_make`.
- **Admin token for a relay:** there is no admin token on the relay (auth is root-key signatures). If one were stored, the choice is: add a name to `PROVIDER_SECRETS`/`SIDECAR_SECRETS` (UI-settable), or add a `KeyStore` method (the team keystore `KeyStore::name` pattern). Neither exists yet.

---

## 8. Mentions of Railway, Cloudflare/wrangler, Fly, Render, Docker, tunnels, deploy

Searched with `git grep -i -E 'railway|wrangler|fly\.io|render\.com|docker|deploy to cloudflare|deploy\.workers'`.

- **Railway:** no mention anywhere in the git-tracked repo (`git grep -l -i railway` returned nothing).
- `relay/README.md`: "The "Deploy to Cloudflare" button in `worker/README.md`, or `npm run deploy` there" (Cloudflare row of "Pick one").
- `relay/worker/README.md`: `[![Deploy to Cloudflare](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https://github.com/Potato-dev-inc/puppet-master/tree/main/relay/worker)`.
- `relay/worker/wrangler.toml` line 2: "Deploy: the button there, or `npm run deploy`".
- `relay/Dockerfile` line 3: `docker build -t pm-relay relay`.
- `relay/HOSTED-RELAY.md` section 3: "Cloudflare Worker (recommended)" vs "Docker image".
- `packages/app/src/components/settings/blocks/TeamSettings.tsx` line 35: "Host your own relay on Cloudflare or with Docker. A hosted relay from Puppet Master is coming."
- `packages/app/src/components/settings/blocks/TeamSettings.test.tsx` line 82: asserts that same help text.
- `.github/workflows/ci.yml` line 68: "The team relay: the Rust server and protocol (cargo test in relay/) and the Cloudflare Worker's own tests." Line 96: Worker tests run under `wrangler dev`.
- `docs/features/relay-worker.md` line 33: "Cloudflare `wrangler` 4 (MIT/Apache-2.0, dev dependency of `relay/worker` only, never global)".
- `docs/features/relay-worker.md` line 134: "`relay/Dockerfile` was written but not built (no Docker engine running here)."
- Tunnels (different feature, phone pairing): `package.json` line 46 `"cloudflared": "0.7.1"`; `.gitignore` lines 58-59 ignore `packages/app/src-tauri/resources/cloudflared(.exe)`; `CHANGELOG.md` line 58 "requests that came through a tunnel". `docs/features/live-room.md` line 225: "against two local wrangler dev servers".
- Fly and Render: not found.

---

## 9. Opening an external URL and running a child process

- **Open URL:** `packages/app/src-tauri/src/app_lifecycle.rs` line 36-42:
  - `#[tauri::command] pub fn open_external_url(url: String) -> Result<(), String>`: accepts only `https://` or `http://` (else `"only http(s) URLs are allowed"`), then calls private `fn open_url(url: &str) -> Result<(), String>` (line 141).
  - `open_url` on Windows: `crate::proc_util::hidden_command("cmd.exe").args(["/C", "start", "", url]).spawn()`. macOS: `hidden_command("open").arg(url)`. Other platforms in the same function.
  - Registered in `lib.rs` line 436 as `app_lifecycle::open_external_url`.
  - TeamSettings.tsx line 42 has a comment saying the desktop app opens the help link in the system browser; the code path behind that comment was not traced.
- **Child process helper:** `packages/app/src-tauri/src/proc_util.rs`:
  - `pub fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command` (line 37): std Command with no console window.
  - `pub fn hide_window(command: &mut Command) -> &mut Command` (line 47).
  - `pub fn hidden_tokio_command<S: AsRef<OsStr>>(program: S) -> tokio::process::Command` (line 59).
  - `pub fn pid_alive(pid: u32) -> bool` (line 72).
- Example use: `canary/live.rs` line 261 `crate::proc_util::hidden_command(opener)`.

---

## 10. Local CLI and MCP tools

Versions run from the Bash shell:

- `railway --version`: **not installed** (`command not found`).
- `wrangler --version`: **not installed** globally (`command not found`). Wrangler exists only as a dev dependency in `relay/worker` after `npm ci` (node_modules not present in `relay/worker` at time of survey).
- `docker --version`: `Docker version 28.3.2, build 578ccf6`.
- `gh --version`: `gh version 2.95.0 (2026-06-17)`.

MCP servers:

- Project `.mcp.json` lists one server: `puppet-master`.
- Railway MCP: **connected in this session** (deferred tools `mcp__a283c013-…__*`, e.g. `whoami`, `list-projects`, `list-services`, `create-service`, `set-variables`, `railway-agent`). Also listed in the session notice as `plugin:railway:railway` requiring OAuth authorization via `/mcp`. The session notice says Claude cannot start OAuth; if a Railway task needs it, the user must authorize via `/mcp` first.
- No Cloudflare MCP server was seen in the session tool list (no `cloudflare` tool names found).
- Local `.claude/` folder: `launch.json`, `settings.local.json`, `worktrees/`.

---

## 11. Settings UI conventions, Labs, Tauri commands, tests

### Adding a Settings section (example: Team and Live Room rows)

- File: `packages/app/src/components/settings/sections/GeneralSection.tsx`. Each entry is an object in an array: `{ id, label, hint, keywords, group, render: (env) => <Component … /> }`.
  - Team entry (line 549): `{ id: 'team', label: 'Team', hint: '…', keywords: '…', group: 'Labs', labs: { name: 'Team', about: '…' }, render: (env) => <TeamSettings onSaved={env.markSaved} /> }`.
  - Live Room entry (line 550): `{ id: 'live-room', label: 'Live Room', …, group: 'Live Room', render: (env) => <LiveRoomRow onSaved={env.markSaved} /> }`.
  - Model Bridge example shape: `render: (env) => <SettingToggle label="Model Bridge" … checked={env.settings.model_bridge_enabled ?? false} onChange={(value) => void env.patch({ model_bridge_enabled: value })} />`.
- A Labs entry carries a `labs: { name, about }` object. Team's Labs on/off is `TeamConfig.enabled` in `team.json` (not a settings_store key).

### Labs flags and DEV_LABS_KEYS

- Rust: `packages/app/src-tauri/src/settings_store.rs` lines 13-21:
  ```
  pub const DEV_LABS_KEYS: &[&str] = &[
      "missions_enabled",
      "night_shift_enabled",
      "potato_bot_enabled",
      "potato_sandbox_enabled",
      "acp_chat_enabled",
      "desk_enabled",
      "model_bridge_enabled",
  ];
  ```
- TypeScript mirror: `packages/app/src/lib/dev-labs.ts` lines 8-16, same seven names (`export const DEV_LABS_KEYS = [ … ] as const;`).
- `team` is **not** in either list.

### Tauri command registration example

- `packages/app/src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]` starting line 198:
  - `team::commands::team_config_set,` (line 267)
  - `commands::secret_set,` (line 444), `commands::secret_get_masked,` (445), `commands::secret_delete,` (446)
  - `app_lifecycle::open_external_url,` (line 436)
- Frontend invokes by snake_case command name (`invoke('secret_set', { name, value })` in `src/lib/secrets.ts`).

### Test commands

- Frontend (from `packages/app/`): `package.json` scripts `"test": "vitest run"`, `"test:watch": "vitest"`. Run `npx vitest run <path>` or `npm test`.
- Root `package.json` line 37: `cargo test --manifest-path packages/app/src-tauri/Cargo.toml --features dev-fake-tui --test fake_tui_e2e -- --test-threads=1` (script `test:fake-tui`). App Rust tests use the same manifest: `cargo test --manifest-path packages/app/src-tauri/Cargo.toml` (plain form; the team module tests are in `src/team/tests*.rs`).
- Relay workspace (from `relay/`): `cargo test` runs protocol unit tests and server end-to-end tests (`relay/README.md` "Tests"). Narrower forms from the same file:
  - `cargo test -p pm-relay-protocol`
  - `cargo test -p pm-relay --test rooms`
  - `cargo test -p pm-relay --test protocol` (inferred from file name `server/tests/protocol.rs`; the README gives the rooms form only)
- Worker (from `relay/worker/`): `npm test`, `npm run test:worker`, `npm run test:app`. Viewer (from `relay/viewer/`): `npm test`, `npm run build`.
- App relay tests against any relay: set `PM_RELAY_TEST_URL` (`team/tests_v1.rs` line 497).
- CI: `.github/workflows/ci.yml` line 66-100 `relay` job on `ubuntu-latest`, `working-directory: relay/worker` for worker tests.

---

## Not found / not read

- `relay/worker/src/team.ts`, `room.ts`, `index.ts` bodies were not read line by line; only headers and grep hits. Exact response bodies for worker-only paths may differ in detail (server is the reference).
- `relay/server/src/lib.rs` lines ~386-600 (store internals, sweep, memory dump) and lines ~598-690 (`register` body and `set_acl` body) were not read in full.
- `relay/viewer/src/*` and `relay/worker/scripts/*` were not read.
- Railway: no repo mention, no deploy config, no CLI installed.
- Cloudflare MCP: none seen. `wrangler` not installed globally.
- No relay admin token exists. No secret is stored for the relay today.
- `relay/relay.sqlite*` files were not opened (no reason to).
- No secrets, tokens or credential values were printed or copied in this survey.
