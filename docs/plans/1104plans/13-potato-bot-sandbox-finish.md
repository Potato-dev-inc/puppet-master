# 13 - Finish Fish's computer (the Sandbox)

Status: plan only. Written 2026-11-04 from `00-FORMAT.md` and `_facts/potato-bot-facts.md` only (no source code was
read, by founder request). `Read first` lists are for the builder. Unknown names are `NEEDS-FACT`, listed at the end.

## What this plan finishes

1. The stale doc sentence in `docs/features/potato-sandbox.md` l.8 (slice S1).
2. Phone take-over: a phone may control Fish's computer only after a yes on this PC, once per phone (S2-S4).
3. Installer bundle check: the installed app really has the brain and Sandbox guest files (S5-S6).
4. A live DeepSeek test that a person runs with their own key (S7).
5. A Local / Sandbox switch per bot, with copy-in of a Git folder and bring-back as a branch opened in Review Desk
   (S8-S11).

## Facts this plan rests on

- The Sandbox is a WSL 2 distro: `potato/sandbox/` with `trait Sandbox` (mod.rs l.140), `WslSandbox` (l.210),
  `FakeSandbox` (l.582), and `manager.rs`, `detect.rs`, `install.rs`, `lifecycle.rs`, `control.rs`, `lease.rs`,
  `screen.rs`, `screen_live.rs`, `browser.rs`, `wsl.rs`, `live.rs`, `secrets.rs` (facts 1a).
- Control lease / take-over is done in `potato/sandbox/lease.rs`; screen proxy and sealed links are partial
  (`bridge/routes/potato_screen.rs`, `potato/sandbox/screen.rs`) (facts 8).
- `docs/features/potato-bot.md` l.126: a paired phone can watch. Control from a phone needs a yes on this PC first;
  "that yes is not built yet"; a phone gets `PHONE_NEEDS_DESKTOP_YES` (facts 6e).
- `docs/features/potato-sandbox.md` l.8 says a bot using it and watching/taking over its screen "come in later
  pieces". This conflicts with `docs/features/potato-bot.md`, which describes both as built. Treated as stale (facts 6e).
- `docs/features/potato-sandbox.md` l.138-139: the install reads guest files from app resources or the source tree.
  `potato/resource_dirs.rs` finds the bundled `potato-brain` and `potato-sandbox` folders (facts 1a, 6e).
- DeepSeek check: `live.rs` `check_key` (l.255) -> `ask_model` (l.266), one request to
  `https://api.deepseek.com/chat/completions`, model `deepseek-chat`, `max_tokens: 1`. Key name `deepseek_api_key` in
  the OS store, given to the brain only as env `DEEPSEEK_API_KEY` (facts 3).
- Brain test files include `test/real-brain.test.ts` and `test/integration.test.ts` (facts 9).
- The window guard (`potato/window_guard.rs`) stops a start if a visible WSLg window exists (facts 6d).
- Founder: phone take-over needs a desktop yes the first time; outside triggers are Sandbox only; Labs flags stay off.

## Slice index (dependency order)

| Slice | Goal | Size | Needs merged first |
|---|---|---|---|
| S1 | Fix the stale sentence in `potato-sandbox.md` | Haiku | none |
| S2 | Phone control allow-list + decision (pure) | Haiku | none |
| S3 | Phone control asks wired into the lease path | Sonnet | S2 |
| S4 | Desktop yes card + Settings list of allowed phones | Haiku | S3 |
| S5 | `resource_check.rs`: required brain and Sandbox files | Haiku | none |
| S6 | `scripts/verify-potato-bundle.mjs` for built installers | Haiku | S5 |
| S7 | Live DeepSeek tests (brain + Rust, opt-in) | Haiku | none |
| S8 | Per-bot work place: Local or Sandbox | Sonnet | plan 11 D2 |
| S9 | Copy-in: a Git folder into the Sandbox | Sonnet | S8 |
| S10 | Bring-back: Sandbox branch into the PC repo + Review Desk | Sonnet | S9 |
| S11 | Thread cards for copy-in and bring-back | Haiku | S10 |

Rules for every slice: never run a dev server or the app on the primary screen; separate `CARGO_TARGET_DIR` per slice
(`D:/pm-night/target-<slice>`); never kill by image name; new Labs flags join `DEV_LABS_KEYS` (none are added in this
plan); `p2p/guard_tests.rs` and `desk/guard_tests.rs` stay green; pairing has no route from agents/MCP/phone; team
traffic never uses the bridge or pairing; UI monochrome; commit finished work with a fresh build id. Commands are Git
Bash from the repo root.

---

## Slice S1 - Fix the stale doc sentence

### Goal
Replace the stale sentence at `docs/features/potato-sandbox.md` l.8 so it agrees with `docs/features/potato-bot.md`.

### Read first
- `docs/features/potato-sandbox.md` l.1-20 and l.125-140.
- `docs/features/potato-bot.md` l.100-130 (what is built: tools, take-over, phone view).

### Files
- edit `docs/features/potato-sandbox.md`

### Steps
1. Replace l.8 exactly as shown in Code.
2. At l.132 ("No sign-in copying yet...") leave the sentence; it is still true.
3. Add one line under the "Not yet" area (wherever l.132 lives) listing what this plan adds later: phone control yes, Local/Sandbox switch, copy-in and bring-back.

### Code
Before (l.8):
```
A bot using it, and watching or taking over its screen, come in later pieces.
```
After:
```
A bot uses it through six tools (computer_observe, computer_act, browser_navigate, browser_snapshot, browser_act,
ask_person_for_screen). You can watch its screen and take over from this PC. A paired phone can watch; control from a
phone needs a yes on this PC first (see docs/plans/1104plans/13-potato-bot-sandbox-finish.md, slices S2-S4).
```
New line in the "Not yet" area:
```
Not yet: the phone control yes, a Local / Sandbox switch per bot, copying a Git folder in, and bringing work back as
a branch (docs/plans/1104plans/13-potato-bot-sandbox-finish.md).
```

### Tests to write
None (doc only). The check is the grep in Run.

### Run
```
grep -n "come in later pieces" docs/features/potato-sandbox.md; echo "exit=$?"
```
Expected: no matching line and `exit=1`.

### Done when
- [ ] The stale sentence is gone; the new text names the six tools exactly.
- [ ] Committed as `docs: potato-sandbox matches potato-bot (S1)`.

### Do NOT
- Do not edit `docs/features/potato-bot.md` in this slice.
- Do not change any code.

### If it fails
1. The sentence is not on l.8 any more: search for `later pieces` and replace it where it is.
2. A docs guard test lists feature docs: run `cd packages/app && npx vitest run` only if a docs test exists (NEEDS-FACT); otherwise none.
3. Line endings change the whole file: keep the file's existing line endings (check with `git diff --stat`, expect 1 file, a few lines).

---

## Slice S2 - Phone control allow-list and decision (pure)

### Goal
A pure store of phones allowed to control Fish's computer and a pure decision: allowed, ask the desktop, or refuse.

### Read first
- `packages/app/src-tauri/src/potato/sandbox/lease.rs`: lease holder types (who holds control: app, phone, bot).
- `packages/app/src-tauri/src/bridge/routes/potato_screen.rs`: where `PHONE_NEEDS_DESKTOP_YES` is returned today (NEEDS-FACT: exact place).
- The phone pairing module that gives each paired phone a stable id (NEEDS-FACT: type and module name).

### Files
- create `packages/app/src-tauri/src/potato/sandbox/phone_yes.rs`
- create `packages/app/src-tauri/src/potato/sandbox/phone_yes_tests.rs`
- edit `packages/app/src-tauri/src/potato/sandbox/mod.rs` (declare both)

### Steps
1. Write `phone_yes.rs` (Code).
2. Declare `pub mod phone_yes;` and its test file in `sandbox/mod.rs`.
3. Write tests.

### Code
```rust
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "phone_control.json";
pub const ASK_TTL_MS: u64 = 120_000;   // a desktop question lasts 2 minutes
pub const MAX_PHONES: usize = 10;
pub const MAX_OPEN_ASKS: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedPhone { pub device_id: String, pub name: String, pub since_ms: u64 }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowList { pub allowed: Vec<AllowedPhone> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask { pub ask_id: String, pub device_id: String, pub device_name: String, pub bot_id: String, pub at_ms: u64 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate { Allowed, AskDesktop(Ask), StillWaiting(String /* ask_id */), Refused }

#[derive(Debug, Default)]
pub struct Asks { open: Vec<Ask>, refused_until: std::collections::HashMap<String, u64> }

pub fn path(state_dir: &Path) -> PathBuf { state_dir.join(FILE) }
pub fn load(state_dir: &Path) -> AllowList;                       // missing/broken -> empty
pub fn save(state_dir: &Path, list: &AllowList) -> Result<(), String>; // tmp + rename

impl AllowList {
    pub fn is_allowed(&self, device_id: &str) -> bool;
    /// Adds or refreshes; keeps at most MAX_PHONES (oldest `since_ms` dropped). Name cut to 60 chars.
    pub fn allow(&mut self, device_id: &str, name: &str, now_ms: u64);
    pub fn revoke(&mut self, device_id: &str) -> bool;
}

/// The phone asks to take control.
/// - allowed -> Allowed
/// - refused in the last 10 minutes -> Refused (stops a phone from spamming the desktop)
/// - an open ask for this phone -> StillWaiting(ask_id)
/// - more than MAX_OPEN_ASKS open -> Refused
/// - else a new Ask (ask_id from `new_id`)
pub fn gate(list: &AllowList, asks: &mut Asks, device_id: &str, device_name: &str, bot_id: &str, now_ms: u64,
            new_id: &mut dyn FnMut() -> String) -> Gate;

/// The desktop answers. Returns the device id that was allowed, or None for Not now / unknown / expired.
/// "Not now" blocks new asks from that phone for 10 minutes.
pub fn answer(list: &mut AllowList, asks: &mut Asks, ask_id: &str, allow: bool, now_ms: u64) -> Option<String>;

/// Drops expired asks; returns them so the UI card can close.
pub fn expire(asks: &mut Asks, now_ms: u64) -> Vec<Ask>;
pub fn open_asks(asks: &Asks) -> Vec<Ask>;
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `unknown_phone_asks` | empty list, `gate(.., "p1", "Ren's phone", "b1", 0)` | `AskDesktop(Ask { device_id: "p1", .. })` |
| `same_phone_waits` | then `gate(.., "p1", .., 1000)` | `StillWaiting(<same ask_id>)` |
| `yes_allows_forever` | `answer(ask, true, 2000)` then `gate(.., 9_999_999)` | `Some("p1")`, then `Allowed` |
| `not_now_blocks_10_min` | `answer(ask, false, 0)`, `gate(.., 599_999)`, `gate(.., 600_000)` | `None`, `Refused`, `AskDesktop(..)` |
| `ask_expires` | `expire(asks, 120_000)` | returns the ask; `answer(that id, true, ..)` is `None` |
| `too_many_open_asks` | 3 phones asking, 4th asks | `Refused` |
| `revoke_removes` | allow p1, revoke p1, gate | `AskDesktop(..)` |
| `cap_ten_phones` | allow 11 phones with growing `since_ms` | 10 kept; the first one gone |
| `load_broken_is_empty` | file `"{"` | `AllowList::default()` |
| `round_trip` | save/load 2 phones | equal |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S2 cargo test --lib potato::sandbox::phone_yes
```
Expected: `test result: ok. 10 passed; 0 failed`.

### Done when
- [ ] Decision is pure and covers allow, wait, refuse, expire, revoke.
- [ ] 10 tests pass.

### Do NOT
- Do not add any bridge route that can call `answer` or `allow`. Only the desktop UI (Tauri command, S4) may.
- Do not touch device pairing code. This list is separate from pairing.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `sandbox/mod.rs` uses a different test-file pattern: copy it.
2. `HashMap` import missing: add `use std::collections::HashMap;`.
3. Cap test drops the wrong phone: sort by `since_ms` ascending and remove index 0 while `len > MAX_PHONES`.

---

## Slice S3 - Phone control asks wired into the lease path

### Goal
When a phone asks to control the Sandbox, use `phone_yes::gate`: allowed phones get the lease, others create a desktop ask and get `PHONE_NEEDS_DESKTOP_YES` with the ask id.

### Read first
- `packages/app/src-tauri/src/bridge/routes/potato_screen.rs` (+ `potato_screen_tests.rs`): the phone control request and `PHONE_NEEDS_DESKTOP_YES`.
- `packages/app/src-tauri/src/potato/sandbox/lease.rs`: how a lease is granted to a holder.
- `packages/app/src-tauri/src/potato/live.rs` l.187-192: event emit style.
- `packages/app/src-tauri/src/potato/sandbox/phone_yes.rs` (S2).

### Files
- edit `packages/app/src-tauri/src/bridge/routes/potato_screen.rs`
- edit `packages/app/src-tauri/src/bridge/routes/potato_screen_tests.rs`
- edit `packages/app/src-tauri/src/potato/sandbox/live.rs` (hold `AllowList` + `Asks` in the live sandbox state; NEEDS-FACT: the struct that owns the lease at runtime)
- edit `packages/app/src-tauri/src/potato/live.rs` (event `potato://phone-control-ask` and `potato://phone-control-closed`)

### Steps
1. In the route that returns `PHONE_NEEDS_DESKTOP_YES` today, call `phone_yes::gate(...)` with the paired phone's device id and name (from the authenticated phone session; never from the request body).
2. `Allowed` -> continue into the existing lease grant.
3. `AskDesktop(ask)` -> emit `potato://phone-control-ask` with the `Ask` JSON; answer the phone `409` with body `{"code":"PHONE_NEEDS_DESKTOP_YES","askId":"<id>","message":"Waiting for a yes on your computer."}`.
4. `StillWaiting(id)` -> same `409` body with that id; no new event.
5. `Refused` -> `403` `{"code":"PHONE_CONTROL_REFUSED","message":"Your computer said not now. Try again later."}`.
6. On the existing 30 s tick, call `phone_yes::expire`; for each expired ask emit `potato://phone-control-closed {askId}`.
7. Tests.

### Code
Response bodies (exact):
```json
{"code":"PHONE_NEEDS_DESKTOP_YES","askId":"a1b2c3d4e5f6","message":"Waiting for a yes on your computer."}
{"code":"PHONE_CONTROL_REFUSED","message":"Your computer said not now. Try again later."}
```
Event payloads:
```json
potato://phone-control-ask    {"askId":"a1b2c3d4e5f6","deviceId":"<id>","deviceName":"Ren's phone","botId":"b1","atMs":0}
potato://phone-control-closed {"askId":"a1b2c3d4e5f6"}
```
If the current `PHONE_NEEDS_DESKTOP_YES` response uses a different status or shape, keep its status and add only
`askId` and `message` (the phone UI in plan 14 P3 reads `code`, `askId`, `message`).

### Tests to write (`potato_screen_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `phone_control_unknown_asks_desktop` | paired phone p1, not allowed | `409`, body `code PHONE_NEEDS_DESKTOP_YES` with an `askId`; one ask event recorded |
| `phone_control_second_request_same_ask` | repeat | same `askId`; still one event |
| `phone_control_allowed_gets_lease` | p1 on the allow list | the existing lease-granted response |
| `phone_control_refused_after_not_now` | answer false, request again | `403 PHONE_CONTROL_REFUSED` |
| `phone_device_id_not_from_body` | body says `deviceId: "p2"`, session is p1 | ask has `deviceId "p1"` |
| `unpaired_caller_rejected` | no phone session | the existing auth rejection (unchanged) |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S3 cargo test --lib bridge::routes::potato_screen
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S3 cargo test --lib potato::sandbox
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S3 cargo test --lib p2p::guard
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S3 cargo test --lib desk::guard
```
Expected: each `test result: ok.` with 0 failed.

### Done when
- [ ] A phone that was never allowed cannot get the lease; it creates one desktop ask.
- [ ] Device identity comes only from the authenticated phone session.
- [ ] Guards stay green.

### Do NOT
- Do not add a route that answers the ask. The answer is desktop-only (S4).
- Do not change the watch (view-only) path for phones.
- Do not read env or print in any p2p code (rule); this slice does not touch `p2p/`.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. The route has no access to the phone's device name: use the pairing record's name; if there is none, use `"Your phone"`.
2. Existing tests expect the old body exactly: update them to also accept the new fields; keep `code` unchanged.
3. Lease is granted before the gate runs: move the gate call above the lease call.

---

## Slice S4 - Desktop yes card and the allowed phones list

### Goal
Show a card on this PC when a phone asks for control, with Allow this phone / Not now, and a Settings list to take a yes back.

### Read first
- `packages/app/src/components/cockpit/potato/ComputerBanner.tsx` (`ComputerBanner`, `bannerDue`) and `ChoiceCard.tsx`.
- `packages/app/src/lib/potato-sandbox.ts` (`SandboxTransport`, `tauriSandboxTransport`) and `packages/app/src/lib/potato-computer.ts` (`COMPUTER_COPY`).
- `packages/app/src/components/settings/sandbox/SandboxRows.tsx` (`makeSandboxRows` l.415).
- `packages/app/src-tauri/src/commands/potato_sandbox.rs` (command style).

### Files
- edit `packages/app/src-tauri/src/commands/potato_sandbox.rs` (3 commands) + register in `packages/app/src-tauri/src/lib.rs`
- create `packages/app/src/components/cockpit/potato/PhoneControlCard.tsx`
- create `packages/app/src/components/cockpit/potato/PhoneControlCard.test.tsx`
- edit `packages/app/src/lib/potato-sandbox.ts` (transport methods + events)
- edit `packages/app/src/components/settings/sandbox/SandboxRows.tsx` (row "Phones that may control")

### Steps
1. Commands (Code). Register in `lib.rs` next to the other `potato_sandbox_*` commands.
2. `potato-sandbox.ts`: add `phoneAsks()`, `phoneAnswer(askId, allow)`, `phonesAllowed()`, `phoneRevoke(deviceId)`, `onPhoneAsk(cb)`, `onPhoneAskClosed(cb)` to `SandboxTransport` and its fake.
3. `PhoneControlCard`: mounted once at the Potato page level (and in the cockpit root if the Potato page is closed: NEEDS-FACT the root overlay slot); shows the oldest open ask.
4. Settings row with the list and `Take back` buttons.
5. Tests.

### Code
```rust
#[tauri::command] pub async fn potato_sandbox_phone_asks() -> Result<Vec<phone_yes::Ask>, String>;
#[tauri::command] pub async fn potato_sandbox_phone_answer(ask_id: String, allow: bool) -> Result<(), String>;
#[tauri::command] pub async fn potato_sandbox_phones() -> Result<Vec<phone_yes::AllowedPhone>, String>;
#[tauri::command] pub async fn potato_sandbox_phone_revoke(device_id: String) -> Result<(), String>;
```
`potato_sandbox_phone_revoke` also ends any lease that phone holds right now.

UI text (add to `COMPUTER_COPY` in `packages/app/src/lib/potato-computer.ts`):
```ts
phoneAskTitle: "Let your phone control Fish's computer?",
phoneAskBody: (phone: string) => `${phone} wants to use the mouse and keyboard on Fish's computer. You only say yes once for this phone. You can take it back in Settings.`,
phoneAllow: 'Allow this phone',
phoneNotNow: 'Not now',
phonesRowTitle: 'Phones that may control',
phonesRowEmpty: 'No phones yet. A phone asks the first time it wants control.',
phonesSince: (date: string) => `Allowed on ${date}`,
phoneTakeBack: 'Take back',
phoneTakenBack: 'Done. That phone can only watch now.',
```
(The computer is called "Fish's computer" in setup checks, facts 4a. If a bot has another name, use
`${botName}'s computer` with the open bot's name; the ask carries `botId`.)

### Tests to write (`PhoneControlCard.test.tsx` + one Settings test)
| Test | Action | Expected |
|---|---|---|
| `shows the ask` | fake emits an ask from `Ren's phone` | title `Let your phone control Fish's computer?` and body starting `Ren's phone wants to use` |
| `allow answers yes` | click `Allow this phone` | fake `phoneAnswer` called with `(askId, true)`; card closes |
| `not now answers no` | click `Not now` | `(askId, false)`; card closes |
| `closed event hides the card` | fake emits closed for that id | card gone |
| `settings lists and takes back` (in `SandboxRows.test.tsx`) | fake has 1 phone; click `Take back` | `phoneRevoke(deviceId)` called; text `Done. That phone can only watch now.` |

Rust: `commands/potato_sandbox.rs` test `phone_answer_unknown_ask_is_ok_noop` (answer with an unknown id returns `Ok(())` and allows nothing).

### Run
```
cd packages/app && npx vitest run src/components/cockpit/potato/PhoneControlCard.test.tsx src/components/settings/sandbox
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S4 cargo test --lib commands::potato_sandbox
```
Expected: vitest 0 failed; Rust `test result: ok.` 0 failed.

### Done when
- [ ] First control request from a phone shows the card on the PC; Allow lets that phone control from then on.
- [ ] Settings shows allowed phones and Take back works and ends a live lease.
- [ ] Monochrome card (no coloured buttons).

### Do NOT
- Do not make the card answerable by keyboard shortcut alone without focus (no global hotkey that says yes).
- Do not auto-allow. No "allow all phones" switch.
- Do not expose these commands on the bridge.
- Do not run the app on the primary screen.

### If it fails
1. Card does not appear when the Potato page is closed: mount it in the cockpit root overlay slot as well (NEEDS-FACT), guarded by `potatoOn`.
2. `listen` names differ: events are `potato://phone-control-ask` and `potato://phone-control-closed` (S3).
3. Revoke does not end the lease: call the lease release for holder = that phone in the command (see `lease.rs`).

---

## Slice S5 - `resource_check.rs`: required brain and Sandbox files

### Goal
A pure list of files the app needs from its bundled resources, a check function, and a clear state message when any are missing.

### Read first
- `packages/app/src-tauri/src/potato/resource_dirs.rs`: how the `potato-brain` and `potato-sandbox` folders are found in dev and release.
- `packages/app/src-tauri/tauri.conf.json`: the `bundle.resources` entries (NEEDS-FACT: exact keys).
- `packages/app/src-tauri/src/potato/sandbox/install.rs`: which guest files install reads.

### Files
- create `packages/app/src-tauri/src/potato/resource_check.rs`
- create `packages/app/src-tauri/src/potato/resource_check_tests.rs`
- edit `packages/app/src-tauri/src/potato/mod.rs`
- edit `packages/app/src-tauri/src/potato/service.rs` (`state()` adds `missing_files: Vec<String>` when non-empty; NEEDS-FACT: `PotatoState` field style)

### Steps
1. Write `resource_check.rs` (Code). Fill `REQUIRED_BRAIN` and `REQUIRED_SANDBOX` from the real file names `install.rs` and `sidecar.rs` use (NEEDS-FACT); the lists below are the minimum known from the facts file.
2. Test against the source tree (the dev layout) so a missing file breaks CI.
3. `state()` reports missing files; the UI copy for it is in Code.

### Code
```rust
use std::path::Path;

/// Paths relative to the bundled `potato-brain` folder.
pub const REQUIRED_BRAIN: &[&str] = &[
    "package.json",
    "src/pm.ts",          // or the release name from sidecar.rs script_path (NEEDS-FACT: brain.mjs?)
    "src/discord.ts",     // plan 11 D8; remove this line if plan 11 is not merged yet
];

/// Paths relative to the bundled `potato-sandbox` folder.
pub const REQUIRED_SANDBOX: &[&str] = &[
    "guest/desktop/apply-recipe.py",
    "guest/desktop/pm-sbx-start-desktop.sh",
    // NEEDS-FACT: every other guest file install.rs copies
];

#[derive(Debug, PartialEq, Eq)]
pub struct Missing { pub brain: Vec<String>, pub sandbox: Vec<String> }

impl Missing { pub fn is_empty(&self) -> bool { self.brain.is_empty() && self.sandbox.is_empty() } }

/// Pure on paths: checks each required file is a non-empty regular file. Never follows paths outside the root.
pub fn check(brain_root: &Path, sandbox_root: &Path) -> Missing {
    let f = |root: &Path, list: &[&str]| -> Vec<String> {
        list.iter()
            .filter(|rel| !rel.contains("..") && std::fs::metadata(root.join(rel)).map(|m| !m.is_file() || m.len() == 0).unwrap_or(true))
            .map(|s| s.to_string())
            .collect()
    };
    Missing { brain: f(brain_root, REQUIRED_BRAIN), sandbox: f(sandbox_root, REQUIRED_SANDBOX) }
}
```
UI text (add to `packages/app/src/lib/potato-firstrun.ts` or the state card copy; NEEDS-FACT: where `BrainStateCard` copy lives):
```
Some Potato Bot files are missing from this install. Reinstall Puppet Master to fix it.
```
(Shown in `BrainStateCard` when `missingFiles` is non-empty. The file names are shown under a `Details` toggle.)

### Tests to write (`resource_check_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `source_tree_has_all_files` | roots = `packages/potato-brain` and `packages/potato-sandbox` (from `env!("CARGO_MANIFEST_DIR")` + `../../`) | `Missing::is_empty()` |
| `empty_dir_reports_all` | two empty temp dirs | all names listed |
| `empty_file_counts_as_missing` | temp dir with a 0-byte `package.json` | `brain` contains `package.json` |
| `no_dotdot_entries` | the two constant lists | no entry contains `..` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S5 cargo test --lib potato::resource_check
```
Expected: `test result: ok. 4 passed; 0 failed`.

### Done when
- [ ] The lists match the files `install.rs` and `sidecar.rs` really use.
- [ ] A missing file shows the reinstall message instead of a silent failure.

### Do NOT
- Do not read files outside the two roots.
- Do not block app start when files are missing; only Potato Bot shows the message.
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `source_tree_has_all_files` fails on `src/discord.ts`: plan 11 D8 is not merged; remove that line (step 1 note).
2. `CARGO_MANIFEST_DIR` points to `packages/app/src-tauri`: roots are `../../potato-brain` and `../../potato-sandbox` from there.
3. Release name differs (`brain.mjs`): the list must hold the name that exists in BOTH dev and release, or keep two lists (`REQUIRED_BRAIN_DEV`, `REQUIRED_BRAIN_RELEASE`) chosen by `cfg!(debug_assertions)`.

---

## Slice S6 - `scripts/verify-potato-bundle.mjs` for built installers

### Goal
A Node script that checks a built app bundle folder contains every required brain and Sandbox file, run as a release step.

### Read first
- `packages/app/src-tauri/src/potato/resource_check.rs` (S5).
- `packages/app/src-tauri/tauri.conf.json` `bundle.resources` (NEEDS-FACT).
- The release checklist location (NEEDS-FACT: file name; plan 15 slice F1 creates `docs/release/CHECKLIST.md` if none exists).

### Files
- create `scripts/verify-potato-bundle.mjs`
- create `scripts/verify-potato-bundle.test.mjs`
- edit `docs/release/CHECKLIST.md` (add the step; created by plan 15 F1)

### Steps
1. The script reads the two lists from `resource_check.rs` (regex over the file, same trick as the D1 mirror test), so there is one source of truth.
2. It takes one argument: the folder that holds the installed or unpacked app resources (the folder that contains `potato-brain/` and `potato-sandbox/`; NEEDS-FACT: the exact resources path inside the Windows build output).
3. Exit 0 and print `OK: all Potato Bot files are in the bundle.`; exit 1 and print one line per missing file `MISSING: potato-brain/src/pm.ts`.
4. Test with `node --test`.
5. Add the release step.

### Code
```js
#!/usr/bin/env node
// Usage: node scripts/verify-potato-bundle.mjs <resources folder>
import { readFileSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const rust = readFileSync(join(here, '..', 'packages', 'app', 'src-tauri', 'src', 'potato', 'resource_check.rs'), 'utf8');

export function listFrom(text, name) {
  const start = text.indexOf(`pub const ${name}`);
  if (start < 0) throw new Error(`list ${name} not found`);
  const end = text.indexOf('];', start);
  return [...text.slice(start, end).matchAll(/^\s*"([^"]+)"/gm)].map((m) => m[1]);
}

export function missingIn(root, brain, sandbox) {
  const bad = [];
  const check = (base, rel) => {
    try { const s = statSync(join(root, base, rel)); if (!s.isFile() || s.size === 0) bad.push(`${base}/${rel}`); }
    catch { bad.push(`${base}/${rel}`); }
  };
  brain.forEach((r) => check('potato-brain', r));
  sandbox.forEach((r) => check('potato-sandbox', r));
  return bad;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const root = process.argv[2];
  if (!root) { console.log('Usage: node scripts/verify-potato-bundle.mjs <resources folder>'); process.exit(2); }
  const bad = missingIn(root, listFrom(rust, 'REQUIRED_BRAIN'), listFrom(rust, 'REQUIRED_SANDBOX'));
  if (bad.length === 0) { console.log('OK: all Potato Bot files are in the bundle.'); process.exit(0); }
  for (const b of bad) console.log(`MISSING: ${b}`);
  process.exit(1);
}
```
Release checklist line (add under the Fish licence line from plan 15 F1):
```
- [ ] Potato Bot files are in the installer: unpack or install the build, then run
      node scripts/verify-potato-bundle.mjs "<resources folder>"   -> expect "OK: all Potato Bot files are in the bundle."
```

### Tests to write (`scripts/verify-potato-bundle.test.mjs`)
| Test | Input | Expected |
|---|---|---|
| `reads lists from rust` | the real `resource_check.rs` | `REQUIRED_BRAIN` includes `package.json` |
| `source tree passes` | root = a temp folder with `potato-brain` and `potato-sandbox` copied from `packages/` (only the listed files) | `missingIn` returns `[]` |
| `missing file reported` | same but delete one file | returns `['potato-sandbox/guest/desktop/apply-recipe.py']` |

### Run
```
node --test scripts/verify-potato-bundle.test.mjs
node scripts/verify-potato-bundle.mjs packages
```
Expected: `# fail 0`; the second prints `OK: all Potato Bot files are in the bundle.` (the `packages` folder has the same `potato-brain/` and `potato-sandbox/` layout in dev).

### Done when
- [ ] One source of truth: the script reads `resource_check.rs`.
- [ ] The release checklist has the step.

### Do NOT
- Do not build the installer in this slice (that is a release step for a person).
- Do not add the script to `package.json` scripts unless the repo has a `scripts` convention there (NEEDS-FACT).

### If it fails
1. `import.meta.url` vs `argv[1]` mismatch on Windows: compare with `pathToFileURL(process.argv[1]).href === import.meta.url`.
2. Regex misses entries with comments after them: the regex reads the first quoted string on each line; comments after are fine.
3. `docs/release/CHECKLIST.md` does not exist yet: plan 15 F1 creates it; merge F1 first or create the file with the one line and a heading `# Release checklist`.

---

## Slice S7 - Live DeepSeek tests (opt-in)

### Goal
Two opt-in live tests a person runs with their own key: the Rust key check through the OS credential store, and one real brain turn.

### Read first
- `packages/app/src-tauri/src/potato/live.rs` l.154 (`secret`), l.255 (`check_key`), l.266 (`ask_model`).
- `packages/app/src-tauri/src/secrets.rs` l.152 (`read_secret`).
- `packages/potato-brain/test/real-brain.test.ts` (what it already covers; reuse its helpers).
- `packages/potato-brain/src/core/provider.ts` (`makeProvider`), `src/config.ts` (env names).

### Files
- create `packages/app/src-tauri/src/potato/live_deepseek_tests.rs` (declared `#[cfg(test)]` from `potato/mod.rs`)
- create `packages/potato-brain/test/live-deepseek.test.ts`
- edit `docs/features/potato-bot.md` (a short "Live test" paragraph with the commands)

### Steps
1. Rust test marked `#[ignore]`; it reads `deepseek_api_key` with `read_secret`, skips (passes with a printed note) if unset, calls `check_key`, asserts `Ok`.
2. Brain test that is skipped unless `PM_LIVE_DEEPSEEK=1`; key from env `DEEPSEEK_API_KEY` (the person sets it in their shell for this run only).
3. Document both.

### Code
Rust:
```rust
/// Run by a person: cargo test --lib potato::live_deepseek -- --ignored
#[test]
#[ignore = "live: uses the DeepSeek key in this PC's credential store and the network"]
fn live_check_key_reads_credential_store() {
    let Some(key) = crate::secrets::read_secret("deepseek_api_key") else {
        eprintln!("skip: no deepseek_api_key in the credential store");
        return;
    };
    let r = crate::potato::live::check_key_for_test(&key); // thin pub(crate) wrapper over check_key's request
    assert!(r.is_ok(), "DeepSeek refused the key: {}", r.err().unwrap_or_default());
}
```
`check_key_for_test` is a `#[cfg(test)] pub(crate) fn` in `live.rs` that runs the same request as `check_key` without
needing a running app (NEEDS-FACT: whether `check_key` needs `&self`). Its error text must not include the key.

Brain (`packages/potato-brain/test/live-deepseek.test.ts`):
```ts
import { test } from "node:test";
import assert from "node:assert/strict";

const on = process.env.PM_LIVE_DEEPSEEK === "1" && (process.env.DEEPSEEK_API_KEY ?? "") !== "";

test("live: one DeepSeek turn says potato", { skip: !on && "set PM_LIVE_DEEPSEEK=1 and DEEPSEEK_API_KEY" }, async () => {
  const { makeProvider } = await import("../src/core/provider.ts");
  const provider = makeProvider(); // reads DEEPSEEK_API_KEY via config (NEEDS-FACT: makeProvider's options)
  const out = await provider.complete({  // NEEDS-FACT: the provider method name and request shape
    messages: [{ role: "user", content: "Reply with the single word: potato" }],
    maxTokens: 10,
  });
  assert.match(String(out.text ?? ""), /potato/i);
  assert.ok((out.usage?.outputTokens ?? 0) > 0);
});
```
Docs paragraph (`docs/features/potato-bot.md`, under tests):
```
Live test (needs your own DeepSeek key, costs well under one cent):
- Key check through the credential store:
  cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-live cargo test --lib potato::live_deepseek -- --ignored
- One real brain turn:
  cd packages/potato-brain && PM_LIVE_DEEPSEEK=1 DEEPSEEK_API_KEY=<your key> node --test test/live-deepseek.test.ts
Never paste the key into a file or a chat. Close the terminal after the run.
```

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `live_check_key_reads_credential_store` (ignored by default) | key in the OS store | `ok`; with no key: passes with the skip note |
| `live: one DeepSeek turn says potato` (skipped by default) | `PM_LIVE_DEEPSEEK=1` + key | reply matches `/potato/i`; output tokens > 0 |

### Run
Default (no key, as CI and builders run it):
```
cd packages/potato-brain && node --test test/live-deepseek.test.ts
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S7 cargo test --lib potato::live_deepseek
```
Expected: brain `# skipped 1`, `# fail 0`; Rust `test result: ok. 0 passed; 0 failed; 1 ignored`.

### Done when
- [ ] Both tests are skipped/ignored by default and need no key to keep CI green.
- [ ] The doc paragraph has the exact commands.
- [ ] A person ran both live once and pasted the pass lines (not the key) into the PR.

### Do NOT
- Do not run the live tests yourself with a key you find anywhere. Only the person runs them.
- Do not print the key, its length, or its last characters in test output.
- Do not add the live test to any default test run.

### If it fails
1. `makeProvider` signature differs: copy how `test/real-brain.test.ts` builds a provider.
2. The Rust test cannot reach `check_key` without an app: write `check_key_for_test` as a free function calling the same HTTP code as `ask_model`.
3. DeepSeek returns 402 (no balance): the test fails with that status; the person tops up; not a code bug.

---

## Slice S8 - Per-bot work place: Local or Sandbox

### Goal
Each bot has a setting "Where this bot works": On this PC (Local, today) or In Fish's computer (Sandbox); outside turns are always Sandbox whatever the setting.

### Read first
- `packages/app/src-tauri/src/potato/bot_settings.rs` and `service.rs` l.514-600 (`set_talk_brain`, `set_tools`).
- `packages/app/src-tauri/src/potato/helpers.rs` (`Folder`, `Started`, `pick_folder`, `folder_problem` l.175).
- `packages/app/src-tauri/src/potato/outside/policy.rs` (plan 11 D2).
- `packages/app/src/components/cockpit/potato/AdvancedTab.tsx`, `AdvancedTools.tsx`; `packages/app/src/lib/potato-botsettings.ts`.

### Files
- edit `packages/app/src-tauri/src/potato/bot_settings.rs` (field + setter)
- edit `packages/app/src-tauri/src/commands/potato_bot_settings.rs` (command `potato_bot_set_work_place`) + register in `lib.rs`
- create `packages/app/src/components/cockpit/potato/AdvancedWorkPlace.tsx`
- create `packages/app/src/components/cockpit/potato/AdvancedWorkPlace.test.tsx`
- edit `packages/app/src/lib/potato-botsettings.ts`

### Steps
1. Add `work_place: WorkPlace` to the bot settings struct with `#[serde(default)]` (default `Local`, so old files load).
2. Add `Service::set_work_place(bot, WorkPlace) -> Result<(), String>`: refuses `Sandbox` with `POTATO_OUTSIDE_NO_SANDBOX: Set up Fish's computer first.` when the Sandbox is not installed or its Labs flag is off.
3. Add `pub fn effective_place(setting: WorkPlace, scope: ToolScope) -> WorkPlace` (Code): `SandboxOnly` always gives `Sandbox`.
4. Command + TS transport method `setWorkPlace(botId, place)`.
5. UI section in the Advanced tab (Code).
6. Tests. (Helpers using the place come in S9/S10.)

### Code
```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPlace { #[default] Local, Sandbox }

pub fn effective_place(setting: WorkPlace, scope: crate::potato::outside::policy::ToolScope) -> WorkPlace {
    match scope {
        crate::potato::outside::policy::ToolScope::SandboxOnly => WorkPlace::Sandbox,
        crate::potato::outside::policy::ToolScope::AsToday => setting,
    }
}
```
UI text:
```ts
export const WORK_PLACE_COPY = {
  title: 'Where this bot works',
  local: 'On this PC',
  localHelp: 'Helpers work in your folders on this PC. You say yes before each start.',
  sandbox: "In Fish's computer",
  sandboxHelp: "Helpers work on a copy of your folder inside Fish's computer. You get the work back as a branch to review.",
  sandboxOff: "Set up Fish's computer first to use this.",
  outsideNote: 'Messages from Discord or Telegram always run in Fish\'s computer.',
} as const;
```
Component: two radio rows; the Sandbox row is disabled with `sandboxOff` when `sandboxReady` is false; `outsideNote`
is always shown under the radios in secondary label colour.

### Tests to write
| Test | Input | Expected |
|---|---|---|
| `work_place_defaults_local` (Rust) | old settings JSON without the field | `WorkPlace::Local` |
| `effective_place_outside_is_sandbox` (Rust) | `(Local, SandboxOnly)` | `Sandbox` |
| `effective_place_app_keeps_setting` (Rust) | `(Local, AsToday)`, `(Sandbox, AsToday)` | `Local`, `Sandbox` |
| `set_sandbox_refused_without_sandbox` (Rust, service_tests) | fake host no sandbox | `Err` starting `POTATO_OUTSIDE_NO_SANDBOX:` |
| `shows both choices` (TS) | render | `On this PC`, `In Fish's computer` |
| `sandbox disabled when not ready` (TS) | `sandboxReady false` | Sandbox radio disabled; text `Set up Fish's computer first to use this.` |
| `picking sandbox saves` (TS) | click Sandbox | transport `setWorkPlace('b1', 'sandbox')` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S8 cargo test --lib potato
cd packages/app && npx vitest run src/components/cockpit/potato/AdvancedWorkPlace.test.tsx
```
Expected: Rust 0 failed; vitest `Tests  3 passed (3)`.

### Done when
- [ ] Setting saved per bot, default Local, old files still load.
- [ ] Outside turns resolve to Sandbox regardless.

### Do NOT
- Do not change helper start behaviour yet (S9/S10 do).
- Do not add a Labs flag (this lives under `potato_sandbox_enabled`).
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Bot settings are stored in several files: add the field where `set_tools` stores tool switches.
2. `AdvancedTab` has a fixed section list: add the new section after `AdvancedTools`.
3. Plan 11 D2 not merged (`ToolScope` missing): merge D2 first; this slice depends on it.

---

## Slice S9 - Copy-in: a Git folder into the Sandbox

### Goal
When a Sandbox-place bot starts a helper on a folder, copy that folder's Git history and uncommitted changes into Fish's computer on a new branch.

### Read first
- `packages/app/src-tauri/src/potato/sandbox/mod.rs` `trait Sandbox` (l.140): the methods to run a command in the guest and to copy a file in (NEEDS-FACT: names, for example `exec` and `push_file`).
- `packages/app/src-tauri/src/potato/sandbox/wsl.rs`: how commands run in the distro (user, working dir, no shell string building).
- `packages/app/src-tauri/src/potato/helpers.rs` (`Folder`, `Started`, `folder_problem`).
- `docs/features/potato-sandbox.md` l.132 (no sign-in copying; unchanged).

### Files
- create `packages/app/src-tauri/src/potato/sandbox/copy_in.rs`
- create `packages/app/src-tauri/src/potato/sandbox/copy_in_tests.rs`
- edit `packages/app/src-tauri/src/potato/sandbox/mod.rs` (declare)
- edit `packages/app/src-tauri/src/potato/helpers.rs` (Sandbox branch of the start path calls `copy_in::run`)

### Steps
1. `copy_in.rs` (Code): a pure plan builder (list of host commands and guest commands as argument vectors) plus a runner that takes `&dyn HostGit` and `&dyn Sandbox`.
2. Host side uses `git` with argument vectors only (no shell): `git -C <folder> rev-parse --is-inside-work-tree`, `git -C <folder> bundle create <tmp>/in.bundle HEAD`, `git -C <folder> diff HEAD --binary` to `<tmp>/dirty.patch`.
3. Size check: bundle + patch <= 500 MB, else refuse.
4. Guest side, in `/home/<sandbox user>/work/<slug>`: `git clone <bundle> <slug>`, `git -C <slug> checkout -b <branch>`, `git -C <slug> apply --whitelist... ` (plain `git apply dirty.patch`), `git -C <slug> add -A`, `git -C <slug> -c user.name=Fish -c user.email=fish@localhost commit -m "Start: copy from your PC" --allow-empty`.
5. Return `CopyIn { slug, branch, base_commit, guest_path }` and store it on the helper record (for S10).
6. Tests with fakes.

### Code
```rust
pub const MAX_COPY_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyIn { pub slug: String, pub branch: String, pub base_commit: String, pub guest_path: String, pub host_folder: String }

#[derive(Debug, PartialEq, Eq)]
pub enum CopyError { NotGit, TooBig(u64), HostGit(String), Guest(String), SandboxOff }

impl CopyError {
    pub fn to_wire(&self) -> String {
        match self {
            CopyError::NotGit => "POTATO_COPY_NOT_GIT: Fish can only copy a folder that uses Git. Pick a Git folder, or set this bot to work on this PC.".into(),
            CopyError::TooBig(_) => "POTATO_COPY_TOO_BIG: This folder is over 500 MB in Git. Pick a smaller one.".into(),
            CopyError::HostGit(_) => "POTATO_COPY_GIT: Git could not read this folder. Check that it opens in Git.".into(),
            CopyError::Guest(_) => "POTATO_COPY_SANDBOX: Fish's computer could not make the copy. Try again.".into(),
            CopyError::SandboxOff => "POTATO_OUTSIDE_NO_SANDBOX: Set up Fish's computer first.".into(),
        }
    }
}

/// "potato/<bot>-<yyyymmdd-hhmm>"; bot part is [a-z0-9-], max 24 chars.
pub fn branch_name(bot_name: &str, local_time: (i32, u32, u32, u32, u32)) -> String;
/// Folder name -> [a-z0-9-], max 40 chars, never empty ("work" fallback).
pub fn slug(folder_name: &str) -> String;

pub trait HostGit {
    /// Runs `git` with these args (no shell). Returns (exit code, stdout bytes, stderr text).
    fn git(&self, args: &[&str]) -> Result<(i32, Vec<u8>, String), String>;
}

pub fn run(host: &dyn HostGit, sandbox: &dyn super::Sandbox, folder: &std::path::Path, bot_name: &str,
           now_local: (i32, u32, u32, u32, u32), tmp: &std::path::Path) -> Result<CopyIn, CopyError>;
```
Untracked files are not copied (Git bundle + diff only include tracked files). The Start card in S11 says so.

### Tests to write (`copy_in_tests.rs`, `FakeHostGit` records args; `FakeSandbox` from `sandbox/mod.rs` l.582 records guest commands)
| Test | Input | Expected |
|---|---|---|
| `not_git_refused` | `rev-parse` exit 128 | `Err(NotGit)`, no guest calls |
| `too_big_refused` | bundle file size 600 MB (fake reports size) | `Err(TooBig(..))` |
| `branch_name_shape` | `("Fish!", (2026,11,4,10,12))` | `"potato/fish-20261104-1012"` |
| `slug_cleans` | `"My App (v2)"` | `"my-app-v2"` |
| `slug_never_empty` | `"***"` | `"work"` |
| `host_git_uses_arg_vectors` | normal run | first host call args `["-C", <folder>, "rev-parse", "--is-inside-work-tree"]` |
| `guest_steps_in_order` | normal run | guest calls: clone, checkout -b, apply, add -A, commit |
| `clean_tree_skips_apply` | `diff HEAD` empty | no `apply` call; commit uses `--allow-empty` |
| `error_text_plain` | each `CopyError` | starts with `POTATO_` and has `: ` |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S9 cargo test --lib potato::sandbox::copy_in
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S9 cargo test --lib potato::helpers
```
Expected: `test result: ok. 9 passed; 0 failed`; helpers 0 failed.

### Done when
- [ ] A Sandbox-place helper start makes a branch copy inside Fish's computer; the PC folder is not changed.
- [ ] Non-Git and over-size folders are refused with the exact text.

### Do NOT
- Do not build shell command strings. Argument vectors only, on host and in the guest.
- Do not copy credentials, `.env` files or sign-ins in (doc l.132: no sign-in copying).
- Do not write into the PC folder (bundle and patch go to the app temp folder, deleted after).
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. `trait Sandbox` has no file push: copy with the guest reading the Windows path through `/mnt/<drive>/...` only if `wsl.rs` already does this for install; else add a `push_file(host, guest)` method to the trait with the WSL impl and the fake (NEEDS-FACT).
2. Windows paths with spaces break the guest clone: pass paths as separate args; never join into one string.
3. `git apply` fails on CRLF: add `--ignore-whitespace` only if the test proves it; otherwise report `Guest(..)`.

---

## Slice S10 - Bring-back: the Sandbox branch into the PC repo, opened in Review Desk

### Goal
When a Sandbox-place helper finishes, bring its branch back into the PC repo as a new branch (never checked out) and open it in Review Desk.

### Read first
- `packages/app/src-tauri/src/potato/sandbox/copy_in.rs` (S9).
- `packages/app/src-tauri/src/potato/helpers.rs` (`Running`, `HelperState`, helper watch: how "finished" is known).
- Review Desk: the command or route that opens a branch for review (NEEDS-FACT: name and module; the MCP tool list shows a `review_worktree` tool, which suggests a Review Desk entry point exists).

### Files
- create `packages/app/src-tauri/src/potato/sandbox/bring_back.rs`
- create `packages/app/src-tauri/src/potato/sandbox/bring_back_tests.rs`
- edit `packages/app/src-tauri/src/potato/sandbox/mod.rs`
- edit `packages/app/src-tauri/src/potato/helpers.rs` (on finish of a Sandbox helper: call `bring_back::run`, then post the S11 card)

### Steps
1. Guest: `git -C <slug> bundle create /tmp/out.bundle <base_commit>..<branch>`; if the range is empty, report `NoChanges`.
2. Copy `out.bundle` to the app temp folder (pull from guest; NEEDS-FACT method).
3. Host: `git -C <folder> bundle verify <bundle>`; refuse on failure.
4. Host: pick a free branch name: `<branch>`, else `<branch>-2` ... `-9` (check with `git -C <folder> rev-parse --verify --quiet refs/heads/<name>`).
5. Host: `git -C <folder> fetch <bundle> <branch>:refs/heads/<free name>`. No checkout, no merge.
6. Open Review Desk on `<free name>` (NEEDS-FACT call).
7. Delete the temp bundle.

### Code
```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroughtBack { pub branch: String, pub commits: u32, pub host_folder: String }

#[derive(Debug, PartialEq, Eq)]
pub enum BackError { NoChanges, BadBundle, NoFreeName, BadBranchName, HostGit(String), Guest(String) }

/// Only names made by `copy_in::branch_name` pass: ^potato/[a-z0-9-]{1,24}-\d{8}-\d{4}(-[2-9])?$
pub fn valid_branch(name: &str) -> bool;

pub fn free_name(host: &dyn HostGit, folder: &Path, wanted: &str) -> Result<String, BackError>;

pub fn run(host: &dyn HostGit, sandbox: &dyn Sandbox, c: &CopyIn, tmp: &Path) -> Result<BroughtBack, BackError>;

pub trait ReviewDesk { fn open_branch(&self, folder: &Path, branch: &str) -> Result<(), String>; }
```
User text (errors):
```
POTATO_BACK_NONE: Fish did not change anything, so there is nothing to bring back.
POTATO_BACK_BAD: The work from Fish's computer did not pass a safety check, so it was not added.
POTATO_BACK_NAME: There are already 9 branches with this name. Delete some, then try again.
```

### Tests to write (`bring_back_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `valid_branch_accepts_ours` | `"potato/fish-20261104-1012"`, `"...-1012-2"` | `true`, `true` |
| `valid_branch_rejects_others` | `"main"`, `"potato/../x"`, `"potato/fish-20261104-1012;rm"` | all `false` |
| `no_changes` | guest bundle range empty | `Err(NoChanges)`, no host fetch |
| `bad_bundle_refused` | `bundle verify` exit 1 | `Err(BadBundle)`, no fetch |
| `free_name_adds_suffix` | `<branch>` exists, `-2` free | `"<branch>-2"` |
| `no_free_name` | all up to `-9` exist | `Err(NoFreeName)` |
| `fetch_never_checks_out` | normal run | host args include `fetch` and `<branch>:refs/heads/<name>`; no `checkout`, `merge`, `reset`, `switch` |
| `opens_review_desk` | normal run with fake `ReviewDesk` | `open_branch(folder, name)` called once |
| `temp_bundle_deleted` | normal run | temp file gone |

### Run
```
cd packages/app/src-tauri && CARGO_TARGET_DIR=D:/pm-night/target-S10 cargo test --lib potato::sandbox::bring_back
```
Expected: `test result: ok. 9 passed; 0 failed`.

### Done when
- [ ] Work comes back as a new `potato/...` branch in the PC repo; the person's current branch and files are untouched.
- [ ] Review Desk opens on that branch.

### Do NOT
- Do not check out, merge, rebase, reset or push. Fetch into a new branch only.
- Do not accept a branch name the guest made up; only `valid_branch` names.
- Do not run Git hooks from the bundle (fetch does not run them; do not add a checkout that would).
- Do not use the default `CARGO_TARGET_DIR`.

### If it fails
1. Review Desk has no "open branch" entry: make `ReviewDesk::open_branch` emit a `cockpit://view` event to the Review Desk view with `{folder, branch}` (pattern from facts 10c), and note it in the PR as `DECISION-OPEN (default: emit the view event)`.
2. `git fetch` from a bundle path with spaces fails: pass the path as one argument (it is), and check the temp folder path has no quotes added.
3. Guest pull method missing: same fix as S9 failure 1, with `pull_file(guest, host)`.

---

## Slice S11 - Thread cards for copy-in and bring-back

### Goal
Show the person what happens: a Start card that says a copy will be made, and a finish card with the branch and an Open in Review Desk button.

### Read first
- `packages/app/src/components/cockpit/potato/HelperCard.tsx`, `ChoiceCard.tsx`.
- `packages/app/src/lib/potato.ts` (message kinds for cards; NEEDS-FACT how a helper card is represented in `PotatoMessage`).

### Files
- edit `packages/app/src/components/cockpit/potato/HelperCard.tsx`
- create `packages/app/src/components/cockpit/potato/BroughtBackCard.tsx`
- create `packages/app/src/components/cockpit/potato/BroughtBackCard.test.tsx`
- edit `packages/app/src/lib/potato.ts` (card type + `openReview(folder, branch)` transport method)

### Steps
1. `HelperCard`: when the bot's work place is Sandbox, the Start card shows `copyNote` under the folder name.
2. `BroughtBackCard` (Code text).
3. Transport method `openReview` -> Tauri command from S10 (`potato_review_open(folder, branch)`, NEEDS-FACT name if Review Desk already has one).
4. Tests.

### Code
```ts
export const SANDBOX_WORK_COPY = {
  copyNote: "Fish will work on a copy inside its own computer. Your folder stays as it is. Files Git does not track are not copied.",
  doneTitle: "Fish finished in its computer",
  doneBody: (branch: string, commits: number) => `The work is on a new branch, ${branch} (${commits} ${commits === 1 ? 'change' : 'changes'}). Nothing in your folder changed until you review it.`,
  openReview: 'Open in Review Desk',
  notNow: 'Not now',
  noChanges: 'Fish did not change anything.',
} as const;
```

### Tests to write (`BroughtBackCard.test.tsx` + one `HelperCard` test)
| Test | Input | Expected |
|---|---|---|
| `shows branch and count` | branch `potato/fish-20261104-1012`, 3 commits | text `The work is on a new branch, potato/fish-20261104-1012 (3 changes).` prefix |
| `one change is singular` | 1 commit | `(1 change)` |
| `open review calls transport` | click `Open in Review Desk` | `openReview(folder, branch)` |
| `start card warns about copy` (HelperCard) | bot place sandbox | text starting `Fish will work on a copy inside its own computer.` |

### Run
```
cd packages/app && npx vitest run src/components/cockpit/potato/BroughtBackCard.test.tsx src/components/cockpit/potato/HelperCard.test.tsx
cd packages/app && npx vitest run src/components/cockpit/potato
```
Expected: 0 failed.

### Done when
- [ ] The person sees the copy note before start and the branch card after.
- [ ] Monochrome; preview-checked off the primary screen.

### Do NOT
- Do not add a "Merge" button. Merging happens in Review Desk only.
- Do not run the app on the primary screen.

### If it fails
1. `HelperCard.test.tsx` does not exist: create it with only the new test.
2. The card type union in `lib/potato.ts` is closed: add `'brought_back'` to it and its Rust twin (NEEDS-FACT).
3. Plural text mismatch: use exactly the `doneBody` function.

---

## Security

| Threat | What can happen | Defence | Test |
|---|---|---|---|
| A paired phone (or a thief with it) takes control of Fish's computer without the owner knowing | Uses the Sandbox's browser sessions | First control needs a yes on this PC; device id from the phone session only; Not now blocks 10 min; max 3 open asks; Take back ends live lease | `unknown_phone_asks`, `not_now_blocks_10_min`, `too_many_open_asks` (S2); `phone_device_id_not_from_body` (S3); settings take-back test (S4) |
| The phone or an agent answers its own ask | Self-approval | No bridge route for `answer`; Tauri command only; pairing has no route from agents/MCP/phone | S3 `Do NOT`; route tests prove only the ask exists (`phone_control_unknown_asks_desktop`) |
| A broken install runs without guest files | Silent failure, unclear state | `resource_check` + release script | `source_tree_has_all_files` (S5), `missing file reported` (S6) |
| Live test leaks the key | Key theft | Key only from the OS store or the person's own shell env; never printed; tests skipped by default | S7 `Do NOT`; default runs show skipped/ignored |
| Copy-in leaks secrets into the Sandbox | Outside users (Discord) reach them through Fish | Only Git-tracked files; no sign-ins; outside turns can use a copied folder only inside the Sandbox | `guest_steps_in_order`, `clean_tree_skips_apply` (S9) |
| Bring-back injects code into the PC | Hooks run, branch overwrites, current work changed | `bundle verify`; fetch into a NEW branch only; strict branch-name check; no checkout/merge | `bad_bundle_refused`, `valid_branch_rejects_others`, `fetch_never_checks_out` (S10) |
| Command injection through folder or bot names | Run commands on the PC | Argument vectors only; `slug` and `branch_name` keep `[a-z0-9-]` | `host_git_uses_arg_vectors`, `slug_cleans`, `branch_name_shape` (S9) |

## What cannot be verified without real hardware or a second computer

- Phone control end to end (a real paired phone on another network, the yes card on the PC, then mouse and keyboard
  on the Sandbox screen) needs a phone and the WSL 2 Sandbox running.
- The installer check on a real Windows installer (`.msi`/`.exe`) needs a release build; S6 runs on the dev layout.
- Live DeepSeek tests need the person's key and network; cost and model output are real.
- Copy-in and bring-back against a real WSL 2 distro (file copy speed, Git in the guest, CRLF behaviour) need the
  installed Sandbox; tests use `FakeSandbox`.
- Review Desk opening on the fetched branch needs the full app running (off the primary screen).

## Open facts

- NEEDS-FACT: where `PHONE_NEEDS_DESKTOP_YES` is returned today and its status/body shape - read `packages/app/src-tauri/src/bridge/routes/potato_screen.rs` and `potato_screen_tests.rs`.
- NEEDS-FACT: the paired phone identity type (device id, name) available in a phone request - read the phone pairing module (search `packages/app/src-tauri/src` for `pair` and `device_id`).
- NEEDS-FACT: the runtime struct that owns the control lease - read `packages/app/src-tauri/src/potato/sandbox/lease.rs` and `packages/app/src-tauri/src/potato/sandbox/live.rs`.
- NEEDS-FACT: the cockpit root overlay slot for cards shown outside the Potato page - read `packages/app/src/CockpitApp.tsx` l.500-540.
- NEEDS-FACT: the `bundle.resources` keys and the Windows build output resources path - read `packages/app/src-tauri/tauri.conf.json` and `packages/app/src-tauri/src/potato/resource_dirs.rs`.
- NEEDS-FACT: the full list of guest files `install.rs` copies and the brain script name in release (`brain.mjs` or `src/pm.ts`) - read `packages/app/src-tauri/src/potato/sandbox/install.rs` and `packages/app/src-tauri/src/potato/sidecar.rs` l.110-140.
- NEEDS-FACT: where `BrainStateCard` copy lives and the `PotatoState` field style - read `packages/app/src/components/cockpit/potato/BrainStateCard.tsx` and `packages/app/src-tauri/src/potato/service.rs` l.160-210.
- NEEDS-FACT: whether a docs test exists that checks feature docs - search `packages/app/src` for `docs/features`.
- NEEDS-FACT: whether `check_key` needs `&self` - read `packages/app/src-tauri/src/potato/live.rs` l.250-290.
- NEEDS-FACT: `makeProvider` options and the provider call method - read `packages/potato-brain/src/core/provider.ts`, `src/core/openai.ts` and `test/real-brain.test.ts`.
- NEEDS-FACT: `trait Sandbox` methods for running a guest command and copying files in/out - read `packages/app/src-tauri/src/potato/sandbox/mod.rs` l.140-210 and `wsl.rs`.
- NEEDS-FACT: how a helper's finish is detected and how helper cards are represented in messages - read `packages/app/src-tauri/src/potato/helpers.rs` and `packages/app/src/lib/potato.ts`.
- NEEDS-FACT: the Review Desk entry point to open a branch - search `packages/app/src` and `packages/app/src-tauri/src` for `Review Desk` / `review_worktree`.
- NEEDS-FACT: whether the repo has a `scripts` convention in the root `package.json` - read `package.json`.
