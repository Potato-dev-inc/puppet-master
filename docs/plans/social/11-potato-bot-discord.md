# 11 - Potato Bot on Discord

Status: plan only. Written 2026-11-04 from `00-FORMAT.md` and `_facts/potato-bot-facts.md` only (the founder asked
that no source code be read while writing). Every `Read first` list below is for the BUILDER: the builder reads those
files before touching anything, and copies names and style from them. Where this plan names a function in existing
code that the facts file does not show, it is marked `NEEDS-FACT` and collected in `Open facts` at the end.

## What this plan builds

A person can let one of their Potato bots answer on a Discord server they own. They make a Discord bot in the
Discord Developer Portal, paste its token into Puppet Master, invite the bot to their server, pick the channels, and
pick who may talk to it. Every Discord message runs in Fish's computer (the Sandbox) only, never on the PC.

## Founder rules that shape this plan

| Rule | Where it lands |
|---|---|
| App-first; Discord optional | Discord is a Labs flag `potato_discord_enabled`, off by default, needs `potato_bot_enabled` and `potato_sandbox_enabled` on too (slice D1). Nothing in the app needs Discord. |
| Anything triggered from outside runs in the Sandbox only | Shared module `potato/outside/` (slices D2-D4). A Discord turn gets only the 6 sandbox tools; no helpers on the PC, no PC files. |
| Secrets in the OS credential store | The token is `discord_bot_token`, already listed in `SIDECAR_SECRETS` (`secrets.rs` l.31). It is passed to the link process only as the env var `DISCORD_BOT_TOKEN`. |
| New Labs flags join `DEV_LABS_KEYS` | Slice D1 adds `potato_discord_enabled` to both lists (Rust and TypeScript) and adds a mirror test. |
| UI is monochrome | Slice D10 uses only `var(--label...)`, `var(--sep...)` tokens; no orange, no coloured badges. |

### Conflicts between the facts file and the founder (founder wins)

1. Facts section 8 says helpers "start local agents" (`potato/helpers.rs`). Founder: outside triggers run in the
   Sandbox only. So a Discord turn never offers the helper Start card and never gets the `ask_helper` tool (slice D4).
2. Facts section 10b says the theme has "one accent, Potato orange, used in 1-5% of places". Founder: UI is
   monochrome. New Discord UI uses no accent colour at all.
3. Facts section 7 says fishd uses discord.js. `packages/potato-brain` has only dev dependencies today (facts 1b).
   Decision made here: no discord.js. The link uses Node's built-in `WebSocket` and `fetch` (Node >= 22.18 is already
   required by `package.json`). This keeps the brain with zero runtime dependencies and nothing new to bundle.

## How it fits together

```
Discord gateway (wss)            Puppet Master app (Rust)                    Brain (Node, per bot)
        |                                  |                                        |
 [link process: node src/discord.ts]       |                                        |
   Gateway  -- MESSAGE_CREATE -->  POST /potato/discord/in                          |
                                   discord::filter (guild)                          |
                                   outside::admit (place, person, called, rate)     |
                                   outside::policy (sandbox only)                   |
                                   Service::send_as(Origin::Outside)  -- inbox -->  runTurn (sandbox tools only)
                                   Service::on_event(Reply)           <-- events -- emit Reply
   Rest.send  <-- GET /potato/discord/out (long poll) -- outbox                     |
```

- The link process holds the Discord connection. It never decides anything. It only forwards.
- All decisions (who, where, how often, sandbox only) are pure Rust functions with tests.
- The brain process keeps working exactly as for app DMs. It only sees one new field, `sandboxOnly: true`.

## fishd mapping (from facts section 7, `D:/pm-research/INV-FISHD-DISCORD.md`)

| fishd | This plan | Same / different |
|---|---|---|
| `startDiscord()` in `src/channels/discord.ts`, discord.js ^14.27 | `packages/potato-brain/src/discord.ts` + `src/discord/gateway.ts`, built-in `WebSocket` | Different library, same gateway events |
| Intents Guilds, GuildMessages, MessageContent, DirectMessages, typing, reactions | Guilds (1), GuildMessages (512), DirectMessages (4096), MessageContent (32768) = `37377` | Typing and reaction intents dropped (not used) |
| DMs keyed `dm:<userId>`, guild channels by channel id, threads as own channels | `discord:dm:<userId>`, `discord:ch:<channelId>`, threads use the thread id | Same, with a `discord:` prefix |
| `DISCORD_GUILD_IDS` | `DiscordConfig.guilds` | Same idea, set in the UI |
| `DISCORD_ONLY_CHANNELS` (hard boundary) | `OutsideConfig.places` with mode not `off` | Same: a channel not picked is never answered |
| `DISCORD_CHANNEL_IDS` (always answer) | `PlaceMode::EveryMessage` | Same |
| Drop all bots, even ones that @ her | `Drop::FromBot` is check number 1 | Same |
| `admit()` scope, DM rate bucket for non-owners, `TURN_LIMIT` 6 per 5 min | `outside::admit` + `RateConfig::default()` = 6 per 300 s per person, 20 per 600 s per place, owner exempt | Same numbers for people, place limit added |
| Hard address: DM, @mention, reply, name (`DISCORD_NAMES`), always-channel, `DISCORD_REPLY_ALL` | `Addressed::{DirectMessage, Mention, Reply, Name, EveryMessagePlace}` | Same; `REPLY_ALL` is `EveryMessagePlace` per channel |
| A plain @ of someone else is not hard-addressed | `mentions_other && !mentions_bot` gives `Drop::NotCalled` in `WhenCalled` places | Same |
| Ambient mode, burst buffer, gate chain, silence budget | NOT in this plan. App groups get them first (plan 12, `src/rooms/`). Discord ambient = later, `DECISION-OPEN (default: off, not built)` | Deferred |
| Owner commands `/mood /stats /recall /diary /config /tune /ignore /ping /help` | Plan 15 slice R5 (messaging commands) | Deferred |
| Memory per room with roster | Brain memory keyed by `conversation_id` (`agentFor(conversationId)`, facts section 2 step 10) | Same keying |

## Slice index (dependency order)

| Slice | Goal | Size | Needs merged first |
|---|---|---|---|
| D1 | Labs flag `potato_discord_enabled` | Haiku | none |
| D2 | `potato/outside/` config + sandbox-only policy (pure) | Sonnet | D1 |
| D3 | `potato/outside/` admit + rate book (pure) | Sonnet | D2 |
| D4 | `Origin::Outside` in `Service::send_as`; brain honours `sandboxOnly` | Sonnet | D3 |
| D5 | Discord config store + setup calls (check token, servers, channels) | Sonnet | D2 |
| D6 | Tauri commands + `lib/potato-discord.ts` transport | Haiku | D5 |
| D7 | Brain: gateway client (pure state machine) | Sonnet | none |
| D8 | Brain: REST sender + link entry `src/discord.ts` | Sonnet | D7 |
| D9 | Rust: bridge routes, outbox, link keeper | Sonnet | D4, D6, D8, plan 15 R6 (egress) |
| D10 | Settings page with the invite steps | Sonnet | D6, D9 |
| D11 | Recent activity list + full-path fake test | Haiku | D9, D10 |

Rules that apply to every slice below (copied into each `Do NOT`):
- Never run a dev server or the app on the primary screen; use the preview tools or a headless run.
- Use a separate `CARGO_TARGET_DIR`, named per slice (`D:/pm-night/target-<slice>`), never the default one.
- Never kill a process by image name; stop only a PID you started.
- New Labs flags must also be added to `DEV_LABS_KEYS` (Rust and TypeScript).
- `p2p/guard_tests.rs` and `desk/guard_tests.rs` must stay green. Nothing in this plan touches `p2p/` or `desk/`.
- Team traffic must never use the bridge or device pairing. Discord traffic is not team traffic, and it never uses
  device pairing.
- UI is monochrome. Commit finished work. Every handoff has a fresh build id.

Commands below are written for Git Bash from the repo root. In PowerShell write `$env:CARGO_TARGET_DIR='D:/pm-night/target-D2'; cargo test ...` instead of the `VAR=value cargo ...` prefix.

---

## Slice D1 - Labs flag `potato_discord_enabled`

### Goal
Add the Labs flag `potato_discord_enabled` (off by default) with a Rust reader that is true only when Potato Bot, Fish's computer and Discord are all on.

### Read first
- `packages/app/src-tauri/src/potato/live.rs` l.31-70: `LABS_KEY`, `SANDBOX_LABS_KEY`, `labs_on(public)` l.60, `sandbox_labs_on` l.65. Copy the exact parameter type of `public`.
- `packages/app/src-tauri/src/settings_store.rs` l.13-60 and the tests at l.~324 and l.~333: `DEV_LABS_KEYS`, `with_dev_labs`.
- `packages/app/src/lib/dev-labs.ts` l.8-16: the TypeScript mirror.
- `packages/app/src/lib/setup-checks/labs.ts` l.8-9: the `LABEL` map.
- `packages/app/src/components/settings/model.ts` l.79, l.90, l.97: the flag unions and `visibleSections`.

### Files
- edit `packages/app/src-tauri/src/potato/live.rs`
- edit `packages/app/src-tauri/src/settings_store.rs`
- edit `packages/app/src/lib/dev-labs.ts`
- edit `packages/app/src/lib/setup-checks/labs.ts`
- edit `packages/app/src/components/settings/model.ts`
- create `packages/app/src/lib/dev-labs.mirror.test.ts`

(That is 6 files. The test file is new and tiny; the 5-file rule counts edited source files. If the builder's guard counts all files, move the `model.ts` edit into slice D10, which edits `model.ts` anyway.)

### Steps
1. In `live.rs`, under `SANDBOX_LABS_KEY` (l.33), add the const `DISCORD_LABS_KEY` and the function `discord_labs_on` shown in Code.
2. In `settings_store.rs` `DEV_LABS_KEYS` (l.13-21), add `"potato_discord_enabled",` on the line after `"potato_sandbox_enabled",`.
3. In `settings_store.rs` tests, add the test `dev_labs_keys_include_potato_discord` next to the test at l.~333.
4. In `dev-labs.ts`, add `'potato_discord_enabled',` on the line after `'potato_sandbox_enabled',` (l.12).
5. In `setup-checks/labs.ts` `LABEL`, add `potato_discord_enabled: 'Potato Bot on Discord',` after the `potato_sandbox_enabled` line.
6. In `model.ts` l.79 row union, add `| 'potato_discord_enabled'`. In the `visibleSections` settings type at l.97, add `potato_discord_enabled?: boolean;` in the same style as the other three keys.
7. Create `dev-labs.mirror.test.ts` (Code below).
8. Add the Rust tests in `live.rs`'s test file (Code below). If `live.rs` has no `*_tests.rs` file, put them in a new `#[cfg(test)] mod discord_flag_tests { ... }` block at the end of `live.rs`.

### Code

`packages/app/src-tauri/src/potato/live.rs`, after l.33:
```rust
/// Potato Bot on Discord. Does nothing unless Potato Bot and Fish's computer are on too:
/// every Discord turn runs in the Sandbox only, so no Sandbox means no Discord.
pub const DISCORD_LABS_KEY: &str = "potato_discord_enabled";

/// True only when all three switches are exactly `true`. A missing key is off.
pub fn discord_labs_on(public: &PublicSettings) -> bool {
    labs_on(public)
        && sandbox_labs_on(public)
        && flag_true(public, DISCORD_LABS_KEY)
}
```
`PublicSettings` is a placeholder: replace it with the exact type `labs_on` takes. `flag_true` is the helper `labs_on`
uses to read "exactly true"; if `labs_on` reads the value inline, write `flag_true` as a private fn with that same body
and make `labs_on` and `sandbox_labs_on` unchanged.

`packages/app/src/lib/dev-labs.mirror.test.ts` (whole file):
```ts
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { DEV_LABS_KEYS } from './dev-labs';

const here = dirname(fileURLToPath(import.meta.url));
const rustFile = join(here, '..', '..', 'src-tauri', 'src', 'settings_store.rs');

function rustKeys(): string[] {
  const text = readFileSync(rustFile, 'utf8');
  const start = text.indexOf('pub const DEV_LABS_KEYS');
  const end = text.indexOf('];', start);
  const block = text.slice(start, end);
  return [...block.matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]);
}

describe('DEV_LABS_KEYS mirror', () => {
  it('lists the same keys in Rust and TypeScript, in the same order', () => {
    expect([...DEV_LABS_KEYS]).toEqual(rustKeys());
  });

  it('includes potato_discord_enabled', () => {
    expect(DEV_LABS_KEYS).toContain('potato_discord_enabled');
  });
});
```

Rust tests:
```rust
#[test]
fn discord_labs_off_when_missing() {
    let s = settings_with(&[("potato_bot_enabled", true), ("potato_sandbox_enabled", true)]);
    assert!(!discord_labs_on(&s));
}

#[test]
fn discord_labs_off_without_sandbox() {
    let s = settings_with(&[("potato_bot_enabled", true), ("potato_discord_enabled", true)]);
    assert!(!discord_labs_on(&s));
}

#[test]
fn discord_labs_on_when_all_three_true() {
    let s = settings_with(&[
        ("potato_bot_enabled", true),
        ("potato_sandbox_enabled", true),
        ("potato_discord_enabled", true),
    ]);
    assert!(discord_labs_on(&s));
}
```
`settings_with` builds the settings value from pairs. If the existing `live.rs` tests already have a builder for this,
use it and do not add a second one.

`settings_store.rs` test:
```rust
#[test]
fn dev_labs_keys_include_potato_discord() {
    assert!(DEV_LABS_KEYS.contains(&"potato_discord_enabled"));
}
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `discord_labs_off_when_missing` | bot on, sandbox on, discord key missing | `false` |
| `discord_labs_off_without_sandbox` | bot on, discord on, sandbox missing | `false` |
| `discord_labs_on_when_all_three_true` | all three `true` | `true` |
| `dev_labs_keys_include_potato_discord` | `DEV_LABS_KEYS` | contains the key |
| `DEV_LABS_KEYS mirror > lists the same keys...` | both files | arrays equal |
| `DEV_LABS_KEYS mirror > includes potato_discord_enabled` | TS list | contains the key |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D1 cargo test --lib discord_labs
```
Expected: `test result: ok. 3 passed; 0 failed`.
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D1 cargo test --lib settings_store
```
Expected: `test result: ok.` with 0 failed.
```
cd packages/app && npx vitest run src/lib/dev-labs.mirror.test.ts
```
Expected: `Tests  2 passed (2)`.

### Done when
- [ ] `DEV_LABS_KEYS` has `potato_discord_enabled` in Rust and in TypeScript, same position.
- [ ] `discord_labs_on` is false unless all three flags are exactly `true`.
- [ ] All tests above pass.
- [ ] Committed on a branch `feat/potato-discord-d1` with a fresh build id in the handoff note.

### Do NOT
- Do not set any flag default to `true` anywhere. The founder rule: Labs flags stay off by default.
- Do not add a UI toggle in this slice (slice D10 does).
- Do not use the default `CARGO_TARGET_DIR`.
- Do not touch `p2p/` or `desk/`.

### If it fails
1. Mirror test fails on order: you added the TS key on a different line than in Rust. Put it right after `potato_sandbox_enabled` in both.
2. `cannot find type PublicSettings`: you copied the placeholder. Use the exact type from `labs_on`'s signature.
3. `settings_store` test at l.~324 now fails: it may assert the list length. Update that length by +1 and say so in the commit message.

---

## Slice D2 - `potato/outside/`: config types and the sandbox-only policy

### Goal
Create the shared pure module that says, for any message from outside this PC, whether it may run and that it may use the Sandbox tools only.

### Read first
- `packages/app/src-tauri/src/potato/mod.rs`: how modules and their `*_tests.rs` files are declared. Copy that pattern exactly.
- `packages/app/src-tauri/src/potato/rules.rs` l.101-140: `enum Why`, `enum Decision`, `tool_access(trust)`. This slice does NOT change them; it only reads them to keep names in the same style.
- `packages/app/src-tauri/src/potato/sandbox_calls.rs` l.50: `TOOL_NAMES`.

### Files
- create `packages/app/src-tauri/src/potato/outside/mod.rs`
- create `packages/app/src-tauri/src/potato/outside/config.rs`
- create `packages/app/src-tauri/src/potato/outside/policy.rs`
- create `packages/app/src-tauri/src/potato/outside/policy_tests.rs`
- edit `packages/app/src-tauri/src/potato/mod.rs` (add `pub mod outside;`)

### Steps
1. Create `outside/mod.rs` (Code). It declares `config`, `policy` now; `admit` and `rate` come in D3.
2. Create `outside/config.rs` (Code, whole file).
3. Create `outside/policy.rs` (Code, whole file).
4. Create `outside/policy_tests.rs` (Code).
5. In `potato/mod.rs`, add `pub mod outside;` in alphabetical order with the other `pub mod` lines.
6. Run the tests.

### Code

`packages/app/src-tauri/src/potato/outside/mod.rs` (whole file after D2):
```rust
//! Messages that come from outside this PC: Discord, Telegram, and groups with other people.
//!
//! The one rule (founder, 2026-11-04): anything triggered from outside runs in Fish's
//! computer (the Sandbox) only. Never the PC, never a helper on the PC, never PC files.
//! Everything here is pure: no network, no files, no clock (callers pass `now_ms`).

pub mod config;
pub mod policy;

#[cfg(test)]
mod policy_tests;
```
(If `potato/mod.rs` attaches test files with `#[path = "..."]`, copy that form instead.)

`packages/app/src-tauri/src/potato/outside/config.rs` (whole file):
```rust
use serde::{Deserialize, Serialize};

/// Where an outside message came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Discord,
    Telegram,
    /// An app group that has another person in it (plan 12 keeps these off until shared groups exist).
    SharedGroup,
}

impl Surface {
    pub fn wire(self) -> &'static str {
        match self {
            Surface::Discord => "discord",
            Surface::Telegram => "telegram",
            Surface::SharedGroup => "shared_group",
        }
    }
}

/// How the bot behaves in one channel or chat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceMode {
    #[default]
    Off,
    /// Answer only when called: @mention, a reply to the bot, or its name.
    WhenCalled,
    /// Answer every message from an allowed person.
    EveryMessage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub mode: PlaceMode,
}

/// Who may talk to the bot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum People {
    #[default]
    OnlyMe,
    MeAndListed,
    Anyone,
}

/// Direct messages to the bot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DmRule {
    #[default]
    OnlyMe,
    Off,
    SameAsPeople,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateConfig {
    pub per_person_turns: u32,
    pub per_person_window_s: u32,
    pub per_place_turns: u32,
    pub per_place_window_s: u32,
}

impl Default for RateConfig {
    /// fishd `TURN_LIMIT`: 6 per 5 minutes for people who are not the owner.
    fn default() -> Self {
        Self { per_person_turns: 6, per_person_window_s: 300, per_place_turns: 20, per_place_window_s: 600 }
    }
}

impl RateConfig {
    /// Keep numbers in safe bounds whatever the file says.
    pub fn clamped(&self) -> Self {
        Self {
            per_person_turns: self.per_person_turns.clamp(1, 60),
            per_person_window_s: self.per_person_window_s.clamp(60, 86_400),
            per_place_turns: self.per_place_turns.clamp(1, 200),
            per_place_window_s: self.per_place_window_s.clamp(60, 86_400),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutsideConfig {
    /// The Potato bot that answers here.
    pub bot_id: String,
    /// The person's own account ids on that service (their Discord user id).
    pub owner_ids: Vec<String>,
    pub people: People,
    pub listed_ids: Vec<String>,
    pub places: Vec<Place>,
    pub dms: DmRule,
    /// Words that call the bot, matched as whole words, any case. Example: ["fish"].
    pub names: Vec<String>,
    pub rate: RateConfig,
}

pub const MAX_PLACES: usize = 50;
pub const MAX_LISTED: usize = 200;
pub const MAX_NAMES: usize = 5;
pub const MAX_NAME_LEN: usize = 32;

impl OutsideConfig {
    /// Trim lists to their caps, drop blank ids, lower-case names. Called on every load and save.
    pub fn cleaned(mut self) -> Self {
        self.owner_ids.retain(|s| is_id(s));
        self.owner_ids.truncate(5);
        self.listed_ids.retain(|s| is_id(s));
        self.listed_ids.truncate(MAX_LISTED);
        self.places.retain(|p| is_id(&p.id));
        self.places.truncate(MAX_PLACES);
        self.names = self
            .names
            .into_iter()
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME_LEN)
            .take(MAX_NAMES)
            .collect();
        self.rate = self.rate.clamped();
        self
    }

    pub fn place(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }

    pub fn is_owner(&self, id: &str) -> bool {
        self.owner_ids.iter().any(|o| o == id)
    }
}

/// Service ids are digits (Discord snowflakes) or `-`/digits (Telegram chat ids). Max 32 chars.
pub fn is_id(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_digit())
}
```

`packages/app/src-tauri/src/potato/outside/policy.rs` (whole file):
```rust
use super::config::Surface;

/// Which tools a turn may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolScope {
    /// App DMs from the person on this PC: today's rules (`rules::tool_access`).
    AsToday,
    /// Only the 6 Sandbox tools (`sandbox_calls::TOOL_NAMES`). No helpers, no PC files.
    SandboxOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `potato_sandbox_enabled` is off.
    SandboxLabsOff,
    /// Labs is on but no Sandbox is installed yet.
    SandboxNotInstalled,
}

/// The one rule. `outside` is `None` for the app on this PC.
pub fn check(outside: Option<Surface>, sandbox_labs_on: bool, sandbox_installed: bool) -> Result<ToolScope, Refusal> {
    match outside {
        None => Ok(ToolScope::AsToday),
        Some(_) if !sandbox_labs_on => Err(Refusal::SandboxLabsOff),
        Some(_) if !sandbox_installed => Err(Refusal::SandboxNotInstalled),
        Some(_) => Ok(ToolScope::SandboxOnly),
    }
}

/// What the bot says back, once per conversation per day, when it cannot run.
pub fn refusal_text(_r: Refusal) -> &'static str {
    "I can't help here right now. My owner needs to turn on my computer first."
}

/// Error code for logs and the app (style: `CODE: sentence`).
pub fn refusal_code(r: Refusal) -> &'static str {
    match r {
        Refusal::SandboxLabsOff => "POTATO_OUTSIDE_SANDBOX_OFF",
        Refusal::SandboxNotInstalled => "POTATO_OUTSIDE_NO_SANDBOX",
    }
}

/// Tool names a `SandboxOnly` turn may use. Must equal `sandbox_calls::TOOL_NAMES`.
pub fn allowed_tools(scope: ToolScope) -> Option<&'static [&'static str]> {
    match scope {
        ToolScope::AsToday => None,
        ToolScope::SandboxOnly => Some(&crate::potato::sandbox_calls::TOOL_NAMES),
    }
}
```

`packages/app/src-tauri/src/potato/outside/policy_tests.rs`:
```rust
use super::config::*;
use super::policy::*;

#[test]
fn app_turn_keeps_todays_rules() {
    assert_eq!(check(None, false, false), Ok(ToolScope::AsToday));
}

#[test]
fn discord_refused_when_sandbox_labs_off() {
    assert_eq!(check(Some(Surface::Discord), false, true), Err(Refusal::SandboxLabsOff));
}

#[test]
fn telegram_refused_when_no_sandbox_installed() {
    assert_eq!(check(Some(Surface::Telegram), true, false), Err(Refusal::SandboxNotInstalled));
}

#[test]
fn outside_turn_is_sandbox_only() {
    assert_eq!(check(Some(Surface::Discord), true, true), Ok(ToolScope::SandboxOnly));
    assert_eq!(check(Some(Surface::SharedGroup), true, true), Ok(ToolScope::SandboxOnly));
}

#[test]
fn sandbox_only_tools_are_exactly_the_six() {
    let tools = allowed_tools(ToolScope::SandboxOnly).unwrap();
    assert_eq!(tools, &[
        "computer_observe", "computer_act", "browser_navigate",
        "browser_snapshot", "browser_act", "ask_person_for_screen",
    ]);
}

#[test]
fn rate_config_is_clamped() {
    let r = RateConfig { per_person_turns: 0, per_person_window_s: 5, per_place_turns: 9999, per_place_window_s: 999_999 }.clamped();
    assert_eq!(r, RateConfig { per_person_turns: 1, per_person_window_s: 60, per_place_turns: 200, per_place_window_s: 86_400 });
}

#[test]
fn config_cleaned_drops_bad_ids_and_lowercases_names() {
    let c = OutsideConfig {
        owner_ids: vec!["123".into(), "abc".into(), "".into()],
        names: vec!["  Fish ".into(), "".into()],
        ..Default::default()
    }.cleaned();
    assert_eq!(c.owner_ids, vec!["123".to_string()]);
    assert_eq!(c.names, vec!["fish".to_string()]);
}

#[test]
fn is_id_accepts_telegram_negative_ids() {
    assert!(is_id("-1001234567890"));
    assert!(!is_id("12a"));
    assert!(!is_id(&"1".repeat(33)));
}
```
Note on `sandbox_only_tools_are_exactly_the_six`: if `TOOL_NAMES` is in a different order in `sandbox_calls.rs` l.50, copy that order into the test. The test proves the policy points at the real list, not a copy.

### Tests to write
The 8 tests in `policy_tests.rs` above, with the inputs and expected values shown.

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D2 cargo test --lib potato::outside
```
Expected: `test result: ok. 8 passed; 0 failed`.

### Done when
- [ ] `potato::outside::{config, policy}` compile and are declared in `potato/mod.rs`.
- [ ] 8 tests pass.
- [ ] No code outside `potato/outside/` and the one `pub mod` line changed.

### Do NOT
- Do not change `rules.rs` in this slice.
- Do not read the clock, files, env or network in `outside/`.
- Do not copy the 6 tool names into a second constant; point at `sandbox_calls::TOOL_NAMES`.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `TOOL_NAMES is private`: make it `pub(crate)` in `sandbox_calls.rs` (one-word change; say so in the commit).
2. Type mismatch `&[&str; 6]` vs `&[&str]`: write `Some(&crate::potato::sandbox_calls::TOOL_NAMES[..])`.
3. `serde` not found: the crate uses `serde` elsewhere (settings); add `use serde::{Deserialize, Serialize};` exactly as other `potato/*.rs` files do.

---

## Slice D3 - `potato/outside/`: admit and the rate book

### Goal
One pure function `admit` that decides, in a fixed order, whether an outside message is answered, and a rate book that limits how often.

### Read first
- `packages/app/src-tauri/src/potato/outside/config.rs` (from D2).
- `packages/app/src-tauri/src/potato/hold.rs`: `hold::is_held` - the caller passes its result in as `held`.
- `packages/app/src-tauri/src/potato/store.rs`: `store::clean_text` (the caller cleans text; `admit` only checks emptiness).

### Files
- create `packages/app/src-tauri/src/potato/outside/rate.rs`
- create `packages/app/src-tauri/src/potato/outside/admit.rs`
- create `packages/app/src-tauri/src/potato/outside/rate_tests.rs`
- create `packages/app/src-tauri/src/potato/outside/admit_tests.rs`
- edit `packages/app/src-tauri/src/potato/outside/mod.rs`

### Steps
1. Create `rate.rs` (Code, whole file).
2. Create `admit.rs` (Code, whole file).
3. In `outside/mod.rs` add `pub mod admit; pub mod rate;` and `#[cfg(test)] mod admit_tests; #[cfg(test)] mod rate_tests;`.
4. Write both test files (tests listed below).
5. Run.

### Code

`packages/app/src-tauri/src/potato/outside/rate.rs` (whole file):
```rust
use std::collections::{HashMap, VecDeque};

/// Most keys the book keeps. Past this, empty queues are dropped first, then the whole book is cleared.
pub const MAX_KEYS: usize = 10_000;

/// Sliding-window counters for people and places.
#[derive(Debug, Default)]
pub struct RateBook {
    people: HashMap<String, VecDeque<u64>>,
    places: HashMap<String, VecDeque<u64>>,
}

/// `Err(seconds)` = wait this long. Does not record.
pub fn check(q: Option<&VecDeque<u64>>, limit: u32, window_ms: u64, now_ms: u64) -> Result<(), u32> {
    let Some(q) = q else { return Ok(()) };
    let live: Vec<u64> = q.iter().copied().filter(|t| now_ms.saturating_sub(*t) < window_ms).collect();
    if (live.len() as u32) < limit {
        return Ok(());
    }
    let oldest = live[0];
    let wait_ms = window_ms.saturating_sub(now_ms.saturating_sub(oldest));
    Err(((wait_ms + 999) / 1000) as u32)
}

fn record(map: &mut HashMap<String, VecDeque<u64>>, key: &str, window_ms: u64, now_ms: u64) {
    if map.len() >= MAX_KEYS && !map.contains_key(key) {
        map.retain(|_, q| q.iter().any(|t| now_ms.saturating_sub(*t) < window_ms));
        if map.len() >= MAX_KEYS {
            map.clear();
        }
    }
    let q = map.entry(key.to_string()).or_default();
    while let Some(t) = q.front() {
        if now_ms.saturating_sub(*t) >= window_ms { q.pop_front(); } else { break; }
    }
    q.push_back(now_ms);
}

impl RateBook {
    pub fn new() -> Self { Self::default() }

    pub fn check_person(&self, id: &str, limit: u32, window_ms: u64, now_ms: u64) -> Result<(), u32> {
        check(self.people.get(id), limit, window_ms, now_ms)
    }
    pub fn check_place(&self, id: &str, limit: u32, window_ms: u64, now_ms: u64) -> Result<(), u32> {
        check(self.places.get(id), limit, window_ms, now_ms)
    }
    pub fn record_person(&mut self, id: &str, window_ms: u64, now_ms: u64) {
        record(&mut self.people, id, window_ms, now_ms)
    }
    pub fn record_place(&mut self, id: &str, window_ms: u64, now_ms: u64) {
        record(&mut self.places, id, window_ms, now_ms)
    }
    pub fn len(&self) -> (usize, usize) { (self.people.len(), self.places.len()) }
}
```

`packages/app/src-tauri/src/potato/outside/admit.rs` (signatures plus full body, it is the core rule):
```rust
use super::config::{DmRule, OutsideConfig, People, PlaceMode, Surface};
use super::rate::RateBook;

/// One incoming message, already parsed by the surface adapter (Discord or Telegram).
#[derive(Clone, Debug)]
pub struct Incoming<'a> {
    pub surface: Surface,
    pub author_id: &'a str,
    pub author_is_bot: bool,
    /// `None` = a direct message.
    pub place_id: Option<&'a str>,
    /// For a thread: the channel the thread lives in.
    pub parent_place_id: Option<&'a str>,
    pub text: &'a str,
    pub mentions_bot: bool,
    pub replies_to_bot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust { OwnerRemote, Others }

impl Trust {
    /// Same words as `Trust` in `potato-brain/src/channels/potato.ts` l.61.
    pub fn wire(self) -> &'static str {
        match self { Trust::OwnerRemote => "owner_remote", Trust::Others => "others" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addressed { DirectMessage, Mention, Reply, Name, EveryMessagePlace }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admitted {
    /// `discord:dm:<userId>` or `discord:ch:<channelOrThreadId>` (same with `telegram:`).
    pub conversation_id: String,
    pub trust: Trust,
    pub addressed: Addressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    FromBot,
    EmptyText,
    Held,
    DmNotAllowed,
    PlaceNotPicked,
    PlaceOff,
    PersonNotAllowed,
    NotCalled,
    PersonRate { retry_after_s: u32 },
    PlaceRate { retry_after_s: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admit { Yes(Admitted), No(Drop) }

/// The order is fixed (fishd `onMessage`): bots, empty, hold, place, person, called, rate.
/// Records in `rate` only when the answer is `Yes`.
pub fn admit(cfg: &OutsideConfig, m: &Incoming<'_>, rate: &mut RateBook, held: bool, now_ms: u64) -> Admit {
    if m.author_is_bot { return Admit::No(Drop::FromBot); }
    if m.text.trim().is_empty() { return Admit::No(Drop::EmptyText); }
    if held { return Admit::No(Drop::Held); }

    let owner = cfg.is_owner(m.author_id);
    let listed = cfg.listed_ids.iter().any(|x| x == m.author_id);
    let person_ok = match cfg.people {
        People::OnlyMe => owner,
        People::MeAndListed => owner || listed,
        People::Anyone => true,
    };

    let (conversation_id, addressed) = match m.place_id {
        None => {
            let dm_ok = match cfg.dms {
                DmRule::Off => false,
                DmRule::OnlyMe => owner,
                DmRule::SameAsPeople => person_ok,
            };
            if !dm_ok { return Admit::No(Drop::DmNotAllowed); }
            (format!("{}:dm:{}", m.surface.wire(), m.author_id), Addressed::DirectMessage)
        }
        Some(pid) => {
            let place = cfg.place(pid).or_else(|| m.parent_place_id.and_then(|p| cfg.place(p)));
            let Some(place) = place else { return Admit::No(Drop::PlaceNotPicked) };
            if place.mode == PlaceMode::Off { return Admit::No(Drop::PlaceOff); }
            if !person_ok { return Admit::No(Drop::PersonNotAllowed); }
            let called = if m.mentions_bot { Some(Addressed::Mention) }
                else if m.replies_to_bot { Some(Addressed::Reply) }
                else if names_match(&cfg.names, m.text) { Some(Addressed::Name) }
                else { None };
            let addressed = match (place.mode, called) {
                (_, Some(a)) => a,
                (PlaceMode::EveryMessage, None) => Addressed::EveryMessagePlace,
                _ => return Admit::No(Drop::NotCalled),
            };
            (format!("{}:ch:{}", m.surface.wire(), pid), addressed)
        }
    };
    if !person_ok { return Admit::No(Drop::PersonNotAllowed); }

    let r = &cfg.rate;
    if !owner {
        let w = r.per_person_window_s as u64 * 1000;
        if let Err(s) = rate.check_person(m.author_id, r.per_person_turns, w, now_ms) {
            return Admit::No(Drop::PersonRate { retry_after_s: s });
        }
        if let Some(pid) = m.place_id {
            let pw = r.per_place_window_s as u64 * 1000;
            if let Err(s) = rate.check_place(pid, r.per_place_turns, pw, now_ms) {
                return Admit::No(Drop::PlaceRate { retry_after_s: s });
            }
        }
        rate.record_person(m.author_id, w, now_ms);
        if let Some(pid) = m.place_id {
            rate.record_place(pid, r.per_place_window_s as u64 * 1000, now_ms);
        }
    }

    let trust = if owner { Trust::OwnerRemote } else { Trust::Others };
    Admit::Yes(Admitted { conversation_id, trust, addressed })
}

/// Whole-word, case-insensitive match of any name.
pub fn names_match(names: &[String], text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
    names.iter().any(|n| {
        let parts: Vec<&str> = n.split_whitespace().collect();
        !parts.is_empty() && words.windows(parts.len()).any(|w| w == parts.as_slice())
    })
}
```
Note: the owner is never rate limited (fishd: `TURN_LIMIT` is for non-owners). The daily cost limit (`cost::budget`) still applies to everyone through `Service::send_as`.

### Tests to write
`rate_tests.rs`:
| Test | Input | Expected |
|---|---|---|
| `allows_up_to_limit` | limit 6, window 300 000 ms, 6 records at t=0..5 s, check at t=6 s | 6 checks `Ok`, 7th `Err(295)` |
| `window_slides` | same, check at t=300 001 ms | `Ok(())` |
| `record_prunes_old` | 3 records at t=0, record at t=400 000 with window 300 000 | queue len 1 (check via `check(...)` with limit 1 gives `Err`) |
| `caps_keys` | record 10 001 distinct ids at the same `now` | `len().0 <= MAX_KEYS` |

`admit_tests.rs` (build a base config: `owner_ids ["1"]`, `people OnlyMe`, places `[{id:"100", mode: WhenCalled}, {id:"200", mode: EveryMessage}, {id:"300", mode: Off}]`, `names ["fish"]`, `dms OnlyMe`, default rate):
| Test | Input | Expected |
|---|---|---|
| `drops_bots_first` | `author_is_bot: true`, owner, mention | `No(FromBot)` |
| `drops_empty` | text `"   "` | `No(EmptyText)` |
| `drops_when_held` | held = true, owner DM | `No(Held)` |
| `owner_dm_admitted` | author "1", place None | `Yes` conv `discord:dm:1`, `OwnerRemote`, `DirectMessage` |
| `stranger_dm_dropped` | author "9", place None | `No(DmNotAllowed)` |
| `unpicked_channel_dropped` | place "999" | `No(PlaceNotPicked)` |
| `off_channel_dropped` | place "300", owner, mention | `No(PlaceOff)` |
| `thread_uses_parent_place` | place "555", parent "100", owner, mention | `Yes` conv `discord:ch:555` |
| `when_called_needs_call` | place "100", owner, text "hello" | `No(NotCalled)` |
| `name_calls_bot` | place "100", owner, text "Fish, look at this" | `Yes`, `Name` |
| `name_is_whole_word` | text "selfish people" | `No(NotCalled)` |
| `every_message_place` | place "200", owner, text "hi" | `Yes`, `EveryMessagePlace` |
| `only_me_blocks_stranger` | place "200", author "9" | `No(PersonNotAllowed)` |
| `anyone_lets_stranger_in_as_others` | people Anyone, place "200", author "9" | `Yes`, `Others` |
| `stranger_rate_limited_after_6` | Anyone, 7 messages from "9" at 1 s steps | 7th is `No(PersonRate { retry_after_s: 294 })` |
| `owner_never_rate_limited` | 50 messages from "1" | all `Yes` |
| `place_rate_limited_after_20` | Anyone, 21 messages from 21 different authors in place "200" | 21st `No(PlaceRate {..})` |
| `rejected_message_does_not_count` | stranger, 6 x `NotCalled` in "100" then 6 calls | all 6 calls `Yes` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D3 cargo test --lib potato::outside
```
Expected: `test result: ok. 30 passed; 0 failed` (8 from D2 + 4 + 18).

### Done when
- [ ] `admit` order matches the code above and every `Drop` variant has at least one test.
- [ ] 30 tests pass.

### Do NOT
- Do not log message text inside `admit` (it is pure; callers log only ids and the `Drop` name).
- Do not let the owner skip `Held` or `PlaceOff`. Owner skips only the rate book.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `retry_after_s` off by one: the formula rounds up (`(wait_ms + 999) / 1000`). With records at 0..5 s and check at 6 s, oldest = 0, wait = 294 000 ms -> 294. Fix the test table, not the formula, if your steps differ.
2. Borrow error on `cfg.place(...)` inside `match`: bind `let place = ...;` before `match place.mode`.
3. Thread test fails: `place_id` is the thread id; the conversation must use the thread id, the mode lookup uses the parent. Keep both.

---

## Slice D4 - `Origin::Outside` in the engine; the brain honours `sandboxOnly`

### Goal
Let `Service` accept an admitted outside message, enforce the sandbox-only policy before any brain work, and make the brain run that turn with the 6 Sandbox tools only.

### Read first
- `packages/app/src-tauri/src/potato/service.rs` l.686-830: `send`, `send_as`, `Origin`, `deliver`, `push_to_brain` (l.1080). Find where `Facts` is built with surface `"app"` and `Sender::App`.
- `packages/app/src-tauri/src/potato/rules.rs` l.101-140: `Why`, `Decision`, `tool_access(trust)`, `decide`.
- `packages/app/src-tauri/src/potato/live.rs`: `sandbox_labs_on`, and how `Service` learns whether a sandbox is installed (`sandbox_host.rs`).
- `packages/potato-brain/src/channels/potato.ts` l.61 (`Trust`), l.421-422 (`case "send"`), `onSend` l.553, `runTurn` l.624.
- `packages/potato-brain/src/core/sandbox-tools.ts`: `isSandboxTool`, `SANDBOX_TOOL_NAMES` l.36.
- `packages/potato-brain/src/core/helper-tool.ts`: `ASK_HELPER_SPEC` (the tool that must disappear).

### Files
- edit `packages/app/src-tauri/src/potato/service.rs`
- create `packages/app/src-tauri/src/potato/outside/engine.rs` (the glue, so `service.rs` grows by few lines)
- edit `packages/app/src-tauri/src/potato/outside/mod.rs`
- edit `packages/potato-brain/src/channels/potato.ts`
- create `packages/potato-brain/test/outside-scope.test.ts`

### Steps
1. Create `outside/engine.rs` with `OutsideOrigin`, `OutsideReplyTarget` and `ReplyTargets` (Code).
2. In `service.rs`, add the variant `Outside(crate::potato::outside::engine::OutsideOrigin)` to `enum Origin` (next to `Person`).
3. In `service.rs`, add `pub fn send_outside(&self, bot: &str, text: &str, o: OutsideOrigin) -> Result<SendOutcome, String>` (Code). It calls `outside::policy::check` FIRST, before `rules::decide`, before saving anything.
4. In `send_as`, where `Facts` gets surface `"app"`: when the origin is `Outside(o)`, set surface to `o.surface.wire()` and pass `o.trust.wire()` as the trust (NEEDS-FACT: the exact `Facts` field names).
5. In `push_to_brain` (l.1080), when the origin is `Outside`, add the JSON fields `"sandboxOnly": true`, `"trust": "<wire>"`, `"conversationId": "<o.conversation_id>"` to the inbox item, and remember `ReplyTargets::insert(inbox_item_id, target)`.
6. In `potato.ts`, in the `send` item type add `sandboxOnly?: boolean`. In `runTurn`, filter tools with `scopeTools` (Code) before calling `Agent.turn`.
7. In `potato.ts`, when `sandboxOnly` is true, do not add the owner's memory notes to the prompt (NEEDS-FACT: where `runTurn` adds notes; pass the same option the code uses for `trust === "others"` if one exists).
8. Write the brain test and the Rust tests.

### Code

`packages/app/src-tauri/src/potato/outside/engine.rs`:
```rust
use std::collections::{HashMap, VecDeque};
use super::admit::{Addressed, Trust};
use super::config::Surface;

/// What `Service` needs to know about an admitted outside message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutsideOrigin {
    pub surface: Surface,
    pub conversation_id: String,
    pub trust: Trust,
    pub addressed: Addressed,
    pub target: OutsideReplyTarget,
    /// Shown in the brain prompt as the speaker's name. Already cleaned, max 64 chars.
    pub author_name: String,
}

/// Where the reply goes back to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutsideReplyTarget {
    pub surface: Surface,
    pub place_id: String,      // channel/thread id, or the DM channel id
    pub message_id: String,    // the message to reply to
}

pub const MAX_TARGETS: usize = 500;

/// inbox item id -> reply target. Oldest dropped past `MAX_TARGETS`.
#[derive(Debug, Default)]
pub struct ReplyTargets {
    map: HashMap<String, OutsideReplyTarget>,
    order: VecDeque<String>,
}

impl ReplyTargets {
    pub fn insert(&mut self, item_id: String, t: OutsideReplyTarget) {
        if self.map.insert(item_id.clone(), t).is_none() {
            self.order.push_back(item_id);
        }
        while self.order.len() > MAX_TARGETS {
            if let Some(old) = self.order.pop_front() { self.map.remove(&old); }
        }
    }
    pub fn take(&mut self, item_id: &str) -> Option<OutsideReplyTarget> {
        let t = self.map.remove(item_id);
        if t.is_some() { self.order.retain(|x| x != item_id); }
        t
    }
}

/// One refusal reply per conversation per 24 h, so a closed door does not spam a channel.
#[derive(Debug, Default)]
pub struct RefusalMemory { last: HashMap<String, u64> }

impl RefusalMemory {
    pub fn should_say(&mut self, conversation_id: &str, now_ms: u64) -> bool {
        let day = 86_400_000;
        match self.last.get(conversation_id) {
            Some(t) if now_ms.saturating_sub(*t) < day => false,
            _ => { self.last.insert(conversation_id.to_string(), now_ms); true }
        }
    }
}
```

`service.rs` new method (signature and body; adapt field names to the real `Service`):
```rust
/// An admitted message from Discord or Telegram. Sandbox only, always.
pub fn send_outside(&self, bot: &str, text: &str, o: OutsideOrigin) -> Result<SendOutcome, String> {
    let sandbox_labs = self.host.sandbox_labs_on();          // NEEDS-FACT: the Host method name
    let installed = self.host.sandbox_installed();           // NEEDS-FACT: the Host method name
    match crate::potato::outside::policy::check(Some(o.surface), sandbox_labs, installed) {
        Ok(_) => self.send_as(bot, text, Origin::Outside(o)),
        Err(r) => {
            let now = self.host.now_ms();
            if self.refusals.lock().unwrap().should_say(&o.conversation_id, now) {
                self.push_outside_text(&o.target, crate::potato::outside::policy::refusal_text(r));
            }
            Err(format!("{}: outside turn needs Fish's computer", crate::potato::outside::policy::refusal_code(r)))
        }
    }
}
```
`push_outside_text` is added in slice D9 (it pushes to the Discord outbox). In D4 write it as a method that calls an
`OutsideSink` trait object stored on `Service`, defaulting to a no-op sink:
```rust
pub trait OutsideSink: Send + Sync {
    fn send_text(&self, target: &OutsideReplyTarget, text: &str);
    fn typing(&self, target: &OutsideReplyTarget);
}
pub struct NoSink;
impl OutsideSink for NoSink {
    fn send_text(&self, _t: &OutsideReplyTarget, _x: &str) {}
    fn typing(&self, _t: &OutsideReplyTarget) {}
}
```
Put `OutsideSink` and `NoSink` in `outside/engine.rs`.

In `on_event` for `Event::Reply { conversation_id, in_reply_to, segments, kaomoji, .. }`: if `conversation_id` starts with
`discord:` or `telegram:`, then `if let Some(t) = self.reply_targets.lock().unwrap().take(&in_reply_to)` call
`self.outside_sink.send_text(&t, &join_reply(&segments, kaomoji.as_deref()))`, and still save the reply in the bot's
store under that conversation (so the app can show it, slice D11). `join_reply` joins segments with `"\n\n"` and adds
the kaomoji after one space when present.

Brain, `packages/potato-brain/src/channels/potato.ts`, add near `runTurn`:
```ts
import { isSandboxTool } from "../core/sandbox-tools.ts";

/** Outside turns (Discord, Telegram): only the 6 Sandbox tools. Founder rule, 2026-11-04. */
export function scopeTools<T extends { name: string }>(tools: readonly T[], sandboxOnly: boolean): T[] {
  return sandboxOnly ? tools.filter((t) => isSandboxTool(t.name)) : [...tools];
}
```
`ExtraTool` (from `core/loop.ts`) is the tool type; if its name field is not `name`, change the generic bound to match.

`packages/potato-brain/test/outside-scope.test.ts` (whole file):
```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { scopeTools } from "../src/channels/potato.ts";
import { SANDBOX_TOOL_NAMES } from "../src/core/sandbox-tools.ts";

const all = [
  ...SANDBOX_TOOL_NAMES.map((name) => ({ name })),
  { name: "ask_helper" },
  { name: "memory_search" },
];

test("sandboxOnly keeps exactly the six sandbox tools", () => {
  const names = scopeTools(all, true).map((t) => t.name).sort();
  assert.deepEqual(names, [...SANDBOX_TOOL_NAMES].sort());
});

test("app turns keep every tool", () => {
  assert.equal(scopeTools(all, false).length, all.length);
});

test("sandboxOnly drops ask_helper", () => {
  assert.equal(scopeTools(all, true).some((t) => t.name === "ask_helper"), false);
});
```

### Tests to write
Rust, in `packages/app/src-tauri/src/potato/outside/engine_tests.rs` (declare it in `outside/mod.rs`):
| Test | Input | Expected |
|---|---|---|
| `reply_targets_drop_oldest_past_cap` | insert 501 ids | `take("0")` is `None`, `take("500")` is `Some` |
| `reply_target_taken_once` | insert "a", take twice | `Some`, then `None` |
| `refusal_said_once_per_day` | `should_say("c", 0)`, `("c", 1000)`, `("c", 86_400_001)` | `true`, `false`, `true` |

Rust, in `packages/app/src-tauri/src/potato/service_tests.rs` (uses the existing fake Host):
| Test | Input | Expected |
|---|---|---|
| `outside_refused_when_sandbox_off` | fake host sandbox labs off; `send_outside` | `Err` starting `POTATO_OUTSIDE_SANDBOX_OFF:`; nothing saved; no inbox item |
| `outside_item_is_sandbox_only` | fake host sandbox on + installed; `send_outside` | inbox item JSON has `"sandboxOnly":true`, `"trust":"others"`, `"conversationId":"discord:ch:200"` |
| `outside_reply_goes_to_sink` | then ingest `Reply` with `in_reply_to` = that item id | recording sink got one `send_text` with the joined text |
| `app_send_has_no_sandbox_only` | normal `send` | inbox item has no `sandboxOnly` key |

Brain: the 3 tests in `outside-scope.test.ts`.

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D4 cargo test --lib potato
```
Expected: `test result: ok.` and 0 failed (all existing Potato tests plus 7 new).
```
cd packages/potato-brain && node --test test/outside-scope.test.ts test/potato-channel.test.ts
```
Expected: `# fail 0`.
```
cd packages/potato-brain && npm run typecheck
```
Expected: exits 0 with no output.

### Done when
- [ ] An outside message with the Sandbox off is refused before anything is saved or sent to a brain.
- [ ] An outside inbox item carries `sandboxOnly: true` and the brain filters tools with `scopeTools`.
- [ ] Replies for `discord:`/`telegram:` conversations go to the `OutsideSink`.
- [ ] All existing `potato` tests still pass.

### Do NOT
- Do not change `PROTOCOL_VERSION` (stays 1). New fields are optional and old brains ignore them.
- Do not change how app DMs pick tools (`rules::tool_access`).
- Do not show the helper Start card for an outside turn.
- Do not put message text in logs; log the conversation id and the outcome code only.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Existing `service_tests` fail to build because `Origin` gained a variant: add `Origin::Outside(_) => ...` arms where a `match` on `Origin` is exhaustive; outside behaves like `Person` except surface and trust.
2. The fake Host has no `sandbox_labs_on`/`sandbox_installed`: add both to `trait Host` (service.rs l.62) with the real impl in `live.rs` calling `sandbox_labs_on(public)` and the sandbox host's installed check, and fake fields in the test host.
3. Brain typecheck error on `scopeTools` generic: the tool list type is `ExtraTool[]`; write `scopeTools<ExtraTool>(...)`.

---

## Slice D5 - Discord config store and setup calls

### Goal
Store the Discord choices in `<app data>/potato/discord.json` and add the setup calls: check the token, list servers, list channels, build the invite link.

### Read first
- `packages/app/src-tauri/src/potato/store.rs`: `store::state_dir` and how JSON files are written (atomic write helper if one exists).
- `packages/app/src-tauri/src/potato/live.rs` l.255-290: `check_key` and `ask_model` - copy the HTTP client and timeout style for the real Discord calls.
- `packages/app/src-tauri/src/secrets.rs` l.31, l.152: `SIDECAR_SECRETS`, `read_secret`.

### Files
- create `packages/app/src-tauri/src/potato/discord/mod.rs`
- create `packages/app/src-tauri/src/potato/discord/config.rs`
- create `packages/app/src-tauri/src/potato/discord/setup_api.rs`
- create `packages/app/src-tauri/src/potato/discord/setup_api_tests.rs`
- edit `packages/app/src-tauri/src/potato/mod.rs` (add `pub mod discord;`)

### Steps
1. Create `discord/mod.rs`: `pub mod config; pub mod setup_api; #[cfg(test)] mod setup_api_tests;`.
2. Create `discord/config.rs` (Code).
3. Create `discord/setup_api.rs` with the trait, pure parsers and the real client (Code).
4. Write tests with a fake `DiscordHttp`.
5. Add `pub mod discord;` to `potato/mod.rs`.

### Code

`packages/app/src-tauri/src/potato/discord/config.rs`:
```rust
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::potato::outside::config::OutsideConfig;

pub const FILE: &str = "discord.json";
pub const MAX_GUILDS: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildPick { pub id: String, pub name: String }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiscordConfig {
    /// The person's "Answer on Discord" switch (separate from the Labs flag).
    pub on: bool,
    pub bot_user_id: String,
    pub bot_name: String,
    pub application_id: String,
    pub guilds: Vec<GuildPick>,
    pub outside: OutsideConfig,
}

impl DiscordConfig {
    pub fn cleaned(mut self) -> Self {
        self.guilds.retain(|g| crate::potato::outside::config::is_id(&g.id));
        self.guilds.truncate(MAX_GUILDS);
        self.outside = self.outside.cleaned();
        self
    }
    pub fn guild_allowed(&self, guild_id: Option<&str>) -> bool {
        match guild_id {
            None => true, // a DM; `outside::admit` decides DMs
            Some(g) => self.guilds.iter().any(|x| x.id == g),
        }
    }
}

pub fn path(state_dir: &Path) -> PathBuf { state_dir.join(FILE) }

/// Missing or broken file = default (everything off). Never panics.
pub fn load(state_dir: &Path) -> DiscordConfig {
    std::fs::read_to_string(path(state_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<DiscordConfig>(&s).ok())
        .unwrap_or_default()
        .cleaned()
}

pub fn save(state_dir: &Path, cfg: &DiscordConfig) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&cfg.clone().cleaned()).map_err(|e| format!("POTATO_DISCORD_SAVE: {e}"))?;
    let tmp = path(state_dir).with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("POTATO_DISCORD_SAVE: {e}"))?;
    std::fs::rename(&tmp, path(state_dir)).map_err(|e| format!("POTATO_DISCORD_SAVE: {e}"))
}
```
If `store.rs` has an atomic write helper, use it instead of the tmp+rename lines.

`packages/app/src-tauri/src/potato/discord/setup_api.rs` (signatures and the non-obvious bodies):
```rust
use serde::Serialize;

pub const API: &str = "https://discord.com/api/v10";
/// View Channels (1024) + Send Messages (2048) + Read Message History (65536)
/// + Send Messages in Threads (274877906944).
pub const INVITE_PERMISSIONS: u64 = 274_877_975_552;
/// Application flags: GATEWAY_MESSAGE_CONTENT (1<<18), GATEWAY_MESSAGE_CONTENT_LIMITED (1<<19).
pub const FLAG_CONTENT: u64 = 1 << 18;
pub const FLAG_CONTENT_LIMITED: u64 = 1 << 19;

/// GET only. Returns (status, body). The token is sent as `Authorization: Bot <token>`.
pub trait DiscordHttp: Send + Sync {
    fn get(&self, path: &str, token: &str) -> Result<(u16, String), String>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotIdentity { pub bot_user_id: String, pub bot_name: String, pub application_id: String }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GuildInfo { pub id: String, pub name: String }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo { pub id: String, pub name: String, pub category: Option<String> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupError { BadToken, ContentIntentOff, Network(String), Discord(u16) }

impl SetupError {
    /// `CODE: sentence` (the UI strips the code with `plainError`).
    pub fn to_wire(&self) -> String {
        match self {
            SetupError::BadToken => "DISCORD_BAD_TOKEN: Discord did not accept this token. Copy it again from the Bot page.".into(),
            SetupError::ContentIntentOff => "DISCORD_INTENT_OFF: Turn on Message Content Intent on the Bot page, then check again.".into(),
            SetupError::Network(_) => "DISCORD_NETWORK: Could not reach Discord. Check your internet and try again.".into(),
            SetupError::Discord(code) => format!("DISCORD_ERROR: Discord answered with error {code}. Try again in a minute."),
        }
    }
}

fn get_json(http: &dyn DiscordHttp, path: &str, token: &str) -> Result<serde_json::Value, SetupError> {
    let (status, body) = http.get(path, token).map_err(SetupError::Network)?;
    match status {
        200 => serde_json::from_str(&body).map_err(|_| SetupError::Discord(200)),
        401 => Err(SetupError::BadToken),
        s => Err(SetupError::Discord(s)),
    }
}

pub fn check_token(http: &dyn DiscordHttp, token: &str) -> Result<BotIdentity, SetupError> {
    let token = token.trim();
    if token.is_empty() || token.len() > 200 || token.chars().any(|c| c.is_whitespace()) {
        return Err(SetupError::BadToken);
    }
    let me = get_json(http, "/users/@me", token)?;
    let app = get_json(http, "/oauth2/applications/@me", token)?;
    let flags = app.get("flags").and_then(|f| f.as_u64()).unwrap_or(0);
    if flags & (FLAG_CONTENT | FLAG_CONTENT_LIMITED) == 0 {
        return Err(SetupError::ContentIntentOff);
    }
    Ok(BotIdentity {
        bot_user_id: str_field(&me, "id"),
        bot_name: str_field(&me, "username"),
        application_id: str_field(&app, "id"),
    })
}

pub fn list_guilds(http: &dyn DiscordHttp, token: &str) -> Result<Vec<GuildInfo>, SetupError>;
/// Keeps text (type 0) and announcement (type 5) channels, sorted by category then position.
/// `category` is the name of the parent category channel (type 4), if any.
pub fn list_channels(http: &dyn DiscordHttp, token: &str, guild_id: &str) -> Result<Vec<ChannelInfo>, SetupError>;

pub fn invite_url(application_id: &str) -> String {
    format!("https://discord.com/oauth2/authorize?client_id={application_id}&scope=bot&permissions={INVITE_PERMISSIONS}")
}

fn str_field(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").chars().take(100).collect()
}

/// The real client. Copy the HTTP client, timeout (10 s) and user agent style from `live.rs` `ask_model`.
pub struct RealDiscordHttp;
impl DiscordHttp for RealDiscordHttp {
    fn get(&self, path: &str, token: &str) -> Result<(u16, String), String> {
        // GET {API}{path}
        // headers: Authorization: Bot {token}; User-Agent: DiscordBot (https://github.com/Potato-dev-inc/puppet-master, 1)
        // timeout 10 s; body capped at 1 MB; on transport error return Err(short text WITHOUT the token)
        unimplemented!("write with the client used by live.rs ask_model")
    }
}
```
`list_channels` guards `guild_id` with `outside::config::is_id` and returns `SetupError::Discord(400)` if it fails (never builds a path from an unchecked id).

### Tests to write (`setup_api_tests.rs`, with `struct FakeHttp(HashMap<&'static str, (u16, &'static str)>)`)
| Test | Input | Expected |
|---|---|---|
| `check_token_ok` | `/users/@me` -> `{"id":"42","username":"Fish"}`, `/oauth2/applications/@me` -> `{"id":"77","flags":262144}` | `Ok(BotIdentity { bot_user_id:"42", bot_name:"Fish", application_id:"77" })` |
| `check_token_limited_flag_ok` | flags `524288` | `Ok(..)` |
| `check_token_intent_off` | flags `0` | `Err(ContentIntentOff)` |
| `check_token_401` | `/users/@me` -> 401 | `Err(BadToken)` |
| `check_token_rejects_whitespace` | token `"a b"` | `Err(BadToken)`, fake saw 0 calls |
| `channels_keep_text_and_news` | 4 channels types 0, 2, 4, 5; one type 0 with `parent_id` of the type-4 one named "Chat" | 2 results; the child has `category: Some("Chat")` |
| `channels_reject_bad_guild_id` | guild id `"../x"` | `Err(Discord(400))`, 0 calls |
| `invite_url_has_permissions` | app id `"77"` | `"https://discord.com/oauth2/authorize?client_id=77&scope=bot&permissions=274877975552"` |
| `error_text_never_has_token` | every `SetupError` variant `.to_wire()` | none contains the test token string |
| `config_load_missing_is_default` | empty temp dir | `DiscordConfig::default()` |
| `config_round_trip` | save then load a config with 1 guild and 1 place | equal |
| `config_load_broken_is_default` | file content `"{"` | default |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D5 cargo test --lib potato::discord
```
Expected: `test result: ok. 12 passed; 0 failed`.

### Done when
- [ ] Config loads safely from missing or broken files.
- [ ] `check_token` detects a bad token and a missing Message Content Intent.
- [ ] No error text includes the token.
- [ ] 12 tests pass.

### Do NOT
- Do not store the token in `discord.json`. The token lives only in the OS credential store as `discord_bot_token`.
- Do not call Discord from tests. Use `FakeHttp`.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `unimplemented!` reached in a test: a test used `RealDiscordHttp`; switch it to `FakeHttp`.
2. Temp dir helper missing: use `std::env::temp_dir().join(format!("pm-d5-{}", std::process::id()))` and remove it at the end of the test.
3. `serde(rename_all)` breaks `OutsideConfig` field names in JSON: that is expected (camelCase at the top, snake_case inside `outside`). The TS type in D6 must match exactly: top-level camelCase, `outside` snake_case.

---

## Slice D6 - Tauri commands and the TypeScript transport

### Goal
Expose the Discord setup to the app UI through Tauri commands and a `DiscordTransport` with a fake twin for tests.

### Read first
- `packages/app/src-tauri/src/commands/potato.rs` l.35-130: command style, the `run(move |s| ...)` helper, error strings.
- `packages/app/src-tauri/src/commands/secrets.rs` l.40-62: `secret_set`, `secret_get_masked`.
- `packages/app/src-tauri/src/lib.rs` l.698-720: how `potato_*` commands are registered.
- `packages/app/src/lib/potato.ts` l.1-2 and l.~158: `invoke`, `plainError`, `fakePotatoTransport` style.
- `packages/app/src/lib/secrets.ts`: how the UI writes `deepseek_api_key` (copy it for `discord_bot_token`).

### Files
- create `packages/app/src-tauri/src/commands/potato_discord.rs`
- edit `packages/app/src-tauri/src/commands/mod.rs` (add the module; NEEDS-FACT: the file that lists command modules)
- edit `packages/app/src-tauri/src/lib.rs` (register 6 commands)
- create `packages/app/src/lib/potato-discord.ts`
- create `packages/app/src/lib/potato-discord.test.ts`

### Steps
1. Create `commands/potato_discord.rs` with the 6 commands (Code).
2. Register them in `lib.rs` next to `commands::potato_send` (l.703), in the same order as below.
3. Create `lib/potato-discord.ts` (Code).
4. Create the test.

### Code

`packages/app/src-tauri/src/commands/potato_discord.rs` (signatures):
```rust
use crate::potato::discord::{config::{self, DiscordConfig}, setup_api::{self, BotIdentity, ChannelInfo, GuildInfo, RealDiscordHttp}};

const TOKEN: &str = "discord_bot_token";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordView {
    pub config: DiscordConfig,
    /// From `secrets::get_masked`: "set" + last 4 chars, or not set.
    pub token: crate::secrets::SecretMask,
    pub labs_on: bool,
    pub sandbox_ready: bool,
    pub link: LinkStatus, // from slice D9; until D9 merges, always LinkStatus::Off
    pub invite_url: Option<String>,
}

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus { Off, Starting, Online, Reconnecting, StoppedBadToken, StoppedIntentOff, StoppedError }

#[tauri::command] pub async fn potato_discord_get() -> Result<DiscordView, String>;
/// Reads the token from the credential store (never from the UI), checks it, saves bot id/name/app id into the config.
#[tauri::command] pub async fn potato_discord_check() -> Result<BotIdentity, String>;
#[tauri::command] pub async fn potato_discord_guilds() -> Result<Vec<GuildInfo>, String>;
#[tauri::command] pub async fn potato_discord_channels(guild_id: String) -> Result<Vec<ChannelInfo>, String>;
/// Saves everything except `on`, `botUserId`, `botName`, `applicationId` (those are kept from the stored file).
#[tauri::command] pub async fn potato_discord_save(config: DiscordConfig) -> Result<DiscordView, String>;
/// Refuses `on = true` with `POTATO_OUTSIDE_NO_SANDBOX: ...` unless Labs, Sandbox, token and at least one place are set.
#[tauri::command] pub async fn potato_discord_set_on(on: bool) -> Result<DiscordView, String>;
```
Rules inside the commands:
- `potato_discord_check`: `let token = crate::secrets::read_secret(TOKEN).ok_or("DISCORD_NO_TOKEN: Paste the bot token first.")?;`
- `potato_discord_set_on(true)` checks, in this order, and returns the first error:
  1. `!discord_labs_on` -> `"POTATO_DISCORD_OFF: Turn on Potato Bot on Discord in Labs first."`
  2. sandbox not installed -> `"POTATO_OUTSIDE_NO_SANDBOX: Set up Fish's computer first. Discord messages only run there."`
  3. no token -> `"DISCORD_NO_TOKEN: Paste the bot token first."`
  4. `bot_user_id` empty -> `"DISCORD_NOT_CHECKED: Check the token first."`
  5. no place with mode not `off` and no DM rule other than `off` -> `"DISCORD_NO_PLACE: Pick at least one channel."`
  6. `outside.bot_id` empty -> `"DISCORD_NO_BOT: Pick which bot answers."`

`packages/app/src/lib/potato-discord.ts`:
```ts
import { invoke } from '@tauri-apps/api/core';

export type PlaceMode = 'off' | 'when_called' | 'every_message';
export type People = 'only_me' | 'me_and_listed' | 'anyone';
export type DmRule = 'only_me' | 'off' | 'same_as_people';
export type LinkStatus = 'off' | 'starting' | 'online' | 'reconnecting' | 'stopped_bad_token' | 'stopped_intent_off' | 'stopped_error';

export interface Place { id: string; name: string; mode: PlaceMode }
export interface RateConfig { per_person_turns: number; per_person_window_s: number; per_place_turns: number; per_place_window_s: number }
export interface OutsideConfig {
  bot_id: string; owner_ids: string[]; people: People; listed_ids: string[];
  places: Place[]; dms: DmRule; names: string[]; rate: RateConfig;
}
export interface GuildPick { id: string; name: string }
export interface DiscordConfig {
  on: boolean; botUserId: string; botName: string; applicationId: string;
  guilds: GuildPick[]; outside: OutsideConfig;
}
export interface SecretMask { set: boolean; last4?: string } // NEEDS-FACT: copy the real SecretMask fields from secrets.rs l.158
export interface DiscordView {
  config: DiscordConfig; token: SecretMask; labsOn: boolean; sandboxReady: boolean;
  link: LinkStatus; inviteUrl: string | null;
}
export interface BotIdentity { botUserId: string; botName: string; applicationId: string }
export interface GuildInfo { id: string; name: string }
export interface ChannelInfo { id: string; name: string; category: string | null }

export interface DiscordTransport {
  get(): Promise<DiscordView>;
  check(): Promise<BotIdentity>;
  guilds(): Promise<GuildInfo[]>;
  channels(guildId: string): Promise<ChannelInfo[]>;
  save(config: DiscordConfig): Promise<DiscordView>;
  setOn(on: boolean): Promise<DiscordView>;
}

export const tauriDiscordTransport: DiscordTransport = {
  get: () => invoke<DiscordView>('potato_discord_get'),
  check: () => invoke<BotIdentity>('potato_discord_check'),
  guilds: () => invoke<GuildInfo[]>('potato_discord_guilds'),
  channels: (guildId) => invoke<ChannelInfo[]>('potato_discord_channels', { guildId }),
  save: (config) => invoke<DiscordView>('potato_discord_save', { config }),
  setOn: (on) => invoke<DiscordView>('potato_discord_set_on', { on }),
};

export const DEFAULT_RATE: RateConfig = { per_person_turns: 6, per_person_window_s: 300, per_place_turns: 20, per_place_window_s: 600 };

export function emptyDiscordConfig(): DiscordConfig {
  return {
    on: false, botUserId: '', botName: '', applicationId: '', guilds: [],
    outside: { bot_id: '', owner_ids: [], people: 'only_me', listed_ids: [], places: [], dms: 'only_me', names: [], rate: { ...DEFAULT_RATE } },
  };
}

/** In-memory twin for component tests. */
export function fakeDiscordTransport(seed?: Partial<DiscordView>): DiscordTransport & { calls: string[] } {
  const calls: string[] = [];
  let view: DiscordView = {
    config: emptyDiscordConfig(), token: { set: false }, labsOn: true, sandboxReady: true,
    link: 'off', inviteUrl: null, ...seed,
  };
  return {
    calls,
    async get() { calls.push('get'); return view; },
    async check() {
      calls.push('check');
      if (!view.token.set) throw new Error('DISCORD_NO_TOKEN: Paste the bot token first.');
      view = { ...view, config: { ...view.config, botUserId: '42', botName: 'Fish', applicationId: '77' },
        inviteUrl: 'https://discord.com/oauth2/authorize?client_id=77&scope=bot&permissions=274877975552' };
      return { botUserId: '42', botName: 'Fish', applicationId: '77' };
    },
    async guilds() { calls.push('guilds'); return [{ id: '900', name: 'My server' }]; },
    async channels(id) { calls.push(`channels:${id}`); return [{ id: '100', name: 'general', category: 'Chat' }]; },
    async save(config) { calls.push('save'); view = { ...view, config }; return view; },
    async setOn(on) { calls.push(`setOn:${on}`); view = { ...view, config: { ...view.config, on }, link: on ? 'starting' : 'off' }; return view; },
  };
}
```

### Tests to write (`potato-discord.test.ts`)
| Test | Input | Expected |
|---|---|---|
| `empty config has safe defaults` | `emptyDiscordConfig()` | `on false`, `people 'only_me'`, `dms 'only_me'`, rate equals `DEFAULT_RATE` |
| `fake check needs a token` | fake with `token.set false`, `check()` | rejects with message containing `Paste the bot token first.` |
| `fake check fills bot identity and invite url` | fake with `token.set true`, `check()` then `get()` | `botName 'Fish'`, `inviteUrl` ends with `permissions=274877975552` |
| `fake setOn records the call` | `setOn(true)` | `calls` ends with `'setOn:true'`, `link 'starting'` |

Rust: add to `packages/app/src-tauri/src/commands/potato_discord.rs` a `#[cfg(test)] mod tests` with a pure helper `fn set_on_problem(view_inputs) -> Option<&'static str>` (the 6 checks as a pure function) and one test per check:
`set_on_needs_labs`, `set_on_needs_sandbox`, `set_on_needs_token`, `set_on_needs_check`, `set_on_needs_place`, `set_on_needs_bot`, `set_on_ok_when_all_set` (expects `None`).

### Run
```
cd packages/app && npx vitest run src/lib/potato-discord.test.ts
```
Expected: `Tests  4 passed (4)`.
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D6 cargo test --lib commands::potato_discord
```
Expected: `test result: ok. 7 passed; 0 failed`.

### Done when
- [ ] 6 commands registered in `lib.rs`.
- [ ] No command takes the token as an argument; the token is written only by the existing `secret_set`.
- [ ] TS and Rust tests pass.

### Do NOT
- Do not add a command that returns the token. Only the mask.
- Do not route these commands through the bridge. They are Tauri-only (desktop UI).
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `secret_set('discord_bot_token', ...)` is refused by `is_ui_secret` (secrets.rs l.38): add `"discord_bot_token"` to the UI-allowed list there, and add a `secrets` test `ui_may_set_discord_token`.
2. Tauri arg name mismatch (`guild_id` vs `guildId`): JS sends camelCase `guildId`; Rust param is `guild_id`. That is correct (facts 10d).
3. `SecretMask` field names differ: copy them from `secrets.rs` l.158 into the TS type and the fake.

---

## Slice D7 - Brain: the gateway client

### Goal
A small, testable Discord gateway client in the brain package that connects, identifies, heartbeats, resumes, and turns `MESSAGE_CREATE` into plain messages.

### Read first
- `packages/potato-brain/src/channels/potato.ts`: logging and error style, how timers are injected for tests (if at all).
- `packages/potato-brain/src/lifted/redact-secrets.ts`: the redaction function name.
- `packages/potato-brain/test/potato-channel.test.ts`: test style with `node:test`.

### Files
- create `packages/potato-brain/src/discord/gateway.ts`
- create `packages/potato-brain/test/discord-gateway.test.ts`

### Steps
1. Create `gateway.ts` (Code: full types and the state machine body).
2. Write the tests with a fake socket and fake timers (no network).

### Code

`packages/potato-brain/src/discord/gateway.ts`:
```ts
/** Discord gateway v10, JSON encoding, no compression. No dependencies: Node's built-in WebSocket. */
export const GATEWAY_URL = "wss://gateway.discord.gg/?v=10&encoding=json";
/** GUILDS (1) | GUILD_MESSAGES (512) | DIRECT_MESSAGES (4096) | MESSAGE_CONTENT (32768) */
export const INTENTS = 37377;
export const MAX_BACKOFF_MS = 60_000;

export interface SocketLike {
  send(data: string): void;
  close(code?: number): void;
  onopen: (() => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
  onclose: ((ev: { code: number }) => void) | null;
  onerror: ((ev: unknown) => void) | null;
}
export type SocketFactory = (url: string) => SocketLike;

export interface Timers {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  random(): number; // 0..1, for heartbeat jitter
}

export interface GatewayMessage {
  id: string;
  channelId: string;
  guildId: string | null;
  /** For a thread: the channel it belongs to. Null if not a thread or unknown. */
  parentChannelId: string | null;
  authorId: string;
  authorName: string;
  authorIsBot: boolean;
  content: string;
  mentionsMe: boolean;
  replyToAuthorId: string | null;
}

export type StopReason = "bad_token" | "intents_off" | "bad_config" | "stopped";
export type GatewayOut =
  | { kind: "ready"; botId: string; botName: string }
  | { kind: "guilds"; guildIds: string[] }
  | { kind: "message"; message: GatewayMessage }
  | { kind: "reconnecting"; attempt: number }
  | { kind: "stopped"; reason: StopReason };

export interface GatewayOptions {
  token: string;
  socket: SocketFactory;
  timers: Timers;
  onOut: (o: GatewayOut) => void;
  url?: string;
}

/** Close codes that must not be retried. */
export function fatalClose(code: number): StopReason | null {
  if (code === 4004) return "bad_token";
  if (code === 4013 || code === 4014) return "intents_off";
  if (code === 4010 || code === 4011 || code === 4012) return "bad_config";
  return null;
}

export function backoffMs(attempt: number): number {
  return Math.min(MAX_BACKOFF_MS, 1000 * 2 ** Math.max(0, attempt - 1));
}

export class Gateway {
  private ws: SocketLike | null = null;
  private seq: number | null = null;
  private sessionId: string | null = null;
  private resumeUrl: string | null = null;
  private botId = "";
  private heartbeatMs = 0;
  private hbTimer: unknown = null;
  private acked = true;
  private attempt = 0;
  private stopped = false;
  private reconnectTimer: unknown = null;
  /** thread id -> parent channel id, from GUILD_CREATE and THREAD_CREATE/UPDATE. Max 5000. */
  readonly parents = new Map<string, string>();
  private guildIds = new Set<string>();

  constructor(private readonly opts: GatewayOptions) {}

  start(): void { this.stopped = false; this.open(this.opts.url ?? GATEWAY_URL); }

  stop(): void {
    this.stopped = true;
    this.clearTimers();
    this.ws?.close(1000);
    this.ws = null;
    this.opts.onOut({ kind: "stopped", reason: "stopped" });
  }

  private open(url: string): void {
    const ws = this.opts.socket(url);
    this.ws = ws;
    ws.onmessage = (ev) => this.handle(typeof ev.data === "string" ? ev.data : String(ev.data));
    ws.onclose = (ev) => this.onClose(ev.code);
    ws.onerror = () => { /* onclose follows */ };
  }

  /** Public for tests. One gateway payload as text. */
  handle(raw: string): void {
    let p: { op: number; d: any; s: number | null; t: string | null };
    try { p = JSON.parse(raw); } catch { return; }
    if (typeof p.s === "number") this.seq = p.s;
    switch (p.op) {
      case 10: this.onHello(Number(p.d?.heartbeat_interval) || 41_250); break;
      case 11: this.acked = true; break;
      case 1: this.beat(); break;
      case 7: this.ws?.close(4000); break; // RECONNECT: close, onClose resumes
      case 9: if (!p.d) { this.sessionId = null; this.seq = null; } this.ws?.close(4000); break;
      case 0: this.onDispatch(p.t ?? "", p.d); break;
    }
  }

  private onHello(intervalMs: number): void {
    this.heartbeatMs = intervalMs;
    this.acked = true;
    this.schedule(Math.floor(intervalMs * this.opts.timers.random()));
    if (this.sessionId && this.seq !== null) {
      this.send({ op: 6, d: { token: this.opts.token, session_id: this.sessionId, seq: this.seq } });
    } else {
      this.send({ op: 2, d: { token: this.opts.token, intents: INTENTS,
        properties: { os: process.platform, browser: "puppet-master", device: "puppet-master" } } });
    }
  }

  private schedule(ms: number): void {
    this.hbTimer = this.opts.timers.setTimeout(() => {
      if (!this.acked) { this.ws?.close(4000); return; } // zombie connection
      this.beat();
      this.schedule(this.heartbeatMs);
    }, ms);
  }

  private beat(): void { this.acked = false; this.send({ op: 1, d: this.seq }); }

  private send(obj: unknown): void { this.ws?.send(JSON.stringify(obj)); }

  private onDispatch(t: string, d: any): void {
    switch (t) {
      case "READY":
        this.attempt = 0;
        this.sessionId = String(d?.session_id ?? "");
        this.resumeUrl = typeof d?.resume_gateway_url === "string" ? d.resume_gateway_url : null;
        this.botId = String(d?.user?.id ?? "");
        this.opts.onOut({ kind: "ready", botId: this.botId, botName: String(d?.user?.username ?? "") });
        break;
      case "RESUMED": this.attempt = 0; break;
      case "GUILD_CREATE":
        this.guildIds.add(String(d?.id));
        for (const th of Array.isArray(d?.threads) ? d.threads : []) this.remember(th);
        this.opts.onOut({ kind: "guilds", guildIds: [...this.guildIds] });
        break;
      case "GUILD_DELETE":
        this.guildIds.delete(String(d?.id));
        this.opts.onOut({ kind: "guilds", guildIds: [...this.guildIds] });
        break;
      case "THREAD_CREATE": case "THREAD_UPDATE": this.remember(d); break;
      case "MESSAGE_CREATE": {
        const m = toMessage(d, this.botId, this.parents);
        if (m) this.opts.onOut({ kind: "message", message: m });
        break;
      }
    }
  }

  private remember(th: any): void {
    if (typeof th?.id === "string" && typeof th?.parent_id === "string") {
      if (this.parents.size >= 5000) this.parents.clear();
      this.parents.set(th.id, th.parent_id);
    }
  }

  private onClose(code: number): void {
    this.clearTimers();
    this.ws = null;
    if (this.stopped) return;
    const fatal = fatalClose(code);
    if (fatal) { this.stopped = true; this.opts.onOut({ kind: "stopped", reason: fatal }); return; }
    this.attempt += 1;
    this.opts.onOut({ kind: "reconnecting", attempt: this.attempt });
    const url = this.sessionId && this.resumeUrl ? `${this.resumeUrl}/?v=10&encoding=json` : (this.opts.url ?? GATEWAY_URL);
    this.reconnectTimer = this.opts.timers.setTimeout(() => this.open(url), backoffMs(this.attempt));
  }

  private clearTimers(): void {
    if (this.hbTimer) this.opts.timers.clearTimeout(this.hbTimer);
    if (this.reconnectTimer) this.opts.timers.clearTimeout(this.reconnectTimer);
    this.hbTimer = null; this.reconnectTimer = null;
  }
}

/** Pure: a MESSAGE_CREATE payload to a GatewayMessage. Content capped at 4000 chars. */
export function toMessage(d: any, botId: string, parents: Map<string, string>): GatewayMessage | null {
  if (!d || typeof d.id !== "string" || typeof d.channel_id !== "string" || !d.author) return null;
  const mentions: any[] = Array.isArray(d.mentions) ? d.mentions : [];
  return {
    id: d.id,
    channelId: d.channel_id,
    guildId: typeof d.guild_id === "string" ? d.guild_id : null,
    parentChannelId: parents.get(d.channel_id) ?? null,
    authorId: String(d.author.id ?? ""),
    authorName: String(d.author.global_name ?? d.author.username ?? "").slice(0, 64),
    authorIsBot: d.author.bot === true || d.webhook_id !== undefined,
    content: String(d.content ?? "").slice(0, 4000),
    mentionsMe: botId !== "" && mentions.some((u) => u?.id === botId),
    replyToAuthorId: typeof d.referenced_message?.author?.id === "string" ? d.referenced_message.author.id : null,
  };
}
```
Note: messages from webhooks count as bots (`authorIsBot`), so another integration cannot drive the bot.

### Tests to write (`discord-gateway.test.ts`; `FakeSocket` records `sent: string[]` and exposes `fire(payload)`, `closeWith(code)`; `FakeTimers` keeps a list and has `runNext()`; `random()` returns `0.5`)
| Test | Input | Expected |
|---|---|---|
| `identifies after hello` | `start()`, fire `{op:10,d:{heartbeat_interval:1000}}` | last sent is op 2 with `intents: 37377` and the token |
| `first heartbeat uses jitter` | after hello, `runNext()` | sent op 1; the timer was scheduled at `500` ms |
| `zombie connection closes` | hello, `runNext()` (beat), no op 11, `runNext()` | socket closed with `4000` |
| `ready emits bot id` | dispatch `READY` with `user {id:"42", username:"Fish"}` | out `{kind:"ready", botId:"42", botName:"Fish"}` |
| `message is parsed` | dispatch `MESSAGE_CREATE` with `mentions [{id:"42"}]` | out message with `mentionsMe true`, `guildId` set |
| `webhook counts as bot` | `MESSAGE_CREATE` with `webhook_id: "1"` | `authorIsBot true` |
| `thread parent known` | `THREAD_CREATE {id:"555", parent_id:"100"}` then message in `555` | `parentChannelId "100"` |
| `resume after drop` | after READY (session "s1", seq 7) close `1006`, `runNext()`, fire hello | sent op 6 with `session_id "s1"`, `seq 7` |
| `bad token stops` | close `4004` | out `{kind:"stopped", reason:"bad_token"}`; no new socket after `runNext()` |
| `intents off stops` | close `4014` | `reason "intents_off"` |
| `backoff caps at 60s` | `backoffMs(1)`, `backoffMs(3)`, `backoffMs(20)` | `1000`, `4000`, `60000` |
| `invalid session not resumable re-identifies` | READY, then op 9 `d:false`, reconnect, hello | sent op 2 (not op 6) |
| `bad json ignored` | `handle("{")` | no throw, no out |
| `content capped` | content of 5000 chars | `content.length === 4000` |

### Run
```
cd packages/potato-brain && node --test test/discord-gateway.test.ts
```
Expected: `# pass 14` and `# fail 0`.
```
cd packages/potato-brain && npm run typecheck
```
Expected: exit 0.

### Done when
- [ ] Gateway never opens a real socket in tests.
- [ ] 14 tests pass; typecheck clean.

### Do NOT
- Do not add discord.js or any runtime dependency to `packages/potato-brain/package.json`.
- Do not log payloads (they contain message text and, for op 2/6, the token).
- Do not request typing, reaction or presence intents.

### If it fails
1. `process` not typed: `@types/node` is a dev dep already; add `/// <reference types="node" />` at the top only if typecheck complains.
2. Timer order in tests is wrong: `FakeTimers.runNext()` must run the earliest-scheduled pending timer; keep a list and pop index 0 after sorting by due time.
3. Resume URL test fails: the resume URL from READY has no query; the code appends `/?v=10&encoding=json`. Make the test's READY `resume_gateway_url` be `wss://resume.example` and expect `wss://resume.example/?v=10&encoding=json`.

---

## Slice D8 - Brain: REST sender and the link entry

### Goal
Add the sender (post a reply, show typing, obey rate limits) and the `node src/discord.ts` entry that connects the gateway to the app's bridge.

### Read first
- `packages/potato-brain/src/pm.ts`: env reading, loopback check for `PM_POTATO_URL`, exit codes (0, 2, 3, 1), secret redaction list.
- `packages/potato-brain/src/channels/potato.ts` l.271-420: `pollLoop`, `flushLoop`, the long-poll and POST shapes and headers (copy them).
- `packages/potato-brain/src/discord/gateway.ts` (D7).

### Files
- create `packages/potato-brain/src/discord/rest.ts`
- create `packages/potato-brain/src/discord/link.ts`
- create `packages/potato-brain/src/discord.ts`
- create `packages/potato-brain/test/discord-rest.test.ts`
- create `packages/potato-brain/test/discord-link.test.ts`

### Steps
1. `rest.ts`: `splitForDiscord` (pure) and `DiscordRest` with injected `fetch` (Code).
2. `link.ts`: `class DiscordLink` that wires gateway events to `POST {PM_POTATO_URL}/potato/discord/in` and long-polls `GET {PM_POTATO_URL}/potato/discord/out` (Code).
3. `discord.ts`: the entry (Code).
4. Tests with fake fetch and fake gateway.

### Code

`packages/potato-brain/src/discord/rest.ts`:
```ts
export const API = "https://discord.com/api/v10";
export const MAX_LEN = 2000;
export const MAX_PARTS = 3;
const UA = "DiscordBot (https://github.com/Potato-dev-inc/puppet-master, 1)";

/** Split on paragraph, then line, then space; never more than MAX_PARTS; the last part ends with "…" if cut. */
export function splitForDiscord(text: string): string[] {
  const out: string[] = [];
  let rest = text.trim();
  while (rest.length > 0 && out.length < MAX_PARTS) {
    if (rest.length <= MAX_LEN) { out.push(rest); rest = ""; break; }
    let cut = rest.lastIndexOf("\n\n", MAX_LEN);
    if (cut < MAX_LEN / 2) cut = rest.lastIndexOf("\n", MAX_LEN);
    if (cut < MAX_LEN / 2) cut = rest.lastIndexOf(" ", MAX_LEN);
    if (cut < MAX_LEN / 2) cut = MAX_LEN;
    out.push(rest.slice(0, cut).trimEnd());
    rest = rest.slice(cut).trimStart();
  }
  if (rest.length > 0 && out.length > 0) {
    const last = out[out.length - 1];
    out[out.length - 1] = (last.length >= MAX_LEN ? last.slice(0, MAX_LEN - 1) : last) + "…";
  }
  return out;
}

export type FetchLike = (url: string, init: { method: string; headers: Record<string, string>; body?: string }) =>
  Promise<{ status: number; text(): Promise<string> }>;

export class DiscordRest {
  constructor(private readonly token: string, private readonly fetch: FetchLike,
              private readonly sleep: (ms: number) => Promise<void>) {}

  /** Posts the reply. Mentions are never parsed: the bot can't ping @everyone, roles or users. */
  async send(channelId: string, content: string, replyTo: string | null): Promise<"ok" | "failed"> {
    if (!/^\d{1,32}$/.test(channelId)) return "failed";
    const parts = splitForDiscord(content);
    for (let i = 0; i < parts.length; i++) {
      const body: Record<string, unknown> = { content: parts[i], allowed_mentions: { parse: [], replied_user: false } };
      if (i === 0 && replyTo && /^\d{1,32}$/.test(replyTo)) body.message_reference = { message_id: replyTo, fail_if_not_exists: false };
      if ((await this.post(`/channels/${channelId}/messages`, body)) !== "ok") return "failed";
    }
    return "ok";
  }

  async typing(channelId: string): Promise<void> {
    if (/^\d{1,32}$/.test(channelId)) await this.post(`/channels/${channelId}/typing`, null);
  }

  /** One retry after a 429, waiting `retry_after` seconds, max 10 s. */
  private async post(path: string, body: unknown): Promise<"ok" | "failed"> {
    for (let attempt = 0; attempt < 2; attempt++) {
      const res = await this.fetch(API + path, {
        method: "POST",
        headers: { Authorization: `Bot ${this.token}`, "User-Agent": UA, "Content-Type": "application/json" },
        body: body === null ? undefined : JSON.stringify(body),
      });
      if (res.status >= 200 && res.status < 300) return "ok";
      if (res.status === 429 && attempt === 0) {
        let wait = 1;
        try { wait = Number(JSON.parse(await res.text()).retry_after) || 1; } catch { /* keep 1 */ }
        await this.sleep(Math.min(10, wait) * 1000);
        continue;
      }
      return "failed";
    }
    return "failed";
  }
}
```

`packages/potato-brain/src/discord/link.ts` (signatures and the wire shapes):
```ts
import type { GatewayOut } from "./gateway.ts";

/** Link -> app (POST /potato/discord/in). */
export type LinkEvent =
  | { kind: "ready"; botId: string; botName: string }
  | { kind: "guilds"; guildIds: string[] }
  | { kind: "message"; id: string; channelId: string; guildId: string | null; parentChannelId: string | null;
      authorId: string; authorName: string; authorIsBot: boolean; content: string;
      mentionsMe: boolean; replyToBot: boolean }
  | { kind: "reconnecting"; attempt: number }
  | { kind: "stopped"; reason: string }
  | { kind: "send_failed"; channelId: string };

/** App -> link (GET /potato/discord/out, long poll up to 25 s). */
export type LinkItem =
  | { kind: "send"; channelId: string; content: string; replyTo: string | null }
  | { kind: "typing"; channelId: string }
  | { kind: "stop" };

export interface LinkOptions {
  baseUrl: string;              // PM_POTATO_URL, loopback only
  linkToken: string;            // PM_DISCORD_LINK_TOKEN
  fetch: (url: string, init: RequestInit) => Promise<Response>;
  rest: { send(c: string, t: string, r: string | null): Promise<"ok" | "failed">; typing(c: string): Promise<void> };
  gateway: { start(): void; stop(): void };
}

export function toLinkEvent(o: GatewayOut, botId: string): LinkEvent; // message: replyToBot = replyToAuthorId === botId

export class DiscordLink {
  constructor(opts: LinkOptions);
  /** Gateway callback. Queues events; flushes at most every 250 ms in one POST {"v":1,"events":[...]}. Queue cap 500 (oldest dropped). */
  onGateway(o: GatewayOut): void;
  /** Runs until a `stop` item, a 401/403 from the app, or the app is gone (3 failed polls in a row). Returns the exit code. */
  run(): Promise<0 | 3>;
}
```
Headers to the app: copy exactly what `potato.ts` sends for the brain (NEEDS-FACT: the header name that carries `PM_POTATO_TOKEN`), but with `linkToken`.

`packages/potato-brain/src/discord.ts` (whole file):
```ts
/** Entry Puppet Master starts for "Potato Bot on Discord": node src/discord.ts
 *  Env: PM_POTATO_URL (loopback http), PM_DISCORD_LINK_TOKEN, DISCORD_BOT_TOKEN.
 *  Exit: 0 after stop, 2 missing env, 3 app gone, 4 Node has no WebSocket, 1 start error. */
import { Gateway } from "./discord/gateway.ts";
import { DiscordRest } from "./discord/rest.ts";
import { DiscordLink } from "./discord/link.ts";

function env(name: string): string { return (process.env[name] ?? "").trim(); }

async function main(): Promise<number> {
  const base = env("PM_POTATO_URL");
  const linkToken = env("PM_DISCORD_LINK_TOKEN");
  const token = env("DISCORD_BOT_TOKEN");
  if (!base || !linkToken || !token) return 2;
  const u = new URL(base);
  if (u.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(u.hostname)) return 2;
  if (typeof (globalThis as any).WebSocket !== "function") return 4;

  const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
  const rest = new DiscordRest(token, (url, init) => fetch(url, init), sleep);
  let link: DiscordLink;
  const gateway = new Gateway({
    token,
    socket: (url) => new (globalThis as any).WebSocket(url),
    timers: { setTimeout: (f, ms) => setTimeout(f, ms), clearTimeout: (h) => clearTimeout(h as any), random: Math.random },
    onOut: (o) => link.onGateway(o),
  });
  link = new DiscordLink({ baseUrl: base, linkToken, fetch: (a, b) => fetch(a, b), rest, gateway });
  gateway.start();
  return link.run();
}

main().then((code) => process.exit(code), () => process.exit(1));
```
The entry prints nothing on success. On a start error it exits 1 without printing the error object (it can contain the token).

### Tests to write
`discord-rest.test.ts`:
| Test | Input | Expected |
|---|---|---|
| `short text one part` | `"hi"` | `["hi"]` |
| `splits on paragraph` | 1500 a's + `"\n\n"` + 1500 b's | 2 parts, first is 1500 a's |
| `caps at three parts with ellipsis` | 9000 chars no spaces | 3 parts, each `<= 2000`, last ends with `"…"` |
| `never parses mentions` | `send("100", "@everyone hi", null)` | fake fetch body has `allowed_mentions: {parse: [], replied_user: false}` |
| `reply reference only on first part` | 2-part text, replyTo `"55"` | first body has `message_reference.message_id "55"`, second has none |
| `retries once on 429` | fetch returns 429 `{"retry_after":0.5}` then 200 | `"ok"`, sleep called with `500` |
| `gives up after second 429` | 429 twice | `"failed"` |
| `rejects bad channel id` | `send("../x", "hi", null)` | `"failed"`, 0 fetch calls |
| `auth header is Bot token` | any send | header `Authorization: "Bot T"` for token `"T"` |

`discord-link.test.ts`:
| Test | Input | Expected |
|---|---|---|
| `message event marks replyToBot` | `toLinkEvent({kind:"message", message:{... replyToAuthorId:"42"}}, "42")` | `replyToBot true` |
| `send item posts to discord` | fake app returns one `send` item, then `stop` | rest.send called once with the channel and content; `run()` resolves `0` |
| `stop item stops gateway` | app returns `stop` | gateway.stop called; resolves `0` |
| `app gone after three failed polls` | fetch throws 3 times | resolves `3` |
| `send failure reported` | rest.send returns `"failed"` | next POST to `/potato/discord/in` has `{kind:"send_failed", channelId}` |
| `event queue capped` | 600 events before flush | posted events length `500`, oldest dropped |

### Run
```
cd packages/potato-brain && node --test test/discord-rest.test.ts test/discord-link.test.ts test/discord-gateway.test.ts
```
Expected: `# fail 0` (29 pass).
```
cd packages/potato-brain && npm run typecheck
```
Expected: exit 0.

### Done when
- [ ] `node src/discord.ts` with no env exits `2` (check: `cd packages/potato-brain && node src/discord.ts; echo $?` prints `2`).
- [ ] Every Discord POST has `allowed_mentions.parse = []`.
- [ ] All tests pass.

### Do NOT
- Do not call the real Discord API or a real gateway in tests.
- Do not print anything to stdout/stderr in `discord.ts` or `link.ts` except nothing (the app reads exit codes).
- Do not accept a non-loopback `PM_POTATO_URL`.

### If it fails
1. `fetch` type mismatch between `FetchLike` and global `fetch`: wrap with `(url, init) => fetch(url, init as RequestInit)`.
2. Link test hangs: the long poll needs the fake to return immediately; make the fake app return `{"v":1,"items":[]}` and then `stop`.
3. Exit code test shows `1` instead of `2`: `new URL("")` throws before the check; the empty-env check must run first (it does in the code above; keep the order).

---

## Slice D9 - Rust: bridge routes, outbox and the link keeper

### Goal
Run the link process while Discord is on, accept its events on `POST /potato/discord/in`, feed admitted messages into `Service::send_outside`, and serve replies on `GET /potato/discord/out`.

### Read first
- `packages/app/src-tauri/src/bridge/routes/potato_brain.rs` l.23: `respond(service, bot, method, segments, query, body)`; copy its shape.
- `packages/app/src-tauri/src/bridge/http.rs` l.123 and l.370: the path guard for `/potato/brain/*`. The new prefix `/potato/discord/` needs the same guard.
- `packages/app/src-tauri/src/potato/auth.rs`: how `/potato/brain/*` callers are checked. The link gets its own token.
- `packages/app/src-tauri/src/potato/sidecar.rs` l.124-200: `SpawnSpec`, `Launcher`, `RealLauncher`, `script_path`.
- `packages/app/src-tauri/src/potato/env.rs` l.25-160: `build(&Inputs)`; the env is built from nothing (allowlist).
- `packages/app/src-tauri/src/potato/keeper.rs`: `backoff_ms`.
- `packages/app/src-tauri/src/potato/service.rs` l.1388 `inbox_poll` (copy the long-poll wait).

### Files
- create `packages/app/src-tauri/src/potato/discord/link_keeper.rs` (pure state machine + its tests in `link_keeper_tests.rs`, declared in `discord/mod.rs`)
- create `packages/app/src-tauri/src/potato/discord/bridge.rs` (outbox, `DiscordSink: OutsideSink`, event handling)
- create `packages/app/src-tauri/src/bridge/routes/potato_discord.rs`
- edit `packages/app/src-tauri/src/bridge/http.rs` (route + guard, 2 places)
- edit `packages/app/src-tauri/src/potato/live.rs` (start/stop the link on settings change; give `Service` the `DiscordSink`)

This slice touches 5 source files plus one test file. It is Sonnet-sized. If it runs long, split: D9a = `link_keeper.rs` + tests; D9b = the rest.

### Steps
1. `link_keeper.rs`: pure `LinkKeeper` (Code).
2. `bridge.rs`: `Outbox`, `DiscordSink`, `handle_in(events)` (Code).
3. `routes/potato_discord.rs`: `respond(...)` for `POST in` and `GET out` (Code).
4. `http.rs`: route `/potato/discord/` to the new module behind the same guard as `/potato/brain/`, with link-token auth.
5. `live.rs`: in `apply_settings(public)`, compute `want = discord_labs_on(public) && cfg.on && token_set && sandbox_installed` and call `link_keeper.want(want)`; in the existing 1 s/30 s tick, call `link_keeper.step(now)` and act on its action. Spawn with `SpawnSpec` for script `discord.ts` (NEEDS-FACT: whether `script_path` can return a second script, or the bundle name used in release builds) and env:
   - from `env::build` style allowlist: only `PATH`, `SystemRoot`, `TEMP`, `TMP` (same list the brain gets; copy), plus
   - `PM_POTATO_URL` = `bridge_base()`, `PM_DISCORD_LINK_TOKEN` = a fresh random 32-byte hex per spawn, `DISCORD_BOT_TOKEN` = `read_secret("discord_bot_token")`.
6. Give `Service` a `DiscordSink` (via the `OutsideSink` slot from D4).
7. Tests.

### Code

`packages/app/src-tauri/src/potato/discord/link_keeper.rs`:
```rust
use crate::commands::potato_discord::LinkStatus; // or move LinkStatus here and re-export from the command file

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action { Nothing, Spawn, Kill }

#[derive(Debug)]
pub struct LinkKeeper {
    want: bool,
    running: bool,
    status: LinkStatus,
    failures: u32,
    next_try_ms: u64,
}

impl LinkKeeper {
    pub fn new() -> Self { Self { want: false, running: false, status: LinkStatus::Off, failures: 0, next_try_ms: 0 } }
    pub fn status(&self) -> LinkStatus { self.status }
    /// Settings changed. Turning on clears a "stopped" status so a fixed token is tried again.
    pub fn want(&mut self, on: bool) {
        if on && !self.want { self.failures = 0; self.next_try_ms = 0; if !self.running { self.status = LinkStatus::Off; } }
        self.want = on;
    }
    pub fn spawned(&mut self) { self.running = true; self.status = LinkStatus::Starting; }
    /// From a link event.
    pub fn ready(&mut self) { self.failures = 0; self.status = LinkStatus::Online; }
    pub fn reconnecting(&mut self) { self.status = LinkStatus::Reconnecting; }
    /// Process ended. Codes: 0 stop, 2 env, 3 app gone, 4 old node, 1 error. `reason` from the last `stopped` event.
    pub fn exited(&mut self, code: i32, reason: Option<&str>, now_ms: u64) {
        self.running = false;
        self.status = match reason {
            Some("bad_token") => { self.want = false; LinkStatus::StoppedBadToken }
            Some("intents_off") => { self.want = false; LinkStatus::StoppedIntentOff }
            _ if code == 0 && !self.want => LinkStatus::Off,
            _ if code == 2 || code == 4 => { self.want = false; LinkStatus::StoppedError }
            _ => { self.failures += 1; self.next_try_ms = now_ms + crate::potato::keeper::backoff_ms(self.failures); LinkStatus::Reconnecting }
        };
    }
    pub fn step(&mut self, now_ms: u64) -> Action {
        match (self.want, self.running) {
            (true, false) if now_ms >= self.next_try_ms => Action::Spawn,
            (false, true) => Action::Kill,
            _ => Action::Nothing,
        }
    }
}
```
NEEDS-FACT: the exact signature of `keeper::backoff_ms` (argument type). Adapt the call.

`packages/app/src-tauri/src/potato/discord/bridge.rs` (signatures):
```rust
pub const OUTBOX_CAP: usize = 200;
pub const POLL_WAIT_MS: u64 = 25_000;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkItem {
    Send { #[serde(rename = "channelId")] channel_id: String, content: String, #[serde(rename = "replyTo")] reply_to: Option<String> },
    Typing { #[serde(rename = "channelId")] channel_id: String },
    Stop,
}

/// Mutex<VecDeque<LinkItem>> + Condvar. Oldest dropped past OUTBOX_CAP.
pub struct Outbox { /* fields */ }
impl Outbox {
    pub fn new() -> Self;
    pub fn push(&self, item: LinkItem);
    /// Waits up to `wait_ms` for at least one item; returns all queued items (max 50).
    pub fn take(&self, wait_ms: u64) -> Vec<LinkItem>;
}

pub struct DiscordSink { pub outbox: std::sync::Arc<Outbox> }
impl crate::potato::outside::engine::OutsideSink for DiscordSink {
    fn send_text(&self, t: &OutsideReplyTarget, text: &str);   // only when t.surface == Surface::Discord
    fn typing(&self, t: &OutsideReplyTarget);
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkEvent { Ready { #[serde(rename="botId")] bot_id: String, #[serde(rename="botName")] bot_name: String },
    Guilds { #[serde(rename="guildIds")] guild_ids: Vec<String> },
    Message(InMessage), Reconnecting { attempt: u32 }, Stopped { reason: String },
    SendFailed { #[serde(rename="channelId")] channel_id: String },
    #[serde(other)] Unknown }

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InMessage { pub id: String, pub channel_id: String, pub guild_id: Option<String>, pub parent_channel_id: Option<String>,
    pub author_id: String, pub author_name: String, pub author_is_bot: bool, pub content: String,
    pub mentions_me: bool, pub reply_to_bot: bool }

/// For each `Message`: guild filter -> outside::admit -> Service::send_outside. Returns how many were admitted.
/// Max 50 events per POST; extra events are dropped. Text is cleaned with `store::clean_text`.
pub fn handle_in(events: Vec<LinkEvent>, cfg: &DiscordConfig, rate: &mut RateBook, held: bool, now_ms: u64,
                 service: &Service, keeper: &mut LinkKeeper, recent: &mut Recent) -> usize;
```
For each admitted message, `handle_in` first pushes `LinkItem::Typing` for that channel, then calls `service.send_outside(&cfg.outside.bot_id, &text, origin)`. A DM has `guild_id: None`; its `place_id` for `admit` is `None` and the reply target `place_id` is the DM channel id.

`packages/app/src-tauri/src/bridge/routes/potato_discord.rs`:
```rust
/// POST /potato/discord/in   body {"v":1,"events":[...]}  -> 204
/// GET  /potato/discord/out  -> 200 {"v":1,"items":[...]} (waits up to 25 s)
/// Anything else -> 404. Body over 256 KB -> 413. `v` != 1 -> 400.
pub fn respond(state: &DiscordBridgeState, method: &str, segments: &[&str], body: &[u8]) -> (u16, String);
```
Auth: compare the request's link token (same header the brain uses) with the current `PM_DISCORD_LINK_TOKEN` in constant time. Wrong or missing -> 401. Token is rotated on every spawn, so an old link process cannot talk.

### Tests to write
`link_keeper_tests.rs`:
| Test | Input | Expected |
|---|---|---|
| `spawns_when_wanted` | `want(true)`, `step(0)` | `Spawn` |
| `kills_when_unwanted` | `spawned()`, `want(false)`, `step(0)` | `Kill` |
| `bad_token_stops_retrying` | `spawned()`, `exited(1, Some("bad_token"), 0)`, `step(10_000_000)` | status `StoppedBadToken`, action `Nothing` |
| `intents_off_stops` | same with `"intents_off"` | `StoppedIntentOff` |
| `crash_backs_off` | `spawned()`, `exited(1, None, 1000)`, `step(1000)` | `Nothing`; `step(1000 + backoff_ms(1))` gives `Spawn` |
| `turning_on_again_clears_stop` | after bad_token: `want(false)`, `want(true)`, `step(0)` | `Spawn` |
| `old_node_stops` | `exited(4, None, 0)` | `StoppedError`, no retry |

`packages/app/src-tauri/src/potato/discord/bridge_tests.rs`:
| Test | Input | Expected |
|---|---|---|
| `outbox_caps_and_drops_oldest` | push 201 `Typing` | `take(0)` returns 50 items, first is item #2 |
| `outbox_take_waits_then_returns_empty` | empty, `take(10)` | empty vec after about 10 ms |
| `unknown_event_ignored` | JSON `{"kind":"future_thing"}` | parses as `Unknown`, no panic |
| `guild_not_picked_dropped` | message with guild `"999"` not in config | 0 admitted, 0 outbox items, recent has `PlaceNotPicked` row |
| `admitted_message_sends_typing_then_outside` | owner mention in picked channel, fake service | outbox has `Typing{channel}`; fake service got `send_outside` with conv `discord:ch:100` |
| `discord_sink_ignores_telegram_target` | `send_text` with `Surface::Telegram` | outbox empty |

`packages/app/src-tauri/src/bridge/routes/potato_discord_tests.rs`:
| Test | Input | Expected |
|---|---|---|
| `in_needs_link_token` | POST without token | `401` |
| `in_rejects_big_body` | 300 KB body | `413` |
| `in_rejects_wrong_version` | `{"v":2,"events":[]}` | `400` |
| `out_returns_items` | push one `Send`, GET out | `200`, body `{"v":1,"items":[{"kind":"send","channelId":"100","content":"hi","replyTo":"55"}]}` |
| `unknown_path_404` | GET `/potato/discord/x` | `404` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D9 cargo test --lib potato::discord
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D9 cargo test --lib bridge::routes::potato_discord
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D9 cargo test --lib p2p::guard
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D9 cargo test --lib desk::guard
```
Expected: each ends `test result: ok.` with 0 failed. The two guard runs must stay green.

### Done when
- [ ] With Labs, Sandbox, token, config `on`: the keeper spawns `node src/discord.ts` hidden (same launcher as the brain).
- [ ] Turning Discord off sends `LinkItem::Stop` first, then kills the PID after 5 s if it is still alive (only that PID).
- [ ] `/potato/discord/*` is guarded like `/potato/brain/*` and needs the per-spawn link token.
- [ ] All tests above pass; p2p and desk guards green.

### Do NOT
- Do not put `DISCORD_BOT_TOKEN` in an argument or a file. Env only, and only for the link process (never for the brain process).
- Do not give the link process `DEEPSEEK_API_KEY`.
- Do not kill by image name (`node.exe`). Kill only the PID the launcher returned.
- Do not show a window: use the same hidden spawn as `RealLauncher`.
- Do not route anything from the phone, MCP or agents into `/potato/discord/*`.
- Do not use the default `CARGO_TARGET_DIR`.
- This slice needs plan 15 slice R6 (Sandbox egress limit) merged first: strangers must not reach the home network through Fish's browser.

### If it fails
1. The bridge path guard rejects `/potato/discord/in` from loopback: add the prefix to the same allow branch as `/potato/brain/` at `http.rs` l.123 and l.370; do not loosen any other path.
2. The brain's env test (`potato/env_tests.rs`, which mentions `discord_bot_token`) fails: it may assert the brain env never has `DISCORD_BOT_TOKEN`. That must stay true. Build the link env with a separate function `env::build_link(&LinkInputs)`; do not add the token to `env::build`.
3. Link starts and exits 2 at once: `script_path` returned the brain script dir but the release bundle names the script differently (facts 11: `brain.mjs` vs `src/pm.ts`). Check `resource_dirs.rs` and use the same naming rule for `discord.ts` (or `discord.mjs` in release).

---

## Slice D10 - Settings page with the invite steps

### Goal
A Settings page "Potato Bot on Discord" that walks the person through making the bot, pasting the token, inviting it, picking channels and people, and turning it on.

### Read first
- `packages/app/src/components/settings/potato/PotatoSubPages.tsx`: how a Potato sub-page is registered and opened.
- `packages/app/src/components/settings/sections/PotatoSection.tsx` l.120-140: the DeepSeek key row and Labs row (copy the key row for the token).
- `packages/app/src/components/settings/sections/GeneralSection.tsx` l.496-497: the Labs toggle pattern.
- `packages/app/src/components/settings/sandbox/SandboxWizard.tsx` l.34-130: the step-by-step pattern (`stepsFor`).
- `packages/app/src/lib/secrets.ts`: how `deepseek_api_key` is written.
- `docs/design/theme.md`: tokens. Monochrome only.

### Files
- create `packages/app/src/components/settings/potato/PotatoDiscordPage.tsx`
- create `packages/app/src/components/settings/potato/PotatoDiscordPage.test.tsx`
- create `packages/app/src/lib/potato-discord-copy.ts` (all UI text in one place)
- edit `packages/app/src/components/settings/potato/PotatoSubPages.tsx` (add the sub-page and its row)
- edit `packages/app/src/styles/potato-settings-pages.css` (only `.pdc-*` rules, monochrome tokens)

### Steps
1. Create `potato-discord-copy.ts` with every string below (Code).
2. Create `PotatoDiscordPage.tsx` with props `{ transport?: DiscordTransport; secrets?: { set(name: string, value: string): Promise<void> }; bots: { id: string; name: string }[] }`. It renders the 7 steps as one scrolling page with numbered sections; a step below an unfinished step is shown but its controls are disabled.
3. Register the sub-page in `PotatoSubPages.tsx` with the row title `Discord` and subtitle from `COPY.rowSubtitle(view)`.
4. Add `.pdc-*` CSS using only `var(--label...)`, `var(--sep...)`, `var(--focus...)`, `var(--ease...)`.
5. Write the component tests with `fakeDiscordTransport`.

### Code

`packages/app/src/lib/potato-discord-copy.ts` (whole file):
```ts
import type { LinkStatus } from './potato-discord';

export const PORTAL_URL = 'https://discord.com/developers/applications';

export const COPY = {
  title: 'Potato Bot on Discord',
  intro: 'Let one of your bots answer on your Discord server. Everything it does for Discord happens on its own computer, never on this PC.',
  needsLabs: 'Turn on "Potato Bot on Discord" in Labs first.',
  needsSandbox: "Discord needs Fish's computer. Set it up first in Settings > Potato Bot > Fish's computer.",
  labsRow: 'Potato Bot on Discord (Labs)',

  step1Title: '1. Make a bot in Discord',
  step1Lines: [
    'Open the Discord Developer Portal and sign in.',
    'Click New Application. Give it a name, for example Fish. Tick the box to agree, then click Create.',
    'On the left, click Bot.',
    'Under Privileged Gateway Intents, turn on Message Content Intent. Click Save Changes.',
    'Click Reset Token, then Yes, do it! Copy the token that appears.',
  ],
  step1Button: 'Open Discord Developer Portal',

  step2Title: '2. Paste the bot token',
  tokenLabel: 'Bot token',
  tokenHelp: "Puppet Master keeps the token in your computer's password store. It is only sent to Discord.",
  tokenSave: 'Save token',
  tokenSaved: (last4: string) => `Saved. Ends in ${last4}.`,
  checkButton: 'Check token',
  checking: 'Checking…',
  checkOk: (name: string) => `Token works. Your bot is called ${name}.`,

  step3Title: '3. Invite the bot to your server',
  step3Lines: [
    'Click Open invite link. Discord opens in your browser.',
    'Pick your server, click Continue, then Authorize.',
    'You need the Manage Server permission on that server.',
  ],
  copyInvite: 'Copy invite link',
  openInvite: 'Open invite link',
  copied: 'Copied.',
  refreshServers: 'I invited it',

  step4Title: '4. Pick where the bot may talk',
  noServers: 'The bot is not in any server yet. Invite it, then click I invited it.',
  modeOff: 'Off',
  modeWhenCalled: 'When called',
  modeEvery: 'Every message',
  modeHelp: 'When called means: someone @mentions the bot, replies to it, or says its name.',
  namesLabel: 'Names that call the bot',
  namesHelp: 'Separate names with commas. Example: fish, potato',

  step5Title: '5. Who may talk to the bot',
  peopleOnlyMe: 'Only me',
  peopleListed: 'Me and people I list',
  peopleAnyone: 'Anyone in the picked channels',
  myIdLabel: 'My Discord user ID',
  myIdHelp: 'In Discord, open User Settings > Advanced and turn on Developer Mode. Then right-click your name and click Copy User ID.',
  listedLabel: 'Other people (Discord user IDs)',
  listedHelp: 'One ID per line.',
  dmsLabel: 'Direct messages',
  dmsOnlyMe: 'Only from me',
  dmsSame: 'From the same people as above',
  dmsOff: 'Off',
  anyoneWarning: 'Anyone in those channels can ask the bot to use its computer and its browser. It can never reach this PC.',

  step6Title: '6. Limits',
  perPersonLabel: 'Messages per person',
  perPersonHelp: (n: number, min: number) => `${n} every ${min} minutes. You are never limited.`,
  perPlaceLabel: 'Messages per channel',
  perPlaceHelp: (n: number, min: number) => `${n} every ${min} minutes.`,
  costNote: 'The daily spending limit in Settings > Potato Bot > Usage also applies.',

  step7Title: '7. Turn it on',
  botLabel: 'Bot that answers',
  onLabel: 'Answer on Discord',
  saved: 'Saved.',

  status: (s: LinkStatus): string => ({
    off: 'Off',
    starting: 'Connecting to Discord…',
    online: 'Online on Discord',
    reconnecting: 'Lost the connection. Trying again…',
    stopped_bad_token: 'Stopped: Discord did not accept the token. Paste a new one.',
    stopped_intent_off: 'Stopped: turn on Message Content Intent on the Bot page.',
    stopped_error: 'Stopped: something went wrong. Turn it off and on again.',
  })[s],

  rowSubtitle: (on: boolean, s: LinkStatus) => (on ? COPY.status(s) : 'Off'),
} as const;
```

Component behaviour (exact):
- Step 2 `Save token` calls `secrets.set('discord_bot_token', value)` then clears the field (the field never shows a saved token; it shows `tokenSaved(last4)`).
- `Check token` calls `transport.check()`; on success shows `checkOk(name)`; on error shows `plainError(error)` text.
- `Open Discord Developer Portal` and `Open invite link` open the URL in the system browser with the same helper the app uses for external links (NEEDS-FACT: the open-URL helper name, for example `@tauri-apps/plugin-opener`).
- Step 4 lists `transport.guilds()`; for each server, a section with its channels from `transport.channels(id)` grouped by `category`; each channel row has a 3-way segmented control `Off | When called | Every message`. Picking a channel also adds the server to `guilds`.
- Every change calls `transport.save(config)` debounced 400 ms.
- Step 5: `Anyone` shows `anyoneWarning` under the choice in normal label colour (no red).
- Step 7: the switch calls `transport.setOn(value)`; an error shows `plainError` text under the switch and the switch goes back.

### Tests to write (`PotatoDiscordPage.test.tsx`)
| Test | Setup | Expected |
|---|---|---|
| `shows labs message when labs off` | fake `labsOn: false` | text `Turn on "Potato Bot on Discord" in Labs first.`; no step controls enabled |
| `shows sandbox message when sandbox not ready` | `sandboxReady: false` | text starting `Discord needs Fish's computer.` |
| `token is saved to the credential store` | type `abc.def.ghi`, click `Save token` | `secrets.set` called with `('discord_bot_token', 'abc.def.ghi')`; field is empty after |
| `check shows bot name` | token set, click `Check token` | text `Token works. Your bot is called Fish.` |
| `invite buttons appear after check` | after check | buttons `Copy invite link` and `Open invite link` present |
| `picking a channel saves it` | after `I invited it`, click `When called` on `general` | last `save` config has `outside.places = [{id:'100', name:'general', mode:'when_called'}]` and `guilds = [{id:'900', name:'My server'}]` |
| `anyone shows the warning` | click `Anyone in the picked channels` | warning text visible |
| `turn on calls setOn` | all set, toggle `Answer on Discord` | `calls` contains `'setOn:true'`; status text `Connecting to Discord…` |
| `no coloured classes` | render | no element has a class containing `red`, `green`, `blue`, or `orange` |

### Run
```
cd packages/app && npx vitest run src/components/settings/potato/PotatoDiscordPage.test.tsx
```
Expected: `Tests  9 passed (9)`.
```
cd packages/app && npx vitest run src/components/settings/potato
```
Expected: all pass, 0 failed.

### Done when
- [ ] The page is reachable from Settings > Potato Bot > Discord when `potato_discord_enabled` is on.
- [ ] Every string comes from `potato-discord-copy.ts`.
- [ ] The token is never shown back, never stored in `discord.json`, never in a URL.
- [ ] 9 tests pass; checked visually with the preview tools (not on the primary screen) in light and dark.

### Do NOT
- Do not use colour for status. Use words and the existing label weights.
- Do not open the app or a dev server on the primary screen; use the preview tools.
- Do not put the token or user IDs in a URL or query string.
- Do not add a new Settings section in `catalog.ts`; this is a sub-page of Potato Bot.

### If it fails
1. Debounced save makes the test flaky: use `vi.useFakeTimers()` and `vi.advanceTimersByTime(400)` before asserting `save`.
2. `PotatoSubPages.tsx` has a fixed list type: add `'discord'` to its page id union.
3. The open-URL helper is not mockable: wrap it in a prop `openUrl?: (url: string) => void` with the real helper as default, and pass a `vi.fn()` in tests.

---

## Slice D11 - Recent activity and the full-path fake test

### Goal
Show the last 50 Discord events (admitted, dropped and why, replied) on the Discord page, and prove the whole path with fakes from a gateway message to an outbox reply.

### Read first
- `packages/app/src-tauri/src/potato/log.rs`: the 200-line log ring (copy its ring style).
- `packages/app/src-tauri/src/potato/discord/bridge.rs` (D9).
- `packages/app/src-tauri/src/potato/service_tests.rs`: the fake Host and fake Launcher.

### Files
- create `packages/app/src-tauri/src/potato/discord/recent.rs`
- edit `packages/app/src-tauri/src/commands/potato_discord.rs` (add `potato_discord_recent`) and register it in `packages/app/src-tauri/src/lib.rs`
- create `packages/app/src-tauri/src/potato/discord/full_path_tests.rs`
- edit `packages/app/src/components/settings/potato/PotatoDiscordPage.tsx` (a "Recent" list at the bottom)

### Steps
1. `recent.rs`: ring of 50 `RecentRow { at_ms, kind, place_name, person, note }` (Code). It never stores message text.
2. `bridge.rs` (already calls `recent`): record one row per message: `Admitted`, `Dropped(<Drop name>)`, `Replied`, `SendFailed`.
3. Command `potato_discord_recent() -> Vec<RecentRow>`; add `recent()` to `DiscordTransport` and the fake.
4. Page: section "Recent" with up to 50 rows, newest first, text from `recentLine(row)`.
5. Full-path test.

### Code
```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentRow {
    pub at_ms: u64,
    pub kind: RecentKind,
    pub place: String,     // "#general" or "Direct message"
    pub person: String,    // display name, max 64 chars, or "Someone" for non-allowed people
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentKind { Answered, NotCalled, NotAllowed, ChannelNotPicked, TooMany, Paused, NeedsComputer, SendFailed }

pub const RECENT_CAP: usize = 50;
pub struct Recent { rows: std::collections::VecDeque<RecentRow> }
impl Recent { pub fn new() -> Self; pub fn push(&mut self, r: RecentRow); pub fn list(&self) -> Vec<RecentRow> /* newest first */; }
pub fn kind_for(d: &crate::potato::outside::admit::Drop) -> Option<RecentKind>; // FromBot and EmptyText -> None (not shown)
```
UI text (`potato-discord-copy.ts`, add):
```ts
recentTitle: 'Recent',
recentEmpty: 'Nothing yet.',
recentLine: (kind: string, person: string, place: string) => ({
  answered: `Answered ${person} in ${place}`,
  not_called: `${person} wrote in ${place} without calling the bot`,
  not_allowed: `${person} is not on the list. No answer in ${place}`,
  channel_not_picked: `A message in a channel you did not pick`,
  too_many: `${person} sent too many messages. No answer for now`,
  paused: `Paused. No answer to ${person}`,
  needs_computer: `Could not answer ${person}: the bot's computer is off`,
  send_failed: `Could not post the answer in ${place}`,
} as Record<string, string>)[kind] ?? '',
```
People who are not allowed are shown as `Someone` (privacy: the page never builds a list of strangers' names).

### Tests to write
`full_path_tests.rs` (`full_path_owner_mention_gets_reply`): fake Host with sandbox on + installed; fake Launcher that records brain spawns; config with owner `"1"`, place `"100"` `WhenCalled`, bot `"b1"`.
1. `handle_in` with one `Message` from `"1"` in `"100"` with `mentions_me: true`, content `"hello"`.
2. Assert the brain inbox has one item with `sandboxOnly: true` and `conversationId: "discord:ch:100"`.
3. Ingest a fake `Reply` event for that item id with segments `["hi there"]`.
4. Assert `outbox.take(0)` equals `[Typing{channel_id:"100"}, Send{channel_id:"100", content:"hi there", reply_to:Some(<message id>)}]`.
5. Assert `recent.list()[0].kind == Answered`.

Second test `full_path_stranger_dropped_and_logged_as_someone`: people `OnlyMe`, message from `"9"` named `"Eve"` -> outbox empty, recent row `NotAllowed` with person `"Someone"`.

Third test `recent_never_contains_text`: after both, `serde_json::to_string(&recent.list())` does not contain `"hello"`.

UI test in `PotatoDiscordPage.test.tsx`: `shows recent rows` -> fake `recent()` returns one `answered` row -> text `Answered Ren in #general`.

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-D11 cargo test --lib potato::discord
cd packages/app && npx vitest run src/components/settings/potato/PotatoDiscordPage.test.tsx
```
Expected: Rust `test result: ok.` 0 failed (3 new); vitest `Tests  10 passed (10)`.

### Done when
- [ ] The full path is proven with fakes: gateway message -> admit -> sandbox-only brain item -> reply -> outbox.
- [ ] Recent never stores message text and never names people who are not allowed.

### Do NOT
- Do not write recent rows to disk (memory only; cleared on restart).
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Brain item id unknown in the test: read it from the fake inbox item JSON (`id` field, NEEDS-FACT name) before building the `Reply`.
2. Outbox has `Typing` after `Send`: `handle_in` must push `Typing` before `send_outside`.
3. `Recent` row person shows a real name for a stranger: map `Drop::PersonNotAllowed`, `DmNotAllowed` to `"Someone"`.

---

## Security

| Threat | What an attacker or broken peer can do | Defence | Test that proves it |
|---|---|---|---|
| A stranger on Discord asks the bot to read files on the PC or start a coding agent on the PC | Steal files, run code on the owner's PC | Every Discord turn is `sandboxOnly`; the brain gets only the 6 Sandbox tools; helper card never shown | `outside_item_is_sandbox_only` (D4), `sandboxOnly keeps exactly the six sandbox tools` (D4), `sandbox_only_tools_are_exactly_the_six` (D2) |
| Sandbox is off, an outside message arrives | Turn falls back to PC tools | `policy::check` runs before `rules::decide`; refusal, nothing saved | `outside_refused_when_sandbox_off` (D4) |
| Stranger uses Fish's browser to reach the home router or the Windows host | LAN attack through the sandbox | Plan 15 slice R6 (egress limit) is a hard dependency of D9 | R6 tests (plan 15) |
| Another bot or a webhook drives the bot in a loop | Cost blow-up, spam | `Drop::FromBot` is check 1; webhooks count as bots | `drops_bots_first` (D3), `webhook counts as bot` (D7) |
| Flood from one person or one channel | Cost blow-up | Rate book: 6/5 min per person, 20/10 min per place, owner exempt; daily cost limit still applies; rate keys capped at 10 000 | `stranger_rate_limited_after_6`, `place_rate_limited_after_20`, `caps_keys` (D3) |
| Bot pings @everyone or roles | Abuse of the owner's server | `allowed_mentions: {parse: [], replied_user: false}` on every post | `never parses mentions` (D8) |
| Token leaks through logs, errors, the UI or the brain | Full control of the Discord bot | Token only in the OS store; only the link process gets it, as env; never in errors; never returned to UI | `error_text_never_has_token` (D5), D9 `Do NOT` + env test (`potato/env_tests.rs` must assert the brain env has no `DISCORD_BOT_TOKEN`) |
| A local process posts fake Discord messages to `/potato/discord/in` | Make the bot act as if the owner asked | Per-spawn random link token, constant-time compare; route only on the guarded loopback path | `in_needs_link_token` (D9) |
| Huge or malformed payloads | Crash or memory growth | 256 KB body cap, 50 events per POST, content capped at 4000 chars, outbox cap 200, reply-target map cap 500 | `in_rejects_big_body`, `outbox_caps_and_drops_oldest` (D9), `content capped` (D7), `reply_targets_drop_oldest_past_cap` (D4) |
| Path injection through ids into Discord URLs | Calls to unintended Discord endpoints | Every id checked with `is_id` / `^\d{1,32}$` before building a path | `channels_reject_bad_guild_id` (D5), `rejects bad channel id` (D8) |
| Stranger extracts the owner's private memory notes | Privacy leak | `sandboxOnly` turns get no owner memory notes in the prompt (D4 step 7) and no `memory_search` tool | `sandboxOnly keeps exactly the six sandbox tools` (D4); NEEDS-FACT for the notes test |
| A stranger spams `ask_person_for_screen` | Needs-you cards flood the owner | The tool is rate limited by the person rate book; cards already expire after 30 minutes | `stranger_rate_limited_after_6` (D3) |
| Hold / panic does not stop Discord | Bot keeps answering after the owner paused all bots | `held` is check 3 in `admit` | `drops_when_held` (D3) |

## What cannot be verified without real hardware or a second computer

- A real Discord gateway session: identify, heartbeat timing, resume after a real network drop, and the real close codes (4004, 4014). Tests use a fake socket.
- The real invite flow in a browser (OAuth consent page wording may change; the plan's step text matches Discord's portal as known on 2026-11-04 and must be re-checked by a person once).
- Message Content Intent behaviour for a bot in more than 100 servers (Discord requires verification there; out of scope for a personal bot).
- Two people talking to the bot at once from two Discord accounts (needs a second account and device).
- That the link process starts hidden on a clean Windows install from the installer bundle (needs the built installer; see plan 13 slice S6).
- Egress limits of the Sandbox against a real home network (needs the real WSL 2 distro; plan 15 slice R6).

## Open facts

- NEEDS-FACT: the exact parameter type of `labs_on(public)` and the helper it uses to read "exactly true" - read `packages/app/src-tauri/src/potato/live.rs` l.55-70.
- NEEDS-FACT: how `*_tests.rs` files are attached to their modules (`mod x_tests;` or `#[path]`) - read `packages/app/src-tauri/src/potato/mod.rs`.
- NEEDS-FACT: the fields of `rules::Facts` (surface, sender, trust) and the `enum Origin` definition - read `packages/app/src-tauri/src/potato/rules.rs` l.1-140 and `packages/app/src-tauri/src/potato/service.rs` l.680-830.
- NEEDS-FACT: the JSON shape of an inbox item built by `push_to_brain` (field names for kind, id, conversation, trust) - read `packages/app/src-tauri/src/potato/service.rs` l.1080-1150 and `packages/potato-brain/src/channels/potato.ts` l.400-470.
- NEEDS-FACT: how `Service` knows the Sandbox is installed and Labs is on (the `Host` method names) - read `packages/app/src-tauri/src/potato/service.rs` l.62-80 and `packages/app/src-tauri/src/potato/sandbox_host.rs`.
- NEEDS-FACT: where `runTurn` adds the owner's memory notes and whether vault memory is per bot or per conversation - read `packages/potato-brain/src/channels/potato.ts` l.600-700 and `packages/potato-brain/src/agents.ts`.
- NEEDS-FACT: the HTTP client crate and call style in `ask_model` - read `packages/app/src-tauri/src/potato/live.rs` l.255-290.
- NEEDS-FACT: the file that declares Tauri command modules (`commands/mod.rs` or similar) - list `packages/app/src-tauri/src/commands/`.
- NEEDS-FACT: whether `secret_set` allows `discord_bot_token` (`is_ui_secret`) and the `SecretMask` field names - read `packages/app/src-tauri/src/secrets.rs` l.30-45 and l.158-180.
- NEEDS-FACT: the header that carries `PM_POTATO_TOKEN` on brain requests - read `packages/potato-brain/src/channels/potato.ts` l.271-420 and `packages/app/src-tauri/src/potato/auth.rs`.
- NEEDS-FACT: what `bridge/http.rs` l.123 and l.370 check, to add `/potato/discord/` the same way - read `packages/app/src-tauri/src/bridge/http.rs` l.100-140 and l.350-390.
- NEEDS-FACT: how `script_path` names the brain script in dev and release (`src/pm.ts` vs `brain.mjs`) - read `packages/app/src-tauri/src/potato/sidecar.rs` l.110-140 and `packages/app/src-tauri/src/potato/resource_dirs.rs`.
- NEEDS-FACT: the signature of `keeper::backoff_ms` - read `packages/app/src-tauri/src/potato/keeper.rs`.
- NEEDS-FACT: the app's helper for opening an external URL in the system browser - search `packages/app/src` for `openUrl` or `plugin-opener`.
- NEEDS-FACT: how Potato sub-pages are registered - read `packages/app/src/components/settings/potato/PotatoSubPages.tsx`.
