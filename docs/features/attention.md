# Attention: one list of what needs you

Code: `packages/app/src-tauri/src/attention/`. Design: `D:\pm-night\DESIGN-ATTENTION-PHONE.md` (phases A to F and the M3 rules: the list, the answer path, push v2, the phone card).

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
| `layout.rs`, `fingerprint.rs` | which choice leads and which is focused (`layoutChoices` ported, so the phone and the desktop cannot drift); the screen fingerprint an answer must send back |
| `answer.rs` | `POST /attention/{id}/answer`: the whole answer sequence, over an `Env` so it is tested with a fake screen |

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

## Answering from the phone (`POST /attention/{id}/answer`)

A paired person answers an approval, a folder-trust question, an agent's question, a consent request, a spent budget or a usage limit. Body: `{choice, prompt_id, occurrence, fingerprint, answer_key}` (a question may send `text`). Always HTTP 200 with an `outcome`, so a card never has to guess from a status code:

| `outcome` | When | Typed? |
|---|---|---|
| `answered` | everything matched; one key went out | once |
| `already_answered` | this device's `answer_key` was already answered (a retry), or the item is a tombstone that says who answered (`by`: `you`, `pc`, `phone` + `device_name`) | never |
| `changed` | a different prompt is up on that pane, or the pane printed under the key | never |
| `gone` | no prompt is up any more, the agent exited, or the item left | never |
| `failed` | the key could not be written (`PROMPT_REPLY_FAILED`) | nothing |

Refusals: 403 `DEVICE_CAP_REQUIRED` (`answer`; a budget or usage-limit card needs `control` too), 403 `PERSON_ONLY` (the caller is not a paired device: an MCP session or a plain local program answers with `answer_prompt`, where the agent-authority consent rules apply), 403 `broad_not_from_phone` (no "don't ask again" from a phone), 400 for a missing choice, fingerprint or a choice the prompt does not offer.

The sequence (`answer.rs`): ability, then the idempotency ledger (per device and `answer_key`, 512 entries, 10 min; a `failed` is not remembered, so "Try again" really retries), then the item, then what the card showed against the list, then under the existing answer lock (`agent_runs::serialize_answers`) the live screen is read again and compared (prompt id, occurrence, fingerprint). The key goes through the current answer route (`agent_runs::answer_decided`: checkpoint, run ledger, turn summary) under an `AnswerPermit` (`agent_runs/readiness.rs`): one pane, one prompt, used once, refused when the pane printed since the prompt was re-read (the answer then reads the screen once more and gives up as `changed` if it is not the same prompt). It is never a `GatePermit`, which stays Puppet Master's own dismiss key on a profile-declared notice.

The fingerprint is `sha256(prompt_id, occurrence, kind, summary, choice ids, risk level)[..16]`: not the raw screen (a ticking status line would make every answer stale), but everything the card printed, so a risk read that changed under the same command is refused too. The desktop's own answer (`answer_pane_prompt`) leaves a tombstone note, so a phone that answers late is told "Already answered on your PC: Allowed once."

## The phone card

`components/mobile/ApprovalCard.tsx` draws one item; `lib/approval-card.ts` is the pure view (`cardView`), so every state is tested without React: open, watch-only (no buttons), sending, lost (`Try again` re-sends the same key), offline, failed, answered, already answered, changed ("This question changed. Look again."), gone, expired, no readable prompt, sign-in screen. High risk leads with Deny and focuses it; a folder-trust question leads with Don't trust. `NeedsYouList` is the queue: every open blocking item in the PC's order, a tombstone for 15 s (3 s for this phone's own answer), and a new card is held while a finger is on the list. `DockedCard` sits OVER the top of the open agent's terminal (a terminal that shrinks is refitted and resized, which must never happen because a card appeared) with "n more waiting · Next: ..." under it. Questions, consent, budget and limit cards keep `QuestionsView` (their choices and text come from `GET /questions`).

Deep link: a v2 push carries `item_id`; a tap writes the intent to Cache Storage (`pm-intent`, 60 s) BEFORE it focuses or opens the app, then posts `pm-open-item` or opens `/?item=<id>`. The app (`hooks/usePushIntents.ts`) reads the launch URL, the message and the cache, on start and on every return to the front (an iOS PWA frozen in the background misses the message), opens each intent once, and resolves it against a fresh `GET /attention`: an open card docks in its pane (or opens as a sheet when it has no pane), a tombstone says what happened, an older item says "That request has been dealt with."

## Push v2

Payload `v: 2` adds `item_id`. Each phone gets its own words (`triggers::View`): Android and desktop browsers (`Full`: the calm command line and Allow / Deny when the phone may answer), an iPhone (`Ios`: Apple's push service shows no action buttons, so no command line and no grant; a question is "Question from Claude Code"), and a phone with **Hide details on lock screen** on (`Hidden`: the agent's name and the risk word only; the title is the agent, never the pane's title, which is often the task). The switch is per phone, default OFF (today's behaviour), kept on the PC (`push/store.rs`, `GET|POST /push/preferences`) and shown in the phone's notification settings. New kinds: folder trust (`Trust this folder? · shop`, or `<agent> needs you on your PC` for a sign-in screen), consent (`Codex wants to control Claude Code`), budget (`Budget used up · 20 of 20 turns`), usage limit (`Usage limit · resets 14:05`); none has a lock-screen action. A lock-screen Allow / Deny is bound to the occurrence and fingerprint the notification announced and is answered through the same function as the card (`bridge/routes/push.rs`): a stale tap types nothing and the service worker says "Couldn't send your answer".

## Not built yet

`done` / `review` producers and the phone's "Ready for you" rows, the loop-guard `stuck` card with Give it a hint / Stop / Ask another agent, the phone's "More options" for broad answers (the PC refuses them from a phone), the desktop reading the Rust list instead of deriving in TypeScript (M4, see below). See the design, phases G and H.

## Why the TypeScript derive is still there (M4)

`lib/attention.ts` `deriveNeedsYou` and `useNeedsYouSource` stay. The only thing that would justify deleting them is a dev-only shadow warning that has never fired in a live run, and that warning cannot fire in a built app: it is compiled in only when `import.meta.env.DEV` is true (`hooks/useNeedsYou.ts`), and the live run of this branch is a built exe. The harness and tests without a bridge also render the desktop from the TypeScript derive. The shared corpus keeps the two implementations equal in CI until the desktop reads the Rust list.
