# Puppet Master relay on Cloudflare

The team relay ("Heads-up") as a Cloudflare Worker with one Durable Object per team. It speaks the
same wire protocol as the Rust server in `../server`, so the app's team settings work against it
unchanged: you only paste its address. It is end-to-end encrypted like the server: the relay sees team
ids, device public keys, sizes and times, never names, branches, notes or keys.

Use it when you want to host your own relay for free-ish, with no machine to keep running. (Prefer a
single binary or Docker? See [`../README.md`](../README.md).)

## Deploy

### One click

[![Deploy to Cloudflare](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https://github.com/Potato-dev-inc/puppet-master/tree/main/relay/worker)

Cloudflare copies this folder into your own Git account, builds it and deploys it to your Cloudflare
account. You need a free Cloudflare account. The repository must be public for the button to work
(Cloudflare reads it without your credentials).

When it finishes you get an address like `https://pm-relay.<your-name>.workers.dev`. Open
`<address>/v1/health`: it should answer `{"ok":true,"protocol":"pm-relay/1"}`.

### By hand

```sh
cd relay/worker
npm install          # wrangler is a dev dependency of this folder, not installed globally
npx wrangler login   # once; opens your browser to sign in to your Cloudflare account
npm run deploy
```

Nothing needs building: the protocol checks are a WebAssembly file committed in `src/wasm/` (see
"How it works"). To try it on your own computer first: `npm run dev` serves it on
`http://127.0.0.1:8787` (no account, no login).

### Then, in Puppet Master

Settings › Team › Relay address: paste the `https://…` address and create the team. Teammates' invite
links carry the relay address, so they don't type it. Use `https://`; the app only accepts plain
`http://` for a relay on the same computer.

## What it keeps, and for how long

Per team, in its Durable Object's SQLite storage:

| What | How long |
|---|---|
| Envelopes (ciphertext + sender device, epoch, counter) | 7 days (an alarm sweeps hourly; reads never return an expired one) |
| Join requests (sealed to the admin's device) | 1 day |
| The `roster` and `keys` slots and the access list (root-signed, opaque) | until replaced by a newer version |
| Replay guard (signatures seen in the last ~11 minutes) and rate-limit counters | minutes to a day, then swept |

Per client address, a counter for team registrations; the object is named by a hash of the address, so no
address is stored. Join requests are counted inside the team's own object under a hash of the address
(an IPv6 address by its /64), which expires after an hour. An idle team costs nothing: open WebSockets use Cloudflare's
hibernation API, so the object sleeps between messages.

## Limits

The same numbers as the server (`Limits` in `../server/src/lib.rs`):

- 60 envelopes a minute and 5,000 a day per device, 600 a minute per team, 24 KiB per envelope;
- 300 signed requests a minute per device, reads included (429);
- a team keeps at most 20,000 envelopes or 128 MiB (507 "the team's mailbox is full"; expiry frees it);
- at most 4 open WebSockets per device (429 "too many open connections for this device");
- join requests: 10 an hour per address and team, 200 a day per team, 5 per invite (checked in that order);
- 30 team registrations an hour per address; long polls up to 25 seconds.

To change a number, uncomment the matching `LIMIT_…` line under `[vars]` in `wrangler.toml` (they are listed there).

Cloudflare's own allowances apply on top (the free plan has daily request, duration and storage-row
limits: see Cloudflare's pricing page for Workers and Durable Objects). A message costs 8 storage-row writes here
(the envelope, the team's counters, four rate-limit counters, the replay guard) and about 2 more when its
replay-guard row expires and 1 when the envelope does; a read costs 3. Opening a socket and sending a message
cost one request each; the messages that arrive over an open socket cost nothing. For a team of five sending
200 messages each a day that is about 11,000 rows written a day, so the free plan (100,000 a day) holds
roughly eight such teams; more than that, or a team whose apps cannot use WebSockets, needs the paid plan.
`../HOSTED-RELAY.md` has the full estimate. A test (`npm run test:worker`) fails if a message starts costing more.

## Hardening (what the server's security review found, done here from the start)

- The mailbox counter only grows (a quiet week cannot make new mail invisible to a client's cursor).
- A request signature is stored only after the device is known to be on the team's access list, and expired
  ones are deleted on every call: strangers leave nothing behind.
- Rate-limit counters live in each team's own object, are pruned as they expire, and a team's daily join
  budget is spent before per-invite counters are created, so junk invite ids cannot grow them.
- A new envelope is built once and sent to every open socket; no database query per socket per message.
  Only a socket that is behind catches up with its own query. A device keeps at most 4 sockets (the fifth is
  refused with 429) and a client message over 4 KiB closes its socket.
- Per-address limits read `CF-Connecting-IP`, which Cloudflare sets and a client cannot forge, so the limit is
  per client and not for the whole relay. An IPv6 client counts by its /64, an IPv4-mapped one as its IPv4 address.
  Join requests are limited per address *and team*, so one address cannot close joining for other teams.
- A team's mailbox holds at most 20,000 envelopes or 128 MiB (`LIMIT_TEAM_MAX_ENVELOPES`, `LIMIT_TEAM_MAX_BYTES`);
  a full mailbox answers 507. The counters are kept in the team row and checked before the insert.
- **Join points.** A device added to the access list reads only what the team posts after it was added
  (`devices.from_seq`); everyone listed at registration reads from the start. Removing a device deletes its row
  and closes its socket and any long poll it has waiting (403), and adding it back gives it a new join point.
  Objects stored before this change are migrated when they wake: every listed device starts at 0 and the
  storage counters are filled from the rows.
- **A cursor ahead of the relay.** Every mailbox reply carries `head` (the team's newest position). When `after`
  is past it (a restored or replaced database), the reply says `"reset": true` and starts again from the device's
  join point; a WebSocket sends `{"reset":true,"head":N}` first. These shapes are `Batch`/`Resync` from
  `relay/protocol`, serialised by the wasm module.

Not covered here: a flood of badly signed requests costs the Worker one signature check each; if that worries
you, add a Cloudflare rate-limiting rule on the Worker's route (dashboard, Security > WAF).

## How it works

```
app (RelayTransport) ──HTTPS / WSS──▶ Worker (src/index.ts) ──▶ Team Durable Object (src/team.ts)
                                         │  size, signature, per-address limit      mailbox · access list · state
                                         └▶ IpGate Durable Object (src/gate.ts)     replay guard · limits · alarm
```

- **`src/index.ts`**: the front door. Sorts the route, refuses oversized bodies (413), checks the device
  signature, counts unauthenticated calls per address, then hands the request to the team's object.
- **`src/team.ts`**: one object per team: team exists, device on the access list, request not
  replayed, rate limits, SQLite storage, the long poll, WebSockets, and the expiry alarm.
- **Protocol checks are Rust.** `wasm/` is a tiny crate over `../protocol` (the MIT crate the app and the
  Rust server also use) compiled to WebAssembly. Every signature, shape and binding rule (strict
  Ed25519, the signed bytes, team id = hash of the root key, size and field checks) therefore *is*
  the shared code, not a port of it. The result is committed in `src/wasm/` so deploying needs no Rust.
- **Wire-compatible, not just similar:** same routes, status codes and error messages, same
  `Delivered`/`Batch` JSON, and a path prefix works because the app signs the path it used.

### Rebuilding the WebAssembly

Only needed when `../protocol` or `wasm/` changes:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match wasm/Cargo.toml
npm run build:wasm                                          # writes src/wasm/, prints the sha256
```

## Differences from the server

- No trusted-proxy setting: the server reads `X-Forwarded-For` only from proxies you list; the Worker reads
  `CF-Connecting-IP`, which Cloudflare sets. (That one test of the shared suite is axum-only.)
- Rate-limit windows and the replay guard live in the team's storage, not in memory (an object's memory can be
  evicted), so they cost row writes.
- Long polls hold a Durable Object awake for up to 25 seconds; clients use the WebSocket first, so this
  is the fallback. WebSockets hibernate.
- The WebSocket has no server ping (the Workers runtime cannot send one while hibernating). A client may send the
  text `ping` now and then: the runtime answers `pong` without waking the object, and Cloudflare closes a
  connection that stays silent for too long, so the app does this every 45 seconds. A dropped connection is
  noticed by the app, which falls back to polling and tries the socket again.
- Cloudflare terminates TLS, so there is nothing to put in front of it.

## Tests

```sh
npm test               # everything below
npm run test:worker    # the Worker's own tests (node:test) against two local `wrangler dev` servers
npm run test:app       # the Rust end-to-end tests, pointed at the local Worker
```

All of it runs on your computer under `wrangler dev` (Miniflare/workerd) with throwaway storage.
Nothing logs in to or deploys to Cloudflare. The Rust parts need `cargo`.

The Rust tests are the **same** end-to-end tests the server passes; set `PM_RELAY_TEST_URL` and they use
that relay instead of the in-process one:

- `relay/server/tests/protocol.rs`: registration, access list, replay, mailbox cursors and long polls,
  WebSocket backlog and live delivery, a removed device's socket closing, signed state slots, join
  requests, rate limits, `head`/`reset`, join points, a removed device's waiting poll, the per-device request
  cap, the storage quota (507) and the socket cap. Only `per_address_limits_read_forwarded_addresses_only_from_trusted_proxies`
  is axum-only.
- the app's `relay_end_to_end_create_invite_join_push_and_card_with_metadata_only_on_the_relay`,
  `on_the_relay_a_removed_member_is_locked_out_and_told_so` and
  `an_envelope_the_relay_drops_shows_up_as_a_gap` (`packages/app/src-tauri/src/team/tests_v1.rs`),
  which drive the real `RelayTransport`.

They need a few things only a test may do (look inside the store, drop an envelope, tighten a limit,
age the clock). Those are `/v1/_test/…` routes that exist only when `TEST_HOOKS=1` is set, which the test
runner does and a deployment never does.

## Licence

Pending the founder's decision, like the server: see [`LICENSE`](LICENSE). The protocol crate it is built
on is MIT.
