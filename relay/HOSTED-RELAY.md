# Puppet Master hosted relay: what it needs

This page is for the founder. It says what to buy, what to type, and what to decide before the
"Puppet Master hosted relay" goes live. Nothing here has been deployed. No account, domain or key
exists yet.

The relay is a small mailbox. Teammates' apps leave sealed (end-to-end encrypted) messages in it and
pick them up. The relay cannot read them. If it is lost, no secrets are lost: the messages are only
kept for 7 days.

## 1. What you need

1. A **Cloudflare account** (free to open). Use a company email, not a personal one.
2. A **plan**. See section 2.
3. A **name** for the relay, for example `relay.<your-domain>`. See section 4.
4. A computer with **Node.js 22 or newer** and this repository. (Rust is not needed to deploy.)

## 2. Plan and cost

Prices were read from Cloudflare's pages on 2026-10-17. Prices change: check the current price before
you pay.

- Workers pricing: <https://developers.cloudflare.com/workers/platform/pricing/>
- Durable Objects pricing: <https://developers.cloudflare.com/durable-objects/platform/pricing/>

| | Free | Workers Paid |
|---|---|---|
| Price | 0 | 5 USD a month (minimum) |
| Worker requests | 100,000 a day | 10 million a month, then 0.30 USD per million |
| Durable Object requests | 100,000 a day | 1 million a month, then 0.15 USD per million |
| Durable Object time | 13,000 GB-s a day | 400,000 GB-s a month, then 12.50 USD per million GB-s |
| Storage rows written | 100,000 a day | 50 million a month, then 1 USD per million |
| Storage rows read | 5 million a day | 25 billion a month |
| Stored data | 5 GB | 5 GB, then 0.20 USD per GB-month |

On the free plan, when a daily limit is reached the relay stops answering until 00:00 UTC. The paid plan
does not stop; it bills more. There is no hard spending cap, so turn on a billing alert in the dashboard.

**Recommendation:** start on **Workers Paid (5 USD a month)** for a public hosted relay. The free plan
is fine for a test and for a few teams (see the estimate), but one busy day should not stop everyone's
team notifications.

### How much does a team use? (an estimate)

How it was worked out: the relay's own tests count the storage rows each request reads and writes in
Cloudflare's local runtime (`npm run test:worker`). One message sent: 1 Worker request, 1 Durable Object
request, 8 rows written, 8 rows read. About 3 more rows are written later, when the message and its
replay-guard row are deleted. Messages that arrive over an open WebSocket cost nothing extra.
The object sleeps (hibernates) about 10 seconds after the last message; Cloudflare does not bill sleeping time.

| Per day | 1 team: 5 people, 200 messages each (1,000 messages) | 50 such teams |
|---|---|---|
| Worker requests | about 1,100 | about 55,000 (limit 100,000: fine) |
| Durable Object requests | about 1,100 | about 55,000 (fine) |
| Storage rows written | about 11,500 | about 575,000 (free limit 100,000: **too much**) |
| Storage rows read | about 10,000 | about 500,000 (limit 5 million: fine) |
| Object time, worst case (every message alone, 10 s awake each) | about 1,300 GB-s | about 64,000 GB-s (free limit 13,000: **too much**) |
| Object time, realistic (messages come in bursts) | about 600 GB-s | about 30,000 GB-s |
| Stored data (7 days of mail, 2 KB each) | about 14 MB | under 1 GB (fine) |

What this means:

- **Free plan:** about **8 teams of this size**. The first limit hit is storage rows written, then object time.
- **50 teams:** needs Workers Paid. Estimated bill: the 5 USD base, plus about 0 to 20 USD for object time
  (a month is 0.9 to 1.9 million GB-s; 0.4 million are included). Rows written (about 17 million a month) and
  requests are inside the included amounts. So **about 5 to 25 USD a month for 50 teams**.
- These are estimates from a model, not a measured production bill. The two numbers that matter most
  (the 10 seconds awake after a message, and how messages cluster) can only be seen on real Cloudflare.

### The one thing that breaks the estimate

If a team's computers cannot keep a WebSocket open (some company proxies block them), the app falls back to
long polling: one request every 20 seconds per computer, and the team's object stays awake all day.
One such team uses about 21,600 requests and about 11,000 GB-s a day: almost all the free object time.
Watch the dashboard for a few teams with very high request counts. It is not a bug in the relay, but it is
the reason to be on the paid plan.

**Live Rooms use more Durable Object time than the team mailbox.** Once the Worker has the Live Room routes (the LR-11
change), each viewer call costs one `IpGate` write (the per-address counter) and one call to the room's Durable
Object. A viewer on a quiet room long-polls, so it calls about every 25 seconds: about 144 calls an hour, or about
2,900 Durable Object requests an hour for 10 viewers. A long poll keeps the room's object awake for up to 25 seconds,
so while a viewer is connected the object is awake most of the time. A busy room calls more often: a viewer waits at
most one second between good calls. These numbers come from reading the code, not from a measured bill, so check
them on the dashboard during the first real rooms. Plan for the paid tier (section 2) before you offer hosted Live
Rooms.

## 3. The Cloudflare Worker and the Docker image

Two ways to run the same relay (same messages, same app). Pick one for the hosted relay:

- **Cloudflare Worker** (recommended): no machine to look after, TLS included, near the user.
- **Docker image**: one small container with a SQLite file. Needs a machine, TLS in front, and backups.
  Use it for people who want to host their own, or if you do not want to depend on Cloudflare.

## 4. The name (domain)

- **Quick start, free:** Cloudflare gives every account `https://pm-relay.<account-name>.workers.dev`.
  Fine for testing. Do not give it to customers: if you ever move, every invite link that contains it breaks.
- **Real use:** a name on your own domain, for example `relay.<your-domain>`. Put the domain on Cloudflare
  (Add a site, change the name servers at the registrar). Then uncomment the `routes` line in
  `relay/worker/wrangler.toml`, put your name in it, and deploy again. Cloudflare creates the DNS record
  and the certificate for you.
- Invite links carry the relay address. **Choose the final name before the first customer uses it.** Changing
  it later means teams must be created again.
- Use `https://` only. The app refuses plain `http://` except on the same computer.

## 5. The commands

Run these in `relay/worker`. `wrangler` is installed by `npm ci` into that folder; nothing is installed
globally. Do this only when you decide to go live.

```sh
cd relay/worker
npm ci                       # exact versions from the lock file
npm test                     # optional: all tests, on this computer, no login (needs Rust for the Rust part)
npx wrangler login           # opens a browser; sign in to the Cloudflare account
npm run deploy               # uploads the Worker and creates its Durable Objects
```

The WebAssembly file (the signature checks, built from `relay/protocol`) is already committed in
`src/wasm/`, so there is nothing to build. It was rebuilt from source on 2026-10-17 and came out
byte for byte the same (sha256 in `src/wasm/BUILD.txt`). Only if `relay/protocol` or `relay/worker/wasm`
changes, rebuild it first:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm run build:wasm
```

`npm run deploy` prints the address. Never set `TEST_HOOKS` on the real deployment: it opens a door
to read and wipe a team's data. (The shipped `wrangler.toml` does not set it.)

### Check that it works

```sh
curl https://<the address>/v1/health
```

The answer must be exactly `{"ok":true,"protocol":"pm-relay/1"}`. Also check that this is **refused**
(404), which proves the test door is closed:

```sh
curl -i https://<the address>/v1/_test/0000000000000000000000000/dump
```

Last check: in Puppet Master, Settings, Team, put the address in "Relay address", create a team, invite a
second computer, and send a Heads-up. The status should read "Relay · live".

### Docker (for people who host it themselves)

On a machine with Docker, from the repository root. The first build takes several minutes.

```sh
docker build -t pm-relay relay
docker run -d --name pm-relay --restart unless-stopped -p 127.0.0.1:8787:8787 -v pm-relay-data:/data pm-relay
docker ps                                    # STATUS becomes "healthy" after about 30 seconds
curl http://127.0.0.1:8787/v1/health         # {"ok":true,"protocol":"pm-relay/1"}
docker stop pm-relay                         # should return at once, not after 10 seconds
docker run --rm pm-relay --help              # shows the options
```

Put a reverse proxy with TLS in front of port 8787 (Caddy or nginx), and tell the relay which address is the
proxy, or every client will share one rate-limit budget:

```sh
docker run -d --name pm-relay --restart unless-stopped -p 127.0.0.1:8787:8787 -v pm-relay-data:/data \
  pm-relay --trusted-proxy 172.16.0.0/12
```

Back up the `pm-relay-data` volume if you want the 7-day mailbox to survive a lost disk (the app copes
with a relay that lost its data: it reads the mailbox again). The image has not been built yet on
Docker in this project; see "Not verified" below.

## 6. Make the app use it by default

Today the app has **no default relay**. The "Relay address" box in Settings, Team, starts empty
(`CreateForm` in `packages/app/src/components/settings/blocks/TeamSettings.tsx`), and the saved
team setting `relay` (`packages/app/src-tauri/src/team/config.rs`) defaults to empty. People who join
by invite link do not type anything: the link carries the address.

To make the hosted relay the default, the change is small and should be made only after the name is final:

1. Add one constant in `TeamSettings.tsx`, for example `DEFAULT_RELAY_URL = "https://<final name>"`
   (empty string until the name exists).
2. Start the "Relay address" box with that constant instead of `''`, so a new team uses it unless the
   person types another address.
3. Change the help text next to it ("A hosted relay from Puppet Master is coming").

No change is needed in the Rust code. No live address was added to the app.

## 7. Limits and protection

The relay already limits itself (per computer, per team, per address). The numbers are in
`relay/worker/README.md`. Cloudflare sets the real client address, so these limits count each client.

Add these in the Cloudflare dashboard (Security, WAF, Rate limiting rules). Check the current free-plan
allowance for rules on Cloudflare's pages:

1. **All of `/v1/`**: at most 100 requests in 10 seconds per IP address, then block for 10 minutes.
2. **`POST /v1/teams`** (registration): at most 5 in 10 minutes per IP address. The relay already allows
   30 an hour; this is a cheaper first wall.
3. Turn on a **billing notification** (Billing, Notifications) for usage on the paid plan.
4. Do **not** turn on "Bot Fight Mode" or a browser check for this name: the app is not a browser and
   would be blocked. Test after any security change with the check in section 5.
5. Leave "Always Use HTTPS" on, and set the minimum TLS version to 1.2.

Not covered by the relay itself: a flood of badly signed requests costs the Worker a small amount of
processing each (about 0.2 ms), and each request counts toward the daily request limit. The rules above
are the answer to that.

## 8. Licence (open decision)

- `relay/protocol`: **MIT**. Decided. Anyone can write a client or a relay that speaks the protocol.
- `relay/server` and `relay/worker`: **licence pending**. Until you decide, "all rights reserved": nobody
  may legally use or copy them, and that includes the "Deploy to Cloudflare" button in
  `relay/worker/README.md`, which copies the code into other people's accounts.
- **Recommended: AGPL-3.0.** Anyone can host their own relay, as promised. Anyone who runs a changed
  version as a public service must publish their changes, so a competitor cannot quietly fork a hosted
  relay. The alternative is proprietary (all rights reserved, binary only): simplest to protect, but
  "host it yourself" then means "run our binary", and the open-source promise is weaker.
- When decided: put the licence text in `relay/LICENSE` and `relay/worker/LICENSE`, set `license` in
  `relay/server/Cargo.toml` and `relay/worker/package.json`, and update the notice in both READMEs.
  Ask a lawyer before you charge for the hosted relay.

## 9. Decisions you must make

- [ ] Licence for `relay/server` and `relay/worker`: AGPL-3.0 (recommended) or proprietary.
- [ ] Which Cloudflare account owns it (company account, company email, two-factor login on).
- [ ] Plan: Workers Paid, 5 USD a month (recommended) or Free for the first tests only.
- [ ] The final name, for example `relay.<your-domain>`, and who owns that domain. It cannot change later
      without breaking invites.
- [ ] Whether the hosted relay is free for everyone, free up to a team size, or paid. The relay has no
      accounts or billing; any paid tier needs a separate system in front of it.
- [ ] How many teams you plan for in the first year (the table in section 2 gives the cost).
- [ ] Should the app use the hosted relay by default (section 6), or ask people to choose?
- [ ] Privacy text: the relay sees team ids, device public keys, message sizes and times, and the client
      IP address while a request is being handled (Cloudflare itself keeps access logs). It never sees
      names, branches, notes or keys. Approve the wording for the website and the app.
- [ ] Who gets the alert when it is down or the bill jumps, and who can deploy.
- [ ] Data location: Cloudflare chooses where a team's object lives (near the first user). If customers
      need a region (for example the EU), check Cloudflare's jurisdiction options before promising it.
- [ ] Docker image: publish a ready-made image (needs a registry account) or only the Dockerfile?

## 10. Not verified

Everything below was done on a development computer with Cloudflare's local runtime (`wrangler dev`,
which runs the same `workerd` engine). The following can only be seen on the real service:

- Cloudflare's own free-plan limits, and the real bill. The row counts are exact for the local
  runtime; whether Cloudflare counts index writes the same way is not stated on its pages.
- How long a sleeping object stays in memory (about 10 seconds is documented; the local runtime
  also put it to sleep within 14 seconds).
- The idle time after which Cloudflare closes a silent WebSocket (it is not published). The app now
  sends "ping" every 45 seconds, and the relay answers without waking the object.
- WebSocket behaviour through company proxies and mobile networks.
- Custom domain setup, certificates, WAF rules.
- Request CPU time on the free plan (10 ms limit): measured here at about 0.2 ms per signature check.
- The Docker image: **never built** (Docker was not running). The healthcheck command was run against the
  release binary and works, but the build, the non-root user and the volume rights are untested.
