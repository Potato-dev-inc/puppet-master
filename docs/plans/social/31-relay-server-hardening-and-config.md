# 31 — Relay server: hardening and zero-config

Status: plan only. Part of the one-click relay family (overview: `30-one-click-relay-overview.md`).

Sources the writer used: `00-FORMAT.md`, `_facts/relay-facts.md` ("relay facts §N"), `_facts/spaces-facts.md`
("spaces facts §N"). No source code was opened by the writer. Every NEEDS-FACT line is listed in `Open facts` near the
end of this file, using the shared numbering `NF-n` used by files 30 to 35.

All work in this file is inside `relay/` (Cargo workspace `relay/Cargo.toml`, crates `pm-relay-protocol` and
`pm-relay`, relay facts §1). Nothing here touches the app, so `p2p/guard_tests.rs` and `desk/guard_tests.rs` are not
affected; builders still run them only if they touch the app (they do not in this file).

---

## 1. What the server must do for a one-click deploy

| Need | Today (relay facts) | After this plan |
|---|---|---|
| Run with no flags in a container | Config is flags only; the Dockerfile ENTRYPOINT passes `--listen 0.0.0.0:8787 --db /data/relay.sqlite` (§1a, §4) | `PM_RELAY_*` environment variables, flags still win (R31-2, R31-8) |
| Tell the app what it is | Only `GET /v1/health` (§2) | `GET /v1/version`, `GET /v1/ready` (R31-3) |
| Admin calls without a secret on the server | No admin token, no bearer token (§2 "Auth and tokens") | An owner **public** key; admin requests signed like device requests (R31-5, R31-5b) |
| Keep strangers off a personal relay | Anyone may register a team (30 per address per hour) | `PM_RELAY_REGISTRATION=owner` (R31-5) |
| Never fill the disk | Per-team caps only; review M5 says "no storage quota" (spaces facts §8) | Whole-database cap `PM_RELAY_MAX_DB_BYTES`, default 1 GiB (R31-6) |
| Fail loudly when the disk is not writable | Unknown | Exit code 3 with a plain message (R31-6) |
| Prove WebSocket works through the provider's proxy | Only real team sockets, which need a registered team and a device signature (§2) | `GET /v1/probe/ws` sends one frame and closes (R31-7) |
| Logs never hold secrets or identifiers | Unknown (NF-25) | One access line per request with the route pattern only; a guard test (R31-4) |
| The Docker image is proven | "Never built" (§1a, §4) | Built and run with exact commands (R31-1), then rebuilt with env settings (R31-8) |

## 2. Admin access design: an owner key, not a token

The task asked for a "first-boot admin token design". The design below replaces the token with a key, for these
reasons:

| Option | Why not |
|---|---|
| A random bearer token printed in the log on first boot | Railway and Cloudflare logs are readable by anyone with dashboard access and end up in screenshots and support chats. A token in a log is a leaked token. |
| A bearer token the app sets as a provider variable | The secret then lives in the provider's variable store (visible in its dashboard), travels on every request and can be replayed by anything that sees one request. |
| "First signed request claims the relay" | Internet scanners find new `*.up.railway.app` names fast. The first claimer may be an attacker. |
| **Chosen: owner public key in `PM_RELAY_OWNER_KEY`** | Nothing secret is on the server or in the provider. The private half stays in this computer's credential store (KeyStore name `relay_owner`, file 32 slice R32-2). Admin requests reuse the existing signed-request format, replay guard and 5-minute clock skew (`AUTH_SKEW_MS`, relay facts §2). |

Rules:

1. The owner key id is a public key id in the same format `pm_relay_protocol::key_id(key)` returns and
   `pm_relay_protocol::is_key_id` accepts (relay facts §5).
2. Admin requests carry header `x-pm-owner` (new constant `pm_relay_protocol::OWNER_HEADER`). Its value is built by
   `pm_relay_protocol::auth_header(owner_signing_key, method, path_and_query, ts_ms, body)`, exactly as a device builds
   `x-pm-auth` (relay facts §2, §5). The server checks it with `pm_relay_protocol::verify_request` against the owner key
   id (exact parameters: NF-3).
3. A signature seen once is refused the second time (the existing replay memory `SeenSigs`, relay facts §2).
4. No owner key set: every admin route answers `404` with body `not found`. The relay does not say whether admin
   routes exist.
5. Rotating the owner key = setting a new `PM_RELAY_OWNER_KEY` and restarting (file 35 slice R35-7).
6. Owner-only registration: with `PM_RELAY_REGISTRATION=owner`, `POST /v1/teams` without a valid `x-pm-owner` answers
   `403` with body `this relay only takes teams from its owner`. Everything else (joins, signed device requests, Live
   Room viewers) is unchanged. Default: `owner` when an owner key is set, `open` when not.
7. An owner-signed registration skips the per-address registration limit (30 per hour, relay facts §2). Reason: behind
   a provider proxy every client can look like one address (review M5, spaces facts §8), and strangers must not be able
   to use up the owner's allowance.

## 3. Settings: flags and environment variables

Strength order: built-in default < environment variable < command-line flag. Every variable name starts with
`PM_RELAY_`. An unknown `PM_RELAY_` name stops the relay with exit code 2 and the message
`pm-relay: Unknown setting <NAME>. See relay/README.md for the list.` The names `PM_RELAY_COMMIT` (a build-time value)
and `PM_RELAY_TEST_URL` (the app's test variable, relay facts §11) are ignored so a developer's shell does not break the
relay.

| Variable | Flag | Default | Slice |
|---|---|---|---|
| `PM_RELAY_LISTEN` | `--listen` | `127.0.0.1:8787` (binary); `0.0.0.0:8787` in the image | R31-2, R31-8 |
| `PM_RELAY_DB` | `--db` | `relay.sqlite` (binary); `/data/relay.sqlite` in the image | R31-2, R31-8 |
| `PM_RELAY_MEMORY` (`1` = on, `0` or unset = off) | `--memory` | off | R31-2 |
| `PM_RELAY_TRUSTED_PROXY` (comma list) | `--trusted-proxy` (repeatable) | none | R31-2 |
| `PM_RELAY_ROOMS_PER_ADDRESS_PER_HOUR` | `--rooms-per-address-per-hour` | 10 | R31-2 |
| `PM_RELAY_MAX_OPEN_ROOMS` | `--max-open-rooms` | 100 | R31-2 |
| `PM_RELAY_ROOM_FRAMES_PER_SECOND` | `--room-frames-per-second` | 2 | R31-2 |
| `PM_RELAY_ROOM_FRAMES_PER_DAY` | `--room-frames-per-day` | 7200 | R31-2 |
| `PM_RELAY_VIEWER_REQUESTS_PER_MINUTE` | `--viewer-requests-per-minute` | 120 | R31-2 |
| `PM_RELAY_SEATS_PER_ADDRESS` | `--seats-per-address` | 4 | R31-2 |
| `PM_RELAY_ROOM_RING_BYTES` | `--room-ring-bytes` | 2097152 | R31-2 |
| `PM_RELAY_GLOBAL_ROOM_BYTES` | `--global-room-bytes` | 268435456 | R31-2 |
| `PM_RELAY_LIMIT_DEVICE_PER_MINUTE` | none | 60 | R31-2 |
| `PM_RELAY_LIMIT_DEVICE_PER_DAY` | none | 5000 | R31-2 |
| `PM_RELAY_LIMIT_TEAM_PER_MINUTE` | none | 600 | R31-2 |
| `PM_RELAY_LIMIT_DEVICE_REQUESTS_PER_MINUTE` | none | 300 | R31-2 |
| `PM_RELAY_LIMIT_JOINS_PER_INVITE` | none | 5 | R31-2 |
| `PM_RELAY_LIMIT_JOINS_PER_TEAM_PER_DAY` | none | 200 | R31-2 |
| `PM_RELAY_LIMIT_JOINS_PER_ADDRESS_PER_HOUR` | none | 10 | R31-2 |
| `PM_RELAY_LIMIT_UNAUTH_PER_IP_PER_HOUR` | none | 30 | R31-2 |
| `PM_RELAY_LIMIT_MAX_WAIT_S` | none | 25 | R31-2 |
| `PM_RELAY_LIMIT_TEAM_MAX_ENVELOPES` | none | 20000 | R31-2 |
| `PM_RELAY_LIMIT_TEAM_MAX_BYTES` | none | 134217728 | R31-2 |
| `PM_RELAY_LIMIT_SOCKETS_PER_DEVICE` | none | 4 | R31-2 |
| `PM_RELAY_LIMIT_MAX_WINDOWS` | none | 500000 | R31-2 |
| `PM_RELAY_LIMIT_MAX_SEEN_SIGS` | none | 2000000 | R31-2 |
| `PM_RELAY_OWNER_KEY` | `--owner-key` | none | R31-5b |
| `PM_RELAY_REGISTRATION` (`open` or `owner`) | `--registration` | `owner` if an owner key is set, else `open` | R31-5b |
| `PM_RELAY_MAX_DB_BYTES` | `--max-db-bytes` | 1073741824 (1 GiB) | R31-6 |
| `PM_RELAY_LIMIT_PROBE_PER_ADDRESS_PER_MINUTE` | none | 60 | R31-7 |
| `PM_RELAY_LIMIT_AUTH_FAILURES_PER_ADDRESS_PER_MINUTE` | none | 30 | file 35 R35-2 |
| `RUST_LOG` (not a `PM_RELAY_` name) | none | `info` (relay facts §1a) | unchanged |

The `PM_RELAY_LIMIT_*` suffixes match the Worker's `LIMIT_*` names (relay facts §3) where both exist, so one table of
limits serves both.

## 4. Endpoints added by this file

| Method and path | Auth | Reply | Slice |
|---|---|---|---|
| `GET /v1/version` | none | 200 `{"name":"pm-relay","version":"0.2.0","protocol":"pm-relay/1","commit":"<sha or unknown>","registration":"owner","owner":"<key id or null>","features":["teams","rooms","admin","probe"]}` | R31-3, R31-5, R31-7 |
| `GET /v1/ready` | none | 200 `{"ok":true,"db":"ok"}`; 503 `{"ok":false,"db":"the database can't be written"}` | R31-3 |
| `GET /v1/admin/status` | owner | 200 `{"version":"0.2.0","uptimeS":12,"teams":1,"envelopes":40,"dbBytes":81920,"maxDbBytes":1073741824,"registration":"owner"}`; 401 `signed by the owner required`; 401 `request replayed`; 403 `not signed by the relay's owner`; 404 `not found` when no owner key | R31-5, R31-6 |
| `GET /v1/probe/ws` | none, 60 per address per minute | WebSocket: one text frame `{"probe":"pm-relay/1"}`, then close; without upgrade 426 `use a WebSocket`; over the limit 429 `slow down` | R31-7 |
| `GET /v1/health` | none | **unchanged**: exactly `{"ok":true,"protocol":"pm-relay/1"}` (HOSTED-RELAY.md §5 depends on it) | none |

File 34 adds `GET /v1/admin/backup` (R34-4) and `PUT /v1/admin/restore` (R34-8).

## 5. iroh relay: co-host or not

Spaces uses iroh (spaces facts §1: `iroh = "=1.4.1"`, `iroh_net.rs` "optional relay", `BindSpec.relays`). Spaces
decision 8 default: "public relays in Labs; our relay/server can run one for self-hosting and the paid plan"
(spaces facts §7.4).

**Decision for v1: do not co-host** (DECISION-OPEN D-6 in file 30, default: no). Reasons:

1. Spaces does not exist yet (spaces facts "Could not find" item 5). Nothing would use the iroh relay.
2. Railway gives each service one public HTTP port behind its proxy and no public UDP. An iroh relay can forward over
   HTTPS, but QUIC address discovery needs UDP. Behaviour of the pinned iroh relay behind a TLS-ending proxy is not
   known (NF-21).
3. An open iroh relay forwards traffic for any node id, so a stranger could use the person's bandwidth bill. Access
   control must follow the Spaces member lists, which do not exist yet.
4. The Cloudflare fallback cannot run an iroh relay at all (it is a native Rust server).
5. Putting it in the same process as `pm-relay` would add the `iroh-relay` server crate to `relay/` with its own
   dependency tree; a crash in either would take down both.

Later (slice R31-9, blocked): a **second Railway service** in the same project, running an iroh relay image pinned to
the version that matches `iroh = "=1.4.1"`, with access limited to node ids the owner pushes. Its address is reported
in `GET /v1/version` as `"iroh":"https://…"` so the app can learn it in one call.

## 6. Spaces sealed mailbox

Spaces decision 9 default: the offline mailbox is off (spaces facts §7.4). When it is turned on, a space can use the
relay's existing team mailbox without relay changes: a space has an owner-signed member list, like a team root-signed
access list (relay facts §5 `TeamRegistration`, `SignedAcl`), and the mailbox already stores only sealed envelopes
for 7 days (`MAILBOX_TTL_MS`). The Spaces plan decides the mapping. On a one-click relay with owner-only registration,
the space owner's app must sign the registration with the relay owner key, so only spaces the relay owner makes can
use it (same rule as teams).

## 7. Logging rules

- Exactly one access line per request: `<METHOD> <route pattern> <status> <ms>ms`, for example
  `POST /v1/teams/:team/envelopes 200 3ms` (the pattern exactly as written in `router`, see NF-4).
- Status 400 and above at `info`; below 400 at `debug`. Health checks every 30 s from Docker therefore do not fill the
  log at the default level `info`.
- Never logged: path values (team ids, room ids, slot names), query strings, any header (`x-pm-auth`, `x-pm-owner`,
  `x-pm-room-read`, `x-pm-room-seat`, `X-Forwarded-For`), request or reply bodies, the client's address, key ids,
  the owner key.
- One start line from `main.rs`: `pm-relay <version> listening on <addr>, registration <open|owner>`.
- A guard test (R31-4) fails the build if any other file in `relay/server/src` logs.

## 8. Persistent volume

- The database is one SQLite file plus its `-wal` and `-shm` files, because `Relay::open` sets
  `PRAGMA journal_mode = WAL` (relay facts §2). All three must be on the volume. The image keeps them in `/data`
  (relay facts §4, `VOLUME /data`).
- Railway: one volume mounted at `/data` on the single `pm-relay` service (file 32). One replica only: SQLite has one
  writer, and the in-memory replay guard and rate windows are per process.
- Backups use `VACUUM INTO` (file 34 R34-4), which gives a consistent single file while the relay runs.
- The data-folder write check at start (R31-6) turns "volume owned by another user" into exit code 3 and the line
  `pm-relay: the data folder /data can't be written. In a container, give the volume to user 10001 or run as root.`

## 9. Limits added

| Limit | Default | Over the limit | Slice |
|---|---|---|---|
| Whole database (pages in use × page size) | 1 GiB | 507 `the relay is full` on new team, state, envelope and join writes | R31-6 |
| WebSocket probe per address | 60 per minute | 429 `slow down` | R31-7 |
| Failed signatures per address | 30 per minute | 429 `slow down` | file 35 R35-2 |

---

## Slices

### R31-1 — Prove the Dockerfile builds and runs

Size: Haiku. Deps: none.

**Goal**
Build `relay/Dockerfile` for the first time, run it, and record the proof.

**Read first**
- `relay/Dockerfile` — the two stages and the ENTRYPOINT you are about to test (relay facts §4).
- `relay/.dockerignore` — today it lists only `target`, `worker/node_modules`, `worker/.wrangler` (relay facts §1).
- `relay/HOSTED-RELAY.md` section 10 — the "Not verified" line you will update.
- `docs/features/relay-worker.md` line 134 — the "written but not built" line you will update.

**Files**
- edit `relay/.dockerignore`
- edit `relay/HOSTED-RELAY.md`
- edit `docs/features/relay-worker.md`

**Steps**
1. Run `docker version --format '{{.Server.Version}}'`. If it prints an error about the engine, stop (see If it fails).
2. Replace the whole of `relay/.dockerignore` with the content in Code. Reason: the folder holds
   `relay.sqlite`, `relay.sqlite-shm`, `relay.sqlite-wal` (relay facts §1) and they must not be sent to the build.
3. Run the build command from Run, step 2. It downloads `rust:1.96-slim-bookworm`; allow 20 minutes.
4. Run the container, check health, check the file in `/data`, wait for Docker's health status.
5. Run the app's relay tests against the container (Run step 6).
6. Restart the container and check that the database file is still there.
7. Check that extra flags after the image name reach the binary (`--help`).
8. Stop and remove only the container id you started, then remove the test volume.
9. In `relay/HOSTED-RELAY.md` section 10, replace the "never built" statement with:
   `Docker image built and run on 2026-11-<day> (Docker 28.3.2, Windows, Linux containers): health OK, Docker health
   "healthy", data kept across restart, app relay tests passed against it.` (Use the real day.)
10. In `docs/features/relay-worker.md` line 134, replace the sentence with:
    `relay/Dockerfile was built and run on 2026-11-<day>; see relay/HOSTED-RELAY.md section 10.`
11. Commit with message `relay: first Docker build proven` and the attribution line.

**Code**
`relay/.dockerignore`, whole file:
```
target
worker/node_modules
worker/.wrangler
worker/dist-bundle
viewer/node_modules
*.sqlite
*.sqlite-shm
*.sqlite-wal
```

**Tests to write**
No new test code. The proof is the command output in Run, recorded in the commit message body (paste the health reply
and the Docker health status) and in the two doc lines.

**Run**
From the repo root `C:\Users\Ren-pc\Desktop\work2\puppet-master`, in PowerShell:
1. `docker version --format '{{.Server.Version}}'` → a version such as `28.3.2`.
2. `docker build -t pm-relay:smoke relay` → ends with `naming to docker.io/library/pm-relay:smoke` and no `ERROR`.
3. `$id = docker run -d -p 127.0.0.1:18787:8787 -v pm-relay-smoke-data:/data pm-relay:smoke; $id` → a 64-character id.
4. `curl.exe -s --retry 15 --retry-connrefused --retry-delay 1 http://127.0.0.1:18787/v1/health` →
   exactly `{"ok":true,"protocol":"pm-relay/1"}`.
5. `docker exec $id ls -l /data` → lists `relay.sqlite` owned by `pmrelay`.
6. `$env:PM_RELAY_TEST_URL='http://127.0.0.1:18787'; $env:CARGO_TARGET_DIR='D:/pm-night/target-r31-1'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib team::tests_v1 -- --test-threads=1`
   → `test result: ok.` with `0 failed`. (The exact test that reads `PM_RELAY_TEST_URL` is at `team/tests_v1.rs`
   line 497, relay facts §11; its name is NF-34. The module filter runs it.)
7. `Start-Sleep -Seconds 40; docker inspect --format '{{.State.Health.Status}}' $id` → `healthy`.
8. `docker restart $id; curl.exe -s --retry 15 --retry-connrefused --retry-delay 1 http://127.0.0.1:18787/v1/health; docker exec $id ls -l /data`
   → the same health reply, and `relay.sqlite` is still listed.
9. `docker run --rm pm-relay:smoke --help` → the flag list from `main.rs`, exit code 0.
10. `docker stop $id; docker rm $id; docker volume rm pm-relay-smoke-data` → prints the id, the id, `pm-relay-smoke-data`.

**Done when**
- [ ] Step 4 printed exactly `{"ok":true,"protocol":"pm-relay/1"}`.
- [ ] Step 6 ended `test result: ok.` with `0 failed`.
- [ ] Step 7 printed `healthy`.
- [ ] Step 8 showed the database survived a restart.
- [ ] `relay/.dockerignore`, `relay/HOSTED-RELAY.md`, `docs/features/relay-worker.md` changed and committed.

**Do NOT**
- Do not start Docker Desktop yourself: it opens a window on the primary screen. Ask the founder.
- Do not use `docker rm -f` on any container you did not start; stop only `$id`.
- Do not change `relay/Dockerfile` in this slice (R31-8 does). This slice proves today's file.
- Do not use the default cargo target folder; use `D:/pm-night/target-r31-1`.
- Do not publish the image anywhere.

**If it fails**
1. `error during connect` / engine not running → stop and report "Docker engine not running" to the founder.
2. The build fails at `cargo build --release --locked` with a lock error → the lock file is out of date with
   `relay/Cargo.toml`. Do not run `cargo update` in this slice; report the exact error line, because changing the lock
   is a separate change.
3. Health never answers and `docker logs $id` shows `unable to open database file` → the volume is not writable by
   uid 10001; record that line in the report. R31-6 adds a clear exit message for this case.

---

### R31-2 — Settings from environment variables

Size: Haiku. Deps: none.

**Goal**
The `pm-relay` binary reads `PM_RELAY_*` variables, with command-line flags still winning.

**Read first**
- `relay/server/src/main.rs` — how flags are parsed, where `Config` and `Limits` are built, what exit code 2 is used
  for (relay facts §2; exact shape is NF-1).
- `relay/server/src/lib.rs` lines 59-128 — `Limits` fields and their types (relay facts §2).
- `relay/server/tests/protocol.rs` — how existing tests start a relay and talk HTTP (to copy the style).

**Files**
- create `relay/server/src/env_config.rs`
- edit `relay/server/src/lib.rs` (add `pub mod env_config;` next to `mod rooms;`)
- edit `relay/server/src/main.rs`
- create `relay/server/tests/env.rs`
- edit `relay/README.md` (add the table from §3 of this plan, rows marked R31-2, under a new heading
  `## Settings from environment variables`)

**Steps**
1. Create `env_config.rs` with the code below.
2. In `lib.rs` add `pub mod env_config;` on the line after `mod rooms;`.
3. In `main.rs`, find where the argument list is read. Replace that read with the "after" block in Code, so env
   arguments come first and real arguments after them (later wins for single-value flags).
4. In `main.rs`, after `Limits` is final and before the relay starts, call `apply_limits` (Code).
5. Write the unit tests inside `env_config.rs` and the binary tests in `tests/env.rs`.
6. Add the README table.
7. Run the tests, commit `relay: settings from PM_RELAY_ environment variables`.

**Code**
`relay/server/src/env_config.rs`:
```rust
//! Settings from environment variables, so a container runs with no flags.
//! Strength: built-in default < environment variable < command-line flag.
//! Every name starts with `PM_RELAY_`; an unknown one is an error so a typo is never silent.

/// What a flag variable holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Passed as `--flag value`.
    Text,
    /// A number (whole or with a decimal point), passed as `--flag value`; the flag parser does the real parse.
    Number,
    /// `1` adds `--flag`; `0` or empty adds nothing.
    Switch,
    /// Comma list; each item adds `--flag item`.
    List,
}

/// Variables that stand for a flag: (variable, flag, kind).
pub const FLAG_VARS: &[(&str, &str, Kind)] = &[
    ("PM_RELAY_LISTEN", "--listen", Kind::Text),
    ("PM_RELAY_DB", "--db", Kind::Text),
    ("PM_RELAY_MEMORY", "--memory", Kind::Switch),
    ("PM_RELAY_TRUSTED_PROXY", "--trusted-proxy", Kind::List),
    ("PM_RELAY_ROOMS_PER_ADDRESS_PER_HOUR", "--rooms-per-address-per-hour", Kind::Number),
    ("PM_RELAY_MAX_OPEN_ROOMS", "--max-open-rooms", Kind::Number),
    ("PM_RELAY_ROOM_FRAMES_PER_SECOND", "--room-frames-per-second", Kind::Number),
    ("PM_RELAY_ROOM_FRAMES_PER_DAY", "--room-frames-per-day", Kind::Number),
    ("PM_RELAY_VIEWER_REQUESTS_PER_MINUTE", "--viewer-requests-per-minute", Kind::Number),
    ("PM_RELAY_SEATS_PER_ADDRESS", "--seats-per-address", Kind::Number),
    ("PM_RELAY_ROOM_RING_BYTES", "--room-ring-bytes", Kind::Number),
    ("PM_RELAY_GLOBAL_ROOM_BYTES", "--global-room-bytes", Kind::Number),
];

/// Team limits without a flag: (variable, `Limits` field name).
pub const LIMIT_VARS: &[(&str, &str)] = &[
    ("PM_RELAY_LIMIT_DEVICE_PER_MINUTE", "device_per_minute"),
    ("PM_RELAY_LIMIT_DEVICE_PER_DAY", "device_per_day"),
    ("PM_RELAY_LIMIT_TEAM_PER_MINUTE", "team_per_minute"),
    ("PM_RELAY_LIMIT_DEVICE_REQUESTS_PER_MINUTE", "device_requests_per_minute"),
    ("PM_RELAY_LIMIT_JOINS_PER_INVITE", "joins_per_invite"),
    ("PM_RELAY_LIMIT_JOINS_PER_TEAM_PER_DAY", "joins_per_team_per_day"),
    ("PM_RELAY_LIMIT_JOINS_PER_ADDRESS_PER_HOUR", "joins_per_address_per_hour"),
    ("PM_RELAY_LIMIT_UNAUTH_PER_IP_PER_HOUR", "unauthenticated_per_ip_per_hour"),
    ("PM_RELAY_LIMIT_MAX_WAIT_S", "max_wait_s"),
    ("PM_RELAY_LIMIT_TEAM_MAX_ENVELOPES", "team_max_envelopes"),
    ("PM_RELAY_LIMIT_TEAM_MAX_BYTES", "team_max_bytes"),
    ("PM_RELAY_LIMIT_SOCKETS_PER_DEVICE", "sockets_per_device"),
    ("PM_RELAY_LIMIT_MAX_WINDOWS", "max_windows"),
    ("PM_RELAY_LIMIT_MAX_SEEN_SIGS", "max_seen_sigs"),
];

/// `PM_RELAY_` names that are not runtime settings and are skipped.
pub const IGNORED: &[&str] = &["PM_RELAY_COMMIT", "PM_RELAY_TEST_URL"];

fn is_number(v: &str) -> bool {
    let mut parts = v.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.map_or(true, |f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

fn known(name: &str) -> bool {
    FLAG_VARS.iter().any(|(v, _, _)| *v == name)
        || LIMIT_VARS.iter().any(|(v, _)| *v == name)
        || IGNORED.contains(&name)
}

/// Turns the environment into flag arguments. `vars` is `std::env::vars().collect()`.
pub fn env_args(vars: &[(String, String)]) -> Result<Vec<String>, String> {
    for (name, _) in vars {
        if name.starts_with("PM_RELAY_") && !known(name) {
            return Err(format!("Unknown setting {name}. See relay/README.md for the list."));
        }
    }
    let get = |n: &str| vars.iter().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_owned());
    let mut out = Vec::new();
    for (var, flag, kind) in FLAG_VARS {
        let Some(value) = get(var) else { continue };
        match kind {
            Kind::Text => {
                if value.is_empty() { continue; }
                out.push((*flag).to_owned());
                out.push(value);
            }
            Kind::Number => {
                if !is_number(&value) { return Err(format!("{var} must be a number")); }
                out.push((*flag).to_owned());
                out.push(value);
            }
            Kind::Switch => match value.as_str() {
                "1" => out.push((*flag).to_owned()),
                "" | "0" => {}
                _ => return Err(format!("{var} must be 1 or 0")),
            },
            Kind::List => {
                for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    out.push((*flag).to_owned());
                    out.push(item.to_owned());
                }
            }
        }
    }
    Ok(out)
}

/// Reads the `PM_RELAY_LIMIT_*` values: (field name, value).
pub fn limit_values(vars: &[(String, String)]) -> Result<Vec<(&'static str, u64)>, String> {
    let mut out = Vec::new();
    for (var, field) in LIMIT_VARS {
        if let Some((_, v)) = vars.iter().find(|(k, _)| k == var) {
            let n: u64 = v.trim().parse().map_err(|_| format!("{var} must be a whole number"))?;
            out.push((*field, n));
        }
    }
    Ok(out)
}

/// Writes the values into `limits`. Field types come from `Limits`; `as _` converts.
pub fn apply_limits(limits: &mut crate::Limits, values: &[(&'static str, u64)]) {
    for (field, v) in values {
        match *field {
            "device_per_minute" => limits.device_per_minute = *v as _,
            "device_per_day" => limits.device_per_day = *v as _,
            "team_per_minute" => limits.team_per_minute = *v as _,
            "device_requests_per_minute" => limits.device_requests_per_minute = *v as _,
            "joins_per_invite" => limits.joins_per_invite = *v as _,
            "joins_per_team_per_day" => limits.joins_per_team_per_day = *v as _,
            "joins_per_address_per_hour" => limits.joins_per_address_per_hour = *v as _,
            "unauthenticated_per_ip_per_hour" => limits.unauthenticated_per_ip_per_hour = *v as _,
            "max_wait_s" => limits.max_wait_s = *v as _,
            "team_max_envelopes" => limits.team_max_envelopes = *v as _,
            "team_max_bytes" => limits.team_max_bytes = *v as _,
            "sockets_per_device" => limits.sockets_per_device = *v as _,
            "max_windows" => limits.max_windows = *v as _,
            "max_seen_sigs" => limits.max_seen_sigs = *v as _,
            _ => {}
        }
    }
}
```
If a `Limits` field is not a plain number (for example a `Duration`), write that arm as
`limits.max_wait_s = std::time::Duration::from_secs(*v)` to match the field's type you read in `lib.rs`.

`main.rs`, the argument read. Before (shape; the exact line is NF-1):
```rust
let args: Vec<String> = std::env::args().skip(1).collect();
```
After:
```rust
let vars: Vec<(String, String)> = std::env::vars().collect();
let mut args = match pm_relay::env_config::env_args(&vars) {
    Ok(a) => a,
    Err(e) => {
        eprintln!("pm-relay: {e}");
        std::process::exit(2);
    }
};
args.extend(std::env::args().skip(1));
let limit_values = match pm_relay::env_config::limit_values(&vars) {
    Ok(v) => v,
    Err(e) => {
        eprintln!("pm-relay: {e}");
        std::process::exit(2);
    }
};
```
After `Limits` is built (where `Config` gets its limits):
```rust
pm_relay::env_config::apply_limits(&mut config.limits, &limit_values);
```
(`config.limits` is the field name to confirm in NF-1; use the real name.)

**Tests to write**
Unit tests in `env_config.rs` (`#[cfg(test)] mod tests`), with `fn v(pairs: &[(&str, &str)]) -> Vec<(String, String)>`:
- `empty_env_gives_no_args`: `env_args(&v(&[]))` → `Ok(vec![])`.
- `listen_and_db`: `PM_RELAY_LISTEN=0.0.0.0:8787`, `PM_RELAY_DB=/data/relay.sqlite` → `["--listen","0.0.0.0:8787","--db","/data/relay.sqlite"]`.
- `memory_switch`: `PM_RELAY_MEMORY=1` → `["--memory"]`; `PM_RELAY_MEMORY=0` → `[]`; `PM_RELAY_MEMORY=yes` → `Err("PM_RELAY_MEMORY must be 1 or 0")`.
- `trusted_proxy_list`: `PM_RELAY_TRUSTED_PROXY="127.0.0.1, 10.0.0.0/8"` → `["--trusted-proxy","127.0.0.1","--trusted-proxy","10.0.0.0/8"]`.
- `room_flag_number`: `PM_RELAY_MAX_OPEN_ROOMS=5` → `["--max-open-rooms","5"]`; `PM_RELAY_ROOM_FRAMES_PER_SECOND=0.5` → `["--room-frames-per-second","0.5"]`.
- `bad_number`: `PM_RELAY_MAX_OPEN_ROOMS=abc` → `Err("PM_RELAY_MAX_OPEN_ROOMS must be a number")`.
- `unknown_name_is_an_error`: `PM_RELAY_LISTN=x` → `Err("Unknown setting PM_RELAY_LISTN. See relay/README.md for the list.")`.
- `ignored_names_are_skipped`: `PM_RELAY_TEST_URL=http://x` → `Ok(vec![])`.
- `team_limits_apply`: `PM_RELAY_LIMIT_DEVICE_PER_MINUTE=7` → after `apply_limits` on `Limits::default()`, `device_per_minute == 7` and `device_per_day == 5000`.
- `team_limit_bad_value`: `PM_RELAY_LIMIT_DEVICE_PER_DAY=-1` → `Err("PM_RELAY_LIMIT_DEVICE_PER_DAY must be a whole number")`.

Binary tests in `tests/env.rs` (use `env!("CARGO_BIN_EXE_pm-relay")`; a helper `free_port()` binds
`std::net::TcpListener::bind("127.0.0.1:0")`, reads the port, drops the listener; a helper `health(port)` sends
`GET /v1/health HTTP/1.0\r\nHost: localhost\r\n\r\n` over `std::net::TcpStream`, retrying for up to 5 s; each test
kills only its own `Child`):
- `binary_listens_where_env_says`: env `PM_RELAY_LISTEN=127.0.0.1:<port>`, `PM_RELAY_MEMORY=1` → reply body contains `{"ok":true,"protocol":"pm-relay/1"}`.
- `binary_flag_beats_env`: env `PM_RELAY_LISTEN=127.0.0.1:1`, `PM_RELAY_MEMORY=1`, arg `--listen 127.0.0.1:<port>` → health answers on `<port>`.
- `binary_unknown_env_exits_2`: env `PM_RELAY_LISTN=x` → exit code 2, stderr contains `Unknown setting PM_RELAY_LISTN`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-2'; cargo test --manifest-path relay/Cargo.toml -p pm-relay`
→ every `test result:` line says `ok.` with `0 failed`; the 10 unit tests and 3 binary tests above are listed as `ok`.

**Done when**
- [ ] `docker`-free run: `$env:PM_RELAY_MEMORY='1'; $env:PM_RELAY_LISTEN='127.0.0.1:18788'; cargo run --manifest-path relay/Cargo.toml -p pm-relay` answers health on 18788 (stop it with Ctrl+C).
- [ ] All tests in Run pass.
- [ ] README has the new table.

**Do NOT**
- Do not read any variable outside `env_config.rs` (one place reads the environment).
- Do not change any default value.
- Do not make unknown variables a warning; they are an error (exit 2).
- Do not use the default cargo target folder.

**If it fails**
1. `apply_limits` does not compile on one arm → that field's type is not a number; write that arm with the field's real
   type (see the note under Code).
2. `binary_flag_beats_env` binds port 1 → the flag parser keeps the first `--listen` instead of the last; change the
   parser so a repeated single-value flag keeps the last value (env args come first).
3. Binary tests hang → the child was not killed on a failed assert; wrap the child in a guard struct whose `Drop`
   calls `kill()` and `wait()`.

---

### R31-3 — `/v1/version` and `/v1/ready`

Size: Haiku. Deps: none.

**Goal**
The relay says what it is and whether its database can be written, without auth.

**Read first**
- `relay/server/src/lib.rs` lines 924-935 — `router` and how `/v1/health` is routed and which state it uses (NF-2).
- `relay/server/src/lib.rs` line 326 — `Relay::open` and how the connection is held (NF-5).
- `relay/server/tests/protocol.rs` — the HTTP helper to copy.

**Files**
- create `relay/server/src/ops.rs`
- edit `relay/server/src/lib.rs` (add `mod ops;`, two routes, `Relay::ping`)
- create `relay/server/tests/ops.rs`
- edit `relay/README.md` (endpoint rows for version and ready)

**Steps**
1. Create `ops.rs` with Code.
2. In `lib.rs` add `pub mod ops;` after `pub mod env_config;`.
3. Add `Relay::ping` to the `impl Relay` block.
4. In `router`, after the `/v1/health` route, add `.route("/v1/version", get(ops::version))` and
   `.route("/v1/ready", get(ops::ready))`, using the same state the health route uses.
5. Write tests, run, commit `relay: version and ready endpoints`.

**Code**
`relay/server/src/ops.rs`:
```rust
//! Operator routes. No auth; nothing here is secret.

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = match option_env!("PM_RELAY_COMMIT") {
    Some(c) => c,
    None => "unknown",
};

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VersionReply {
    pub name: &'static str,
    pub version: &'static str,
    pub protocol: &'static str,
    pub commit: &'static str,
    pub features: Vec<&'static str>,
}

pub fn version_reply() -> VersionReply {
    VersionReply {
        name: "pm-relay",
        version: VERSION,
        protocol: pm_relay_protocol::PROTOCOL,
        commit: COMMIT,
        features: vec!["teams", "rooms"],
    }
}

pub async fn version() -> Json<VersionReply> {
    Json(version_reply())
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ReadyReply {
    pub ok: bool,
    pub db: &'static str,
}

/// `S` is the router's shared state type (NF-2); it must give access to the `Relay` store.
pub async fn ready(State(state): State<crate::SharedState>) -> (StatusCode, Json<ReadyReply>) {
    match state.relay().ping() {
        Ok(()) => (StatusCode::OK, Json(ReadyReply { ok: true, db: "ok" })),
        Err(()) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyReply { ok: false, db: "the database can't be written" }),
        ),
    }
}
```
Replace `crate::SharedState` and `state.relay()` with the real state type and the real way the health handler reaches
the store (NF-2).

`Relay::ping` in `lib.rs` (connection access per NF-5; if the connection sits in a `Mutex<Connection>` field named
`conn`, this is the body):
```rust
/// True when the database answers and can take a write lock. Never returns the error text (it can name paths).
pub fn ping(&self) -> Result<(), ()> {
    let conn = self.conn.lock().map_err(|_| ())?;
    conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").map_err(|_| ())
}
```

**Tests to write**
In `tests/ops.rs`, starting a relay the way `tests/protocol.rs` does:
- `version_reports_name_protocol_and_version`: `GET /v1/version` → 200; JSON `name == "pm-relay"`, `protocol == "pm-relay/1"`, `version == env!("CARGO_PKG_VERSION")`, `features` contains `"teams"` and `"rooms"`.
- `ready_is_ok_on_a_file_db`: relay with a db file in a temp dir → `GET /v1/ready` → 200 body exactly `{"ok":true,"db":"ok"}`.
- `ready_is_ok_in_memory`: `--memory` style config → 200 `{"ok":true,"db":"ok"}`.
- `health_body_is_unchanged`: `GET /v1/health` → body exactly `{"ok":true,"protocol":"pm-relay/1"}`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-3'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test ops`
→ `test result: ok. 4 passed; 0 failed`. Then `cargo test --manifest-path relay/Cargo.toml` → all `ok.`, `0 failed`.

**Done when**
- [ ] 4 new tests pass; old tests pass.
- [ ] `/v1/health` reply unchanged.

**Do NOT**
- Do not put any count, address, key or path in `/v1/version` or `/v1/ready`.
- Do not change the `/v1/health` handler.
- Do not run a write that is committed in `ping`.

**If it fails**
1. `State` type mismatch → the router uses a different state type; use exactly the type in the health handler's
   signature.
2. `option_env!` in a `const` errors on an old compiler → replace the const with `pub fn commit() -> &'static str`
   containing the same `match`.
3. `ready_is_ok_on_a_file_db` gets 503 → another test holds a write transaction on the same file; give each test its
   own temp dir.

---

### R31-4 — Access log without secrets

Size: Haiku. Deps: none.

**Goal**
One log line per request with the route pattern only, and a guard test that keeps logging in two files.

**Read first**
- `relay/server/src/main.rs` line 24 — the `tracing_subscriber::EnvFilter` set-up (relay facts §1a).
- `relay/server/src/lib.rs` `router` (lines 924-935) — where to add the layer.
- `relay/server/Cargo.toml` — axum version, to pick the middleware signature (NF-4).
- Every `tracing::`/`info!`/`warn!`/`error!`/`debug!` call already in `relay/server/src` (NF-25):
  `rg -n "tracing::|info!\(|warn!\(|error!\(|debug!\(|trace!\(|println!\(|eprintln!\(" relay/server/src`.

**Files**
- create `relay/server/src/access.rs`
- edit `relay/server/src/lib.rs` (add `mod access;` and one `.route_layer(...)` line at the end of `router`)
- create `relay/server/tests/access_log.rs`
- create `relay/server/tests/log_guard.rs`

**Steps**
1. Create `access.rs` (Code).
2. In `router`, add `.route_layer(axum::middleware::from_fn(access::log))` after the last `.route(...)` and before any
   `.with_state(...)`. `route_layer` runs after routing, so `MatchedPath` is set.
3. Write the two test files.
4. Run the `rg` command from Read first. For each hit outside `main.rs` and `access.rs`: if the line prints a fixed
   message with no value from a header, body, address, team id, room id or key, add `"<file>:<exact line text>"` to
   `ALLOWED_EXISTING` in `log_guard.rs`; otherwise change the line to a fixed message without the value.
5. Run tests, commit `relay: access log with route patterns only`.

**Code**
`relay/server/src/access.rs` (axum 0.7 form; on axum 0.6 the types are `Request<B>` and `Next<B>`, see NF-4):
```rust
//! One line per request: method, route pattern, status, time. Never path values (team ids, room ids), the query,
//! headers, bodies or the client's address.

use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use std::time::Instant;

pub fn line(method: &str, route: &str, status: u16, ms: u128) -> String {
    format!("{method} {route} {status} {ms}ms")
}

pub async fn log(req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_owned();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| "?".to_owned());
    let started = Instant::now();
    let res = next.run(req).await;
    let status = res.status().as_u16();
    let text = line(&method, &route, status, started.elapsed().as_millis());
    if status >= 400 {
        tracing::info!(target: "pm_relay::access", "{text}");
    } else {
        tracing::debug!(target: "pm_relay::access", "{text}");
    }
    res
}
```

`relay/server/tests/log_guard.rs`:
```rust
//! Only main.rs and access.rs may log. Keeps team ids, keys and addresses out of logs.
use std::{fs, path::Path};

const MACROS: &[&str] = &["tracing::", "info!(", "warn!(", "error!(", "debug!(", "trace!(", "println!(", "eprintln!(", "dbg!("];
const ALLOWED_FILES: &[&str] = &["main.rs", "access.rs"];
const ALLOWED_EXISTING: &[&str] = &[];
const NEVER_IN_MAIN: &[&str] = &["owner", "token", "auth", "header", "body", "peer"];

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { walk(&p, out) } else if p.extension().map_or(false, |x| x == "rs") { out.push(p) }
    }
}

#[test]
fn only_main_and_access_log() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&f).unwrap();
        for line in text.lines() {
            let code = line.split("//").next().unwrap_or("");
            if !MACROS.iter().any(|m| code.contains(m)) { continue; }
            if name == "main.rs" {
                let low = code.to_lowercase();
                for w in NEVER_IN_MAIN { assert!(!low.contains(w), "main.rs logs `{w}`: {line}"); }
                continue;
            }
            if ALLOWED_FILES.contains(&name.as_str()) { continue; }
            let key = format!("{name}:{}", line.trim());
            assert!(ALLOWED_EXISTING.contains(&key.as_str()), "only main.rs and access.rs may log: {key}");
        }
    }
}
```
`relay/server/tests/access_log.rs`: one test, a global subscriber writing to a shared buffer:
```rust
use std::sync::{Arc, Mutex};
#[derive(Clone)]
struct Buf(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
```
Set it with `tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).with_writer(move || buf.clone()).init()`
once at the start of the single test.

**Tests to write**
- `access_line_format` (unit, inside `access.rs`): `line("POST", "/v1/teams/:team/envelopes", 200, 3)` → `"POST /v1/teams/:team/envelopes 200 3ms"`.
- `log_shows_pattern_not_values` (in `access_log.rs`): start a relay; `GET /v1/teams/TEAMSECRETID/state/roster` with no `x-pm-auth` → 401; the buffer contains `GET /v1/teams/` + the router's pattern for that route + ` 401` and does **not** contain `TEAMSECRETID`, `127.0.0.1` or `x-pm-auth`.
- `only_main_and_access_log` (in `log_guard.rs`): passes.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-4'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test access_log --test log_guard`
→ two `test result: ok. 1 passed; 0 failed` lines. Then the whole workspace: all `ok.`.

**Done when**
- [ ] Both new test files pass; the unit test passes; the old tests pass.
- [ ] At `RUST_LOG=info` a 200 request prints nothing; a 401 prints one line.

**Do NOT**
- Do not log `req.uri()`; it holds team ids and the query.
- Do not use `.layer(...)` for this middleware; `MatchedPath` is not set there.
- Do not add a dependency; `tracing` and `tracing_subscriber` are already used by `main.rs`.

**If it fails**
1. `Next` needs a type parameter → axum is 0.6; change to `Request<axum::body::Body>` and `Next<axum::body::Body>`.
2. The pattern in the log is `?` → the layer is applied with `.layer`; switch to `.route_layer`.
3. `init()` panics "a global default trace dispatcher has already been set" → another test in the same file set one;
   keep exactly one test in `access_log.rs`.

---

### R31-5 — Owner key, owner-only registration, `/v1/admin/status`

Size: Sonnet (signature checks and a protocol constant). Deps: R31-2, R31-3.

**Goal**
The server knows an optional owner public key, accepts owner-signed admin calls, and can refuse team registrations not
signed by the owner.

**Read first**
- `relay/protocol/src/lib.rs` — `auth_header`, `verify_request`, `auth_message`, `key_id`, `is_key_id`,
  `AUTH_HEADER`, `AUTH_SKEW_MS` (relay facts §5; exact parameter types NF-3).
- `relay/server/src/lib.rs` lines 466-490 — `authorize`, to copy how a header is read, verified and put into `SeenSigs`
  (relay facts §2; `SeenSigs` shape NF-27).
- `relay/server/src/lib.rs` — the `POST /v1/teams` handler and its per-address limit (relay facts §2 HTTP routes table);
  whether it takes `Json<TeamRegistration>` or bytes (NF-2).
- `relay/server/src/lib.rs` — `Config` struct and its `Default` (NF-1).
- `relay/server/tests/protocol.rs` — the team registration helper to copy.

**Files**
- edit `relay/protocol/src/lib.rs` (one constant)
- create `relay/server/src/owner.rs`
- edit `relay/server/src/lib.rs` (`Config` fields, `mod owner;`, register check, admin route, `Relay::counts`)
- edit `relay/server/src/ops.rs` (version reply gains `registration`, `owner`, feature `admin`)
- create `relay/server/tests/owner.rs`

**Steps**
1. In `protocol/src/lib.rs`, next to `AUTH_HEADER`, add
   `pub const OWNER_HEADER: &str = "x-pm-owner";` with doc comment
   `/// Header for requests signed by a relay's owner key. Same format as AUTH_HEADER.`
2. Create `owner.rs` (Code).
3. Add to `Config`: `pub owner_key: Option<String>` (default `None`) and `pub registration: owner::Registration`
   (default `Registration::Open`). Keep `#[derive(Default)]` or the manual `Default`, whichever `Config` has.
4. Add `started: std::time::Instant` to the shared state, set when `serve`/`spawn_local` builds it (NF-2).
5. Change the `POST /v1/teams` handler as in Code. If it takes `Json<TeamRegistration>`, change the extractor to
   `body: axum::body::Bytes` and parse with `serde_json::from_slice::<TeamRegistration>(&body)`, answering
   400 `bad registration` on error (the same text as today).
6. Add `Relay::counts` (Code) and the route `.route("/v1/admin/status", get(owner::admin_status))`.
7. Change `ops::version_reply` to take `&Config` (Code) and update the `version` handler to pass the config.
8. Write `tests/owner.rs`. Run, commit `relay: owner key and owner-only registration`.

**Code**
`relay/server/src/owner.rs`:
```rust
//! The relay's owner: one Ed25519 public key id set at start. There is no admin token.
//! Owner requests carry `x-pm-owner`, built exactly like a device's `x-pm-auth` but signed by the owner key.
//! No owner key: admin routes answer 404 and registration is open.

use axum::{extract::State, http::{HeaderMap, StatusCode, Uri}, response::{IntoResponse, Response}, Json};
use serde::Serialize;

pub const OWNER_HEADER: &str = pm_relay_protocol::OWNER_HEADER;
pub const NOT_OWNER: &str = "this relay only takes teams from its owner";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Registration {
    #[default]
    Open,
    Owner,
}

impl Registration {
    pub fn as_str(self) -> &'static str {
        match self { Self::Open => "open", Self::Owner => "owner" }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s { "open" => Some(Self::Open), "owner" => Some(Self::Owner), _ => None }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OwnerCheck { Missing, Bad, Replayed, Ok }

/// Checks `x-pm-owner` against `owner_key`. Records the signature in the replay memory only when it is valid.
pub fn check_owner(
    owner_key: &str,
    headers: &HeaderMap,
    method: &str,
    path_and_query: &str,
    body: &[u8],
    now_ms: u64,
    seen: &crate::SeenSigs,
) -> OwnerCheck {
    let Some(value) = headers.get(OWNER_HEADER).and_then(|v| v.to_str().ok()) else {
        return OwnerCheck::Missing;
    };
    // Same call `authorize` makes for x-pm-auth, with the owner key id as the expected signer (NF-3).
    if pm_relay_protocol::verify_request(owner_key, value, method, path_and_query, now_ms, body).is_err() {
        return OwnerCheck::Bad;
    }
    // Same replay call `authorize` makes (NF-27).
    if !seen.insert_once(value, now_ms) {
        return OwnerCheck::Replayed;
    }
    OwnerCheck::Ok
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AdminStatus {
    pub version: &'static str,
    pub uptime_s: u64,
    pub teams: u64,
    pub envelopes: u64,
    pub db_bytes: u64,
    pub registration: &'static str,
}

pub async fn admin_status(State(state): State<crate::SharedState>, headers: HeaderMap, uri: Uri) -> Response {
    let config = state.config();
    let Some(key) = config.owner_key.as_deref() else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/v1/admin/status");
    match check_owner(key, &headers, "GET", pq, b"", crate::now_ms(), state.seen()) {
        OwnerCheck::Ok => {}
        OwnerCheck::Missing => return (StatusCode::UNAUTHORIZED, "signed by the owner required").into_response(),
        OwnerCheck::Replayed => return (StatusCode::UNAUTHORIZED, "request replayed").into_response(),
        OwnerCheck::Bad => return (StatusCode::FORBIDDEN, "not signed by the relay's owner").into_response(),
    }
    let Ok(c) = state.relay().counts() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "the relay is busy, try again shortly").into_response();
    };
    Json(AdminStatus {
        version: crate::ops::VERSION,
        uptime_s: state.started().elapsed().as_secs(),
        teams: c.teams,
        envelopes: c.envelopes,
        db_bytes: c.db_bytes,
        registration: config.registration.as_str(),
    })
    .into_response()
}
```
Names `crate::SharedState`, `state.config()`, `state.seen()`, `state.relay()`, `state.started()`, `crate::now_ms()` and
`seen.insert_once(...)` stand for the real state type, its fields and the existing replay call; write them the way
`authorize` reaches the same things (NF-2, NF-27). The argument order of `verify_request` is NF-3: copy it from the
call inside `authorize`.

Register handler, new first lines (before the existing per-address limit):
```rust
let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/v1/teams");
let owner_signed = match (state.config().owner_key.as_deref(), headers.contains_key(owner::OWNER_HEADER)) {
    (Some(key), true) => match owner::check_owner(key, &headers, "POST", pq, &body, now_ms(), state.seen()) {
        owner::OwnerCheck::Ok => true,
        owner::OwnerCheck::Replayed => return (StatusCode::UNAUTHORIZED, "request replayed").into_response(),
        _ => return (StatusCode::FORBIDDEN, owner::NOT_OWNER).into_response(),
    },
    _ => false,
};
if state.config().registration == owner::Registration::Owner && !owner_signed {
    return (StatusCode::FORBIDDEN, owner::NOT_OWNER).into_response();
}
// Existing per-address registration limit runs only when !owner_signed. Wrap it in `if !owner_signed { ... }`.
```

`Relay::counts` in `lib.rs`:
```rust
pub struct Counts { pub teams: u64, pub envelopes: u64, pub db_bytes: u64 }

pub fn counts(&self) -> Result<Counts, ()> {
    let conn = self.conn.lock().map_err(|_| ())?;
    let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).map_err(|_| ());
    let teams = one("SELECT COUNT(*) FROM teams")?;
    let envelopes = one("SELECT COUNT(*) FROM envelopes")?;
    let pages = one("PRAGMA page_count")?;
    let free = one("PRAGMA freelist_count")?;
    let size = one("PRAGMA page_size")?;
    Ok(Counts { teams: teams as u64, envelopes: envelopes as u64, db_bytes: ((pages - free).max(0) * size) as u64 })
}
```

`ops.rs` changes:
```rust
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VersionReply {
    pub name: &'static str,
    pub version: &'static str,
    pub protocol: &'static str,
    pub commit: &'static str,
    pub registration: &'static str,
    pub owner: Option<String>,
    pub features: Vec<&'static str>,
}

pub fn version_reply(config: &crate::Config) -> VersionReply {
    let mut features = vec!["teams", "rooms"];
    if config.owner_key.is_some() { features.push("admin"); }
    VersionReply {
        name: "pm-relay",
        version: VERSION,
        protocol: pm_relay_protocol::PROTOCOL,
        commit: COMMIT,
        registration: config.registration.as_str(),
        owner: config.owner_key.clone(),
        features,
    }
}
```

**Tests to write**
In `tests/owner.rs`. Owner key: `ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])`; other key `[8u8; 32]`; id with
`pm_relay_protocol::key_id`. Header with `pm_relay_protocol::auth_header(&key, method, path, now_ms, body)`. Relay
started with `Config { owner_key: Some(id), registration: Registration::Owner, ..Config::default() }` unless the test
says "open relay".
- `owner_relay_refuses_unsigned_team_registration` → `POST /v1/teams` with a valid `TeamRegistration`, no `x-pm-owner` → 403, body `this relay only takes teams from its owner`.
- `owner_relay_accepts_owner_signed_registration` → same body plus owner header → 201.
- `other_key_is_refused` → header signed by key `[8u8;32]` → 403, body `this relay only takes teams from its owner`.
- `owner_signature_replay_is_refused` → the same header value sent twice on two registrations of two different teams → second answer 401 `request replayed`.
- `owner_signed_registration_skips_address_limit` → open relay with `unauthenticated_per_ip_per_hour = 1` and an owner key: first unsigned registration 201, second unsigned 429 `slow down`, third owner-signed 201.
- `open_relay_still_accepts_unsigned_registration` → `Config::default()` → 201.
- `joins_need_no_owner_signature_on_owner_relay` → owner-signed registration, then `POST /v1/teams/{team}/joins` with a `JoinBox` (copy the join helper from `tests/protocol.rs`) and no owner header → 202.
- `admin_status_needs_owner` → no header 401 `signed by the owner required`; other key 403 `not signed by the relay's owner`; owner 200 with JSON `teams == 0`, `registration == "owner"`.
- `admin_status_is_404_without_owner_key` → `Config::default()` → 404 `not found`.
- `version_shows_owner_mode` → `GET /v1/version` → `registration == "owner"`, `owner == id`, `features` contains `"admin"`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-5'; cargo test --manifest-path relay/Cargo.toml`
→ every line `ok.`, `0 failed`; `tests/owner.rs` shows `10 passed`.

**Done when**
- [ ] 10 new tests pass, all old tests pass (`tests/protocol.rs` 20, `tests/rooms.rs` 21 still green).
- [ ] With no owner key the relay behaves exactly as before (old tests prove it).

**Do NOT**
- Do not add any bearer token, password or shared secret.
- Do not log the owner key or the header.
- Do not apply owner-only mode to joins, envelopes, state, rooms or viewers.
- Do not put the signature into the replay memory before it is verified (that is the review M3 bug, spaces facts §8).

**If it fails**
1. `verify_request` wants a decoded key, not an id → convert with the same helper `authorize` uses on the device id.
2. Body bytes differ between signer and server (signature fails) → the server must verify the raw request bytes, not
   re-serialized JSON; use the `Bytes` extractor.
3. `owner_signed_registration_skips_address_limit` sees 429 on the third call → the limit runs before the owner check;
   move it inside `if !owner_signed`.

---

### R31-5b — Owner flags and variables

Size: Haiku. Deps: R31-5, R31-2.

**Goal**
`--owner-key`, `--registration` and their `PM_RELAY_` variables set the owner settings from R31-5.

**Read first**
- `relay/server/src/main.rs` — the flag loop and `--help` text (NF-1).
- `relay/server/src/env_config.rs` — `FLAG_VARS` (from R31-2).
- `relay/server/src/owner.rs` — `Registration::parse` (from R31-5).

**Files**
- edit `relay/server/src/main.rs`
- edit `relay/server/src/env_config.rs`
- edit `relay/server/tests/env.rs`
- edit `relay/README.md`

**Steps**
1. Add to `FLAG_VARS`: `("PM_RELAY_OWNER_KEY", "--owner-key", Kind::Text)` and
   `("PM_RELAY_REGISTRATION", "--registration", Kind::Text)`.
2. In the flag loop add the two flags (Code). Add their `--help` lines.
3. After the loop, apply the default rule (Code).
4. Add the README rows. Write tests, run, commit `relay: owner flags and variables`.

**Code**
Flag loop arms (shape of the existing arms, NF-1):
```rust
"--owner-key" => {
    let v = next_value(&mut it, "--owner-key");
    if !pm_relay_protocol::is_key_id(&v) {
        eprintln!("pm-relay: --owner-key is not a key id");
        std::process::exit(2);
    }
    config.owner_key = Some(v);
}
"--registration" => {
    let v = next_value(&mut it, "--registration");
    match pm_relay::owner::Registration::parse(&v) {
        Some(r) => registration = Some(r),
        None => {
            eprintln!("pm-relay: --registration must be open or owner");
            std::process::exit(2);
        }
    }
}
```
(`next_value` stands for however the loop reads a flag's value today.)

After the loop:
```rust
config.registration = match (registration, config.owner_key.is_some()) {
    (Some(pm_relay::owner::Registration::Owner), false) => {
        eprintln!("pm-relay: --registration owner needs --owner-key");
        std::process::exit(2);
    }
    (Some(r), _) => r,
    (None, true) => pm_relay::owner::Registration::Owner,
    (None, false) => pm_relay::owner::Registration::Open,
};
```
`--help` lines:
```
  --owner-key <key id>        The relay owner's public key id. Turns on admin routes.
  --registration open|owner   Who may add teams. Default: owner when --owner-key is set, else open.
```

**Tests to write**
In `tests/env.rs` (key id from `pm_relay_protocol::key_id(&SigningKey::from_bytes(&[7u8;32]))`):
- `owner_key_from_env_shows_in_version`: env `PM_RELAY_OWNER_KEY=<id>`, `PM_RELAY_MEMORY=1`, `PM_RELAY_LISTEN=127.0.0.1:<port>` → `GET /v1/version` body contains `"registration":"owner"` and the id.
- `registration_open_with_key`: also `PM_RELAY_REGISTRATION=open` → body contains `"registration":"open"`.
- `registration_owner_without_key_exits_2`: env `PM_RELAY_REGISTRATION=owner` → exit 2, stderr `pm-relay: --registration owner needs --owner-key`.
- `bad_owner_key_exits_2`: env `PM_RELAY_OWNER_KEY=nope` → exit 2, stderr `pm-relay: --owner-key is not a key id`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-5b'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test env`
→ `test result: ok. 7 passed; 0 failed` (3 from R31-2 plus 4).

**Done when**
- [ ] 4 new tests pass.
- [ ] `--help` shows both flags.

**Do NOT**
- Do not print the owner key id in the start line.
- Do not accept `--registration owner` without a key.

**If it fails**
1. `is_key_id` rejects the test id → the test built the id from the wrong helper; use `key_id` from the same crate.
2. Exit code is 1 → the error path uses `?` from `main`; call `std::process::exit(2)` directly.
3. Version shows `open` with a key → the default rule runs before the flag loop; move it after.

---

### R31-6 — Storage limit and data-folder check

Size: Haiku. Deps: R31-5b.

**Goal**
The whole database never grows past `--max-db-bytes`, and an unwritable data folder stops the relay with exit 3.

**Read first**
- `relay/server/src/lib.rs` — the handlers for `POST /v1/teams`, `PUT …/state/{slot}`, `POST …/envelopes`,
  `POST …/joins` (relay facts §2) and where each writes.
- `relay/server/src/lib.rs` — `Relay::counts` (R31-5).
- `relay/server/src/main.rs` — where `Relay::open` is called (NF-1).

**Files**
- edit `relay/server/src/lib.rs`
- edit `relay/server/src/main.rs`
- edit `relay/server/src/env_config.rs`
- edit `relay/server/src/owner.rs`
- create `relay/server/tests/quota.rs`

**Steps**
1. Add `pub max_db_bytes: u64` to `Config` with default `1_073_741_824`.
2. Add `Relay::over_quota(&self, max: u64) -> bool` (Code).
3. In each of the four write handlers, after auth and before the write, add the check in Code.
4. Add flag `--max-db-bytes <n>` and `FLAG_VARS` entry `("PM_RELAY_MAX_DB_BYTES", "--max-db-bytes", Kind::Number)`.
5. In `main.rs`, before `Relay::open`, add `check_data_folder` (Code).
6. In `owner.rs`, add `max_db_bytes: u64` to `AdminStatus`, filled from `config.max_db_bytes`.
7. Tests, run, commit `relay: storage limit and data folder check`.

**Code**
```rust
// lib.rs
pub const RELAY_FULL: &str = "the relay is full";

impl Relay {
    /// True when pages in use × page size is at or over `max`. Errors count as full (safe side).
    pub fn over_quota(&self, max: u64) -> bool {
        match self.counts() { Ok(c) => c.db_bytes >= max, Err(()) => true }
    }
}
```
Handler check (each of the four writes):
```rust
if state.relay().over_quota(state.config().max_db_bytes) {
    return (StatusCode::INSUFFICIENT_STORAGE, RELAY_FULL).into_response();
}
```
`main.rs`:
```rust
fn check_data_folder(db: &std::path::Path) {
    let dir = db.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let probe = dir.join(".pm-relay-write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok() && std::fs::remove_file(&probe).is_ok();
    if !ok {
        eprintln!(
            "pm-relay: the data folder {} can't be written. In a container, give the volume to user 10001 or run as root.",
            dir.display()
        );
        std::process::exit(3);
    }
}
```
Call it only when `config.db` is `Some(path)`.

**Tests to write**
In `tests/quota.rs`:
- `full_relay_refuses_new_team`: `Config { max_db_bytes: 1, .. }` → `POST /v1/teams` → 507 `the relay is full`.
- `relay_under_limit_takes_team`: default config → 201.
- `full_relay_refuses_owner_signed_registration_too`: `max_db_bytes: 1` and an owner key; owner-signed registration → 507 `the relay is full` (the owner gets no way around the disk limit).
- `admin_status_shows_limit`: owner relay → status JSON has `maxDbBytes == 1073741824`.
- `unwritable_data_folder_exits_3`: create a temp file `blocker`; run the binary with `--db <tmp>/blocker/relay.sqlite` → exit code 3, stderr contains `can't be written`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-6'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test quota`
→ `test result: ok. 5 passed; 0 failed`. Then the whole workspace: all `ok.`.

**Done when**
- [ ] 5 tests pass; old tests pass.
- [ ] Rooms (in memory) are not limited by this check.

**Do NOT**
- Do not run `VACUUM` automatically to make room.
- Do not delete data to make room.
- Do not print the database path anywhere except the exit-3 message.

**If it fails**
1. Every write is 507 in old tests → the default `max_db_bytes` is 0; set the `Default` value.
2. Exit code 3 test gets 0 on Windows → the probe path under a file still "works"; assert the file `blocker` is a file,
   not a folder, before running.
3. `counts()` is slow under load → acceptable for v1 (three header reads); do not add caching in this slice.

---

### R31-7 — WebSocket probe `/v1/probe/ws`

Size: Haiku. Deps: R31-3.

**Goal**
A no-auth WebSocket route proves that the provider's proxy passes WebSocket upgrades.

**Read first**
- `relay/server/src/lib.rs` lines 782-873 — how team sockets upgrade and send text frames (relay facts §2).
- `relay/server/src/lib.rs` — the rate-window helper used for `viewer_requests_per_minute` (NF-27).
- `relay/server/tests/protocol.rs` — the WebSocket client used in tests (NF-4).

**Files**
- edit `relay/server/src/ops.rs`
- edit `relay/server/src/lib.rs` (route, `Limits.probe_per_address_per_minute`)
- edit `relay/server/src/env_config.rs` (`LIMIT_VARS` entry and `apply_limits` arm)
- edit `relay/server/tests/ops.rs`
- edit `relay/README.md`

**Steps**
1. Add `probe_per_address_per_minute: 60` to `Limits` and its `Default`.
2. Add `("PM_RELAY_LIMIT_PROBE_PER_ADDRESS_PER_MINUTE", "probe_per_address_per_minute")` to `LIMIT_VARS` and the arm
   to `apply_limits`.
3. Add `probe_ws` to `ops.rs` (Code) and the route `.route("/v1/probe/ws", get(ops::probe_ws))`.
4. Add `"probe"` to `features` in `version_reply`.
5. Tests, run, commit `relay: WebSocket probe`.

**Code**
```rust
pub const PROBE_FRAME: &str = r#"{"probe":"pm-relay/1"}"#;

pub async fn probe_ws(
    State(state): State<crate::SharedState>,
    connect: axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    ws: Option<axum::extract::ws::WebSocketUpgrade>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // Same address rule as every other per-address limit (relay facts §2 `client_addr`, `address_key`).
    let addr = crate::client_addr(connect.0, &headers, state.trusted());
    if !state.allow_window("probe", &crate::address_key(addr), state.config().limits.probe_per_address_per_minute, 60_000) {
        return (axum::http::StatusCode::TOO_MANY_REQUESTS, "slow down").into_response();
    }
    let Some(ws) = ws else {
        return (axum::http::StatusCode::UPGRADE_REQUIRED, "use a WebSocket").into_response();
    };
    ws.on_upgrade(|mut socket| async move {
        use axum::extract::ws::Message;
        let _ = socket.send(Message::Text(PROBE_FRAME.into())).await;
        let _ = socket.send(Message::Close(None)).await;
    })
}
```
`state.allow_window(...)` stands for the existing window helper (NF-27); call it the way the viewer limit calls it.
On axum 0.8, `Option<WebSocketUpgrade>` needs the optional extractor form; see NF-4.

**Tests to write**
In `tests/ops.rs`:
- `probe_sends_one_frame_then_closes`: connect `ws://127.0.0.1:<port>/v1/probe/ws` → first message text exactly `{"probe":"pm-relay/1"}`, then a close.
- `probe_without_upgrade_is_426`: plain `GET /v1/probe/ws` → 426 `use a WebSocket`.
- `probe_is_limited_per_address`: `probe_per_address_per_minute = 2` → third plain GET within a minute → 429 `slow down`.
- `version_lists_probe`: `features` contains `"probe"`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-7'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test ops`
→ `test result: ok. 8 passed; 0 failed`.

**Done when**
- [ ] 4 new tests pass.
- [ ] The probe never reads the database.

**Do NOT**
- Do not keep the socket open or wait for client frames.
- Do not echo anything the client sends.

**If it fails**
1. `ConnectInfo` missing in tests → the test server is not started with `into_make_service_with_connect_info`; use the
   same start helper as `tests/protocol.rs`.
2. The client sees close before text → send the text frame first and do not split the two sends into separate tasks.
3. 429 on the first call → the window helper's limit argument is per hour in that helper; pass the minute window
   explicitly as the helper expects.

---

### R31-8 — Dockerfile reads env settings; version bump

Size: Haiku. Deps: R31-1, R31-2, R31-3, R31-5, R31-5b, R31-6, R31-7.

**Goal**
The image starts with `PM_RELAY_*` defaults instead of fixed flags, records the git commit, and the relay version goes
up one minor step.

**Read first**
- `relay/Dockerfile` (relay facts §4).
- `relay/server/Cargo.toml` — the `version` line (NF-4).
- `relay/README.md` — the Docker run line (relay facts §1a).

**Files**
- edit `relay/Dockerfile`
- edit `relay/server/Cargo.toml`
- edit `relay/README.md`
- edit `relay/HOSTED-RELAY.md`

**Steps**
1. In the build stage, add `ARG PM_RELAY_COMMIT=unknown` after `WORKDIR /src`.
2. Replace the build `RUN` line as in Code.
3. In the runtime stage, add the `ENV` block after `USER pmrelay`.
4. Replace the `ENTRYPOINT` line as in Code. Keep every other line exactly.
5. Add the `LABEL` line before `ENTRYPOINT`.
6. In `relay/server/Cargo.toml`, raise the minor version by one and set patch to 0 (for example `0.1.3` → `0.2.0`).
   Update `relay/Cargo.lock` by running the build in Run (cargo rewrites the lock entry for `pm-relay` only).
7. In `relay/README.md`, under the Docker row, add:
   "Settings are environment variables (see Settings from environment variables). Flags after the image name still
   work and win over variables."
8. In `relay/HOSTED-RELAY.md` section 10 add the line `Image reads PM_RELAY_* settings since <version>.`
9. Run, commit `relay: image takes settings from environment, version <new>`.

**Code**
Before:
```dockerfile
RUN cargo build --release --locked -p pm-relay
```
After:
```dockerfile
RUN PM_RELAY_COMMIT="$PM_RELAY_COMMIT" cargo build --release --locked -p pm-relay
```
Add after `USER pmrelay`:
```dockerfile
ENV PM_RELAY_LISTEN=0.0.0.0:8787 \
    PM_RELAY_DB=/data/relay.sqlite
```
Before:
```dockerfile
ENTRYPOINT ["tini", "--", "pm-relay", "--listen", "0.0.0.0:8787", "--db", "/data/relay.sqlite"]
```
After:
```dockerfile
LABEL org.opencontainers.image.source="https://github.com/Potato-dev-inc/puppet-master"
ENTRYPOINT ["tini", "--", "pm-relay"]
```

**Tests to write**
No Rust test. The proof is the Run output.

**Run**
From the repo root, PowerShell:
1. `docker build --build-arg PM_RELAY_COMMIT=$(git rev-parse HEAD) -t pm-relay:smoke2 relay` → no `ERROR`.
2. `$id = docker run -d -p 127.0.0.1:18787:8787 -v pm-relay-smoke2:/data pm-relay:smoke2; $id`
3. `curl.exe -s --retry 15 --retry-connrefused --retry-delay 1 http://127.0.0.1:18787/v1/health` → `{"ok":true,"protocol":"pm-relay/1"}`.
4. `curl.exe -s http://127.0.0.1:18787/v1/version` → JSON with the new `version`, `commit` equal to `git rev-parse HEAD`, `registration` `open`, `owner` `null`.
5. `docker exec $id ls -l /data` → `relay.sqlite`.
6. `docker stop $id; docker rm $id`
7. `docker run --rm -e PM_RELAY_MAX_OPEN_ROOMS=abc pm-relay:smoke2; $LASTEXITCODE` → prints `pm-relay: PM_RELAY_MAX_OPEN_ROOMS must be a number` and `2`.
8. `$id2 = docker run -d -p 127.0.0.1:18788:8787 pm-relay:smoke2 --memory; curl.exe -s --retry 15 --retry-connrefused --retry-delay 1 http://127.0.0.1:18788/v1/ready; docker stop $id2; docker rm $id2`
   → `{"ok":true,"db":"ok"}`.
9. `docker volume rm pm-relay-smoke2`
10. `$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-8'; cargo test --manifest-path relay/Cargo.toml` → all `ok.`.

**Done when**
- [ ] Steps 3, 4, 7, 8 printed what is written above.
- [ ] Version bumped; lock updated for `pm-relay` only (`git diff relay/Cargo.lock` shows one package version line).

**Do NOT**
- Do not remove `USER pmrelay`, `VOLUME /data` or the `HEALTHCHECK`.
- Do not pin base images by digest in this slice (separate decision; R32-1 pins the published image instead).
- Do not push the image.

**If it fails**
1. `commit` shows `unknown` → the `RUN` line does not pass the ARG; check the ARG is in the build stage, not the runtime.
2. Step 7 exits 0 → `env_args` is not called in `main.rs`; R31-2 is not merged.
3. Health fails with the volume but works with `--memory` → volume permission; R31-6's exit 3 message shows in
   `docker logs`; recreate the volume (`docker volume rm`) because an old volume can hold root-owned files.

---

### R31-9 — iroh relay as a second Railway service (spike, blocked)

Size: Sonnet. Deps: Spaces D-0 merged; NF-21 answered. **Do not start before both are true.**

**Goal**
Prove that an iroh relay pinned to the app's iroh version runs as a second Railway service behind HTTPS and only
serves node ids the owner allows.

**Read first**
- `packages/app/src-tauri/src/p2p/iroh_net.rs` — how the app binds iroh and whether it can take a custom relay URL
  (`RelayMode`, `BindSpec.relays`, spaces facts §1).
- `packages/app/src-tauri/src/p2p/guard_tests.rs` — `only_one_file_touches_the_network_and_no_test_binds_it` and
  `p2p_code_writes_no_file_and_prints_nothing` (spaces facts §2): a custom relay URL must arrive as data, not as an
  env read in `p2p/`.
- The iroh relay server documentation for the version matching `iroh = "=1.4.1"` (NF-21).

**Files**
- create `relay/iroh/Dockerfile`
- create `relay/iroh/iroh-relay.toml`
- create `relay/iroh/README.md`
- edit `relay/server/src/ops.rs` (version reply field `iroh: Option<String>` from `--iroh-url` / `PM_RELAY_IROH_URL`)
- edit `relay/server/src/env_config.rs`

**Steps**
1. Answer NF-21: crate or image name, version matching iroh 1.4.1, config keys for no-TLS behind a proxy, access
   control by node id (allowlist or HTTP check).
2. Write `relay/iroh/Dockerfile` that installs that exact version (no `latest`).
3. Write `iroh-relay.toml` with: HTTP only on port 3340 (Railway ends TLS), QUIC address discovery off, access set to
   an allowlist file `/data/allow.txt` or the HTTP check, whichever NF-21 says the pinned version supports.
4. Add `--iroh-url` / `PM_RELAY_IROH_URL` to `pm-relay` so `/v1/version` returns `"iroh":"https://…"`.
5. Deploy by hand to a test Railway project (founder's account, founder present), with two test profiles from
   Spaces D-0 using it; record hole-punch and relayed results.
6. Write the findings into `relay/iroh/README.md`. Commit `relay: iroh relay spike`.

**Code**
`version_reply` addition:
```rust
pub iroh: Option<String>,   // from Config.iroh_url; None when not set
```
`FLAG_VARS` addition: `("PM_RELAY_IROH_URL", "--iroh-url", Kind::Text)`.
The iroh config file content depends on NF-21 and is written in step 3 from that answer.

**Tests to write**
- `version_has_no_iroh_by_default` → `iroh` is `null`.
- `version_shows_iroh_url` → `--iroh-url https://iroh.example` → `"iroh":"https://iroh.example"`.
- Manual: two test profiles connect through the iroh relay; a third node id not on the allowlist is refused.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r31-9'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test ops`
→ `ok.`, `0 failed`. Manual results recorded in `relay/iroh/README.md`.

**Done when**
- [ ] NF-21 answered in the README.
- [ ] A refused node id is shown in the spike notes.

**Do NOT**
- Do not add the iroh relay to the app's one-click flow in this slice.
- Do not add any network or env code to `packages/app/src-tauri/src/p2p/` other than `iroh_net.rs`.
- Do not leave the test Railway project running after the spike; delete it with the founder.

**If it fails**
1. The pinned version has no access control → stop; write "iroh relay stays public-relay only" in the README.
2. Clients cannot connect through Railway's proxy → record the error; the fallback is a VPS with UDP, which is a new
   founder decision.
3. Version mismatch between app iroh and relay → pin the relay to the exact app version; never upgrade app iroh in this
   slice.

---

## Open facts

- NEEDS-FACT NF-1: the exact flag-parsing loop in `relay/server/src/main.rs` (how args are read, how a flag's value is
  taken, how exit code 2 is produced) and every field of `Config` in `relay/server/src/lib.rs` with its default
  (including the name of the limits field). Read `relay/server/src/main.rs` and `Config` in `relay/server/src/lib.rs`.
- NEEDS-FACT NF-2: the signatures of `router` and `serve` and the shared state type, and whether the `POST /v1/teams`
  handler takes `Json<TeamRegistration>` or raw bytes. Read `relay/server/src/lib.rs` lines 600-690 and 924-960.
- NEEDS-FACT NF-3: exact parameter lists and return types of `auth_header`, `verify_request`, `key_id` and `is_key_id`.
  Read `relay/protocol/src/lib.rs`.
- NEEDS-FACT NF-4: `relay/server/Cargo.toml`: the package `version`, the axum major version (0.6, 0.7 or 0.8 changes the
  middleware and optional-extractor code), and which WebSocket and HTTP client crates the tests use. Read
  `relay/server/Cargo.toml` and the top of `relay/server/tests/protocol.rs`.
- NEEDS-FACT NF-5: how `Relay` holds its SQLite connection (field name, `Mutex<Connection>` or other). Read
  `relay/server/src/lib.rs` around line 326.
- NEEDS-FACT NF-21: the iroh relay server that matches `iroh = "=1.4.1"`: crate name and version or official image,
  config keys for plain HTTP behind a TLS-ending proxy, and access control by node id. Read the iroh relay docs for that
  version and `packages/app/src-tauri/src/p2p/iroh_net.rs`.
- NEEDS-FACT NF-25: every logging call already present in `relay/server/src`. Run
  `rg -n "tracing::|info!\(|warn!\(|error!\(|debug!\(|println!\(|eprintln!\(" relay/server/src`.
- NEEDS-FACT NF-26: where the app's Live Room client sends `POST /v1/rooms` (needed only if D-5 changes to owner-only
  rooms). Search `packages/app/src-tauri/src` for `/v1/rooms`.
- NEEDS-FACT NF-27: the shape of `SeenSigs` (its insert method), the rate-window helper used for per-address limits,
  and what `spawn_local` returns. Read `relay/server/src/lib.rs` lines 160-260 and the end of the file.
- NEEDS-FACT NF-34: the name of the test in `packages/app/src-tauri/src/team/tests_v1.rs` near line 497 that reads
  `PM_RELAY_TEST_URL`.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Stranger registers teams on a personal relay | Uses the owner's disk and bill | Owner-only registration | `owner_relay_refuses_unsigned_team_registration`, `other_key_is_refused` |
| Stranger uses up the registration allowance behind a proxy | Owner cannot create a team | Owner-signed registration skips the per-address limit | `owner_signed_registration_skips_address_limit` |
| Replay of a captured owner request | Repeat an admin call or registration | Replay memory, 5-minute skew | `owner_signature_replay_is_refused` |
| Admin route discovery | Learn that a relay has an owner | 404 `not found` without an owner key; version shows only a public key id | `admin_status_is_404_without_owner_key` |
| Fill the disk | Relay crashes or the volume bill grows | `--max-db-bytes`, 507 | `full_relay_refuses_new_team` |
| Logs leak identifiers | Provider staff or log viewers link teams and addresses | Route patterns only; guard test | `log_shows_pattern_not_values`, `only_main_and_access_log` |
| Probe used for a flood | CPU and bandwidth | 60 per address per minute, no database, closes at once | `probe_is_limited_per_address` |
| Typo in a variable silently weakens a limit | A limit stays at a wrong value | Unknown `PM_RELAY_` names stop the relay | `unknown_name_is_an_error`, `binary_unknown_env_exits_2` |
| Volume not writable | Relay loops on restart, "502" for users | Exit 3 with a plain line; Railway shows the crash | `unwritable_data_folder_exits_3` |
| Owner key stolen from the computer | Admin calls: status, and later backup (ciphertext only) | Key only in the credential store; rotation (R35-7) | file 35 |

## What cannot be verified without real hardware or a second computer

- The image on Railway's real proxy and volume (file 32).
- Docker health checks on Linux hosts other than Docker Desktop on this Windows PC.
- That `0.0.0.0:8787` inside the container is reachable only through Railway's proxy and not on another public port.
- The iroh relay spike (R31-9) needs two computers behind different networks.
