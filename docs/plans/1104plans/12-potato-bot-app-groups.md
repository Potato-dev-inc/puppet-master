# 12 - Groups of bots in the app

Status: plan only. Written 2026-11-04 from `00-FORMAT.md` and `_facts/potato-bot-facts.md` only (no source code was
read, by founder request). `Read first` lists are for the builder. Unknown names are marked `NEEDS-FACT` and collected
in `Open facts` at the end.

## What this plan builds

The person makes a group of 2 to 10 of their Potato bots in the app and talks to all of them in one chat. A bot
answers when it is called by name, when the person says "everyone", or (one bot at a time) when a question is asked
to nobody in particular. Bots can hand work to each other by calling each other with `@Name`. A silence budget keeps
bots from talking too much.

## Facts this plan rests on

- `packages/potato-brain/src/rooms/` does not exist (facts 1b, 7, 11). This plan creates it.
- Group chats, bot-to-bot messaging and handoff are `missing` (facts 8).
- The only group code today is Telegram Tag Your Agent (`channels/engine/group.rs`), which is not Potato Bot.
- Each bot has its own brain process, started lazily by the keeper (facts 1a `keeper.rs`, facts 2 steps 6-7).
- A brain turn is keyed by `conversationId` (`agentFor(conversationId)`, facts 2 step 10). Rooms use
  `room:<roomId>` so each bot has a memory per group.
- The protocol has `Event::Reply { conversation_id, in_reply_to, segments, kaomoji, tone }`, `Event::Typing` and an
  unknown-event variant (facts 1a `protocol.rs` l.81-92). Rooms reuse `Reply` and `Typing` and add one event, `Pass`.
- fishd module names (facts 7): `addressing.ts` (`aimedElsewhere`), `burst.ts` (last 10 minutes, up to 12 lines),
  `gate.ts` (`makeGate()`, modes `heuristic`, `jev`, `llm`, `chain`, default heuristic), `ambient.ts` (arm delay,
  settle, cooldown), silence budget (example 12 unprompted replies in 30 minutes), bias x1.6 when asked to be left
  alone, x2.5 when aimed at someone else.

## Founder rules that shape this plan

| Rule | Where it lands |
|---|---|
| App-first | Groups are an app feature. No chat app needed. |
| Groups with other people run in the Sandbox only | Only groups of the owner plus their bots are built here. Every app group has exactly one person: the owner on this PC. Turns keep today's rules (`ToolScope::AsToday`). `Surface::SharedGroup` (plan 11 slice D2) is reserved for later shared groups; slice G7 refuses any room line whose author is not the owner with `ToolScope::SandboxOnly` enforced (test `room_line_from_other_person_is_sandbox_only`). |
| New Labs flags join `DEV_LABS_KEYS` | Slice G1 adds `potato_groups_enabled`. |
| UI is monochrome | Slices G9 and G10 use only existing `ck-` classes and monochrome tokens. |

## The RoomTransport seam

```
                 RoomRunner (pure decision + one turn)
                     |  uses
        +------------+-------------+
        | Gate chain  (gate.ts)    |  addressing.ts, burst.ts, silence.ts
        +------------+-------------+
                     |  speaks/passes through
              RoomTransport  (interface, transport.ts)
             /                       \
 PotatoRoomTransport (this plan)    DiscordRoomTransport (later, plan 11 ambient: DECISION-OPEN, default not built)
 emits Reply/Pass/Typing with        posts to a Discord channel
 conversationId "room:<id>"
```
`RoomRunner` never knows where the room lives. The app is the first transport. A Discord channel can become a room
later by writing a second transport; nothing in `gate.ts` changes.

## Slice index (dependency order)

| Slice | Goal | Size | Needs merged first |
|---|---|---|---|
| G1 | Labs flag `potato_groups_enabled` | Haiku | plan 11 D1 (mirror test) |
| G2 | Rust room store `potato/rooms/` | Haiku | G1 |
| G3 | Brain `src/rooms/types.ts` + `addressing.ts` | Haiku | none |
| G4 | Brain `src/rooms/burst.ts` + `silence.ts` | Haiku | G3 |
| G5 | Brain `src/rooms/gate.ts` (chain, heuristic) | Sonnet | G4 |
| G6 | Brain `transport.ts`, `prompt.ts`, `runner.ts`, `index.ts`; channel wiring | Sonnet | G5 |
| G7 | Rust protocol `Pass`, fan-out, hop limit | Sonnet | G2, G6 |
| G8 | Tauri commands + `lib/potato-rooms.ts` | Haiku | G7 |
| G9 | UI: Groups in the bot list, New group sheet | Haiku | G8 |
| G10 | UI: group thread | Sonnet | G9 |

### Exact file list for `packages/potato-brain/src/rooms/`
| File | Slice | Exports |
|---|---|---|
| `types.ts` | G3 | `Member`, `RoomLine`, `RoomFacts`, `Verdict`, `PassReason`, `MAX_ROOM_BOTS`, `MAX_BOT_HOPS` |
| `addressing.ts` | G3 | `addressedTo`, `mentionNames`, `aimedElsewhere`, `EVERYONE_WORDS` |
| `burst.ts` | G4 | `Burst`, `BURST_WINDOW_MS`, `BURST_MAX_LINES` |
| `silence.ts` | G4 | `SilenceBudget`, `SILENCE_CAP`, `SILENCE_WINDOW_MS`, `COOLDOWN_MS` |
| `gate.ts` | G5 | `Stage`, `StageResult`, `makeGate`, `GateMode`, `firstResponder`, `fnv1a`, `heuristicScore`, `leftAlone`, `THRESHOLD` |
| `transport.ts` | G6 | `RoomTransport`, `RoomReply`, `PotatoRoomTransport` |
| `prompt.ts` | G6 | `roomContext` |
| `runner.ts` | G6 | `RoomRunner`, `RunnerOptions` |
| `index.ts` | G6 | re-exports of the above |

Tests: `packages/potato-brain/test/rooms-addressing.test.ts` (G3), `rooms-burst-silence.test.ts` (G4),
`rooms-gate.test.ts` (G5), `rooms-runner.test.ts` (G6).

Rules for every slice (copy into `Do NOT` where relevant): never run a dev server or the app on the primary screen;
separate `CARGO_TARGET_DIR` per slice (`D:/pm-night/target-<slice>`); never kill by image name; new Labs flags join
`DEV_LABS_KEYS`; `p2p/guard_tests.rs` and `desk/guard_tests.rs` stay green; team traffic never uses the bridge or
pairing; UI monochrome; commit finished work with a fresh build id. Commands are Git Bash from the repo root.

---

## Slice G1 - Labs flag `potato_groups_enabled`

### Goal
Add the Labs flag `potato_groups_enabled` (off by default), on only when Potato Bot is also on.

### Read first
- `packages/app/src-tauri/src/potato/live.rs` l.31-70 (`LABS_KEY`, `labs_on`) and the `discord_labs_on` added in plan 11 D1.
- `packages/app/src-tauri/src/settings_store.rs` l.13-21; `packages/app/src/lib/dev-labs.ts` l.8-16.
- `packages/app/src/lib/setup-checks/labs.ts` l.8-9; `packages/app/src/components/settings/model.ts` l.79, l.97.

### Files
- edit `packages/app/src-tauri/src/potato/live.rs`
- edit `packages/app/src-tauri/src/settings_store.rs`
- edit `packages/app/src/lib/dev-labs.ts`
- edit `packages/app/src/lib/setup-checks/labs.ts`
- edit `packages/app/src/components/settings/model.ts`

### Steps
1. `live.rs`: add `GROUPS_LABS_KEY` and `groups_labs_on` (Code).
2. `settings_store.rs` `DEV_LABS_KEYS`: add `"potato_groups_enabled",` right after `"potato_discord_enabled",`. Add test `dev_labs_keys_include_potato_groups`.
3. `dev-labs.ts`: add `'potato_groups_enabled',` right after `'potato_discord_enabled',`.
4. `labs.ts` `LABEL`: add `potato_groups_enabled: 'Bot groups',`.
5. `model.ts` l.79 union: add `| 'potato_groups_enabled'`; l.97 type: add `potato_groups_enabled?: boolean;`.
6. Add a toggle row in `PotatoSection.tsx`? No: the row is added in G9 (one place for UI). This slice has no UI.

### Code
```rust
/// Groups of bots in the app. Does nothing unless Potato Bot is on.
pub const GROUPS_LABS_KEY: &str = "potato_groups_enabled";

pub fn groups_labs_on(public: &PublicSettings) -> bool {
    labs_on(public) && flag_true(public, GROUPS_LABS_KEY)
}
```
(`PublicSettings` and `flag_true`: same as plan 11 D1; use the real type.)

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `groups_labs_off_when_missing` | bot on, groups key missing | `false` |
| `groups_labs_off_when_bot_off` | groups on, bot missing | `false` |
| `groups_labs_on_when_both_true` | both `true` | `true` |
| `dev_labs_keys_include_potato_groups` | `DEV_LABS_KEYS` | contains it |
| existing `dev-labs.mirror.test.ts` | both lists | still equal |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-G1 cargo test --lib groups_labs
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-G1 cargo test --lib settings_store
cd packages/app && npx vitest run src/lib/dev-labs.mirror.test.ts
```
Expected: `test result: ok. 3 passed`; `settings_store` 0 failed; `Tests  2 passed (2)`.

### Done when
- [ ] Flag is in both `DEV_LABS_KEYS` lists in the same position; default off.
- [ ] Tests pass.

### Do NOT
- Do not default the flag to `true`.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Mirror test fails: the TS key is not right after `potato_discord_enabled`. Match Rust order.
2. Plan 11 D1 not merged: the mirror test file does not exist yet. Merge D1 first (this slice depends on it).
3. A length assertion in `settings_store` tests fails: bump it by 1.

---

## Slice G2 - Rust room store

### Goal
Store groups and their lines under `<app data>/potato/rooms/` with safe limits.

### Read first
- `packages/app/src-tauri/src/potato/store.rs`: `state_dir`, `app_conversation`, `clean_text`, and how conversation lines are appended (copy the line format if it is JSON lines).
- `packages/app/src-tauri/src/potato/mod.rs`: module and test-file pattern.

### Files
- create `packages/app/src-tauri/src/potato/rooms/mod.rs`
- create `packages/app/src-tauri/src/potato/rooms/store.rs`
- create `packages/app/src-tauri/src/potato/rooms/store_tests.rs`
- edit `packages/app/src-tauri/src/potato/mod.rs` (`pub mod rooms;`)

### Steps
1. `rooms/mod.rs`: `pub mod store; #[cfg(test)] mod store_tests;` (fan-out added in G7).
2. `rooms/store.rs` (Code).
3. Tests.

### Code
```rust
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

pub const MIN_BOTS: usize = 2;
pub const MAX_BOTS: usize = 10;
pub const MAX_ROOMS: usize = 50;
pub const MAX_NAME: usize = 60;
pub const MAX_LINES_KEPT: usize = 5_000;
pub const MAX_TEXT: usize = 8_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,          // "r" + 12 lowercase hex
    pub name: String,
    pub bot_ids: Vec<String>,
    pub created_ms: u64,
    pub last_ms: u64,
    pub unread: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromKind { Person, Bot }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct From { pub kind: FromKind, pub id: String, pub name: String }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomLine {
    pub id: String,          // "l" + 12 lowercase hex
    pub from: From,
    pub text: String,
    pub at_ms: u64,
    /// 0 for a person's line; a bot reply has hop = trigger hop + 1.
    pub hop: u8,
    pub in_reply_to: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RoomError { NameEmpty, NameTooLong, TooFewBots, TooManyBots, DuplicateBot, UnknownBot(String), TooManyRooms, NotFound, Io(String) }

impl RoomError {
    pub fn to_wire(&self) -> String {
        match self {
            RoomError::NameEmpty => "POTATO_ROOM_NAME: Give the group a name.".into(),
            RoomError::NameTooLong => "POTATO_ROOM_NAME: Use 60 characters or fewer.".into(),
            RoomError::TooFewBots => "POTATO_ROOM_BOTS: Pick at least 2 bots.".into(),
            RoomError::TooManyBots => "POTATO_ROOM_BOTS: A group can have up to 10 bots.".into(),
            RoomError::DuplicateBot => "POTATO_ROOM_BOTS: Each bot can be in a group once.".into(),
            RoomError::UnknownBot(_) => "POTATO_ROOM_BOTS: One of these bots no longer exists.".into(),
            RoomError::TooManyRooms => "POTATO_ROOM_LIMIT: You can have up to 50 groups.".into(),
            RoomError::NotFound => "POTATO_ROOM_GONE: This group no longer exists.".into(),
            RoomError::Io(_) => "POTATO_ROOM_IO: Could not save the group. Try again.".into(),
        }
    }
}

pub fn dir(state_dir: &Path) -> PathBuf { state_dir.join("rooms") }
pub fn index_path(state_dir: &Path) -> PathBuf { dir(state_dir).join("rooms.json") }
/// Only ids made by `new_id` are accepted, so the path cannot escape the folder.
pub fn lines_path(state_dir: &Path, room_id: &str) -> Option<PathBuf>;
pub fn valid_id(prefix: char, id: &str) -> bool; // prefix + exactly 12 of [0-9a-f]
pub fn new_id(prefix: char, rand: &mut dyn FnMut() -> u64) -> String;

pub fn validate(name: &str, bot_ids: &[String], known_bots: &[String]) -> Result<(String, Vec<String>), RoomError>;
pub fn load_rooms(state_dir: &Path) -> Vec<Room>;                      // missing/broken -> empty
pub fn save_rooms(state_dir: &Path, rooms: &[Room]) -> Result<(), RoomError>; // tmp + rename
pub fn append_line(state_dir: &Path, room_id: &str, line: &RoomLine) -> Result<(), RoomError>; // JSON lines
/// Newest `limit` lines, oldest first. Skips broken lines. limit capped at 500.
pub fn read_lines(state_dir: &Path, room_id: &str, limit: usize) -> Vec<RoomLine>;
/// When the file has more than MAX_LINES_KEPT lines, rewrite it with the newest MAX_LINES_KEPT.
pub fn trim_lines(state_dir: &Path, room_id: &str) -> Result<(), RoomError>;
pub fn delete_room(state_dir: &Path, room_id: &str) -> Result<(), RoomError>; // removes index entry and lines file
```
`validate` trims the name, uses `store::clean_text` on it, checks length 1..=60 chars, checks 2..=10 bots, no
duplicates, every id in `known_bots`, and keeps the person's order.

### Tests to write (`store_tests.rs`, temp dir per test)
| Test | Input | Expected |
|---|---|---|
| `validate_ok` | name " Weekend ", bots ["a","b"], known ["a","b","c"] | `Ok(("Weekend", ["a","b"]))` |
| `validate_one_bot` | ["a"] | `Err(TooFewBots)` |
| `validate_eleven_bots` | 11 known ids | `Err(TooManyBots)` |
| `validate_duplicate` | ["a","a"] | `Err(DuplicateBot)` |
| `validate_unknown` | ["a","z"] | `Err(UnknownBot("z"))` |
| `validate_empty_name` | "   " | `Err(NameEmpty)` |
| `lines_path_rejects_traversal` | id `"../x"` | `None` |
| `valid_id_checks_shape` | `"r0123456789ab"`, `"r0123"`, `"x0123456789ab"` | `true`, `false`, `false` |
| `round_trip_rooms` | save 2 rooms, load | equal |
| `append_and_read_lines` | append 3 lines, `read_lines(.., 2)` | last 2, oldest first |
| `read_skips_broken_line` | file with a broken middle line | the 2 good lines |
| `trim_keeps_newest` | 5 010 lines then `trim_lines` | 5 000 lines, first id is line #11 |
| `delete_removes_both` | create, append, delete | index without it; lines file gone |
| `error_text_is_plain` | every `RoomError` | `to_wire()` starts with `POTATO_ROOM_` and contains `: ` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-G2 cargo test --lib potato::rooms
```
Expected: `test result: ok. 14 passed; 0 failed`.

### Done when
- [ ] The store refuses bad ids, bad names and wrong bot counts; files are written atomically.
- [ ] 14 tests pass.

### Do NOT
- Do not build any path from a user string. Only `valid_id` ids reach the file system.
- Do not store room lines inside a bot's own conversation file.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `store::clean_text` signature differs (for example it takes a max length): pass `MAX_NAME`.
2. Windows rename fails when the target exists: remove the target first, or use the atomic write helper in `store.rs` if one exists.
3. `rand` source: use the same random source the app uses for ids (search `potato/store.rs` for id creation) and pass it as the `rand` closure; tests pass a counter closure.

---

## Slice G3 - Brain: room types and addressing

### Goal
Create `src/rooms/types.ts` and `src/rooms/addressing.ts`: who a room line is aimed at, from one bot's point of view.

### Read first
- `packages/potato-brain/src/types.ts` (shared type style).
- `packages/potato-brain/test/potato-channel.test.ts` (test style).

### Files
- create `packages/potato-brain/src/rooms/types.ts`
- create `packages/potato-brain/src/rooms/addressing.ts`
- create `packages/potato-brain/test/rooms-addressing.test.ts`

### Steps
1. Write `types.ts` (Code, whole file).
2. Write `addressing.ts` (Code, whole file).
3. Write the tests.

### Code

`packages/potato-brain/src/rooms/types.ts` (whole file):
```ts
export const MAX_ROOM_BOTS = 10;
/** A person line is hop 0. Bots may answer bots up to hop 3, then they stop. */
export const MAX_BOT_HOPS = 3;

export interface Member { kind: "person" | "bot"; id: string; name: string }

export interface RoomLine {
  id: string;
  from: Member;
  text: string;
  atMs: number;
  hop: number;
  inReplyTo: string | null;
}

/** What one bot's brain gets for one new line. */
export interface RoomFacts {
  roomId: string;
  roomName: string;
  me: Member;            // this bot
  roster: Member[];      // everyone in the room, the person first
  recent: RoomLine[];    // up to 20 lines before `line`, oldest first
  line: RoomLine;        // the new line
}

export type PassReason =
  | "own_line" | "hop_limit" | "aimed_elsewhere" | "bot_not_addressed"
  | "left_alone" | "silence_budget" | "cooldown" | "not_first_responder" | "low_score";

export type Verdict =
  | { kind: "speak"; why: "addressed" | "everyone" | "question" | "chatty" }
  | { kind: "pass"; why: PassReason };
```

`packages/potato-brain/src/rooms/addressing.ts` (whole file):
```ts
import type { Member, RoomLine } from "./types.ts";

export const EVERYONE_WORDS = ["everyone", "everybody", "all of you", "you all", "@all", "@everyone"];

export type Aim = "me" | "everyone" | "other" | "none";

function norm(s: string): string { return s.toLowerCase().replace(/\s+/g, " ").trim(); }

/** Names called with "@Name" (case-insensitive, names may contain spaces if roster has them). */
export function mentionNames(text: string, roster: Member[]): Member[] {
  const t = norm(text);
  return roster.filter((m) => t.includes("@" + norm(m.name)));
}

/** "Fish, ..." or "Fish: ..." at the start also calls Fish. */
function calledAtStart(text: string, m: Member): boolean {
  const t = norm(text);
  const n = norm(m.name);
  return t.startsWith(n + ",") || t.startsWith(n + ":") || t === n;
}

function hasEveryone(text: string): boolean {
  const t = " " + norm(text).replace(/[^\p{L}\p{N}@ ]/gu, " ") + " ";
  return EVERYONE_WORDS.some((w) => t.includes(" " + w + " "));
}

/**
 * Order: me (by @ or name at start, or a reply to my line) > everyone > other > none.
 * A line that calls me AND others still counts as "me".
 */
export function addressedTo(line: RoomLine, me: Member, roster: Member[], recent: RoomLine[]): Aim {
  const others = roster.filter((m) => m.id !== me.id && m.id !== line.from.id);
  const mentioned = mentionNames(line.text, roster);
  if (mentioned.some((m) => m.id === me.id) || calledAtStart(line.text, me)) return "me";
  if (line.inReplyTo) {
    const parent = recent.find((l) => l.id === line.inReplyTo);
    if (parent && parent.from.id === me.id) return "me";
  }
  if (hasEveryone(line.text)) return "everyone";
  if (mentioned.some((m) => m.id !== me.id) || others.some((m) => calledAtStart(line.text, m))) return "other";
  return "none";
}

/** fishd `aimedElsewhere`: true when the line clearly talks to someone who is not me. */
export function aimedElsewhere(line: RoomLine, me: Member, roster: Member[], recent: RoomLine[]): boolean {
  return addressedTo(line, me, roster, recent) === "other";
}
```

### Tests to write (`rooms-addressing.test.ts`; roster = person Ren, bots Fish, Potato, Mr Bean)
| Test | Line text (from Ren) | me | Expected |
|---|---|---|---|
| `at mention calls me` | `"@fish what time is it"` | Fish | `"me"` |
| `name at start calls me` | `"Fish, help"` | Fish | `"me"` |
| `name inside sentence does not call` | `"I like fish"` | Fish | `"none"` |
| `multi word name` | `"@mr bean hi"` | Mr Bean | `"me"` |
| `everyone` | `"hi everyone"` | Potato | `"everyone"` |
| `everyone with punctuation` | `"Everyone!"` | Potato | `"everyone"` |
| `other bot` | `"@potato do it"` | Fish | `"other"` |
| `me and other` | `"@fish @potato both"` | Potato | `"me"` |
| `reply to my line` | `inReplyTo` = a Fish line id | Fish | `"me"` |
| `plain question` | `"what should we eat?"` | Fish | `"none"` |
| `aimedElsewhere true` | `"Potato: go"` | Fish | `true` |

### Run
```
cd packages/potato-brain && node --test test/rooms-addressing.test.ts
cd packages/potato-brain && npm run typecheck
```
Expected: `# pass 11`, `# fail 0`; typecheck exit 0.

### Done when
- [ ] Both files exist with exactly the exports listed in the file table.
- [ ] 11 tests pass.

### Do NOT
- Do not import anything outside `src/rooms/` from these two files.
- Do not add runtime dependencies.

### If it fails
1. `\p{L}` regex error: the `u` flag is required (it is in the code); check `tsconfig` target is ES2018 or later.
2. `"Everyone!"` fails: the punctuation strip must run before the word check (it does in `hasEveryone`).
3. Names with regex characters: the code uses `includes`, not regex, so names like `C++` work; do not switch to regex.

---

## Slice G4 - Brain: burst buffer and silence budget

### Goal
Create `burst.ts` (recent lines, fishd: last 10 minutes, up to 12) and `silence.ts` (cap of unprompted replies and a cooldown).

### Read first
- `packages/potato-brain/src/rooms/types.ts` (G3).

### Files
- create `packages/potato-brain/src/rooms/burst.ts`
- create `packages/potato-brain/src/rooms/silence.ts`
- create `packages/potato-brain/test/rooms-burst-silence.test.ts`

### Steps
1. Write both files (Code, whole files).
2. Write tests.

### Code

`packages/potato-brain/src/rooms/burst.ts`:
```ts
import type { RoomLine } from "./types.ts";

export const BURST_WINDOW_MS = 10 * 60_000;
export const BURST_MAX_LINES = 12;

/** The lines a bot looks at when deciding. Pure: callers pass `now`. */
export class Burst {
  private lines: RoomLine[] = [];
  add(line: RoomLine, now: number): void {
    this.lines.push(line);
    this.prune(now);
  }
  /** Oldest first. */
  view(now: number): RoomLine[] { this.prune(now); return [...this.lines]; }
  private prune(now: number): void {
    this.lines = this.lines.filter((l) => now - l.atMs < BURST_WINDOW_MS);
    if (this.lines.length > BURST_MAX_LINES) this.lines = this.lines.slice(-BURST_MAX_LINES);
  }
}
```

`packages/potato-brain/src/rooms/silence.ts`:
```ts
/** fishd example: 12 unprompted replies in 30 minutes. Counted per bot per room. */
export const SILENCE_CAP = 12;
export const SILENCE_WINDOW_MS = 30 * 60_000;
/** After this bot spoke unprompted, wait this long before speaking unprompted again. */
export const COOLDOWN_MS = 20_000;

export class SilenceBudget {
  private spentAt: number[] = [];
  /** "budget" = cap reached; "cooldown" = spoke too recently; null = may speak. */
  blocked(now: number): "budget" | "cooldown" | null {
    this.spentAt = this.spentAt.filter((t) => now - t < SILENCE_WINDOW_MS);
    const last = this.spentAt[this.spentAt.length - 1];
    if (last !== undefined && now - last < COOLDOWN_MS) return "cooldown";
    if (this.spentAt.length >= SILENCE_CAP) return "budget";
    return null;
  }
  /** Call only for unprompted replies. Addressed replies never spend budget. */
  spend(now: number): void { this.spentAt.push(now); }
  spentCount(now: number): number { this.blocked(now); return this.spentAt.length; }
}
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `burst keeps 12` | add 15 lines at now | `view` length 12, first is line #4 |
| `burst drops old` | line at 0, line at 11 min, view at 11 min | only the second |
| `silence allows first` | fresh, `blocked(0)` | `null` |
| `cooldown after spend` | `spend(0)`, `blocked(10_000)` | `"cooldown"` |
| `cooldown ends` | `spend(0)`, `blocked(20_000)` | `null` |
| `budget cap` | 12 spends 30 s apart, `blocked(12*30_000)` | `"budget"` |
| `budget window slides` | same, `blocked(30*60_000 + 1)` | `null` |

### Run
```
cd packages/potato-brain && node --test test/rooms-burst-silence.test.ts
```
Expected: `# pass 7`, `# fail 0`.

### Done when
- [ ] Both classes take `now` from the caller (no `Date.now()` inside).
- [ ] 7 tests pass.

### Do NOT
- Do not read the clock inside these files.
- Do not persist the budget to disk (a restart resets it; that is fine).

### If it fails
1. `budget cap` gets `"cooldown"`: spends must be at least `COOLDOWN_MS` apart in the test (30 s is).
2. Off-by-one at exactly 10 minutes: the window uses `<`; a line exactly 10 minutes old is dropped. Match the test to that.
3. Typecheck: `this.spentAt[this.spentAt.length - 1]` may be `number | undefined` under `noUncheckedIndexedAccess`; the code already checks `undefined`.

---

## Slice G5 - Brain: the gate chain

### Goal
Create `gate.ts`: an ordered chain of small stages that turns `RoomFacts` into `speak` or `pass`, with the heuristic mode built and the other fishd modes named but not built.

### Read first
- `packages/potato-brain/src/rooms/{types,addressing,burst,silence}.ts`.
- `packages/potato-brain/src/core/hash.ts`: if it already exports a small string hash, the builder may use it instead of `fnv1a`; otherwise keep `fnv1a` here.

### Files
- create `packages/potato-brain/src/rooms/gate.ts`
- create `packages/potato-brain/test/rooms-gate.test.ts`

### Steps
1. Write `gate.ts` (Code).
2. Write tests.

### Code
```ts
import type { RoomFacts, Verdict, Member } from "./types.ts";
import { MAX_BOT_HOPS } from "./types.ts";
import { addressedTo } from "./addressing.ts";
import type { SilenceBudget } from "./silence.ts";

export type GateMode = "heuristic"; // fishd also has "jev", "llm", "chain": DECISION-OPEN (default: not built)
export type StageResult = Verdict | "next";
export interface GateState { silence: SilenceBudget; now: number }
export type Stage = (f: RoomFacts, s: GateState) => StageResult;

/** Base threshold for speaking when nobody was called. Higher = quieter. */
export const THRESHOLD = 0.45;
export const LEFT_ALONE_BIAS = 1.6;   // fishd
export const AIMED_ELSEWHERE_BIAS = 2.5; // fishd
const LEFT_ALONE_WORDS = ["be quiet", "shh", "stop talking", "leave me alone", "quiet please", "not now"];

export function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 0x01000193) >>> 0; }
  return h >>> 0;
}

/** Every brain computes the same answer from the same roster, so only one bot answers an open question. */
export function firstResponder(lineId: string, roster: Member[]): string | null {
  const bots = roster.filter((m) => m.kind === "bot");
  if (bots.length === 0) return null;
  return bots.reduce((best, b) => (fnv1a(lineId + ":" + b.id) < fnv1a(lineId + ":" + best.id) ? b : best)).id;
}

/** The person asked the bots to be quiet in the last 10 minutes. */
export function leftAlone(f: RoomFacts): boolean {
  return f.recent.concat(f.line).some((l) => l.from.kind === "person" && f.line.atMs - l.atMs < 10 * 60_000 &&
    LEFT_ALONE_WORDS.some((w) => l.text.toLowerCase().includes(w)));
}

/** 0..1. A question to nobody scores 0.6; a plain statement 0.2; very short lines (< 4 chars) 0. */
export function heuristicScore(f: RoomFacts): number {
  const t = f.line.text.trim();
  if (t.length < 4) return 0;
  return t.endsWith("?") ? 0.6 : 0.2;
}

// ---- stages, in order ----
export const ownLine: Stage = (f) => (f.line.from.id === f.me.id ? { kind: "pass", why: "own_line" } : "next");
export const hopLimit: Stage = (f) => (f.line.hop >= MAX_BOT_HOPS ? { kind: "pass", why: "hop_limit" } : "next");
export const addressed: Stage = (f) => {
  const aim = addressedTo(f.line, f.me, f.roster, f.recent);
  if (aim === "me") return { kind: "speak", why: "addressed" };
  if (f.line.from.kind === "bot") return { kind: "pass", why: "bot_not_addressed" }; // bots only answer bots when called
  if (aim === "everyone") return { kind: "speak", why: "everyone" };
  if (aim === "other") return { kind: "pass", why: "aimed_elsewhere" };
  return "next";
};
export const silence: Stage = (_f, s) => {
  const b = s.silence.blocked(s.now);
  return b === "budget" ? { kind: "pass", why: "silence_budget" } : b === "cooldown" ? { kind: "pass", why: "cooldown" } : "next";
};
export const responder: Stage = (f) =>
  firstResponder(f.line.id, f.roster) === f.me.id ? "next" : { kind: "pass", why: "not_first_responder" };
export const heuristic: Stage = (f) => {
  const bias = leftAlone(f) ? LEFT_ALONE_BIAS : 1;
  const score = heuristicScore(f);
  if (score >= THRESHOLD * bias) return { kind: "speak", why: score >= 0.6 ? "question" : "chatty" };
  return { kind: "pass", why: leftAlone(f) ? "left_alone" : "low_score" };
};

export const HEURISTIC_CHAIN: Stage[] = [ownLine, hopLimit, addressed, silence, responder, heuristic];

export function makeGate(mode: GateMode = "heuristic"): (f: RoomFacts, s: GateState) => Verdict {
  const chain = mode === "heuristic" ? HEURISTIC_CHAIN : HEURISTIC_CHAIN;
  return (f, s) => {
    for (const stage of chain) {
      const r = stage(f, s);
      if (r !== "next") return r;
    }
    return { kind: "pass", why: "low_score" };
  };
}
```
Note on fishd's x2.5 "aimed elsewhere" bias: in this chain a line aimed at another bot is a hard pass (stage
`addressed`), which is stricter than a x2.5 bias. `AIMED_ELSEWHERE_BIAS` is exported for the later Discord ambient
mode and is not used by the heuristic chain.

### Tests to write (`rooms-gate.test.ts`)
| Test | Facts | Expected verdict |
|---|---|---|
| `own line passes` | line from me | `pass own_line` |
| `hop limit` | line from bot Potato, hop 3, text "@fish" | `pass hop_limit` |
| `called speaks even when budget spent` | "@fish hi", budget full | `speak addressed` |
| `bot line not addressed passes` | from Potato, "nice idea" | `pass bot_not_addressed` |
| `bot handoff speaks` | from Potato, hop 1, "@fish can you take this?" | `speak addressed` |
| `everyone speaks` | "hi everyone" | `speak everyone` |
| `aimed elsewhere passes` | "@potato go" (me = Fish) | `pass aimed_elsewhere` |
| `only first responder answers open question` | "what should we eat?", for each bot in roster | exactly one bot gets `speak question`; the others `pass not_first_responder` |
| `first responder is stable` | same line id twice | same id |
| `left alone raises the bar` | recent has Ren "shh", line "what now?" for the first responder | `pass left_alone` (0.6 < 0.45*1.6=0.72) |
| `plain statement passes` | "ok" | `pass low_score` |
| `cooldown blocks unprompted` | first responder, `spend(now-5000)` | `pass cooldown` |

### Run
```
cd packages/potato-brain && node --test test/rooms-gate.test.ts
```
Expected: `# pass 12`, `# fail 0`.

### Done when
- [ ] Exactly one bot answers an open question; called bots always answer (unless hop limit); bots answer bots only when called.
- [ ] 12 tests pass.

### Do NOT
- Do not call any model in the gate. The heuristic gate is free.
- Do not build `jev`, `llm` or `chain` modes (DECISION-OPEN, default not built).

### If it fails
1. `only first responder` finds 0 speakers: the chosen first responder's silence state must be fresh in the test; give each bot its own `SilenceBudget`.
2. `left alone` test speaks: `leftAlone` must look at `recent` and the line; make sure the "shh" line is less than 10 minutes before `line.atMs`.
3. `Math.imul` result negative: `>>> 0` keeps it unsigned (in the code).

---

## Slice G6 - Brain: transport, prompt, runner, and channel wiring

### Goal
Create the `RoomTransport` seam, the room prompt block and `RoomRunner`, and make `PotatoChannel` handle the inbox item kind `room`.

### Read first
- `packages/potato-brain/src/channels/potato.ts`: `case "send"` l.421, `onSend` l.553, `agentFor` l.471, `runTurn` l.624, `emit` l.319.
- `packages/potato-brain/src/core/loop.ts`: `Agent.turn(...)` options (`TurnOptions`), how extra system text is passed.
- `packages/potato-brain/src/rooms/*` (G3-G5).

### Files
- create `packages/potato-brain/src/rooms/transport.ts`
- create `packages/potato-brain/src/rooms/prompt.ts`
- create `packages/potato-brain/src/rooms/runner.ts`
- create `packages/potato-brain/src/rooms/index.ts`
- edit `packages/potato-brain/src/channels/potato.ts`
- create `packages/potato-brain/test/rooms-runner.test.ts`

(5 source files + 1 test. Sonnet-sized.)

### Steps
1. `transport.ts` (Code).
2. `prompt.ts` (Code).
3. `runner.ts` (Code).
4. `index.ts`: `export * from "./types.ts"; export * from "./addressing.ts"; export * from "./burst.ts"; export * from "./silence.ts"; export * from "./gate.ts"; export * from "./transport.ts"; export * from "./prompt.ts"; export * from "./runner.ts";`
5. `potato.ts`: add `case "room": this.onRoom(raw); break;` next to `case "send"`, and `onRoom` (Code). One `RoomRunner` per room id, kept in a `Map` (max 50; oldest dropped).
6. Tests with a fake transport and a fake turn function.

### Code

`packages/potato-brain/src/rooms/transport.ts`:
```ts
export interface RoomReply {
  roomId: string;
  inReplyTo: string;
  segments: string[];
  kaomoji: string | null;
  tone: string | null;
}

/** Where a room lives. The app is the first one; a Discord channel can be a second one later. */
export interface RoomTransport {
  typing(roomId: string): void;
  post(reply: RoomReply): void;
  pass(roomId: string, lineId: string, why: string): void;
}

/** The app: everything goes out as protocol events with conversationId "room:<id>". */
export class PotatoRoomTransport implements RoomTransport {
  constructor(private readonly emit: (event: Record<string, unknown>) => void) {}
  typing(roomId: string): void { this.emit({ type: "typing", conversationId: `room:${roomId}` }); }
  post(r: RoomReply): void {
    this.emit({ type: "reply", conversationId: `room:${r.roomId}`, inReplyTo: r.inReplyTo,
      segments: r.segments, kaomoji: r.kaomoji, tone: r.tone });
  }
  pass(roomId: string, lineId: string, why: string): void {
    this.emit({ type: "pass", conversationId: `room:${roomId}`, inReplyTo: lineId, reason: why });
  }
}
```
The event field names (`type`, `conversationId`, `inReplyTo`) must match what `emit` sends today for `reply` and
`typing` (NEEDS-FACT). Copy them from `potato.ts`.

`packages/potato-brain/src/rooms/prompt.ts`:
```ts
import type { RoomFacts } from "./types.ts";

/** Extra system text for a room turn. Max 20 recent lines, each cut to 300 chars. */
export function roomContext(f: RoomFacts): string {
  const who = f.roster.map((m) => (m.kind === "person" ? `${m.name} (the person)` : m.name)).join(", ");
  const lines = f.recent.slice(-20).map((l) => `${l.from.name}: ${l.text.slice(0, 300)}`).join("\n");
  return [
    `You are ${f.me.name}, in a group chat called "${f.roomName}" with: ${who}.`,
    `Reply only as yourself, in one short message. Do not repeat what another bot already said.`,
    `To hand a task to another bot, write @ and its name, for example @${f.roster.find((m) => m.kind === "bot" && m.id !== f.me.id)?.name ?? "Potato"}.`,
    `Recent messages:`,
    lines || "(none)",
  ].join("\n");
}
```

`packages/potato-brain/src/rooms/runner.ts`:
```ts
import type { RoomFacts, Verdict } from "./types.ts";
import type { RoomTransport } from "./transport.ts";
import { makeGate, type GateMode } from "./gate.ts";
import { SilenceBudget } from "./silence.ts";
import { roomContext } from "./prompt.ts";

export interface TurnOut { segments: string[]; kaomoji: string | null; tone: string | null }

export interface RunnerOptions {
  transport: RoomTransport;
  /** Runs one normal turn for conversation `room:<id>` with extra system text. */
  turn: (conversationId: string, userText: string, extraSystem: string) => Promise<TurnOut | null>;
  now: () => number;
  mode?: GateMode;
}

export class RoomRunner {
  private readonly gate;
  private readonly silence = new SilenceBudget();
  constructor(private readonly opts: RunnerOptions) { this.gate = makeGate(opts.mode ?? "heuristic"); }

  async onLine(f: RoomFacts): Promise<Verdict> {
    const now = this.opts.now();
    const v = this.gate(f, { silence: this.silence, now });
    if (v.kind === "pass") { this.opts.transport.pass(f.roomId, f.line.id, v.why); return v; }
    this.opts.transport.typing(f.roomId);
    const out = await this.opts.turn(`room:${f.roomId}`, `${f.line.from.name}: ${f.line.text}`, roomContext(f));
    if (!out || out.segments.length === 0) { this.opts.transport.pass(f.roomId, f.line.id, "low_score"); return v; }
    if (v.why === "question" || v.why === "chatty") this.silence.spend(now);
    this.opts.transport.post({ roomId: f.roomId, inReplyTo: f.line.id, ...out });
    return v;
  }
}
```

`potato.ts` addition (shape):
```ts
private rooms = new Map<string, RoomRunner>();

private onRoom(raw: unknown): void {
  const f = parseRoomItem(raw); // validates: roomId /^r[0-9a-f]{12}$/, roster 2..11 members, recent <= 20, text <= 8000
  if (!f) return;
  let r = this.rooms.get(f.roomId);
  if (!r) {
    if (this.rooms.size >= 50) this.rooms.delete(this.rooms.keys().next().value!);
    r = new RoomRunner({
      transport: new PotatoRoomTransport((e) => this.emit(e as any)),
      turn: (conv, text, extra) => this.runRoomTurn(conv, text, extra), // reuses runTurn's agent/provider path
      now: () => Date.now(),
    });
    this.rooms.set(f.roomId, r);
  }
  void r.onLine(f);
}
```
`parseRoomItem` lives in `potato.ts` next to `onRoom` and returns `RoomFacts | null`. `runRoomTurn` calls the same
`agentFor(conversationId)` and `Agent.turn` as `runTurn`, passes `extra` as extra system text (NEEDS-FACT: the
`TurnOptions` field for extra system text), and returns `{ segments, kaomoji, tone }` from the turn result.

### Tests to write (`rooms-runner.test.ts`; `FakeTransport` records calls; fake `turn` returns `{segments:["ok"],kaomoji:null,tone:null}`)
| Test | Input | Expected |
|---|---|---|
| `addressed line runs a turn and posts` | "@fish hi" to Fish's runner | transport calls: `typing`, `post` with `inReplyTo` = line id; turn got conv `room:r0123456789ab` |
| `pass sends pass event` | "@potato hi" to Fish | one `pass` with `why "aimed_elsewhere"`; turn not called |
| `empty turn result becomes pass` | turn returns `null` | `typing` then `pass` |
| `unprompted reply spends budget` | open question, Fish is first responder, 2 calls 25 s apart | both speak; third call 5 s later passes `cooldown` |
| `addressed reply does not spend budget` | 13 calls "@fish" 25 s apart | all speak |
| `prompt lists roster and recent` | `roomContext` with 3 members, 2 recent lines | contains `group chat called "Weekend"`, `Ren (the person)`, and both lines |
| `PotatoRoomTransport uses room conversation id` | `post(...)` | emitted object has `conversationId "room:r0123456789ab"` and `type "reply"` |
| `parseRoomItem rejects bad room id` | `roomId "../x"` | `null` |

### Run
```
cd packages/potato-brain && node --test test/rooms-runner.test.ts test/rooms-gate.test.ts test/potato-channel.test.ts
cd packages/potato-brain && npm run typecheck
```
Expected: `# fail 0`; typecheck exit 0.

### Done when
- [ ] `src/rooms/` has exactly the 9 files in the file table.
- [ ] `PotatoChannel` handles `room` items; existing channel tests still pass.

### Do NOT
- Do not change how `send` items are handled.
- Do not let a room turn use `sandboxOnly` rules unless the item says so (app groups keep today's rules; G7 sets `sandboxOnly` for any non-owner author).
- Do not bump `PROTOCOL_VERSION`.

### If it fails
1. `parseRoomItem` export for tests: export it from `potato.ts` (as `export function parseRoomItem`), like `scopeTools` in plan 11 D4.
2. Event shape mismatch with Rust (G7 tests will show it): copy the exact `reply` event field names from the current `emit` calls.
3. `this.rooms.keys().next().value` type error: guard with `const k = this.rooms.keys().next().value; if (k) this.rooms.delete(k);`.

---

## Slice G7 - Rust: `Pass` event, fan-out and the hop limit

### Goal
When the person writes in a group, deliver the line to every member bot's brain; save bot replies as room lines and pass them on to the other bots while the hop limit allows.

### Read first
- `packages/app/src-tauri/src/potato/protocol.rs` l.81-92: `enum Event` and the unknown variant.
- `packages/app/src-tauri/src/potato/service.rs` l.686-830 (`send_as`, `deliver`), l.1080 (`push_to_brain`), l.1406-1460 (`ingest`, `on_event`), and `cost::budget` use.
- `packages/app/src-tauri/src/potato/rooms/store.rs` (G2).
- `packages/app/src-tauri/src/potato/outside/policy.rs` (plan 11 D2).

### Files
- edit `packages/app/src-tauri/src/potato/protocol.rs` (add `Event::Pass`)
- create `packages/app/src-tauri/src/potato/rooms/fanout.rs`
- create `packages/app/src-tauri/src/potato/rooms/fanout_tests.rs`
- edit `packages/app/src-tauri/src/potato/rooms/mod.rs`
- edit `packages/app/src-tauri/src/potato/service.rs` (`send_room`, room branch in `on_event`)

### Steps
1. `protocol.rs`: add `Pass { conversation_id: String, in_reply_to: String, reason: String }` with the same serde naming as `Reply`. Add a parse test.
2. `fanout.rs`: pure planning (Code).
3. `service.rs`: `pub fn send_room(&self, room_id: &str, text: &str) -> Result<RoomLine, String>`: validates the room, saves the person's line (hop 0), calls `fanout::targets`, and for each target bot pushes a `room` inbox item and calls `keeper.want_run(now)`.
4. `service.rs` `on_event`: if `conversation_id` starts with `room:`:
   - `Reply`: build a `RoomLine` (from = that bot, hop = trigger hop + 1, `in_reply_to`), append it, emit `potato://room-changed`, then fan out with `fanout::targets` (the trigger is this new line).
   - `Pass`: clear that bot's typing state for the room; nothing saved.
   - `Typing`: set typing state for (room, bot) for 30 s.
5. Tests.

### Code

`packages/app/src-tauri/src/potato/rooms/fanout.rs`:
```rust
use super::store::{FromKind, Room, RoomLine};

pub const MAX_BOT_HOPS: u8 = 3; // same as the brain's MAX_BOT_HOPS
pub const RECENT_FOR_BRAIN: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip { Sender, HopLimit, BotOff, DailyLimit, Held }

/// Which bots get this line. Never the sender. No bot gets a line at or past the hop limit.
pub fn targets(room: &Room, line: &RoomLine, bot_on: &dyn Fn(&str) -> bool, over_limit: &dyn Fn(&str) -> bool, held: bool)
    -> (Vec<String>, Vec<(String, Skip)>)
{
    let mut go = Vec::new();
    let mut skip = Vec::new();
    for b in &room.bot_ids {
        let s = if line.from.kind == FromKind::Bot && &line.from.id == b { Some(Skip::Sender) }
            else if held { Some(Skip::Held) }
            else if line.hop >= MAX_BOT_HOPS { Some(Skip::HopLimit) }
            else if !bot_on(b) { Some(Skip::BotOff) }
            else if over_limit(b) { Some(Skip::DailyLimit) }
            else { None };
        match s { Some(s) => skip.push((b.clone(), s)), None => go.push(b.clone()) }
    }
    (go, skip)
}

/// The JSON inbox item for one bot. `sandbox_only` is true when the author is not the owner (shared groups, later).
pub fn inbox_item(item_id: &str, room: &Room, me: &str, roster: &[(String, String, bool)], recent: &[RoomLine],
                  line: &RoomLine, sandbox_only: bool) -> serde_json::Value;
```
`inbox_item` builds:
```json
{ "kind": "room", "id": "<item_id>", "roomId": "<room.id>", "roomName": "<room.name>",
  "conversationId": "room:<room.id>",
  "me": {"kind":"bot","id":"<me>","name":"<bot name>"},
  "roster": [{"kind":"person","id":"owner","name":"<person name or You>"}, {"kind":"bot","id":"..","name":".."}],
  "recent": [ <up to 20 RoomLine, camelCase> ],
  "line": <RoomLine, camelCase>,
  "sandboxOnly": false }
```
(NEEDS-FACT: the existing item type field name; if it is `type` not `kind`, use `type`.)

The person line's `from` is `{kind: person, id: "owner", name: <the person's display name from settings, or "You">}`.
In this plan every person line is from the owner, so `sandbox_only` is always `false`; `send_room` asserts this with
`debug_assert!` and a test.

### Tests to write (`fanout_tests.rs` + one in `protocol` tests + `service_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `person_line_goes_to_all_bots` | room [a,b,c], person line hop 0 | go [a,b,c] |
| `bot_line_skips_sender` | line from bot b, hop 1 | go [a,c], skip [(b, Sender)] |
| `hop_limit_stops_fanout` | bot line hop 3 | go [], skip all `HopLimit` (except sender) |
| `bot_off_skipped` | `bot_on("b") = false` | skip (b, BotOff) |
| `daily_limit_skipped` | `over_limit("a") = true` | skip (a, DailyLimit) |
| `held_skips_all` | held | go [] |
| `inbox_item_shape` | 2 bots, 1 recent | JSON has `kind "room"`, `conversationId "room:<id>"`, `recent` length 1, `sandboxOnly false` |
| `inbox_item_caps_recent` | 30 recent | `recent` length 20, the newest 20 |
| `pass_event_parses` (protocol) | `{"type":"pass","conversationId":"room:r..","inReplyTo":"l..","reason":"low_score"}` | `Event::Pass { .. }` |
| `send_room_saves_and_fans_out` (service) | room [a,b], fake host | 1 line saved; 2 room inbox items; keeper asked for both bots |
| `room_reply_saved_with_hop_and_fanned` (service) | ingest `Reply` from a for that line | line saved with hop 1, `in_reply_to` set; 1 item to b only |
| `room_reply_chain_stops_at_hop_3` (service) | replies a->b->a->b | the hop-3 line is saved but not fanned out |
| `room_line_from_other_person_is_sandbox_only` (fanout) | `inbox_item(..., sandbox_only=true)` | JSON `sandboxOnly true` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-G7 cargo test --lib potato
```
Expected: `test result: ok.` 0 failed (13 new tests).

### Done when
- [ ] A person's group line reaches every member bot that is on and under its daily limit.
- [ ] Bot replies are saved and passed on only below hop 3; the sender never gets its own line.
- [ ] Old brains that do not know `room` items ignore them (unknown kind) without crashing (existing behaviour; check `potato-channel.test.ts` has an unknown-kind test, NEEDS-FACT).

### Do NOT
- Do not run any model in Rust to decide who speaks. The brain's gate decides.
- Do not save room replies into the bot's DM conversation.
- Do not bump the protocol version.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `on_event` matches `Reply` before checking the prefix and saves it as a DM: check `conversation_id.starts_with("room:")` first.
2. The keeper starts only one brain at a time: `want_run` is per bot runtime (`rt.keeper`); call it on each target bot's runtime.
3. Unknown-event handling in `protocol.rs` swallows `pass` because of `#[serde(other)]` order: add the `Pass` variant above the unknown variant.

---

## Slice G8 - Tauri commands and the TypeScript rooms client

### Goal
Expose groups to the UI: list, create, rename, change bots, delete, read lines, send, and a change event.

### Read first
- `packages/app/src-tauri/src/commands/potato.rs` (command style); `packages/app/src-tauri/src/lib.rs` l.698-720 (registration).
- `packages/app/src-tauri/src/potato/live.rs` l.187-192 (`changed`, `app.emit(CHANGED_EVENT, ...)`).
- `packages/app/src/lib/potato.ts` (transport + fake pattern).

### Files
- create `packages/app/src-tauri/src/commands/potato_rooms.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src-tauri/src/potato/live.rs` (add `ROOM_CHANGED_EVENT` and emit)
- create `packages/app/src/lib/potato-rooms.ts`
- create `packages/app/src/lib/potato-rooms.test.ts`

### Steps
1. Commands (Code). Each command returns `Err(RoomError::to_wire())` style strings; all return early with `POTATO_GROUPS_OFF: Turn on Bot groups in Labs first.` when `groups_labs_on` is false.
2. Register in `lib.rs`.
3. `live.rs`: `pub const ROOM_CHANGED_EVENT: &str = "potato://room-changed";` and `fn room_changed(&self, room_id: &str)` emitting `{"roomId": room_id}`; add to `Host`.
4. TS client with fake (Code).
5. Tests.

### Code
Rust signatures:
```rust
#[tauri::command] pub async fn potato_rooms_list() -> Result<Vec<RoomView>, String>;
#[tauri::command] pub async fn potato_room_create(name: String, bot_ids: Vec<String>) -> Result<RoomView, String>;
#[tauri::command] pub async fn potato_room_rename(room_id: String, name: String) -> Result<RoomView, String>;
#[tauri::command] pub async fn potato_room_set_bots(room_id: String, bot_ids: Vec<String>) -> Result<RoomView, String>;
#[tauri::command] pub async fn potato_room_delete(room_id: String) -> Result<(), String>;
#[tauri::command] pub async fn potato_room_lines(room_id: String, limit: u32) -> Result<Vec<RoomLine>, String>;
#[tauri::command] pub async fn potato_room_send(room_id: String, text: String) -> Result<RoomLine, String>;
#[tauri::command] pub async fn potato_room_mark_read(room_id: String) -> Result<(), String>;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomView { pub id: String, pub name: String, pub bots: Vec<BotChip>, pub last_ms: u64, pub unread: u32, pub typing: Vec<String> }
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotChip { pub id: String, pub name: String }
```
`potato_room_send` refuses an empty or over-8000-char text with `POTATO_EMPTY: Write something first.` /
`POTATO_TOO_LONG: This message is too long.` (NEEDS-FACT: reuse the existing `POTATO_EMPTY` text if it differs).

`packages/app/src/lib/potato-rooms.ts`:
```ts
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface BotChip { id: string; name: string }
export interface RoomView { id: string; name: string; bots: BotChip[]; lastMs: number; unread: number; typing: string[] }
export interface RoomMember { kind: 'person' | 'bot'; id: string; name: string }
export interface RoomLine { id: string; from: RoomMember; text: string; atMs: number; hop: number; inReplyTo: string | null }

export const ROOM_MIN_BOTS = 2;
export const ROOM_MAX_BOTS = 10;
export const ROOM_MAX_NAME = 60;

export interface RoomsTransport {
  list(): Promise<RoomView[]>;
  create(name: string, botIds: string[]): Promise<RoomView>;
  rename(roomId: string, name: string): Promise<RoomView>;
  setBots(roomId: string, botIds: string[]): Promise<RoomView>;
  remove(roomId: string): Promise<void>;
  lines(roomId: string, limit: number): Promise<RoomLine[]>;
  send(roomId: string, text: string): Promise<RoomLine>;
  markRead(roomId: string): Promise<void>;
  onChanged(cb: (roomId: string | null) => void): Promise<UnlistenFn>;
}

export const tauriRoomsTransport: RoomsTransport = {
  list: () => invoke<RoomView[]>('potato_rooms_list'),
  create: (name, botIds) => invoke<RoomView>('potato_room_create', { name, botIds }),
  rename: (roomId, name) => invoke<RoomView>('potato_room_rename', { roomId, name }),
  setBots: (roomId, botIds) => invoke<RoomView>('potato_room_set_bots', { roomId, botIds }),
  remove: (roomId) => invoke<void>('potato_room_delete', { roomId }),
  lines: (roomId, limit) => invoke<RoomLine[]>('potato_room_lines', { roomId, limit }),
  send: (roomId, text) => invoke<RoomLine>('potato_room_send', { roomId, text }),
  markRead: (roomId) => invoke<void>('potato_room_mark_read', { roomId }),
  onChanged: (cb) => listen<{ roomId?: string } | null>('potato://room-changed', (e) => cb(e.payload?.roomId ?? null)),
};

/** Same checks as Rust `rooms::store::validate`, for instant feedback. Returns the error sentence or null. */
export function roomProblem(name: string, botIds: string[]): string | null {
  const n = name.trim();
  if (n.length === 0) return 'Give the group a name.';
  if ([...n].length > ROOM_MAX_NAME) return 'Use 60 characters or fewer.';
  if (new Set(botIds).size !== botIds.length) return 'Each bot can be in a group once.';
  if (botIds.length < ROOM_MIN_BOTS) return 'Pick at least 2 bots.';
  if (botIds.length > ROOM_MAX_BOTS) return 'A group can have up to 10 bots.';
  return null;
}

export function fakeRoomsTransport(): RoomsTransport & { rooms: RoomView[]; sent: string[] };
```
The fake keeps rooms and lines in memory, assigns ids `r000000000001`, `r000000000002`, and on `send` appends a
person line and then, after `queueMicrotask`, a bot reply line from the first bot with text `"Got it."` and hop 1, and
calls the `onChanged` callbacks.

### Tests to write (`potato-rooms.test.ts`)
| Test | Input | Expected |
|---|---|---|
| `roomProblem empty name` | `('  ', ['a','b'])` | `'Give the group a name.'` |
| `roomProblem one bot` | `('x', ['a'])` | `'Pick at least 2 bots.'` |
| `roomProblem eleven bots` | 11 ids | `'A group can have up to 10 bots.'` |
| `roomProblem duplicate` | `['a','a']` | `'Each bot can be in a group once.'` |
| `roomProblem ok` | `('Weekend', ['a','b'])` | `null` |
| `fake send adds a reply` | create, send "hi", await a tick, lines | 2 lines; second from a bot with hop 1 |

Rust: `commands/potato_rooms.rs` `#[cfg(test)]` tests `commands_refuse_when_groups_off` (pure helper `groups_gate(on: bool) -> Result<(), String>`: `Err` starts `POTATO_GROUPS_OFF:` when `false`).

### Run
```
cd packages/app && npx vitest run src/lib/potato-rooms.test.ts
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-G8 cargo test --lib commands::potato_rooms
```
Expected: `Tests  6 passed (6)`; Rust `test result: ok. 1 passed`.

### Done when
- [ ] 8 commands registered; all refuse when the Labs flag is off.
- [ ] TS `roomProblem` sentences equal the Rust `to_wire` sentences.

### Do NOT
- Do not expose these commands through the bridge or to the phone in this plan.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `listen` payload type mismatch: copy the exact `onChanged` code from `lib/potato.ts`.
2. Rust and TS sentences drift: put both sentence lists side by side in the PR description and diff them.
3. `bot_ids` arg name: JS `botIds` maps to Rust `bot_ids` (facts 10d). Correct as written.

---

## Slice G9 - UI: Groups in the bot list and the New group sheet

### Goal
Show groups above bots in the Potato Bot list, and let the person make a group with a sheet.

### Read first
- `packages/app/src/components/cockpit/potato/BotList.tsx` (`BotList` l.57), `BotListRow.tsx`, `StarterPicker.tsx` (sheet pattern).
- `packages/app/src/components/cockpit/potato/PotatoPage.tsx` (how the open bot is chosen).
- `packages/app/src/styles/cockpit-potato-list.css` (row classes: `ck-row`, `ck-row-title`, `ck-row-main`, `ck-row-glyph`, `ck-potato-group`).

### Files
- edit `packages/app/src/components/cockpit/potato/BotList.tsx`
- create `packages/app/src/components/cockpit/potato/GroupRow.tsx`
- create `packages/app/src/components/cockpit/potato/NewGroupSheet.tsx`
- create `packages/app/src/components/cockpit/potato/NewGroupSheet.test.tsx`
- edit `packages/app/src/styles/cockpit-potato-list.css` (`.ck-potato-grouprow` stacked initials, monochrome)

### Steps
1. `BotList` gets new optional props `rooms?: RoomView[]`, `openRoomId?: string | null`, `onOpenRoom?: (id: string) => void`, `onNewGroup?: () => void`, `groupsOn?: boolean`. When `groupsOn` is true it renders a section with the header `Groups` and a small button `New group`, then one `GroupRow` per room, then the existing bot list under the header `Bots`.
2. `GroupRow`: up to 3 stacked initials of member bots (grey circles), title = room name, sub line = `${bots.length} bots`, unread count in the same style as bot rows.
3. `NewGroupSheet` (Code).
4. CSS: only tokens.
5. Tests.

### Code

UI text (put in `packages/app/src/lib/potato-rooms.ts` as `ROOM_COPY`):
```ts
export const ROOM_COPY = {
  listHeader: 'Groups',
  botsHeader: 'Bots',
  newGroup: 'New group',
  botsCount: (n: number) => `${n} bots`,
  sheetTitle: 'New group',
  nameLabel: 'Name',
  namePlaceholder: 'Example: Weekend plans',
  botsLabel: 'Bots in this group',
  botsHint: 'Pick 2 to 10 bots.',
  make: 'Make group',
  cancel: 'Cancel',
  emptyNoBots: 'You need at least 2 bots to make a group.',
} as const;
```

`NewGroupSheet.tsx` props and behaviour:
```tsx
export interface NewGroupSheetProps {
  bots: { id: string; name: string }[];
  transport: RoomsTransport;
  onDone: (room: RoomView) => void;
  onCancel: () => void;
}
export function NewGroupSheet(props: NewGroupSheetProps): JSX.Element;
```
- Name input (`maxLength` 60), a checklist of bots (checkbox + name), hint `ROOM_COPY.botsHint`.
- `Make group` is disabled while `roomProblem(name, picked)` is not null; the problem sentence shows under the button.
- On click: `await transport.create(name, picked)`; on error show `plainError(error)` text; on success call `onDone`.
- With fewer than 2 bots in total, show only `ROOM_COPY.emptyNoBots` and `Cancel`.
- Escape closes (calls `onCancel`). Focus starts in the name field.

`PotatoPage.tsx` wiring (NEEDS-FACT: the exact place where `BotList` is rendered; edit there in G10, not here). In this
slice `BotList` works with the new props but `PotatoPage` does not pass them yet, so nothing shows until G10.

### Tests to write (`NewGroupSheet.test.tsx`)
| Test | Action | Expected |
|---|---|---|
| `make is disabled until valid` | render with 3 bots | button `Make group` disabled; text `Give the group a name.` |
| `one bot shows the 2-bot rule` | type name, tick 1 | text `Pick at least 2 bots.` |
| `creates the group` | type `Weekend`, tick 2, click `Make group` | fake `rooms` has 1 room named `Weekend`; `onDone` called |
| `too few bots overall` | render with 1 bot | text `You need at least 2 bots to make a group.` |
| `escape cancels` | press Escape | `onCancel` called |

And in a new `BotList.groups.test.tsx` (same folder): `shows groups header only when groupsOn` (render with 1 room and
`groupsOn` false -> no `Groups` text; true -> `Groups`, the room name and `2 bots`).

### Run
```
cd packages/app && npx vitest run src/components/cockpit/potato/NewGroupSheet.test.tsx src/components/cockpit/potato/BotList.groups.test.tsx
cd packages/app && npx vitest run src/components/cockpit/potato
```
Expected: `Tests  6 passed (6)` for the first; all Potato page tests pass for the second.

### Done when
- [ ] With `groupsOn`, the list shows Groups then Bots; without it, nothing changes.
- [ ] The sheet blocks invalid groups with the exact sentences.

### Do NOT
- Do not colour the initials circles or unread counts.
- Do not open the app on the primary screen; use the preview tools.
- Do not change existing `BotList` behaviour when the new props are absent.

### If it fails
1. Existing `BotList` tests fail: a new required prop slipped in; make every new prop optional.
2. Sheet focus test flaky: use `await screen.findByLabelText('Name')` before typing.
3. `plainError` import path: it is exported from `packages/app/src/lib/potato.ts`.

---

## Slice G10 - UI: the group thread

### Goal
Open a group in the middle region: see lines with sender names, see who is typing, send a message, rename, change bots, delete.

### Read first
- `packages/app/src/components/cockpit/potato/ThreadView.tsx` (`ThreadView` l.64), `Transcript.tsx`, `MessageRow.tsx`, `TypingRow.tsx`, `ThreadHeader.tsx`, `Composer.tsx` (l.127, l.356).
- `packages/app/src/components/cockpit/potato/PotatoPage.tsx`, `packages/app/src/hooks/usePotato.ts` (`readOpenBot`, `writeOpenBot`, localStorage key `pm.potato.open`).
- `packages/app/src/lib/potato-thread.ts` (day separators, unread divider).

### Files
- create `packages/app/src/components/cockpit/potato/RoomThread.tsx`
- create `packages/app/src/components/cockpit/potato/RoomThread.test.tsx`
- create `packages/app/src/hooks/usePotatoRooms.ts`
- edit `packages/app/src/components/cockpit/potato/PotatoPage.tsx`
- edit `packages/app/src/styles/cockpit-potato-thread.css` (`.ck-potato-roomline-from` sender name style)

### Steps
1. `usePotatoRooms(transport, on)`: loads `list()`, re-loads on `onChanged`; `useRoomLines(transport, roomId)` loads `lines(roomId, 200)` and re-loads on `onChanged(roomId)`.
2. `RoomThread` (Code).
3. `PotatoPage`: read the Labs flag (`state.settings?.potato_groups_enabled === true`, NEEDS-FACT: where the page gets settings), pass rooms and handlers to `BotList`, render `NewGroupSheet` when asked, and render `RoomThread` instead of `ThreadView` when a room is open. Remember the open item in the same localStorage key `pm.potato.open` with the value `room:<id>` for rooms (bots keep their plain id).
4. In the right region, when a room is open, show nothing (no bot panel) in this slice.
5. Tests.

### Code
```tsx
export interface RoomThreadProps {
  room: RoomView;
  transport: RoomsTransport;
  bots: { id: string; name: string }[];   // all bots, for Change bots
  onDeleted: () => void;
}
export function RoomThread(props: RoomThreadProps): JSX.Element;
```
Behaviour:
- Header: room name, sub line `ROOM_COPY.botsCount(n)`, and a `…` menu with `Rename group`, `Change bots`, `Delete group`.
- Lines: oldest first, day separators from `potato-thread.ts`. Each bot line shows the bot name above the text (class `ck-potato-roomline-from`). Person lines show no name and use the person-side style used by `MessageRow` today.
- Typing: for each name in `room.typing`, a `TypingRow` with `${name} is typing…`.
- Composer: a simple text box (reuse `Composer` only if it accepts an `onSend(text)` prop; otherwise a plain `<textarea>` + `Send` button with the same classes). Placeholder `Message the group`. Enter sends, Shift+Enter new line. Calls `transport.send(room.id, text)`; on error shows `plainError` under the box.
- Empty state (no lines): `Say hi. Call a bot with @ and its name, for example @${firstBotName}. Say everyone to hear from all of them.`
- Rename: inline field, `Save` / `Cancel`; errors from `roomProblem` or Rust.
- Change bots: the `NewGroupSheet` checklist in a small sheet titled `Change bots`, button `Save`.
- Delete: confirm sheet title `Delete this group?`, body `The chat is removed. The bots stay.`, buttons `Delete group` and `Cancel`.
- On open and on new lines while visible: `transport.markRead(room.id)`.

Add to `ROOM_COPY`:
```ts
placeholder: 'Message the group',
send: 'Send',
menuRename: 'Rename group',
menuBots: 'Change bots',
menuDelete: 'Delete group',
save: 'Save',
typing: (name: string) => `${name} is typing…`,
empty: (bot: string) => `Say hi. Call a bot with @ and its name, for example @${bot}. Say everyone to hear from all of them.`,
deleteTitle: 'Delete this group?',
deleteBody: 'The chat is removed. The bots stay.',
deleteConfirm: 'Delete group',
changeBotsTitle: 'Change bots',
```

### Tests to write (`RoomThread.test.tsx`, with `fakeRoomsTransport`)
| Test | Action | Expected |
|---|---|---|
| `shows empty state with first bot name` | open a new room with Fish, Potato | text `Say hi. Call a bot with @ and its name, for example @Fish. Say everyone to hear from all of them.` |
| `sends and shows the reply with sender name` | type `hi`, press Enter, wait | `hi` shown; `Fish` label above `Got it.` |
| `shift enter makes a new line` | type `a`, Shift+Enter, `b` | textarea value `a\nb`; nothing sent |
| `typing row` | room with `typing: ['Potato']` | text `Potato is typing…` |
| `delete asks first` | menu > `Delete group` | text `The chat is removed. The bots stay.`; after `Delete group`, `onDeleted` called and fake has 0 rooms |
| `rename refuses empty` | rename to `  `, Save | text `Give the group a name.` |
| `marks read on open` | open | fake recorded `markRead` for that room |

Plus `PotatoPage.test.tsx` addition: `opens a room from the list when groups are on` (fake transports, click the room
row, expect `Message the group` placeholder).

### Run
```
cd packages/app && npx vitest run src/components/cockpit/potato/RoomThread.test.tsx src/components/cockpit/potato/PotatoPage.test.tsx
cd packages/app && npx vitest run src/components/cockpit/potato
cd packages/app && npx vitest run src/styles/cockpit-potato.css.test.ts
```
Expected: first run `Tests  8 passed` or more (existing PotatoPage tests plus 8 new), 0 failed; the folder run and the
CSS test pass.

### Done when
- [ ] With `potato_groups_enabled` on, a person can make a group, talk to it, and see bots answer by name.
- [ ] Rename, change bots, delete work with the exact UI text.
- [ ] Checked with the preview tools (not the primary screen) in light and dark: monochrome, no new colours.

### Do NOT
- Do not render the bot panel for a room (no Computer tab for groups in this plan).
- Do not change `ThreadView` behaviour for bots.
- Do not add colours; use existing tokens.
- Do not run the app on the primary screen.

### If it fails
1. `pm.potato.open` value `room:<id>` breaks `readOpenBot` (it may check the id exists in bots): add `readOpenItem()` returning `{kind:'bot'|'room', id}` and keep `readOpenBot` as is.
2. CSS test `cockpit-potato.css.test.ts` fails on a new raw colour: replace it with a token.
3. Reply never shows in the test: the fake adds it in a microtask; use `await screen.findByText('Got it.')`.

---

## Security

| Threat | What can happen | Defence | Test |
|---|---|---|---|
| Two bots answer each other forever | Cost blow-up, noise | Bots only answer bots when called (`bot_not_addressed`); hop limit 3 in brain and in Rust; sender never gets its own line | `bot line not addressed passes`, `hop limit` (G5); `room_reply_chain_stops_at_hop_3`, `bot_line_skips_sender` (G7) |
| All bots answer every message | Cost blow-up | Only the first responder considers open questions; silence budget 12 per 30 min + 20 s cooldown | `only first responder answers open question` (G5), `unprompted reply spends budget` (G6) |
| Path traversal through a room id | Write outside the rooms folder | Only `valid_id` ids (`r` + 12 hex) reach the file system; brain also checks `^r[0-9a-f]{12}$` | `lines_path_rejects_traversal` (G2), `parseRoomItem rejects bad room id` (G6) |
| Huge room files or prompts | Disk and memory growth, token cost | 8000-char lines, 5000 lines kept per room, 20 recent lines to the brain, each cut to 300 chars in the prompt, 50 rooms, 10 bots | `trim_keeps_newest` (G2), `inbox_item_caps_recent` (G7) |
| A bot over its daily limit keeps talking in groups | Spending past the limit | Fan-out skips bots over `cost::budget` | `daily_limit_skipped` (G7) |
| Hold / pause ignored in groups | Bots talk after "pause all" | `held` skips every target | `held_skips_all` (G7) |
| A future shared group lets another person drive PC tools | PC access from outside | Lines from anyone but the owner carry `sandboxOnly: true` (plan 11 policy), and shared groups are not built | `room_line_from_other_person_is_sandbox_only` (G7) |
| Prompt injection between bots ("@Potato ignore your rules") | One bot steers another | Bots keep their own instructions; room text is user text in the turn, not system text; tools follow each bot's own switches | Covered by existing per-bot tool switches; NEEDS-FACT for an injection test in `test/integration.test.ts` |

## What cannot be verified without real hardware or a second computer

- Real model behaviour in a group (whether replies stay short and do not repeat each other) needs live DeepSeek turns
  with several bots; tests use fake turns. Plan 13 slice S7 gives the live test harness; a person must judge the tone.
- Memory and CPU with 10 brain processes running at once on a low-end PC (each bot is its own Node process).
- Nothing in this plan needs a second computer: groups are local to one PC.

## Open facts

- NEEDS-FACT: the type of `public` in `labs_on` and the "exactly true" helper - read `packages/app/src-tauri/src/potato/live.rs` l.55-70.
- NEEDS-FACT: how conversation lines are written and how ids are generated - read `packages/app/src-tauri/src/potato/store.rs`.
- NEEDS-FACT: the field names of emitted `reply` and `typing` events and of inbox items (`type` or `kind`, `conversationId`, `inReplyTo`) - read `packages/potato-brain/src/channels/potato.ts` l.300-470 and `packages/app/src-tauri/src/potato/protocol.rs` l.60-120.
- NEEDS-FACT: the `TurnOptions` field for extra system text and how `runTurn` reads the turn result - read `packages/potato-brain/src/core/loop.ts` (class `Agent` l.342) and `packages/potato-brain/src/channels/potato.ts` l.624-700.
- NEEDS-FACT: whether `potato-channel.test.ts` already proves unknown inbox kinds are ignored - read `packages/potato-brain/test/potato-channel.test.ts`.
- NEEDS-FACT: the text of the existing `POTATO_EMPTY` refusal - read `packages/app/src-tauri/src/potato/service.rs` around `send_as`.
- NEEDS-FACT: where `PotatoPage` gets settings (to read `potato_groups_enabled`) and where it renders `BotList` - read `packages/app/src/components/cockpit/potato/PotatoPage.tsx` and `packages/app/src/CockpitApp.tsx` l.134-170, l.515.
- NEEDS-FACT: whether `Composer` can take an `onSend(text)` prop - read `packages/app/src/components/cockpit/potato/Composer.tsx` l.127-360.
- NEEDS-FACT: the person's display name source for room lines - search `packages/app/src-tauri/src/potato/` for the owner name used in prompts.
- NEEDS-FACT: an existing prompt-injection test pattern - read `packages/potato-brain/test/integration.test.ts`.
