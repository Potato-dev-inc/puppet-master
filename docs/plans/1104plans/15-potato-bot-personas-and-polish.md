# 15 - Personas and polish

Status: plan only. Written 2026-11-04 from `00-FORMAT.md` and `_facts/potato-bot-facts.md` only (no source code was
read, by founder request). `Read first` lists are for the builder. Unknown names are `NEEDS-FACT`, listed at the end.

## What this plan covers

1. The Fish licence check, as the first line of the release checklist (F1).
2. A Fish | Potato persona switch per bot (F2), and a note on the mascot pack.
3. The Rakazo parity items the facts file marks `missing` (and two marked `partial` that block them), as slices
   R1-R10, plus a table of the missing items this round does not build, each with its reason.

## Facts this plan rests on

- Persona code: `packages/potato-brain/src/persona/` (`persona.ts` `loadPersona`, `moodStore`, `selfStore`,
  `saveMood`, `loadSelf`; `packs.ts` `listPacks`; `mood.ts`, `express.ts`, `kaomoji.ts`, `self.ts`, `notes.ts`,
  `people.ts`, `share.ts`, `dsh-import.ts`) (facts 1b).
- Tauri command `potato_set_persona` exists (`commands/potato.rs` l.57); UI `PersonaSwitch.tsx` and `MoodChip.tsx`
  exist (facts 1a, 1c). Their current values are not in the facts file (NEEDS-FACT).
- The Labs label map calls the Sandbox "Fish's computer" (facts 4a).
- Rakazo parity rows marked `missing` (facts 8): group chats, bot-to-bot, handoff (all in plan 12), semantic memory
  providers, settings from chat, lazy tool catalog, change-watch routines, webhook/Git/Slack triggers, stuck-work
  reminders, voice calls, team vs private computer, egress restriction, shell guard / desktop command guard, teach a
  task, SSRF-safe fetch (for Potato), site trust, web search and fetch, MCP client, tickets board, media and plots,
  messaging commands, settings-from-chat undo cards. Marked `partial` and touched here: DM reactions (none found),
  bot duplicate/archive (none found).
- Open work list: `potato/open_work.rs`, `OpenWork.tsx`; notifications: `potato/notify.rs`; routines:
  `potato/routines.rs` (`routines.json`, 30 s tick, `add_pending`, `PendingRun`) (facts 1a, 8).
- Sandbox calls: brain emits `sandbox_call` -> `sandbox_calls.rs` dispatcher -> `sandbox_result` (facts 2).

## Founder rules that shape this plan

| Rule | Where it lands |
|---|---|
| Default persona Fish; its licence check heads the release checklist | F1 makes `docs/release/CHECKLIST.md` start with it, with a test that keeps it first. F2 makes `fish` the default persona for new bots. |
| Outside triggers run in the Sandbox only | R5 commands never run tools; R6 limits the Sandbox's network; R7 guards Sandbox actions in outside turns; R2 and R8 are app-only and never offered to `sandboxOnly` turns. |
| UI monochrome | R1 reactions are monochrome line glyphs, not colour emoji. |
| New Labs flags join `DEV_LABS_KEYS` | No new flag in this plan; everything sits under `potato_bot_enabled` / `potato_sandbox_enabled`. |

## Slice index (dependency order)

| Slice | Goal | Size | Needs merged first |
|---|---|---|---|
| F1 | Release checklist headed by the Fish licence check | Haiku | none |
| F2 | Persona per bot: Fish or Potato, default Fish | Haiku | none |
| R6 | Sandbox egress limit (no home network, no host) | Sonnet | none (blocks plan 11 D9 and plan 14 T3 going live) |
| R7 | Action guard for outside turns | Haiku | plan 11 D4 |
| R5 | Messaging commands on Discord and Telegram | Haiku | plan 11 D9, plan 14 T3 |
| R3 | Stuck-work reminders | Haiku | none |
| R10 | Duplicate and archive a bot | Haiku | none |
| R1 | Reactions on bot messages | Sonnet | none |
| R2 | Settings from chat with an Undo card | Sonnet | F2 |
| R8 | SSRF-safe `web_fetch` tool (app turns only) | Sonnet | plan 11 D4 |
| R4 | Change-watch routines | Sonnet | R8 |
| R9 | Lazy tool catalog | Sonnet | R8 |

Rules for every slice: never run a dev server or the app on the primary screen; separate `CARGO_TARGET_DIR` per slice
(`D:/pm-night/target-<slice>`); never kill by image name; new Labs flags join `DEV_LABS_KEYS`; `p2p/guard_tests.rs`
and `desk/guard_tests.rs` stay green; team traffic never uses the bridge or pairing; UI monochrome; commit finished
work with a fresh build id. Commands are Git Bash from the repo root.

---

## Slice F1 - Release checklist headed by the Fish licence check

### Goal
Create (or edit) the release checklist so its first item is the Fish persona licence check, and add a test that keeps it first.

### Read first
- The existing release checklist, if any (NEEDS-FACT: search `docs/` for `release` and `checklist`).
- `packages/potato-brain/src/persona/packs.ts` (`listPacks`) and the Fish pack files it lists (where Fish's text and any art come from).
- `packages/potato-brain/src/lifted/` and its `PORTED.md` (how ported code records its source).

### Files
- create (or edit) `docs/release/CHECKLIST.md`
- create `docs/licences/fish-persona.md`
- create `packages/app/src/lib/release-checklist.test.ts`

### Steps
1. If a release checklist already exists elsewhere, move its content into `docs/release/CHECKLIST.md` and leave a one-line pointer in the old file.
2. Make the first item exactly the Fish line (Code).
3. Create `docs/licences/fish-persona.md` with the record template (Code). The builder fills the "Source" rows from the pack files; the licence answer is a person's job: leave `Status: NOT CHECKED` until the founder signs it.
4. Add the test.

### Code
`docs/release/CHECKLIST.md` (top of file):
```markdown
# Release checklist

1. [ ] Fish licence check: docs/licences/fish-persona.md says "Status: CLEARED" with a date and a name.
       If it says anything else, do not ship a build where Fish is the default persona.
2. [ ] Potato Bot files are in the installer: node scripts/verify-potato-bundle.mjs "<resources folder>"
       -> "OK: all Potato Bot files are in the bundle."  (plan 13 S6)
3. [ ] Labs flags potato_bot_enabled and potato_sandbox_enabled are off in a fresh install (open Settings > Labs on a
       clean user profile and look).
```
(Keep any existing items after these three, renumbered.)

`docs/licences/fish-persona.md`:
```markdown
# Fish persona: licence record

Status: NOT CHECKED
Checked by:
Date:

## What Fish is made of
| Part | File(s) in this repo | Where it came from | Licence | Notes |
|---|---|---|---|---|
| Persona text (instructions, voice) | packages/potato-brain/src/persona/... | | | |
| Kaomoji set | packages/potato-brain/src/persona/kaomoji.ts | | | |
| Name "Fish" | (product copy) | | | trademark search: |
| Any art or icon of Fish | | | | |

## Result
- CLEARED: every row has a licence that allows shipping in a paid product, or the part is our own work.
- BLOCKED: list the row and what must change.
```

`packages/app/src/lib/release-checklist.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..');

describe('release checklist', () => {
  it('starts with the Fish licence check', () => {
    const text = readFileSync(join(root, 'docs', 'release', 'CHECKLIST.md'), 'utf8');
    const first = text.split('\n').find((l) => /^\d+\.\s*\[ \]/.test(l.trim()));
    expect(first?.trim().startsWith('1. [ ] Fish licence check')).toBe(true);
  });

  it('has a Fish licence record with a Status line', () => {
    const rec = readFileSync(join(root, 'docs', 'licences', 'fish-persona.md'), 'utf8');
    expect(rec).toMatch(/^Status: (NOT CHECKED|CLEARED|BLOCKED)/m);
  });
});
```

### Tests to write
The two tests above.

### Run
```
cd packages/app && npx vitest run src/lib/release-checklist.test.ts
```
Expected: `Tests  2 passed (2)`.

### Done when
- [ ] `docs/release/CHECKLIST.md` item 1 is the Fish licence check.
- [ ] `docs/licences/fish-persona.md` exists with `Status: NOT CHECKED` and the Source rows filled from the pack files.

### Do NOT
- Do not write `Status: CLEARED` yourself. Only the founder (or someone they name) can.
- Do not guess a licence. If a source is unknown, write `unknown` in the row.

### If it fails
1. Path depth wrong in the test: `packages/app/src/lib` is 4 levels below the repo root; adjust the `..` count if the file moved.
2. An older checklist uses `- [ ]` bullets: the test looks for numbered items; convert to numbers.
3. A docs linter rejects the new folder: add `docs/licences/` and `docs/release/` to its allow list (NEEDS-FACT if one exists).

---

## Slice F2 - Persona per bot: Fish or Potato, default Fish

### Goal
Each bot has persona `fish` or `potato`; new bots default to `fish`; the choice is a switch in the bot panel and reaches the brain.

### Read first
- `packages/app/src-tauri/src/commands/potato.rs` l.52-62 (`potato_bot_add`, `potato_set_persona`) and the bot struct in `potato/service.rs` l.80 (`BotProfile`).
- `packages/app/src/components/cockpit/potato/PersonaSwitch.tsx` (and its test, if any).
- `packages/potato-brain/src/persona/persona.ts` (`loadPersona`) and `packs.ts` (`listPacks`).
- `packages/app/src/lib/potato-starters.ts` (new-bot starters).

### Files
- edit `packages/app/src-tauri/src/potato/service.rs` (persona field type and default, only if not already exactly `fish | potato`)
- edit `packages/app/src/components/cockpit/potato/PersonaSwitch.tsx`
- create or edit `packages/app/src/components/cockpit/potato/PersonaSwitch.test.tsx`
- create `packages/potato-brain/test/persona-packs.test.ts`
- edit `packages/app/src/lib/potato-starters.ts` (default persona `fish`)

### Steps
1. Read what `potato_set_persona` accepts today. If it already accepts exactly `fish` and `potato`, skip the Rust change and only add tests.
2. Otherwise: `enum Persona { #[default] Fish, Potato }` with `serde(rename_all = "snake_case")`; unknown stored values load as `Fish`.
3. `PersonaSwitch`: a 2-way segmented control `Fish | Potato`, with one line under it (Code).
4. Brain test: `listPacks()` contains both `fish` and `potato`, and `loadPersona("fish")` / `loadPersona("potato")` load without throwing.
5. Starters: every starter that sets no persona gets `fish`.

### Code
```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Persona {
    #[default]
    Fish,
    Potato,
    #[serde(other)]
    Unknown, // maps to Fish when read; never written
}
impl Persona { pub fn effective(self) -> Persona { if self == Persona::Unknown { Persona::Fish } else { self } } }
```
UI text:
```ts
export const PERSONA_COPY = {
  label: 'Personality',
  fish: 'Fish',
  potato: 'Potato',
  fishHelp: 'Calm and curious. Short answers, a little playful.',
  potatoHelp: 'Cheerful and silly. Loves a joke.',
  note: 'The name stays the same. Only the way the bot talks changes.',
} as const;
```
(The two help lines describe the packs; the builder replaces them with one sentence taken from each pack's own
description if the pack has one: NEEDS-FACT.)

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `persona_default_is_fish` (Rust) | new bot without persona | `Persona::Fish` |
| `persona_unknown_loads_as_fish` (Rust) | stored `"pirate"` | `effective() == Fish` |
| `switch shows both` (TS) | render | `Fish`, `Potato`, note text |
| `switch calls set persona` (TS) | click `Potato` | `potato_set_persona` invoked with `{ botId, persona: 'potato' }` (through the fake transport) |
| `packs include fish and potato` (brain) | `listPacks()` | contains both ids |
| `both personas load` (brain) | `loadPersona` each | no throw |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-F2 cargo test --lib potato
cd packages/app && npx vitest run src/components/cockpit/potato/PersonaSwitch.test.tsx
cd packages/potato-brain && node --test test/persona-packs.test.ts
```
Expected: 0 failed in each.

### Done when
- [ ] New bots are Fish; each bot can switch to Potato and back.
- [ ] Unknown stored values do not crash; they act as Fish.

### Do NOT
- Do not rename bots when the persona changes.
- Do not add colour to the switch.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `PersonaSwitch` already has more personas: keep them visible only if `listPacks` returns them; the default stays `fish`.
2. `loadPersona` needs a state dir: pass a temp dir as the existing persona tests do.
3. `#[serde(other)]` needs a unit variant: `Unknown` is one; place it last.

### Mascot pack note (no slice in this round)
- A mascot pack = the art for a persona (avatar, a few moods) used by `MoodChip` and companion avatars.
- It must be monochrome (grey line art on the theme tokens), no orange fill, no colour glass.
- It goes through the same licence record as Fish (`docs/licences/<pack>-mascot.md`, `Status: NOT CHECKED` until signed).
- Where packs live is `packages/potato-brain/src/persona/packs.ts`; the art location in the app is NEEDS-FACT.
- `DECISION-OPEN (default: no mascot art ships until a licensed or in-house monochrome set exists; MoodChip keeps text moods)`.

---

## Slice R6 - Sandbox egress limit

### Goal
Fish's computer can reach the internet but not this PC, the home network, or link-local addresses; DNS still works.

### Read first
- `packages/app/src-tauri/src/potato/sandbox/lifecycle.rs` (`Lifecycle::ensure_ready`), `wsl.rs` (how root commands run in the distro), `install.rs` (how guest files are copied).
- `packages/potato-sandbox/guest/` (guest scripts layout; facts list `guest/desktop/apply-recipe.py`, `guest/desktop/pm-sbx-start-desktop.sh`).
- `packages/app/src-tauri/src/potato/sandbox/screen.rs` (the direction of the screen connection: host -> guest or guest -> host; NEEDS-FACT).

### Files
- create `packages/app/src-tauri/src/potato/sandbox/egress.rs`
- create `packages/app/src-tauri/src/potato/sandbox/egress_tests.rs`
- create `packages/potato-sandbox/guest/net/pm-sbx-egress.sh`
- edit `packages/app/src-tauri/src/potato/sandbox/lifecycle.rs` (apply on every start, before the desktop starts)
- edit `packages/app/src-tauri/src/potato/resource_check.rs` (add `guest/net/pm-sbx-egress.sh` to `REQUIRED_SANDBOX`; plan 13 S5)

### Steps
1. `egress.rs`: pure `ruleset(resolvers: &[IpAddr]) -> String` that returns an nftables script (Code).
2. Guest script reads the ruleset from stdin and applies it with `nft -f -`; `--check` prints `EGRESS_OK` if the table `pm_egress` exists.
3. `lifecycle.rs`: on start, read `/etc/resolv.conf` nameservers from the guest, build the ruleset, pipe it to the script as root, then run `--check`; if it does not print `EGRESS_OK`, stop the start with `LifecycleError::Egress` and the user text below.
4. Tests for the pure ruleset.

### Code
```rust
use std::net::IpAddr;

pub const BLOCK_V4: &[&str] = &["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "100.64.0.0/10", "127.0.0.0/8", "224.0.0.0/4", "0.0.0.0/8"];
pub const BLOCK_V6: &[&str] = &["fc00::/7", "fe80::/10", "::1/128", "ff00::/8"];

/// nftables script. Order: allow loopback; allow replies to connections made TO the guest (the host's screen and
/// control links); allow DNS to the given resolvers only; drop the private ranges; allow everything else out.
pub fn ruleset(resolvers: &[IpAddr]) -> String {
    let mut s = String::from("table inet pm_egress\ndelete table inet pm_egress\ntable inet pm_egress {\n  chain out {\n    type filter hook output priority 0; policy accept;\n    oif \"lo\" accept\n    ct state established,related accept\n");
    for r in resolvers {
        let fam = if r.is_ipv4() { "ip" } else { "ip6" };
        s.push_str(&format!("    {fam} daddr {r} udp dport 53 accept\n    {fam} daddr {r} tcp dport 53 accept\n"));
    }
    s.push_str(&format!("    ip daddr {{ {} }} drop\n", BLOCK_V4.join(", ")));
    s.push_str(&format!("    ip6 daddr {{ {} }} drop\n", BLOCK_V6.join(", ")));
    s.push_str("  }\n}\n");
    s
}

/// Nameserver lines from resolv.conf text. Ignores anything that is not an IP. Max 3.
pub fn parse_resolvers(resolv_conf: &str) -> Vec<IpAddr>;
```
Note: `ct state established,related accept` lets the guest answer connections the host opens to it (screen proxy,
control). If NEEDS-FACT shows the guest opens connections TO the host for the screen, add one allow line for exactly
that host IP and port, built from the same value the screen code uses, and a test for it.

User text when the limit cannot be applied:
```
Fish's computer did not start: its network safety rules could not be turned on. Try Restart in Settings > Potato Bot > Fish's computer.
```

`packages/potato-sandbox/guest/net/pm-sbx-egress.sh`:
```sh
#!/bin/sh
# Applies the Puppet Master egress rules read from stdin. Run as root. No arguments = apply; --check = verify.
set -eu
if [ "${1:-}" = "--check" ]; then
  nft list table inet pm_egress >/dev/null 2>&1 && echo EGRESS_OK && exit 0
  echo EGRESS_MISSING; exit 1
fi
command -v nft >/dev/null 2>&1 || { echo NFT_MISSING; exit 2; }
nft -f -
```
If the distro has no `nft` (`NFT_MISSING`), the install recipe must add the `nftables` package (NEEDS-FACT: the
recipe file `apply-recipe.py` package list).

### Tests to write (`egress_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `blocks_private_v4` | `ruleset(&[])` | contains `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `169.254.0.0/16` in one `drop` line |
| `blocks_private_v6` | same | contains `fc00::/7` and `fe80::/10` |
| `dns_allowed_before_drop` | resolver `172.29.0.1` | the `172.29.0.1 udp dport 53 accept` line comes before the `drop` line |
| `replies_allowed_first` | any | `ct state established,related accept` before the drops |
| `parse_resolvers_reads_ips` | `"nameserver 10.255.255.254\nnameserver bad\noptions x"` | `[10.255.255.254]` |
| `parse_resolvers_caps_three` | 5 nameservers | 3 |
| `script_is_idempotent_text` | `ruleset` output | starts with `table inet pm_egress\ndelete table inet pm_egress` (so re-applying replaces it) |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R6 cargo test --lib potato::sandbox::egress
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R6 cargo test --lib potato::sandbox
```
Expected: `test result: ok. 7 passed`; sandbox suite 0 failed.

### Done when
- [ ] Every Sandbox start applies the rules and checks them; a failure stops the start with the text above.
- [ ] Tests pass. Real-distro check is listed under hardware.

### Do NOT
- Do not change Windows firewall settings on the PC (system setting; not allowed).
- Do not block DNS to the resolver, or the guest loses the internet.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. WSL mirrored networking puts the host on `127.0.0.1`: `127.0.0.0/8` is blocked except `lo` traffic; if the screen proxy needs guest -> host on loopback, add the exact port allow (see note) with a test.
2. `nft -f -` errors on `delete table` when it does not exist: the first line `table inet pm_egress` creates it empty first, so delete always works. Keep both lines.
3. Root command path in `wsl.rs` cannot pipe stdin: write the ruleset to a guest temp file with the existing file copy, then run `nft -f <file>`, then delete the file.

---

## Slice R7 - Action guard for outside turns

### Goal
In `sandboxOnly` turns, refuse Sandbox actions that open a terminal or type risky shell commands, as a second line of defence after the Sandbox itself.

### Read first
- `packages/app/src-tauri/src/potato/sandbox_calls.rs` (dispatcher l.50-80; the action shapes for `computer_act`).
- `packages/app/src-tauri/src/potato/outside/engine.rs` (how the turn's scope reaches the dispatcher; NEEDS-FACT: whether `SandboxCall` knows the conversation/scope).

### Files
- create `packages/app/src-tauri/src/potato/outside/guard.rs`
- create `packages/app/src-tauri/src/potato/outside/guard_tests.rs`
- edit `packages/app/src-tauri/src/potato/outside/mod.rs`
- edit `packages/app/src-tauri/src/potato/sandbox_calls.rs` (call the guard when the call's turn is `sandboxOnly`)

### Steps
1. `guard.rs` (Code): pure checks on key combos and typed text.
2. Dispatcher: if the call belongs to a `sandboxOnly` turn and the tool is `computer_act`, run `check_actions` first; a hit returns `sandbox_result_err` with the hit's message; nothing runs.
3. Tests.

### Code
```rust
#[derive(Debug, PartialEq, Eq)]
pub enum Hit { TerminalKey(String), RiskyText(&'static str) }

pub const TERMINAL_KEYS: &[&str] = &["ctrl+alt+t", "super+t", "ctrl+shift+t"];
pub const RISKY: &[(&str, &str)] = &[
    ("sudo ", "sudo"),
    ("| sh", "pipe to sh"), ("|sh", "pipe to sh"), ("| bash", "pipe to bash"), ("|bash", "pipe to bash"),
    ("rm -rf /", "rm -rf /"), ("mkfs", "mkfs"), (":(){", "fork bomb"),
    ("chmod 777 /", "chmod 777 /"), ("/dev/tcp/", "raw socket"), ("nc -e", "netcat shell"),
];

/// `keys`: normalised combos like "ctrl+alt+t" (lower case, '+' joined, sorted modifiers first).
/// `typed`: text the action would type.
pub fn check_actions(keys: &[String], typed: &[String]) -> Result<(), Hit>;

pub fn hit_text(h: &Hit) -> String {
    format!("I can't do that from a chat app: it could change my computer in a risky way. ({})",
        match h { Hit::TerminalKey(k) => k.as_str(), Hit::RiskyText(w) => w })
}
```
Matching is case-insensitive on `typed` after collapsing runs of spaces to one space.

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `terminal_key_refused` | keys `["ctrl+alt+t"]` | `Err(TerminalKey("ctrl+alt+t"))` |
| `curl_pipe_sh_refused` | typed `["curl x.sh |  sh"]` | `Err(RiskyText("pipe to sh"))` |
| `sudo_refused_any_case` | `["SUDO apt install x"]` | `Err(RiskyText("sudo"))` |
| `normal_typing_ok` | `["hello world", "https://example.com"]` | `Ok(())` |
| `app_turn_not_guarded` (dispatcher test) | same risky text, scope `AsToday` | action is passed to the fake sandbox |
| `outside_turn_guarded` (dispatcher test) | scope `SandboxOnly` | `sandbox_result_err` with `hit_text`; fake sandbox saw no action |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R7 cargo test --lib potato::outside::guard
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R7 cargo test --lib potato::sandbox_calls
```
Expected: `test result: ok. 4 passed`; dispatcher 0 failed (2 new).

### Done when
- [ ] Outside turns cannot open a terminal by shortcut or type the listed patterns.

### Do NOT
- Do not present this guard as the security boundary in docs; the Sandbox and R6 are the boundary.
- Do not guard app-DM turns (the owner may use a terminal in their own Sandbox).

### If it fails
1. Dispatcher does not know the scope: add `sandbox_only: bool` to the pending call record when the brain's `SandboxCall` arrives for an item that had `sandboxOnly`.
2. Key names differ (`Control` vs `ctrl`): normalise in one helper `norm_combo` and test it.
3. False positive on normal text containing "sudo" as a word in prose: accepted (outside turns rarely need it); keep the pattern.

---

## Slice R5 - Messaging commands on Discord and Telegram

### Goal
Answer `/help`, `/ping` for everyone allowed, and `/mood`, `/pause`, `/resume` for the owner, without any model turn.

### Read first
- `packages/app/src-tauri/src/potato/discord/bridge.rs` (`handle_in`) and `packages/app/src-tauri/src/potato/telegram/link.rs` (plan 11 D9, plan 14 T3).
- `packages/potato-brain/src/persona/mood.ts` and the Rust side of mood (`Event::Mood`, `MoodChip.tsx`) to know where the current mood is stored (NEEDS-FACT).

### Files
- create `packages/app/src-tauri/src/potato/outside/commands.rs`
- create `packages/app/src-tauri/src/potato/outside/commands_tests.rs`
- edit `packages/app/src-tauri/src/potato/outside/mod.rs`
- edit `packages/app/src-tauri/src/potato/discord/bridge.rs` (call `commands::parse` after `admit` says Yes)
- edit `packages/app/src-tauri/src/potato/telegram/link.rs` (same)

### Steps
1. `parse(text, bot_username) -> Option<Cmd>`: first word only; accepts `/help` and `/help@<bot_username>` (Telegram style).
2. `run(cmd, owner, ctx) -> Reply` pure, returns text and an optional effect (`PausePlace(1h)`, `ResumePlace`).
3. Discord and Telegram call it after `admit` (so commands are rate limited and need an allowed person); a command never calls `send_outside`.
4. Paused places: `admit` gets `Drop::Held` for that place until the time passes (add a `paused_places: HashMap<String, u64>` to the caller and pass `held = true` for that place).

### Code
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd { Help, Ping, Mood, Pause, Resume }

pub struct Ctx<'a> { pub bot_name: &'a str, pub call_name: &'a str, pub mood: Option<&'a str> }

#[derive(Debug, PartialEq, Eq)]
pub enum Effect { None, PausePlace { minutes: u32 }, ResumePlace }

pub fn parse(text: &str, bot_username: &str) -> Option<Cmd>;
pub fn run(cmd: Cmd, owner: bool, ctx: &Ctx<'_>) -> (String, Effect);
```
Texts (exact):
```
/help (anyone):  "I'm {bot}. Call me with @{call} or reply to me. Commands: /help, /ping."
/help (owner):   "I'm {bot}. Call me with @{call} or reply to me. Commands: /help, /ping, /mood, /pause, /resume."
/ping:           "Here."
/mood (owner):   "Mood: {mood}."   or "Mood: not set."
/pause (owner):  "Paused here for 1 hour. Send /resume to undo."      Effect::PausePlace { minutes: 60 }
/resume (owner): "Back."                                               Effect::ResumePlace
/mood, /pause, /resume (not owner): "Only my owner can use that."
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `parse_plain` | `"/help"` | `Some(Help)` |
| `parse_telegram_suffix` | `"/ping@ren_fish_bot"`, username `ren_fish_bot` | `Some(Ping)` |
| `parse_other_bot_suffix_ignored` | `"/ping@other_bot"` | `None` |
| `parse_not_first_word` | `"hey /help"` | `None` |
| `help_owner_lists_more` | `Help`, owner | text contains `/pause` |
| `mood_not_owner_refused` | `Mood`, not owner | `"Only my owner can use that."` |
| `pause_effect` | `Pause`, owner | effect `PausePlace { minutes: 60 }` |
| `command_does_not_start_turn` (discord bridge test) | owner `/ping` in a picked channel | outbox `Send` with `"Here."`; fake service got no `send_outside` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R5 cargo test --lib potato::outside::commands
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R5 cargo test --lib potato::discord
```
Expected: `test result: ok. 7 passed`; discord 0 failed.

### Done when
- [ ] Commands answer instantly without a model call, on both chat apps.

### Do NOT
- Do not add commands that change settings, show memory (`/recall`, `/diary`), or show stats about other people. fishd's `/recall`, `/diary`, `/config`, `/tune`, `/ignore`, `/stats` are not built: `DECISION-OPEN (default: not built; they expose memory or settings to a chat app)`.

### If it fails
1. Mood is only in the brain: `/mood` answers `"Mood: not set."` until the Rust side stores the last `Event::Mood` per bot (add that in this slice if it is one field; else leave it).
2. Discord sends `/help` as plain text (no slash commands registered): correct; this plan uses text commands only.
3. Paused place not respected: pass `held || paused_until(place) > now` into `admit`.

---

## Slice R3 - Stuck-work reminders

### Goal
Remind the person once a day about open work items a bot has not touched for 24 hours.

### Read first
- `packages/app/src-tauri/src/potato/open_work.rs` (item fields: id, title, updated time; NEEDS-FACT).
- `packages/app/src-tauri/src/potato/notify.rs` (how a notification is sent, per-bot notify settings).
- `packages/app/src-tauri/src/potato/routines.rs` (the 30 s tick to hook into).

### Files
- create `packages/app/src-tauri/src/potato/stuck.rs`
- create `packages/app/src-tauri/src/potato/stuck_tests.rs`
- edit `packages/app/src-tauri/src/potato/mod.rs`
- edit `packages/app/src-tauri/src/potato/routines.rs` (call `stuck::due` from the tick, every 10 minutes)
- edit `packages/app/src/lib/potato-notify.ts` (one switch "Remind me about stuck work", default on)

### Steps
1. `due(items, last_sent, now) -> Vec<String>` pure (Code).
2. From the tick, every 10 minutes: for each bot with the switch on, send one notification per due item; store `last_sent` in `<app data>/potato/stuck.json`.
3. Notification text (Code).
4. Tests.

### Code
```rust
pub const STUCK_AFTER_MS: u64 = 24 * 3_600_000;
pub const REMIND_EVERY_MS: u64 = 24 * 3_600_000;
pub const MAX_PER_RUN: usize = 3;

pub struct ItemView<'a> { pub id: &'a str, pub title: &'a str, pub updated_ms: u64, pub done: bool }

/// Items not done, untouched for 24 h, and not reminded in the last 24 h. Oldest first. Max 3.
pub fn due(items: &[ItemView<'_>], last_sent: &std::collections::HashMap<String, u64>, now_ms: u64) -> Vec<String>;

pub fn text(bot: &str, title: &str) -> String { format!("{bot} has not moved on \"{title}\" for a day. Open it to check.") }
```
UI switch text: `Remind me about stuck work` with help `Once a day, for work a bot has not touched in 24 hours.`

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `fresh_item_not_due` | updated 1 h ago | `[]` |
| `old_item_due` | updated 25 h ago, never sent | `["a"]` |
| `done_item_never_due` | old, done | `[]` |
| `reminded_recently_not_due` | old, sent 2 h ago | `[]` |
| `caps_three_oldest_first` | 5 old items | 3 ids, oldest first |
| `text_quotes_title` | `text("Fish", "Fix login")` | `Fish has not moved on "Fix login" for a day. Open it to check.` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R3 cargo test --lib potato::stuck
```
Expected: `test result: ok. 6 passed`.

### Done when
- [ ] At most 3 reminders per run, one per item per day, only when the switch is on.

### Do NOT
- Do not wake a brain or run a model for reminders.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Open work items have no `updated_ms`: use the latest of created time and last status change; NEEDS-FACT names.
2. Tick runs every 30 s: keep a `last_stuck_check_ms` and skip unless 10 minutes passed.
3. Notifications spam after restart: `last_sent` must be loaded from `stuck.json` on start.

---

## Slice R10 - Duplicate and archive a bot

### Goal
Duplicate a bot (settings and instructions, no chats or memory) and archive a bot (hidden and stopped, data kept, can be restored).

### Read first
- `packages/app/src-tauri/src/potato/bot_settings.rs` (`export_markdown`, `delete_bot`) and `service.rs` l.514-600.
- `packages/app/src/components/cockpit/potato/BotContextMenu.tsx`, `BotList.tsx`.
- `packages/app/src-tauri/src/potato/trash.rs` (archive must not reuse trash: trash deletes later).

### Files
- edit `packages/app/src-tauri/src/potato/bot_settings.rs` (`duplicate_bot`, `set_archived`)
- edit `packages/app/src-tauri/src/commands/potato_bot_settings.rs` (+ register 2 commands in `lib.rs`)
- edit `packages/app/src/components/cockpit/potato/BotContextMenu.tsx`
- edit `packages/app/src/components/cockpit/potato/BotList.tsx` (an `Archived` section, collapsed by default)
- create `packages/app/src/components/cockpit/potato/BotArchive.test.tsx`

### Steps
1. `Service::duplicate_bot(bot) -> Result<String /*new id*/, String>`: new id, name `Copy of <name>` (cut to 60 chars), same brain, persona, instructions (current only, not history), tool switches, voice; no conversations, memory, routines, artifacts.
2. `Service::set_archived(bot, bool)`: archived bots are stopped (`stop_bot`), receive nothing (sends refused with `POTATO_ARCHIVED: This bot is archived. Restore it first.`), are skipped by groups fan-out (plan 12) and by outside configs (Discord/Telegram `bot_id` pointing to an archived bot -> refusal text from plan 11 policy is not used; instead the link stays on and answers nothing; Recent logs `paused`).
3. Context menu items: `Duplicate`, `Archive` (or `Restore` when archived).
4. List: archived bots under a collapsed header `Archived (n)`.

### Code
UI text:
```ts
export const BOT_MENU_COPY = {
  duplicate: 'Duplicate',
  duplicated: (name: string) => `Made "${name}".`,
  archive: 'Archive',
  restore: 'Restore',
  archivedHeader: (n: number) => `Archived (${n})`,
  archivedNote: 'Archived bots are stopped and hidden. Their chats are kept.',
} as const;
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `duplicate_copies_settings_not_chats` (Rust) | bot with 3 messages and instructions | new bot has instructions; 0 messages; name `Copy of Fish` |
| `archived_bot_refuses_send` (Rust) | archive, send | `Err` starting `POTATO_ARCHIVED:` |
| `archived_bot_stopped` (Rust) | archive running bot | fake launcher saw a stop |
| `menu shows duplicate and archive` (TS) | open menu | both items |
| `archived section collapsed` (TS) | 1 archived bot | `Archived (1)` header; bot name hidden until expanded |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R10 cargo test --lib potato
cd packages/app && npx vitest run src/components/cockpit/potato/BotArchive.test.tsx
```
Expected: 0 failed.

### Done when
- [ ] Duplicate and archive work from the bot menu; archived bots never run.

### Do NOT
- Do not copy memory or conversations on duplicate.
- Do not put archived bots in trash.

### If it fails
1. Bot settings spread over several files: copy each one `export_markdown` reads.
2. `BotList` filter breaks unread totals (`totalUnread` in `model.ts`): exclude archived bots from the total.
3. Context menu has no test file: create `BotArchive.test.tsx` covering both menu and list.

---

## Slice R1 - Reactions on bot messages

### Goal
The person can react to a bot message with one of four monochrome marks; the bot sees the reaction in its next turn.

### Read first
- `packages/app/src/components/cockpit/potato/MessageRow.tsx`, `packages/app/src/lib/potato.ts` (`PotatoMessage`).
- `packages/app/src-tauri/src/potato/store.rs` (message storage), `service.rs` `messages` (l.465).
- `packages/potato-brain/src/channels/potato.ts` (`onSend`, how history is given to the turn).

### Files
- edit `packages/app/src-tauri/src/potato/store.rs` (optional `reaction` on a stored message; `set_reaction`)
- edit `packages/app/src-tauri/src/commands/potato.rs` (`potato_react(bot_id, message_id, reaction)`) + register in `lib.rs`
- edit `packages/app/src/components/cockpit/potato/MessageRow.tsx`
- create `packages/app/src/components/cockpit/potato/Reactions.test.tsx`
- edit `packages/app/src-tauri/src/potato/service.rs` (next inbox item for that conversation carries `reactions: [{messageId, reaction}]` since the last turn)

### Steps
1. `Reaction` enum: `like`, `dislike`, `love`, `done`; `None` clears.
2. Command validates the message is a bot message in that bot's app conversation.
3. `MessageRow`: on hover/focus of a bot message, a small row of 4 icon buttons (line glyphs from the existing icon set `ThreadIcons.tsx`; NEEDS-FACT names) with `aria-label`s; the chosen one stays visible under the message.
4. Next send item includes pending reactions; the brain adds one line to the user turn: `(Ren reacted "<label>" to your message: "<first 80 chars>")`.
5. Tests.

### Code
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction { Like, Dislike, Love, Done }
```
UI labels (`aria-label` and the line the bot sees):
```ts
export const REACTION_COPY = { like: 'Good', dislike: 'Not good', love: 'Love it', done: 'Done', clear: 'Remove reaction' } as const;
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `react_on_bot_message` (Rust) | `potato_react(b, m, Some(Like))` | stored message has `reaction like` |
| `react_on_person_message_refused` (Rust) | person message id | `Err` starting `POTATO_REACT:` |
| `pending_reactions_in_next_item` (Rust) | react, then send | inbox item has `reactions: [{"messageId": m, "reaction": "like"}]`; the item after has none |
| `shows four buttons on focus` (TS) | focus a bot message | buttons `Good`, `Not good`, `Love it`, `Done` |
| `click reacts` (TS) | click `Good` | transport `react(bot, msg, 'like')` |
| `no reactions on person messages` (TS) | focus a person message | no reaction buttons |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R1 cargo test --lib potato
cd packages/app && npx vitest run src/components/cockpit/potato/Reactions.test.tsx
```
Expected: 0 failed.

### Done when
- [ ] Reactions show, persist, and reach the bot once.

### Do NOT
- Do not use colour emoji. Use the monochrome icon set.
- Do not start a brain turn because of a reaction alone.

### If it fails
1. Stored messages are append-only lines: store reactions in a side file `reactions.json` per conversation keyed by message id.
2. Icon set lacks a glyph: draw a 16 px SVG line icon in `ThreadIcons.tsx` with `currentColor`.
3. Brain ignores `reactions`: add the field to the `send` item type and the one-line prefix in `onSend`.

---

## Slice R2 - Settings from chat with an Undo card

### Goal
In an app chat, the person can ask a bot to change a few of its own settings; the change happens at once and the thread shows a card with Undo.

### Read first
- `packages/app/src-tauri/src/potato/bot_settings.rs` (setters), `protocol.rs` (`Event` list), `service.rs` `on_event`.
- `packages/potato-brain/src/core/loop.ts` (`ExtraTool`, `ToolLevel`), `channels/potato.ts` (`runTurn` tool list).
- `packages/app/src/components/cockpit/potato/ChoiceCard.tsx` (card style).

### Files
- edit `packages/app/src-tauri/src/potato/protocol.rs` (`Event::SettingChange { call_id, key, value }`, answer item `setting_result`)
- create `packages/app/src-tauri/src/potato/chat_settings.rs` (+ `chat_settings_tests.rs`)
- create `packages/potato-brain/src/core/settings-tool.ts` (+ `test/settings-tool.test.ts`)
- edit `packages/potato-brain/src/channels/potato.ts` (add the tool for app turns only; never when `sandboxOnly`)
- create `packages/app/src/components/cockpit/potato/SettingChangeCard.tsx` (+ test)

### Steps
1. Allowed keys only: `name` (1-40 chars), `persona` (`fish`|`potato`), `read_aloud` (`true`|`false`). Everything else is refused.
2. Brain tool `change_my_setting({key, value})` (Code); emits `setting_change`, waits for `setting_result` (30 s timeout).
3. Rust applies through the existing setters, stores an undo record `{id, bot, key, old, new, at}` (keep 20 per bot, 24 h), saves a card message in the thread, answers `ok` or an error.
4. Card: text + `Undo` button; Undo restores `old` and the card shows `Undone.`.
5. The tool is not offered when the item has `sandboxOnly` or trust `others` (scopeTools from plan 11 already drops it, since it is not a sandbox tool; add a test).

### Code
```ts
export const CHANGE_SETTING_SPEC = {
  name: "change_my_setting",
  description: "Change one of your own settings when the person asks. Keys: name (new name, 1-40 chars), persona (fish or potato), read_aloud (true or false).",
  parameters: {
    type: "object",
    properties: { key: { type: "string", enum: ["name", "persona", "read_aloud"] }, value: { type: "string", maxLength: 40 } },
    required: ["key", "value"],
    additionalProperties: false,
  },
} as const;
```
```rust
pub const KEYS: &[&str] = &["name", "persona", "read_aloud"];
pub fn validate(key: &str, value: &str) -> Result<(), &'static str>;
pub fn card_text(key: &str, old: &str, new: &str, bot: &str) -> String;
```
Card texts:
```
name:       "{old} changed its name to {new}."
persona:    "{bot} now talks like {Fish|Potato}."
read_aloud: "{bot} will read its answers aloud." / "{bot} will stop reading its answers aloud."
Undo button: "Undo"     after undo: "Undone."
Refused key (tool answer to the bot): "That setting can only be changed in the bot's settings."
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `validate_allows_three_keys` (Rust) | each key with a good value | `Ok` |
| `validate_refuses_others` (Rust) | `daily_limit`, `tools` | `Err` |
| `rename_saves_undo_and_card` (Rust) | `name` -> `Finn` | bot named Finn; undo record; card text `Fish changed its name to Finn.` |
| `undo_restores` (Rust) | undo that record | name Fish |
| `tool_absent_in_sandbox_only` (brain) | `scopeTools([..., change_my_setting], true)` | no `change_my_setting` |
| `card undo` (TS) | click `Undo` | transport undo called; text `Undone.` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R2 cargo test --lib potato
cd packages/potato-brain && node --test test/settings-tool.test.ts test/outside-scope.test.ts
cd packages/app && npx vitest run src/components/cockpit/potato/SettingChangeCard.test.tsx
```
Expected: 0 failed in each.

### Done when
- [ ] Only the 3 keys change from chat; every change has a working Undo for 24 h.

### Do NOT
- Do not allow changing money limits, tools, brain/model, instructions, or anything about Discord/Telegram from chat.
- Do not offer the tool in outside turns.

### If it fails
1. Waiting for `setting_result` blocks the turn loop: reuse the same await pattern as `sandbox_call`/`sandbox_result`.
2. Rename changes the bot id: ids must stay; only the display name changes.
3. Card message type unknown to the UI: add `setting_change` to the message kinds in `lib/potato.ts` and the Rust twin.

---

## Slice R8 - SSRF-safe `web_fetch` tool (app turns only)

### Goal
Give app-DM turns a `web_fetch(url)` tool that the app runs on the PC with strict network rules; outside turns use Fish's browser instead.

### Read first
- `packages/app/src-tauri/src/potato/protocol.rs` (`SandboxCall` + `sandbox_result` pattern to copy).
- `packages/app/src-tauri/src/potato/live.rs` `ask_model` (HTTP client).
- `packages/potato-brain/src/core/sandbox-tools.ts` (`CALL_TIMEOUT_MS`, `MAX_RESULT_CHARS`) and `core/loop.ts` (`ExtraTool`).

### Files
- create `packages/app/src-tauri/src/potato/web_fetch.rs` (+ `web_fetch_tests.rs`)
- edit `packages/app/src-tauri/src/potato/protocol.rs` (`Event::FetchCall { call_id, url }`, answer `fetch_result`)
- edit `packages/app/src-tauri/src/potato/service.rs` (handle `FetchCall` on a worker thread; max 2 at once per bot)
- create `packages/potato-brain/src/core/fetch-tool.ts` (+ `test/fetch-tool.test.ts`)
- edit `packages/potato-brain/src/channels/potato.ts` (tool only when not `sandboxOnly`)

### Steps
1. Pure checks (Code): scheme, host, port, IP class.
2. Resolve DNS on the PC; reject if ANY resolved address is not public; connect to the checked IP (no second lookup).
3. Max 3 redirects; each redirect target goes through the same checks.
4. Limits: 15 s total, 2 MB body, `text/*`, `application/json`, `application/xhtml+xml` only; HTML turned into plain text (strip tags, scripts, styles); result cut to 20 000 chars.
5. No cookies, no auth headers, user agent `PuppetMaster-PotatoBot/1 (+https://github.com/Potato-dev-inc/puppet-master)`.
6. Per-bot tool switch "Read web pages" in Advanced tools, default on (`DECISION-OPEN (default: on for app chats)`).

### Code
```rust
#[derive(Debug, PartialEq, Eq)]
pub enum FetchError { BadUrl, NotHttp, PrivateAddress, BadPort, TooManyRedirects, TooBig, NotText, Timeout, Network }

pub const ALLOWED_PORTS: &[u16] = &[80, 443, 8080, 8443];

pub fn check_url(url: &str) -> Result<url::Url, FetchError>;          // NEEDS-FACT: url crate present
pub fn is_public(ip: std::net::IpAddr) -> bool;                        // false for loopback, private, link-local,
                                                                       // CGNAT 100.64/10, multicast, unspecified,
                                                                       // documentation ranges, IPv4-mapped private v6
pub fn html_to_text(html: &str, max_chars: usize) -> String;
pub fn user_text(e: &FetchError) -> &'static str;
```
`user_text` (what the tool returns to the bot):
```
BadUrl / NotHttp / BadPort: "That is not a web address I can open."
PrivateAddress:             "That address is inside a private network, so I can't open it."
TooManyRedirects:           "That page sent me around too many times."
TooBig / NotText:           "That page is too big or is not text."
Timeout / Network:          "I couldn't reach that page."
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `rejects_file_scheme` | `file:///c:/x` | `Err(NotHttp)` |
| `rejects_loopback` | `is_public(127.0.0.1)`, `::1` | `false` |
| `rejects_private_and_cgnat` | `10.1.1.1`, `192.168.0.1`, `100.64.0.1`, `169.254.169.254` | all `false` |
| `rejects_mapped_v6` | `::ffff:192.168.0.1` | `false` |
| `accepts_public` | `1.1.1.1`, `2606:4700::1111` | `true` |
| `rejects_odd_port` | `http://example.com:22` | `Err(BadPort)` |
| `rejects_userinfo` | `http://user:pw@example.com` | `Err(BadUrl)` |
| `html_to_text_drops_scripts` | `<script>x</script><p>Hi</p>` | `"Hi"` |
| `tool_absent_in_sandbox_only` (brain) | `scopeTools([web_fetch,...], true)` | no `web_fetch` |
| `fetch_tool_round_trip` (brain, fake channel) | call -> fake `fetch_result` | tool returns the text |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R8 cargo test --lib potato::web_fetch
cd packages/potato-brain && node --test test/fetch-tool.test.ts test/outside-scope.test.ts
```
Expected: `test result: ok. 8 passed`; brain 0 failed.

### Done when
- [ ] App chats can read public pages; private, loopback and metadata addresses are refused, including through redirects and DNS.

### Do NOT
- Do not fetch from the brain process (the check must happen where the connection is made: Rust).
- Do not send cookies, keys or the DeepSeek key in any header.
- Do not offer `web_fetch` in outside turns.

### If it fails
1. HTTP client cannot connect to a pre-resolved IP: use its "resolve override" option (most clients have one) or build the request to `http(s)://<ip>` with the `Host` header and SNI set to the name (NEEDS-FACT for the client).
2. `url` crate missing: add it only if the repo already depends on it indirectly; else parse with the client's URL type.
3. Redirect test needs a server: test the redirect loop with a fake HTTP trait, not a real server.

---

## Slice R4 - Change-watch routines

### Goal
A routine can watch a web page and run its prompt only when the page text changed.

### Read first
- `packages/app/src-tauri/src/potato/routines.rs` (`routines.json`, 30 s tick, `add_pending`, `PendingRun`), `packages/app/src/components/cockpit/potato/RoutineForm.tsx`, `lib/potato-routines.ts`.
- `packages/app/src-tauri/src/potato/web_fetch.rs` (R8).

### Files
- edit `packages/app/src-tauri/src/potato/routines.rs` (new trigger kind)
- create `packages/app/src-tauri/src/potato/routines_watch.rs` (+ `routines_watch_tests.rs`)
- edit `packages/app/src/components/cockpit/potato/RoutineForm.tsx`
- edit `packages/app/src/lib/potato-routines.ts`
- create `packages/app/src/components/cockpit/potato/RoutineWatch.test.tsx`

### Steps
1. Trigger `WatchPage { url, every_minutes }` (`every_minutes` 15..=1440, default 60). Max 10 watch routines per bot.
2. On due: `web_fetch` the page (R8 rules), hash the text (SHA-256 of normalised whitespace), compare with the stored hash; first fetch stores the hash and does not run.
3. Changed -> `add_pending` with the routine prompt plus `The page changed. New text starts: "<first 500 chars>"`.
4. Fetch errors: keep the old hash; after 3 failures in a row, pause the routine and show `Paused: could not reach the page 3 times.`.

### Code
```rust
pub fn normalise(text: &str) -> String;                 // collapse whitespace, trim
pub fn changed(old_hash: Option<&str>, new_text: &str) -> (bool, String /*new hash*/); // first time -> (false, hash)
```
UI text (`RoutineForm`):
```
Trigger choice: "On a schedule" | "When a web page changes"
URL label: "Web page"   help: "Fish checks this page and runs the routine only when its text changes."
Every label: "Check every"   options: "15 minutes", "1 hour", "6 hours", "1 day"
Paused text: "Paused: could not reach the page 3 times."
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `first_fetch_does_not_run` | `changed(None, "a")` | `(false, h)` |
| `same_text_no_run` | `changed(Some(h("a")), " a ")` | `(false, ..)` |
| `new_text_runs` | `changed(Some(h("a")), "b")` | `(true, ..)` |
| `three_failures_pause` | 3 failed checks | routine paused |
| `limit_ten_watches` | add 11th | `Err` starting `POTATO_ROUTINE_LIMIT:` |
| `form shows watch fields` (TS) | pick `When a web page changes` | `Web page` and `Check every` shown |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-R4 cargo test --lib potato::routines
cd packages/app && npx vitest run src/components/cockpit/potato/RoutineWatch.test.tsx
```
Expected: 0 failed.

### Done when
- [ ] Watch routines run only on change, use R8's safe fetch, and pause after 3 failures.

### Do NOT
- Do not fetch more often than every 15 minutes.
- Do not run watch routines for archived bots (R10).

### If it fails
1. SHA-256 crate missing: use the hash the repo already uses (search `sha2`); else `std::hash` is NOT stable across runs: do not use it for stored hashes.
2. `routines.json` schema is versioned: bump its version and migrate old entries as schedule triggers.
3. The tick fetches synchronously and blocks: run fetches on the R8 worker.

---

## Slice R9 - Lazy tool catalog

### Goal
Send the model only short names and one-line descriptions of rarely used tools, and load a tool's full schema the first time the model asks for it.

### Read first
- `packages/potato-brain/src/core/loop.ts` (`Agent`, `ExtraTool`, `ToolLevel`, how tool specs are sent each turn), `core/cache.ts` (prefix cache: the catalog must not break caching).
- `packages/potato-brain/src/lifted/tool-schemas.ts`.

### Files
- create `packages/potato-brain/src/core/tool-catalog.ts`
- create `packages/potato-brain/test/tool-catalog.test.ts`
- edit `packages/potato-brain/src/core/loop.ts` (use the catalog when more than 8 tools are present)

### Steps
1. Split tools into `always` (the 6 Sandbox tools when the Sandbox is on, `ask_helper`, `memory_search`) and `lazy` (everything else, for example `web_fetch`, `change_my_setting`).
2. Add one meta tool `use_tool({name})` whose description lists each lazy tool as `name - one line`.
3. When the model calls `use_tool`, add that tool's full spec to the tool list for the rest of the conversation (sorted by name to keep the prefix cache stable) and answer `Ready. Call <name> now.`.
4. Below 9 tools in total, do nothing (send all specs as today).

### Code
```ts
export const ALWAYS = new Set(["computer_observe","computer_act","browser_navigate","browser_snapshot","browser_act","ask_person_for_screen","ask_helper","memory_search"]);
export interface ToolSpecLite { name: string; description: string; parameters: unknown }
export function splitTools(all: ToolSpecLite[]): { always: ToolSpecLite[]; lazy: ToolSpecLite[] };
export function catalogSpec(lazy: ToolSpecLite[]): ToolSpecLite;        // name "use_tool"
export class LoadedTools { load(name: string): boolean; specs(all: ToolSpecLite[]): ToolSpecLite[] }  // sorted by name
export const LAZY_MIN_TOTAL = 9;
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `few_tools_unchanged` | 6 tools | specs sent = the 6 |
| `catalog_lists_lazy` | 10 tools, 3 lazy | `use_tool` description has 3 lines `name - ...` |
| `load_adds_spec_sorted` | load `web_fetch` | specs = always + `use_tool` + `web_fetch`, lazy part sorted |
| `unknown_load_refused` | `use_tool({name:"rm"})` | answer `No tool called rm.` |
| `stable_order_for_cache` | same load twice | identical spec arrays |

### Run
```
cd packages/potato-brain && node --test test/tool-catalog.test.ts
cd packages/potato-brain && node --test test/*.test.ts
cd packages/potato-brain && npm run typecheck
```
Expected: `# fail 0` for both runs; typecheck exit 0.

### Done when
- [ ] With many tools, the first turn's tool list is smaller; loading a tool works and keeps the order stable.

### Do NOT
- Do not make the Sandbox tools lazy (outside turns rely on them).
- Do not reorder tools between turns (breaks the prefix cache).

### If it fails
1. `ExtraTool` has no `description`: use its spec's description field.
2. The model calls a lazy tool directly without `use_tool`: accept the call if the name is in the lazy set (load it on the fly).
3. Cache tests (`core/cache.ts`) fail: tool specs are part of the prefix; append loaded tools at the end only.

---

## Missing Rakazo items not built in this round

| Item (facts 8) | Why not now | Default |
|---|---|---|
| Group chats, bot-to-bot, handoff | Built in plan 12 | - |
| Semantic memory providers | Needs an embeddings provider (cost, a second key, a choice of model) | `DECISION-OPEN (default: not built; Markdown memory stays)` |
| Webhook / Git / Slack triggers | Each is an outside trigger: needs the plan 11 outside rules, a public endpoint and Slack is a new chat app | `DECISION-OPEN (default: not built)` |
| Voice calls | Needs real-time audio streaming and a speech model | `DECISION-OPEN (default: not built; read-aloud and dictation stay)` |
| Team vs private computer | Depends on team features and a second Sandbox per person | `DECISION-OPEN (default: one private Sandbox per PC)` |
| Teach a task | Needs recording and replay in the Sandbox; large | `DECISION-OPEN (default: not built)` |
| Site trust | Folded into R8 rules (public addresses only); a per-site allow/deny list is not built | `DECISION-OPEN (default: not built)` |
| Web search | Needs a search provider and key | `DECISION-OPEN (default: not built; Fish's browser can search)` |
| MCP client for Potato | Puts third-party tools inside bots; needs its own permission design | `DECISION-OPEN (default: not built)` |
| Tickets board | Overlaps with the open work list | `DECISION-OPEN (default: not built)` |
| Media and plots | Needs a safe renderer for model-made charts | `DECISION-OPEN (default: not built)` |
| Settings-from-chat undo cards | Built in R2 | - |
| Shell guard / desktop command guard | Built as R7 for outside turns (app turns unguarded by design) | - |
| Egress restriction | Built as R6 | - |
| Lazy tool catalog | Built as R9 | - |
| Change-watch routines | Built as R4 | - |
| Stuck-work reminders | Built as R3 | - |
| Messaging commands | Built as R5 | - |
| SSRF-safe fetch, web fetch | Built as R8 (fetch only) | - |

## Security

| Threat | What can happen | Defence | Test |
|---|---|---|---|
| Shipping a persona we have no right to | Legal risk | Release checklist item 1; record starts NOT CHECKED; test keeps it first | `starts with the Fish licence check` (F1) |
| Stranger uses Fish's browser to reach the home network, router or the PC | LAN attack | nftables egress drop for private ranges, DNS only to resolvers; start fails closed | `blocks_private_v4`, `dns_allowed_before_drop`, `replies_allowed_first` (R6) |
| Stranger makes Fish open a terminal and run code | Damage inside the Sandbox, a base for attacks | R7 guard on outside turns + R6 network limit + the Sandbox itself | `outside_turn_guarded` (R7) |
| Chat-app commands leak memory or change settings | Privacy, control | Only help/ping for others; owner-only mood/pause/resume; no recall/diary/config | `mood_not_owner_refused` (R5), `Do NOT` list |
| Bot changes its own dangerous settings | Spending or tools changed by prompt injection | Only name, persona, read-aloud; Undo; never in outside turns | `validate_refuses_others`, `tool_absent_in_sandbox_only` (R2) |
| SSRF through `web_fetch` | Reach `169.254.169.254`, the bridge on loopback, LAN devices | Rust-side DNS + IP checks, pinned connect, redirects re-checked, ports limited, no credentials | `rejects_loopback`, `rejects_private_and_cgnat`, `rejects_mapped_v6`, `rejects_odd_port` (R8) |
| Watch routine as a polling cannon | Hammering a site, cost | Min 15 minutes, max 10 per bot, pause after 3 failures, R8 rules | `limit_ten_watches`, `three_failures_pause` (R4) |
| Archived bot still answers outside | Unexpected replies | Archived bots refuse sends and are skipped by fan-out | `archived_bot_refuses_send` (R10) |
| Reminder spam | Annoyance | 3 per run, once per item per day, switch | `caps_three_oldest_first`, `reminded_recently_not_due` (R3) |

## What cannot be verified without real hardware or a second computer

- R6 on a real WSL 2 distro: that nftables is present, the rules apply, DNS still works, the screen proxy still works,
  and a request to the home router (for example `192.168.1.1`) is dropped. Needs the installed Sandbox and a home LAN.
- R8 against the real internet: redirects, slow sites, and DNS rebinding behaviour.
- R4 timing over days.
- The Fish licence result itself: a person must check the sources and sign the record.

## Open facts

- NEEDS-FACT: an existing release checklist location - search `docs/` for `release` and `checklist`.
- NEEDS-FACT: the Fish pack files and their recorded sources - read `packages/potato-brain/src/persona/packs.ts` and the pack folder it reads; `packages/potato-brain/src/lifted/PORTED.md`.
- NEEDS-FACT: whether a docs linter restricts new folders under `docs/` - search the repo for a docs check script.
- NEEDS-FACT: the values `potato_set_persona` accepts and `PersonaSwitch.tsx` shows today, and each pack's description - read `packages/app/src-tauri/src/commands/potato.rs` l.52-62, `packages/app/src/components/cockpit/potato/PersonaSwitch.tsx`, `packages/potato-brain/src/persona/packs.ts`.
- NEEDS-FACT: where mascot/avatar art lives in the app - search `packages/app/src` for `avatar` in `components/cockpit/potato/` and `companion`.
- NEEDS-FACT: the direction of the Sandbox screen/control connections (host -> guest or guest -> host) - read `packages/app/src-tauri/src/potato/sandbox/screen.rs` and `screen_live.rs`.
- NEEDS-FACT: the distro's package list and whether `nftables` is installed - read `packages/potato-sandbox/guest/desktop/apply-recipe.py`.
- NEEDS-FACT: how root commands are run and stdin piped in the guest - read `packages/app/src-tauri/src/potato/sandbox/wsl.rs`.
- NEEDS-FACT: the `computer_act` action shape (keys, typed text) and whether the dispatcher knows the turn's scope - read `packages/app/src-tauri/src/potato/sandbox_calls.rs` and `packages/potato-brain/src/core/sandbox-tools.ts` l.159-260.
- NEEDS-FACT: where the current mood is stored on the Rust side - search `packages/app/src-tauri/src/potato/` for `Mood`.
- NEEDS-FACT: open work item fields (`updated_ms`, `done`) - read `packages/app/src-tauri/src/potato/open_work.rs`.
- NEEDS-FACT: icon names in `ThreadIcons.tsx` - read `packages/app/src/components/cockpit/potato/ThreadIcons.tsx`.
- NEEDS-FACT: whether the crate depends on `url` and `sha2`, and whether the HTTP client supports a resolve override - read `packages/app/src-tauri/Cargo.toml` and `packages/app/src-tauri/src/potato/live.rs` `ask_model`.
- NEEDS-FACT: how tool specs are sent each turn and how the prefix cache keys them - read `packages/potato-brain/src/core/loop.ts` and `src/core/cache.ts`.
- NEEDS-FACT: the `routines.json` schema version - read `packages/app/src-tauri/src/potato/routines.rs`.
