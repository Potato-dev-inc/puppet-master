# 35 — One-click relay: security

Status: plan only. Part of the one-click relay family (overview: `30-one-click-relay-overview.md`).

Sources the writer used: `00-FORMAT.md`, `_facts/relay-facts.md` ("relay facts §N"), `_facts/spaces-facts.md`
("spaces facts §N", including the cowork security review items M1-M6 in §8). No source code was opened by the writer.
NEEDS-FACT lines use the shared `NF-n` numbering and are listed in `Open facts` near the end.

---

## 1. What we protect

| Asset | Where it lives | Why it matters |
|---|---|---|
| Railway account token, Railway project token, Cloudflare token | Credential store only: KeyStore names `relay_railway_token`, `relay_railway_project_token`, `relay_cloudflare_token` (service `com.puppetmaster.app`, name `team/<profile>/<name>`, file 32 R32-2) | Spends the person's money; can delete or replace the relay |
| Relay owner key (Ed25519 private key) | Credential store only: KeyStore name `relay_owner` (and `relay_owner_next` during a rotation) | Admin calls on the relay: status, backup, restore, owner-only registration |
| Team content (Heads-up cards, later Spaces messages) | Sealed before it leaves the computer (spaces facts §3 `crypto.rs`, `secure.rs`) | Private work |
| Team metadata (who, when, how much) | On the relay's disk and in the provider's logs | Shows who works with whom and when |
| Relay uptime | Railway or Cloudflare | Teammates stop getting updates |
| The person's bill | Railway | Abuse costs real money |

## 2. Who might attack, and what they can and cannot do

| Actor | Can | Cannot (and why) | Tests |
|---|---|---|---|
| A stranger who finds the relay address (scanners find new `*.up.railway.app` names) | Call `/v1/health`, `/v1/version`, `/v1/ready`, `/v1/probe/ws`; post join requests to a team id they guess; open the Live Room viewer for a room id they know | Register teams (owner-only registration); call admin routes (owner signature, 404 without an owner key); read any team route (device signature and access list, relay facts §2) | `owner_relay_refuses_unsigned_team_registration`, `admin_status_needs_owner`, `probe_is_limited_per_address` |
| Someone holding an invite link | Send up to 5 join requests for that invite, within 10 minutes (`INVITE_TTL_MS`, `MAX_ATTEMPTS`, spaces facts §3; `joins_per_invite`, relay facts §2) | Get in without the admin pressing approve; read anything before approval; create a team on the relay | `joins_need_no_owner_signature_on_owner_relay` (joins still work), existing join tests in `relay/server/tests/protocol.rs` |
| A removed former member | Keep what they already received | Read new messages (removal rotates the key, spaces facts §3 `membership::remove` and `rotate`); post (device dropped from the access list → 403) | existing team tests (`tests_hardening.rs`) |
| A malicious team admin | Move the team to a relay they run (`relay_moved`) | More than they can already do: admins already control the access list and keys | `relay_moved_from_non_admin_is_ignored` (non-admins cannot) |
| The relay operator: the person, Railway or Cloudflare staff, or someone with the provider account | See metadata (section 4); drop, delay or reorder envelopes; serve old state | Read content (sealed); forge envelopes (device signatures); forge access lists or state (root signatures); replay old access lists or state (`409 "older …"`, version checks, relay facts §2) | existing protocol tests; `SeqTracker` gap detection (spaces facts §3) |
| Malware running as the person on their computer | Read the credential store like any app of that user | Nothing this plan can stop; the plan limits the damage: the account token is swapped for a project-only token (D-2), and the owner key can be rotated (R35-7) | `deploy_happy_path_makes_everything_in_order` (swap) |
| Agents, MCP tools, the phone, the bridge inside Puppet Master | Nothing on Your relay | Call any `relay_host_*` command or read relay tokens (not registered on those routes; names not UI secrets) | R35-6 guards |
| Supply chain (GHCR package, the GitHub org) | Replace the image under a tag | Change what a new relay runs: the app pins `RELAY_IMAGE` by digest | `relay_image_is_pinned_by_digest` |
| Network attacker between the app and the relay | Nothing beyond blocking | Read or change traffic: `https://` only for non-loopback (`normalize_relay_url`, relay facts §6); provider TLS | existing `normalize_relay_url` tests; `relay_moved_to_http_is_refused` |

## 3. Token leakage: every path and its block

| Path | Block | Test (slice) |
|---|---|---|
| A file (`relay_host.json`, `team.json`, settings) | `HostConfig` has no token field; only `KeyStore::set_relay_token` stores tokens | `host_config_has_no_secret_fields` (R32-2), `saved_config_never_has_the_token` (R32-5), `token_never_leaves_the_credential_store` (R35-5) |
| Logs | `relay_host` has no log calls at all | `relay_host_prints_nothing` (R35-6) |
| Command replies and `relay://changed` events | `RelayHostView` has no token field; `connect` returns the view | `connect_stores_token_and_returns_no_token` (R33-2), R35-5 |
| Error text (provider echoes the token) | `redact()` on every GraphQL message; ureq error text dropped | `graphql_error_message_is_redacted` (R32-3), R35-5 |
| `Debug` output, panics | Manual `Debug` for `RailwayAuth`, `CfAuth` | `debug_of_auth_hides_token`, `debug_of_cf_auth_hides_token` |
| Generic secret commands (`secret_get_masked`, `secret_set`) | Relay names are not in `PROVIDER_SECRETS` or `SIDECAR_SECRETS`, so `guard()` refuses them (relay facts §7) | `relay_tokens_are_not_ui_secrets` (R35-6) |
| Provider variables (visible in Railway's dashboard) | Only the owner **public** key id and fixed settings | `railway_vars_are_exactly_these` (R32-5) |
| URLs and query strings | Tokens go only in headers | R32-3 header tests |
| The screen and Live Room screen shares | Token field is `type="password"`; the token is never shown again | `field hides characters` (R33-4) |
| The clipboard after paste | Not cleared (DECISION-OPEN D-16, default: no clipboard clearing; clearing what the person copied is surprising) | none |
| The relay's own logs | The relay never sees provider tokens; it logs route patterns only | `log_shows_pattern_not_values` (R31-4) |

## 4. What the relay operator can see (metadata)

| Seen | By whom | Notes |
|---|---|---|
| Team ids, device key ids, number of devices per team | Anyone with the disk or a backup | Ids are pseudonymous hashes and public keys (relay facts §5 `team_id`) |
| Time and size bucket of each envelope | Same | Sizes are padded to 1, 4 or 16 KiB (`PAD_BUCKETS`, spaces facts §3) |
| Join request times per invite id | Same | Join content is HPKE-sealed (spaces facts §3 `seal_join`) |
| Client IP addresses | The provider's edge and the container's peer address | Our code never logs them (R31-4); Railway's and Cloudflare's own edge logs are outside our control |
| Live Room ids, frame sizes and times, viewer counts | Same | Frames are sealed (`ct`, `nonce`, relay facts §5) |
| Which provider account pays for the relay | Railway or Cloudflare | Links the relay to the person's billing identity |
| The owner key id | Anyone (`/v1/version`) | A public key; it lets the app spot a wrong relay without signing |

The one-click relay does not add new metadata beyond `/v1/version` and the owner-only `/v1/admin/status` counts.

## 5. End-to-end guarantees kept

Unchanged by this plan, because the one-click relay runs the same server and the same app transport:
1. Envelopes are sealed with the team epoch key (XChaCha20-Poly1305) and padded (spaces facts §3 `seal_with_key`).
2. Epoch keys reach devices sealed with HPKE (`make_grant`, `open_grant`).
3. The access list and state blobs are signed by the team root key (`SignedAcl`, `SignedBlob`, relay facts §5).
4. Every device request is signed and replay-checked (`x-pm-auth`, `AUTH_SKEW_MS`, relay facts §2).
5. Join boxes are sealed to the inviter (`JoinBox.enc`, `ct`).
6. Live Room frames are sealed; viewers prove a read key by hash (`read_hash`).

New code never touches team crypto: `relay_host` must not import `team::crypto` or `team::secure` (guard in R35-6).

## 6. Rate limits after this plan

| Limit | Value | Where | Slice |
|---|---|---|---|
| Team registrations per address per hour | 30 (skipped for owner-signed) | server | existing; R31-5 |
| Owner-only registration | on for one-click relays | server and Worker | R31-5, R32-7 |
| Failed signatures per address per minute (before any replay memory) | 30 | server | R35-2 (review M3) |
| WebSocket wakes per socket | coalesced into one query per 100 ms | server | R35-3 (review M4) |
| Client frames per WebSocket | burst 10, then 1 per second; over → close 1008 | server | R35-3 (review M4) |
| Join tries per invite | 5, counter forgotten after 1 day | server | R35-4 (review M5) |
| WebSocket probe per address | 60 per minute | server | R31-7 |
| Whole database | 1 GiB | server | R31-6 (review M5 "no storage quota") |
| Per-address limits behind Railway's proxy | Correct only after `PM_RELAY_TRUSTED_PROXY` is set (NF-8) | deploy | R35-4 |

Residual risk until NF-8 is answered: behind Railway's proxy, every client looks like the proxy address, so the 10 joins
per address per hour per team are shared by all invitees. A stranger with a team id can use them up for an hour. The
owner-only rule removes the same risk for registrations, not for joins.

## 7. Cowork review items this plan closes

From spaces facts §8: M1 (mailbox counter), M3 (auth before replay memory), M4 (WebSocket wake amplification), M5
(proxy addresses, invite counter, storage quota). The review's own order is "M3, M4, M5 before any hosted relay goes
public", and one-click relays are exactly that. M2 and M6 are app-side Merge-when-green issues and are not in this plan.

---

## Slices

### R35-1 — M1: the mailbox counter never goes back

Size: Sonnet. Deps: none. Skip this slice if `git log --oneline -- relay/server/src/lib.rs` already shows an M1 fix.

**Goal**
Envelope sequence numbers keep rising after the mailbox empties, and a client whose cursor is ahead of the relay is told
to reset.

**Read first**
- `relay/server/src/lib.rs` lines 351-361 (`SELECT COALESCE(MAX(seq), 0) + 1 FROM envelopes WHERE team = ?1`), 202-208
  (`sweep` deletes envelopes older than 7 days), 394-438 (reads for long poll and WebSocket) (spaces facts §8 M1).
- `relay/server/src/lib.rs` `Relay::open` migrations (relay facts §2; schema NF-22).
- `packages/app/src-tauri/src/team/transport/relay.rs` lines 190-194 and 224-226 (cursor only moves forward) and
  `team/service.rs` lines 170-172 (how `reset` is handled, NF-39).

**Files**
- edit `relay/server/src/lib.rs`
- create `relay/server/tests/mailbox_seq.rs`
- edit `packages/app/src-tauri/src/team/transport/relay.rs`
- edit `packages/app/src-tauri/src/team/service.rs` (only if NF-39 shows `reset` is ignored)

**Steps**
1. Migration in `Relay::open` (Code).
2. Replace the `MAX(seq)` statement with the `next_seq` read and update, inside the same transaction as the insert.
3. In the long-poll and WebSocket read paths: when `after > head` (head = `next_seq - 1`), reply with a reset (Code).
4. App: when a batch or WebSocket frame has `reset: true`, set the cursor to `head` (not `max(cursor, head)`). Write a
   pure helper `fn next_cursor(cursor: u64, batch_next: u64, reset: bool, head: Option<u64>) -> u64` and use it in both
   places.
5. Tests, run, commit `relay: mailbox counter keeps rising (M1)`.

**Code**
```rust
// Relay::open, after the tables exist
let has_next_seq = conn
    .prepare("SELECT 1 FROM pragma_table_info('teams') WHERE name = 'next_seq'")?
    .exists([])?;
if !has_next_seq {
    conn.execute_batch(
        "ALTER TABLE teams ADD COLUMN next_seq INTEGER NOT NULL DEFAULT 1;
         UPDATE teams SET next_seq =
           COALESCE((SELECT MAX(seq) FROM envelopes WHERE envelopes.team = teams.team), 0) + 1;",
    )?;
}
```
```rust
// insert path, inside the existing transaction `tx`
let seq: i64 = tx.query_row("SELECT next_seq FROM teams WHERE team = ?1", [team], |r| r.get(0))?;
tx.execute("UPDATE teams SET next_seq = next_seq + 1 WHERE team = ?1", [team])?;
```
```rust
// read paths
let head = next_seq - 1;
if after > head {
    return Batch { items: vec![], next: head, head: Some(head), reset: true };   // long poll
    // WebSocket: send Resync { reset: true, head } as the first frame
}
```
```rust
// app, relay.rs
pub(crate) fn next_cursor(cursor: u64, batch_next: u64, reset: bool, head: Option<u64>) -> u64 {
    if reset { return head.unwrap_or(batch_next); }
    cursor.max(batch_next)
}
```

**Tests to write**
`relay/server/tests/mailbox_seq.rs`:
- `seq_keeps_rising_after_mailbox_empties` → post 3 envelopes (seq 1, 2, 3); run the sweep with a time 8 days later (NF-27: the sweep entry point that takes a time, or a new `pub fn sweep_at(&self, now_ms: u64)`); post one more → `{"seq":4}`.
- `cursor_ahead_of_head_gets_reset` → head 3; `GET …/envelopes?after=100&wait=0` → `reset: true`, `head: 3`, no items.
- `caught_up_cursor_is_not_reset` → `after=3` → `reset` absent or false.
- `old_database_gets_next_seq` → build a database file with the old `teams` table (no `next_seq`, statement from NF-22) and two envelopes for one team (seq 7, 8); open it with `Relay::open`; post → `{"seq":9}`.
App unit tests in `relay.rs` (`mod cursor_tests`):
- `reset_moves_cursor_back` → `next_cursor(100, 0, true, Some(3))` == 3.
- `normal_batch_moves_forward` → `next_cursor(5, 9, false, None)` == 9; `next_cursor(9, 5, false, None)` == 9.

**Run**
- `$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-1'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test mailbox_seq` → `test result: ok. 4 passed`.
- `cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib cursor_tests` (same target dir variable) → `test result: ok. 2 passed`.
- Whole relay workspace and `--lib team` → `ok.`.

**Done when**
- [ ] 6 tests pass; old relay and team tests pass.

**Do NOT**
- Do not renumber stored envelopes.
- Do not change the wire types in `relay/protocol` (`Batch` and `Resync` already have `reset` and `head`).

**If it fails**
1. `ALTER TABLE … DEFAULT 1` then `UPDATE` runs on every start → guard it with the `pragma_table_info` check.
2. App never resets → the reset frame arrives on the WebSocket as `Resync`; apply `next_cursor` there too.
3. Sweep cannot be driven in tests → add `pub fn sweep_at(&self, now_ms: u64)` and have the 600 s sweeper call it with
   the real time.

---

### R35-2 — M3: slow down failed signatures before any replay memory

Size: Sonnet. Deps: R31-2.

**Goal**
Bad or unknown signatures cannot fill the replay memory or burn CPU without limit.

**Read first**
- `relay/server/src/lib.rs` lines 172-182 (replay insert before the device check) and 466-490 (`authorize`)
  (spaces facts §8 M3; relay facts §2).
- `SeenSigs` and the rate-window helper (NF-27).
- `relay/server/src/env_config.rs` (R31-2).

**Files**
- edit `relay/server/src/lib.rs`
- edit `relay/server/src/env_config.rs`
- create `relay/server/tests/auth_limits.rs`
- edit `relay/README.md`

**Steps**
1. Add `auth_failures_per_address_per_minute: 30` to `Limits` and its `Default`; add
   `("PM_RELAY_LIMIT_AUTH_FAILURES_PER_ADDRESS_PER_MINUTE", "auth_failures_per_address_per_minute")` to `LIMIT_VARS` and
   the `apply_limits` arm.
2. Reorder `authorize` exactly as in Code.
3. Add `pub fn seen_count(&self) -> usize` to the replay memory and expose it on the store for tests.
4. Tests, README row, run, commit `relay: limit failed signatures before replay memory (M3)`.

**Code**
Order inside `authorize`:
```text
1. read x-pm-auth                         → missing: 401 "signed request required"
2. address = client_addr(...)             → if failures(address) in the current minute ≥ limit: 429 "slow down"
3. verify_request(...)                    → fail: count a failure for address; 401 with the reason
4. team known? device on access list?     → fail: count a failure for address; 404 "unknown team" / 403 "device not on this team"
5. replay memory insert                   → seen before: 401 "request replayed"; table full: 503 "the relay is busy, try again shortly"
6. per-device request window              → 429 "slow down"
```
Steps 2 and the failure counts use the same window helper as the other per-address limits (NF-27), keyed
`("auth-fail", address_key(address))`, 60 000 ms window.

**Tests to write**
`tests/auth_limits.rs`:
- `bad_signatures_from_one_address_are_slowed` → limit 3; 3 requests with a garbage `x-pm-auth` → 401 each; the 4th → 429 `slow down`.
- `failed_auth_does_not_fill_replay_memory` → 50 requests signed by a key that is not on the team → `seen_count()` stays 0.
- `good_device_still_passes_from_another_address` → trusted proxy `127.0.0.1`; attacker requests carry `X-Forwarded-For: 203.0.113.9` until 429; a real device with `X-Forwarded-For: 198.51.100.7` → 200.
- `replay_still_refused` → the same valid request twice → second 401 `request replayed`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-2'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test auth_limits`
→ `test result: ok. 4 passed`. Whole workspace → `ok.` (the 20 tests in `tests/protocol.rs` must still pass).

**Done when**
- [ ] 4 tests pass; old tests pass; README lists the new variable.

**Do NOT**
- Do not count successful requests as failures.
- Do not change any status code or message of `authorize` (relay facts §2 lists them).

**If it fails**
1. A protocol test sends a deliberately bad signature many times and now gets 429 → raise that test's limit through its
   config, do not raise the default.
2. `seen_count` needs a lock → take the same lock the insert takes.
3. The window helper counts per hour → pass a 60 000 ms window explicitly.

---

### R35-3 — M4: WebSocket wake coalescing and client frame limit

Size: Sonnet. Deps: none.

**Goal**
A burst of posts wakes each team socket into at most one database query per 100 ms, and a socket that sends too many
frames is closed.

**Read first**
- `relay/server/src/lib.rs` lines 394-438 (especially 419-425: every post wakes every socket, which queries SQLite) and
  782-873 (`stream_team`, ping every 30 s, `MAX_CLIENT_FRAME = 4 * 1024`) (spaces facts §8 M4; relay facts §2).

**Files**
- edit `relay/server/src/lib.rs`
- create `relay/server/tests/ws_wake.rs`

**Steps**
1. Add `pub const WAKE_COALESCE_MS: u64 = 100;` and an `AtomicU64` counter `ws_queries` on the store with
   `pub fn ws_queries(&self) -> u64`.
2. In each socket's loop, after a wake: sleep `WAKE_COALESCE_MS`, drain any further pending wake, then run one backlog
   query (count it).
3. Add a per-socket token bucket for incoming client frames: capacity 10, refill 1 per second. Over → send
   `Close(1008, "too many messages")` and end the loop.
4. Tests, run, commit `relay: coalesce socket wakes, limit client frames (M4)`.

**Code**
```rust
pub const WAKE_COALESCE_MS: u64 = 100;
pub const CLIENT_FRAME_BURST: f64 = 10.0;
pub const CLIENT_FRAMES_PER_SECOND: f64 = 1.0;

struct Bucket { tokens: f64, last: std::time::Instant }
impl Bucket {
    fn new() -> Self { Self { tokens: CLIENT_FRAME_BURST, last: std::time::Instant::now() } }
    fn take(&mut self) -> bool {
        let now = std::time::Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * CLIENT_FRAMES_PER_SECOND).min(CLIENT_FRAME_BURST);
        self.last = now;
        if self.tokens >= 1.0 { self.tokens -= 1.0; true } else { false }
    }
}
```
Wake handling (shape; the notifier type is whatever line 419-425 uses):
```rust
notified.await;
tokio::time::sleep(std::time::Duration::from_millis(WAKE_COALESCE_MS)).await;
// drain: if the notifier is a tokio::sync::Notify, a second notified() is not pending; if it is a watch or broadcast,
// read until empty without awaiting.
relay.ws_queries.fetch_add(1, Ordering::Relaxed);
let batch = relay.backlog(team, cursor, BATCH)?;
```

**Tests to write**
`tests/ws_wake.rs`:
- `many_posts_wake_each_socket_few_times` → 4 sockets open on one team; post 100 envelopes as fast as the per-device and per-team limits allow (raise them in the test config); wait 1 s; every socket received all 100 items; `ws_queries()` ≤ 4 × 40.
- `chatty_client_socket_is_closed` → send 30 ping text frames at once → the socket gets a close with code 1008.
- `quiet_client_socket_stays_open` → send 1 frame per second for 5 s → still open.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-3'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test ws_wake`
→ `test result: ok. 3 passed`. Whole workspace → `ok.`.

**Done when**
- [ ] 3 tests pass; `tests/protocol.rs` WebSocket tests pass.

**Do NOT**
- Do not drop or reorder delivered items.
- Do not change the 30 s ping.
- Do not close sockets that only answer pings (pongs are not client text frames).

**If it fails**
1. Items arrive late in old tests → 100 ms is added latency; tests that wait less than 200 ms need a longer wait.
2. `ws_queries` too high → the drain step awaits instead of draining; drain without awaiting.
3. Close code not seen by the client → send the close frame before dropping the socket and flush it.

---

### R35-4 — M5: proxy addresses on Railway and invite counters that expire

Size: Sonnet. Deps: R31-2, R32-5.

**Goal**
Per-address limits see real client addresses on Railway, and per-invite join counters do not grow forever.

**Read first**
- `relay/server/src/lib.rs` lines 253 and 442 (`ConnectInfo` used for per-address limits), 344-350 (per-device and
  per-team limits), the per-invite counter (spaces facts §8 M5).
- `relay/server/src/lib.rs` `client_addr(peer, headers, trusted)` (relay facts §2).
- `packages/app/src-tauri/src/relay_host/deploy.rs` `railway_vars` (R32-5).
- NF-8 answer (Railway proxy range).

**Files**
- edit `relay/server/src/lib.rs`
- create `relay/server/tests/join_limits.rs`
- edit `packages/app/src-tauri/src/relay_host/deploy.rs`
- edit `packages/app/src-tauri/src/relay_host/deploy_tests.rs`
- edit `relay/README.md`

**Steps**
1. Check that every per-address limit uses `client_addr(peer, headers, trusted)` and not the raw `ConnectInfo` address
   (lines 253 and 442). Change any that do not.
2. Per-invite counters: store `(count, first_ms)`; when `now - first_ms > JOIN_TTL_MS` (1 day, relay facts §2) start a
   new count; the 600 s sweeper removes entries older than `JOIN_TTL_MS` through a new
   `pub fn expire_join_counters(&self, now_ms: u64) -> usize` (returns how many it removed).
3. App: when NF-8 gives Railway's proxy range, add `PM_RELAY_TRUSTED_PROXY` with that exact value to `railway_vars` and
   update `railway_vars_are_exactly_these`. Until NF-8 is answered, leave `railway_vars` unchanged and write the residual
   risk line (section 6) into `relay/README.md` under "Limits".
4. README: say that limits are kept in memory and start again after a restart.
5. Tests, run, commit `relay: proxy-aware limits and expiring invite counters (M5)`.

**Code**
```rust
pub fn expire_join_counters(&self, now_ms: u64) -> usize {
    let mut map = self.invite_counts.lock().unwrap();          // the existing per-invite map (NF-27)
    let before = map.len();
    map.retain(|_, (_, first_ms)| now_ms.saturating_sub(*first_ms) <= pm_relay_protocol::JOIN_TTL_MS);
    before - map.len()
}
```

**Tests to write**
`tests/join_limits.rs`:
- `invite_counter_forgets_after_a_day` → 5 joins for one invite → 6th is 429 `too many join requests`; call `expire_join_counters(now + JOIN_TTL_MS + 1)` → returns 1; a new join for that invite → 202.
- `address_limits_use_forwarded_for_from_trusted_proxy` → trusted `127.0.0.1`; 10 joins with `X-Forwarded-For: 203.0.113.9` → 11th 429; a join with `X-Forwarded-For: 198.51.100.7` → 202.
- `forwarded_for_ignored_without_trust` → no trusted proxy; header `X-Forwarded-For` varies; the limit still trips after 10 (all from 127.0.0.1).
App (`deploy_tests.rs`), only after NF-8 is answered: `railway_vars_include_trusted_proxy` → key present with the NF-8 value.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-4'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test join_limits`
→ `test result: ok. 3 passed`. App: `cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::deploy_tests` → `ok.`.

**Done when**
- [ ] 3 relay tests pass; the app test passes (or the README risk line is written while NF-8 is open).

**Do NOT**
- Do not trust `X-Forwarded-For` from addresses not in `--trusted-proxy`.
- Do not guess Railway's range; use only the NF-8 answer.

**If it fails**
1. Joins already use `client_addr` → step 1 is a no-op; say so in the PR.
2. The invite map key includes the team → keep the key, change only the value to `(count, first_ms)`.
3. IPv6 addresses in tests count separately per address → the server counts IPv6 per /64 (`address_key`); use IPv4 in
   tests.

---

### R35-5 — Tests: tokens never leave the credential store

Size: Haiku. Deps: R33-2.

**Goal**
One test runs connect, deploy, check and remove with a real `HttpRailway` over a scripted HTTP fake and proves the token
appears nowhere except the credential store.

**Read first**
- `relay_host/railway.rs` (`Post` seam, `HttpRailway::with_post`), `relay_host/service.rs`, `relay_host/railway_tests.rs`
  `RecordingPost` (R32-3).

**Files**
- create `packages/app/src-tauri/src/relay_host/leak_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs` (`#[cfg(test)] mod leak_tests;`)

**Steps**
1. Write `ScriptedPost`: answers by the GraphQL operation name in the body (`me`, `projectCreate`, …) with canned
   replies; for `variableCollectionUpsert` it answers once with
   `{"errors":[{"message":"invalid value near rw_tok_7f3a9c_SECRET"}]}` and then success.
2. Build a `RelayHost` with `HttpRailway::with_post(ScriptedPost)`, a FakeProbe, a MemoryBackend KeyStore and a temp
   app dir; collect every notifier view as JSON.
3. Run: `connect(Railway, "rw_tok_7f3a9c_SECRET")`, `start_deploy(Railway)` (fails once at variables, then
   `start_deploy` again succeeds), `check_now(true)`, then (after R34-7 merges) `remove(false)`.
4. Assert (Tests). Run, commit `relay_host: token leak test`.

**Code**
```rust
const TOKEN: &str = "rw_tok_7f3a9c_SECRET";
const PROJECT_TOKEN: &str = "rw_proj_19b2e4_SECRET";

fn all_text_under(dir: &std::path::Path) -> String {
    let mut out = String::new();
    for e in walkdir_simple(dir) { if let Ok(b) = std::fs::read(&e) { out.push_str(&String::from_utf8_lossy(&b)); } }
    out
}
```
(`walkdir_simple` is a 10-line recursive `read_dir` helper in the test file; no new crate.)

**Tests to write**
- `token_never_leaves_the_credential_store` → after the run: no collected view JSON, no returned `Err` string, and no byte of any file under the temp app dir contains `TOKEN` or `PROJECT_TOKEN`; the deploy error text contains `[hidden]`; the KeyStore holds `PROJECT_TOKEN` under `relay_railway_project_token` (positive control) and nothing under `relay_railway_token`.
- `debug_of_host_state_has_no_token` → `format!("{:?}", host.view())` contains neither token.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-5'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::leak_tests`
→ `test result: ok. 2 passed`.

**Done when**
- [ ] 2 tests pass.

**Do NOT**
- Do not weaken `redact` to make the test pass.
- Do not skip the failing-once step; it is what proves error redaction end to end.

**If it fails**
1. The error string contains the token → the deploy path formats the raw GraphQL message somewhere; route it through
   `ApiError::Api(redact(…))`.
2. A file contains the token → find which writer wrote it (the test prints the file name, never the content).
3. `remove` is not merged yet → leave step 3's remove call out and add it in the R34-7 PR.

---

### R35-6 — Guard tests for `relay_host`

Size: Haiku. Deps: R33-2.

**Goal**
Source guards keep `relay_host` desktop-only, silent, away from p2p and team crypto, and pinned to a digest.

**Read first**
- `packages/app/src-tauri/src/p2p/guard_tests.rs` — helpers `tauri_src()`, `packages()`, `files()`, `read`, `code_only`
  and the root list in `p2p_is_not_reachable_from_the_bridge_mcp_or_the_phone` (spaces facts §2).
- `packages/app/src-tauri/src/secrets.rs` — `is_ui_secret` (relay facts §7).

**Files**
- create `packages/app/src-tauri/src/relay_host/guard_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs` (`#[cfg(test)] mod guard_tests;`)

**Steps**
1. Copy the helper functions from `p2p/guard_tests.rs` (same bodies; do not import them, so the p2p guard file stays
   unchanged).
2. Write the 8 tests below.
3. Run, commit `relay_host: guard tests`.

**Code**
Root list for test 1 (from spaces facts §2): `src/bridge`, `src/tool_registry`, `src/mcp_shim`, `src/companion`,
`src/mobile_pairing`, `src/channels`, `src/bin`, `packages/mcp-server/src`, `packages/cli/src`,
`packages/app/src/pwa`, `packages/app/src/PwaApp.tsx`, `packages/app/src/PwaLegacyApp.tsx`.
Words: `relay_host`, `relay-host`, `relayHost`, `RelayHost`, `relay_railway_token`, `relay_cloudflare_token`,
`relay_owner`.

Digest pattern for test 4:
```rust
let re = regex::Regex::new(r"^ghcr\.io/potato-dev-inc/pm-relay:\d+\.\d+\.\d+@sha256:[0-9a-f]{64}$").unwrap();
assert!(re.is_match(super::image::RELAY_IMAGE), "RELAY_IMAGE must be pinned by digest");
```
(`regex` is used by `pairing.rs`, spaces facts §5; if it is not a dependency of the app crate, check the 64 hex
characters by hand with `chars().all(|c| c.is_ascii_hexdigit())`.)

**Tests to write**
1. `relay_host_is_not_reachable_from_bridge_mcp_or_phone` → none of the words appear in the roots (code only, comments dropped).
2. `relay_host_prints_nothing` → in `relay_host/*.rs` except files ending `_tests.rs`: no `println!`, `eprintln!`, `dbg!`, `tracing::`, `log::`, `info!(`, `warn!(`, `error!(`, `debug!(`.
3. `relay_host_never_sets_test_hooks` → `TEST_HOOKS` appears in no `relay_host` file except `guard_tests.rs`.
4. `relay_image_is_pinned_by_digest` → the pattern above.
5. `relay_host_does_not_use_p2p` → no `p2p::` in `relay_host`.
6. `relay_host_does_not_touch_team_crypto` → no `team::crypto` and no `team::secure` in `relay_host`.
7. `relay_tokens_are_not_ui_secrets` → `!crate::secrets::is_ui_secret(n)` for `relay_railway_token`, `relay_railway_project_token`, `relay_cloudflare_token`, `relay_owner`.
8. `relay_host_writes_only_its_files` → `std::fs::write`, `File::create`, `fs::rename`, `OpenOptions` appear only in `config.rs` and `backup.rs` (and `_tests.rs` files).

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-6'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::guard_tests`
→ `test result: ok. 8 passed`. Then `--lib p2p::guard_tests` and `--lib desk` → `ok.`.

**Done when**
- [ ] 8 tests pass; p2p and desk guards pass.

**Do NOT**
- Do not edit `p2p/guard_tests.rs` or `desk/guard_tests.rs`.
- Do not add `relay_host` to any allow list in other guards.

**If it fails**
1. Test 1 hits `packages/app/src/pwa` because a shared settings file imports `relay-host.ts` → move the import so the
   phone app never loads it; never add an exception.
2. Test 4 fails on the placeholder → R32-1 has not published yet; this guard is meant to fail until the digest is in.
3. Test 8 hits `service.rs` → move that write into `config.rs` or `backup.rs`.

---

### R35-7 — New owner key (rotation), Rust

Size: Sonnet. Deps: R32-5, R33-5.

**Goal**
`relay_host_rotate_owner` gives the relay a new owner key and keeps the old one if anything fails.

**Read first**
- `team/keystore.rs` — `relay_owner`, `get_or_make`, `set32`/`get32` (R32-2; NF-17).
- `relay_host/deploy.rs` — `railway_vars`, deploy polling (R32-5); `relay_host/cloudflare.rs` — `upload_script`.

**Files**
- edit `packages/app/src-tauri/src/team/keystore.rs`
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- create `packages/app/src-tauri/src/relay_host/rotate_tests.rs`

**Steps**
1. KeyStore: `pub const RELAY_OWNER_NEXT: &str = "relay_owner_next";`, `pub fn relay_owner_next(&self) -> Result<SigningKey, String>`
   (same body as `relay_owner` with the other name), `pub fn promote_relay_owner(&self) -> Result<(), String>` (write
   the next key's 32 bytes under `relay_owner` with the private `set32`, then delete `relay_owner_next`),
   `pub fn drop_relay_owner_next(&self) -> Result<(), String>`.
2. Service: `pub fn rotate_owner(self: &Arc<Self>) -> Result<RelayHostView, String>`, run with `run_job(Phase::Fixing, …)`,
   steps `RotateStep { MakeKey, GiveKey, Start, Check }` (Code).
3. Command `relay_host_rotate_owner(app: AppHandle) -> Result<Value, String>`; register it.
4. Tests, run, commit `relay_host: rotate owner key`.

**Code**
```rust
pub const ROTATED: &str = "Your relay now uses a new owner key.";
pub const ROTATE_FAILED: &str = "We couldn't change the key, so your relay keeps the old one.";
```
Order: `MakeKey` (`relay_owner_next`); `GiveKey` (Railway: `set_variables` with `railway_vars(key_id(next))`;
Cloudflare: `upload_script(first = false)` with the new id); `Start` (Railway: `deploy` and poll to `SUCCESS`);
`Check` (`wait_until_ready` with `owner = next`, `my_owner_id = key_id(next)`); success → `promote_relay_owner`,
notice `ROTATED`. Any failure after `GiveKey` → put the old id back the same way, deploy, wait, `drop_relay_owner_next`,
error `ROTATE_FAILED`.

**Tests to write**
`rotate_tests.rs`:
- `rotate_happy_path_promotes_new_key` → afterwards `relay_owner()` public key == the `next` key made during the run, and != the key before; `relay_owner_next` gone; notice `ROTATED`.
- `rotate_failure_keeps_old_key` → FakeProbe refuses the new owner → `relay_owner()` unchanged; `relay_owner_next` gone; the last `set_variables` call carried the old id; error `ROTATE_FAILED`.
- `rotate_sets_only_the_owner_variable_change` → the `set_variables` map equals `railway_vars(new_id)` exactly.
- `promote_without_next_is_an_error` → `promote_relay_owner()` with no next key → `Err`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r35-7'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::rotate_tests`
→ `test result: ok. 4 passed`.

**Done when**
- [ ] 4 tests pass; command registered.

**Do NOT**
- Do not delete the old key before the relay accepts the new one.
- Do not rotate the team root key or device keys here (different keys, different rules).

**If it fails**
1. `set32` is private and takes another type → write `promote_relay_owner` inside `keystore.rs`, next to `set32`.
2. The relay keeps answering with the old owner for a while → `wait_until_ready` repeats for 3 minutes; that covers a
   Railway restart.
3. Cloudflare re-upload error → see NF-11; fix only `metadata(first = false)`.

---

### R35-8 — New owner key button (UI)

Size: Haiku. Deps: R35-7, R34-12.

**Goal**
The ready card has an Advanced part with **Make a new owner key**.

**Read first**
- `RelayReadyCard.tsx`, `relay-host.ts`, `RelayHostSettings.test.tsx` (file 33, R34-12).

**Files**
- edit `packages/app/src/lib/relay-host.ts`
- edit `packages/app/src/components/settings/relay/RelayReadyCard.tsx`
- edit `packages/app/src/components/settings/relay/RelayHostSettings.tsx`
- edit `packages/app/src/components/settings/relay/RelayHostSettings.test.tsx`

**Steps**
1. `relay-host.ts`: `rotateOwner: () => invoke<RelayHostView>('relay_host_rotate_owner')`; view field
   `rotateStep: 'make_key' | 'give_key' | 'start' | 'check' | null`; `ROTATE_STEPS` with texts `Making a new key`,
   `Giving it to your relay`, `Starting the relay`, `Checking that it works`.
2. `RelayReadyCard.tsx`: a `<details>` with summary `Advanced`, the text
   `Use this if you think someone copied this computer's files. Your relay restarts once, and your team notices nothing.`
   and the button `Make a new owner key`.
3. `RelayHostSettings.tsx`: in `fixing`, when `rotateStep` is set, render `RelayWorking` with title
   `Changing the owner key` and `ROTATE_STEPS`.
4. Tests, run, commit `ui: new owner key button`.

**Code**
```tsx
<details className="ck-relay-advanced">
  <summary>Advanced</summary>
  <p>Use this if you think someone copied this computer&rsquo;s files. Your relay restarts once, and your team notices nothing.</p>
  <button type="button" onClick={onRotate}>Make a new owner key</button>
</details>
```
(`RelayReadyCard` gains the prop `onRotate: () => void`.)

**Tests to write**
- `advanced shows Make a new owner key` → open `Advanced` → button present → click → `api.rotateOwner` called.
- `rotating shows the steps` → `phase: 'fixing', rotateStep: 'give_key'` → heading `Changing the owner key`, current item `Giving it to your relay`.

**Run**
`npx vitest run packages/app/src/components/settings/relay` → all pass, 2 new.

**Done when**
- [ ] 2 tests pass.

**Do NOT**
- Do not show any key id or key text.

**If it fails**
1. `<details>` content not found until opened → click the `Advanced` summary first in the test.
2. Two `Checking that it works` items match → scope to the section named `Changing the owner key`.
3. `onRotate` missing in older tests → give the prop a default no-op in those tests' render helper.

---

## Open facts

- NEEDS-FACT NF-8: Railway's proxy source address range and its `X-Forwarded-For` behaviour. Read Railway's networking
  docs, or log the peer address in a founder-run test deploy.
- NEEDS-FACT NF-17: the private `KeyStore` helpers `name`, `get_or_make`, `get32`, `set32`. Read
  `packages/app/src-tauri/src/team/keystore.rs`.
- NEEDS-FACT NF-22: the old `teams` table statement (for the migration test). Read `Relay::open` in
  `relay/server/src/lib.rs`.
- NEEDS-FACT NF-27: the `SeenSigs` type, the rate-window helper, the per-invite counter map, and the sweep entry point.
  Read `relay/server/src/lib.rs` lines 160-260 and the sweeper in `serve`.
- NEEDS-FACT NF-32: whether any app code outside `relay_host` logs request headers of outbound HTTP calls (a global
  ureq middleware or logger would see the `Authorization` header). Search `packages/app/src-tauri/src` for `ureq` agent
  builders with middleware and for a logging init that captures HTTP.
- NEEDS-FACT NF-39: how the app handles `reset: true` from a batch or a `Resync` frame today. Read
  `packages/app/src-tauri/src/team/transport/relay.rs` lines 180-230 and `team/service.rs` lines 160-180.

## Security

This whole file is the security plan. Defences and their tests, in one list:

| Defence | Test |
|---|---|
| Owner-only registration | `owner_relay_refuses_unsigned_team_registration`, `other_key_is_refused` (R31-5), `owner worker refuses unsigned registration` (R32-7) |
| Owner signature on admin routes, 404 without owner | `admin_status_needs_owner`, `admin_status_is_404_without_owner_key` (R31-5), `backup_needs_owner` (R34-4), `restore_needs_owner` (R34-8) |
| Replay guard on owner requests | `owner_signature_replay_is_refused` (R31-5) |
| Tokens only in the credential store | `token_never_leaves_the_credential_store` (R35-5) |
| Least privilege after setup | `deploy_happy_path_makes_everything_in_order` (R32-5) |
| No test hooks in production | `railway_vars_are_exactly_these` (R32-5), `metadata_never_has_test_hooks` (R32-9), `relay_host_never_sets_test_hooks` (R35-6) |
| Digest-pinned image | `relay_image_is_pinned_by_digest` (R35-6) |
| Desktop-only commands | `relay_host_is_not_reachable_from_bridge_mcp_or_phone` (R35-6) |
| Wrong relay detection | `judge_owner_mismatch_is_wrong_relay` (R32-4) |
| Admin-only team moves | `relay_moved_from_non_admin_is_ignored` (R34-11) |
| M1 counter | `seq_keeps_rising_after_mailbox_empties`, `cursor_ahead_of_head_gets_reset` (R35-1) |
| M3 auth before replay memory | `failed_auth_does_not_fill_replay_memory`, `bad_signatures_from_one_address_are_slowed` (R35-2) |
| M4 wake coalescing, frame limit | `many_posts_wake_each_socket_few_times`, `chatty_client_socket_is_closed` (R35-3) |
| M5 proxy addresses, invite counters, storage | `address_limits_use_forwarded_for_from_trusted_proxy`, `invite_counter_forgets_after_a_day` (R35-4), `full_relay_refuses_new_team` (R31-6) |
| Logs without identifiers | `log_shows_pattern_not_values`, `only_main_and_access_log` (R31-4) |
| Owner key rotation keeps the old key on failure | `rotate_failure_keeps_old_key` (R35-7) |

Residual risks (stated to the founder, not fixed here): shared per-address join limits on Railway until NF-8; Railway's
and Cloudflare's own edge logs keep client addresses; malware running as the person can read the credential store; the
pasted token stays in the clipboard (D-16).

## What cannot be verified without real hardware or a second computer

- Real client addresses as seen behind Railway's proxy (NF-8).
- M3 and M4 under real internet load; the tests use loopback.
- Credential-store isolation between two OS user accounts and between dev profiles on another machine.
- Whether a real Railway project token is refused for `projectDelete` (NF-9), which decides the remove path.
- An attacker on a second network flooding joins while a real invitee joins from a third network.
