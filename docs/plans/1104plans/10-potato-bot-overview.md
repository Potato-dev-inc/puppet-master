# 10 - Potato Bot: overview and build order

Status: plan only. Written 2026-11-04. Inputs were ONLY `00-FORMAT.md` and `_facts/potato-bot-facts.md`.

## How these plans were written (and one rule that was bent)

`00-FORMAT.md` rule 4 says the writer reads the code before writing each step. The founder asked that no source code
be read for these plans. The founder wins. So:
- Every fact in plans 11-15 comes from `_facts/potato-bot-facts.md` (read on 2026-11-04 from `main`).
- Every slice has a `Read first` list for the BUILDER, who must read those files before changing anything and copy
  names and style from them.
- Where a slice needs a name the facts file does not have, the plan says `NEEDS-FACT: <what, which file>` and is written
  around it. All of them are listed at the end of each plan, and summarised at the end of this file.

## The plans

| File | What it builds | Slices |
|---|---|---|
| `11-potato-bot-discord.md` | Potato Bot on Discord, Sandbox only, with the invite steps | 11 (D1-D11) |
| `12-potato-bot-app-groups.md` | Groups of 2-10 bots in the app; `packages/potato-brain/src/rooms/` | 10 (G1-G10) |
| `13-potato-bot-sandbox-finish.md` | Phone control yes, installer check, live DeepSeek test, Local/Sandbox switch with copy-in and bring-back, stale doc fix | 11 (S1-S11) |
| `14-potato-bot-phone-and-telegram.md` | Phone surface; Potato Bot on Telegram; Sandbox-only for Tag Your Agent groups | 8 (P1-P3, T1-T5) |
| `15-potato-bot-personas-and-polish.md` | Fish licence check, Fish/Potato persona, Rakazo parity items | 12 (F1, F2, R1-R10) |
| **Total** | | **52 slices** |

## Founder decisions and where each one lands

| Decision | Where |
|---|---|
| App-first | The build order below does app work (Sandbox finish, phone, groups, polish) before Discord and Telegram. Nothing in the app needs a chat app. |
| Discord optional | Behind Labs flag `potato_discord_enabled`, off by default (11 D1). |
| Talk brain default = DeepSeek API, key in the OS credential store | Already true in the facts (`deepseek_api_key` via `keyring`, given to the brain only as env `DEEPSEEK_API_KEY`). 13 S7 adds the opt-in live test. No plan changes the default brain. |
| Default persona Fish; its licence check heads the release checklist | 15 F1 (checklist item 1 + test that keeps it first), 15 F2 (`fish` default). |
| Anything triggered from outside (Discord, Telegram, groups with other people) runs in the Sandbox only | Shared module `potato/outside/` (11 D2-D4): `policy::check` runs before any other rule; outside turns get only the 6 Sandbox tools. Used by Discord (11), Telegram (14 T3), Tag Your Agent groups (14 T2), and future shared groups (12 G7). Hardened by 15 R6 (network) and R7 (actions). |
| Phone take-over needs a desktop yes the first time | 13 S2-S4 (allow-list, ask, card), 14 P3 (phone side). |
| Labs flags `potato_bot_enabled` and `potato_sandbox_enabled` stay off by default | No plan changes their defaults. 15 F1 adds a release checklist line to check a fresh install. |
| New Labs flags must join `DEV_LABS_KEYS` | Three new flags, each added to the Rust and TypeScript lists: `potato_discord_enabled` (11 D1), `potato_groups_enabled` (12 G1), `potato_telegram_enabled` (14 T1). 11 D1 adds a mirror test that fails if the two lists differ. |
| UI is monochrome | Every UI slice uses only existing tokens; 11 D10 has a test that no coloured class is used; 15 R1 uses line glyphs, not colour emoji. |

### Where the facts file and the founder disagree (founder followed)

1. Facts: helpers "start local agents" on the PC. Founder: outside triggers run in the Sandbox only. Result: outside
   turns never get `ask_helper` or the helper Start card (11 D4).
2. Facts: the theme has "one accent, Potato orange, used in 1-5% of places". Founder: monochrome. Result: no new UI
   uses the accent.
3. Founder: "Telegram is already merged". Facts: what is merged is Tag Your Agent (Telegram for coding agents), and
   Potato Bot has no Telegram path. Result: plan 14 builds Potato Bot on Telegram and applies the Sandbox-only rule to
   the merged Tag Your Agent group path (14 T2).
4. `00-FORMAT.md` rule 4 (writer reads code) vs the founder's "do not read code" (see top of this file).

## Done vs missing (from the facts file)

### Done today
| Area | Evidence (facts section) |
|---|---|
| Potato Bot engine, bots, DMs, brain process (lazy start, idle stop, crash backoff) | `potato/service.rs`, `keeper.rs`, `sidecar.rs` (1a, 2) |
| DeepSeek key in the OS store, key check, env-only hand-off, redaction in the brain | `secrets.rs`, `live.rs` `check_key`, `env.rs`, `pm.ts` (3) |
| Labs flags `potato_bot_enabled`, `potato_sandbox_enabled` and `DEV_LABS_KEYS` mirror | `live.rs` l.31/33, `settings_store.rs` l.13-21, `dev-labs.ts` (4) |
| Fish's computer (WSL 2 Sandbox): install wizard, lifecycle, 6 tools, browser tools, control lease, noVNC viewer, clipboard bridge, window guard | `potato/sandbox/*`, `sandbox_calls.rs`, `window_guard.rs` (6, 8) |
| Bot settings (instructions history, tools, export, delete), usage and cost, routines, artifacts, open work, notify, voice settings, trash, hold | `potato/*` (1a, 8) |
| Markdown memory, tool-loop guard, time anchor, stale-screen pruning | `potato-brain/src/memory`, `lifted/` (1b, 8) |
| Telegram Tag Your Agent (coding agents; group path default off) | `channels/*`, PR #198 (5a) |

### Missing or partial, and which slice builds it
| Missing / partial | Slice |
|---|---|
| Doc conflict: `potato-sandbox.md` l.8 says bot use and take-over come later | 13 S1 |
| Phone control yes (`PHONE_NEEDS_DESKTOP_YES` has no yes) | 13 S2-S4, 14 P3 |
| Installer has the brain and guest files (stated requirement, not checked) | 13 S5-S6 |
| Live DeepSeek test | 13 S7 |
| Local / Sandbox switch, copy-in, bring-back to Review Desk | 13 S8-S11 |
| Phone Potato surface (mobile is pairing only) | 14 P1-P3 |
| Group chats, bot-to-bot, handoff; `src/rooms/` does not exist | 12 G1-G10 |
| Discord: no code, only the fishd design | 11 D1-D11 |
| Potato Bot on Telegram; Sandbox-only for Tag Your Agent groups | 14 T1-T5 |
| Fish licence check, persona default | 15 F1-F2 |
| Egress restriction, shell/desktop command guard | 15 R6-R7 |
| Messaging commands | 15 R5 |
| Stuck-work reminders, duplicate/archive, reactions, settings from chat + undo, SSRF-safe fetch, change-watch routines, lazy tool catalog | 15 R3, R10, R1, R2, R8, R4, R9 |
| Semantic memory, webhook/Git/Slack triggers, voice calls, team vs private computer, teach a task, site trust, web search, MCP client, tickets board, media and plots | Not this round; each has a `DECISION-OPEN` default in plan 15's last table |

## Build order (all 52 slices, dependency order, app-first)

H = Haiku-sized, S = Sonnet-sized (per `00-FORMAT.md` sizing guide). "Needs" lists slices that must be merged first.

| # | Slice | Plan | Goal | Size | Needs |
|---|---|---|---|---|---|
| 1 | F1 | 15 | Release checklist headed by the Fish licence check | H | - |
| 2 | S1 | 13 | Fix the stale sentence in `potato-sandbox.md` | H | - |
| 3 | F2 | 15 | Persona per bot: Fish or Potato, default Fish | H | - |
| 4 | S5 | 13 | `resource_check.rs`: required brain and Sandbox files | H | - |
| 5 | S6 | 13 | `scripts/verify-potato-bundle.mjs` + checklist line | H | S5, F1 |
| 6 | S7 | 13 | Live DeepSeek tests (opt-in) | H | - |
| 7 | S2 | 13 | Phone control allow-list and decision (pure) | H | - |
| 8 | S3 | 13 | Phone control asks wired into the lease path | S | S2 |
| 9 | S4 | 13 | Desktop yes card + allowed phones list | H | S3 |
| 10 | P1 | 14 | Phone bridge routes for Potato | S | - |
| 11 | P2 | 14 | Phone transport, bot list, thread | S | P1 |
| 12 | P3 | 14 | Phone: watch and ask for control | H | P2, S3 |
| 13 | D1 | 11 | Labs flag `potato_discord_enabled` + DEV_LABS mirror test | H | - |
| 14 | G1 | 12 | Labs flag `potato_groups_enabled` | H | D1 |
| 15 | G2 | 12 | Rust room store | H | G1 |
| 16 | G3 | 12 | Brain rooms: types + addressing | H | - |
| 17 | G4 | 12 | Brain rooms: burst + silence budget | H | G3 |
| 18 | G5 | 12 | Brain rooms: gate chain | S | G4 |
| 19 | G6 | 12 | Brain rooms: transport, prompt, runner, channel wiring | S | G5 |
| 20 | G7 | 12 | Rust: `Pass` event, fan-out, hop limit | S | G2, G6 |
| 21 | G8 | 12 | Tauri commands + `lib/potato-rooms.ts` | H | G7 |
| 22 | G9 | 12 | UI: Groups in the list, New group sheet | H | G8 |
| 23 | G10 | 12 | UI: group thread | S | G9 |
| 24 | R3 | 15 | Stuck-work reminders | H | - |
| 25 | R10 | 15 | Duplicate and archive a bot | H | - |
| 26 | R1 | 15 | Reactions on bot messages | S | - |
| 27 | D2 | 11 | `potato/outside/` config + sandbox-only policy | S | D1 |
| 28 | S8 | 13 | Per-bot work place: Local or Sandbox | S | D2 |
| 29 | S9 | 13 | Copy-in: a Git folder into the Sandbox | S | S8 |
| 30 | S10 | 13 | Bring-back: branch into the PC repo + Review Desk | S | S9 |
| 31 | S11 | 13 | Thread cards for copy-in and bring-back | H | S10 |
| 32 | D3 | 11 | `potato/outside/` admit + rate book | S | D2 |
| 33 | D4 | 11 | `Origin::Outside`; brain honours `sandboxOnly` | S | D3 |
| 34 | R2 | 15 | Settings from chat + Undo card | S | F2, D4 |
| 35 | R8 | 15 | SSRF-safe `web_fetch` (app turns only) | S | D4 |
| 36 | R4 | 15 | Change-watch routines | S | R8 |
| 37 | R9 | 15 | Lazy tool catalog | S | R8 |
| 38 | R6 | 15 | Sandbox egress limit | S | - |
| 39 | R7 | 15 | Action guard for outside turns | H | D4 |
| 40 | D5 | 11 | Discord config store + setup calls | S | D2 |
| 41 | D6 | 11 | Discord Tauri commands + TS transport | H | D5 |
| 42 | D7 | 11 | Brain: Discord gateway client | S | - |
| 43 | D8 | 11 | Brain: REST sender + link entry | S | D7 |
| 44 | D9 | 11 | Rust: Discord bridge routes, outbox, link keeper | S | D4, D6, D8, R6 |
| 45 | D10 | 11 | Discord settings page with invite steps | S | D6, D9 |
| 46 | D11 | 11 | Discord recent activity + full-path fake test | H | D9, D10 |
| 47 | T1 | 14 | Labs flag `potato_telegram_enabled` | H | D1 |
| 48 | T2 | 14 | Tag Your Agent group path: Sandbox only | S | D2 |
| 49 | T3 | 14 | Potato Telegram link: updates -> admit -> `send_outside` | S | T1, D3, D4 |
| 50 | T4 | 14 | Telegram replies out | H | T3 |
| 51 | T5 | 14 | Telegram settings page with BotFather steps | H | T4 |
| 52 | R5 | 15 | Messaging commands on Discord and Telegram | H | D9, T3 |

Sizes: 26 Haiku, 26 Sonnet. Slices with no "Needs" can run in parallel (at most 3 at a time, per the usage rule in
memory); each uses its own `CARGO_TARGET_DIR` (`D:/pm-night/target-<slice>`).

Labs flag list order: the three new keys go into `DEV_LABS_KEYS` in the order their slices merge (D1, then G1, then
T1). Only "same order in Rust and TypeScript" matters; the mirror test checks it.

### Gates before anything faces the outside world
Discord (D9 onwards) and Potato Telegram (T3 onwards) must not be switched on by any person until all of these are
merged and green: D2, D3, D4 (Sandbox-only policy), R6 (egress limit), R7 (action guard), F1 (Fish licence record
exists; the founder signs it before a release).

## Rules every slice copies (from `00-FORMAT.md`)
- Never run a dev server or the app on the primary screen; use the preview tools or a headless run.
- Separate `CARGO_TARGET_DIR` per slice, never the default one.
- Never kill a process by image name; stop only a PID you started.
- New Labs flags must also be added to `DEV_LABS_KEYS`.
- `p2p/guard_tests.rs` and `desk/guard_tests.rs` stay green; only `iroh_net.rs` touches the network in `p2p/`; p2p code
  prints nothing and reads no env; pairing has no route from agents/MCP/phone.
- Team traffic never uses the bridge or device pairing.
- UI is monochrome. Commit finished work; every handoff has a fresh build id.

## Test commands used across the plans (all from the facts file, section 9)
- App (vitest): `cd packages/app && npx vitest run <path>`; folder runs `src/components/cockpit/potato`,
  `src/components/settings/potato`, `src/components/settings/sandbox src/lib/potato-sandbox.test.ts`, `src/lib/potato`,
  `src/styles/cockpit-potato.css.test.ts`.
- Brain: `cd packages/potato-brain && node --test test/<file>.test.ts`; all: `node --test test/*.test.ts`; types:
  `npm run typecheck`.
- Rust: `cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-<slice> cargo test --lib <prefix>`
  with prefixes such as `potato`, `potato::sandbox`, `settings_store`, `secrets`.

## Security (whole programme)

| Threat | Defence | Proven by |
|---|---|---|
| Someone outside this PC (Discord, Telegram, a Telegram group asking Tag Your Agent) gets code or files on the PC | One shared policy, checked first: outside turns run only with the 6 Sandbox tools; refused if the Sandbox is off | 11 `outside_refused_when_sandbox_off`, `outside_item_is_sandbox_only`, `sandboxOnly keeps exactly the six sandbox tools`; 14 `group_run_refused_without_sandbox`, `group_run_never_local` |
| The Sandbox becomes a door into the home network | nftables egress drop for private ranges and the host; start fails closed | 15 `blocks_private_v4`, `dns_allowed_before_drop` |
| Strangers drive risky actions inside the Sandbox | Action guard on outside turns | 15 `outside_turn_guarded` |
| Floods, loops and cost blow-ups | Rate book (6/5 min person, 20/10 min place), daily cost limit, bots ignore bots, group hop limit 3, silence budget, first-responder rule | 11 `stranger_rate_limited_after_6`, `drops_bots_first`; 12 `room_reply_chain_stops_at_hop_3`, `only first responder answers open question` |
| Tokens and keys leak | OS credential store only; env-only hand-off to the one process that needs it; never in errors, logs, UI, URLs | 11 `error_text_never_has_token`; 14 `token_not_sidecar_secret`; 13 S7 `Do NOT` |
| Phone takes control without the owner | First control needs a yes on this PC; device id from the session; no bridge route to answer | 13 `unknown_phone_asks`, `phone_device_id_not_from_body` |
| Work from the Sandbox changes the PC repo | Bundle verify, fetch into a new branch only, strict branch names, no checkout | 13 `bad_bundle_refused`, `fetch_never_checks_out`, `valid_branch_rejects_others` |
| SSRF from app chats | Rust-side DNS + IP checks, pinned connect, redirect re-checks | 15 `rejects_loopback`, `rejects_private_and_cgnat`, `rejects_mapped_v6` |
| Shipping a persona without the right to | Checklist item 1, record must be signed | 15 `starts with the Fish licence check` |

## What cannot be verified without real hardware or a second computer
- Real Discord and Telegram sessions (gateway resume, close codes, privacy mode, rate limits, invite and BotFather
  screens as they look today), and a second account to test "people I list" and "anyone".
- The WSL 2 Sandbox on a real PC: egress rules, DNS, screen proxy, copy-in/bring-back speed and CRLF behaviour.
- A paired phone on another network controlling the Sandbox after the desktop yes.
- The release installer containing every Potato file (needs a release build).
- Live DeepSeek turns (needs the person's key; real cost) and how groups of bots sound with a real model.
- The Fish licence result (a person must check and sign).

## Open facts (summary; each plan lists its own in full)
- NEEDS-FACT: the default value site of `potato_bot_enabled` / `potato_sandbox_enabled` in the settings store (facts 11) - read `packages/app/src-tauri/src/settings_store.rs` beyond l.60 and the public settings struct.
- NEEDS-FACT: `rules::Facts`, `Origin`, inbox item JSON shape and brain event field names - plans 11, 12, 14.
- NEEDS-FACT: `Host` methods for Sandbox state, `store.rs` helpers, `keeper::backoff_ms`, `script_path` release name - plans 11, 12, 13.
- NEEDS-FACT: bridge path guard lines, brain/link token header, phone session type, route module file - plans 11, 13, 14.
- NEEDS-FACT: `trait Sandbox` methods for guest commands and file copy; screen connection direction; distro packages - plans 13, 15.
- NEEDS-FACT: Tag Your Agent token name, group run start point, poller reuse - plan 14.
- NEEDS-FACT: persona values today, pack files and sources, release checklist location, Review Desk entry point - plans 13, 15.
