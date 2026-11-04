# Spaces: overview (D-0 .. D-9)

Status: plan only. No code is changed by this document.

Sources: the writer read only `docs/plans/social/00-FORMAT.md` and `docs/plans/social/_facts/spaces-facts.md`
(founder instruction: no source reading). Every claim about existing code cites the facts file as `facts §N`.
Anything the facts file does not say is listed as `NEEDS-FACT` in `Open facts` of the plan that needs it. The builder
of each slice reads the files in that slice's `Read first` list before touching anything, and stops if what they see
contradicts the plan.

Plan files:

| File | Covers |
|---|---|
| `20-spaces-overview.md` | this file: architecture, slice index, decisions, glossary |
| `21-spaces-d0-d1-transport-spike-and-secure-core.md` | Labs flag, p2p seams for Spaces, module skeleton, guard tests, iroh spike, `secure_core/`, wire format helpers |
| `22-spaces-d2-identity-invite-roster.md` | word list, identity and device certificates, control log (roster), invite, join, SpacePermissions, join sheets |
| `23-spaces-d3-log-store-sync.md` | SQLite store, message log entry, sync, catch-up, retention |
| `24-spaces-d4-channels-chat-ui.md` | Spaces page, space list, channel view, composer, member list, presence dots |
| `25-spaces-d5-moderation-and-limits.md` | remove + key rotation, block, delete tombstones, reactions/replies, rate limits, cowork M1-M6 |
| `26-spaces-d6-d7-rooms-and-social-mode.md` | `RoomTransport`, `room_message`, rooms modules, Fish bridge, social mode, caps, leak check |
| `27-spaces-d8-d9-files-presence-mailbox.md` | files, presence, mailbox node (off by default) |

---

## 1. What Spaces is

Spaces is small-group chat between people's own computers. No server stores messages. A space is like a small
Discord server: it has a name, up to 50 members and up to 20 channels. Each member's PC keeps a copy of the space
(the last 90 days or 200 MB, whichever comes first). When two members are online at the same time, their PCs swap
what the other one is missing. Fish (the Potato Bot) can sit in a channel, but in a space it only talks: no tools, no
computer, no web, no files, and its own memory per space.

UI name: **Spaces** (decision 20). Labs flag: **`spaces_enabled`** (off by default; joins `DEV_LABS_KEYS`).

## 2. Architecture in words

### 2.1 Layers (bottom to top)

1. **Link layer (exists, `p2p/`, facts §1).** One iroh endpoint per app (`p2p/iroh_net.rs`, the only file that
   touches the network). Capabilities register by ALPN; `admit()` refuses unknown ALPNs, relays on direct-only
   capabilities, and peers without permission. Spaces already has its reserved ALPN `pm/spaces/1` (`alpn::SPACES`)
   and capability id `spaces` (`capability::SPACES`) (facts §1). Spaces adds three small, generic things here
   (slice D0.2): a per-capability Labs key, a `RoutedPermissions` that lets one capability use its own permission
   rule, and a relay hint in `PeerAddr` so a peer behind NAT can be reached through its home relay.
2. **Secure core (new, `src/secure_core/`).** `team/crypto.rs` moves here unchanged (golden tests prove byte-equal
   output before and after). New generic helpers: domain-separated Ed25519 sign/verify and "seal one key to many
   devices" (HPKE). Team keeps working through a one-line re-export.
3. **Spaces core (new, `src/spaces/`).** Pure Rust, no network, no Tauri:
   - `wire.rs`: the byte writer/reader every format uses (big-endian, length-prefixed), plus base32.
   - `ids.rs`: `SpaceId` (16 bytes), `ChannelId` (8 bytes), `EntryId` (32 bytes), `NodeKey` (32 bytes).
   - `words.rs` + `words/en.txt`: the 2048-word list for invite words.
   - `identity.rs`: a person's member key, device key, KEM key, node id, and the per-space `DeviceCert`.
   - `roster.rs`: the **control log**: owner-signed, hash-linked operations (genesis, add/remove member, channels,
     key rotation, Fish settings). Replaying it gives the roster, the channel list and the current epoch.
   - `invite.rs`: invite ticket (link/QR), 8 invite words, the invite rules (24 h, one use, 5 wrong tries).
   - `log.rs`: the **message log** entry: signed by the author's device, body sealed with the space epoch key.
   - `store.rs`: one SQLite file per space.
   - `sync.rs`: the sync protocol as a pure state machine (frames in, frames out).
4. **Spaces net seam (new, `spaces/net.rs`).** `SpaceNet` / `SpaceLink` traits. The real implementation
   `P2pSpaceNet` sits on the app's single `P2p` service using capability `spaces`. `spaces/net.rs` is the ONLY file
   in `spaces/` allowed to name `crate::p2p`. Tests use `spaces/memnet.rs` (in-process, no sockets).
5. **Spaces service (new, `spaces/service.rs`).** Owns open spaces, keeps up to 12 live links per node, runs
   sync, pushes new entries, answers join requests, enforces rate limits, emits `spaces://changed` and
   `spaces://message` to the UI.
6. **Commands + UI.** `spaces/commands.rs` (Tauri commands, each gated by `spaces_enabled`), registered in
   `lib.rs`. Frontend `packages/app/src/lib/spaces.ts`, `packages/app/src/components/spaces/*`,
   `packages/app/src/styles/cockpit-spaces.css`. Sidebar row "Spaces" like the "My computers" row (facts §11).
7. **Fish in spaces.** `spaces/fish_bridge.rs` (app side) hands channel messages to Potato Brain as
   `room_message` with room id `space:ch:<space>:<channel>`, and takes Fish's text back. Potato Brain gets
   `src/rooms/` (shared with Potato Bot groups) and a `social` tools mode. Hard caps and a leak check run on the app
   side as well, so a broken brain cannot exceed them.

### 2.2 One message, end to end

1. Sam types "hi" in `#general` and presses Enter. `space_send` command.
2. Service: rate limit check (10 per 10 s, 600 per hour per device). Build entry: author = Sam's device key,
   `seq` = Sam's last seq + 1, `lamport` = max seen + 1, body sealed with the current epoch key (XChaCha20-Poly1305,
   `secure_core::crypto::seal_with_key`), header bytes are the AEAD associated data, Ed25519 signature over all of it.
3. Store it (SQLite `entries` + decrypted row in `messages`), emit `spaces://message`.
4. Send `Push{entry}` on every live link of that space. Each receiver checks signature, author is a current member,
   epoch key exists, rate limit; stores it; forwards it once to its other links (dedupe by entry id).
5. A member who was offline connects later: `Hello` frames swap "lowest and highest seq per author"; each side asks
   `Want` for the gaps; any member can serve any entry, because entries are sealed and signed (nobody can forge or
   read them without the epoch key).

### 2.3 Joining, end to end

1. Owner Ren clicks **Invite people**. The PC makes an invite: id (8 random bytes), secret (88 bits shown as 8
   words), expiry now + 24 h. The secret goes into the OS keystore; the invite row (no secret) into SQLite.
2. Ren shares the **invite link** (text) and says the **8 words** another way, or shows the **QR** (link + words).
3. Sam pastes the link, types the 8 words. Sam's PC dials Ren's node id (via Ren's relay or LAN addresses from the
   link) on `pm/spaces/1`. `SpacePermissions` lets a stranger in only while Ren has an open invite.
4. Sam sends `JoinRequest{space, invite id, DeviceCert, proof}` where `proof = HMAC(secret, transcript bound to
   Sam's node id)`. Ren's PC checks it. Wrong: strike (5 strikes kill the invite). Right: the invite is used
   (one use) and Ren sees **"Sam wants to join Book club"** with **Let in** and **No**.
5. **Let in**: Ren's PC appends `AddMember{cert, epoch key sealed to Sam's device}` to the control log and sends
   `JoinWelcome{control log}`. Sam verifies the whole log from genesis and opens the key. Sam is in.

### 2.4 Where data lives

| Data | Place |
|---|---|
| Member key, device key, KEM key, node key | OS keystore through `team::keystore::KeyStore` (facts §3): `member()`, `device_sign()`, `device_kem()`, `p2p_node()` |
| Space epoch keys | `KeyStore::set_epoch_key("space:<space_hex>", epoch, key)` |
| Invite secrets | `KeyStore::set_invite_code("space:<space_hex>", "<invite_hex>", "<secret_hex>")` |
| Everything else of a space | `app_paths::app_data_dir()/spaces/<space_hex>/space.sqlite` |
| File cache | `app_paths::app_data_dir()/spaces/<space_hex>/files/<sha256_hex>` |
| Fish memory per space | `vault/spaces/<space_hex>/` (Potato Brain vault, D7.2) |

### 2.5 What never happens

- Space traffic never goes through the bridge (`src/bridge`), MCP, the phone route, device pairing (`PairManager`,
  `TrustList`) or the team relay. Guard tests enforce it (D0.3).
- No file in `spaces/` except `net.rs` (and the test-only `spike.rs`, `net_tests.rs`) names `crate::p2p`.
- No file in `p2p/` except `iroh_net.rs` touches the network (existing guard, facts §2).
- A space member never gets any desk permission: `RoutedPermissions` answers only for capability `spaces`.
- Fish in a space never runs a tool.

---

## 3. Slice index

Model size: **H** = Haiku-sized, **S** = Sonnet-sized (00-FORMAT sizing guide).

| Slice | Goal (short) | Size | Needs merged first | Plan file |
|---|---|---|---|---|
| D0.1 | Labs flag `spaces_enabled` (Rust + TS lists, label, settings toggle, gate hook) | H | none | 21 |
| D0.2 | p2p seams: per-capability Labs key, `RoutedPermissions`, relay hint in `PeerAddr` | S | D0.1 | 21 |
| D0.3 | `spaces/` module skeleton, `limits.rs`, `spaces/guard_tests.rs`, p2p guard allow-list | H | D0.2 | 21 |
| D0.4 | iroh spike: two PCs talk direct and through a relay, measured (ignored test, feature `spaces-spike`) | S | D0.3, D1.3 | 21 |
| D1.1 | `secure_core/crypto.rs` move with golden tests, team re-export | S | none | 21 |
| D1.2 | `secure_core/sign.rs` + `secure_core/grant.rs` | S | D1.1 | 21 |
| D1.3 | `spaces/wire.rs` (writer/reader/base32) + `spaces/ids.rs` | H | D0.3, D1.1 | 21 |
| D2.1 | Word list file + `spaces/words.rs` | H | D1.3 | 22 |
| D2.2 | `spaces/identity.rs`: `SpaceIdentity`, `DeviceCert` | H | D1.2, D1.3 | 22 |
| D2.3 | `spaces/roster.rs`: control log + replay rules | S | D2.2 | 22 |
| D2.4 | `spaces/invite.rs`: ticket, words secret, proof, invite rules | H | D2.1, D2.3 | 22 |
| D3.1 | `spaces/store.rs`: open, full schema v1, meta, control ops, invites, join requests | S | D2.4 | 23 |
| D2.5 | `spaces/net.rs` + `spaces/memnet.rs` + `SpacePermissions` | S | D0.2, D2.3 | 22 |
| D2.6 | `spaces/join.rs`: join protocol both sides | S | D2.5, D3.1 | 22 |
| D3.2 | `spaces/log.rs`: entry layout, seal, sign, open | S | D2.3 | 23 |
| D3.3 | store: ingest entries, decrypted messages, reads | S | D3.1, D3.2 | 23 |
| D3.4 | `spaces/sync.rs`: frames + pure sync state machine | S | D3.3 | 23 |
| D3.5 | `spaces/service.rs`: links, push, catch-up, joins wired | S | D2.6, D3.4 | 23 |
| D3.6 | Retention: 90 days or 200 MB | H | D3.5 | 23 |
| D2.7 | Commands + `lib/spaces.ts` for create, invite, join, answer | H | D3.5 | 22 |
| D2.8 | `InviteSheet` + `JoinRequestSheet` (+ first `cockpit-spaces.css`) | H | D2.7 | 22 |
| D2.9 | `JoinSheet` | H | D2.8 | 22 |
| D2.R | Sonnet review of D2.1-D2.9 and D3.1 (read-only checklist) | S | D2.9 | 22 |
| D4.1 | Commands for detail, messages, send, read, channels + TS wrappers and hooks | H | D3.5, D2.7 | 24 |
| D4.2 | Sidebar row, `SpacesPage`, `SpaceList`, sheets mounted | H | D4.1, D2.9 | 24 |
| D4.3 | `ChannelView`, `MessageRow`, `Composer` | H | D4.2 | 24 |
| D4.4 | `ChannelList`, `NewChannelSheet`, feed mounted | H | D4.3 | 24 |
| D4.5 | `MemberList` + presence dots, `NewSpaceSheet`, Space info, full CSS | H | D4.4 | 24 |
| D5.1 | Remove member + key rotation; leave; delete my copy | S | D4.5 | 25 |
| D5.2 | Block (local only) + TS wrappers for D-5 | H | D5.1 | 25 |
| D5.3 | React and delete (backend) | H | D5.2 | 25 |
| D5.4 | Message and member menus, leave/removed UI | H | D5.3 | 25 |
| D5.5 | Rate limits, link meter, cowork M1-M6 checks | H | D5.4 | 25 |
| D6.1 | Potato Brain `rooms/`: types, ids, `RoomTransport` | H | none (TS only) | 26 |
| D6.2 | Potato Brain `rooms/`: gate, caps, fence | H | D6.1 | 26 |
| D6.3 | `spaces/fish_bridge.rs` app side (owner-only, caps persisted, FishText) | S | D5.5, D6.2 | 26 |
| D6.4 | Potato Brain space transport wired to the app | S | D6.3 | 26 |
| D7.1 | Social mode: tools value `social` + tests that prove no tool runs | S | D6.4 | 26 |
| D7.2 | Per-space memory + caps survive restarts | H | D7.1 | 26 |
| D7.3 | Leak check + Fish mode/mute commands | H | D7.2 | 26 |
| D7.4 | Fish settings in the channel header | H | D7.3 | 26 |
| D8.1 | Files: offer entry + chunk transfer (backend) | S | D5.5 | 27 |
| D8.2 | Files UI | H | D8.1 | 27 |
| D8.3 | Presence (backend) | H | D5.5 | 27 |
| D8.4 | Presence reporting from the UI + "Show when I'm online" | H | D8.3 | 27 |
| D9.1 | Mailbox node, client side (only if decision 9 becomes yes) | H | D8.4 | 27 |
| D9.2 | Mailbox UI (only if decision 9 becomes yes) | H | D9.1 | 27 |

**Merge order** (one line, left to right): D0.1, D0.2, D0.3, D1.1, D1.2, D1.3, D0.4, D2.1, D2.2, D2.3, D2.4, D3.1,
D2.5, D2.6, D3.2, D3.3, D3.4, D3.5, D3.6, D2.7, D2.8, D2.9, D2.R, D4.1, D4.2, D4.3, D4.4, D4.5, D5.1, D5.2, D5.3,
D5.4, D5.5, D6.1, D6.2, D6.3, D6.4, D7.1, D7.2, D7.3, D7.4, D8.1, D8.2, D8.3, D8.4, D9.1, D9.2.

D6.1 and D6.2 touch only Potato Brain and can be built in parallel with any D-2..D-5 slice. D0.4 needs two real
computers; it runs any time after D1.3 and blocks nothing after it.

Slice count: **47** (46 build slices + 1 review slice D2.R). D9.1 and D9.2 are built only if decision 9 changes.

---

## 4. The 20 decisions (from the R17 plan, facts §7.4) with defaults

All are `DECISION-OPEN` until the founder confirms; the plans build the default.

| # | Decision | Default the plans build | Used in |
|---|---|---|---|
| 1 | Model Bridge provider adapters | Ship the generic bridge; a provider's TOML only after its terms say yes | not Spaces |
| 2 | Write into Codex's settings file | Yes, with backup and Undo, only on click | not Spaces |
| 3 | Model Bridge faces | Loopback only, forever | not Spaces |
| 4 | Hosted catalog for personas | No in v1 | not Spaces (no public directory, T-20 not built) |
| 5 | Share the default Fish persona | No until the Fish licence check passes | D-7 (Fish persona is never sent to members) |
| 6 | Project Map spend | Cheapest installed agent, estimate first, cap $0.50 | not Spaces |
| 7 | Project Map cache place | App data; "Save to repo" button | not Spaces |
| 8 | Spaces relays | **Public iroh relays in Labs**; our own relay later for self-hosting and the paid plan | D0.2, D0.4 |
| 9 | Offline mailbox | **Off**; members catch up from each other | D9.1 is built behind a second switch, default off |
| 10 | Fish social mode without the sandbox | Yes: no tools, no computer, no helpers, per-space memory, fenced input, daily caps | D-7 |
| 11 | Web search in social mode | No | D7.1 |
| 12 | Owner's own lines in a space also social mode | Yes; full Fish only in the app DM | D7.1 |
| 13 | History per space | **90 days or 200 MB**, whichever first | D3.6 |
| 14 | Space size v1 | **50 members, 20 channels** | D2.3, D5.4 |
| 15 | Fish ambient talk | "Only when called"; "When it helps" is an owner choice | D6.2, D7.3 |
| 16 | Hand a task to a teammate's agent (T-3) | Always needs Accept; runs in a worktree | not built here |
| 17 | Team spending sharing (T-9) | Opt-in, daily totals only | not built here |
| 18 | Voice rooms (T-10) | Later (R25), P2P, 6 people | not built here |
| 19 | Phone push for mentions (T-18) | Yes, no message text in the push | not built here |
| 20 | UI name | **"Spaces"** | everywhere |

### 4.1 Plan decisions made in these plans (founder can overrule each)

| Id | Decision | Why | Conflict with facts? |
|---|---|---|---|
| P-1 | Invite lives **24 h**, **one use**, **5 wrong word attempts kill it**. Max 3 open invites per space. | Founder instruction | Yes: `team/invite.rs` uses 10 min / 5 attempts (facts §3, §"Could not find" 6). Spaces gets its own constants in `spaces/limits.rs`; `team/invite.rs` is not changed. |
| P-2 | Invite = **link** (text) + **8 words** (said or sent another way). The **QR** holds both. "Copy link" copies the link only. | A link seen by the wrong person is not enough to join. 8 words carry 88 bits, not enough for an address, so the link carries the address. | Old plan said "8-word code **or** QR"; words alone cannot locate the owner without a lookup server. |
| P-3 | One "used" invite = the first request with the right words, even if the owner then says **No**. | Simple and safe. | none |
| P-4 | A joiner node with 3 failed join tries is locked out for 10 minutes (copied from pairing `MAX_STRIKES = 3`, `LOCKOUT_MS = 600_000`, facts §1). | Reuse a known rule. | none |
| P-5 | **No new crates.** No `iroh-gossip`, no `iroh-blobs`. Sync and file transfer are our own small protocol on `pm/spaces/1` streams. | Gossip/blobs would open sockets outside `iroh_net.rs` and their compatibility with `iroh = "=1.4.1"` is unknown. | Yes: old plan named gossip + blobs (facts §7.1, "Could not find" 7). |
| P-6 | Spaces has its **own control log** (`spaces/roster.rs`) instead of wrapping `team::roster`. | Team `OpBody`/`Role` fields are not in the facts and team roles do not match Owner/Member + channels. Same idea (signed, hash-linked), new format. | Old plan said "wraps team::roster". |
| P-7 | Only `team/crypto.rs` moves to `secure_core/`. `team/secure.rs` stays (its grant types are tied to the team `Roster`); Spaces gets a generic grant in `secure_core/grant.rs`. | Smaller, safer move. | Old plan said "move crypto.rs + secure.rs helpers". |
| P-8 | No `spaces/net/{mod,iroh,memory}`; one `spaces/net.rs` on top of the existing `P2p` service. | Founder: `iroh_net.rs` is the only file that touches the network. | Old plan listed a `net/iroh.rs`. |
| P-9 | Person identity in a space = `KeyStore::member()` key (same key in all spaces and teams of that person). | Reuse; T-23 multi-device later. Cost: two spaces with a shared member can link that person. Stated in Security. | none |
| P-10 | Roles v1: **Owner** and **Member**. Only the owner signs control ops (invite, remove, channels, Fish). Members can leave. Ownership cannot be handed over in v1. | Facts §7.1 roles v1. | none |
| P-11 | Fish v1 in a space = the **owner's** Fish only, entries authored by the owner's device with kind `FishText`. | Simple trust story: "Fish (Ren's)". | none |
| P-12 | Live topology: each node keeps at most **12** links per space set; new entries are pushed and flooded once with dedupe. | 50 members max. | none |
| P-13 | Decrypted message text is kept in the space SQLite (for display and later search). Disk encryption is the OS's job. | Stated honestly in Security. | none |
| P-14 | Message text max **4000** characters; entry max **64 KiB**; frame max **1 MiB**; files max **25 MB**. | Bounds for parsers and abuse. | none |
| P-15 | Leaving Labs (Spaces on by default) needs cowork M1-M6 closed. `DECISION-OPEN (default: Spaces stays Labs-only until M1-M6 are closed)`. | Facts §7.2 risk line. | none |

---

## 5. Limits (single source: `packages/app/src-tauri/src/spaces/limits.rs`, created in D0.3)

| Const | Value |
|---|---|
| `MAX_MEMBERS` | 50 |
| `MAX_CHANNELS` | 20 (not archived) |
| `MAX_CHANNELS_EVER` | 100 (including archived) |
| `KEEP_MS` | 90 days = `90 * 24 * 60 * 60 * 1000` |
| `KEEP_BYTES` | 200 MB = `200 * 1024 * 1024` |
| `TRIM_TO_BYTES` | 180 MB = `180 * 1024 * 1024` |
| `INVITE_TTL_MS` | 24 h = `24 * 60 * 60 * 1000` |
| `INVITE_MAX_STRIKES` | 5 |
| `MAX_OPEN_INVITES` | 3 |
| `JOIN_LOCKOUT_STRIKES` | 3 |
| `JOIN_LOCKOUT_MS` | 600 000 |
| `JOIN_WAIT_MS` | 30 minutes |
| `JOINS_PER_MINUTE` | 20 (all strangers together) |
| `NAME_MAX_CHARS` | 40 (space and person names) |
| `CHANNEL_NAME_MAX_CHARS` | 32 |
| `MAX_TEXT_CHARS` | 4000 |
| `MAX_ENTRY_BYTES` | 65 536 |
| `MAX_FRAME_BYTES` | 1 048 576 |
| `MAX_LINKS` | 12 |
| `RATE_BURST` / `RATE_BURST_WINDOW_MS` | 10 per 10 000 ms |
| `RATE_HOUR` | 600 per hour |
| `MAX_WANT_PER_ROUND` | 500 seqs |
| `FILE_MAX_BYTES` | 25 MB |
| `FILE_CHUNK` | 65 536 |
| `FISH_DAY_CAP` | 30 answers per space per day |
| `FISH_PERSON_HOUR_CAP` | 5 per person per hour |
| `FISH_UNPROMPTED_CAP` / window | 4 per 30 minutes |

---

## 6. Glossary

| Word | Meaning |
|---|---|
| Space | A group with a name, members and channels. Id = 16 random bytes (`SpaceId`), shown as 32 hex chars. |
| Channel | A named stream of messages in a space, e.g. `#general`. Id = 8 random bytes. |
| Owner | The person who made the space. Signs every control op. |
| Member | A person in the space. Identified by their member public key (32 bytes). |
| Device | One PC of a member. Has a device signing key, a KEM key and a node id, tied together by a `DeviceCert`. |
| Node id | The iroh public key of a PC (`p2p::endpoint::NodeId`, facts §1). In `spaces/` it is `NodeKey`. |
| Control log | Owner-signed, hash-linked list of control ops. Replay = roster + channels + epoch. |
| Message log | All entries (messages, reactions, deletes, file offers) from all authors. Each author device has its own `seq` 1, 2, 3, ... |
| Entry | One signed, sealed item of the message log. Id = SHA-256 of its bytes. |
| Epoch | A generation of the space key. A new epoch starts when someone is removed or leaves. |
| Epoch key | 32-byte XChaCha20-Poly1305 key that seals entry bodies. Sealed to each device with HPKE. |
| Grant | One epoch key sealed to one device (`SealedKey`). |
| Invite | Lets one person ask to join: id, 88-bit secret (8 words), expiry 24 h, one use. |
| Invite link | `pms1:` + base32 payload: space id, invite id, owner node, relay, LAN addresses, expiry, space name. |
| Invite words | The 8 words that encode the invite secret. |
| Strike | One wrong word attempt against an invite. |
| Lockout | A stranger node that failed 3 joins cannot try again for 10 minutes. |
| Tombstone | A `Delete` entry that hides a message. Old copies may still exist on other PCs. |
| Block | Hide a member's messages on your own PC only. |
| Relay | iroh relay server that forwards encrypted QUIC packets when two PCs cannot connect directly. Sees only ciphertext. |
| Mailbox node | A headless member-like node that stores sealed entries for 7 days and cannot read them (D9.1, off). |
| Social mode | Fish with zero tools: talk only. |
| Room | Potato Brain's word for a group conversation; a space channel is room `space:ch:<space_hex>:<channel_hex>`. |
| Labs | Settings switches for unfinished features. `spaces_enabled` gates everything here. |

---

## Security (summary; each plan has its own section with tests)

| Threat | Defence | Proving test (plan) |
|---|---|---|
| Stranger connects to a PC with no invite open | `SpacePermissions` refuses `spaces` unless peer is a member or an invite is open; `admit()` closes the link | `stranger_is_refused_when_no_invite_is_open` (22) |
| Space member reaches desk input or clipboard | `RoutedPermissions` routes only `spaces`; desk keeps its own rule | `space_member_is_refused_on_desk_alpn` (21) |
| Guessing invite words | 88-bit secret; 5 strikes kill the invite; 3 failed joins lock a node out 10 min | `fifth_wrong_try_kills_the_invite` (22) |
| Forged control op | Ed25519 signature by the owner member key, hash chain from genesis | `op_signed_by_member_is_rejected` (22) |
| Forged or altered message | Device signature + AEAD with header as associated data | `flipped_byte_in_header_fails_open` (23) |
| Removed member reads new messages | Key rotation: new epoch key sealed only to remaining devices | `removed_member_cannot_open_new_epoch` (25) |
| Flood | Per-device rate limit, frame and entry size caps, link cap | `eleventh_entry_in_ten_seconds_is_dropped` (25) |
| Fish runs a tool or leaks owner data | Social mode with empty tool list; app-side caps; leak check before sealing | `tool_call_in_social_reply_is_not_executed` (26), `home_path_in_fish_reply_is_blocked` (26) |
| Space traffic through bridge/MCP/phone/pairing | Guard tests | `spaces_is_not_reachable_from_the_bridge_mcp_or_the_phone` (21) |

## What cannot be verified without real hardware or a second computer

- Hole punching success rate and relay fallback on real NATs (D0.4 is the measurement; it needs two PCs on
  different networks).
- Real latency of a message between two homes, and catch-up time for 90 days of history.
- That the public iroh relays accept our traffic in Labs and what happens when they are down.
- Windows firewall prompts on first bind, and macOS "allow incoming connections" prompts.
- 50 real members online at once (only simulated with `memnet`).
- The QR scan with a real phone camera.
