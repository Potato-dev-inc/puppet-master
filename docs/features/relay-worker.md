# Relay on Cloudflare (Worker + Durable Objects)

Branch `feat/relay-worker` (base `fix/live-r12` @ 2111483, which contains Team "Heads-up" v1). Code: `relay/worker/`.
Plan: `D:\pm-night\COWORK-PLAN.md` §3 (host architecture) and §8 (privacy and security). Evidence logs and shots:
`D:\pm-night\p0\relay-worker\` (`logs\`, `shots\`).

**What.** A second implementation of the team relay with the SAME wire protocol as `relay/server` (axum + SQLite),
as a Cloudflare Worker with one Durable Object per team. The app's `RelayTransport` works against it unchanged:
the user pastes the Worker's address in Settings > Team > Relay address. It gives the founder's free self-hosted
option (one-click "Deploy to Cloudflare", no machine to run) next to Docker/the binary, and the same code can run a
paid Puppet Master-hosted relay later. Agent-agnostic: the relay only ever sees sealed envelopes.

**Personas.** A (developers) deploy it with the button or `npm run deploy`; B and C never see it: a teammate pastes
an address, and invite links carry it.

## Design

| Piece | Where | Notes |
|---|---|---|
| Front door | `relay/worker/src/index.ts` | Routes, body caps (413 before anything else, as the server), the device signature, the per-address budget for unauthenticated calls, then the team's object. Works under a path prefix (the app signs the path it used). |
| Team object | `src/team.ts` | One Durable Object per team, SQLite-backed: team and access list, `roster`/`keys` slots, mailbox, join requests, replay guard, rate-limit windows. Long polls; WebSockets on the **hibernation API** (`acceptWebSocket`, per-socket cursor in the attachment). A device taken off the access list has its open socket closed. |
| Address gate | `src/gate.ts` | One object per client address (named by a hash, so no address is stored): the 30-an-hour counter for registrations and join requests. |
| Protocol checks | `relay/worker/wasm` (crate `pm-relay-wasm`) -> `src/wasm/` | `relay/protocol` itself, compiled to WebAssembly: strict Ed25519 verify, the signed bytes, team id = hash(root key, salt), shapes and size rules. It returns the same status codes and messages as the server. Committed, so deploy needs no Rust; `npm run build:wasm` rebuilds it (pinned wasm-bindgen 0.2.129). |
| Expiry | `Team.alarm()` | Mailbox 7 days, join requests 1 day, replay guard and counters shortly after. Hourly while anything can expire, none when empty. Reads also refuse an expired envelope, so the alarm is housekeeping, not correctness. |
| Tests | `relay/worker/test`, `scripts/run-tests.mjs` | See Evidence. |

**Choice: Rust compiled to WASM, but not all of `workers-rs`.** The brief preferred reusing `relay/protocol` for
exact compatibility "if `workers-rs` makes that practical". Reusing the crate was easy; running the whole relay in
`workers-rs` was not attractive (young Durable Object, SQLite and hibernation support; `worker-build`/Rust on the
user's deploy path would break one-click). So the stateful half (routing, storage, sockets, limits) is TypeScript on
the stable Workers API, and every validity decision is the shared Rust crate through a ~200 KB wasm module.

**OSS used:** Cloudflare `wrangler` 4 (MIT/Apache-2.0, dev dependency of `relay/worker` only, never global),
`@cloudflare/workers-types` and `typescript` (dev only), `wasm-bindgen` 0.2.129 (MIT/Apache-2.0). Rust side adds only
`wasm-bindgen` and `serde_json` to `relay/protocol`'s own deps.

**Licence.** `relay/worker/LICENSE` is a placeholder: "LICENCE PENDING FOUNDER DECISION", like `relay/LICENSE`.
The deploy button copies the folder into the user's Git account, so the founder's choice matters before it is public.

## Differences from the axum relay (all intentional)

- The mailbox counter only grows. The server uses `MAX(seq)+1`, which starts again at 1 once every envelope has
  expired (a week of silence); a client holding a higher cursor would then miss new mail. The Worker keeps a counter.
  (The server is untouched on this branch; flagged under "what is left".)
- Long polls hold the object awake up to 25 s (clients prefer the WebSocket). WebSockets hibernate.
- No server ping on the WebSocket (a hibernating object cannot send one); the app already falls back to polling.
- Rate-limit windows live in storage (a few extra row writes per message) because an object's memory can be evicted.
- Test routes `/v1/_test/{team}/{dump|drop|limits|age|alarm}` exist, but only when `TEST_HOOKS=1`, which only the
  local test runner sets. `wrangler.toml` says never to set it on a deployment.
- A finding worth knowing: the Workers runtime gzips responses for clients that accept it, even a bodiless `204`.
  `ureq` (the app's HTTP client) then cannot read that body to its end and `RelayTransport::call` fails with "unexpected
  end of file". Every empty reply therefore carries `content-encoding: identity` (`src/http.ts`, `empty()`).

## Security review items (D:\pm-night\SECURITY-REVIEW-COWORK.md) not copied into the Worker

- M1 mailbox counter restart: a monotonic per-team counter (`team.next_seq`), test "the relay's cursor never goes back".
- M3: ACL check before the replay guard records anything; expired replay rows deleted per call; rate-limit windows
  pruned (every 200 hits and by the alarm); the per-team join budget is counted before per-invite counters. Tests:
  "only devices on the access list leave a trace in the replay guard", "junk invite ids cannot grow the counters".
- M4: broadcast built once per post to every socket (cursor == seq-1), no per-socket query; catch-up only for a socket
  that is behind; ACL change closes removed sockets without queries; client frames are ignored; 5 sockets per device.
- M5: per-address limits read `CF-Connecting-IP`; invite windows have a finite life (1 day); counters are persisted
  in the team's SQLite; a per-team cap of 20,000 stored envelopes (test "a team's mailbox has a cap").
- Not done: rate-limiting failed signatures per address (left to a Cloudflare WAF rule, noted in the README).
- `fix/cowork-hardening` (D:\pm-cowork-harden) had not landed when this was written (no commits past 2111483), so the
  semantics are from the review text; compare the final server numbers (invite lifetime, caps) when it lands.

## Settings copy

Settings > General > Labs > Team > Create a team > **Relay address** now has a help line under the field:
"Use Puppet Master's relay, or host your own (Cloudflare or Docker)." + link "How to host your own" to
`relay/README.md` on GitHub (opens the system browser in the desktop app). Settings copy only; no backend change.
Test: `TeamSettings.test.tsx` "create: the relay address says in plain words where to host your own...". Shots:
`D:\pm-night\p0\relay-worker\shots\{before,after}\settings-team-create.png`.

`relay/README.md` gained a "Pick one" table (Cloudflare, Docker, binary, Puppet Master's own) and `relay/Dockerfile`
(+ `.dockerignore`) so the help line's "Docker" has something to point at.

## Evidence

See the commands and result lines in `D:\pm-night\p0\relay-worker\REPORT.md`. In short:

- The Rust end-to-end tests are the SAME ones the axum relay passes, parameterised by `PM_RELAY_TEST_URL`:
  `relay/server/tests/protocol.rs` (7 tests) and the app's `relay_end_to_end_create_invite_join_push_and_card_with_metadata_only_on_the_relay`,
  `on_the_relay_a_removed_member_is_locked_out_and_told_so`, `an_envelope_the_relay_drops_shows_up_as_a_gap`
  (`packages/app/src-tauri/src/team/tests_v1.rs`), against `wrangler dev`, and unchanged against axum when the variable is not set.
- Worker-only tests (`relay/worker/test/worker.test.mjs`, an independent client written with `node:crypto`): 7-day and
  1-day expiry by the alarm, the cursor never going back, 413, spoofed internal headers, the per-address limit, no test
  hooks as deployed, a path prefix.
- Fail-before: the Settings test fails on the previous component (`logs/vitest-fail-before.log`); the Worker fails the
  app's end-to-end tests before the 204 fix (the "unexpected end of file" above); mutation checks on the expiry and the
  address limit make the Worker tests fail.
- No Cloudflare account was used: nothing logged in, nothing deployed; all of it runs under `wrangler dev`.

## Overlaps with other branches (file level)

`relay/Cargo.toml` and `relay/Cargo.lock` (one workspace member, `worker/wasm`), `relay/server/tests/protocol.rs` and
`packages/app/src-tauri/src/team/tests_v1.rs` (tests only: parameterised by URL), `relay/README.md`,
`docs/features/team-heads-up.md` (one line), `packages/app/src/components/settings/blocks/TeamSettings.tsx` (+ test) and
`packages/app/src/styles/settings.css` (a help line, `.st-link`). The `tests_v1.rs` and `TeamSettings` edits are the
likely merge hot-spots with any other co-work branch.

## Limits / what is left

- Not deployed anywhere, so Cloudflare's real edge (WebSocket idle behaviour, free-plan quotas, the one-click flow
  itself) is unverified; the deploy button needs the repository to be public.
- `relay/Dockerfile` was written but not built (no Docker engine running here).
- The wasm in `src/wasm/` is committed. Rebuilding it here twice gave the same sha256 (`597eeb36...`), but a CI check
  that a rebuild matches the committed file is not set up.
- The server's `MAX(seq)+1` behaviour (above) is not fixed on this branch.
- There is no Puppet Master-hosted relay yet; the Settings line mentions it as the founder asked.
- Not done: billing/auth for a paid hosted relay, per-team storage caps, an admin page.
