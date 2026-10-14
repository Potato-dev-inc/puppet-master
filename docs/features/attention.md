# Attention: one list of what needs you

Code: `packages/app/src-tauri/src/attention/`. Design: `D:\pm-night\DESIGN-ATTENTION-PHONE.md` (phases A to C and the M3 rules; the phone card, the answer path and push v2 are not in this build).

## What and why

Five surfaces used to count "needs you" four ways (Inbox 5, phone 5, tray 3, away digest 3 on one mixed fixture). Now one module decides what needs a person, in what order and in which lane, and every surface renders that list: the Inbox, the sidebar group and the Doto counter, the tray badge and tooltip, the phone counter, the away digest, and the push.

The number is `counts.blocking`: open items in the blocking lane. `ready` items (an Ask-bar answer nobody saw, a finished run, something to review) are listed but carry no number.

## Shape

| File | Job |
|---|---|
| `model.rs` | kinds, lanes, rank, wire types (`Snapshot`, `Item`), the facts `derive` reads |
| `derive.rs` | `derive(&Facts)`: an exact port of `deriveNeedsYou` (`lib/attention.ts`) |
| `store.rs` | merges derived and raised items, applies the rules, keeps tombstones, stamps `rev` |
| `copy.rs` | the pane heading ("Claude Code 2"), the risk line, plain sentences |
| `mod.rs` | the process-wide store, `on_pane_status` / `on_prompt` / `on_changed` hooks, `raise` / `clear`, the `attention-tick` thread |
| `routes.rs`, `caps.rs` | `GET /attention` body; the device's capabilities (`mobile_pairing::Caps`) |

Wire: `GET /attention` (a paired device needs `watch` or `answer`), `event: attention` carrying the FULL snapshot on the SSE, WebSocket and poll fanout, and the Tauri event `attention://changed` plus command `attention_snapshot`. A client applies a frame only when its `rev` is greater than the last one it applied (`newerSnapshot`), and re-reads `GET /attention` on reconnect. There are no deltas, so there is no drift.

## Rules

- **Lanes and rank** (`model.rs`): consent, trust gate and a high-risk approval lead; then medium or unrated approvals; then low-risk approvals and questions; then budget, limit, tamper, setup; then errors and stuck panes. Ties: oldest first, then id. The ready lane is newest first.
- **One pane-state item per pane:** trust gate > approval > error > stuck. A question about a pane is its own item.
- **A trust gate** is an approval whose prompt kind is in `STARTUP_GATE_KINDS` (folder trust, sign-in). Only the person passes it; it never has a lock-screen action.
- **Occurrences:** the pane's prompt tracker (`pty://prompt`) numbers each time a pane asks. The same command asked twice is two items with two ids.
- **Tombstones:** an item that leaves stays 15 s as `state: resolved` (never counted) so a card on screen can say what happened. Questions expire at their deadline, consent after 10 min, done after 24 h.
- **Producers** (consent, budget, limit, tamper, setup, review, done) call `attention::raise(Raised)` and `attention::clear(key, resolution)`. Raising the same key again updates in place and keeps `since`. Consent is also derived today from a question that carries `consent`.
- **Item ids** are opaque (`base64url(sha256(kind|subject|occurrence))[..10]`) and never contain a pane id.

## Parity with the front end

`packages/shared/fixtures/attention/*.json` (25 cases) are run by vitest (`lib/attention.corpus.test.ts`) and by cargo (`attention::tests::parity`). Each file holds the input, the exact output of `deriveNeedsYou` and the sorted ids that COUNT (`blockingItems` in TS, the store in Rust). Neither implementation can change without the other test failing.

The desktop Inbox still derives its list in TypeScript; dev builds also read the Rust snapshot and `console.warn` when the two disagree for more than a second (`hooks/useNeedsYou.ts`).

## Push (what changed with it)

- Lock-screen Allow / Deny only for LOW risk. "Check first" (medium), high and unrated open the card.
- The risk words are the desktop card's: Low risk, Check first, High risk (`attention::model::Level::word`).
- Titles read `<pane heading> · Needs you`; the agent name moves into the body. An approval body is the risk line (at most 56 characters), the command only for a calm command, then the agent.
- A second prompt on the same pane inside the 30 s window is held, not dropped. When the window ends the attention list says what is still open: nothing (silence), one thing (its normal push) or several (one summary, `<pane> · 2 things need you`, never with a button).
- `push/golden/*.json` pin the exact payloads (`PM_UPDATE_GOLDEN=1 cargo test --lib push::tests_golden` regenerates them).

## Not built yet

The phone approval card, `POST /attention/{id}/answer` with the screen fingerprint and the answer ledger, `done` / `review` producers, the iOS / hide-details push variants, `?item=` deep links and the service-worker intent cache. See the design, phases D to H.
