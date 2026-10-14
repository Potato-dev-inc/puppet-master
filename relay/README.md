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

## What it keeps

| What | How long |
|---|---|
| Envelopes (ciphertext + sender device, epoch, counter) | 7 days |
| Join requests (sealed to the admin's device) | 1 day |
| The `roster` and `keys` slots (signed by the team's root key, opaque) | until replaced by a newer version |
| The access list (device public keys allowed per team, root-signed) | until replaced |

## Limits

60 envelopes a minute and 5,000 a day per device, 600 a minute per team, 24 KiB per envelope,
5 tries per invite, 20 join requests a day per team, 30 unauthenticated requests an hour per address.

## Tests

```sh
cargo test        # protocol unit tests + server end-to-end over loopback HTTP and WebSocket
```

The server's end-to-end tests (`server/tests/protocol.rs`) and the app's relay tests run against any relay
when `PM_RELAY_TEST_URL` is set: that is how the Worker variant is proven (`worker/README.md`, Tests).
