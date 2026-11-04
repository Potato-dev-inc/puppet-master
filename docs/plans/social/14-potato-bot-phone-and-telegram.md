# 14 - Potato Bot on the phone, and on Telegram

Status: plan only. Written 2026-11-04 from `00-FORMAT.md` and `_facts/potato-bot-facts.md` only (no source code was
read, by founder request). `Read first` lists are for the builder. Unknown names are `NEEDS-FACT`, listed at the end.

## What this plan builds

Part A (phone): the person's paired phone gets a Potato Bot surface in the phone web app: the bot list, a thread to
chat in, and Fish's computer to watch and (after a yes on the PC, plan 13) control.

Part B (Telegram): Potato Bot can answer on Telegram with the same rules as Discord (plan 11): which chats, who may
talk, rate limits, and Sandbox only.

## Facts this plan rests on

- Telegram code lives in `packages/app/src-tauri/src/channels/` (not in `potato/`). PR #198 (`856c0acc`, "Tag Your
  Agent") added `config.rs`, `engine.rs`, `engine/group.rs`, `fake_bot.rs`, `host.rs`, `host_live.rs`, `setup_tag.rs`,
  `tag_config.rs`, `tag_log.rs`, `tag_prompt.rs`, `telegram/api.rs`, `telegram/api/wire.rs`, `telegram/poller.rs`,
  fixtures `group_mention.json`, `group_reply_to_bot.json`, `test_host.rs`, `commands/channels.rs`, and UI
  `settings/blocks/TelegramTagBot.tsx`, `settings/sections/PhoneSection.tsx`, `lib/telegram-api.ts` (facts 5a).
- The Telegram group path is default off (facts 5a).
- Potato Bot itself has no Telegram code path; `docs/features/potato-bot.md` l.91 calls chat apps a later phase.
- A paired phone can watch Fish's computer; control needs a desktop yes (`PHONE_NEEDS_DESKTOP_YES`) (facts 6e).
- The phone app has a theme root `styles/phone.css` `.ph` (facts 10b). Mobile app is "partial: phone pairing (not
  Potato-specific)" (facts 8).
- The brain knows `Trust = "owner_pc" | "owner_remote" | "others"` (`potato-brain/src/channels/potato.ts` l.61).
- noVNC viewer, `mobile-keyboard.js` and `clipboard-bridge.js` are served by `screen.rs` l.715 (facts 8).

### Conflict with the founder's wording, and how it is resolved

The founder wrote "Telegram is already merged". The facts show what is merged is **Tag Your Agent**: Telegram for
coding agents, not for Potato Bot. So this plan:
1. lists what is missing for Potato Bot on Telegram (table below) and builds it (T1, T3-T5);
2. applies the founder rule "anything triggered from outside runs in the Sandbox only" to the merged Tag Your Agent
   group path too (T2). This may change Tag Your Agent's behaviour; it is the founder's rule, so it wins. The path is
   default off already, so nobody's setup breaks silently.

### What is missing for Potato Bot on Telegram

| Item | Discord (plan 11) | Telegram today | Slice |
|---|---|---|---|
| Labs flag, off by default, in `DEV_LABS_KEYS` | `potato_discord_enabled` | none for Potato | T1 |
| Token in the OS credential store | `discord_bot_token` | Tag Your Agent has its own token (NEEDS-FACT name) | T3 (`potato_telegram_bot_token`) |
| Which chats | `OutsideConfig.places` | Tag Your Agent config | T3 (reuses `potato/outside/`) |
| Who may talk | `People`, owner ids, listed ids | NEEDS-FACT | T3 |
| Rate limits | `RateBook` 6/5 min person, 20/10 min place | NEEDS-FACT | T3 |
| Sandbox only | `outside::policy` | not enforced | T2 (Tag Your Agent), T3 (Potato) |
| Replies out, plain text, no mass mentions | `allowed_mentions.parse=[]` | NEEDS-FACT | T4 (no `parse_mode`) |
| Settings page with step-by-step setup | D10 | `TelegramTagBot.tsx` block only | T5 |
| Owner commands (`/help`, `/ping`, `/mood`) | plan 15 R5 | none | plan 15 R5 |

## Slice index (dependency order)

| Slice | Goal | Size | Needs merged first |
|---|---|---|---|
| P1 | Phone bridge routes for Potato (state, messages, send, read) | Sonnet | none |
| P2 | Phone transport + bot list + thread screens | Sonnet | P1 |
| P3 | Phone: watch Fish's computer and ask for control | Haiku | P2, plan 13 S3 |
| T1 | Labs flag `potato_telegram_enabled` | Haiku | plan 11 D1 |
| T2 | Tag Your Agent group path: Sandbox only | Sonnet | plan 11 D2 |
| T3 | Potato Telegram link: updates -> admit -> `send_outside` | Sonnet | T1, plan 11 D3, D4 |
| T4 | Replies out: `sendMessage`, typing, 429 | Haiku | T3 |
| T5 | Settings page with BotFather steps | Haiku | T4 |

Rules for every slice: never run a dev server or the app on the primary screen; separate `CARGO_TARGET_DIR` per slice
(`D:/pm-night/target-<slice>`); never kill by image name; new Labs flags join `DEV_LABS_KEYS`; `p2p/guard_tests.rs`
and `desk/guard_tests.rs` stay green; pairing has no route from agents/MCP/phone; team traffic never uses the bridge
or pairing; UI monochrome; commit finished work with a fresh build id. Commands are Git Bash from the repo root.

---

## Part A - Phone

Decision for phone turns: the phone is the owner, away from the PC. Trust is `owner_remote` and tool rules are
today's `rules::tool_access("owner_remote")`. The founder's Sandbox-only list (Discord, Telegram, groups with other
people) does not name the owner's own phone. `DECISION-OPEN (default: phone keeps today's owner_remote rules; a helper
Start card from a phone turn is shown on the PC, and the phone shows "Start this on your computer.")`.

## Slice P1 - Phone bridge routes for Potato

### Goal
Give a paired phone four Potato routes: state, messages, send, mark read, all behind the existing phone auth.

### Read first
- `packages/app/src-tauri/src/bridge/routes/potato_screen.rs`: how a phone request is authenticated today (the phone route pattern).
- `packages/app/src-tauri/src/bridge/http.rs` l.100-140 and l.350-390: path guards.
- `packages/app/src-tauri/src/commands/potato.rs` l.35-80: `potato_state`, `potato_messages`, `potato_send`, `potato_mark_read` (the routes return the same shapes).
- `packages/app/src-tauri/src/potato/service.rs` l.686-700: `send`, `send_as`, `Origin`.

### Files
- create `packages/app/src-tauri/src/bridge/routes/potato_phone.rs`
- create `packages/app/src-tauri/src/bridge/routes/potato_phone_tests.rs`
- edit `packages/app/src-tauri/src/bridge/routes/mod.rs` (declare; NEEDS-FACT file name)
- edit `packages/app/src-tauri/src/bridge/http.rs` (route `/potato/phone/`)
- edit `packages/app/src-tauri/src/potato/service.rs` (`Origin::Phone` if it does not exist; trust `owner_remote`)

### Steps
1. Write `respond(...)` (Code) with exactly these routes.
2. Each route first checks the phone session (same check as the screen routes), then `labs_on`; off -> `404` (the phone hides Potato).
3. `send` calls `service.send_as(bot, text, Origin::Phone)`; `Facts` surface `"phone"`, trust `owner_remote` (NEEDS-FACT: field names).
4. Wire into `http.rs` under the phone-authenticated branch only.
5. Tests.

### Code
```rust
/// GET  /potato/phone/state                       -> 200 PotatoState JSON (same as potato_state), no secrets
/// GET  /potato/phone/messages?bot=<id>&limit=<n> -> 200 [PotatoMessage] (limit 1..=200, default 50)
/// POST /potato/phone/send   {"botId":"..","text":".."} -> 200 SendOutcome | 400 {"code","message"}
/// POST /potato/phone/read   {"botId":".."}        -> 204
/// Anything else -> 404. Body > 64 KB -> 413. Not a paired phone -> the existing phone-auth error.
pub fn respond(service: &Service, phone: &PhoneSession, method: &str, segments: &[&str],
               query: &str, body: &[u8]) -> (u16, String);
```
`PhoneSession` is the type the screen routes get after phone auth (NEEDS-FACT name). The state JSON for the phone
drops any field with a key mask (`deepseek` mask) and settings: send only `enabled`, `bots` (id, name, persona, unread,
brain state), and `held`.

Error body for refused sends: `{"code":"POTATO_PAUSED","message":"<the same sentence the desktop shows>"}` using the
existing codes `POTATO_OFF`, `POTATO_EMPTY`, `POTATO_PAUSED`, `POTATO_RATE`, `POTATO_REFUSED` (facts 2 step 4).

### Tests to write (`potato_phone_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `state_needs_phone_session` | no session | existing phone-auth rejection status |
| `state_hidden_when_labs_off` | session, labs off | `404` |
| `state_has_no_key_mask` | session, labs on, key set | body does not contain `deepseek` |
| `messages_limit_clamped` | `limit=999` | at most 200 messages requested from the service |
| `send_uses_owner_remote` | send `hi` | fake service saw `Origin::Phone`; inbox item trust `owner_remote` |
| `send_empty_refused` | text `"  "` | `400`, `code POTATO_EMPTY` |
| `big_body_refused` | 100 KB body | `413` |
| `unknown_route_404` | `GET /potato/phone/x` | `404` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-P1 cargo test --lib bridge::routes::potato_phone
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-P1 cargo test --lib p2p::guard
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-P1 cargo test --lib desk::guard
```
Expected: `test result: ok. 8 passed`; guards 0 failed.

### Done when
- [ ] A paired phone can read state and messages and send to a bot; nothing else.
- [ ] No pairing, settings or secret route was added.

### Do NOT
- Do not add any pairing or settings route for the phone (rule: pairing has no route from the phone).
- Do not return the DeepSeek mask, settings or file paths to the phone.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. No `Origin::Phone`: add it next to `Person`, mapped to surface `"phone"` and trust `owner_remote`; add the match arms.
2. The phone routes live under a different prefix in `http.rs` (for example `/phone/`): use that prefix + `potato/...` and update the TS paths in P2.
3. `PotatoState` serialises the mask: build a separate `PhonePotatoState` struct with only the listed fields.

---

## Slice P2 - Phone transport, bot list and thread screens

### Goal
In the phone web app, show Potato Bot: a list of bots and a chat thread, using a `PotatoTransport` that talks to the P1 routes.

### Read first
- `packages/app/src/lib/potato.ts`: `PotatoTransport` interface and `fakePotatoTransport`.
- The phone app entry and its router (NEEDS-FACT: folder, for example `packages/app/src/phone/`), `packages/app/src/styles/phone.css` (`.ph` tokens).
- `packages/app/src/lib/potato-thread.ts` (`THREAD_COPY`, day separators) - reuse, do not copy.

### Files
- create `packages/app/src/lib/potato-phone-transport.ts`
- create `packages/app/src/components/phone/potato/PhonePotato.tsx` (move under the real phone folder if different; NEEDS-FACT)
- create `packages/app/src/components/phone/potato/PhonePotato.test.tsx`
- edit the phone app router to add a `Potato` tab (NEEDS-FACT file)
- edit `packages/app/src/styles/phone.css` (`.ph-potato-*` rules, tokens only)

### Steps
1. `potato-phone-transport.ts`: implements `PotatoTransport` with `fetch` to the P1 routes using the phone session the phone app already has (NEEDS-FACT: the phone fetch helper). `onChanged` polls `state` every 4 s while visible (no websocket in this slice).
2. `PhonePotato`: two screens in one component: list (bot rows with name, unread) and thread (messages, a text box, `Send`). Back button returns to the list.
3. Router: add a tab `Potato` that shows only when `state` loads (404 -> tab hidden).
4. Tests with `fakePotatoTransport`.

### Code
```ts
export function phonePotatoTransport(api: PhoneApi): PotatoTransport;
// PhoneApi = the phone app's authenticated fetch wrapper (NEEDS-FACT name): get(path), post(path, body)
```
UI text:
```ts
export const PHONE_POTATO_COPY = {
  tab: 'Potato',
  listTitle: 'Bots',
  empty: 'No bots yet. Make one on your computer.',
  placeholder: (bot: string) => `Message ${bot}`,
  send: 'Send',
  back: 'Back',
  offline: "Can't reach your computer. Check that Puppet Master is open.",
  helperOnPc: 'Start this on your computer.',
} as const;
```

### Tests to write (`PhonePotato.test.tsx`)
| Test | Action | Expected |
|---|---|---|
| `lists bots` | fake with 2 bots | both names shown |
| `empty state` | 0 bots | `No bots yet. Make one on your computer.` |
| `opens thread and sends` | tap Fish, type `hi`, tap `Send` | fake `send('fish-id','hi')` called; `hi` shown |
| `back returns to list` | tap `Back` | list shown |
| `offline message` | fake `state` rejects | `Can't reach your computer. Check that Puppet Master is open.` |
| `phone transport paths` (in `potato-phone-transport.test.ts`) | `send('b','x')` with fake `PhoneApi` | POST `/potato/phone/send` body `{"botId":"b","text":"x"}` |

### Run
```
cd packages/app && npx vitest run src/components/phone/potato src/lib/potato-phone-transport.test.ts
```
Expected: 0 failed (6 tests).

### Done when
- [ ] On a paired phone (checked with the preview tools at mobile size, not the primary screen), the Potato tab lists bots and chats work.
- [ ] Monochrome using `.ph` tokens.

### Do NOT
- Do not store messages in phone `localStorage` beyond the open bot id.
- Do not add a websocket or push in this slice.
- Do not run the app on the primary screen; use `preview_start` and resize to mobile.

### If it fails
1. Phone folder differs: place the component in the real phone folder; keep the file names.
2. Polling keeps running when hidden: stop on `document.visibilitychange` hidden.
3. `PotatoTransport` has methods the phone cannot serve (settings, files): implement them as `Promise.reject(new Error('Not on the phone.'))` and do not render their UI.

---

## Slice P3 - Phone: watch Fish's computer and ask for control

### Goal
From a bot's thread on the phone, open Fish's computer view (watch), and ask to control it with the plan 13 desktop-yes flow.

### Read first
- `packages/app/src/components/cockpit/potato/useScreenLink.ts` (`KEEP_EVERY_MS`, `SILENT_VIEWER_MS`), `useScreenState.ts`, `lib/potatoScreen.ts` (`ScreenTransport`, `headerWords`).
- `packages/app/src-tauri/src/bridge/routes/potato_screen.rs` (phone view and control routes, plan 13 S3 responses).

### Files
- create `packages/app/src/components/phone/potato/PhoneComputer.tsx`
- create `packages/app/src/components/phone/potato/PhoneComputer.test.tsx`
- edit `packages/app/src/components/phone/potato/PhonePotato.tsx` (a `Computer` button in the thread header)

### Steps
1. `PhoneComputer` embeds the existing noVNC `embed.html` view the screen route serves (view-only by default).
2. Button `Control` -> request control; responses: lease granted -> control on; `409 PHONE_NEEDS_DESKTOP_YES` -> show waiting text and poll every 3 s for up to 2 minutes; `403 PHONE_CONTROL_REFUSED` -> show refused text.
3. Button `Give back` releases the lease.

### Code
UI text:
```ts
export const PHONE_COMPUTER_COPY = {
  open: 'Computer',
  watching: 'Watching',
  control: 'Control',
  waiting: 'Waiting for a yes on your computer.',
  refused: 'Your computer said not now. Try again later.',
  expired: 'No answer from your computer. Try again.',
  controlling: 'You have control',
  giveBack: 'Give back',
} as const;
```

### Tests to write (`PhoneComputer.test.tsx`, fake screen transport)
| Test | Action | Expected |
|---|---|---|
| `starts watching` | open | `Watching`; control off |
| `needs desktop yes` | tap `Control`, fake returns 409 with askId | `Waiting for a yes on your computer.` |
| `yes arrives` | next poll returns granted | `You have control` and `Give back` |
| `refused` | 403 | `Your computer said not now. Try again later.` |
| `expires after 2 minutes` | fake timers, 41 polls of 409 | `No answer from your computer. Try again.` and polling stopped |

### Run
```
cd packages/app && npx vitest run src/components/phone/potato/PhoneComputer.test.tsx
```
Expected: `Tests  5 passed (5)`.

### Done when
- [ ] Phone can watch; control only after the PC said yes once for this phone.

### Do NOT
- Do not add any way for the phone to answer the ask itself.
- Do not keep polling after leaving the screen.

### If it fails
1. The screen route paths differ for the phone: copy them from `lib/potatoScreen.ts`.
2. Fake timers and `fetch` promises interleave badly: `await vi.advanceTimersByTimeAsync(3000)` per poll.
3. Embed blocked in tests: render the iframe with `src` only; do not load it in jsdom.

---

## Part B - Telegram

## Slice T1 - Labs flag `potato_telegram_enabled`

### Goal
Add `potato_telegram_enabled` (off by default), on only when Potato Bot and Fish's computer are on.

### Read first
- Same files as plan 11 slice D1; `live.rs` `discord_labs_on` (copy it).

### Files
- edit `packages/app/src-tauri/src/potato/live.rs`
- edit `packages/app/src-tauri/src/settings_store.rs`
- edit `packages/app/src/lib/dev-labs.ts`
- edit `packages/app/src/lib/setup-checks/labs.ts`
- edit `packages/app/src/components/settings/model.ts`

### Steps
1. `live.rs`: `pub const TELEGRAM_LABS_KEY: &str = "potato_telegram_enabled";` and `telegram_labs_on(public)` = `labs_on && sandbox_labs_on && flag_true(TELEGRAM_LABS_KEY)`.
2. `DEV_LABS_KEYS` (Rust and TS): add `potato_telegram_enabled` right after `potato_groups_enabled` (plan 12 G1) or, if G1 is not merged, right after `potato_discord_enabled`. Same position in both.
3. `LABEL`: `potato_telegram_enabled: 'Potato Bot on Telegram',`.
4. `model.ts`: add to the row union and the `visibleSections` type.

### Code
```rust
pub const TELEGRAM_LABS_KEY: &str = "potato_telegram_enabled";

pub fn telegram_labs_on(public: &PublicSettings) -> bool {
    labs_on(public) && sandbox_labs_on(public) && flag_true(public, TELEGRAM_LABS_KEY)
}
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `telegram_labs_off_when_missing` | bot + sandbox on | `false` |
| `telegram_labs_off_without_sandbox` | bot + telegram on | `false` |
| `telegram_labs_on_when_all_three` | all on | `true` |
| `dev_labs_keys_include_potato_telegram` | list | contains |
| `dev-labs.mirror.test.ts` | both lists | equal |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T1 cargo test --lib telegram_labs
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T1 cargo test --lib settings_store
cd packages/app && npx vitest run src/lib/dev-labs.mirror.test.ts
```
Expected: `test result: ok. 3 passed`; 0 failed; `Tests  2 passed (2)`.

### Done when
- [ ] Flag in both lists, same place, default off.

### Do NOT
- Do not default to `true`. Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Mirror order mismatch: same position in both files.
2. Length assertion in `settings_store` tests: bump by 1.
3. `flag_true` missing: it was added in plan 11 D1; merge D1 first.

---

## Slice T2 - Tag Your Agent group path: Sandbox only

### Goal
Apply the founder rule to the merged Tag Your Agent group path: a coding-agent run triggered from a Telegram group runs only in Fish's computer, or is refused.

### Read first
- `packages/app/src-tauri/src/channels/engine/group.rs` and `engine/group_tests.rs`: where a group message turns into an agent run (NEEDS-FACT: the function that starts the run and where it runs).
- `packages/app/src-tauri/src/channels/tag_config.rs`, `setup_tag.rs`, `tag_prompt.rs`.
- `packages/app/src-tauri/src/potato/outside/policy.rs` (plan 11 D2).
- `docs/features/tag-your-agent.md`.

### Files
- edit `packages/app/src-tauri/src/channels/engine/group.rs`
- edit `packages/app/src-tauri/src/channels/engine/group_tests.rs`
- edit `packages/app/src-tauri/src/channels/host.rs` (host method `sandbox_ready() -> (bool /*labs*/, bool /*installed*/)` + `host_live.rs` + `test_host.rs`)
- edit `packages/app/src/components/settings/blocks/TelegramTagBot.tsx` (one line of text)
- edit `docs/features/tag-your-agent.md`

### Steps
1. Before the group path starts any agent run, call `potato::outside::policy::check(Some(Surface::Telegram), labs, installed)`.
2. `Err(_)` -> do not start; reply in the group with `TAG_SANDBOX_TEXT` (Code) once per chat per 24 h (reuse `outside::engine::RefusalMemory`).
3. `Ok(SandboxOnly)` -> start the run with its working place set to the Sandbox (NEEDS-FACT: how the run picks a place; if Tag Your Agent can only run local panes today, the result is always "refuse" until a Sandbox runner exists: `DECISION-OPEN (default: refuse with TAG_SANDBOX_TEXT; a Sandbox runner for coding agents comes with plan 13 S8-S10)`).
4. Settings block: add the note under the group switch.
5. Doc: add a "Safety" paragraph.

### Code
```rust
pub const TAG_SANDBOX_TEXT: &str =
    "I can only run agents from a group inside my own computer. Ask my owner to set it up.";
```
Settings note (`TelegramTagBot.tsx`):
```
Agents asked from a Telegram group only run in Fish's computer, never on this PC.
```
Doc paragraph (`docs/features/tag-your-agent.md`):
```
Safety: a run asked from a Telegram group runs only in Fish's computer (the Sandbox). If the Sandbox is off or not
set up, the bot says so in the group once a day and starts nothing. Founder rule, 2026-11-04.
```

### Tests to write (`group_tests.rs`, using `fixtures/group_mention.json`)
| Test | Input | Expected |
|---|---|---|
| `group_run_refused_without_sandbox` | mention fixture, test host sandbox labs off | no run started; one reply with `TAG_SANDBOX_TEXT` |
| `group_refusal_once_a_day` | same fixture twice, 1 minute apart | one reply total |
| `group_run_never_local` | sandbox on + installed | no local pane/run started (the recorded host calls contain no local start); per step 3 default, a refusal reply |
| `dm_path_unchanged` | an existing DM test | still passes (DMs from the owner are not covered by this slice: `DECISION-OPEN (default: Tag Your Agent DMs keep today's behaviour)`) |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T2 cargo test --lib channels
```
Expected: `test result: ok.` 0 failed.

### Done when
- [ ] No Telegram group message can start an agent run on the PC.
- [ ] The group gets one plain message a day when refused.

### Do NOT
- Do not change the Tag Your Agent DM path in this slice.
- Do not turn the group path on by default.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Plan 11 D2/D4 not merged: `potato::outside` missing. Merge them first.
2. `test_host.rs` has no sandbox state: add two `bool` fields with defaults `false`.
3. The group path starts runs from more than one place: put the check in the one function all of them call; if there is none, add `fn may_start_from_group(&self) -> Result<(), &'static str>` and call it at each start.

---

## Slice T3 - Potato Telegram link: updates -> admit -> `send_outside`

### Goal
Poll Telegram for the Potato bot's own token, turn each update into an `outside::admit` input, and send admitted messages to `Service::send_outside` with `Surface::Telegram`.

### Read first
- `packages/app/src-tauri/src/channels/telegram/api.rs`, `api/wire.rs` (types for `Update`, `Message`, `getUpdates`), `poller.rs` (+ `poller/tests.rs`).
- `packages/app/src-tauri/src/potato/outside/{config,admit,engine}.rs` (plan 11).
- `packages/app/src-tauri/src/potato/discord/config.rs` and `bridge.rs` (plan 11 D5, D9): copy the structure.
- `packages/app/src-tauri/src/secrets.rs` l.31-45 (`SIDECAR_SECRETS`, `is_ui_secret`).

### Files
- create `packages/app/src-tauri/src/potato/telegram/mod.rs`
- create `packages/app/src-tauri/src/potato/telegram/link.rs`
- create `packages/app/src-tauri/src/potato/telegram/link_tests.rs`
- edit `packages/app/src-tauri/src/secrets.rs` (allow UI to set `potato_telegram_bot_token`; NOT a sidecar secret)
- edit `packages/app/src-tauri/src/potato/live.rs` (start/stop the poller thread with `telegram_labs_on && cfg.on && token`)

### Steps
1. `potato/telegram/mod.rs`: `pub mod link;` and `TelegramConfig` (same shape as `DiscordConfig` without guilds: `on`, `bot_user_id`, `bot_username`, `outside: OutsideConfig`, `last_dm_from: Option<(String, String)>`), stored in `<app data>/potato/telegram.json` with `load`/`save` like D5.
2. `link.rs`: `to_incoming(update, bot_id, bot_username) -> Option<IncomingOwned>` (pure) and `TelegramLink` that runs the existing poller with the Potato token (NEEDS-FACT: whether `poller.rs` can be instantiated twice with different tokens and sinks).
3. For each admitted message: `send_chat_action typing` (T4), then `service.send_outside(bot, text, OutsideOrigin { surface: Telegram, conversation_id, trust, addressed, target: { surface: Telegram, place_id: chat_id, message_id }, author_name })`.
4. Remember the last private-chat author (`last_dm_from`) so the settings page can offer "This is me" (T5).
5. Refuse to start if the Potato token equals the Tag Your Agent token (`getUpdates` on one token from two pollers gives Telegram error 409): status `"POTATO_TELEGRAM_SAME_TOKEN: Use a different bot for Potato Bot than for Tag Your Agent."`.
6. Tests with fixture JSON.

### Code
```rust
pub const TOKEN: &str = "potato_telegram_bot_token";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingOwned {
    pub author_id: String, pub author_name: String, pub author_is_bot: bool,
    pub chat_id: String, pub is_private: bool, pub thread_id: Option<String>,
    pub message_id: String, pub text: String, pub mentions_bot: bool, pub replies_to_bot: bool,
}

/// Pure. Private chat -> place None (a DM). Group/supergroup -> place = chat id; forum topic -> place = "<chat>:<topic>",
/// parent = chat id. Mentions: an entity of type "mention" equal to "@<bot_username>" (any case) or a
/// "text_mention" whose user id is the bot. Text from `text` or `caption`, capped at 4000 chars.
pub fn to_incoming(update: &serde_json::Value, bot_id: &str, bot_username: &str) -> Option<IncomingOwned>;

impl IncomingOwned {
    pub fn as_incoming(&self) -> crate::potato::outside::admit::Incoming<'_>;
}
```
`as_incoming` sets `surface: Surface::Telegram`, `place_id: if is_private { None } else { Some(<place>) }`,
`parent_place_id: thread_id.map(|_| chat_id)`.

Note: Telegram ids are numbers that can be negative (groups: `-100...`). `outside::config::is_id` already accepts a
leading `-` (plan 11 D2 test `is_id_accepts_telegram_negative_ids`). Forum places `"<chat>:<topic>"` contain `:`,
which `is_id` rejects: picked places are whole chats only; topics inherit their chat's mode through `parent_place_id`.

### Tests to write (`link_tests.rs`; fixtures: reuse `channels/telegram/fixtures/group_mention.json` and `group_reply_to_bot.json`, plus inline JSON)
| Test | Input | Expected |
|---|---|---|
| `group_mention_fixture` | `group_mention.json`, bot username from the fixture | `mentions_bot true`, `is_private false`, place = chat id |
| `group_reply_fixture` | `group_reply_to_bot.json` | `replies_to_bot true` |
| `private_chat_is_dm` | private chat update | `as_incoming().place_id == None` |
| `bot_author_flagged` | `from.is_bot true` | `author_is_bot true` |
| `caption_used` | photo with caption | text = caption |
| `edited_message_ignored` | `edited_message` update | `None` |
| `forum_topic_parent` | `message_thread_id 7` in chat `-100123` | place `"-100123:7"`, parent `"-100123"` |
| `admit_owner_dm` | private chat from owner id with `OutsideConfig` owner | `Admit::Yes` conv `telegram:dm:<id>` |
| `same_token_refused` | Potato token == Tag token | start returns `Err` starting `POTATO_TELEGRAM_SAME_TOKEN:` |
| `token_not_sidecar_secret` (secrets tests) | `SIDECAR_SECRETS` | does not contain `potato_telegram_bot_token` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T3 cargo test --lib potato::telegram
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T3 cargo test --lib secrets
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T3 cargo test --lib channels
```
Expected: `test result: ok. 9 passed` for the first; 0 failed for the others.

### Done when
- [ ] Potato Bot has its own Telegram token and poller, separate from Tag Your Agent.
- [ ] Every Telegram turn goes through `outside::admit` and `send_outside` (Sandbox only).

### Do NOT
- Do not give the Telegram token to any child process (it stays in Rust).
- Do not reuse the Tag Your Agent token or config file.
- Do not log message text; log chat id and the `Drop` name only.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. The poller is hard-wired to Tag Your Agent's engine: add a sink trait to `poller.rs` (`trait UpdateSink { fn on_update(&self, u: &serde_json::Value); }`) with the existing engine as one impl and `TelegramLink` as the second.
2. `is_ui_secret` refuses the new name: add it to the UI list (not to `SIDECAR_SECRETS`).
3. Fixtures do not have the bot username: read it from the fixture's entity text and pass it to `to_incoming`.

---

## Slice T4 - Replies out: `sendMessage`, typing, rate limits

### Goal
Send Potato replies back to Telegram as plain text, split to Telegram's limit, with typing and one retry after a 429.

### Read first
- `packages/app/src-tauri/src/channels/telegram/api.rs` (`sendMessage` wrapper if any; HTTP client).
- `packages/app/src-tauri/src/potato/outside/engine.rs` (`OutsideSink`).

### Files
- create `packages/app/src-tauri/src/potato/telegram/send.rs`
- create `packages/app/src-tauri/src/potato/telegram/send_tests.rs`
- edit `packages/app/src-tauri/src/potato/telegram/mod.rs`
- edit `packages/app/src-tauri/src/potato/live.rs` (register `TelegramSink` next to `DiscordSink` in a combined sink that routes by `target.surface`)

### Steps
1. `split_for_telegram(text) -> Vec<String>`: max 4096 chars per part, max 3 parts, same rules as Discord's `splitForDiscord` (paragraph, line, space, hard cut, ellipsis).
2. `send_body(chat_id, text, reply_to, thread_id) -> serde_json::Value` (Code).
3. `TelegramSink: OutsideSink`: queue + one worker thread; for each item call `sendMessage`; on 429 read `parameters.retry_after` (cap 10 s), wait, retry once; on other errors record `SendFailed` in the Telegram recent list.
4. `typing`: `sendChatAction` with `action: "typing"`.
5. Combined sink routes `Surface::Discord` -> `DiscordSink`, `Surface::Telegram` -> `TelegramSink`.

### Code
```rust
pub const MAX_LEN: usize = 4096;
pub const MAX_PARTS: usize = 3;

pub fn split_for_telegram(text: &str) -> Vec<String>;

/// Plain text only: no parse_mode, so the bot's words can't become links or formatting tricks.
pub fn send_body(chat_id: &str, text: &str, reply_to: Option<&str>, thread_id: Option<&str>) -> serde_json::Value {
    let mut b = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "link_preview_options": { "is_disabled": true },
    });
    if let Some(r) = reply_to {
        b["reply_parameters"] = serde_json::json!({ "message_id": r.parse::<i64>().unwrap_or(0), "allow_sending_without_reply": true });
    }
    if let Some(t) = thread_id { b["message_thread_id"] = serde_json::json!(t.parse::<i64>().unwrap_or(0)); }
    b
}
```
For a forum place `"<chat>:<topic>"`, the sink splits it into `chat_id` and `thread_id` before `send_body`.

### Tests to write (`send_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `short_one_part` | `"hi"` | `["hi"]` |
| `caps_three_parts` | 20 000 chars | 3 parts, each `<= 4096` chars, last ends with `…` |
| `body_has_no_parse_mode` | any | JSON has no `parse_mode` key |
| `body_disables_previews` | any | `link_preview_options.is_disabled == true` |
| `reply_only_first_part` | 2 parts, reply `55` | first body has `reply_parameters.message_id 55`, second none |
| `forum_place_split` | place `"-100123:7"` | `chat_id "-100123"`, `message_thread_id 7` |
| `retry_once_on_429` | fake HTTP 429 `{"parameters":{"retry_after":1}}` then 200 | 2 calls, waited 1 s (fake clock) |
| `combined_sink_routes_by_surface` | targets Discord and Telegram | each sink got exactly its own |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T4 cargo test --lib potato::telegram
```
Expected: `test result: ok. 17 passed` (9 from T3 + 8), 0 failed.

### Done when
- [ ] Replies reach the right chat (and topic) as plain text; long replies split; 429 handled.

### Do NOT
- Do not set `parse_mode`. Do not enable link previews.
- Do not retry more than once.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Char vs byte length: Telegram counts UTF-16 units; split at 4000 chars to stay safe if a test with emoji fails, and say so in the commit.
2. Worker thread blocks shutdown: use the same stop signal as other Potato threads (`stop_all`).
3. `reply_parameters` not supported by the existing wrapper: build the JSON body directly with the HTTP client used in `api.rs`.

---

## Slice T5 - Settings page with BotFather steps

### Goal
A Settings sub-page "Potato Bot on Telegram" that walks the person through BotFather, the token, adding the bot to a group, picking chats and people, and turning it on.

### Read first
- `packages/app/src/components/settings/potato/PotatoDiscordPage.tsx` and `lib/potato-discord-copy.ts` (plan 11 D10): copy the structure.
- `packages/app/src/components/settings/blocks/TelegramTagBot.tsx` and `lib/telegram-api.ts` (existing Telegram UI style).

### Files
- create `packages/app/src-tauri/src/commands/potato_telegram.rs` (commands mirroring plan 11 D6: `get`, `check`, `save`, `set_on`, `recent`, plus `potato_telegram_claim_last_dm()`) + register in `packages/app/src-tauri/src/lib.rs`
- create `packages/app/src/lib/potato-telegram.ts` (transport + fake + copy)
- create `packages/app/src/components/settings/potato/PotatoTelegramPage.tsx`
- create `packages/app/src/components/settings/potato/PotatoTelegramPage.test.tsx`
- edit `packages/app/src/components/settings/potato/PotatoSubPages.tsx`

### Steps
1. Commands: `potato_telegram_check` calls `getMe` with the stored token and saves `bot_user_id`, `bot_username`. `potato_telegram_claim_last_dm` adds `last_dm_from.0` to `owner_ids` and clears `last_dm_from`.
2. Chats list: the page shows chats the bot has seen (groups from updates, kept in config as `seen_chats: Vec<{id, title}>`, max 50) with the 3-way mode control.
3. Copy and page (Code).
4. Tests with the fake.

### Code
```ts
export const TG_COPY = {
  title: 'Potato Bot on Telegram',
  intro: 'Let one of your bots answer on Telegram. Everything it does for Telegram happens on its own computer, never on this PC.',
  needsLabs: 'Turn on "Potato Bot on Telegram" in Labs first.',
  needsSandbox: "Telegram needs Fish's computer. Set it up first in Settings > Potato Bot > Fish's computer.",
  step1Title: '1. Make a bot with BotFather',
  step1Lines: [
    'Open Telegram and search for @BotFather. Check it has the blue tick.',
    'Send /newbot.',
    'Send a name for your bot, for example Fish.',
    'Send a username that ends in bot, for example ren_fish_bot.',
    'BotFather sends you a token. Copy it.',
  ],
  step1Note: 'Use a new bot just for Potato Bot. Do not reuse the bot you made for Tag Your Agent.',
  step2Title: '2. Paste the bot token',
  tokenLabel: 'Bot token',
  tokenHelp: "Puppet Master keeps the token in your computer's password store. It is only sent to Telegram.",
  tokenSave: 'Save token',
  checkButton: 'Check token',
  checkOk: (u: string) => `Token works. Your bot is @${u}.`,
  sameToken: 'Use a different bot for Potato Bot than for Tag Your Agent.',
  step3Title: '3. Tell the bot who you are',
  step3Lines: ['In Telegram, open your bot and send /start.', 'Then come back here.'],
  lastDm: (name: string) => `Last message came from ${name}.`,
  claim: 'This is me',
  claimed: 'Saved. The bot knows you now.',
  step4Title: '4. Add the bot to a group (optional)',
  step4Lines: [
    'Open the group, tap its name, then Add members.',
    'Search for your bot\'s username and add it.',
    'Send a message in the group that starts with @ and the bot\'s username, so the bot sees the group.',
  ],
  privacyNote: 'Telegram only shows a bot the messages that call it. To use Every message, send /setprivacy to @BotFather, pick your bot, then Disable.',
  step5Title: '5. Pick where the bot may talk',
  noChats: 'No groups yet. Add the bot to a group and call it once.',
  step6Title: '6. Who may talk to the bot',
  step7Title: '7. Limits',
  step8Title: '8. Turn it on',
  onLabel: 'Answer on Telegram',
} as const;
```
The mode control, people choices, DM choices, limits text, status words and Recent list reuse the strings in
`lib/potato-discord-copy.ts` (`modeOff`, `modeWhenCalled`, `modeEvery`, `peopleOnlyMe`, `peopleListed`, `peopleAnyone`,
`dmsOnlyMe`, `dmsSame`, `dmsOff`, `anyoneWarning`, `perPersonHelp`, `perPlaceHelp`, `costNote`, `recentLine`). The
Discord-only line `myIdHelp` is not used; Telegram uses the "This is me" claim instead.

### Tests to write (`PotatoTelegramPage.test.tsx`)
| Test | Action | Expected |
|---|---|---|
| `labs message` | `labsOn false` | `Turn on "Potato Bot on Telegram" in Labs first.` |
| `token saved to credential store` | type token, `Save token` | `secrets.set('potato_telegram_bot_token', ...)`; field cleared |
| `check shows username` | `Check token` | `Token works. Your bot is @ren_fish_bot.` |
| `same token message` | fake check rejects with `POTATO_TELEGRAM_SAME_TOKEN: ...` | `Use a different bot for Potato Bot than for Tag Your Agent.` |
| `claim last dm` | fake `lastDm` = Ren, tap `This is me` | transport `claimLastDm()` called; `Saved. The bot knows you now.` |
| `pick a group` | fake seen chat `Family`, tap `When called` | saved `outside.places` has that chat with `when_called` |
| `turn on` | toggle `Answer on Telegram` | `setOn(true)` |

### Run
```
cd packages/app && npx vitest run src/components/settings/potato/PotatoTelegramPage.test.tsx
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-T5 cargo test --lib commands::potato_telegram
```
Expected: `Tests  7 passed (7)`; Rust 0 failed.

### Done when
- [ ] A person can set up Potato Bot on Telegram from the page alone, following the words on screen.
- [ ] Monochrome; preview-checked off the primary screen.

### Do NOT
- Do not show the token back. Do not put ids in URLs.
- Do not add a new top-level Settings section.

### If it fails
1. `secrets.set` refuses the name: T3 step allows it in `is_ui_secret`; merge T3 first.
2. Shared copy import cycle: import the shared strings from `potato-discord-copy.ts` only (not the page).
3. `seen_chats` grows without limit: cap at 50 in Rust `cleaned()`.

---

## Security

| Threat | What can happen | Defence | Test |
|---|---|---|---|
| Phone surface leaks settings or secrets | Key mask or paths exposed | Phone state is a reduced struct; no settings/secret routes | `state_has_no_key_mask` (P1) |
| Phone gets new powers through Potato routes | Pairing or settings changed from a phone | Only 4 routes; pairing has no phone route; guards green | `unknown_route_404` (P1), p2p/desk guard runs |
| Phone takes control without the owner | Covered by plan 13 S2-S4 | Desktop yes | `needs desktop yes` (P3) + plan 13 tests |
| Telegram group starts a coding agent on the PC (Tag Your Agent) | Remote code on the PC | Sandbox-only check before any run | `group_run_refused_without_sandbox`, `group_run_never_local` (T2) |
| Stranger in a Telegram group drives Potato | PC access, cost | `outside::admit` + `send_outside` (Sandbox only, rate limits) | `admit_owner_dm` (T3) + plan 11 D3/D4 tests |
| Two pollers on one token | Missed messages, confusing errors | Same-token refusal | `same_token_refused` (T3) |
| Telegram token leaks | Bot takeover | OS store only; never a sidecar secret; never shown | `token_not_sidecar_secret` (T3), `token saved to credential store` (T5) |
| Bot output turned into links/formatting tricks | Phishing in a group | No `parse_mode`; previews off | `body_has_no_parse_mode`, `body_disables_previews` (T4) |
| Someone else claims to be the owner | Owner trust given to a stranger | "This is me" is a desktop-only action on the last private-chat author, shown by name before tapping | `claim last dm` (T5); no bridge route for claim |

## What cannot be verified without real hardware or a second computer

- The phone surface on a real phone on mobile data (latency, the 4 s polling, battery).
- Phone control of the real Sandbox screen with touch input and `mobile-keyboard.js`.
- Real Telegram delivery: privacy mode behaviour in groups, forum topics, 429 limits, BotFather steps as Telegram
  shows them today.
- A second Telegram account to test "Me and people I list" and "Anyone".

## Open facts

- NEEDS-FACT: how a phone request is authenticated and the session type name - read `packages/app/src-tauri/src/bridge/routes/potato_screen.rs` and `packages/app/src-tauri/src/bridge/http.rs`.
- NEEDS-FACT: the file that declares bridge route modules - list `packages/app/src-tauri/src/bridge/routes/`.
- NEEDS-FACT: whether `Origin::Phone` exists and the `Facts` field names for surface and trust - read `packages/app/src-tauri/src/potato/service.rs` l.680-830 and `packages/app/src-tauri/src/potato/rules.rs`.
- NEEDS-FACT: the phone web app folder, router and authenticated fetch helper - search `packages/app/src` for `className="ph` and the phone entry.
- NEEDS-FACT: the name of the Tag Your Agent token secret - read `packages/app/src-tauri/src/channels/tag_config.rs` and `setup_tag.rs`.
- NEEDS-FACT: where the Tag Your Agent group path starts an agent run and where it runs (local pane or other) - read `packages/app/src-tauri/src/channels/engine/group.rs` and `engine.rs`.
- NEEDS-FACT: whether `telegram/poller.rs` can run twice with different tokens and sinks - read `packages/app/src-tauri/src/channels/telegram/poller.rs` and `poller/tests.rs`.
- NEEDS-FACT: the `Update`/`Message` types in `telegram/api/wire.rs` and the bot username inside the fixtures - read `packages/app/src-tauri/src/channels/telegram/api/wire.rs` and `telegram/fixtures/*.json`.
- NEEDS-FACT: `is_ui_secret` allow list - read `packages/app/src-tauri/src/secrets.rs` l.33-45.
