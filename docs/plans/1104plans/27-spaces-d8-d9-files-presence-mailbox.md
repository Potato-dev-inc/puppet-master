# Spaces D-8 and D-9: files, presence, mailbox

Read `20-spaces-overview.md` and the shared rules R1-R7 at the top of
`21-spaces-d0-d1-transport-spike-and-secure-core.md`. R1-R7 apply in full to every slice here.

Writer's sources: `00-FORMAT.md` and `_facts/spaces-facts.md` only (facts cited as `facts §N`).

Slices: D8.1, D8.2, D8.3, D8.4, D9.1, D9.2 (6 slices). D9.1 and D9.2 are built only after the founder changes
decision 9 (offline mailbox) to yes; default is off.

## File rules (decided)

| Rule | Value |
|---|---|
| Max size | 25 MB (`FILE_MAX_BYTES`) |
| Transport | Our own chunk stream on `pm/spaces/1` (no `iroh-blobs`, plan decision P-5) |
| Chunk size | 64 KiB (`FILE_CHUNK`); at most 400 chunks |
| Integrity | The offer lists the SHA-256 of every chunk and of the whole file; each chunk is checked on arrival |
| When it downloads | Only on click (`Get file`), never automatically |
| Who serves | Any member who has the whole file; the author first, then other live links |
| Where it is kept | `app_data_dir()/spaces/<space_hex>/files/<sha256_hex>` (counts toward the 200 MB rule; cached files are the first thing pruned) |
| Opening | Never opened or run by the app. Only `Save to Downloads`. |
| Names | Cleaned: no path separators, no control chars, max 120 chars; empty becomes `file` |
| Concurrency | At most 2 uploads and 4 downloads at a time per PC |

## Presence rules (decided)

| State | Meaning |
|---|---|
| Online | The app is open and I used it in the last 10 minutes |
| Away | The app is open, no input in the app for 10 minutes |
| Offline | No report about me for 90 seconds |
| Hidden | Setting "Show when I'm online" off: I send nothing; others see me as Offline |

Presence is ephemeral (never stored in the log). Each PC tells its live links its own state every 30 s and on change,
plus what it heard from its other links (seen within 90 s), so members who are not directly linked still see dots.
"Agent working" (T-1) is not in v1: `DECISION-OPEN (default: not shown)`.

## Words on screen (exact)

| Place | Text |
|---|---|
| Composer attach button | `Attach file` |
| Attach too big | `Files can be up to 25 MB.` |
| File card | `{name}` · `{size}` (for example `2.4 MB`) |
| File card buttons | `Get file` (not here yet), `Save to Downloads` (here) |
| Getting | `Getting file… {percent}%` |
| No one has it | `No one online has this file right now. Try again later.` |
| Broken file | `This file didn't check out and was thrown away.` |
| Saved | `Saved to Downloads as {name}.` |
| Warning under every file card | `Files from other people can be harmful. Only open files you expect.` |
| Presence setting (Space info) | checkbox `Show when I'm online` (all spaces on this computer) |
| Mailbox (owner, Space info) | `Keep messages for 7 days on a mailbox` + help `A mailbox holds sealed messages for people who are offline. It cannot read them.` |
| Mailbox field | `Mailbox address`, placeholder `pms-mailbox1:…` |
| Mailbox on | `Mailbox on. Sealed messages wait there for 7 days.` |

---

## D8.1 Files: offer entry and chunk transfer (backend)
Needs merged first: D5.5 (plan 25).

### Goal
Send a file as a `FileOffer` entry, serve it in 64 KiB checked chunks to members who ask, download on request from
the author or any member who has it, and save a copy to Downloads.

### Read first
- `spaces/log.rs` (`KIND_FILE`, `Body::File`), `spaces/ingest.rs` (`apply_body` for kind 6, `file_json`),
  `spaces/service.rs` (`serve_first_frame`, `send`), `spaces/store.rs` (`files` table), `spaces/retention.rs`.
- How the app finds the Downloads folder (Open facts F-1).

### Files
- create `packages/app/src-tauri/src/spaces/files.rs`
- create `packages/app/src-tauri/src/spaces/files_tests.rs`
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

### Steps
1. `files.rs`: offer body encode/decode, `clean_file_name`, `hash_chunks`, transfer frames, `serve_file`, `fetch_file`.
2. `ingest.rs` behaviour for kind 6 is in `apply_body` already as "File -> messages row"; in this slice
   `service.rs` sets `file_json` and inserts a `files` row (`have = 0`) when a FileOffer is stored (the service hook,
   not ingest, so `ingest.rs` stays unchanged).
3. `service.rs`: `serve_first_frame` dispatches `FILE_WANT` (0x20) to `files::serve_file` when the peer is a member
   device of that space; methods `offer_file`, `get_file`, `save_file`.
4. Commands; register.
5. Tests on `memnet`.

### Code
FileOffer body (inside the sealed entry, after the body version byte):

| Field | Bytes |
|---|---|
| name | `str`, cleaned, ≤ 120 chars |
| size | `u64`, 1 ..= 26 214 400 |
| sha256 of the whole file | `[32]` |
| chunk size | `u32` = 65 536 |
| chunk count n | `u16` = ceil(size / 65 536), ≤ 400 |
| chunk hashes | n x `[32]` SHA-256 of each chunk |
| mime | `str` ≤ 64, informational only, never used to open anything |

Transfer frames (a new stream whose first frame is `FileWant`):

| Type | Name | Direction | Body |
|---|---|---|---|
| `0x20` | FileWant | getter -> server | `[16]` space, `[32]` file sha, `u16` first chunk |
| `0x21` | FileChunk | server -> getter | `u16` index, `blob` data (≤ 65 536) |
| `0x22` | FileNone | server -> getter | nothing (I don't have it, or I'm busy) |
| `0x23` | FileDone | server -> getter | nothing |

```rust
//! Files in Spaces: offer in the log, bytes on demand, every chunk checked.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use super::ids::SpaceId;
use super::limits::{FILE_CHUNK, FILE_MAX_BYTES};
use super::net::SpaceStream;

pub const FILE_WANT: u8 = 0x20;
pub const FILE_CHUNK_FRAME: u8 = 0x21;
pub const FILE_NONE: u8 = 0x22;
pub const FILE_DONE: u8 = 0x23;
pub const MAX_UPLOADS: usize = 2;
pub const MAX_DOWNLOADS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOffer { pub name: String, pub size: u64, pub sha: [u8; 32], pub chunks: Vec<[u8; 32]>, pub mime: String }

impl FileOffer {
    pub fn to_body(&self) -> Vec<u8>;
    pub fn from_body(b: &[u8]) -> Result<Self, &'static str>;
    // checks: size in 1..=FILE_MAX_BYTES; chunk size field == 65 536; n == ceil(size/65 536); name == clean_file_name(name)
}

/// Strip folders (both slash kinds), control and bidi chars, leading dots; cap 120 chars; empty -> "file".
pub fn clean_file_name(raw: &str) -> String;

/// Read a local file (≤ 25 MB) and build its offer. Errors: "Files can be up to 25 MB." / "Can't read that file."
pub fn offer_from_path(path: &Path) -> Result<(FileOffer, Vec<u8>), String>;

pub fn cache_path(space_dir: &Path, sha: &[u8; 32]) -> PathBuf;     // space_dir/files/<hex>

/// Server side: read the cached file and send chunks from `first`, then FileDone. Sends FileNone if not cached.
pub async fn serve_file(s: &mut SpaceStream, space_dir: &Path, have: bool, sha: &[u8; 32], first: u16) -> Result<(), ()>;

#[derive(Debug, PartialEq, Eq)]
pub enum Fetch { Done, None, Broken }

/// Getter side: write chunks to `<cache>.part`, check each hash, on the last one check the whole sha and rename.
/// `progress(done_chunks, total)` is called after each chunk.
pub async fn fetch_file(s: &mut SpaceStream, space: &SpaceId, offer: &FileOffer, space_dir: &Path,
                        progress: &(dyn Fn(u16, u16) + Send + Sync)) -> Fetch;

/// Copy to `downloads` with the cleaned name; add " (1)", " (2)" before the extension if taken. Returns the final name.
pub fn save_copy(cached: &Path, downloads: &Path, name: &str) -> Result<String, String>;
```

Service methods:
```rust
pub async fn offer_file(&self, space: &str, channel: &str, path: &str) -> Result<Value, String>;
// offer_from_path; write cache (sha name); files row have = 1, cached_ms = now; send(Body::File(offer.to_body()))
pub async fn get_file(&self, space: &str, sha: &str) -> Result<(), String>;
// already have -> Ok; candidates: author's device nodes first, then every live link of the space; for each: open a
// stream, FileWant, fetch_file; Done -> have = 1; None -> next; Broken -> delete .part, next; none left ->
// Err("No one online has this file right now. Try again later."); at most MAX_DOWNLOADS at once.
pub fn save_file(&self, space: &str, sha: &str) -> Result<Value, String>;  // {"name": final name}
```
`SpaceMessage.file` JSON: `{ "sha": "<hex>", "name": "...", "size": 123, "have": false, "state": "idle" |
"getting" | "failed", "percent": 0 }` (add `percent` to the TS type in D8.2).

Commands:
```rust
#[tauri::command] pub async fn space_file_offer(app: AppHandle, space: String, channel: String, path: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_file_get(app: AppHandle, space: String, sha: String) -> Result<Value, String>;
#[tauri::command] pub async fn space_file_save(app: AppHandle, space: String, sha: String) -> Result<Value, String>;
```

### Tests to write (`spaces/files_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `offer_body_round_trip` | 200 KB file | `from_body(to_body()) == offer`; 4 chunks |
| `offer_over_25_mb_is_refused` | 26 214 401 bytes | `Err("Files can be up to 25 MB.")` |
| `offer_with_wrong_chunk_count_is_rejected` | body with n = 3 for 200 KB | `Err` |
| `file_names_are_cleaned` | `..\..\evil.exe`, `/etc/passwd`, `a\u{202E}gpj.exe`, `""` | `evil.exe`, `passwd`, `agpj.exe`, `file` |
| `member_gets_a_file_from_the_author` | A offers 300 KB; B `get_file` | B cache has the same bytes; `have = 1` |
| `third_member_gets_it_from_a_non_author` | A offers; B gets; A offline; C gets | C gets it from B |
| `bad_chunk_is_detected` | B serves with one flipped byte (test hook) | C `Fetch::Broken`; no cache file; `.part` removed |
| `no_one_has_it` | only the author has it and is offline | `Err("No one online has this file right now. Try again later.")` |
| `stranger_cannot_fetch` | node D (not a member) sends FileWant while an invite is open | stream closed (first frame must be JoinRequest for strangers) |
| `save_copy_adds_suffix` | Downloads has `a.txt` | saved as `a (1).txt` |
| `files_count_toward_retention` | cached 60 MB + entries 150 MB | D3.6 prune removes the file first |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d81 cargo test --lib spaces::files_tests
# expected: test result: ok. 11 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d81 cargo test --lib spaces::guard_tests
# expected: test result: ok. 8 passed; 0 failed
```

### Done when
- [ ] 11 tests pass; nothing in Spaces opens or runs a file (search `open::`, `opener`, `Command::new` in `spaces/`:
      none).

### Do NOT
- R1-R7.
- Do not add `iroh-blobs` or any crate.
- Do not download files automatically or preview them (no image decoding in v1).
- Do not write outside the space's `files/` folder and the Downloads folder.
- Do not trust the offer's `mime` for anything.

### If it fails
1. Downloads folder not found: Open facts F-1; if the app has no helper, use Tauri's path API for the download
   directory from the `AppHandle` in the command and pass it into `save_file`.
2. Large files stall on `memnet`: the duplex buffer is 1 MiB; the server must `flush` after each chunk frame.
3. `.part` left behind on error: wrap `fetch_file` so every non-Done result removes the `.part` file.

---

## D8.2 Files UI
Needs merged first: D8.1.

### Goal
Add `Attach file` to the composer, a file card in messages with `Get file` / progress / `Save to Downloads`, and the
safety line.

### Read first
- `components/spaces/Composer.tsx`, `MessageRow.tsx`, `lib/spaces.ts`.
- Whether `@tauri-apps/plugin-dialog` is in `packages/app/package.json` (Open facts F-2).

### Files
- edit `packages/app/src/lib/spaces.ts`
- create `packages/app/src/components/spaces/FileCard.tsx`
- create `packages/app/src/components/spaces/FileCard.test.tsx`
- edit `packages/app/src/components/spaces/MessageRow.tsx`
- edit `packages/app/src/components/spaces/Composer.tsx`

### Steps
1. `lib/spaces.ts`: wrappers `spaceFileOffer(space, channel, path)`, `spaceFileGet(space, sha)`,
   `spaceFileSave(space, sha)`; `percent: number` on `SpaceFile`; `formatSize(bytes)`.
2. `FileCard.tsx` (below); `MessageRow` renders `<FileCard>` when `message.file` is set (the text line is the file
   name, so `MessageRow` does not render `text` for file messages).
3. `Composer.tsx`: if the dialog plugin exists (F-2), an `Attach file` button calls its `open({ multiple: false })`
   and then `onAttach(path)`; if it does not exist, the button is not rendered. New prop
   `onAttach?: (path: string) => Promise<string | null>`.
4. Tests.

### Code
```tsx
import type { SpaceFile } from '../../lib/spaces';

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export interface FileCardProps {
  file: SpaceFile & { percent: number };
  error: string | null;
  savedAs: string | null;
  onGet: () => void;
  onSave: () => void;
}

export function FileCard({ file, error, savedAs, onGet, onSave }: FileCardProps) {
  return (
    <div className="ck-spaces-file">
      <p className="ck-spaces-file-name">{file.name} · {formatSize(file.size)}</p>
      {file.state === 'getting' ? (
        <p className="ck-spaces-hint" role="status">Getting file… {file.percent}%</p>
      ) : file.have ? (
        <button type="button" className="ck-spaces-btn" onClick={onSave}>Save to Downloads</button>
      ) : (
        <button type="button" className="ck-spaces-btn" onClick={onGet}>Get file</button>
      )}
      {savedAs && <p className="ck-spaces-hint" role="status">Saved to Downloads as {savedAs}.</p>}
      {error && <p className="ck-spaces-hint is-error" role="alert">{error}</p>}
      <p className="ck-spaces-hint">Files from other people can be harmful. Only open files you expect.</p>
    </div>
  );
}
```
CSS (append to `cockpit-spaces.css`; a stylesheet edit is allowed in this slice as the 6th file only for these two
rules): `.ck-spaces-file { margin-top: 4px; padding: 6px 8px; border: 1px solid var(--sp-line); border-radius:
var(--sp-radius); max-width: 360px; }` and `.ck-spaces-file-name { margin: 0 0 4px; font-weight: 600; }`.

### Tests to write (`FileCard.test.tsx`)
| Test | Input | Expected |
|---|---|---|
| `not here shows Get file` | `have: false, state: 'idle'` | button `Get file`; click calls `onGet` |
| `getting shows percent` | `state: 'getting', percent: 40` | `Getting file… 40%` |
| `here shows Save to Downloads` | `have: true` | button `Save to Downloads`; click calls `onSave` |
| `saved line` | `savedAs: 'a (1).txt'` | `Saved to Downloads as a (1).txt.` |
| `safety line always shows` | any | `Files from other people can be harmful. Only open files you expect.` |
| `formatSize` | 512, 2048, 2 516 582 | `512 B`, `2 KB`, `2.4 MB` |
| `no open button exists` | any | no button named `Open` |

### Run
```bash
npx vitest run packages/app/src/components/spaces
# expected: no failed tests (FileCard.test.tsx 7 passed)
```

### Done when
- [ ] File messages show a card; attach works when the dialog plugin exists; 7 tests pass.

### Do NOT
- R1-R7.
- No `Open` button, no preview, no thumbnail.
- Do not read file bytes in TS (the backend reads the path).

### If it fails
1. Dialog plugin missing: hide the button (step 3) and record F-2 in the commit.
2. `…` character renders oddly in tests: compare with the exact string including U+2026.
3. `MessageRow` tests break: file messages have `text` = name; keep rendering text for non-file messages only.

---

## D8.3 Presence (backend)
Needs merged first: D5.5 (plan 25).

### Goal
Send and relay presence (online / away) on live links, keep a 90-second table, and expose each member's state in
`detail()`; respect "Show when I'm online".

### Read first
- `spaces/sync.rs` (`PRESENCE` = `0x06`), `spaces/service.rs` (`run_sync` writer, `detail()`), `spaces/store.rs`
  (meta).

### Files
- create `packages/app/src-tauri/src/spaces/presence.rs`
- create `packages/app/src-tauri/src/spaces/presence_tests.rs`
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

### Steps
1. `presence.rs`: frame encode/decode and `PresenceTable` (below).
2. `service.rs`: every 30 s and on change, write a Presence frame on each live sync stream; on receiving one,
   `table.heard(..)`; `detail()` uses `table.state_of(member, now)`. Ignore Presence entries about members not in
   the space. My own state comes from `set_my_presence`. A global meta-like setting `show_online` (stored in the app
   data dir file `spaces/settings.json` as `{ "showOnline": true }`; default true) — when false send nothing.
3. Commands `space_presence(state)` (`"online"` / `"away"`) and `space_show_online(on)`.
4. Tests.

### Code
Presence frame body (`0x06`): `u8` my state (1 online, 2 away), `u16` n (≤ 50), n x (`[32]` member, `u8` state,
`u64` seen_ms).

```rust
//! Presence: ephemeral, never stored in the log. 90-second freshness.
use std::collections::BTreeMap;
use super::ids::MemberKey;

pub const FRESH_MS: u64 = 90_000;
pub const SEND_EVERY_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence { Online, Away, Offline }

pub struct PresenceTable { seen: BTreeMap<MemberKey, (Presence, u64)> }

impl PresenceTable {
    pub fn new() -> Self;
    /// Keep the newest report per member; reports with seen_ms in the future by more than 10 s are clamped to now.
    pub fn heard(&mut self, member: MemberKey, state: Presence, seen_ms: u64, now_ms: u64);
    pub fn state_of(&self, member: &MemberKey, now_ms: u64) -> Presence; // Offline when older than FRESH_MS
    /// What I relay: fresh entries only, at most 50.
    pub fn relay_list(&self, now_ms: u64) -> Vec<(MemberKey, Presence, u64)>;
}

pub fn encode(mine: Presence, relay: &[(MemberKey, Presence, u64)]) -> Vec<u8>;
pub fn decode(body: &[u8]) -> Result<(Presence, Vec<(MemberKey, Presence, u64)>), super::wire::WireError>;
```
The sender of a frame is the link's peer; its own state is recorded as `heard(peer_member, mine, now, now)`.

### Tests to write (`spaces/presence_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `fresh_report_is_online` | heard Sam online at now | `Online` |
| `stale_after_90_seconds` | heard at now - 90 001 | `Offline` |
| `newest_report_wins` | away at t, online at t+5 s, then away at t+1 s arrives | `Online` |
| `future_seen_is_clamped` | seen = now + 1 h | entry seen = now; stale after 90 s |
| `relay_list_has_only_fresh` | 3 fresh, 2 stale | 3 |
| `frame_round_trip` | encode/decode with 2 entries | equal |
| `two_hop_presence` | services A-B-C (cut A-C); A online | C sees A `online` within 31 s (manual clock) |
| `hidden_sends_nothing` | A `show_online(false)` | B sees A `offline` after 90 s; no Presence frame from A |
| `away_state_reaches_others` | A `space_presence("away")` | B sees A `away` |
| `presence_about_strangers_is_ignored` | frame lists a non-member key | not in the table |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d83 cargo test --lib spaces::presence_tests
# expected: test result: ok. 10 passed; 0 failed
```

### Done when
- [ ] `detail().members[].presence` is real; 10 tests pass; nothing about presence is written to SQLite.
- [ ] `view()` JSON has `"showOnline": bool` (read from `spaces/settings.json`, default `true`).

### Do NOT
- R1-R7.
- Do not store presence history.
- Do not show "agent working" or any activity detail (decision open; default not shown).

### If it fails
1. Two-hop takes longer than 31 s: B relays on its 30 s tick; the test must advance the clock past one tick.
2. Hidden mode still relays others: allowed (relaying others is fine); only my own state is not sent.
3. `settings.json` write fails in tests: use the test's temp dir as the service `dir`.

---

## D8.4 Presence reporting from the UI
Needs merged first: D8.3.

### Goal
Report Online/Away from the app window's activity and add the "Show when I'm online" checkbox to Space info.

### Read first
- `components/spaces/SpacesPage.tsx` (Space info sheet), `lib/spaces.ts`.

### Files
- edit `packages/app/src/lib/spaces.ts`
- edit `packages/app/src/lib/spaces.test.ts`
- edit `packages/app/src/components/spaces/SpacesPage.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.test.tsx`

### Steps
1. `lib/spaces.ts`: `spacePresence(state)`, `spaceShowOnline(on)`, and `usePresenceReporter(enabled)`:
   listens to `pointerdown`, `keydown` and `focus` on `window`; sends `online` on the first activity after being
   away; sends `away` after 10 minutes without activity (one timer); sends at most one call per 30 s.
2. `SpacesPage`: call `usePresenceReporter(true)`; in Space info add the checkbox `Show when I'm online` bound to
   `view.showOnline` (add `showOnline: boolean` to the `SpacesView` TS type; the backend field comes from D8.3).
   On change call `spaceShowOnline(on)` and apply the returned view.
3. Tests.

### Code
```ts
export const spacePresence = (state: 'online' | 'away') => invoke<void>('space_presence', { state });
export const spaceShowOnline = (on: boolean) => invoke<SpacesView>('space_show_online', { on });

export const AWAY_AFTER_MS = 10 * 60 * 1000;

export function usePresenceReporter(enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return;
    let state: 'online' | 'away' = 'online';
    let last = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const report = (s: 'online' | 'away') => {
      const now = Date.now();
      if (s === state && now - last < 30_000) return;
      state = s; last = now;
      void spacePresence(s).catch(() => undefined);
    };
    const active = () => {
      report('online');
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => report('away'), AWAY_AFTER_MS);
    };
    active();
    window.addEventListener('pointerdown', active);
    window.addEventListener('keydown', active);
    window.addEventListener('focus', active);
    return () => {
      if (timer) clearTimeout(timer);
      window.removeEventListener('pointerdown', active);
      window.removeEventListener('keydown', active);
      window.removeEventListener('focus', active);
    };
  }, [enabled]);
}
```
The backend `view()` field `"showOnline": bool` is added by D8.3 (see its Done when).

### Tests to write
`spaces.test.ts`:
| Test | Input | Expected |
|---|---|---|
| `reporter sends online at start` | mount with fake timers | `space_presence` with `online` |
| `reporter goes away after ten minutes` | advance 10 min | `away` sent |
| `activity brings it back online` | after away, `keydown` | `online` sent |
| `reporter does not spam` | 20 keydowns in 1 s | only the first `online` call |
`SpacesPage.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `show online checkbox calls the command` | open Space info, uncheck `Show when I'm online` | `space_show_online` with `on: false` |

### Run
```bash
npx vitest run packages/app/src/lib/spaces.test.ts packages/app/src/components/spaces/SpacesPage.test.tsx
# expected: no failed tests
```

### Done when
- [ ] Dots change in the people column from real reports (D8.3 tests), and the UI reports activity (4 + 1 tests).

### Do NOT
- R1-R7.
- Do not track activity outside the app window (no OS-level idle APIs).

### If it fails
1. Fake timers and `Date.now`: use `vi.useFakeTimers({ toFake: ['setTimeout', 'Date'] })`.
2. `focus` fires in jsdom on mount: the first report is `online` anyway.
3. `showOnline` missing from the view: D8.3 must add it; check D8.3 is merged.

---

## D9.1 Mailbox node (client side) — only if decision 9 becomes yes
Needs merged first: D8.4, and founder decision 9 changed to yes.

### Goal
Let the owner add a mailbox node to a space (a node that stores and serves sealed entries for 7 days and can never
read them), so members who are never online at the same time still catch up.

### Read first
- `spaces/roster.rs` (`AddMailbox`/`RemoveMailbox`, rules 16-17, `MAX_MAILBOXES = 2`), `spaces/service.rs` (dial
  candidates, `refresh_permissions`), `spaces/sync.rs`.
- Facts §8 M1 (mailbox counter bug in the team relay) — the mailbox here has no counter: it serves per-author seqs.
- Open facts B-1 (the mailbox server program is a separate plan).

### Files
- create `packages/app/src-tauri/src/spaces/mailbox.rs`
- create `packages/app/src-tauri/src/spaces/mailbox_tests.rs`
- edit `packages/app/src-tauri/src/spaces/service.rs`
- edit `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

### Steps
1. `mailbox.rs`: mailbox address format `pms-mailbox1:` + base32 of (`u8` version 1, `[32]` node, `str` relay,
   `u8` k ≤ 4, k x `str` addr); `parse_mailbox`, `mailbox_link`.
2. `service.rs`: `add_mailbox(space, address)` (owner only; `next_op(AddMailbox)`; save the hint in `peers`),
   `remove_mailbox(space, node)`; mailbox nodes are dial candidates right after the owner; permissions include
   them; sync with a mailbox is the same `SyncSession` (it sends Hello with heads; it never gets grants, so it can
   only store sealed entries). Entries pushed to a mailbox follow the normal `forward`.
3. Commands `space_mailbox_add(space, address)`, `space_mailbox_remove(space, node)`; register.
4. Tests: a "mailbox" in tests is a `SpacesService` with a special test mode that opens a space from its control log
   without a grant (helper `MailboxSim` in the test file).

### Code
```rust
//! Mailbox nodes: store and serve sealed entries for 7 days; never get an epoch key.
use super::ids::NodeKey;
use super::net::PeerHint;

pub const PREFIX: &str = "pms-mailbox1:";
pub const MAILBOX_KEEP_MS: u64 = 7 * 24 * 60 * 60 * 1000;

pub fn mailbox_link(h: &PeerHint) -> String;
pub fn parse_mailbox(text: &str) -> Result<PeerHint, &'static str>;   // "That doesn't look like a mailbox address."
pub fn is_mailbox(roster: &super::roster::Roster, node: &NodeKey) -> bool { roster.mailboxes.contains(node) }
```

### Tests to write (`spaces/mailbox_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `mailbox_address_round_trip` | hint with relay and 2 addrs | equal after parse |
| `only_owner_adds_a_mailbox` | member adds | `Err("Only the owner can add a mailbox.")` |
| `mailbox_never_gets_a_key` | add mailbox M; rotate | no grant for any M device; M cannot open any entry |
| `offline_members_catch_up_through_the_mailbox` | A sends while B offline and M online; A goes offline; B comes online | B gets A's messages from M |
| `mailbox_serves_per_author_seqs_after_pruning` | M pruned entries older than 7 days (floor raised); A sends seq 41 | B gets seq 41 (no counter reset, cowork M1) |
| `third_mailbox_is_refused` | 3rd AddMailbox | `Err` (rule `too many mailboxes`) |
| `removed_mailbox_is_refused` | RemoveMailbox, M dials | refused |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d91 cargo test --lib spaces::mailbox_tests
# expected: test result: ok. 7 passed; 0 failed
```

### Done when
- [ ] Founder said yes to decision 9 (written in the commit message with the date). Otherwise this slice is not
      built.
- [ ] 7 tests pass. The mailbox server program itself is NOT part of this slice (Open facts B-1).

### Do NOT
- R1-R7.
- Do not send epoch keys to a mailbox, ever.
- Do not use HTTP or the team relay for the mailbox: it speaks `pm/spaces/1` over iroh like any member (founder
  rule: only `iroh_net.rs` touches the network).
- Do not add a Labs flag for this; it is a per-space owner choice gated by decision 9.

### If it fails
1. `Rotate` rule `grants do not match devices` fails with a mailbox: mailboxes are not devices; `all_devices()`
   must not include them.
2. The mailbox is refused by `SpacePermissions`: `refresh_permissions` must add `roster.mailboxes`.
3. The mailbox stores entries it cannot check: it can check signatures (device certs are public in the control
   log); it skips `open`. `ingest_entry` with an empty `EpochKeys` already stores without showing.

---

## D9.2 Mailbox UI — only if decision 9 becomes yes
Needs merged first: D9.1.

### Goal
Let the owner turn a mailbox on in Space info by pasting a mailbox address, and show its status.

### Read first
- `components/spaces/SpacesPage.tsx` (Space info sheet), `lib/spaces.ts`.

### Files
- edit `packages/app/src/lib/spaces.ts`
- create `packages/app/src/components/spaces/MailboxSettings.tsx`
- create `packages/app/src/components/spaces/MailboxSettings.test.tsx`
- edit `packages/app/src/components/spaces/SpacesPage.tsx`

### Steps
1. Wrappers `spaceMailboxAdd(space, address)`, `spaceMailboxRemove(space, node)`; `SpaceDetail.mailboxes:
   string[]` (node short ids).
2. `MailboxSettings.tsx`: owner only; when none: the line `Keep messages for 7 days on a mailbox`, help text,
   field `Mailbox address` (placeholder `pms-mailbox1:…`), button `Turn on`; when on: `Mailbox on. Sealed messages
   wait there for 7 days.` + button `Turn off`. Errors shown as returned.
3. Mount in Space info for owners.
4. Tests.

### Code
```tsx
export interface MailboxSettingsProps {
  isOwner: boolean;
  mailboxes: string[];
  onAdd: (address: string) => Promise<string | null>;   // error text or null
  onRemove: (node: string) => void;
}
// Renders nothing when !isOwner. Texts exactly as in "Words on screen".
```

### Tests to write (`MailboxSettings.test.tsx`)
| Test | Input | Expected |
|---|---|---|
| `member sees nothing` | `isOwner: false` | empty |
| `owner can turn on` | type an address, click `Turn on` | `onAdd(address)` |
| `on state text` | `mailboxes: ['4F7Q-9K2M']` | `Mailbox on. Sealed messages wait there for 7 days.`; `Turn off` calls `onRemove('4F7Q-9K2M')` |
| `error shows` | `onAdd` returns `That doesn't look like a mailbox address.` | that text |

### Run
```bash
npx vitest run packages/app/src/components/spaces/MailboxSettings.test.tsx
# expected: Tests  4 passed (4)
```

### Done when
- [ ] Decision 9 is yes; 4 tests pass.

### Do NOT
- R1-R7.
- Do not offer a default mailbox address (no hosted service is assumed; the paid relay is a separate decision).

### If it fails
1. `SpaceDetail` lacks `mailboxes`: add it to `detail()` in D9.1 (it is the service file of that slice).
2. Placeholder ellipsis mismatch: U+2026.
3. Sheet layout overflow: reuse `ck-spaces-field`.

---

## Open facts

- NEEDS-FACT F-1: how the app finds the user's Downloads folder (an existing helper, or Tauri's path API in this
  Tauri version). Search `download_dir` and `Downloads` in `packages/app/src-tauri/src`.
- NEEDS-FACT F-2: whether `@tauri-apps/plugin-dialog` (or another file picker) is installed and allowed in the
  Tauri capabilities. Read `packages/app/package.json` and `packages/app/src-tauri/tauri.conf.json` /
  `capabilities/`.
- NEEDS-FACT B-1: where a headless mailbox node program would live and be deployed (`relay/server/` exists per facts
  §8; whether it can host an iroh node). Read `relay/server/src/lib.rs` header and the relay plan
  (`docs/plans/1104plans/_facts/relay-facts.md` was not read by this writer).
- NEEDS-FACT B-2: whether the founder's "paid SaaS relay" plan (memory: Co-work Heads-up decisions) covers hosting a
  Spaces mailbox node. Ask the founder; until then D9.1 stays unbuilt.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Malicious file content | Member sends malware | Never opened or run by the app; only saved on click; warning line under every file | `no open button exists`, safety line test |
| Path tricks in file names | `..\..\Startup\x.exe` | `clean_file_name` strips folders and bidi chars | `file_names_are_cleaned` |
| Corrupt or swapped file | A member serves different bytes | Per-chunk SHA-256 from the signed, sealed offer; whole-file check | `bad_chunk_is_detected` |
| Disk fill with files | Many 25 MB offers | Download only on click; cached files count toward 200 MB and are pruned first | `files_count_toward_retention` |
| Stranger downloads files | Non-member asks for a file | Strangers may only send JoinRequest | `stranger_cannot_fetch` |
| Presence spying | Track when someone is online | Presence is ephemeral, never stored; "Show when I'm online" off sends nothing | `hidden_sends_nothing` |
| Fake presence | Claim someone is online | Only members' reports are kept; reports go stale in 90 s; future times clamped | `future_seen_is_clamped`, `presence_about_strangers_is_ignored` |
| Mailbox reads messages | Operator of the mailbox | Mailbox never gets an epoch key; entries are sealed | `mailbox_never_gets_a_key` |
| Mailbox counter bug (cowork M1) | New mail never seen after the box empties | No counter: per-author seqs and floors | `mailbox_serves_per_author_seqs_after_pruning` |
| Mailbox as a network back door | HTTP calls from Spaces | Mailbox speaks `pm/spaces/1` over iroh only; `iroh_net.rs` stays the only network file | p2p `only_one_file_touches_the_network_and_no_test_binds_it`, Spaces `pure_spaces_files_open_no_files_or_sockets` |

## What cannot be verified without real hardware or a second computer

- File transfer speed and resume behaviour over a relayed link between two homes.
- Antivirus behaviour on Windows when a file lands in Downloads.
- Presence dots across three real PCs where two are not directly linked.
- A real mailbox node on a server (no program exists yet; Open facts B-1).
