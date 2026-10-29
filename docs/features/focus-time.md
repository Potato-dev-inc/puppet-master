# Focus Time (Preview)

Focus is a short quiet window. You press **Focus** for 25, 45 or 90 minutes (45 by default). While it is on, the
pings that can wait are held, and the cards that block an agent reach your phone and Telegram quietly. Agents keep
working. Focus never pauses, stops, types into, answers or changes the budget of any agent.

Code: `packages/app/src-tauri/src/attention/focus/` (the UI word is **Focus**; the code name is `attention::focus`).
Spec: `SPEC n33` section 3 (design) and section 4 (guards). Status: built and unit-tested; the live check is not run.

## What you see

| Where | What |
|---|---|
| Inbox, top note (`components/cockpit/FocusNote.tsx`) | Off: `Focus` and chips `25 min` `45 min` `90 min`. On: `Focus · 31 min left`, `Holding 3 updates. Things that need you still show.`, `+15 min`, `End Focus`. |
| Sidebar header (`FocusPill.tsx`) | Only while on: `Focus 31m`. A small menu with `+15 min` and `End Focus`. |
| Command palette (`palette-focus.ts`) | `Focus 25 minutes`, `Focus 45 minutes`, `Focus 90 minutes`; while on `End Focus` and `Add 15 minutes to Focus`. |
| Tray (`tray.rs`) | A `Focus` submenu. Tooltip: `Puppet Master · Focus 31 min left · 1 needs you`. The tray glyph and badge still count only what blocks. |
| Settings > General > Focus Time (`FocusSettings.tsx`) | Default length (5 to 240, 45 by default). Switch `Make cards that wait for you quiet during Focus` (on). |
| Phone, top of the All view (`components/mobile/FocusBar.tsx`) | Off: `Focus` and chips `25` `45` `90`. On: `Focus · 31 min left` and `End`. |
| The end | One desktop card (`AwayDigest`, title `While you focused`), one phone push `Focus is over`, one Telegram note. |

## The three tiers

Code: `attention/focus/policy.rs`. Pure functions, no clock, no I/O, no agent names. Each table is a `match` with no
wildcard arm, so a new attention or push kind does not compile until someone picks its tier. A test lists every kind.

| Attention card (`attention::model::Kind`) | Tier | Why |
|---|---|---|
| `Consent` | Through | an agent wants control |
| `Approval`, risk High | Through | safety |
| `Approval`, risk Medium, Low or unrated | Quiet | the agent is blocked; waiting is safe |
| `TrustGate`, `Question`, `Limit`, `Setup`, `Error` | Quiet | only the person can pass them |
| `Tamper`, `Budget`, `Stuck`, `Answer` | Through | money, loops, safety, or an answer to you |
| `Review`, `Done` | Hold | ready lane, nothing is blocked |

| Push kind (`push::triggers::Kind`) | Tier |
|---|---|
| `WaitingInput` | High risk: Through; otherwise Quiet |
| `TrustGate`, `Question`, `Limit`, `Error` | Quiet |
| `Consent`, `Budget` | Through |
| `RunFinished`, `RunFailed`, `CompanionReply`, `NightShift` | Hold |
| `Focus` (the end message and the reminder) | Through (Focus builds them itself) |

`decide(tier, settings)` gives the gate for a hook: `Hold` -> `Gate::Hold`; `Quiet` -> `Gate::Quiet` when
`quiet_blocking` is on, otherwise `Gate::Send`; `Through` -> `Gate::Send`.

Blocking cards are **never hidden and never held**. The Inbox, the tray badge and the count are the same as today.
Only the buzz changes. Quiet means: phone push with `quiet: true` (the service worker shows it silent, no renotify)
and Telegram with `disable_notification`. The text and the buttons are unchanged.

Invariants pinned by tests (`policy.rs`): every blocking attention kind is never `Hold`; every urgent push kind is
never `Hold`; `Hold` is exactly the set of Review, Done, RunFinished, RunFailed, CompanionReply and NightShift;
`decide(Through, _)` is always `Send`.

## Flows

Code: `attention/focus/live.rs` (the `Engine<I: Io>`; `LiveIo` is the real one). One `Mutex<State>`. The engine never
calls a sink while it holds the lock.

- **Start / change** (`start(minutes, by)`). Refused for `By::Local`. Minutes must be 5 to 240. Off: a new session
  with an empty ledger. On: the end moves to now + minutes, the start time and the held list are kept. The end is
  clamped to start + 240 minutes. Audited as `Focus started: 45 min` or `Focus changed: 45 min`.
- **Extend** (`+15 min`). Same clamp. Refused when off (`FOCUS_NOT_ON`).
- **End** (`end(by)`, also the timer). Idempotent. Builds one summary, keeps it as `last_summary`, clears the session
  and the ledger, then tells the desktop, the phones and the chat. Exactly once per session id.
- **Tick** (`focus-tick` thread, every 5 s). Ends a session whose time is up. Adds new blocking cards to the waiters
  list. Sends the one reminder. Saves what the hooks changed. A tick that never runs changes nothing: `is_on()` checks
  the deadline on every call.
- **Seeing cards** (`scan.rs`, pure). An open blocking card raised at or after the start, not yet known, becomes a
  waiter with the tier from `policy`. Hold kinds and the ready lane are ignored. Cap 200.
- **Reminder** (`nudge.rs`, pure). A Quiet waiter still open 20 minutes after Focus saw it gets one message:
  `1 agent has waited 20 minutes for you.` / `3 agents have waited 20 minutes for you.` Counts only. Sent as a
  `Focus` push and a Telegram note. Only while `quiet_blocking` is on (see gaps).
- **Restart** (`load_and_resume`). A session still running resumes. One that ended while the app was closed, or whose
  end is more than 240 minutes away (clock set back), ends now. Its summary waits as the pending digest.
- **Damaged file** (`store.rs`). Saved as `state.json` in `<app data>/focus/` (private folder, atomic write). A damaged
  or future-version file is renamed `state.json.bad` and Focus starts off with default settings.

### The summary

Code: `attention/focus/summary.rs`. Counts come from the turn tracker (`turn_summary`) and the ledger.

- `turns done`: distinct (pane, turn) with end `idle`, ended at or after the start.
- `failed`: the larger of distinct turns that ended in `error` or with outcome `failed_checks` / `failed`, and the held
  failed runs. Never added together.
- `asked for you`: Quiet waiters. `urgent`: Through waiters. `still waits`: open blocking cards now.
- Held updates (companion, review, night) are listed as `N updates held`. Held run ends are counted as turns and
  failed runs, not as held updates.

Lines:
- `While you focused (45 min): 4 turns done, 1 failed, 2 asked for you, 1 urgent. 1 still waits.`
- Early end: `Focus ended after 18 min: ...`
- Nothing happened: `Quiet. Nothing happened while you focused (45 min).`

An empty summary (nothing done, asked, held or waiting) shows no card and sends nothing.

### Where a held ping goes

- **Push** (`push/mod.rs`): `dispatch` asks `live::hold_push` first. A held run end or companion reply is counted and
  listed (cap 100 entries; the counts stay exact). The night push is held in `notify_night`. A run end in a
  conversation the person is chatting in on Telegram is still answered there (`channels::offer_answer`); that is an
  answer, not a ping.
- **Desktop**: the away digest does not fire while Focus is on (`turn_summary::on_return`). Desktop notifications
  for held kinds do not exist in the app today. Any new desktop ping must ask `attention::focus::is_on()` or the
  `live::gate_*` functions first (rule in `docs/REPO_MAP.md`).

## Guards (SPEC section 4)

| # | Guard | Where it is enforced | Test |
|---|---|---|---|
| S1 | Only a person starts or changes Focus; a local program may only end it; agents refused | `live.rs` (`By::Local` refused on start and extend), `bridge/routes/focus.rs` (MCP session -> 403 `PERSON_ONLY`) | `an_mcp_session_is_refused_on_every_focus_route`, engine tests |
| S2 | Dangerous kinds at full strength; blocking cards never hidden | `policy.rs` (no wildcard arms), pinned invariants | `policy.rs` table tests |
| S3 | Cannot stay "on" by accident | `is_on()` compares the deadline with the clock; clamp to 240; bad file -> off | `if_the_tick_never_runs_the_hooks_still_stop_holding_at_the_deadline`, `store_tests.rs` |
| S4 | Focus never writes to or answers a pane | Source scan of the module for `write_input`, `send_keys`, `press_key`, `TypingPermit`, `GatePermit`, `AnswerPermit`, `write_all`, `answer` | `s4_focus_module_never_writes_to_a_pane` |
| S5 | Held text does not leak | Phone view, push body and Telegram carry counts only; held sentences go through `redact::text` and the 120 character cap | `held_sentences_are_redacted_and_cut_before_they_are_kept`, `the_title_and_body_pass_through_redaction` |
| S6 | Bridge auth invariant | `bridge/routes/focus.rs` calls `authorize_caller` first; no unauthenticated route | route tests |
| S7 | Nothing is deleted | Hold delays a ping only; the cards, receipts and turn summaries stay | engine tests |
| S8 | No reminder spam, no double summary | One reminder per waiter; `end` is idempotent per session id | engine tests |
| S9 | Generic code never names an agent | Source scan of `attention/focus/` for `claude`, `codex`, `cursor`, `opencode`, `agy` | `s9_focus_module_never_names_an_agent` |
| S10 | No race between tick, hooks and clicks | One mutex; hooks only bump counters; no sink under the lock | engine tests |
| S11 | Telegram quiet message: same text and buttons | `channels/notify.rs` (only `silent` differs) | `notify_tests.rs` |
| S13 | No personal data in records | State holds ids, agent display names, counts and short redacted sentences | review of fixtures; `node scripts/check-no-personal-paths.mjs` |

## Routes, commands and events

Bridge (`bridge/routes/focus.rs`, `FocusPort` trait like `NightPort`):

| Route | Caller | Result |
|---|---|---|
| `GET /focus` | paired phone (`watch`), local program | `PhoneView`: on, end time, minutes, held total, waiting, last summary line. No pane id, no name, no sentence. |
| `POST /focus/start {minutes}` | paired phone (`control`) | the phone view. Local program: 403 `FOCUS_PERSON_ONLY`. |
| `POST /focus/extend` | paired phone (`control`) | the phone view. Local program: 403. |
| `POST /focus/end` | paired phone (`control`), local program | the phone view (idempotent). |

Any request with an MCP session header is refused with 403 `PERSON_ONLY` on every Focus route.

Tauri commands (`commands/focus.rs`, desktop window only, registered in `lib.rs`): `focus_state`, `focus_start`,
`focus_extend`, `focus_end`, `focus_settings_set`, `focus_summary_dismiss`. The tray calls the same engine with
`By::Tray` (menu ids `focus:25`, `focus:45`, `focus:90`, `focus:end`, `focus:more`).

Events: desktop `focus://changed` (desktop view) and `digest://ready`; phone SSE `event: focus` (phone view); audit
kind `focus` (counts only; the audit line names who pressed: desktop, tray, phone or local).

Push: kind `focus`, tag `pm-focus`, not urgent, sent only to phones that may watch and are not looking at the app.
A phone with "Hide details" gets `Open Puppet Master to see what happened.` Code: `push/focus.rs`, `push/hub.rs`
(`focus_deliveries`).

Chat: `channels::focus_note(line)` (counts only, no buttons, short mode: `Focus is over. Open Puppet Master.`).

## Error codes

| Code | Meaning | Bridge status and slug | Desktop text |
|---|---|---|---|
| `FOCUS_BAD_MINUTES` | Length outside 5 to 240 | 400, `focus_bad_minutes` | `Pick between 5 and 240 minutes.` |
| `FOCUS_NOT_ON` | Extend while Focus is off | 409, `focus_not_on` | `Focus is not on.` |
| `FOCUS_PERSON_ONLY` | A local program tried to start or extend (the engine also refuses `By::Local` on start and extend) | 403, `focus_person_only` | `Only you can start or change Focus.` (never reaches the desktop UI) |
| `PERSON_ONLY` | An MCP session (an agent) called a Focus route | 403, `focus_person_only` (code `PERSON_ONLY`) | - |

## v2 (not built)

Telegram `/focus 45` and `/focus off` (needs `control` and a confirm); scheduled Focus (weekday 9 to 11); per-project
Focus; "VIP agents" that always break through; start Focus when a screen share starts; Windows Focus Assist link;
keyboard shortcut; hold Team Heads-up cards (they make no push today); a Focus with a goal ("until tests are green");
a line in the companion's `today.md`.

## Known gaps (as the code is today)

- **Finished missions are not counted.** The Inbox folds the finished-mission rows (`MissionsSection`, `foldReady`),
  but no hook records them. `Class::Review` exists in the model and nothing writes it, so the summary never reports
  held missions.
- **No reminder when `quiet_blocking` is off.** The cards went out at full strength, so a reminder would repeat them.
  The spec does not say this; the code does.
- **Telegram gets the full summary line** (with the minutes in the header), not only the counts part. It has no names
  and no panes. The push gets the counts part only.
- **`Stuck` has no push yet.** Its tier is Through, so a push added later cannot be held silently.
- **The live check (SPEC section 6) is not run.** The unit and module tests are the only evidence so far.
