# Team ("Heads-up") v1: real teams, end-to-end encrypted

Branch `feat/cowork-v1` (base `integration/r10` @ 404e20d). Plan: `D:\pm-night\COWORK-PLAN.md` §3, §4, §8, §9 v1, §13.
Evidence logs and shots: `D:\pm-night\p0\cowork-v1\` (`lib1.log`, `lib2.log`, `bins.log`, `vitest.log`, `relay-tests.log`, `fail-before-v0.log`, `shots\`, `demo\`).

**What.** v0 put a teammate's push in your Inbox through a shared folder, with no identity. v1 makes it a real
team: people join with an invite and the admin's yes, every message is signed and sealed end to end, the wire is
a relay you host (or a folder), GitHub facts (PRs opened, merged or closed; CI red or green) arrive as the same
cards, and an open PR offers **Merge when green**. Agent-agnostic: events come from git refs, GitHub and PM's
own runs; no code path names an agent.

## How it works

| Piece | Where | Notes |
|---|---|---|
| Keys | `team/keystore.rs` | Member, device (Ed25519 + X25519) and epoch keys in the OS credential store, per profile. Never in a file, a log or a message. |
| Roster | `team/roster.rs` | Append-only log, each op hash-linked and signed by the admin (root) key. Team id = hash(root key, salt). Devices certified by their member's key. |
| Sealing | `team/secure.rs`, `team/crypto.rs` | Envelope = XChaCha20-Poly1305 (header as AAD, padded to 1/4/16 KiB) + Ed25519 by the sending device. `Session::open` checks shape, team, roster membership, signature, key, replay, then the payload (actor = signer, role). `ingest` takes only a `Verified`. |
| Keys to devices | `secure::make_grant` | The epoch key sealed per device with HPKE (RFC 9180, X25519/HKDF-SHA256/ChaCha20-Poly1305). Removing a member appends `Remove` + `Rotate`; the admin's app also rotates every 30 days. |
| Gaps | `secure::SeqTracker` | Per-device counters; a hole shows "Some team updates may be missing". |
| Wire | `team/transport/` | `TeamTransport` carries envelopes, two signed slots (`roster`, `keys`) and join boxes. `FolderTransport` (no plain events any more) and `RelayTransport` (WebSocket, long-poll fallback, device-signed requests). Outbound only. |
| Relay | `relay/` (top level) | `relay/protocol` (MIT): wire types. `relay/server` (licence pending): axum + SQLite, 7-day mailbox with a per-team counter that never goes back, root-signed access list, join points, rate limits, per-team storage quota. |
| Invites | `team/invite.rs`, `team/membership.rs` | `pm://team/join?…` link + 8-character code (10 min, single use, 5 tries). The request carries an HMAC of the code and is HPKE-sealed to the admin's device. The admin approves by hand. |
| GitHub facts | `team/sources/github.rs` | `gh api --include` with `If-None-Match`; 304s are free; baseline first; facts are local cards and are refused if a teammate relays one. |
| Merge when green | `team/merge_green.rs` | Offered only on cards this app made from your own `gh` (never a teammate's). The sheet shows GitHub's own answer for `repos/o/r/pulls/n` (repo, title, author, branch, head commit); confirming runs `gh pr merge <n> --auto --<method> --match-head-commit <sha> --repo o/r`. GitHub merges when checks pass; PM never merges. |
| Phone | `team/phone.rs`, `bridge/routes/team.rs` | `GET /team/cards` (watch), dismiss (answer or control), pull/queue/apply/discard (control). Agent sessions refused. |
| Audit | `audit/` | `team.sent`, `team.received`, `team.action`, `team.joined`, `team.role_changed`, `team.key_rotated`. |

## Safety (COWORK-PLAN §5.2, §8)
- A teammate's event is untrusted text that can only become a card; nothing runs on arrival.
- Team traffic never reaches the bridge: the transport only connects out, no team route is public, and the phone
  routes go through the normal bearer + device-caps gate.
- The relay enforces removal: a device taken off the root-signed access list loses new requests and its open
  WebSocket (found live and fixed: before, an open socket kept streaming ciphertext to a removed device).
- The relay sees team id, device public keys, epoch, counters, sizes and times. Test
  `relay_end_to_end_…_metadata_only_on_the_relay` greps its whole database for names, branches, notes, the code
  and the team key.

- Merge when green can't be steered by a teammate: the button is only on GitHub-fact cards, the sheet comes
  from GitHub, and the arm is bound to the commit the sheet showed.
- The queued-merge agent gets the teammate's branch, name, note and the conflicted file names only in one JSON data
  line labelled untrusted. It still reads the teammate's file contents (that's the job), so what really limits a
  hostile commit is the agent's permissions: a queued merge starts an agent by itself only when that agent's
  headless mode can't run commands without asking (`TuiProfile::headless_runs_commands_unattended`: Claude Code
  and Cursor Agent qualify; Codex, OpenCode and unknown agents don't). For the others the job waits at "Start <agent>" and runs in a
  pane you can see.
- "Checks passed", "merged" and "closed" come only from your own `gh`; a teammate's message claiming one is refused,
  and a teammate's event never replaces a GitHub card.

## OSS used (all permissive)
`ed25519-dalek` 2 (BSD-3), `chacha20poly1305` 0.10 and `hmac` 0.12 (RustCrypto, MIT/Apache-2.0; the AEAD was
audited by NCC Group in 2020), `hpke` 0.12 (rust-hpke, MIT/Apache-2.0), `axum` 0.8 (MIT), `rusqlite` 0.32 (MIT,
bundled SQLite: public domain), `tungstenite` 0.24 (MIT/Apache-2.0).

## Overlaps with other branches (file level)
`audit/mod.rs` (agent-authority: additive kinds), `mobile_pairing/caps.rs` and `device_caps_tests.rs` (device-caps:
one rule, four matrix rows), `bridge/routes/mod.rs` (one line), `lib.rs` (command list), `Cargo.toml`
(new deps; `tungstenite` gains its rustls feature for `wss://`).

## Limits
- One admin (the creator). Leaving is local; the admin removes the member. A member can't revoke their own lost
  device yet (the admin can).
- A device that misses more than one rotation while offline can't read mail sealed under the skipped epochs; the
  gap warning says so.
- No Cloudflare Worker variant yet (the axum relay is the tested one).
- **No forward secrecy.** Every team key a device was given stays in its credential store until it leaves the team,
  and the relay keeps ciphertext for 7 days. Someone who gets a device's keys and the relay's database (or recorded
  traffic) can read everything that device could. Removing a member rotates the key, so they can't read what is sent
  after their removal; members whose app hadn't seen the rotation yet may seal under the old key for up to about
  10 seconds before their next send re-reads the keys.
- **New members start at their join point (relay).** The relay hands a device only envelopes posted after it was
  added to the access list, so a new member doesn't receive the team's last 7 days. On a **shared folder** there is
  no such cut: a new member can read every envelope still in the folder that was sealed with the current team key.
- **A shared folder has no access control.** Removing a member rotates the key, but they keep read and write access
  to the folder itself (they can read what lagging members seal under the old key, and add or delete files). Use a
  relay for real teams.
- **Per-project filtering is not implemented.** Every event goes to every member of the team, so each member sees
  the repository, branch and pull request title of every project you announce. (COWORK-PLAN §8.4 plans filtering by
  project membership before sealing.)
- GitHub logins are trusted for attribution only after the admin confirms them (Settings > Team > Confirm GitHub);
  until then a GitHub fact shows the plain login.
