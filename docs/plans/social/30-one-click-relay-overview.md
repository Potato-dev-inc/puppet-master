# 30 — One-click relay: overview

Status: plan only, no code changed. Written 2026-11-04.

Sources the writer used: `docs/plans/social/00-FORMAT.md`, `docs/plans/social/_facts/relay-facts.md` (cited below as
"relay facts §N"), `docs/plans/social/_facts/spaces-facts.md` (cited as "spaces facts §N"). The writer was told not to
open source code. Every claim about existing code cites a facts section. Anything the facts do not cover is a
`NEEDS-FACT` line in `Open facts` at the end of the plan that needs it. Builders still follow 00-FORMAT rule 4: they
read the files in each slice's `Read first` before they touch anything.

Plan family:

| File | What it covers |
|---|---|
| `30-one-click-relay-overview.md` (this file) | What the user sees, the option comparison and the pick, open decisions, the slice index |
| `31-relay-server-hardening-and-config.md` | The relay server: proof that the Dockerfile builds, settings from environment variables, version and ready endpoints, owner key, storage limit, logs without secrets, WebSocket probe, iroh relay decision |
| `32-relay-deploy-flows.md` | Railway (default) and Cloudflare (fallback) deploys: API calls, image publishing, template files, how the app learns the address and checks it |
| `33-relay-app-ui-and-settings.md` | Settings section "Your relay": states, every string, components, Tauri commands, Labs flag |
| `34-relay-health-and-recovery.md` | Health checks, the "isn't answering" message with a Fix button, restart, backup, update, remove, moving a team to a new relay |
| `35-relay-security.md` | Threat model, token handling, invite-link attackers, operator metadata, the four open relay review items (M1, M3, M4, M5), tests for each defence |

---

## 1. The problem in one paragraph

Team Heads-up (and later Spaces) needs a relay: a small server that passes sealed messages between computers. The app
ships with no relay (relay facts §1a: "app has no default relay"; the Relay address box starts empty). A non-developer
has no idea how to host one. When a relay someone set up goes down, the person sees raw text such as
`The relay said 502` (relay facts §6, `relay_error` "other" arm) or `relay WebSocket unavailable`. The founder wants a
non-developer to make their own relay from the app, and to see a plain message with a Fix button when it breaks.

## 2. What the user sees, first click to "Your relay is ready"

This is the default path (Railway). Each line is one screen state. All strings are final; file 33 repeats them as the
string table the code uses.

1. **Settings › Labs.** The person turns on **Your relay** (Labs key `relay_host_enabled`, file 33 slice R33-1).
   A new row **Your relay** appears right under **Team**.
2. **Your relay, idle.** Text:
   "Your team needs a relay so your computers can reach each other. Make your own in about 3 minutes. It runs on your
   own Railway account, and Railway charges you about 5 US dollars a month. Your messages are sealed: the relay can't
   read them."
   Buttons: **Make my relay with Railway** (primary). Link under it: **Use Cloudflare instead (free, with daily limits)**.
   Second link: **I already have a relay** (it says "Type its address in Team above.").
3. **Connect Railway sheet** (opens on the primary button). Title "Connect Railway". Three numbered lines:
   1. "Open Railway and sign in, or make a free account." Button **Open Railway** (opens
      `https://railway.com/account/tokens` in the system browser).
   2. "Make a new token. Name it Puppet Master. Copy it."
   3. "Paste the token here." Field (hidden characters), label "Railway token", placeholder "Paste token".
   Footer text: "The token is kept in this computer's password store, never in a file. Puppet Master uses it only to
   set up and look after your relay." Buttons **Connect** (primary), **Cancel**.
4. **Connected.** The sheet closes. The row says "Railway connected as ren@example.com." Button **Make my relay**.
5. **Making your relay** (one click on **Make my relay**). A list of 7 steps, each with an empty dot, a spinner, or a
   check mark:
   1. "Checking your Railway account"
   2. "Making a project called Puppet Master relay"
   3. "Adding the relay"
   4. "Adding storage for messages"
   5. "Getting a web address"
   6. "Starting the relay (this can take 2 minutes)"
   7. "Checking that it works"
   Under the list: "You can close Settings. We'll keep going." Button **Stop**.
6. **Your relay is ready.** Title "Your relay is ready". Lines: "Address: https://pm-relay-production-1a2b.up.railway.app"
   with **Copy**; "Working · checked just now"; "Version 0.2.0". Button **Use it for my team**.
7. **Use it for my team**, three cases:
   - No team yet: text "Go to Team above, type your team's name and press Create team. Your relay is already filled
     in." The Team form's Relay address field shows the new address (file 33 slice R33-6).
   - The team already uses this relay: text "Your team uses this relay." Button **Invite people**.
   - The team uses another relay or a folder: button **Move my team here** (file 34 slice R34-10).
8. **Invite people.** Shows the invite link from the existing `team_invite` command (spaces facts §3,
   `service::create_invite`) with **Copy link**, and the text "Send this link to the people you want on your team.
   It stops working after 10 minutes. When they open it, you'll see a request here to let them in." (10 minutes is
   `INVITE_TTL_MS`, spaces facts §3 invite.rs.)

Count: 1 Labs switch, 1 button, 1 browser trip with 3 actions on Railway's site, 1 paste, 1 **Connect**,
1 **Make my relay**, 1 **Use it for my team**. The "one click" is **Make my relay**: everything between it and
"Your relay is ready" is automatic, including learning the address and checking it.

The Cloudflare path is the same shape with a different sheet (file 32 §B) and 4 steps instead of 7.

## 3. Options compared

| | A. Railway, through Railway's API (the app does the work) | B. Railway template button | C. Cloudflare Worker, through Cloudflare's API | D. Cloudflare "Deploy" button | E. Docker + tunnel on the person's PC | F. Relay run by Puppet Master (paid) |
|---|---|---|---|---|---|---|
| What the person needs | A Railway account and a card after the trial; paste one token | A Railway account; copy the address back by hand | A Cloudflare account, no card; paste one token | A Cloudflare account **and** a GitHub account; the button forks the repo | Docker Desktop (large install, admin rights), a PC that never sleeps, and for a fixed address a Cloudflare account plus their own domain | Nothing but a payment |
| Cost to the person | About 5 US dollars a month (NEEDS-FACT NF-9, file 32, confirms it before the string ships) | Same | Free plan: stops until 00:00 UTC when a daily limit is hit; paid 5 US dollars a month (relay facts §1a) | Same as C | Free plus electricity | Price not set |
| Cost to Puppet Master | None | None | None | None | None | 5 US dollars a month minimum per HOSTED-RELAY.md plus support and abuse work (relay facts §1a) |
| How the app learns the address | From the API reply (`serviceDomainCreate`) | The person pastes it | From the API (`workers/subdomain`) | The person pastes it | Quick tunnel: printed by cloudflared, **changes every restart** | Known in advance |
| Runs the reference Rust server | Yes (the Docker image) | Yes | No, the Worker port (relay facts §3, "Differences from the server") | No | Yes | Our choice |
| WebSocket | Yes, server pings every 30 s (relay facts §2) | Yes | Yes, no server ping | Yes | Through the tunnel | Yes |
| Can also host an iroh relay for Spaces | Yes, as a second service, HTTPS only (no public UDP) | Same | No | No | Yes | Yes |
| Storage | A Railway volume at `/data` | Same | Durable Object SQLite | Same | A Docker volume | Ours |
| Backups and admin | Yes (file 34) | Yes | No (no admin routes on the Worker in v1) | No | Yes | Ours |
| Main ways it breaks | Card expired or credit used up so Railway stops it; volume owned by root so the non-root image can't write (NEEDS-FACT NF-7, file 32); Railway outage | Same, plus a wrong pasted address | Free daily limit; Worker and server drift apart; workers.dev name not set up | Same as C plus a fork the person must keep updated | PC asleep or off; quick-tunnel address changes and breaks every invite and every teammate's saved address | One outage hits every customer; Puppet Master sees metadata of every team |
| Verdict | **Default** | Not in v1 | **Fallback** | Stays in `relay/worker/README.md` for developers | Developers only, README only, not in the app UI | Later, needs founder decisions |

### The pick

- **Default: A. Railway through its API.** Reasons: it runs the reference server we test (relay facts §2), keeps a real
  disk for SQLite, supports WebSocket and later an iroh relay, and the app gets the address, status, restart and
  delete through one token. The person never touches a terminal.
- **Fallback: C. Cloudflare Worker through its API.** Reasons: free with no card, so it serves the person who will not
  pay Railway. It cannot host an iroh relay and has no backups, and the UI says so.
- **Always available: "I already have a relay".** The existing Relay address field in Team settings stays as it is
  (relay facts §6, `CreateForm`).
- **Not in the app UI: B, D, E.** D stays as the existing README button. E stays in `relay/README.md` (Docker run
  command from relay facts §1a), written for developers.
- **F is a founder decision** (D-10 below). The existing line "A hosted relay from Puppet Master is coming." stays.

## 4. How the pieces fit

```
 Person's computer (Puppet Master app)                         Person's Railway account
 ┌──────────────────────────────────────────────┐              ┌──────────────────────────────────┐
 │ Settings › Your relay  (React, file 33)       │              │ Project "Puppet Master relay"    │
 │   │ invoke relay_host_*                       │   HTTPS      │   service pm-relay                │
 │ relay_host module (Rust, files 32-34)         │── GraphQL ──▶│   image ghcr.io/potato-dev-inc/   │
 │   tokens + owner key in credential store      │              │         pm-relay@sha256:…         │
 │   relay_host.json (ids and address only)      │              │   volume /data (relay.sqlite)     │
 │   checks: /v1/health /v1/version /v1/probe/ws │── HTTPS ────▶│   domain *.up.railway.app         │
 │           /v1/admin/status (owner-signed)     │              └──────────────────────────────────┘
 │ team transport (existing RelayTransport)      │── HTTPS/WSS ▶ same relay: /v1/teams/…
 └──────────────────────────────────────────────┘
 Teammates' computers ── invite link carries the relay address ──▶ same relay
```

Key design choices (each is detailed in the file named):

1. **No admin token.** The relay gets an *owner public key* in the variable `PM_RELAY_OWNER_KEY`. The app signs admin
   requests with the matching private key, using the same signed-request scheme the relay already uses for devices
   (relay facts §2 "Auth and tokens"). Nothing secret is stored on Railway. File 31 §2.
2. **Private by default.** A one-click relay accepts new teams only when the owner signs the registration
   (`PM_RELAY_REGISTRATION=owner`). Strangers who find the address cannot fill it with their teams. Invite joins and
   members' signed requests work as today. File 31 slice R31-5.
3. **Zero-config container.** The image reads `PM_RELAY_*` environment variables, so Railway needs no start command.
   File 31 slice R31-2.
4. **Prebuilt image, pinned by digest.** The app tells Railway to run `ghcr.io/potato-dev-inc/pm-relay:<version>@sha256:<digest>`.
   Railway never builds Rust. File 32 slice R32-1.
5. **Tokens only in the credential store.** New team KeyStore names `relay_railway_token`,
   `relay_railway_project_token`, `relay_cloudflare_token`, plus the key `relay_owner`. After setup the app swaps the
   account-wide Railway token for a token that only reaches this one project. File 32 slice R32-2, file 35.
6. **"Ready" means proven.** The app says ready only after `/v1/health` returns exactly
   `{"ok":true,"protocol":"pm-relay/1"}`, `/v1/version` shows our owner key, a WebSocket probe frame arrives through
   Railway's proxy, and an owner-signed `/v1/admin/status` call succeeds. File 32 §C.

## 5. Decisions still open (each has a default the builders use)

- **DECISION-OPEN D-1 (default: Railway is the default provider, Cloudflare is the fallback).** The alternative is
  Cloudflare first because it is free. The UI order and the button text follow this default.
- **DECISION-OPEN D-2 (default: yes, swap the account token for a project token after setup).** Less power left on the
  computer. Cost: removing the relay may need the person to delete it on Railway's site if a project token cannot
  delete projects (NEEDS-FACT NF-9, file 32). File 34 slice R34-7 handles both outcomes.
- **DECISION-OPEN D-3 (default: publish the image to GHCR as a public package, from a manual `workflow_dispatch`
  workflow the founder starts).** This is a release-like step, so the founder says yes before each publish.
- **DECISION-OPEN D-4 (default: one-click relays take new teams only from their owner).**
- **DECISION-OPEN D-5 (default: Live Room registrations stay open on a one-click relay, with the existing limits of
  10 rooms per address per hour and 100 open rooms).** Owner-only rooms would need the Live Room client to sign with
  the owner key; NEEDS-FACT NF-26, file 31.
- **DECISION-OPEN D-6 (default: no iroh relay on one-click relays in v1; Spaces uses public iroh relays in Labs, as in
  Spaces decision 8).** File 31 §5 explains why and plans a later slice R31-9.
- **DECISION-OPEN D-7 (default: the Spaces sealed mailbox is off, as in Spaces decision 9).** When it is turned on, a
  space uses the relay's existing team mailbox routes (a space registers like a team), so the relay needs no new
  code. File 31 §6.
- **DECISION-OPEN D-8 (default: automatic backup once a week, keep the newest 4 files).** File 34 slice R34-5.
- **DECISION-OPEN D-9 (default: after a team moves to a new relay, the old relay stays up 7 days, then the app asks to
  remove it).** 7 days matches the mailbox time-to-live `MAILBOX_TTL_MS` (relay facts §2). File 34 slice R34-10.
- **DECISION-OPEN D-10 (default: no Puppet Master-run relay in v1).** The existing text "A hosted relay from Puppet
  Master is coming." stays.
- **DECISION-OPEN D-11 (default: no public Railway template in v1).** The API flow replaces it.
- **DECISION-OPEN D-12 (default: no "run on this PC" option in the app UI).** Docker stays in the README.
- **DECISION-OPEN D-13 (default: when a Cloudflare token sees several accounts, use the first one and show its name
  with the line "Using your Cloudflare account Ren's Account.").**
- **DECISION-OPEN D-14 (default: the image is published under the licence in `relay/LICENSE` as it is on the day of
  publishing).** HOSTED-RELAY.md section 8 says the server licence is still pending (relay facts §1).
- **DECISION-OPEN D-15 (default: a member's app follows a `relay_moved` team event only when an admin sent it).**
  File 34 slice R34-11.

- **DECISION-OPEN D-16 (default: the app does not clear the clipboard after a token is pasted).** File 35 §3.

## 6. Slice index

Order rule: a slice never needs something a later slice makes. "Deps" lists the slices that must be merged first.
Sizes follow 00-FORMAT "Slice sizing guide". Keep at most 3 builders running at once (founder's usage rule).

### Wave 1: the relay server (all in `relay/`, no app changes)

| Id | Slice | File | Size | Deps |
|---|---|---|---|---|
| R31-1 | Prove the Dockerfile builds and runs | 31 | Haiku | none |
| R31-2 | Settings from environment variables | 31 | Haiku | none |
| R31-3 | `/v1/version` and `/v1/ready` | 31 | Haiku | none |
| R31-4 | Access log without secrets | 31 | Haiku | none |
| R31-5 | Owner key, owner-only registration, `/v1/admin/status` | 31 | Sonnet | R31-2, R31-3 |
| R31-5b | Owner flags and variables | 31 | Haiku | R31-5 |
| R31-6 | Storage limit and data-folder check | 31 | Haiku | R31-5b |
| R31-7 | WebSocket probe `/v1/probe/ws` | 31 | Haiku | R31-3 |
| R31-8 | Dockerfile reads env settings; version bump | 31 | Haiku | R31-1, R31-2, R31-3, R31-5, R31-5b, R31-6, R31-7 |
| R35-1 | M1: mailbox counter never goes back | 35 | Sonnet | none |
| R35-2 | M3: slow down failed signatures before replay memory | 35 | Sonnet | R31-2 |
| R35-3 | M4: WebSocket wake coalescing and client frame limit | 35 | Sonnet | none |
| R35-4 | M5: proxy addresses, invite counters that expire | 35 | Sonnet | R31-2, R32-5 |
| R34-4 | Owner-only backup endpoint (server) | 34 | Sonnet | R31-5 |
| R34-8 | Owner-only restore endpoint (server) | 34 | Sonnet | R34-4, R35-1 |

### Wave 2: image and the app's deploy engine

| Id | Slice | File | Size | Deps |
|---|---|---|---|---|
| R32-1 | Publish the image to GHCR (founder starts the workflow) | 32 | Haiku + founder | R31-8 |
| R32-2 | `relay_host` module: `relay_host.json`, KeyStore names | 32 | Haiku | none |
| R32-3 | Railway API client | 32 | Sonnet | R32-2 |
| R32-4 | Relay checks: health, version, WebSocket probe, owner proof | 32 | Sonnet | R32-2, R31-5, R31-7 |
| R32-5 | Railway deploy runner (resumable steps) | 32 | Sonnet | R32-1, R32-3, R32-4 |
| R32-6 | App signs team registration with the owner key | 32 | Haiku | R31-5, R32-2 |

### Wave 3: the Settings UI

| Id | Slice | File | Size | Deps |
|---|---|---|---|---|
| R33-1 | Labs flag `relay_host_enabled` + `DEV_LABS_KEYS` | 33 | Haiku | none |
| R33-2 | `RelayHost` service, Tauri commands, `relay://changed` | 33 | Sonnet | R32-5, R33-1 |
| R33-3 | `lib/relay-host.ts` client and `useRelayHost` hook | 33 | Haiku | R33-2 |
| R33-4 | Idle state and Connect sheet | 33 | Haiku | R33-3 |
| R33-5 | Deploying, ready, unhealthy, failed, removed states | 33 | Haiku | R33-4 |
| R33-6 | Settings row, Team form hand-off, invite link | 33 | Haiku | R33-5, R32-6 |

### Wave 4: health and recovery

| Id | Slice | File | Size | Deps |
|---|---|---|---|---|
| R34-1 | Plain words for 500/502/503/504 and blocked WebSocket | 34 | Haiku | none |
| R34-2 | Health monitor and schedule | 34 | Sonnet | R33-2 |
| R34-3 | Fix and restart (Rust) | 34 | Sonnet | R34-1, R34-2 |
| R34-5 | Backups in the app (button + weekly) | 34 | Haiku | R34-4, R34-2 |
| R34-6 | Update the relay, with roll back | 34 | Sonnet | R34-5 |
| R34-7 | Remove the relay | 34 | Haiku | R34-5 |
| R34-9 | Team side: `relay_moved` event, putting a team on a new relay | 34 | Sonnet | R32-6 |
| R34-10 | Move my team here (relay maker side) | 34 | Sonnet | R34-8, R34-9, R33-6 |
| R34-11 | Members follow a moved team; paste a new address | 34 | Sonnet | R34-9 |
| R34-12 | Recovery buttons in the Your relay row (UI) | 34 | Haiku | R34-3, R34-5, R34-6, R34-7, R34-10 |
| R34-13 | Relay-down notice in Team settings (UI) | 34 | Haiku | R34-1, R34-11, R34-12 |

### Wave 5: fallback provider and the last security pieces

| Id | Slice | File | Size | Deps |
|---|---|---|---|---|
| R32-7 | Worker: `/v1/version`, `/v1/probe/ws`, owner-only registration | 32 | Sonnet | R31-5 |
| R32-8 | Worker bundle for upload by the app | 32 | Haiku | R32-7 |
| R32-9 | Cloudflare API client and deploy runner | 32 | Sonnet | R32-8, R32-4, R33-2 |
| R35-5 | Tests: tokens never leave the credential store | 35 | Haiku | R33-2 |
| R35-6 | Guard tests for `relay_host` | 35 | Haiku | R33-2 |
| R35-7 | New owner key (rotation), Rust | 35 | Sonnet | R32-5, R33-5 |
| R35-8 | New owner key button (UI) | 35 | Haiku | R35-7, R34-12 |

### Later, blocked

| Id | Slice | File | Size | Deps |
|---|---|---|---|---|
| R31-9 | iroh relay as a second Railway service (spike) | 31 | Sonnet | Spaces D-0, NEEDS-FACT NF-21, file 31 |

Total: 46 slices (45 now, 1 blocked). Haiku: 25 (R32-1 also needs the founder). Sonnet: 21.
Per file: 31 has 10, 32 has 9, 33 has 6, 34 has 13, 35 has 8.

NEEDS-FACT numbering is shared across files 30 to 35 (`NF-1` to `NF-39`); each plan lists the ones it needs in its own
`Open facts` section.

Ship gate: the Labs flag may be on in dev builds as soon as Wave 3 merges. It must not be promoted out of Labs until
R35-1, R35-2, R35-3, R35-4, R35-5 and R35-6 are merged. This matches the cowork review's own order: M1 first, and
"M3, M4, M5 before any hosted relay goes public" (spaces facts §8).

## Open facts

None for this file. The NEEDS-FACT lines live in the plan that needs them: files 31, 32, 33, 34 and 35.

## Security

Summary only; file 35 is the full threat model with a test for each defence.

| Threat | What an attacker or a broken peer can do | Defence | Test that proves it (slice) |
|---|---|---|---|
| Railway or Cloudflare token copied from this computer | Run services on the person's bill, delete the relay | Token only in the OS credential store; swapped for a project-only token after setup; never in files, logs, events or command replies | `token_never_leaves_the_credential_store` (R35-5), `deploy_happy_path_makes_everything_in_order` checks the swap (R32-5) |
| A stranger finds the relay address | Register their own teams and use the person's storage and bill | Owner-only registration | `owner_relay_refuses_unsigned_team_registration` (R31-5) |
| Someone gets an invite link | Send join requests | Existing: join is sealed, rate-limited (5 per invite, 10 per address per hour), and the owner must approve | `joins_need_no_owner_signature_on_owner_relay` (R31-5) keeps joins working; existing limits are tested in `server/tests/protocol.rs` |
| The relay operator (the person, Railway, Cloudflare) reads the disk | Sees team ids, device key ids, sizes, timing; never content | Unchanged end-to-end sealing (spaces facts §3 crypto.rs, secure.rs) | Existing team crypto tests (spaces facts §3); R34-4 `backup_holds_the_registered_team` shows what a backup holds |
| A fake relay at the saved address | Collect metadata, drop messages | Owner-signed `/v1/admin/status` check before "ready" and on every deep check | `judge_owner_mismatch_is_wrong_relay` (R32-4) |
| A fake "team moved" event | Send members to an attacker's relay | Only admin-signed events are followed, and the new relay must already hold the team | `relay_moved_from_non_admin_is_ignored` (R34-11) |
| Agents, MCP tools or the phone press Deploy or Remove | Spend money or delete the relay | Commands are not reachable from bridge, MCP or phone | `relay_host_is_not_reachable_from_bridge_mcp_or_phone` (R35-6) |

## What cannot be verified without real hardware or a second computer

- A real Railway deploy. It needs a Railway account with billing and a real token, which only the founder can make.
  Every runner test uses a fake Railway API.
- That Railway's proxy forwards WebSocket upgrades and keeps a socket open past 30 s pings.
- That Railway volumes let the image's user write `/data` (NEEDS-FACT NF-7, file 32).
- A real Cloudflare upload with Durable Object migrations, and the free-plan daily stop.
- Two computers in one team through a one-click relay: create, invite, join, approve, Heads-up card arrives.
- Moving a team while one member is offline, then that member coming back within 7 days and after 7 days.
- The credential store on macOS and Linux (only Windows Credential Manager is on this PC).
- The real price lines in the UI (NEEDS-FACT NF-9, file 32).
