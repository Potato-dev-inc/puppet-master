# Spaces D-3: message log, SQLite store, sync, catch-up, retention

Read `20-spaces-overview.md` and the shared rules R1-R7 at the top of
`21-spaces-d0-d1-transport-spike-and-secure-core.md`. R1-R7 apply in full to every slice here.

Writer's sources: `00-FORMAT.md` and `_facts/spaces-facts.md` only (facts cited as `facts §N`).

Slices: D3.1, D3.2, D3.3, D3.4, D3.5, D3.6 (6 slices). Merge order: D3.1 goes right after D2.4 (plan 22 needs it),
then D2.5, D2.6, then D3.2-D3.6.

---

## D3.1 Space store: open, schema v1, meta, control ops, invites, join requests
Needs merged first: D2.4 (plan 22).

### Goal
Add `spaces/store.rs`: one SQLite file per space with the full v1 schema and the functions D-2 needs (meta, control
ops, invites, join requests).

### Read first
- `packages/app/src-tauri/src/chat_index/store.rs` lines 1-140: `use rusqlite::{params, Connection,
  OptionalExtension};`, `pub fn open(path: &Path, cap: usize)`, `pub fn open_memory(cap: usize)` (facts §5). Copy
  its style (error type, pragma calls, how it creates the parent folder).
- `packages/app/src-tauri/Cargo.toml` line 74: `rusqlite = { version = "0.32", features = ["bundled"] }` (facts §5).
- `spaces/roster.rs` (`ControlOp::to_bytes/from_bytes`), `spaces/invite.rs` (`InviteRecord`, `InviteState`).

### Files
- create `packages/app/src-tauri/src/spaces/store.rs`
- create `packages/app/src-tauri/src/spaces/store_tests.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `store.rs` with `SCHEMA` (exact SQL below) and the functions below.
2. Create `store_tests.rs`; add `pub mod store;` and `#[cfg(test)] mod store_tests;` to `mod.rs`.

### Code
Place: `app_paths::app_data_dir()/spaces/<space_hex>/space.sqlite` (the service computes the path; facts §9).

`SCHEMA` (run once per open inside one `execute_batch`; `auto_vacuum` must come before the first table):
```sql
PRAGMA auto_vacuum = INCREMENTAL;
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS control_ops (
  seq   INTEGER PRIMARY KEY,
  hash  BLOB NOT NULL UNIQUE,
  bytes BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS authors (
  device BLOB PRIMARY KEY,
  floor  INTEGER NOT NULL DEFAULT 0,
  high   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS entries (
  id          BLOB PRIMARY KEY,
  device      BLOB NOT NULL,
  seq         INTEGER NOT NULL,
  channel     BLOB NOT NULL,
  lamport     INTEGER NOT NULL,
  created_ms  INTEGER NOT NULL,
  received_ms INTEGER NOT NULL,
  kind        INTEGER NOT NULL,
  ref_id      BLOB,
  epoch       INTEGER NOT NULL,
  bytes       BLOB NOT NULL,
  size        INTEGER NOT NULL,
  gone        INTEGER NOT NULL DEFAULT 0,
  UNIQUE (device, seq)
);
CREATE INDEX IF NOT EXISTS entries_by_created ON entries (created_ms);
CREATE INDEX IF NOT EXISTS entries_by_ref ON entries (ref_id);

CREATE TABLE IF NOT EXISTS messages (
  id          BLOB PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
  channel     BLOB NOT NULL,
  member      BLOB NOT NULL,
  device      BLOB NOT NULL,
  lamport     INTEGER NOT NULL,
  created_ms  INTEGER NOT NULL,
  kind        INTEGER NOT NULL,
  text        TEXT NOT NULL,
  reply_to    BLOB,
  deleted_by  BLOB,
  deleted_ms  INTEGER,
  file_json   TEXT
);
CREATE INDEX IF NOT EXISTS messages_by_channel ON messages (channel, lamport, created_ms, id);

CREATE TABLE IF NOT EXISTS reactions (
  target  BLOB NOT NULL,
  member  BLOB NOT NULL,
  emoji   TEXT NOT NULL,
  is_on   INTEGER NOT NULL,
  lamport INTEGER NOT NULL,
  entry   BLOB NOT NULL,
  PRIMARY KEY (target, member, emoji)
);

CREATE TABLE IF NOT EXISTS reads (
  channel      BLOB PRIMARY KEY,
  last_lamport INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS invites (
  id         BLOB PRIMARY KEY,
  created_ms INTEGER NOT NULL,
  expires_ms INTEGER NOT NULL,
  strikes    INTEGER NOT NULL DEFAULT 0,
  state      TEXT NOT NULL CHECK (state IN ('open','pending','used','dead','cancelled')),
  joiner     BLOB
);

CREATE TABLE IF NOT EXISTS join_requests (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  invite       BLOB NOT NULL,
  joiner       BLOB NOT NULL,
  cert         BLOB NOT NULL,
  name         TEXT NOT NULL,
  received_ms  INTEGER NOT NULL,
  answer_by_ms INTEGER NOT NULL,
  state        TEXT NOT NULL CHECK (state IN ('waiting','let_in','refused','expired'))
);

CREATE TABLE IF NOT EXISTS blocks (
  member   BLOB PRIMARY KEY,
  since_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peers (
  node         BLOB PRIMARY KEY,
  relay        TEXT NOT NULL DEFAULT '',
  lan          TEXT NOT NULL DEFAULT '',
  last_seen_ms INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS files (
  sha       BLOB PRIMARY KEY,
  name      TEXT NOT NULL,
  size      INTEGER NOT NULL,
  have      INTEGER NOT NULL DEFAULT 0,
  offer     BLOB NOT NULL,
  cached_ms INTEGER
);

CREATE TABLE IF NOT EXISTS fish_usage (
  at_ms  INTEGER NOT NULL,
  person BLOB,
  kind   TEXT NOT NULL CHECK (kind IN ('called','unprompted'))
);
CREATE INDEX IF NOT EXISTS fish_usage_by_time ON fish_usage (at_ms);

PRAGMA user_version = 1;
```

Meta keys (exact): `space_id` (16 bytes), `role` (`owner` / `member` / `joining`), `my_member` (32), `my_device`
(32), `lamport` (8 bytes big-endian u64), `joined_ms` (8 bytes), `pending_link` (UTF-8 link text, joiner only, removed
when joined), `name_in_space` (UTF-8, my display name here).

```rust
//! One SQLite file per space. Never holds keys or invite secrets (those live in the OS keystore).
use std::path::Path;
use rusqlite::{params, Connection, OptionalExtension};

use super::ids::{InviteId, NodeKey};
use super::invite::{InviteRecord, InviteState};
use super::roster::ControlOp;

pub const SCHEMA: &str = "...the SQL above...";

pub struct SpaceStore { pub(crate) conn: Connection }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinRequestRow {
    pub id: i64, pub invite: InviteId, pub joiner: NodeKey, pub cert: Vec<u8>, pub name: String,
    pub received_ms: u64, pub answer_by_ms: u64, pub state: String,
}

impl SpaceStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self>;   // create parent dir like chat_index::store::open, then init
    pub fn open_memory() -> rusqlite::Result<Self>;
    fn init(conn: Connection) -> rusqlite::Result<Self> {
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v == 0 { conn.execute_batch(SCHEMA)?; }
        else { conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?; }
        Ok(Self { conn })
    }
    pub fn schema_version(&self) -> rusqlite::Result<i64>;

    pub fn meta_get(&self, key: &str) -> rusqlite::Result<Option<Vec<u8>>> {
        self.conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0)).optional()
    }
    pub fn meta_set(&self, key: &str, value: &[u8]) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        ).map(|_| ())
    }
    pub fn meta_delete(&self, key: &str) -> rusqlite::Result<()>;
    pub fn meta_u64(&self, key: &str) -> rusqlite::Result<u64>;           // missing = 0
    pub fn meta_set_u64(&self, key: &str, v: u64) -> rusqlite::Result<()>; // 8 bytes big-endian

    /// Appends only if op.seq == last seq + 1; otherwise Err(rusqlite::Error::InvalidQuery) is NOT used:
    /// return Ok(false) and do not write. Ok(true) = written.
    pub fn control_append(&self, op: &ControlOp) -> rusqlite::Result<bool>;
    pub fn control_ops(&self) -> rusqlite::Result<Vec<ControlOp>>;        // ORDER BY seq; an unparsable row is an Err
    pub fn control_ops_after(&self, seq: u64) -> rusqlite::Result<Vec<ControlOp>>;
    pub fn control_hash_at(&self, seq: u64) -> rusqlite::Result<Option<[u8; 32]>>;

    pub fn invite_put(&self, r: &InviteRecord) -> rusqlite::Result<()>;   // upsert by id
    pub fn invite_get(&self, id: &InviteId) -> rusqlite::Result<Option<InviteRecord>>;
    pub fn invites(&self) -> rusqlite::Result<Vec<InviteRecord>>;         // ORDER BY created_ms

    pub fn join_request_add(&self, invite: &InviteId, joiner: &NodeKey, cert: &[u8], name: &str,
                            received_ms: u64, answer_by_ms: u64) -> rusqlite::Result<i64>;
    pub fn join_request_set(&self, id: i64, state: &str) -> rusqlite::Result<()>;
    pub fn join_request(&self, id: i64) -> rusqlite::Result<Option<JoinRequestRow>>;
    pub fn join_requests_waiting(&self) -> rusqlite::Result<Vec<JoinRequestRow>>;
}

/// InviteState <-> (state text, joiner column).
pub fn state_to_sql(s: InviteState) -> (&'static str, Option<[u8; 32]>) {
    match s {
        InviteState::Open => ("open", None),
        InviteState::Pending(n) => ("pending", Some(n.0)),
        InviteState::Used => ("used", None),
        InviteState::Dead => ("dead", None),
        InviteState::Cancelled => ("cancelled", None),
    }
}
pub fn state_from_sql(text: &str, joiner: Option<Vec<u8>>) -> Option<InviteState>;
```

### Tests to write (`spaces/store_tests.rs`, all on `open_memory()`, one test on a temp file)
| Test | Input | Expected |
|---|---|---|
| `fresh_store_has_schema_v1` | `open_memory()` | `schema_version() == 1`; 13 tables exist (query `sqlite_master`) |
| `reopen_keeps_data_and_does_not_rerun_schema` | temp dir file, `meta_set("role", b"owner")`, drop, `open` again | `meta_get("role") == Some(b"owner")`, version 1 |
| `auto_vacuum_is_incremental` | `PRAGMA auto_vacuum` | `2` |
| `meta_u64_round_trip` | `meta_set_u64("lamport", 77)` | `meta_u64("lamport") == 77`; missing key gives 0 |
| `control_append_needs_next_seq` | genesis op (seq 1) then an op with seq 3 | first `Ok(true)`, second `Ok(false)`, `control_ops().len() == 1` |
| `control_ops_round_trip_and_replay` | genesis + AddChannel appended | `Roster::replay(space, &control_ops()?)` ok, 2 channels |
| `invite_round_trip_all_states` | put records in each of the 5 states | `invite_get` returns equal records |
| `join_request_lifecycle` | add, `join_requests_waiting` len 1, set `let_in` | waiting list empty; `join_request(id).state == "let_in"` |
| `bad_invite_state_text_is_rejected_by_sql` | raw `INSERT ... state='weird'` | SQL error (CHECK constraint) |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d31 cargo test --lib spaces::store_tests
# expected: test result: ok. 9 passed; 0 failed
```

### Done when
- [ ] 9 tests pass; no keys or secrets in any table (check: no column named `key`, `secret`, `words`).

### Do NOT
- R1-R7.
- Do not add the store to `PURE` in the guard (it opens files).
- Do not use `serde_json` to store bytes; use BLOB columns.
- Do not store the invite secret, the words, or epoch keys.

### If it fails
1. `auto_vacuum` returns 0: the pragma ran after a table existed; it must be the first statement on a new file.
2. `foreign_keys` cascade does nothing: `PRAGMA foreign_keys = ON` must run on every open (done in `init`).
3. Temp dir: use `std::env::temp_dir().join(format!("pm-spaces-test-{}", rand))` and remove it at the end; do not
   add the `tempfile` crate if it is not already a dev-dependency (check `Cargo.toml`).

---

## D3.2 Message log entry
Needs merged first: D2.3 (plan 22), D3.1.

### Goal
Add `spaces/log.rs`: the entry byte layout, body kinds, sealing with the epoch key, device signature, and the
validity checks every receiver runs.

### Read first
- `spaces/roster.rs` (`Roster`, `device`, `gone`), `spaces/identity.rs`, `secure_core/crypto.rs`
  (`seal_with_key`, `open_with_key`; facts §3: `seal_with_key` pads to `PAD_BUCKETS` 1024/4096/16384).
- `team/event.rs` `clean_text` (facts §3) for the text-cleaning style. Do not import it.

### Files
- create `packages/app/src-tauri/src/spaces/log.rs`
- create `packages/app/src-tauri/src/spaces/log_tests.rs`
- edit `packages/app/src-tauri/src/spaces/roster.rs` (add `past` devices, see step 3)
- edit `packages/app/src-tauri/src/spaces/roster_tests.rs` (one new test)
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `log.rs` as below.
2. Create `log_tests.rs`; add `pub mod log;` and `#[cfg(test)] mod log_tests;`.
3. `roster.rs`: add field `pub past: BTreeMap<DeviceKey, (MemberKey, u32)>` to `Roster` (initialised empty). In
   apply rule 9 (`RemoveMember`), before removing, insert every device of that member with
   `(member, self.epoch + 1)`. Add:
   ```rust
   /// Author of a device: (member, None) if current; (member, Some(e)) if removed, valid only for epochs < e.
   pub fn author(&self, d: &DeviceKey) -> Option<(MemberKey, Option<u32>)> {
       if let Some((m, _)) = self.device(d) { return Some((m.member, None)); }
       self.past.get(d).map(|(m, e)| (*m, Some(*e)))
   }
   ```
   Add the roster test `removed_device_is_kept_in_past` to `roster_tests.rs` (count 19 after this slice).

### Code
Entry byte layout (big-endian; signature domain `pm/spaces/entry/v1`):

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `PMSE` |
| 4 | 1 | version = 1 |
| 5 | 16 | space id |
| 21 | 8 | channel id |
| 29 | 32 | author device key |
| 61 | 8 | seq (per author device per space, from 1) |
| 69 | 8 | lamport |
| 77 | 8 | created_ms (author's clock) |
| 85 | 4 | epoch |
| 89 | 1 | kind |
| 90 | 32 | ref entry id (all zeros = none) |
| 122 | 24 | nonce |
| 146 | 4 + n | `blob` sealed body (XChaCha20-Poly1305, padded by `seal_with_key`) |
| . | 64 | Ed25519 signature by the author device key over bytes 0 .. end of sealed body |

- AEAD associated data ("header") = bytes 0..122 (magic through ref).
- Entry id = SHA-256 of all bytes including the signature.
- Whole entry ≤ `MAX_ENTRY_BYTES` (65 536).

Plain body (before sealing) = `u8` body version (1) + kind-specific bytes:

| Kind | Name | Body bytes | ref |
|---|---|---|---|
| 1 | Text | `str` text (≤ 4000 chars, ≤ 16 000 bytes), `u8` m (≤ 20), m x `[32]` mentioned member | reply target or zeros |
| 3 | React | `str` reaction code (one of the 8 codes below), `u8` on (1 / 0) | target (required) |
| 4 | Delete | nothing | target (required) |
| 5 | Leave | nothing | zeros |
| 6 | FileOffer | defined in plan 27 D8.1 | zeros |
| 7 | FishText | `str` text (≤ 4000 chars), `[32]` member who called Fish (zeros if unprompted) | message Fish answers, or zeros |
| 2, 8..=63 | reserved | stored and forwarded, never shown | any |
| 64..=255 | invalid | rejected | |

Reaction codes (UI maps them to pictures): `thumbs_up`, `heart`, `laugh`, `wow`, `sad`, `party`, `eyes`, `check`.

```rust
//! The message log: one signed, sealed entry per message, reaction, delete, file offer.
use sha2::{Digest, Sha256};

use super::identity::SpaceIdentity;
use super::ids::{ChannelId, DeviceKey, EntryId, MemberKey, SpaceId};
use super::limits::{KEEP_MS, MAX_ENTRY_BYTES, MAX_TEXT_CHARS};
use super::roster::Roster;
use super::wire::{Reader, Writer};
use crate::secure_core::crypto::{open_with_key, seal_with_key};
use crate::secure_core::sign;

pub const ENTRY_DOMAIN: &[u8] = b"pm/spaces/entry/v1";
pub const MAGIC: &[u8; 4] = b"PMSE";
pub const HEADER_LEN: usize = 122;
pub const FUTURE_SKEW_MS: u64 = 10 * 60 * 1000;
pub const REACTIONS: [&str; 8] = ["thumbs_up", "heart", "laugh", "wow", "sad", "party", "eyes", "check"];

pub const KIND_TEXT: u8 = 1;
pub const KIND_REACT: u8 = 3;
pub const KIND_DELETE: u8 = 4;
pub const KIND_LEAVE: u8 = 5;
pub const KIND_FILE: u8 = 6;
pub const KIND_FISH: u8 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogError(pub &'static str);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryHeader {
    pub space: SpaceId, pub channel: ChannelId, pub device: DeviceKey, pub seq: u64, pub lamport: u64,
    pub created_ms: u64, pub epoch: u32, pub kind: u8, pub refers: Option<EntryId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Text { text: String, mentions: Vec<MemberKey> },
    React { code: String, on: bool },
    Delete,
    Leave,
    File(Vec<u8>),                       // raw body, decoded by files.rs (D8.1)
    Fish { text: String, answered: Option<MemberKey> },
    Reserved(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry { pub header: EntryHeader, pub nonce: [u8; 24], pub sealed: Vec<u8>, pub sig: [u8; 64] }

impl EntryHeader {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.fixed(MAGIC).u8(1).fixed(&self.space.0).fixed(&self.channel.0).fixed(&self.device.0)
            .u64(self.seq).u64(self.lamport).u64(self.created_ms).u32(self.epoch).u8(self.kind)
            .fixed(&self.refers.map(|e| e.0).unwrap_or([0u8; 32]));
        w.finish()
    }
}

impl Entry {
    pub fn seal(me: &SpaceIdentity, header: EntryHeader, body: &Body, epoch_key: &[u8; 32]) -> Result<Entry, LogError> {
        if header.device != me.device_key() { return Err(LogError("not my device")); }
        let plain = encode_body(body)?;
        let head = header.to_bytes();
        let (nonce, sealed) = seal_with_key(epoch_key, &head, &plain).map_err(|_| LogError("seal"))?;
        let mut e = Entry { header, nonce, sealed, sig: [0u8; 64] };
        e.sig = me.sign_as_device(ENTRY_DOMAIN, &e.signed_part());
        if e.to_bytes().len() > MAX_ENTRY_BYTES { return Err(LogError("too big")); }
        Ok(e)
    }
    fn signed_part(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.fixed(&self.header.to_bytes()).fixed(&self.nonce).blob(&self.sealed);
        w.finish()
    }
    pub fn to_bytes(&self) -> Vec<u8> { let mut b = self.signed_part(); b.extend_from_slice(&self.sig); b }
    pub fn from_bytes(b: &[u8]) -> Result<Entry, LogError>;  // size cap first, magic, version, kind < 64, done()
    pub fn id(&self) -> EntryId { EntryId(Sha256::digest(self.to_bytes()).into()) }
    pub fn signature_ok(&self) -> bool {
        sign::verify(&self.header.device.0, ENTRY_DOMAIN, &self.signed_part(), &self.sig)
    }
    pub fn open(&self, epoch_key: &[u8; 32]) -> Result<Body, LogError> {
        let plain = open_with_key(epoch_key, &self.nonce, &self.header.to_bytes(), &self.sealed)
            .map_err(|_| LogError("cannot open"))?;
        decode_body(self.header.kind, &plain)
    }
}

pub fn encode_body(body: &Body) -> Result<Vec<u8>, LogError>;
pub fn decode_body(kind: u8, plain: &[u8]) -> Result<Body, LogError>;
pub fn kind_of(body: &Body) -> u8;

/// Keep new lines and tabs, drop other control chars and bidi overrides, trim the end, cap at 4000 chars.
pub fn clean_text(raw: &str) -> String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Valid; `member` is the author's member key.
    Ok { member: MemberKey },
    /// Valid but too old to keep (older than KEEP_MS): do not store, raise the floor.
    Old,
    Reject(&'static str),
}

/// Every check a receiver runs before storing, in this order. Does not open the body.
pub fn check(roster: &Roster, e: &Entry, now_ms: u64) -> Verdict {
    if e.header.space != roster.space { return Verdict::Reject("wrong space"); }
    let Some((member, until)) = roster.author(&e.header.device) else { return Verdict::Reject("unknown author") };
    if e.header.epoch == 0 || e.header.epoch > roster.epoch { return Verdict::Reject("future epoch"); }
    if let Some(limit) = until { if e.header.epoch >= limit { return Verdict::Reject("removed author"); } }
    if !e.signature_ok() { return Verdict::Reject("bad signature"); }
    if e.header.seq == 0 { return Verdict::Reject("bad seq"); }
    if e.header.created_ms > now_ms + FUTURE_SKEW_MS { return Verdict::Reject("from the future"); }
    if e.header.created_ms + KEEP_MS < now_ms { return Verdict::Old; }
    if roster.channel(&e.header.channel).is_none() { return Verdict::Reject("unknown channel"); }
    if e.header.kind == KIND_FISH && !roster.is_owner(&member) { return Verdict::Reject("fish not from owner"); }
    if matches!(e.header.kind, KIND_REACT | KIND_DELETE) && e.header.refers.is_none() {
        return Verdict::Reject("missing target");
    }
    Verdict::Ok { member }
}
```

### Tests to write (`spaces/log_tests.rs`; a helper builds a roster with owner seed 1 and member Sam seed 2, epoch key
`[9;32]`, `NOW = 10_000_000_000`)
| Test | Input | Expected |
|---|---|---|
| `text_entry_round_trips` | Sam seals Text "hi" | `from_bytes(to_bytes())` equal; `open([9;32]) == Text{"hi", []}` |
| `entry_layout_offsets` | any entry | bytes[0..4] `PMSE`, byte 4 = 1, bytes[29..61] = Sam's device key, byte 89 = 1 |
| `id_is_sha256_of_bytes` | entry | `id().0 == Sha256(to_bytes())` |
| `flipped_byte_in_header_fails_open` | flip byte 70 (lamport) in bytes, re-sign with Sam's device | `open` is `Err("cannot open")` (AAD mismatch) |
| `flipped_byte_breaks_signature` | flip one byte of `sealed` | `signature_ok() == false`; `check` gives `Reject("bad signature")` |
| `wrong_key_cannot_open` | `open([8;32])` | `Err` |
| `stranger_author_is_rejected` | entry by seed 9 | `Reject("unknown author")` |
| `removed_author_old_epoch_is_ok_new_epoch_rejected` | remove Sam, rotate to 2; Sam's entry epoch 1 / epoch 2 | `Ok`, `Reject("removed author")` |
| `future_entry_is_rejected` | created_ms = NOW + 11 min | `Reject("from the future")` |
| `ninety_one_day_old_entry_is_old` | created_ms = NOW - 91 days | `Old` |
| `fish_text_by_member_is_rejected` | Sam seals kind 7 | `Reject("fish not from owner")` |
| `react_without_target_is_rejected` | React with `refers: None` | `Reject("missing target")` |
| `oversized_entry_is_refused` | Text of 4000 chars x 4-byte chars plus 20 mentions builds fine; `from_bytes` of 65 537 bytes | first ok (≤ 64 KiB), second `Err("too big")` |
| `reserved_kind_is_stored_not_shown` | kind 8 entry | `decode_body` gives `Reserved(8)`; `check` gives `Ok` |
| `kind_64_is_invalid` | kind byte 64 | `from_bytes` `Err` |
| `clean_text_rules` | `"a\u{202E}b\u{0007}\n\tc   "`; 5000 chars | `"ab\n\tc"`; 4000 chars |
| `reaction_code_must_be_known` | React code `"fire"` | `encode_body` `Err` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d32 cargo test --lib spaces::log_tests
# expected: test result: ok. 17 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d32 cargo test --lib spaces::roster_tests
# expected: test result: ok. 19 passed; 0 failed
```

### Done when
- [ ] 17 log tests and 19 roster tests pass; guard passes (log.rs is PURE).

### Do NOT
- R1-R7.
- Do not sign with the node key or the member key; entries are signed by the device key.
- Do not put any plaintext (text, mentions, reaction code) in the header.
- Do not accept kind 7 from anyone but the owner (plan decision P-11).

### If it fails
1. `seal_with_key` returns `([u8; 24], Vec<u8>)` (facts §3) — if it returns a `Vec` nonce, convert with
   `try_into()`.
2. `flipped_byte_in_header_fails_open` passes for the wrong reason (signature check first): call `open` directly,
   not `check`.
3. Padding makes the 4000-char test exceed 64 KiB: 4000 x 4 bytes = 16 000 + 20 x 32 = 16 640 bytes, padded to
   16 384 bucket or beyond; if `seal_with_key` pads above 16 384 by doubling, the entry is still < 64 KiB. If it
   fails, lower the test to 3000 chars and report the padding rule in the commit.

---

## D3.3 Store: ingest, decrypted messages, reads
Needs merged first: D3.1, D3.2.

### Goal
Add ingest (validate, store, decrypt into `messages`, apply reactions and deletes), author heads, gap finding, and
message pages to `SpaceStore`.

### Read first
- `spaces/store.rs`, `spaces/log.rs`, `spaces/roster.rs`.

### Files
- create `packages/app/src-tauri/src/spaces/ingest.rs`
- create `packages/app/src-tauri/src/spaces/ingest_tests.rs`
- edit `packages/app/src-tauri/src/spaces/store.rs` (read functions only)
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. `store.rs`: add the read/write helpers listed under "store additions".
2. Create `ingest.rs` with `EpochKeys`, `Ingest`, `ingest_entry`, `apply_body`.
3. Tests; `mod.rs`: `pub mod ingest; #[cfg(test)] mod ingest_tests;`.

### Code
Store additions (`impl SpaceStore`):
```rust
pub fn has_entry(&self, id: &EntryId) -> rusqlite::Result<bool>;
pub fn has_seq(&self, device: &DeviceKey, seq: u64) -> rusqlite::Result<bool>;
/// One row per author device: (device, low, high). low = floor + 1, high = highest seq seen.
pub fn heads(&self) -> rusqlite::Result<Vec<(DeviceKey, u64, u64)>>;
pub fn author_floor(&self, device: &DeviceKey) -> rusqlite::Result<u64>;
pub fn raise_floor(&self, device: &DeviceKey, seq: u64) -> rusqlite::Result<()>; // floor = max(floor, seq)
/// Seqs in [from, to] that are neither stored nor below the floor. At most `max`, ascending.
pub fn missing(&self, device: &DeviceKey, from: u64, to: u64, max: usize) -> rusqlite::Result<Vec<u64>>;
/// For a Want: (bytes of stored, not gone entries) and (seqs that are gone).
pub fn entries_for(&self, device: &DeviceKey, seqs: &[u64]) -> rusqlite::Result<(Vec<Vec<u8>>, Vec<u64>)>;
pub fn insert_entry(&self, e: &Entry, id: &EntryId, bytes: &[u8], now_ms: u64) -> rusqlite::Result<()>; // + authors.high
pub fn insert_gone_placeholder(&self, device: &DeviceKey, seq: u64) -> rusqlite::Result<()>;
pub fn next_seq(&self, device: &DeviceKey) -> rusqlite::Result<u64>;        // high + 1
pub fn bump_lamport(&self, seen: u64) -> rusqlite::Result<u64>;             // meta lamport = max(cur, seen); returns it
pub fn next_lamport(&self) -> rusqlite::Result<u64>;                        // cur + 1, stored
pub fn messages_page(&self, channel: &ChannelId, before: Option<(u64, EntryId)>, limit: usize)
    -> rusqlite::Result<Vec<MessageRow>>;  // newest first by (lamport, created_ms, id) DESC, then reversed
pub fn reactions_for(&self, targets: &[EntryId]) -> rusqlite::Result<Vec<(EntryId, String, u32, Vec<MemberKey>)>>;
pub fn mark_read(&self, channel: &ChannelId, lamport: u64) -> rusqlite::Result<()>;
pub fn unread(&self, channel: &ChannelId, me: &MemberKey) -> rusqlite::Result<u32>; // messages after last_lamport not by me, not deleted
```
`MessageRow { id, channel, member, device, lamport, created_ms, kind, text, reply_to, deleted_by, deleted_ms,
file_json }`.

Gone placeholders: an `entries` row with `bytes = x''`, `size = 0`, `gone = 1`, `id = SHA-256("gone" || device ||
seq)`, `channel = 8 zero bytes`, `kind = 0`. A real entry for the same `(device, seq)` replaces the placeholder
(`DELETE` placeholder, then `INSERT`) when it arrives later from another peer (plan decision: gone records are weak).

`spaces/ingest.rs`:
```rust
//! Store an incoming entry: check, insert, decrypt into messages, apply reactions and deletes.
use std::collections::BTreeMap;
use super::ids::{EntryId, MemberKey};
use super::log::{self, Body, Entry, Verdict};
use super::roster::Roster;
use super::store::SpaceStore;

/// Epoch keys this PC can use, by epoch.
#[derive(Default, Clone)]
pub struct EpochKeys(pub BTreeMap<u32, [u8; 32]>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingest {
    /// Stored. `shown` = a messages row was written or changed (UI should refresh).
    Stored { id: EntryId, shown: bool },
    Duplicate,
    Old,
    Rejected(&'static str),
}

pub fn ingest_entry(store: &SpaceStore, roster: &Roster, keys: &EpochKeys, bytes: &[u8], now_ms: u64) -> Ingest {
    let Ok(e) = Entry::from_bytes(bytes) else { return Ingest::Rejected("unreadable") };
    let id = e.id();
    if store.has_entry(&id).unwrap_or(false) { return Ingest::Duplicate; }
    let member = match log::check(roster, &e, now_ms) {
        Verdict::Ok { member } => member,
        Verdict::Old => { let _ = store.raise_floor(&e.header.device, e.header.seq); return Ingest::Old; }
        Verdict::Reject(r) => return Ingest::Rejected(r),
    };
    if store.has_seq(&e.header.device, e.header.seq).unwrap_or(false) && !store.is_gone(&e.header.device, e.header.seq).unwrap_or(false) {
        // same (device, seq) but different bytes: the author signed two entries with one seq
        return Ingest::Rejected("seq reused");
    }
    let body = keys.0.get(&e.header.epoch).and_then(|k| e.open(k).ok());
    if keys.0.contains_key(&e.header.epoch) && body.is_none() { return Ingest::Rejected("cannot open"); }
    // one transaction: insert entry (+ replace a gone placeholder), lamport, then apply the body
    let shown = match store.in_tx(|s| {
        s.insert_entry(&e, &id, bytes, now_ms)?;
        s.bump_lamport(e.header.lamport)?;
        match &body { Some(b) => apply_body(s, roster, &e, &id, &member, b, now_ms), None => Ok(false) }
    }) { Ok(v) => v, Err(_) => return Ingest::Rejected("store") };
    Ingest::Stored { id, shown }
}

/// Text/Fish/File -> messages row (and apply a waiting tombstone for it). React -> reactions (by lamport).
/// Delete -> if target exists and (target member == author member or author is owner): set deleted_by,
/// deleted_ms, text = '', and drop the target's sealed bytes (bytes = x'', gone = 1). If the target is not here
/// yet, nothing else: when the target arrives, apply_body checks `entries_by_ref` for a valid Delete first.
/// Leave -> nothing in messages (the owner's service turns it into RemoveMember + Rotate, D5.1).
/// Reserved -> nothing.
fn apply_body(s: &SpaceStore, roster: &Roster, e: &Entry, id: &EntryId, member: &MemberKey, body: &Body, now_ms: u64)
    -> rusqlite::Result<bool>;
```
Add `pub fn in_tx<T>(&self, f: impl FnOnce(&SpaceStore) -> rusqlite::Result<T>) -> rusqlite::Result<T>` and
`pub fn is_gone(&self, device, seq) -> rusqlite::Result<bool>` to the store.

Reaction rule: `INSERT ... ON CONFLICT(target, member, emoji) DO UPDATE SET is_on = excluded.is_on, lamport =
excluded.lamport, entry = excluded.entry WHERE excluded.lamport > reactions.lamport OR (excluded.lamport =
reactions.lamport AND excluded.entry > reactions.entry)`.

### Tests to write (`spaces/ingest_tests.rs`, owner seed 1, Sam seed 2, epoch key `[9;32]`)
| Test | Input | Expected |
|---|---|---|
| `text_is_stored_and_shown` | ingest Sam's Text "hi" | `Stored{shown:true}`; `messages_page` has 1 row text "hi" |
| `same_bytes_twice_is_duplicate` | ingest twice | second `Duplicate` |
| `seq_reuse_is_rejected` | two different Texts with seq 1 by Sam | second `Rejected("seq reused")` |
| `heads_and_missing` | Sam seqs 1,2,5 stored | `heads == [(sam,1,5)]`; `missing(sam,1,5,10) == [3,4]` |
| `missing_skips_below_floor` | `raise_floor(sam, 3)` | `missing(sam,1,5,10) == [4]` |
| `old_entry_raises_floor` | entry 91 days old seq 7 | `Old`; `author_floor == 7` |
| `delete_by_author_hides_and_drops_bytes` | Text then Sam's Delete of it | message `deleted_by == sam`, text `""`; target entry `gone == 1`, `bytes` empty |
| `delete_by_other_member_is_ignored` | Text by owner, Delete by Sam | message unchanged |
| `delete_by_owner_works_on_anyone` | Text by Sam, Delete by owner | hidden |
| `delete_before_target_applies_when_target_arrives` | Delete first, then Text | Text row is hidden on arrival |
| `reaction_last_lamport_wins` | React on (lamport 5), off (lamport 4) | reaction stays on |
| `entry_for_unknown_epoch_is_stored_but_not_shown` | keys without epoch 1 | `Stored{shown:false}`, no messages row |
| `entries_for_returns_gone_list` | Sam seq 1 deleted (gone), seq 2 normal | `entries_for(sam,[1,2])` gives 1 bytes and gone `[1]` |
| `real_entry_replaces_gone_placeholder` | placeholder (sam,3), then real entry seq 3 | `Stored`, `is_gone == false` |
| `lamport_advances` | ingest entry with lamport 50 | `next_lamport() == 51` |
| `unread_counts_others_only` | 2 by Sam, 1 by me, `mark_read` at first | `unread == 1` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d33 cargo test --lib spaces::ingest_tests
# expected: test result: ok. 16 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d33 cargo test --lib spaces::store_tests
# expected: test result: ok. 9 passed; 0 failed
```

### Done when
- [ ] 16 + 9 tests pass.
- [ ] Every ingest runs in one SQLite transaction.

### Do NOT
- R1-R7.
- Do not serve (`entries_for`) any entry with `gone = 1`.
- Do not trust `created_ms` for ordering in the UI; order by lamport, then created_ms, then id.
- Do not decrypt and keep text of an entry that failed `check`.

### If it fails
1. Foreign key failure inserting `messages`: insert `entries` first in the same transaction.
2. `delete_before_target_applies_when_target_arrives` fails: in `apply_body` for Text, look up `entries` where
   `ref_id = id AND kind = 4`, open each, and apply the first valid one.
3. Placeholder conflicts with `UNIQUE(device, seq)`: delete the placeholder row before inserting the real one.

---

## D3.4 Sync protocol (pure)
Needs merged first: D3.3, D2.6 (plan 22).

### Goal
Add `spaces/sync.rs`: the sync frames and a pure state machine (`SyncSession`) that, given frames from a peer and a
read view of the store, says what to send, what to ingest, and which control ops to apply.

### Read first
- `spaces/frame.rs`, `spaces/join.rs` (`hint_to_bytes`), `spaces/store.rs`, `spaces/ingest.rs`, `spaces/roster.rs`.

### Files
- create `packages/app/src-tauri/src/spaces/sync.rs`
- create `packages/app/src-tauri/src/spaces/sync_tests.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `sync.rs` with frames, `SyncView`, `SyncSession`, `Out`.
2. Tests: a `FakeView` struct over an in-memory `SpaceStore` + `Roster`. Two sessions are wired back to back in the
   tests (A's `Send` outputs are fed to B's `on_frame`, and the reverse), with `Ingest` outputs applied through
   `ingest_entry`.

### Code
Sync frames (on a space stream; a stream's first frame is `Hello` for members):

| Type | Name | Body |
|---|---|---|
| `0x01` | Hello | `u8` version 1, `[16]` space, `u64` control seq, `[32]` control head, `u16` n (≤ 120), n x (`[32]` device, `u64` low, `u64` high), PeerHint (my address) |
| `0x02` | ControlOps | `u16` n (≤ 100), n x `blob` op bytes |
| `0x03` | Want | `u16` n (≤ 64), n x (`[32]` device, `u16` k, k x `u64` seq); total seqs ≤ 500 |
| `0x04` | Entries | `u16` n, n x `blob` entry bytes, `u16` g, g x (`[32]` device, `u64` seq) gone; whole frame ≤ 1 MiB |
| `0x05` | Push | `blob` entry bytes |
| `0x06` | Presence | defined in plan 27 D8.3 |
| `0x07` | AddrBook | `u8` n (≤ 50), n x PeerHint |
| `0x08` | Bye | `u8` reason: 1 normal, 2 control logs differ, 3 not a member, 4 too much traffic |

Rules:

1. Both sides send `Hello` first (`start`).
2. On `Hello`: if `space` differs: `Close(3)`. Compare control logs: if their seq ≤ mine and their head equals my op
   hash at their seq: if their seq < mine, send `ControlOps` with my ops after their seq (≤ 100 per frame, several
   frames). If their seq > mine: wait for their `ControlOps`. If the hashes at the shorter seq differ: `Close(2)`.
3. On `ControlOps`: output `ApplyControl(ops)`; the service applies them through `Roster::apply` and saves them;
   one failing op stops the batch and the session sends `Bye(2)`.
4. Wants: from their heads `(device, low, high)`, for each device known to my roster (current or past), ask
   `missing(device, max(low, my_floor + 1), high, budget)`; total ≤ `MAX_WANT_PER_ROUND` (500). Devices in order of
   largest gap first. Send one `Want` if any seq is missing. Remember each asked `(device, seq)`.
5. On `Want`: reply `Entries` from `entries_for` (split into frames ≤ 1 MiB; never more than 500 entries per Want).
   Seqs I do not have are silently left out.
6. On `Entries`: output `Ingest(bytes list)` and `Gone(list)`; remove them from "asked". When "asked" becomes empty,
   compute the next round (rule 4) using the peer's last heads; stop when nothing is missing.
7. On `Push`: output `Ingest([bytes])` and update the peer's head for that device (`high = max(high, seq)`).
8. Any frame other than these before `Hello`: `Close(3)`. Unknown frame types after `Hello` are ignored (forward
   compatible), but more than 100 unknown frames in a row: `Close(4)`.

```rust
//! Sync between two members over one stream. Pure: no I/O, no clock. The service drives it.
use std::collections::{BTreeMap, BTreeSet};
use super::frame::Frame;
use super::ids::{DeviceKey, NodeKey, SpaceId};
use super::net::PeerHint;
use super::roster::ControlOp;

pub const HELLO: u8 = 0x01;
pub const CONTROL_OPS: u8 = 0x02;
pub const WANT: u8 = 0x03;
pub const ENTRIES: u8 = 0x04;
pub const PUSH: u8 = 0x05;
pub const PRESENCE: u8 = 0x06;
pub const ADDR_BOOK: u8 = 0x07;
pub const BYE: u8 = 0x08;

/// What the session needs to read. Implemented by the service over SpaceStore + Roster.
pub trait SyncView {
    fn space(&self) -> SpaceId;
    fn control_seq(&self) -> u64;
    fn control_head(&self) -> [u8; 32];
    fn control_hash_at(&self, seq: u64) -> Option<[u8; 32]>;
    fn control_ops_after(&self, seq: u64) -> Vec<ControlOp>;
    fn heads(&self) -> Vec<(DeviceKey, u64, u64)>;
    fn floor(&self, device: &DeviceKey) -> u64;
    fn knows_device(&self, device: &DeviceKey) -> bool;
    fn missing(&self, device: &DeviceKey, from: u64, to: u64, max: usize) -> Vec<u64>;
    fn entries_for(&self, device: &DeviceKey, seqs: &[u64]) -> (Vec<Vec<u8>>, Vec<u64>);
    fn my_hint(&self) -> PeerHint;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Send(u8, Vec<u8>),
    Ingest(Vec<Vec<u8>>),
    Gone(Vec<(DeviceKey, u64)>),
    ApplyControl(Vec<ControlOp>),
    Hint(PeerHint),
    Close(u8),
}

pub struct SyncSession {
    pub peer: NodeKey,
    said_hello: bool,
    heard_hello: bool,
    their_heads: BTreeMap<DeviceKey, (u64, u64)>,
    asked: BTreeSet<(DeviceKey, u64)>,
    unknown_in_row: u32,
}

impl SyncSession {
    pub fn new(peer: NodeKey) -> Self;
    pub fn start(&mut self, v: &dyn SyncView) -> Vec<Out>;                 // [Send(HELLO, ..)]
    pub fn on_frame(&mut self, v: &dyn SyncView, f: &Frame) -> Vec<Out>;
    /// Called by the service after it ingested an Entries frame or applied control ops.
    pub fn next_round(&mut self, v: &dyn SyncView) -> Vec<Out>;
    pub fn push_frame(entry_bytes: &[u8]) -> Out;                          // Send(PUSH, blob)
    pub fn is_synced(&self) -> bool;                                       // heard hello and asked is empty
}

pub fn encode_hello(v: &dyn SyncView) -> Vec<u8>;
pub fn decode_hello(body: &[u8]) -> Result<Hello, super::wire::WireError>;
pub struct Hello { pub space: SpaceId, pub control_seq: u64, pub control_head: [u8; 32],
                   pub heads: Vec<(DeviceKey, u64, u64)>, pub hint: PeerHint }
pub fn encode_want(asks: &[(DeviceKey, Vec<u64>)]) -> Vec<u8>;
pub fn decode_want(body: &[u8]) -> Result<Vec<(DeviceKey, Vec<u64>)>, super::wire::WireError>; // rejects > 500 seqs
/// Splits into frames of at most MAX_FRAME_BYTES - 64.
pub fn encode_entries(entries: &[Vec<u8>], gone: &[(DeviceKey, u64)]) -> Vec<Vec<u8>>;
pub fn decode_entries(body: &[u8]) -> Result<(Vec<Vec<u8>>, Vec<(DeviceKey, u64)>), super::wire::WireError>;
```

### Tests to write (`spaces/sync_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `start_sends_hello_first` | `start` | exactly `[Send(HELLO, _)]` |
| `frame_before_hello_closes` | `on_frame(Want)` before Hello | `[Close(3)]` |
| `wrong_space_closes` | Hello with other space | contains `Close(3)` |
| `equal_stores_need_nothing` | A and B same entries | after hellos, no `Want`; `is_synced()` both |
| `b_gets_what_a_has` | A has Sam 1..10, B none | B sends Want for 1..10; after Entries B has 10 |
| `both_ways_at_once` | A has Sam 1..5, B has Ren 1..3 | both end with 8 entries |
| `want_is_capped_at_500` | A has 1200 entries of one author | B's first Want has 500 seqs; total rounds 3; B ends with 1200 |
| `entries_frames_stay_under_one_mib` | 30 entries of 60 KiB each | `encode_entries` gives ≥ 2 frames, each < 1 MiB |
| `behind_control_log_gets_ops` | A at control seq 4, B at 2 (same prefix) | A sends `ControlOps` with seqs 3,4; B outputs `ApplyControl` |
| `forked_control_log_closes` | same seq 2, different op hash | `Close(2)` |
| `push_updates_their_head` | Push of Sam seq 11 | `Ingest` output; their head for Sam high = 11 |
| `gone_seqs_are_reported` | A has Sam 2 gone | B gets `Gone([(sam,2)])` and stops asking for 2 |
| `want_over_500_is_rejected_by_decoder` | body with 501 seqs | `decode_want` `Err` |
| `unknown_device_is_not_asked` | their heads include a device not in B's roster | no Want for it |
| `hundred_unknown_frames_close` | 101 frames of type 0x40 after Hello | last output `Close(4)` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d34 cargo test --lib spaces::sync_tests
# expected: test result: ok. 15 passed; 0 failed
```

### Done when
- [ ] 15 tests pass; `sync.rs` has no `tokio`, no `std::time`, no I/O (it is in `PURE`).

### Do NOT
- R1-R7.
- Do not ask for seqs of devices the roster has never had (a peer could make us store junk).
- Do not trust the peer's `low`/`high` to delete anything; they only guide asking.
- Do not answer a Want with more than 500 entries.

### If it fails
1. Rounds never end: remove received and gone seqs from `asked`, and treat seqs the peer did not send as done for
   this round (they will be asked from another peer).
2. Hello size over 1 MiB: 120 devices x 48 bytes is about 6 KiB; if larger, the roster has more devices than 50 x 2;
   cap n at 120 and send the rest in a second Hello is NOT allowed — drop extra devices with the smallest gap.
3. Control ops apply out of order: `ApplyControl` must keep seq order; sort by seq before output.

---

## D3.5 Spaces service: links, sync, push, joins
Needs merged first: D2.6 (plan 22), D3.4.

### Goal
Add `spaces/service.rs`: load spaces from disk, run the accept and dial loops on `SpaceNet`, run `SyncSession`s,
push and forward new entries, serve joins, keep `SpacePermissions` in step, and send the UI view.

### Read first
- Every `spaces/*.rs` file so far. `app_paths.rs` (`app_data_dir`, facts §9). `pairing_commands.rs` (how the app
  gets the `P2p` and an `AppHandle`, facts §4) and the file found in plan 21 Open facts F-2.

### Files
- create `packages/app/src-tauri/src/spaces/service.rs`
- create `packages/app/src-tauri/src/spaces/links.rs`
- create `packages/app/src-tauri/src/spaces/service_tests.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. `links.rs`: `LinkTable` (one live link per peer node, preferred-link rule, dial backoff).
2. `service.rs`: `SpacesService` and `get_or_start` (code below). Implement the loops described in "Behaviour".
3. Tests on `memnet` with 3 or 4 services in one process, each with its own temp data dir and test keystore.
4. `mod.rs`: `pub mod links; pub mod service; #[cfg(test)] mod service_tests;`.

### Code
```rust
//! Spaces service: owns open spaces, links, sync sessions and joins. One per app.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use serde_json::Value;

use super::ids::{ChannelId, EntryId, NodeKey, SpaceId};
use super::identity::SpaceIdentity;
use super::ingest::EpochKeys;
use super::links::LinkTable;
use super::net::{SpaceNet, SpacePermissions};
use super::roster::Roster;
use super::store::SpaceStore;
use crate::team::keystore::KeyStore;

pub type Notifier = Arc<dyn Fn(&Value) + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct OpenSpace {
    pub id: SpaceId,
    pub store: SpaceStore,
    pub roster: Roster,
    pub keys: EpochKeys,
    pub me: SpaceIdentity,
}

pub struct SpacesService {
    net: Arc<dyn SpaceNet>,
    perms: Arc<SpacePermissions>,
    keystore: KeyStore,
    dir: PathBuf,                     // app_data_dir()/spaces
    clock: Clock,
    spaces: Mutex<BTreeMap<SpaceId, OpenSpace>>,
    links: Mutex<LinkTable>,
    lockouts: Mutex<super::join::Lockouts>,
    joining: Mutex<BTreeMap<SpaceId, JoiningState>>,
    answers: Mutex<BTreeMap<i64, tokio::sync::oneshot::Sender<super::join::Answer>>>,
    notifier: Mutex<Option<Notifier>>,
    message_notifier: Mutex<Option<Notifier>>,
}

pub struct JoiningState { pub space_name: String, pub state: &'static str, pub tries_left: u8, pub answer_by_ms: Option<u64> }

impl SpacesService {
    pub fn new(net: Arc<dyn SpaceNet>, perms: Arc<SpacePermissions>, keystore: KeyStore, dir: PathBuf, clock: Clock)
        -> Arc<Self>;
    /// Opens every `dir/<hex>/space.sqlite`, replays control logs, loads epoch keys from the keystore,
    /// updates permissions, starts the net, spawns the accept loop, the dial loop (every 30 s) and the
    /// housekeeping loop (every 60 s: expire invites and join requests; every 6 h: prune, D3.6).
    pub async fn start(self: &Arc<Self>) -> Result<(), String>;
    pub async fn stop(&self);
    pub fn set_notifier(&self, n: Notifier);
    pub fn set_message_notifier(&self, n: Notifier);
    pub fn view(&self) -> Value;                       // SpacesView JSON (plan 22 D2.7 types), camelCase

    pub async fn create_space(&self, name: &str) -> Result<String, String>;
    pub async fn create_invite(&self, space: &str) -> Result<Value, String>;   // SpaceInvite JSON incl. words + qr
    pub fn cancel_invite(&self, space: &str, invite: &str) -> Result<(), String>;
    pub async fn start_join(self: &Arc<Self>, text: &str, words: &str) -> Result<(), String>;
    pub async fn answer_join(&self, space: &str, request: i64, allow: bool) -> Result<(), String>;
    pub fn cancel_join(&self, space: &str);
    pub async fn send(&self, space: &SpaceId, channel: &ChannelId, body: super::log::Body,
                      refers: Option<EntryId>) -> Result<EntryId, String>;

    fn refresh_permissions(&self);                     // set_nodes(all member nodes + mailboxes), set_open_invites(n)
    fn changed(&self);                                 // notifier(view())
}

/// The app's single service, started lazily by commands::ready. Uses the app's P2p and RoutedPermissions
/// (plan 21 F-2) through P2pSpaceNet::register, the real KeyStore::app(), and app_paths::app_data_dir()/spaces.
pub fn get_or_start(app: &tauri::AppHandle) -> Result<Arc<SpacesService>, String>;
```

Behaviour (each bullet is one function in `service.rs`; names in brackets):

- **Create** (`create_space`): name cleaned (`clean_name`); empty after trim -> `Give the space a name.`;
  `SpaceId::random()`, `ChannelId::random()`, epoch key `random_bytes::<32>()`; store it with
  `keystore.set_epoch_key(&space_tag(id), 1, &key)`; `Roster::genesis`; create the folder and store; append the op;
  meta `role=owner`, `space_id`, `my_member`, `my_device`, `name_in_space`; insert into `spaces`; `changed()`.
  Owner name = the name the user set in Teams if the keystore/config has one; otherwise `"Me"`
  (DECISION-OPEN (default: "Me"; a "Your name in Spaces" setting comes in D4.4)).
- **Invite** (`create_invite`): owner only; count invites with `is_open(now)` < 3; `new_invite(...)` with
  `net.my_hint()` relay and LAN addresses; `keystore.set_invite_code(&space_tag, &invite_hex, &hex(secret))`;
  `invite_put(InviteRecord::new(..))`; `refresh_permissions()`; return JSON `{ id, space, link, words, qr,
  expiresAtMs, state:"open" }`.
- **Cancel invite**: `record.cancel()`, `invite_put`, `keystore.forget_invite(..)`, `refresh_permissions()`.
- **Accept loop** (`accept_loop`): for each link from `net.accept()`: `links.offer(link, dialed=false)`; if kept,
  spawn `serve_link`.
- **Serve link** (`serve_link`): loop `accept_stream()`; for each stream spawn `serve_first_frame` with a 10 s
  timeout on the first frame.
- **First frame** (`serve_first_frame`): if `Hello` and the peer's node is a device of that space: run
  `run_sync(stream, space, peer)`. If `JoinRequest` and the space has an open invite: `join::serve_join` with a
  `ServiceJoinHost` (implements `JoinHost` with the D2.6 `owner_check` + store + keystore + `answers` oneshots).
  Any other case: `link.close(2, "not allowed")`.
- **Sync** (`run_sync`): a `SyncSession`; a writer task that owns `stream.send` and a `mpsc::Receiver<Vec<u8>>`
  per (space, peer) registered in `links` for pushes; read frames, call `on_frame`, execute `Out`s:
  `Ingest` -> `ingest_entry` for each (then `forward` every `Stored`), `ApplyControl` -> apply each through
  `roster.apply`, `store.control_append`, then if an `AddMember`/`Rotate` grant is for me, open and save the epoch
  key; `refresh_permissions()`; `Gone` -> `insert_gone_placeholder`; `Hint` -> save into `peers`; `Close(c)` ->
  write `Bye(c)`, close the stream.
- **Dial loop** (`dial_round`, every 30 s and once at start): for each open space, candidates = member nodes except
  mine, owner first, then by `peers.last_seen_ms` desc; skip nodes in `links` and nodes in backoff; dial until the
  link table has `MAX_LINKS` links. On success `links.offer(link, dialed=true)`; open one stream per common space and
  run `run_sync`. Backoff per node: 5 s, doubling, max 5 minutes; reset on success.
- **Preferred link rule** (`LinkTable::offer`): at most one link per peer. If a second link to the same peer
  arrives, keep the one dialed by the smaller `NodeKey` (compare bytes) and close the other with code 0 `"duplicate"`.
- **Send** (`send`): rate check (D5.4 adds it; until then none); `seq = next_seq(my device)`,
  `lamport = next_lamport()`, `Entry::seal` with the current epoch key; `ingest_entry` locally (same path as remote);
  `forward(None, bytes)`; return id.
- **Forward** (`forward(from: Option<NodeKey>, bytes)`): send `Push` to every live sync writer of that space except
  `from`. Each entry is forwarded at most once (it is forwarded only when `ingest_entry` returned `Stored`).
- **Join (joiner)** (`start_join`): parse link (+ words or QR words) -> errors from plan 22 D2.7 table; check not
  expired; check not already a member; create the folder with meta `role=joining`, `pending_link`; set joining
  state `connecting`; spawn `join::run_joiner`; on `Wait` -> `waiting`; on `Joined` -> save ops, epoch key
  (`set_epoch_key`), meta `role=member`, remove `pending_link`, open the space, dial the hints, `joined`; on
  `Refused` -> map reason to state (`WrongWords`->`wrongWords`, `Gone`->`gone`, `No`->`no`, `LockedOut`->`lockedOut`,
  `Busy`->`busy`, `Full`->`full`, `NoAnswer`->`noAnswer`, `Bad`->`bad`) and delete the folder; on `Unreachable` ->
  `unreachable`, keep the folder for 24 h and retry on next app start while the link is not expired.
- **Answer** (`answer_join`): owner only; `allow` -> `owner_let_in` op, apply, append, invite `answered()`,
  request `let_in`, oneshot `LetIn{ops: all control ops, hints: peers table, max 12}`; else request `refused`,
  invite `answered()`, oneshot `No`. Then `refresh_permissions()`, `changed()`.
- **Housekeeping** (`tick`, every 60 s): invites past expiry -> `Dead`; join requests past `answer_by_ms` ->
  `expired`, invite `timed_out()`, oneshot `NoAnswer`; `refresh_permissions()` if anything changed.

`view()` JSON (camelCase), matching `SpacesView` in `lib/spaces.ts`:
```json
{
  "spaces": [{ "id": "<hex>", "name": "Book club", "isOwner": true, "unread": 3, "members": 4, "online": 2 }],
  "requests": [{ "id": 7, "space": "<hex>", "spaceName": "Book club", "name": "Sam", "short": "4F7Q-9K2M",
                 "receivedAtMs": 0, "answerByMs": 0 }],
  "joining": [{ "space": "<hex>", "spaceName": "Book club", "state": "waiting", "triesLeft": 0, "answerByMs": 0 }],
  "invites": [{ "id": "<hex>", "space": "<hex>", "link": "", "words": [], "qr": "",
                "expiresAtMs": 0, "state": "open" }],
  "online": true,
  "relayed": false
}
```
In `view()` the invites carry empty `link`, `words` and `qr` (they are returned only once, by `create_invite`).
`online` = the net started; `relayed` = every live link is relayed.

### Tests to write (`spaces/service_tests.rs`; `MemoryHub`, services A (owner), B, C, each with a temp dir, a test
keystore (plan 22 Open facts T-1) and a manual clock)
| Test | Input | Expected |
|---|---|---|
| `create_space_makes_owner_and_general` | A `create_space("Book club")` | view has 1 space, `isOwner`, store has 1 op |
| `full_join_through_services` | A invites; B `start_join(link, words)`; A answers allow | B view joining `joined`, then B has the space with 2 members |
| `two_members_see_each_others_messages` | after join, A sends "hi", B sends "yo" | both stores have 2 messages within 2 s |
| `offline_member_catches_up_from_a_third_member` | A, B, C in space; cut A-C; B online; A sends 20 messages; then A goes offline (stop A); C connects to B | C has 20 messages from A |
| `push_is_forwarded_once` | chain A-B-C (cut A-C), A sends 1 message | C stores it once; B forwards once (count `Push` frames written by B = 1) |
| `stranger_first_frame_must_be_join_request` | D (stranger) while an invite is open sends `Hello` first | link closed with code 2; nothing stored |
| `permissions_follow_roster_and_invites` | A: no invites -> D refused; create invite -> D admitted; cancel -> refused | as described |
| `owner_no_answer_expires_after_thirty_minutes` | B asks; clock +30 min + 61 s; tick | B joining `noAnswer`; invite `dead` |
| `duplicate_links_keep_one` | A and B dial each other at once | each side has 1 link to the other |
| `restart_reloads_spaces` | A creates, stop, new service on same dir | view has the space, roster replays |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d35 cargo test --lib spaces::service_tests
# expected: test result: ok. 10 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d35 cargo test --lib spaces
# expected: 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d35 cargo test --lib p2p::guard_tests
# expected: test result: ok. 9 passed; 0 failed
```

### Done when
- [ ] 10 service tests pass, and the whole `spaces` test set passes.
- [ ] `service.rs` and `links.rs` do not name `p2p` (only `get_or_start` calls `net::P2pSpaceNet::register`, which
      lives in `net.rs`; `get_or_start` receives the `P2p` through a function in `net.rs`:
      `pub fn app_space_net(perms) -> Result<Arc<dyn SpaceNet>, String>`; add that function to `net.rs` in this
      slice — that is the 5th file).

### Do NOT
- R1-R7.
- Do not hold the `spaces` mutex across an `.await` (clone what you need, drop the guard, then await).
- Do not forward an entry that was `Duplicate`, `Old` or `Rejected`.
- Do not run any real network in tests; `memnet` only.
- Do not log message text, names, links or keys.

### If it fails
1. Deadlock in tests: a mutex guard is alive across `.await`; restructure into "lock, copy, unlock, await".
2. `offline_member_catches_up...` fails because C never dials B: C's dial candidates come from its roster and
   `peers`; the test must let C learn B's hint from the welcome (hints) or from A's AddrBook.
3. Flaky timing: use the manual clock for expiry and `tokio::time::pause()` only in tests that do not use duplex
   pipes; for the others wait with a loop of `yield_now` up to 2 s.

---

## D3.6 Retention: 90 days or 200 MB
Needs merged first: D3.5.

### Goal
Delete what is older than 90 days, and when a space uses more than 200 MB delete the oldest data until it uses 180
MB, without ever re-downloading what was deleted.

### Read first
- `spaces/store.rs`, `spaces/ingest.rs`, `spaces/service.rs` (housekeeping loop), `spaces/limits.rs`.

### Files
- create `packages/app/src-tauri/src/spaces/retention.rs`
- create `packages/app/src-tauri/src/spaces/retention_tests.rs`
- edit `packages/app/src-tauri/src/spaces/service.rs` (call `prune` at start, every 6 h, and after every 1000
  stored entries)
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `retention.rs` with `prune` doing the 5 steps below in one transaction (step 5 outside).
2. Wire it in the service; tests; `mod.rs`.

### Code
```rust
//! Keep a space small: 90 days or 200 MB, whichever comes first (founder decision 13).
use super::limits::{KEEP_BYTES, KEEP_MS, TRIM_TO_BYTES};
use super::store::SpaceStore;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport { pub old_entries: u64, pub size_entries: u64, pub files: u64, pub bytes_after: u64 }

pub fn used_bytes(s: &SpaceStore) -> rusqlite::Result<u64> {
    let e: i64 = s.conn.query_row("SELECT COALESCE(SUM(size), 0) FROM entries", [], |r| r.get(0))?;
    let f: i64 = s.conn.query_row("SELECT COALESCE(SUM(size), 0) FROM files WHERE have = 1", [], |r| r.get(0))?;
    Ok((e + f) as u64)
}

/// `delete_file` removes a cached file from disk (service passes a closure; tests pass a recorder).
pub fn prune(s: &SpaceStore, now_ms: u64, delete_file: &dyn Fn(&[u8; 32])) -> rusqlite::Result<PruneReport>;
```
Steps of `prune`:
1. **Age.** `cutoff = now_ms - KEEP_MS`. For each device: `UPDATE authors SET floor = MAX(floor, (SELECT
   COALESCE(MAX(seq), 0) FROM entries WHERE device = authors.device AND created_ms < ?cutoff))`. Then
   `DELETE FROM entries WHERE created_ms < ?cutoff` (messages rows go by cascade).
2. **Files.** If `used_bytes > KEEP_BYTES`: select cached files `ORDER BY cached_ms ASC`; for each: `delete_file(sha)`,
   `UPDATE files SET have = 0, cached_ms = NULL`; stop when `used_bytes <= TRIM_TO_BYTES`.
3. **Size.** If still `> KEEP_BYTES`: repeat: take the 500 oldest entries by `created_ms`, raise each device's
   floor to the max seq deleted for that device, delete them; stop when `<= TRIM_TO_BYTES`.
4. **Leftovers.** `DELETE FROM reactions WHERE target NOT IN (SELECT id FROM entries)`;
   `DELETE FROM files WHERE offer NOT IN (SELECT id FROM entries) AND have = 0`.
5. `PRAGMA incremental_vacuum(2000);` (outside the transaction).

Floors make sync never ask for deleted seqs again (D3.3 `missing` skips `seq <= floor`), and ingest already
refuses entries older than 90 days (`Verdict::Old`).

UI text for this (shown in the space's settings, D4.4): `Messages live on members' computers. If everyone is
offline, new messages wait. This computer keeps the last 90 days, up to 200 MB per space.`

### Tests to write (`spaces/retention_tests.rs`)
| Test | Input | Expected |
|---|---|---|
| `entries_older_than_90_days_go` | entries at now-91 d (seq 1-3) and now-1 d (seq 4) | 3 deleted; floor = 3; `missing(dev,1,4,10) == []` |
| `messages_cascade_with_entries` | old Text with messages row | messages row gone |
| `size_rule_trims_to_180_mb` | fake sizes: 250 rows with `size = 1 MB` each (insert rows directly, bytes empty) | `used_bytes <= 180 MB`; oldest deleted first |
| `files_go_before_messages` | 150 MB entries + 60 MB cached files | files removed until ≤ 180 MB; entries untouched; recorder got the file shas |
| `nothing_happens_under_limits` | 10 recent entries | report all zeros |
| `pruned_entry_is_not_reingested` | prune seq 1 by age; then ingest old seq 1 again | `Old` (and not stored) |
| `reactions_to_deleted_targets_go` | reaction on an entry that gets pruned | reactions table empty |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d36 cargo test --lib spaces::retention_tests
# expected: test result: ok. 7 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d36 cargo test --lib spaces
# expected: 0 failed
```

### Done when
- [ ] 7 tests pass; service calls `prune` at start, every 6 h, and every 1000 stored entries.

### Do NOT
- R1-R7.
- Do not delete control ops (the roster must always replay from genesis).
- Do not run `VACUUM` (full) — it blocks; only `incremental_vacuum`.
- Do not delete my own unsent data: there is none (entries are stored before send), so no special case.

### If it fails
1. `incremental_vacuum` inside a transaction errors: run it after commit.
2. `size_rule_trims_to_180_mb` loops forever: if a batch deletes 0 rows, break.
3. Floor update is wrong when entries are not age-ordered by seq: the floor is "max seq deleted", which can skip a
   younger lower seq; that is accepted (it is still stored and served; we only stop asking for it).

---

## Open facts

- NEEDS-FACT S-1: the error type and folder-creation style of `chat_index::store::open` (to copy). Read
  `packages/app/src-tauri/src/chat_index/store.rs` lines 100-140.
- NEEDS-FACT S-2: whether `tempfile` is a dev-dependency. Read `packages/app/src-tauri/Cargo.toml`
  `[dev-dependencies]`.
- NEEDS-FACT S-3: whether `seal_with_key` pads above 16 384 bytes and how (facts §3 lists buckets 1024, 4096, 16384
  only). Read `secure_core/crypto.rs` `seal_with_key` body.
- NEEDS-FACT S-4: where the person's display name lives (Teams config or keystore), to prefill "your name" when
  creating or joining a space. Read `packages/app/src-tauri/src/team/config.rs` (`TeamConfig` fields).
- NEEDS-FACT S-5: how a Tauri `AppHandle` reaches the code that owns the app's `P2p` (for `get_or_start`); see plan
  21 F-2.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Forge a message as another member | Send an entry with Sam's device key | Device signature checked; author must be in the roster | `flipped_byte_breaks_signature`, `stranger_author_is_rejected` |
| Change a message's channel or time | Edit header bytes | Header is AEAD associated data and signed | `flipped_byte_in_header_fails_open` |
| Removed member keeps posting | Old device posts with the old key | `past` devices valid only for epochs before removal | `removed_author_old_epoch_is_ok_new_epoch_rejected` |
| Two different messages with one seq (equivocation) | Hide a message from some members | `seq reused` rejection; first one wins on each PC | `seq_reuse_is_rejected` |
| Fill my disk | Send many large entries | 64 KiB per entry; 200 MB per space; ingest refuses > 90 days old | `oversized_entry_is_refused`, `size_rule_trims_to_180_mb` |
| Make me fetch forever | Advertise huge `high` values | Want ≤ 500 per round; only known devices; rounds end when the peer sends nothing | `want_is_capped_at_500`, `unknown_device_is_not_asked` |
| Suppress messages with fake "gone" | Claim seqs are gone | Gone placeholders are weak; a real entry from another peer replaces them | `real_entry_replaces_gone_placeholder` |
| Fork the control log | Feed two different member lists | Hash chain compared on Hello; mismatch closes | `forked_control_log_closes` |
| Read deleted content after delete | Ask for a deleted entry | Target bytes dropped (`gone`), never served | `entries_for_returns_gone_list` |
| Stranger sends sync frames | Skip the join | First frame must be JoinRequest for non-members | `stranger_first_frame_must_be_join_request` |
| Disk theft | Read the SQLite file | Plaintext text is stored (plan decision P-13); OS disk encryption is the defence | none (documented) |
| A modified client ignores deletes | Keeps showing deleted text | Cannot be prevented; UI says so ("People who already saw it may still have a copy.") | none (documented) |

## What cannot be verified without real hardware or a second computer

- Catch-up time for 90 days of a busy space over a relayed link.
- Behaviour when a laptop sleeps mid-sync and wakes on another network.
- SQLite file size growth on a real disk vs the `size` column estimate (WAL file size is not counted).
- Clock skew between real PCs (the 10-minute future rule) and its effect on message order.
