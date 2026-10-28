# Team relay

The post office for Puppet Master's Team ("Heads-up") feature: teammates' apps leave sealed,
signed envelopes here and pick them up later. It is end-to-end encrypted; the relay sees only team
ids, device public keys, sizes and times.

- `protocol/` (`pm-relay-protocol`, **MIT**): the wire types and signature checks, shared with the app.
- `server/` (`pm-relay`, **licence pending**, see `LICENSE`): a single Rust binary (axum + SQLite).
- `worker/` (**licence pending**, see `worker/LICENSE`): the same relay as a Cloudflare Worker with one
  Durable Object per team. Same wire protocol, so the app works against either unchanged.

## Pick one

| | Good for | How |
|---|---|---|
| **Cloudflare** | No machine to run, free to start, TLS included | The "Deploy to Cloudflare" button in [`worker/README.md`](worker/README.md), or `npm run deploy` there |
| **Docker** | A machine you already have | `docker build -t pm-relay relay` then `docker run -d --restart unless-stopped -p 127.0.0.1:8787:8787 -v pm-relay-data:/data pm-relay`, with TLS in front |
| **The binary** | The same, without Docker | `cargo run --release -p pm-relay` (below) |
| Puppet Master's own | Nothing to host | A hosted relay is planned; it speaks the same protocol (see [`HOSTED-RELAY.md`](HOSTED-RELAY.md)) |

All of them keep only sealed envelopes, so which one you pick changes who runs the mailbox, not who can
read it. Settings › Team › Relay address takes the `https://…` address of whichever you run.

## Run the binary

```sh
cargo run --release -p pm-relay -- --listen 127.0.0.1:8787 --db relay.sqlite
```

Put it behind TLS (a reverse proxy, or `cloudflared`) for teammates on other networks: the app only
accepts `http://` for a relay on the same computer.

Behind a reverse proxy, tell the relay which addresses are your proxies, so per-address limits count each client
and not the proxy (without it, every client looks like the proxy and shares one budget):

```sh
pm-relay --listen 127.0.0.1:8787 --db relay.sqlite --trusted-proxy 127.0.0.1 --trusted-proxy 10.0.0.0/8
```

`X-Forwarded-For` is read only from those addresses (right to left, skipping them); from anyone else it is ignored.
IPv6 clients are counted per /64.

## What it keeps

| What | How long |
|---|---|
| Envelopes (ciphertext + sender device, epoch, counter) | 7 days |
| Join requests (sealed to the admin's device) | 1 day |
| The `roster` and `keys` slots (signed by the team's root key, opaque) | until replaced by a newer version |
| The access list (device public keys allowed per team, root-signed) | until replaced |

## Mailbox positions

Each team has a counter that only grows (`teams.next_seq`); an envelope's position never repeats, even after every
older envelope has expired, so clients' cursors stay valid. If a client asks for a position past the newest one (a
restored or replaced database), the reply says `"reset": true` with the team's `head` (a `{"reset":true,"head":N}`
frame on the WebSocket) and starts again from the beginning; the app takes the lower cursor and drops envelopes it
has already seen.

## Who reads what (no forward secrecy)

- A device reads only envelopes posted **after it was added to the access list** (`devices.from_seq`). A new member
  does not receive the team's last 7 days.
- There is **no forward secrecy**: the relay keeps ciphertext for 7 days and every device keeps every team key it was
  given until it leaves the team. Whoever gets a device's keys and this database can read what that device could.
- Removing a device cuts it off here at once (new requests and open sockets). Members whose app hasn't seen the new
  key yet may still seal a message or two under the old one for about 10 seconds.

## Limits

60 envelopes a minute and 5,000 a day per device, 600 a minute per team, 300 signed requests a minute per device,
24 KiB per envelope, at most 20,000 envelopes or 128 MiB stored per team (more is refused with 507), 4 open
WebSockets per device, 5 tries per invite (for a day), 200 join requests a day per team, 10 join requests an hour per
address per team, 30 team registrations an hour per address.

Memory stays bounded: request signatures are remembered for the replay window only, and only for devices on the
access list (a stranger's signed requests are refused before anything is stored); rate-limit windows are pruned by
the sweeper. Rate limits live in memory and reset when the relay restarts. Registration is open: a public relay
should sit behind a proxy that limits it.

## Tests

```sh
cargo test        # protocol unit tests + server end-to-end over loopback HTTP and WebSocket
```

The server's end-to-end tests (`server/tests/protocol.rs`) and the app's relay tests run against any relay
when `PM_RELAY_TEST_URL` is set: that is how the Worker variant is proven (`worker/README.md`, Tests).

## Live Rooms

A Live Room is a read-only, end-to-end encrypted link to some agents' chat (`docs/features/live-room.md`). The
relay keeps the room and serves the viewer page. It never sees the key, which lives in the link's `#` part. The
protocol types are in `protocol/src/room.rs`; the routes are in `server/src/rooms.rs` and `server/src/rooms/routes.rs`.
Room routes are on the same listener as the team routes and have their own limits. The Cloudflare Worker has the
same room routes (`worker/src/room.ts`, `roomproto.ts`, `index.ts`).

### Routes

| Route | Who | Body and answer |
|---|---|---|
| `POST /v1/rooms` | anyone (the signature is inside) | `RoomRegistration`, at most 8 KiB. 201 `{"room": …}`. A used id is 409 (tombstones included) |
| `PUT /v1/rooms/{room}/snapshot` | owner (`x-pm-auth`, room key) | one `snap` frame, at most 96 KiB. Replaces the snapshot and drops the older deltas. 200 `RoomStatus` |
| `POST /v1/rooms/{room}/frames` | owner | one `delta` or `state` frame, at most 24 KiB, `seq` must rise. 200 `RoomStatus` |
| `GET /v1/rooms/{room}/status` | owner | 200 `RoomStatus` (viewer count and end time only) |
| `DELETE /v1/rooms/{room}` | owner | 204. Frames and snapshot are wiped at once; the tombstone stays |
| `GET /v1/rooms/{room}/snapshot` | viewer (`x-pm-room-read`, `x-pm-room-seat`) | 200 frame, 204 (none yet), 404, 410 `{"ended": …}`, 429 `{"full": true}` |
| `GET /v1/rooms/{room}/frames?after=N&wait=S` | viewer | 200 batch of up to 100 frames; `wait` is capped at 25 s |
| `GET /r/`, `/r/viewer.js`, `/r/viewer.css` | anyone | the viewer page, with a strict policy (`default-src 'none'`, scripts and styles from the relay only, no framing, no referrer) |

Viewer calls are GET only and refuse a body. Wrong read token and unknown room both answer 404. Answers on room
routes carry `Cache-Control: no-store`. No CORS headers: the page is same-origin.

### Limits

| Limit | Default | Flag |
|---|---|---|
| Room registrations per address | 10 an hour | `--rooms-per-address-per-hour` |
| Open rooms on this relay | 100 (more is 503) | `--max-open-rooms` |
| Owner frames per room | burst 10, then 2 a second; 7,200 a day | `--room-frames-per-second`, `--room-frames-per-day` |
| Viewer calls per address | 120 a minute | `--viewer-requests-per-minute` |
| Seats per address in one room | 4 | `--seats-per-address` |
| Deltas kept per room | the newest 300, or 2 MiB | `--room-ring-bytes` |
| Room bytes on this relay | 256 MiB in all (at the cap the busiest room drops its oldest deltas; 507 if snapshots alone do not fit) | `--global-room-bytes` |
| Room lifetime | 8 hours at most (the app asks for less) | fixed |
| Viewers per room | 50 at most (the room sets 1 to 50) | fixed |
| Seat quiet time | a viewer seat that makes no call for 40 s frees its place | fixed |

Seats are counted in memory only, and are never logged. The relay writes no log line per viewer request.

### What it keeps

| What | Where | How long |
|---|---|---|
| Ciphertext frames (the newest snapshot and the deltas) | memory of the binary | until the room stops or its time is up; stop or expiry wipes them at once. At most 8 hours |
| Registration: room id, room public key, hash of the read token, expiry, viewer limit | memory of the binary | until the tombstone ends |
| Tombstone (a stopped or ended room answers 410 `{"ended": …}`) | memory of the binary | until 1 hour after the planned end, then 404 |
| Seats (one random id per viewer page load) | memory of the binary | 40 s after the last call; counted, never stored |

Rooms are in memory only. A restart of the binary ends every room, and the owner's app then sees "ended by the
relay". Nothing about a room goes to the `--db` SQLite file (that file is for teams).

On the Cloudflare Worker each room is one Durable Object with its own SQLite tables (`room`, `frames`,
`snapshot`). An alarm at the room's end ends it and wipes its frames. A second alarm, 1 hour after the end, wipes
the rows. One more object (`RoomIndex`) counts the open rooms for the cap of 100: a stop frees the slot at once,
and an expired room frees it when the next room is registered. The Worker's seats are also in memory only.

### Tests

```sh
cargo test -p pm-relay-protocol   # the room types and checks
cargo test -p pm-relay --test rooms   # register, snapshot, frames, viewer reads, limits, stop and expiry, viewer page headers
cd worker && npm run test:worker  # the Worker's room tests (test/room.test.mjs) against local wrangler dev
```

The viewer page has its own tests (`relay/viewer`, `npm test`). Build it with `npm run build` there when `src/`
changes, and commit `dist/`: the relay serves the committed files.
