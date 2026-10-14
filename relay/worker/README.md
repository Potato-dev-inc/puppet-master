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

Per client address, a counter for registrations and join requests; the object is named by a hash of the
address, so no address is stored. An idle team costs nothing: open WebSockets use Cloudflare's
hibernation API, so the object sleeps between messages.

## Limits

The same as the server: 60 envelopes a minute and 5,000 a day per device, 600 a minute per team, 24 KiB per
envelope, 5 tries per invite, 20 join requests a day per team, 30 unauthenticated requests an hour per
address, long polls up to 25 seconds. To change a number, uncomment the matching `LIMIT_…` line under
`[vars]` in `wrangler.toml` (they are listed there).

Cloudflare's own allowances apply on top (the free plan has daily request, duration and storage-row
limits: see Cloudflare's pricing page for Workers and Durable Objects). A message costs about seven
storage-row writes here (the envelope, the counter, three rate-limit counters, the replay guard).
That is plenty for a team, and the paid plan removes the ceiling.

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

- The mailbox counter only ever grows. (The server numbers a new envelope as the highest one it still
  holds plus one, so after a week of silence, when every old envelope has expired, it starts again at 1
  and a client holding a higher cursor would not see the new mail. The Worker keeps a counter.)
- Long polls hold a Durable Object awake for up to 25 seconds; clients use the WebSocket first, so this
  is the fallback. WebSockets hibernate.
- The WebSocket has no server ping (the Workers runtime cannot send one while hibernating). A dropped
  connection is noticed by the app, which falls back to polling and tries the socket again.
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
  requests, rate limits.
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
