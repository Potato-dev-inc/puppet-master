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
| **Docker** | A machine you already have | `docker build -t pm-relay relay` then `docker run -d -p 8787:8787 -v pm-relay-data:/data pm-relay`, with TLS in front |
| **The binary** | The same, without Docker | `cargo run --release -p pm-relay` (below) |
| Puppet Master's own | Nothing to host | A hosted relay is planned; it speaks the same protocol |

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
