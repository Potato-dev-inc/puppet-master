# Spaces D-2: identity, invite, roster (control log), join

Read `20-spaces-overview.md` (architecture, merge order, limits) and the shared rules R1-R7 at the top of
`21-spaces-d0-d1-transport-spike-and-secure-core.md`. R1-R7 apply in full to every slice here.

Writer's sources: `00-FORMAT.md` and `_facts/spaces-facts.md` only (facts cited as `facts §N`).

Slices: D2.1, D2.2, D2.3, D2.4, D2.5, D2.6, D2.7, D2.8, D2.9, D2.R (10 slices; D3.1 from plan 23 is merged between
D2.4 and D2.5).

## The invite rules (decided; written once here, used by every slice)

| Rule | Value | Where enforced |
|---|---|---|
| Who can invite | Only the space owner | `space_invite_create` checks `roster.owner == me` |
| Open invites per space | at most 3 (`MAX_OPEN_INVITES`) | `space_invite_create` |
| What an invite is | id (8 random bytes) + secret (11 random bytes = 88 bits, shown as 8 words) + expiry | `invite::new_invite` |
| Lifetime | 24 hours from creation (`INVITE_TTL_MS`) | `InviteRecord::check` |
| Uses | one: the first request with the right words uses it, whatever the owner answers (plan decision P-3) | `InviteRecord::check` |
| Wrong words | each wrong proof is a strike; the 5th strike kills the invite (`INVITE_MAX_STRIKES`) | `InviteRecord::check`, persisted in SQLite |
| Unknown word typed | not a strike; the joiner's own PC says which word is wrong before anything is sent | `words::parse_typed` |
| Stranger lockout | a node with 3 failed joins (any invite, any space on this PC) is refused for 10 minutes (copied from pairing `MAX_STRIKES = 3`, `LOCKOUT_MS = 600_000`, facts §1) | `Lockouts` in `join.rs` |
| Busy rule | at most 20 join requests per minute from all strangers together (`JOINS_PER_MINUTE`) | `Lockouts::busy` |
| Who may connect at all | members' nodes always; strangers only while this PC has at least one open invite (like pairing: "admits only while a code is on screen", facts §1) | `SpacePermissions` |
| What a stranger may send | only `JoinRequest` as the first frame; anything else closes the link | `serve_first_frame` (D3.5) |
| Owner answer window | 30 minutes (`JOIN_WAIT_MS`); no answer = request expires, invite dies | `JoinHost::wait_answer` |
| Space full | 50 members (`MAX_MEMBERS`): request refused with "full" (does not use the invite) | `owner_check` |
| How it is shared | link (text) + 8 words said or sent another way; QR = link + words | `InviteSheet` |
| Joiner's clock check | a link whose expiry is more than 10 minutes in the past by the joiner's clock is refused locally | `JoinSheet` via `space_join` |

---

## D2.1 Word list and invite words
Needs merged first: D1.3.

### Goal
Add a 2048-word list file and `spaces/words.rs` that turns an 88-bit secret into 8 words and back, with typo-friendly
parsing.

### Read first
- `packages/app/src-tauri/src/spaces/mod.rs`, `spaces/wire.rs`.
- Search the repo for an existing BIP-39 English list: `node_modules/**/wordlists/english.json`,
  `node_modules/**/english.txt` (for example from `bip39` or `@scure/bip39`). If found, use its 2048 words.

### Files
- create `packages/app/src-tauri/src/spaces/words/en.txt`
- create `packages/app/src-tauri/src/spaces/words.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `spaces/words/en.txt`: the **BIP-39 English word list**, 2048 lines, one lowercase word per line, sorted,
   LF line endings, final newline. Source: an existing copy found in "Read first"; otherwise the file
   `bip-0039/english.txt` from the `bitcoin/bips` GitHub repository. Do not edit any word.
2. Check: 2048 lines; every word is 3-8 letters `a-z`; the first 4 letters of every word are unique (BIP-39 has
   this property). The test below checks it too.
3. Create `spaces/words.rs` as below. Add `pub mod words;` to `spaces/mod.rs`.
4. Add `words.rs` to nothing else; it is already in the guard's `PURE` list.

Where the list lives and its size, for the record: `packages/app/src-tauri/src/spaces/words/en.txt`, 2048 words,
compiled into the app with `include_str!`. 8 words x 11 bits = 88 bits = exactly 11 bytes; there is no checksum
word (a wrong word gives a wrong secret, which the owner counts as a strike; an unknown word is caught locally).

### Code
`spaces/words.rs`:
```rust
//! Invite words: 11 secret bytes <-> 8 words from the BIP-39 English list (2048 words, 11 bits each).
use std::sync::OnceLock;

static RAW: &str = include_str!("words/en.txt");
pub const WORD_COUNT: usize = 2048;
pub const CODE_WORDS: usize = 8;
pub const SECRET_BYTES: usize = 11;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordsError {
    /// How many words were typed (not 8).
    Count(usize),
    /// 1-based position and the typed word.
    Unknown(usize, String),
}

pub fn list() -> &'static [&'static str] {
    static LIST: OnceLock<Vec<&'static str>> = OnceLock::new();
    LIST.get_or_init(|| RAW.lines().map(str::trim).filter(|w| !w.is_empty()).collect())
}

pub fn encode(secret: &[u8; SECRET_BYTES]) -> [&'static str; CODE_WORDS] {
    let l = list();
    let mut out = [""; CODE_WORDS];
    let (mut acc, mut bits, mut i) = (0u32, 0u32, 0usize);
    for &b in secret {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 11 {
            bits -= 11;
            out[i] = l[((acc >> bits) & 0x7ff) as usize];
            i += 1;
        }
        acc &= (1 << bits) - 1;
    }
    out
}

fn index_of(word: &str) -> Option<usize> {
    let l = list();
    if let Ok(i) = l.binary_search(&word) { return Some(i); }
    // A unique 4-letter (or longer) prefix is accepted: "abandon" can be typed "aban".
    if word.len() >= 4 {
        let hits: Vec<usize> = (0..l.len()).filter(|&i| l[i].starts_with(word)).collect();
        if hits.len() == 1 { return Some(hits[0]); }
    }
    None
}

pub fn decode(words: &[&str]) -> Result<[u8; SECRET_BYTES], WordsError> {
    if words.len() != CODE_WORDS { return Err(WordsError::Count(words.len())); }
    let mut out = [0u8; SECRET_BYTES];
    let (mut acc, mut bits, mut o) = (0u32, 0u32, 0usize);
    for (pos, w) in words.iter().enumerate() {
        let idx = index_of(w).ok_or_else(|| WordsError::Unknown(pos + 1, w.to_string()))?;
        acc = (acc << 11) | idx as u32;
        bits += 11;
        while bits >= 8 {
            bits -= 8;
            out[o] = ((acc >> bits) & 0xff) as u8;
            o += 1;
        }
        acc &= (1 << bits) - 1;
    }
    Ok(out)
}

/// What a person typed: any mix of spaces, dashes, commas, dots and new lines between words; any case.
pub fn parse_typed(text: &str) -> Result<[u8; SECRET_BYTES], WordsError> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || c == '-' || c == ',' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();
    decode(&words)
}

/// Up to 5 list words starting with `prefix` (for the word field's suggestions).
pub fn suggest(prefix: &str) -> Vec<&'static str> {
    let p = prefix.trim().to_lowercase();
    if p.is_empty() { return Vec::new(); }
    list().iter().copied().filter(|w| w.starts_with(&p)).take(5).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_has_2048_sorted_unique_words() {
        let l = list();
        assert_eq!(l.len(), WORD_COUNT);
        assert!(l.windows(2).all(|w| w[0] < w[1]));
        assert!(l.iter().all(|w| (3..=8).contains(&w.len()) && w.bytes().all(|c| c.is_ascii_lowercase())));
    }
    #[test]
    fn first_four_letters_are_unique() {
        let mut p: Vec<&str> = list().iter().map(|w| &w[..w.len().min(4)]).collect();
        p.sort();
        p.dedup();
        assert_eq!(p.len(), WORD_COUNT);
    }
    #[test]
    fn zero_secret_is_eight_times_the_first_word() {
        assert_eq!(encode(&[0u8; 11]), [list()[0]; 8]);
    }
    #[test]
    fn all_ones_secret_is_eight_times_the_last_word() {
        assert_eq!(encode(&[0xffu8; 11]), [list()[2047]; 8]);
    }
    #[test]
    fn round_trip() {
        let s: [u8; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        assert_eq!(decode(&encode(&s)).unwrap(), s);
    }
    #[test]
    fn typed_with_dashes_caps_and_prefixes_works() {
        let s: [u8; 11] = [200, 1, 77, 3, 99, 5, 250, 7, 8, 9, 10];
        let w = encode(&s);
        let typed = format!(
            "{}-{} {},{}\n{} {} {} {}",
            w[0].to_uppercase(), &w[1][..4.min(w[1].len())], w[2], w[3], w[4], w[5], w[6], w[7]
        );
        assert_eq!(parse_typed(&typed).unwrap(), s);
    }
    #[test]
    fn wrong_count_and_unknown_word_are_reported() {
        assert_eq!(parse_typed("one two"), Err(WordsError::Count(2)));
        let w = encode(&[0u8; 11]);
        let typed = format!("{} {} zzzzzz {} {} {} {} {}", w[0], w[1], w[3], w[4], w[5], w[6], w[7]);
        assert_eq!(parse_typed(&typed), Err(WordsError::Unknown(3, "zzzzzz".into())));
    }
    #[test]
    fn suggest_gives_at_most_five() {
        assert!(suggest("a").len() <= 5);
        assert!(suggest("").is_empty());
        assert_eq!(suggest(list()[10]), vec![list()[10]]);
    }
}
```
Note for `suggest(list()[10])`: if another word starts with word 10 (for example "act" and "action"), the test
must use a word that is not a prefix of another; pick the list's word 0 only if it passes, otherwise change the
index to the first index `i` where no other word starts with `list()[i]` (compute it in the test with a loop).

### Tests to write
8 tests above. Expected: `test result: ok. 8 passed; 0 failed`.

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d21 cargo test --lib spaces::words
# expected: test result: ok. 8 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d21 cargo test --lib spaces::guard_tests
# expected: test result: ok. 7 passed; 0 failed
```

### Done when
- [ ] `en.txt` has 2048 words and its SHA-256 matches the value in Open facts W-1 (or W-1 is resolved and the value
      written into this plan).
- [ ] 8 tests pass.
- [ ] The word list's licence is recorded in the commit message (Open facts W-2).

### Do NOT
- R1-R7.
- Do not edit, reorder or "clean up" any word in the list.
- Do not add a checksum word (the format is fixed at 8 words).
- Do not use a different list (EFF, Diceware): the 4-letter-prefix rule is relied on.

### If it fails
1. `list_has_2048_sorted_unique_words` fails with 2049: a blank or BOM line; save as UTF-8 without BOM, LF.
2. `first_four_letters_are_unique` fails: the file is not the BIP-39 English list.
3. `round_trip` fails: bit order bug; `encode` must take the high 11 bits first (big-endian), as written.

---

## D2.2 Identity and device certificate
Needs merged first: D1.2, D1.3.

### Goal
Add `SpaceIdentity` (member key, device key, KEM key, node key) and the per-space `DeviceCert` that ties them
together, signed by the member key.

### Read first
- `packages/app/src-tauri/src/team/keystore.rs` lines 80-110: `member()`, `device_sign()`, `device_kem()`
  (facts §3). Check whether each creates the key on first use or returns an error when missing.
- `packages/app/src-tauri/src/secure_core/sign.rs`, `spaces/wire.rs`, `spaces/ids.rs`.

### Files
- create `packages/app/src-tauri/src/spaces/identity.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `identity.rs` as below. Add `pub mod identity;` to `spaces/mod.rs`.
2. If `KeyStore::member()` does not create a key on first use, `SpaceIdentity::load` returns the error text
   `"Set up your name in Teams first."`; record this in the commit message. (The keystore creates keys elsewhere;
   Spaces does not add a second key creator, guard rule `identity_uses_the_existing_keystore` spirit.)

### Code
DeviceCert byte layout (all big-endian; signature domain `pm/spaces/device-cert/v1`):

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | version = 1 |
| 1 | 16 | space id |
| 17 | 32 | member public key (Ed25519) |
| 49 | 32 | device public key (Ed25519, `device_sign`) |
| 81 | 32 | node id (iroh public key) |
| 113 | 2 + n | `str` KEM public key, base64 exactly as `KemSecret::public_b64()` gives it, n ≤ 64 |
| . | 2 + m | `str` display name, cleaned, m ≤ 160 bytes (≤ 40 chars) |
| . | 8 | issued_ms |
| . | 64 | signature by the member key over every byte before it |

`spaces/identity.rs`:
```rust
//! Who I am in a space: member key (the person), device key (this PC), KEM key (to receive keys), node id.
use ed25519_dalek::SigningKey;

use super::ids::{DeviceKey, MemberKey, NodeKey, SpaceId};
use super::limits::NAME_MAX_CHARS;
use super::wire::{Reader, WireError, Writer};
use crate::secure_core::crypto::KemSecret;
use crate::secure_core::sign;
use crate::team::keystore::KeyStore;

pub const CERT_DOMAIN: &[u8] = b"pm/spaces/device-cert/v1";

pub struct SpaceIdentity {
    member: SigningKey,
    device: SigningKey,
    kem: KemSecret,
    node: NodeKey,
}

impl SpaceIdentity {
    pub fn load(keys: &KeyStore, node: NodeKey) -> Result<Self, String> {
        Ok(Self { member: keys.member()?, device: keys.device_sign()?, kem: keys.device_kem()?, node })
    }
    pub fn from_parts(member: SigningKey, device: SigningKey, kem: KemSecret, node: NodeKey) -> Self {
        Self { member, device, kem, node }
    }
    /// Test helper: deterministic identity from one seed byte.
    #[cfg(test)]
    pub fn for_test(seed: u8) -> Self {
        Self::from_parts(
            SigningKey::from_bytes(&[seed; 32]),
            SigningKey::from_bytes(&[seed.wrapping_add(100); 32]),
            KemSecret::from_bytes([seed.wrapping_add(200); 32]),
            NodeKey([seed; 32]),
        )
    }
    pub fn member_key(&self) -> MemberKey { MemberKey(sign::public_of(&self.member)) }
    pub fn device_key(&self) -> DeviceKey { DeviceKey(sign::public_of(&self.device)) }
    pub fn node(&self) -> NodeKey { self.node }
    pub fn kem(&self) -> &KemSecret { &self.kem }
    pub fn sign_as_member(&self, domain: &[u8], msg: &[u8]) -> [u8; 64] { sign::sign(&self.member, domain, msg) }
    pub fn sign_as_device(&self, domain: &[u8], msg: &[u8]) -> [u8; 64] { sign::sign(&self.device, domain, msg) }

    pub fn certify(&self, space: &SpaceId, name: &str, now_ms: u64) -> DeviceCert {
        let mut c = DeviceCert {
            space: *space,
            member: self.member_key(),
            device: self.device_key(),
            node: self.node,
            kem_b64: self.kem.public_b64(),
            name: clean_name(name),
            issued_ms: now_ms,
            sig: [0u8; 64],
        };
        c.sig = self.sign_as_member(CERT_DOMAIN, &c.signed_part());
        c
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceCert {
    pub space: SpaceId,
    pub member: MemberKey,
    pub device: DeviceKey,
    pub node: NodeKey,
    pub kem_b64: String,
    pub name: String,
    pub issued_ms: u64,
    pub sig: [u8; 64],
}

impl DeviceCert {
    fn signed_part(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(1).fixed(&self.space.0).fixed(&self.member.0).fixed(&self.device.0).fixed(&self.node.0)
            .str(&self.kem_b64).str(&self.name).u64(self.issued_ms);
        w.finish()
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = self.signed_part();
        b.extend_from_slice(&self.sig);
        b
    }
    pub fn from_bytes(b: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(b);
        if r.u8()? != 1 { return Err(WireError("cert version")); }
        let c = DeviceCert {
            space: SpaceId(r.fixed()?),
            member: MemberKey(r.fixed()?),
            device: DeviceKey(r.fixed()?),
            node: NodeKey(r.fixed()?),
            kem_b64: r.str(64)?,
            name: r.str(160)?,
            issued_ms: r.u64()?,
            sig: r.fixed()?,
        };
        r.done()?;
        Ok(c)
    }
    /// Signature is valid AND the name is already clean (a peer cannot smuggle control characters).
    pub fn verify(&self) -> bool {
        self.name == clean_name(&self.name)
            && sign::verify(&self.member.0, CERT_DOMAIN, &self.signed_part(), &self.sig)
    }
}

/// Trim, drop control and bidi-override characters, collapse spaces, cap at 40 chars; empty becomes "Someone".
pub fn clean_name(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|c| !c.is_control() && !matches!(*c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200F}'))
        .collect();
    let collapsed = filtered.split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = collapsed.chars().take(NAME_MAX_CHARS).collect();
    let capped = capped.trim().to_string();
    if capped.is_empty() { "Someone".to_string() } else { capped }
}
```

### Tests to write (in a `#[cfg(test)] mod tests` at the end of `identity.rs`)
| Test | Input | Expected |
|---|---|---|
| `cert_verifies_and_round_trips` | `for_test(1).certify(&SpaceId([2;16]), "Sam", 5)` | `verify()`; `from_bytes(to_bytes()) == cert` |
| `cert_signed_by_other_member_fails` | cert from `for_test(1)`, then `member = for_test(2).member_key()` | `verify() == false` |
| `changed_name_fails` | cert, then `name = "Eve"` | `verify() == false` |
| `unclean_name_in_cert_fails` | cert with name `"Sam\u{202E}"` re-signed by hand over `signed_part()` | `verify() == false` |
| `trailing_byte_is_rejected` | `to_bytes()` + `[0]` | `from_bytes` is `Err` |
| `clean_name_rules` | `"  Sam   Lee \n"`, `"\u{202E}"`, 60 x `"a"` | `"Sam Lee"`, `"Someone"`, 40 x `"a"` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d22 cargo test --lib spaces::identity
# expected: test result: ok. 6 passed; 0 failed
```

### Done when
- [ ] 6 tests pass; guard tests pass (identity.rs is PURE: no fs, no net).

### Do NOT
- R1-R7.
- Do not generate keys in Spaces (`SigningKey::generate`, `new_signing_key`) outside `#[cfg(test)]`.
- Do not use the node key (`p2p_node`) to sign anything; Spaces signs with the member or device key only.

### If it fails
1. `keys.member()` returns `Result<SigningKey, String>` (facts §3); if the error type differs, map with `.map_err`.
2. `KemSecret::from_bytes` signature differs: it is `from_bytes(bytes: [u8; 32])` (facts §3).
3. The `matches!` with char ranges does not compile: replace with explicit `(c as u32)` range checks.

---

## D2.3 Control log (the roster)
Needs merged first: D2.2.

### Goal
Add `spaces/roster.rs`: the owner-signed, hash-linked control log, its byte format, and the replay rules that
produce members, channels, epoch and grants.

### Read first
- `spaces/identity.rs`, `spaces/wire.rs`, `spaces/ids.rs`, `spaces/limits.rs`, `secure_core/grant.rs`.
- Check `sha2` is a direct dependency in `packages/app/src-tauri/Cargo.toml` (facts §5 says it is imported by
  pairing; the version was not read). If it is not a direct dependency, stop and record NEEDS-FACT R-1.

### Files
- create `packages/app/src-tauri/src/spaces/roster.rs`
- create `packages/app/src-tauri/src/spaces/roster_tests.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `roster.rs` with the types and functions below. Bodies not written out here are spelled out in the
   "Apply rules" table; implement exactly those checks in that order.
2. Create `roster_tests.rs` with the tests listed. Add to `mod.rs`: `pub mod roster;` and
   `#[cfg(test)] mod roster_tests;`.
3. Add `"roster_tests.rs"` to nothing; it is a test file (guard `NOT_SHIPPED` does not need it because it has no
   print or env).

### Code
Control op byte layout (signature domain `pm/spaces/control/v1`):

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `PMSC` |
| 4 | 1 | version = 1 |
| 5 | 16 | space id |
| 21 | 8 | seq (genesis = 1, then +1) |
| 29 | 32 | prev hash (SHA-256 of the previous op's full bytes; zeros for genesis) |
| 61 | 8 | created_ms |
| 69 | 1 | op type |
| 70 | 4 + n | `blob` body, n ≤ 60 000 |
| . | 32 | signer (owner member public key) |
| . | 64 | Ed25519 signature over every byte before it |

Op hash = SHA-256 over all bytes including the signature.

Op bodies (all `wire` encoded):

| Type | Name | Body |
|---|---|---|
| 1 | Genesis | `str` space name, `blob` owner DeviceCert, `[8]` first channel id, `str` first channel name (`general`), `blob` SealedKey (epoch 1, owner device) |
| 2 | AddMember | `blob` DeviceCert, `u32` epoch, `blob` SealedKey |
| 3 | RemoveMember | `[32]` member key, `u8` reason (1 = removed by owner, 2 = left) |
| 4 | Rotate | `u32` new epoch, `u16` count, count x `blob` SealedKey |
| 5 | AddChannel | `[8]` channel id, `str` name |
| 6 | RenameChannel | `[8]` channel id, `str` name |
| 7 | ArchiveChannel | `[8]` channel id |
| 8 | RenameSpace | `str` name |
| 9 | SetFish | `[8]` channel id, `u8` mode (0 off, 1 only when called, 2 when it helps) |
| 10 | AddMailbox | `[32]` node id (D9.1) |
| 11 | RemoveMailbox | `[32]` node id (D9.1) |

SealedKey bytes: `[32]` device key, `blob` enc (≤ 64), `blob` ct (≤ 128).
Epoch grant HPKE info: `pm/spaces/epoch/v1` + space id (16) + epoch (u32 big-endian).

```rust
//! The control log: owner-signed, hash-linked ops. Replaying it from genesis gives the roster.
use std::collections::BTreeMap;
use sha2::{Digest, Sha256};

use super::identity::{clean_name, DeviceCert, SpaceIdentity};
use super::ids::{ChannelId, DeviceKey, MemberKey, NodeKey, SpaceId};
use super::limits::*;
use super::wire::{Reader, WireError, Writer};
use crate::secure_core::grant::{open_sealed_key, seal_key_to, SealedKey};
use crate::secure_core::sign;

pub const CONTROL_DOMAIN: &[u8] = b"pm/spaces/control/v1";
pub const MAGIC: &[u8; 4] = b"PMSC";
pub const MAX_MAILBOXES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterError(pub String);
impl From<WireError> for RosterError { fn from(e: WireError) -> Self { RosterError(e.0.into()) } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role { Owner, Member }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FishMode { Off, Called, Helps }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveReason { Removed, Left }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpBody {
    Genesis { name: String, owner: DeviceCert, channel: ChannelId, channel_name: String, grant: SealedKey },
    AddMember { cert: DeviceCert, epoch: u32, grant: SealedKey },
    RemoveMember { member: MemberKey, reason: RemoveReason },
    Rotate { epoch: u32, grants: Vec<SealedKey> },
    AddChannel { channel: ChannelId, name: String },
    RenameChannel { channel: ChannelId, name: String },
    ArchiveChannel { channel: ChannelId },
    RenameSpace { name: String },
    SetFish { channel: ChannelId, mode: FishMode },
    AddMailbox { node: NodeKey },
    RemoveMailbox { node: NodeKey },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlOp {
    pub space: SpaceId,
    pub seq: u64,
    pub prev: [u8; 32],
    pub created_ms: u64,
    pub body: OpBody,
    pub signer: MemberKey,
    pub sig: [u8; 64],
}

impl ControlOp {
    pub fn signed_part(&self) -> Vec<u8>;              // magic..signer, per the table
    pub fn to_bytes(&self) -> Vec<u8>;                 // signed_part + sig
    pub fn from_bytes(b: &[u8]) -> Result<Self, RosterError>; // rejects unknown type, trailing bytes
    pub fn hash(&self) -> [u8; 32] { Sha256::digest(self.to_bytes()).into() }
    pub fn signature_ok(&self) -> bool {
        sign::verify(&self.signer.0, CONTROL_DOMAIN, &self.signed_part(), &self.sig)
    }
}

pub fn encode_body(body: &OpBody) -> (u8, Vec<u8>);
pub fn decode_body(op_type: u8, body: &[u8]) -> Result<OpBody, RosterError>;
pub fn sealed_to_bytes(s: &SealedKey) -> Vec<u8>;
pub fn sealed_from_bytes(b: &[u8]) -> Result<SealedKey, RosterError>;

pub fn epoch_info(space: &SpaceId, epoch: u32) -> Vec<u8> {
    let mut v = b"pm/spaces/epoch/v1".to_vec();
    v.extend_from_slice(&space.0);
    v.extend_from_slice(&epoch.to_be_bytes());
    v
}

/// Channel names: lowercase, spaces become '-', only a-z 0-9 '-', 1..=32 chars, no leading/trailing '-'.
pub fn clean_channel_name(raw: &str) -> Option<String> {
    let s: String = raw.trim().to_lowercase().chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .collect();
    let s = s.trim_matches('-').to_string();
    let s: String = s.chars().take(CHANNEL_NAME_MAX_CHARS).collect();
    if s.is_empty() { None } else { Some(s) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub member: MemberKey,
    pub name: String,
    pub role: Role,
    pub devices: Vec<DeviceCert>,
    pub joined_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel { pub id: ChannelId, pub name: String, pub archived: bool, pub fish: FishMode }

#[derive(Clone, Debug)]
pub struct Roster {
    pub space: SpaceId,
    pub name: String,
    pub owner: MemberKey,
    pub members: BTreeMap<MemberKey, Member>,
    pub channels: Vec<Channel>,
    pub epoch: u32,
    pub seq: u64,
    pub head: [u8; 32],
    pub mailboxes: Vec<NodeKey>,
    /// Every grant ever published, by (epoch, device).
    pub grants: BTreeMap<(u32, DeviceKey), SealedKey>,
    /// Members removed or left, with the epoch that started right after.
    pub gone: BTreeMap<MemberKey, u32>,
    needs_rotate: bool,
}

impl Roster {
    /// Owner creates a space: genesis op + roster. `epoch_key` is the new epoch-1 key.
    pub fn genesis(owner: &SpaceIdentity, space: SpaceId, name: &str, owner_name: &str,
                   first_channel: ChannelId, epoch_key: &[u8; 32], now_ms: u64)
        -> Result<(Roster, ControlOp), RosterError>;

    /// Verify a whole log from genesis. Fails on the first bad op.
    pub fn replay(space: SpaceId, ops: &[ControlOp]) -> Result<Roster, RosterError> {
        let first = ops.first().ok_or_else(|| RosterError("empty log".into()))?;
        let mut r = Roster::from_genesis(space, first)?;
        for op in &ops[1..] { r.apply(op)?; }
        Ok(r)
    }

    fn from_genesis(space: SpaceId, op: &ControlOp) -> Result<Roster, RosterError>;
    pub fn apply(&mut self, op: &ControlOp) -> Result<(), RosterError>;

    /// Owner builds the next op; it is checked by applying it to a clone first.
    pub fn next_op(&self, owner: &SpaceIdentity, body: OpBody, now_ms: u64) -> Result<ControlOp, RosterError> {
        let mut op = ControlOp {
            space: self.space, seq: self.seq + 1, prev: self.head, created_ms: now_ms,
            body, signer: owner.member_key(), sig: [0u8; 64],
        };
        op.sig = owner.sign_as_member(CONTROL_DOMAIN, &op.signed_part());
        self.clone().apply(&op)?;
        Ok(op)
    }

    pub fn device(&self, d: &DeviceKey) -> Option<(&Member, &DeviceCert)>;
    pub fn device_by_node(&self, n: &NodeKey) -> Option<(&Member, &DeviceCert)>;
    pub fn all_devices(&self) -> Vec<&DeviceCert>;
    pub fn member_nodes(&self) -> Vec<NodeKey>;   // every device node of every current member
    pub fn active_channels(&self) -> Vec<&Channel>;
    pub fn channel(&self, id: &ChannelId) -> Option<&Channel>;
    pub fn is_owner(&self, m: &MemberKey) -> bool { *m == self.owner }
    pub fn open_epoch_key(&self, me: &SpaceIdentity, epoch: u32) -> Option<[u8; 32]> {
        let g = self.grants.get(&(epoch, me.device_key()))?;
        open_sealed_key(g, me.kem(), &epoch_info(&self.space, epoch))
    }
    /// Grants of `key` for every current device (owner uses this for Rotate).
    pub fn grants_for_all(&self, epoch: u32, key: &[u8; 32]) -> Result<Vec<SealedKey>, RosterError> {
        self.all_devices().iter()
            .map(|c| seal_key_to(c.device.0, &c.kem_b64, &epoch_info(&self.space, epoch), key)
                .map_err(|_| RosterError("seal failed".into())))
            .collect()
    }
}
```

Apply rules (in `apply`, in this order; the first failing check returns `RosterError(<text>)` with the text shown):

| # | Check | Error text |
|---|---|---|
| 1 | `op.space == self.space` | `wrong space` |
| 2 | `op.seq == self.seq + 1` | `wrong seq` |
| 3 | `op.prev == self.head` | `broken chain` |
| 4 | `op.signer == self.owner` | `not the owner` |
| 5 | `op.signature_ok()` | `bad signature` |
| 6 | if `self.needs_rotate` then body must be `Rotate` | `rotate first` |
| 7 | body `Genesis` here | `second genesis` |
| 8 | `AddMember`: `cert.verify()`, `cert.space == space`, member not present, member not owner, `members.len() < MAX_MEMBERS`, `epoch == self.epoch`, `grant.device == cert.device`, no other member has `cert.node` | `bad cert` / `already a member` / `space full` / `wrong epoch` / `grant mismatch` / `node in use` |
| 9 | `RemoveMember`: member present and not owner; then remove, `gone[member] = self.epoch + 1`, `needs_rotate = true` | `not a member` / `cannot remove owner` |
| 10 | `Rotate`: `epoch == self.epoch + 1`; grants' device set equals exactly the set of current devices (same size, no duplicates, no extras); then `self.epoch += 1`, `needs_rotate = false`, store grants | `wrong epoch` / `grants do not match devices` |
| 11 | `AddChannel`: id not used; cleaned name equals given name; no active channel with that name; active < `MAX_CHANNELS`; total < `MAX_CHANNELS_EVER` | `channel exists` / `bad channel name` / `name taken` / `too many channels` |
| 12 | `RenameChannel`: exists, not archived, name rules as 11 | `no such channel` / `bad channel name` / `name taken` |
| 13 | `ArchiveChannel`: exists, not archived, not `channels[0]` (the first channel) | `no such channel` / `cannot archive the first channel` |
| 14 | `RenameSpace`: `clean_name(name) == name` and not `"Someone"` | `bad space name` |
| 15 | `SetFish`: channel exists and not archived | `no such channel` |
| 16 | `AddMailbox`: not present, `mailboxes.len() < MAX_MAILBOXES`, node not a member device | `mailbox exists` / `too many mailboxes` / `node in use` |
| 17 | `RemoveMailbox`: present | `no such mailbox` |
| 18 | finally `self.seq = op.seq; self.head = op.hash();` | |

`from_genesis` checks: seq 1, prev zeros, `Genesis` body, `owner.verify()`, `owner.space == space`,
`owner.member == op.signer`, signature ok, channel name `general` passes `clean_channel_name`, space name clean,
`grant.device == owner.device`. Result: one member (Owner), one channel, epoch 1, grant (1, owner device).

### Tests to write (`spaces/roster_tests.rs`; helper `fn owner() -> SpaceIdentity { SpaceIdentity::for_test(1) }`,
`fn sam() -> SpaceIdentity { SpaceIdentity::for_test(2) }`, `const NOW: u64 = 1_000_000;`)
| Test | Input | Expected |
|---|---|---|
| `genesis_replays` | `Roster::genesis(...)`, then `Roster::replay(space, &[op])` | 1 member Owner, 1 channel `general`, epoch 1, seq 1 |
| `op_bytes_round_trip` | genesis op `to_bytes` -> `from_bytes` | equal |
| `owner_can_open_epoch_one` | genesis with key `[9;32]` | `open_epoch_key(&owner(), 1) == Some([9;32])` |
| `add_member_then_sam_opens_the_key` | `next_op(AddMember{sam cert, epoch 1, grant to sam})` applied | 2 members; `open_epoch_key(&sam(), 1) == Some([9;32])` |
| `op_signed_by_member_is_rejected` | an AddChannel op signed by `sam()` | `Err("not the owner")` |
| `broken_chain_is_rejected` | op with `prev = [1;32]` re-signed by owner | `Err("broken chain")` |
| `replayed_op_is_rejected` | apply the same op twice | second: `Err("wrong seq")` |
| `flipped_signature_byte_is_rejected` | `sig[0] ^= 1` | `Err("bad signature")` |
| `fifty_first_member_is_rejected` | add 49 test members (seeds 2..=50), then seed 51 | 51st: `Err("space full")` |
| `twenty_first_channel_is_rejected` | add 19 channels `c1..c19`, then `c20` | `Err("too many channels")` |
| `remove_requires_rotate_next` | RemoveMember(sam), then AddChannel | `Err("rotate first")` |
| `rotate_must_cover_exactly_current_devices` | after remove, Rotate with grants incl. sam's device | `Err("grants do not match devices")` |
| `removed_member_cannot_open_new_epoch` | remove sam, rotate with `grants_for_all(2, [8;32])` | `open_epoch_key(&sam(), 2) == None`; owner gets `Some([8;32])` |
| `cannot_remove_owner` | RemoveMember(owner) | `Err("cannot remove owner")` |
| `cannot_archive_first_channel` | ArchiveChannel(channels[0]) | `Err("cannot archive the first channel")` |
| `channel_names_are_cleaned` | `clean_channel_name("  Book Club! ")`, `("---")` | `Some("book-club")`, `None` |
| `unknown_op_type_is_rejected` | `from_bytes` with op type byte 99 | `Err` |
| `node_in_use_is_rejected` | AddMember with a cert whose node equals the owner's node | `Err("node in use")` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d23 cargo test --lib spaces::roster_tests
# expected: test result: ok. 18 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d23 cargo test --lib spaces::guard_tests
# expected: test result: ok. 7 passed; 0 failed
```

### Done when
- [ ] 18 tests pass.
- [ ] `Roster` has no setter that bypasses `apply` (fields are `pub` for reading; nothing outside `roster.rs` assigns
      them — check with a search for `.epoch =`, `.members.insert` outside `roster.rs`).

### Do NOT
- R1-R7.
- Do not use `serde` for this format; it is byte-exact for signatures.
- Do not let members (non-owners) sign control ops in v1.
- Do not wrap or modify `team::roster` (plan decision P-6).

### If it fails
1. `fifty_first_member_is_rejected` fails at 50 because the owner counts: `members.len()` includes the owner, so
   the 50th member (owner + 49) is allowed and the 51st refused; check the test adds 49 then 1.
2. `grants_for_all` order differs between runs: `all_devices` must iterate `members` (a `BTreeMap`) and each
   member's `devices` in stored order.
3. Signature fails after `from_bytes`: `signed_part` must re-encode exactly; encode the body with `encode_body` and
   keep `blob` length prefixes.

---

## D2.4 Invite ticket, invite words, proof, invite rules
Needs merged first: D2.1, D2.3.

### Goal
Add `spaces/invite.rs`: the invite link format, the QR text, the join proof, and the pure invite rules (24 h, one
use, 5 strikes).

### Read first
- `spaces/words.rs`, `spaces/wire.rs`, `spaces/ids.rs`, `secure_core/crypto.rs` (`hmac`, `same_mac`,
  `random_bytes`).
- `team/invite.rs` lines 1-70, for naming style only (facts §3). Do not import from it.

### Files
- create `packages/app/src-tauri/src/spaces/invite.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `invite.rs` as below; add `pub mod invite;` to `spaces/mod.rs`.

### Code
Invite link: `pms1:` + base32 (lowercase, no padding) of this payload:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | version = 1 |
| 1 | 16 | space id |
| 17 | 8 | invite id |
| 25 | 32 | owner node id |
| 57 | 8 | expires_ms |
| 65 | 2 + n | `str` owner relay URL (n ≤ 200; empty = none) |
| . | 1 | LAN address count k (≤ 4) |
| . | k x (2 + m) | `str` `ip:port` (m ≤ 64) |
| . | 2 + s | `str` space name (cleaned, s ≤ 160) |

QR payload: the link, then `#`, then the 8 words joined with `-`:
`pms1:<base32>#word1-word2-word3-word4-word5-word6-word7-word8`.

Join proof: `HMAC-SHA256(key = secret (11 bytes), msg = "pm/spaces/join/v1" || space (16) || invite (8) ||
joiner node (32) || joiner member key (32))`, using `secure_core::crypto::hmac`.

```rust
//! Space invites: link + 8 words. 24 h, one use, 5 wrong tries.
use super::identity::clean_name;
use super::ids::{InviteId, MemberKey, NodeKey, SpaceId};
use super::limits::{INVITE_MAX_STRIKES, INVITE_TTL_MS};
use super::wire::{base32_decode, base32_encode, Reader, Writer};
use super::words::{self, WordsError, SECRET_BYTES};
use crate::secure_core::crypto::{hmac, random_bytes, same_mac};

pub const LINK_PREFIX: &str = "pms1:";
pub const PROOF_DOMAIN: &[u8] = b"pm/spaces/join/v1";
pub const CLOCK_SKEW_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteError { NotALink, Words(WordsError), Expired }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteTicket {
    pub space: SpaceId,
    pub invite: InviteId,
    pub owner_node: NodeKey,
    pub expires_ms: u64,
    pub relay: String,
    pub lan: Vec<String>,
    pub space_name: String,
}

impl InviteTicket {
    pub fn to_link(&self) -> String {
        let mut w = Writer::new();
        w.u8(1).fixed(&self.space.0).fixed(&self.invite.0).fixed(&self.owner_node.0).u64(self.expires_ms)
            .str(&self.relay).u8(self.lan.len().min(4) as u8);
        for a in self.lan.iter().take(4) { w.str(a); }
        w.str(&self.space_name);
        format!("{LINK_PREFIX}{}", base32_encode(&w.finish()))
    }

    /// Accepts the link, or the QR text (link + "#" + words). Leading/trailing spaces are ignored.
    pub fn parse(text: &str) -> Result<(InviteTicket, Option<[u8; SECRET_BYTES]>), InviteError> {
        let t = text.trim();
        let rest = t.strip_prefix(LINK_PREFIX).ok_or(InviteError::NotALink)?;
        let (b32, words_part) = match rest.split_once('#') { Some((a, b)) => (a, Some(b)), None => (rest, None) };
        let bytes = base32_decode(b32).ok_or(InviteError::NotALink)?;
        let mut r = Reader::new(&bytes);
        let bad = |_| InviteError::NotALink;
        if r.u8().map_err(bad)? != 1 { return Err(InviteError::NotALink); }
        let space = SpaceId(r.fixed().map_err(bad)?);
        let invite = InviteId(r.fixed().map_err(bad)?);
        let owner_node = NodeKey(r.fixed().map_err(bad)?);
        let expires_ms = r.u64().map_err(bad)?;
        let relay = r.str(200).map_err(bad)?;
        let k = r.u8().map_err(bad)?;
        if k > 4 { return Err(InviteError::NotALink); }
        let mut lan = Vec::new();
        for _ in 0..k { lan.push(r.str(64).map_err(bad)?); }
        let space_name = clean_name(&r.str(160).map_err(bad)?);
        r.done().map_err(bad)?;
        let secret = match words_part {
            Some(w) => Some(words::parse_typed(w).map_err(InviteError::Words)?),
            None => None,
        };
        Ok((InviteTicket { space, invite, owner_node, expires_ms, relay, lan, space_name }, secret))
    }

    /// Joiner side clock check (10 minutes of skew allowed).
    pub fn check_not_expired(&self, now_ms: u64) -> Result<(), InviteError> {
        if now_ms > self.expires_ms.saturating_add(CLOCK_SKEW_MS) { Err(InviteError::Expired) } else { Ok(()) }
    }
}

pub fn qr_text(ticket: &InviteTicket, secret: &[u8; SECRET_BYTES]) -> String {
    format!("{}#{}", ticket.to_link(), words::encode(secret).join("-"))
}

pub struct NewInvite { pub ticket: InviteTicket, pub secret: [u8; SECRET_BYTES] }

pub fn new_invite(space: SpaceId, owner_node: NodeKey, relay: &str, lan: &[String], space_name: &str, now_ms: u64)
    -> NewInvite
{
    NewInvite {
        ticket: InviteTicket {
            space, invite: InviteId::random(), owner_node, expires_ms: now_ms + INVITE_TTL_MS,
            relay: relay.to_string(), lan: lan.iter().take(4).cloned().collect(), space_name: clean_name(space_name),
        },
        secret: random_bytes::<SECRET_BYTES>(),
    }
}

pub fn join_proof(secret: &[u8; SECRET_BYTES], space: &SpaceId, invite: &InviteId, joiner: &NodeKey,
                  member: &MemberKey) -> [u8; 32] {
    let mut m = PROOF_DOMAIN.to_vec();
    m.extend_from_slice(&space.0);
    m.extend_from_slice(&invite.0);
    m.extend_from_slice(&joiner.0);
    m.extend_from_slice(&member.0);
    hmac(secret, &m)
}

pub fn proof_matches(expected: &[u8; 32], got: &[u8; 32]) -> bool { same_mac(expected, got) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteState { Open, Pending(NodeKey), Used, Dead, Cancelled }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteRecord { pub id: InviteId, pub created_ms: u64, pub expires_ms: u64, pub strikes: u32, pub state: InviteState }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check { Accept, WrongWords { tries_left: u32 }, Gone }

impl InviteRecord {
    pub fn new(id: InviteId, created_ms: u64) -> Self {
        Self { id, created_ms, expires_ms: created_ms + INVITE_TTL_MS, strikes: 0, state: InviteState::Open }
    }
    pub fn is_open(&self, now_ms: u64) -> bool {
        matches!(self.state, InviteState::Open | InviteState::Pending(_)) && now_ms < self.expires_ms
    }
    /// The whole rule set. The caller persists `self` after every call.
    pub fn check(&mut self, proof_ok: bool, joiner: NodeKey, now_ms: u64) -> Check {
        match self.state {
            InviteState::Used | InviteState::Dead | InviteState::Cancelled => return Check::Gone,
            _ => {}
        }
        if now_ms >= self.expires_ms { self.state = InviteState::Dead; return Check::Gone; }
        if let InviteState::Pending(j) = self.state {
            if j != joiner { return Check::Gone; }
        }
        if proof_ok {
            self.state = InviteState::Pending(joiner);
            return Check::Accept;
        }
        self.strikes += 1;
        if self.strikes >= INVITE_MAX_STRIKES { self.state = InviteState::Dead; return Check::Gone; }
        Check::WrongWords { tries_left: INVITE_MAX_STRIKES - self.strikes }
    }
    /// Owner answered (either way): the invite is used (plan decision P-3).
    pub fn answered(&mut self) { self.state = InviteState::Used; }
    pub fn cancel(&mut self) { self.state = InviteState::Cancelled; }
    /// No answer within JOIN_WAIT_MS.
    pub fn timed_out(&mut self) { self.state = InviteState::Dead; }
}
```

### Tests to write (`#[cfg(test)] mod tests` in `invite.rs`)
| Test | Input | Expected |
|---|---|---|
| `link_round_trips` | ticket with relay `https://r.example`, 2 LAN addrs, name `Book club` | `parse(to_link())` gives `(ticket, None)` |
| `qr_text_carries_the_words` | `qr_text(t, [3;11])` | `parse` gives `(t, Some([3;11]))` |
| `link_starts_with_prefix_and_is_lowercase` | `to_link()` | starts with `pms1:`, rest is `[a-z2-7]` only |
| `not_a_link_is_rejected` | `"hello"`, `"pms1:!!!"`, `"pms1:" + base32 of [2]` | `Err(NotALink)` each |
| `expiry_is_24_hours` | `new_invite(.., now=1000)` | `expires_ms == 1000 + 86_400_000` |
| `joiner_clock_allows_ten_minutes_skew` | expires 1000; now 1000+599_999 / 1000+600_001 | `Ok` / `Err(Expired)` |
| `right_words_accept_and_pend` | `check(true, A, now)` | `Accept`, state `Pending(A)` |
| `one_use_other_joiner_gets_gone` | after Accept for A: `check(true, B, now)` | `Gone` |
| `same_joiner_can_reconnect_while_pending` | after Accept for A: `check(true, A, now)` | `Accept` |
| `fifth_wrong_try_kills_the_invite` | 4 x `check(false, A)` then 1 more | `tries_left` 4,3,2,1 then `Gone`, state `Dead` |
| `dead_invite_stays_dead_even_with_right_words` | after 5 strikes: `check(true, A)` | `Gone` |
| `expired_invite_is_gone` | `check(true, A, expires_ms)` | `Gone`, state `Dead` |
| `answered_invite_is_used` | Accept, `answered()`, `check(true, A)` | `Gone` |
| `proof_binds_the_joiner_node` | proof for node A vs node B | different; `proof_matches(a, a)` true |
| `parse_reports_bad_word_in_qr` | QR text with word 2 replaced by `zzzz` | `Err(Words(Unknown(2, "zzzz")))` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d24 cargo test --lib spaces::invite
# expected: test result: ok. 15 passed; 0 failed
```

### Done when
- [ ] 15 tests pass; guard tests pass (invite.rs is PURE).
- [ ] No secret appears in `InviteTicket` or `to_link()` output (test `link_round_trips` gives `None` secret).

### Do NOT
- R1-R7.
- Do not change `team/invite.rs` or its 10-minute TTL (facts §3).
- Do not put the secret in the link. Only the QR text holds the words.
- Do not reset `strikes` anywhere (cowork M5 said a counter that never resets was a bug for the relay; here
  "never resets" is the rule on purpose, because the invite dies).

### If it fails
1. `link_starts_with_prefix_and_is_lowercase` fails: `base32_encode` must output lowercase (D1.3).
2. `hmac` takes `&[u8]` key: pass `secret` as `&secret[..]`.
3. `fifth_wrong_try_kills_the_invite` gives `tries_left` 5 first: strikes must be incremented before computing.

---

(D3.1 `spaces/store.rs` from plan 23 is merged here.)

---

## D2.5 Net seam, in-memory net, SpacePermissions
Needs merged first: D0.2, D2.3, D3.1 (plan 23).

### Goal
Add `spaces/net.rs` (the only Spaces file naming `crate::p2p`) with `SpaceNet`/`SpaceLink`, the real
`P2pSpaceNet`, and `SpacePermissions`; add `spaces/memnet.rs` for tests; prove admission rules on the p2p
`MemoryNet`.

### Read first
- `p2p/capability.rs` (`CapabilityHandler`, `Capability::new`, `RelayPolicy`, `Registry::register`,
  `RoutedPermissions` from D0.2).
- `p2p/endpoint.rs` (`Link`, `Transport`, `PeerAddr`, `admit`, `accept_loop`).
- `p2p/mod.rs` (`P2p::want`, `P2p::dial`, `registry()`).
- The file that constructs the app's `P2p` (Open facts F-2 of plan 21).
- `p2p/tests.rs` top 80 lines: how a `P2p` is built on `MemoryNet` (copy it for `net_tests.rs`).

### Files
- create `packages/app/src-tauri/src/spaces/net.rs`
- create `packages/app/src-tauri/src/spaces/memnet.rs`
- create `packages/app/src-tauri/src/spaces/net_tests.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`
- edit the file that constructs the app's `P2p` (wrap its permissions in `RoutedPermissions` and keep the `Arc`;
  skip if D0.2 already did it)

### Steps
1. Create `net.rs` with the code below. The `SpaceHandler` implements `CapabilityHandler`; copy the trait's method
   signature from `capability.rs` and forward the admitted link into the channel (see NEEDS-FACT N-1).
2. Create `memnet.rs` (test only) and `net_tests.rs`.
3. `mod.rs`: add `pub mod net;`, `#[cfg(test)] pub mod memnet;`, `#[cfg(test)] mod net_tests;`.
4. App `P2p` construction: `let routed = RoutedPermissions::new(old_perms);` pass `routed.clone()` to `P2p::new`,
   store `routed` next to the `P2p` `Arc` (same struct or static the file uses), and expose it with a function
   `pub fn routed_permissions() -> Option<Arc<RoutedPermissions>>` in that file.

### Code
`spaces/net.rs`:
```rust
//! The only Spaces file that names the link layer. Everything above talks to SpaceNet / SpaceLink.
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, Mutex};

use super::ids::NodeKey;
use crate::p2p::capability::{self, Capability, Permissions, RelayPolicy, RoutedPermissions};
use crate::p2p::endpoint::{Link, NodeId, PathKind, PeerAddr};
use crate::p2p::{alpn, P2p};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerHint { pub node: NodeKey, pub relay: String, pub lan: Vec<String> }

pub struct SpaceStream {
    pub send: Box<dyn AsyncWrite + Send + Unpin>,
    pub recv: Box<dyn AsyncRead + Send + Unpin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError { Off, Unreachable, Refused, Closed }

#[async_trait]
pub trait SpaceLink: Send + Sync {
    fn peer(&self) -> NodeKey;
    fn relayed(&self) -> bool;
    async fn open(&self) -> Result<SpaceStream, NetError>;
    async fn accept_stream(&self) -> Result<SpaceStream, NetError>;
    fn close(&self, code: u32, reason: &str);
    async fn closed(&self) -> u32;
}

#[async_trait]
pub trait SpaceNet: Send + Sync {
    fn me(&self) -> Option<NodeKey>;
    fn my_hint(&self) -> Option<PeerHint>;
    async fn start(&self) -> Result<(), NetError>;
    async fn stop(&self);
    async fn dial(&self, peer: &PeerHint) -> Result<Arc<dyn SpaceLink>, NetError>;
    async fn accept(&self) -> Option<Arc<dyn SpaceLink>>;
}

/// Who may open a `spaces` link to this PC: member nodes of my spaces, mailboxes, and anyone while an invite is open.
pub struct SpacePermissions {
    nodes: RwLock<BTreeSet<[u8; 32]>>,
    open_invites: AtomicUsize,
}

impl SpacePermissions {
    pub fn new() -> Arc<Self> { Arc::new(Self { nodes: RwLock::new(BTreeSet::new()), open_invites: AtomicUsize::new(0) }) }
    pub fn set_nodes(&self, nodes: impl IntoIterator<Item = NodeKey>) {
        if let Ok(mut n) = self.nodes.write() { *n = nodes.into_iter().map(|k| k.0).collect(); }
    }
    pub fn set_open_invites(&self, count: usize) { self.open_invites.store(count, Ordering::SeqCst); }
    pub fn is_known(&self, node: &NodeKey) -> bool { self.nodes.read().map(|n| n.contains(&node.0)).unwrap_or(false) }
    pub fn allows(&self, node: &NodeKey) -> bool { self.is_known(node) || self.open_invites.load(Ordering::SeqCst) > 0 }
}

impl Permissions for SpacePermissions {
    fn allowed(&self, peer: &NodeId, cap: &str) -> bool { cap == capability::SPACES && self.allows(&NodeKey(peer.0)) }
    /// A peer never sets permissions; Spaces changes them only through set_nodes / set_open_invites.
    fn set(&self, _peer: &NodeId, _cap: &str, _on: bool) {}
}

struct P2pLink(Arc<dyn Link>);

#[async_trait]
impl SpaceLink for P2pLink {
    fn peer(&self) -> NodeKey { NodeKey(self.0.peer().0) }
    fn relayed(&self) -> bool { matches!(self.0.path(), PathKind::Relayed) } // use the real relayed variant name
    async fn open(&self) -> Result<SpaceStream, NetError> {
        let s = self.0.open_bi().await.map_err(|_| NetError::Closed)?;
        Ok(SpaceStream { send: s.send, recv: s.recv })
    }
    async fn accept_stream(&self) -> Result<SpaceStream, NetError> {
        let s = self.0.accept_bi().await.map_err(|_| NetError::Closed)?;
        Ok(SpaceStream { send: s.send, recv: s.recv })
    }
    fn close(&self, code: u32, reason: &str) { self.0.close(code, reason) }
    async fn closed(&self) -> u32 { self.0.closed().await }
}

pub struct P2pSpaceNet {
    p2p: Arc<P2p>,
    inbox: Mutex<mpsc::Receiver<Arc<dyn Link>>>,
}

impl P2pSpaceNet {
    /// Register the `spaces` capability (relays allowed) and route its permissions to `perms`.
    pub fn register(p2p: Arc<P2p>, routed: &Arc<RoutedPermissions>, perms: Arc<SpacePermissions>)
        -> Result<Arc<Self>, String>
    {
        let (tx, rx) = mpsc::channel(64);
        let handler = Arc::new(SpaceHandler { tx });
        let cap = Capability::new(capability::SPACES, alpn::SPACES, RelayPolicy::/*relay-allowed variant*/Allowed, handler);
        p2p.registry().register(cap).map_err(|e| format!("{e:?}"))?;
        routed.route(capability::SPACES, perms);
        Ok(Arc::new(Self { p2p, inbox: Mutex::new(rx) }))
    }
}

struct SpaceHandler { tx: mpsc::Sender<Arc<dyn Link>> }
// impl capability::CapabilityHandler for SpaceHandler {
//     <copy the trait method that receives an admitted link>: `let _ = self.tx.try_send(link);`
// }

#[async_trait]
impl SpaceNet for P2pSpaceNet {
    fn me(&self) -> Option<NodeKey> { self.p2p.node_id().map(|n| NodeKey(n.0)) }
    fn my_hint(&self) -> Option<PeerHint>; // node_id + home relay of the wanted transport + local_addrs (as strings)
    async fn start(&self) -> Result<(), NetError> {
        self.p2p.want(capability::SPACES).await.map(|_| ()).map_err(|_| NetError::Off)
    }
    async fn stop(&self) { self.p2p.unwant(capability::SPACES).await }
    async fn dial(&self, peer: &PeerHint) -> Result<Arc<dyn SpaceLink>, NetError> {
        let addr = PeerAddr::new(NodeId(peer.node.0)).with_relay(&peer.relay); // + LAN addrs (F-6 of plan 21)
        let link = self.p2p.dial(&addr, capability::SPACES).await.map_err(|_| NetError::Unreachable)?;
        Ok(Arc::new(P2pLink(link)))
    }
    async fn accept(&self) -> Option<Arc<dyn SpaceLink>> {
        let link = self.inbox.lock().await.recv().await?;
        Some(Arc::new(P2pLink(link)))
    }
}
```

`spaces/memnet.rs` (test only, no sockets):
```rust
//! In-process SpaceNet for tests: duplex pipes, permission checks, cut and relayed switches.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex as StdMutex};
use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex};

use super::ids::NodeKey;
use super::net::{NetError, PeerHint, SpaceLink, SpaceNet, SpacePermissions, SpaceStream};

pub struct MemoryHub {
    nodes: StdMutex<BTreeMap<NodeKey, (Arc<SpacePermissions>, mpsc::Sender<Arc<dyn SpaceLink>>)>>,
    cut: StdMutex<BTreeSet<(NodeKey, NodeKey)>>,
    relayed: StdMutex<BTreeSet<(NodeKey, NodeKey)>>,
}

impl MemoryHub {
    pub fn new() -> Arc<Self>;
    pub fn node(self: &Arc<Self>, key: NodeKey, perms: Arc<SpacePermissions>) -> Arc<MemNet>;
    pub fn cut(&self, a: NodeKey, b: NodeKey, on: bool);      // dial between a and b fails; open links close
    pub fn set_relayed(&self, a: NodeKey, b: NodeKey, on: bool);
}

pub struct MemNet { hub: Arc<MemoryHub>, me: NodeKey, inbox: Mutex<mpsc::Receiver<Arc<dyn SpaceLink>>> }

/// One side of a link: streams are tokio::io::duplex(1 << 20) pairs passed over an mpsc channel.
pub struct MemLink { /* peer, relayed flag, stream channels in both directions, closed watch */ }

// impl SpaceNet for MemNet: dial() returns Refused when the target's SpacePermissions::allows(me) is false,
// Unreachable when cut or the target is unknown; otherwise creates a MemLink pair and sends one half to the
// target's inbox. my_hint() = PeerHint { node: me, relay: "", lan: vec![] }.
```

### Tests to write (`spaces/net_tests.rs`, on the p2p `MemoryNet` through `P2pSpaceNet`)
| Test | Input | Expected |
|---|---|---|
| `stranger_is_refused_when_no_invite_is_open` | A registers Spaces, `SpacePermissions` empty, 0 invites; B dials A | B's dial fails or the link closes with `CLOSE_NOT_ALLOWED`; A's `accept()` gets nothing within 500 ms |
| `stranger_is_admitted_while_an_invite_is_open` | A `set_open_invites(1)`; B dials | A's `accept()` returns a link with `peer() == B` |
| `member_node_is_admitted_without_invites` | A `set_nodes([B])` | admitted |
| `closing_the_invite_refuses_new_strangers` | `set_open_invites(1)` then `(0)`; C dials | refused |
| `space_permissions_never_answer_for_other_capabilities` | `allowed(B, "desk/input")` with B known | `false` |
| `peer_set_is_a_no_op` | `set(B, "spaces", true)` on empty perms | `allows(B) == false` |
| `memnet_refuses_unknown_and_cut_peers` | `MemoryHub`: dial unknown key; `cut(A,B)` then dial | `Unreachable` both |
| `memnet_stream_carries_bytes_both_ways` | dial, `open()`, write 5 bytes, other side `accept_stream()` reads 5 | equal bytes |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d25 cargo test --lib spaces::net_tests
# expected: test result: ok. 8 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d25 cargo test --lib spaces::guard_tests
# expected: test result: ok. 7 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d25 cargo test --lib p2p
# expected: 0 failed
```

### Done when
- [ ] 8 tests pass; Spaces and p2p guards pass (`net.rs`, `net_tests.rs` are allow-listed).
- [ ] The app's `P2p` uses `RoutedPermissions`; desk behaviour unchanged (`cargo test --lib desk`: 0 failed).

### Do NOT
- R1-R7.
- Do not name `crate::p2p` in `memnet.rs` (it is not allow-listed).
- Do not call `TrustList`, `PairManager` or `trust_set_permission` (R5; Spaces guard).
- Do not add `spaces` to `PERMISSIONABLE`.

### If it fails
1. `RelayPolicy` has no "allowed" variant name like `Allowed`: use the variant whose `relay_allowed()` returns
   true (read `capability.rs`).
2. `accept()` never receives links: `CapabilityHandler` is not called by `accept_loop` for this ALPN until the
   transport is wanted; call `start()` first in the test.
3. `PathKind::Relayed` does not exist: use the variant the spike printed for a relayed path (D0.4 results).

---

## D2.6 Join protocol (both sides)
Needs merged first: D2.5, D3.1.

### Goal
Add `spaces/frame.rs` (length-prefixed frames on a stream) and `spaces/join.rs` (joiner run, owner checks,
lockouts, welcome), tested end to end on `memnet`.

### Read first
- `spaces/invite.rs`, `spaces/roster.rs`, `spaces/identity.rs`, `spaces/net.rs`, `spaces/memnet.rs`.
- `spaces/store.rs` (D3.1): `invite_get`, `invite_put`, `join_request_add`, `join_request_set`, `control_ops`.
- `team/keystore.rs` lines 116-128 (`invite_code`, `set_invite_code`, `forget_invite`, facts §3).

### Files
- create `packages/app/src-tauri/src/spaces/frame.rs`
- create `packages/app/src-tauri/src/spaces/join.rs`
- create `packages/app/src-tauri/src/spaces/join_tests.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `frame.rs` (whole file below).
2. Create `join.rs` with the frames, `Lockouts`, `JoinHost`, `serve_join`, `run_joiner`, `owner_check`,
   `owner_let_in`, `accept_welcome` below.
3. Create `join_tests.rs`; add `pub mod frame; pub mod join; #[cfg(test)] mod join_tests;` to `mod.rs`.

### Code
`spaces/frame.rs`:
```rust
//! Frames on a Spaces stream: u32 length (of type + body), u8 type, body. Max 1 MiB.
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use super::limits::MAX_FRAME_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame { pub kind: u8, pub body: Vec<u8> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError { TooBig, Empty, Io }

pub async fn write_frame<W: AsyncWrite + Unpin + ?Sized>(w: &mut W, kind: u8, body: &[u8]) -> Result<(), FrameError> {
    let len = body.len() + 1;
    if len > MAX_FRAME_BYTES { return Err(FrameError::TooBig); }
    let mut buf = Vec::with_capacity(4 + len);
    buf.extend_from_slice(&(len as u32).to_be_bytes());
    buf.push(kind);
    buf.extend_from_slice(body);
    w.write_all(&buf).await.map_err(|_| FrameError::Io)?;
    w.flush().await.map_err(|_| FrameError::Io)
}

pub async fn read_frame<R: AsyncRead + Unpin + ?Sized>(r: &mut R) -> Result<Frame, FrameError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await.map_err(|_| FrameError::Io)?;
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 { return Err(FrameError::Empty); }
    if len > MAX_FRAME_BYTES { return Err(FrameError::TooBig); } // checked BEFORE allocating
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await.map_err(|_| FrameError::Io)?;
    Ok(Frame { kind: buf[0], body: buf[1..].to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn frame_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        write_frame(&mut a, 7, b"hi").await.unwrap();
        assert_eq!(read_frame(&mut b).await.unwrap(), Frame { kind: 7, body: b"hi".to_vec() });
    }
    #[tokio::test]
    async fn oversized_length_is_refused_without_allocating() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&(u32::MAX).to_be_bytes()).await.unwrap();
        assert_eq!(read_frame(&mut b).await, Err(FrameError::TooBig));
    }
    #[tokio::test]
    async fn zero_length_is_refused() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&0u32.to_be_bytes()).await.unwrap();
        assert_eq!(read_frame(&mut b).await, Err(FrameError::Empty));
    }
}
```

Join frames (type byte, body):

| Type | Name | Direction | Body |
|---|---|---|---|
| `0x11` | JoinRequest | joiner -> owner | `u8` version 1, `[16]` space, `[8]` invite, `blob` DeviceCert (≤ 1024), `[32]` proof |
| `0x12` | JoinWait | owner -> joiner | `u64` answer-by ms |
| `0x13` | JoinWelcome | owner -> joiner | `u16` n, n x `blob` control op (≤ 60 000 each), `u8` k (≤ 12), k x PeerHint |
| `0x14` | JoinRefused | owner -> joiner | `u8` reason, `u8` tries left |

PeerHint bytes: `[32]` node, `str` relay (≤ 200), `u8` k (≤ 4), k x `str` addr (≤ 64).

Refuse reasons: `1` wrong words, `2` invite gone, `3` owner said no, `4` locked out, `5` busy, `6` space full,
`7` no answer in 30 minutes, `8` bad request.

A JoinWelcome larger than 1 MiB is split: the owner sends several `0x13` frames, each with up to 100 ops; the last
one has `k > 0` hints or the joiner reads until the stream ends. Rule: the joiner stops reading after the frame whose
last op's seq equals the seq of `AddMember` for its own cert.

```rust
//! Joining a space. Joiner: dial owner, send JoinRequest, wait. Owner: check, ask the person, welcome.
use std::collections::BTreeMap;
use async_trait::async_trait;

use super::frame::{read_frame, write_frame, Frame};
use super::identity::{DeviceCert, SpaceIdentity};
use super::ids::{InviteId, NodeKey, SpaceId};
use super::invite::{join_proof, Check, InviteTicket};
use super::limits::*;
use super::net::{NetError, PeerHint, SpaceNet, SpaceStream};
use super::roster::{ControlOp, Roster};
use super::words::SECRET_BYTES;

pub const JOIN_REQUEST: u8 = 0x11;
pub const JOIN_WAIT: u8 = 0x12;
pub const JOIN_WELCOME: u8 = 0x13;
pub const JOIN_REFUSED: u8 = 0x14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefuseReason { WrongWords = 1, Gone = 2, No = 3, LockedOut = 4, Busy = 5, Full = 6, NoAnswer = 7, Bad = 8 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinRequestFrame { pub space: SpaceId, pub invite: InviteId, pub cert: DeviceCert, pub proof: [u8; 32] }
impl JoinRequestFrame { pub fn to_bytes(&self) -> Vec<u8>; pub fn from_bytes(b: &[u8]) -> Result<Self, ()>; }

pub fn hint_to_bytes(h: &PeerHint, w: &mut super::wire::Writer);
pub fn hint_from_bytes(r: &mut super::wire::Reader) -> Result<PeerHint, super::wire::WireError>;

/// Strangers' failed joins, across all spaces on this PC. In memory, at most 256 nodes (oldest dropped).
pub struct Lockouts { by_node: BTreeMap<NodeKey, (u32, u64)>, minute_start: u64, minute_count: u32 }
impl Lockouts {
    pub fn new() -> Self;
    pub fn locked(&self, node: &NodeKey, now_ms: u64) -> bool;   // strikes >= 3 and now < until
    pub fn strike(&mut self, node: NodeKey, now_ms: u64);         // +1; at 3 set until = now + JOIN_LOCKOUT_MS
    pub fn clear(&mut self, node: &NodeKey);                      // after an accepted join
    /// Counts this request; true when over JOINS_PER_MINUTE in the current minute.
    pub fn busy(&mut self, now_ms: u64) -> bool;
}

pub enum Verdict { Wait { request_id: i64, answer_by_ms: u64 }, Refuse(RefuseReason, u8) }
pub enum Answer { LetIn { ops: Vec<ControlOp>, hints: Vec<PeerHint> }, No, NoAnswer }

#[async_trait]
pub trait JoinHost: Send + Sync {
    /// Runs the rules in "owner_check order" below; persists invite state and the request.
    fn check(&self, req: &JoinRequestFrame, joiner: NodeKey, now_ms: u64) -> Verdict;
    /// Resolves when the owner clicks, or after JOIN_WAIT_MS with NoAnswer.
    async fn wait_answer(&self, request_id: i64) -> Answer;
}

/// Owner side, called by the service when a stranger's first frame is JoinRequest.
pub async fn serve_join(mut s: SpaceStream, joiner: NodeKey, first: Frame, host: &dyn JoinHost, now_ms: u64)
    -> Result<(), NetError>
{
    let Ok(req) = JoinRequestFrame::from_bytes(&first.body) else {
        let _ = write_frame(&mut s.send, JOIN_REFUSED, &[RefuseReason::Bad as u8, 0]).await;
        return Ok(());
    };
    match host.check(&req, joiner, now_ms) {
        Verdict::Refuse(r, left) => { let _ = write_frame(&mut s.send, JOIN_REFUSED, &[r as u8, left]).await; }
        Verdict::Wait { request_id, answer_by_ms } => {
            write_frame(&mut s.send, JOIN_WAIT, &answer_by_ms.to_be_bytes()).await.map_err(|_| NetError::Closed)?;
            match host.wait_answer(request_id).await {
                Answer::LetIn { ops, hints } => send_welcome(&mut s, &ops, &hints).await?,
                Answer::No => { let _ = write_frame(&mut s.send, JOIN_REFUSED, &[RefuseReason::No as u8, 0]).await; }
                Answer::NoAnswer => { let _ = write_frame(&mut s.send, JOIN_REFUSED, &[RefuseReason::NoAnswer as u8, 0]).await; }
            }
        }
    }
    Ok(())
}

async fn send_welcome(s: &mut SpaceStream, ops: &[ControlOp], hints: &[PeerHint]) -> Result<(), NetError>;

#[derive(Debug, PartialEq, Eq)]
pub enum JoinOutcome { Joined { roster: Roster, epoch_key: [u8; 32], hints: Vec<PeerHint> }, Refused(RefuseReason, u8) }

#[derive(Debug, PartialEq, Eq)]
pub enum JoinError { Unreachable, BadWelcome(String), Closed }

/// Joiner side. `on_wait` is called once with the answer-by time when the owner's PC says "waiting".
pub async fn run_joiner(net: &dyn SpaceNet, ticket: &InviteTicket, secret: &[u8; SECRET_BYTES],
                        me: &SpaceIdentity, my_name: &str, now_ms: u64, on_wait: &(dyn Fn(u64) + Send + Sync))
    -> Result<JoinOutcome, JoinError>
{
    let hint = PeerHint { node: ticket.owner_node, relay: ticket.relay.clone(), lan: ticket.lan.clone() };
    let link = net.dial(&hint).await.map_err(|_| JoinError::Unreachable)?;
    let mut s = link.open().await.map_err(|_| JoinError::Closed)?;
    let cert = me.certify(&ticket.space, my_name, now_ms);
    let proof = join_proof(secret, &ticket.space, &ticket.invite, &me.node(), &me.member_key());
    let req = JoinRequestFrame { space: ticket.space, invite: ticket.invite, cert: cert.clone(), proof };
    write_frame(&mut s.send, JOIN_REQUEST, &req.to_bytes()).await.map_err(|_| JoinError::Closed)?;
    let mut ops: Vec<ControlOp> = Vec::new();
    loop {
        let f = tokio::time::timeout(
            std::time::Duration::from_millis(JOIN_WAIT_MS + 60_000), read_frame(&mut s.recv)
        ).await.map_err(|_| JoinError::Closed)?.map_err(|_| JoinError::Closed)?;
        match f.kind {
            JOIN_WAIT => on_wait(u64::from_be_bytes(f.body.get(..8).ok_or(JoinError::Closed)?.try_into().unwrap())),
            JOIN_REFUSED => {
                let reason = refuse_reason(*f.body.first().unwrap_or(&8));
                return Ok(JoinOutcome::Refused(reason, *f.body.get(1).unwrap_or(&0)));
            }
            JOIN_WELCOME => {
                let (mut part, hints) = decode_welcome(&f.body).map_err(JoinError::BadWelcome)?;
                ops.append(&mut part);
                if let Some(done) = accept_welcome(ticket, me, &cert, &ops, hints)? { return Ok(done); }
            }
            _ => return Err(JoinError::BadWelcome("unexpected frame".into())),
        }
    }
}

fn refuse_reason(b: u8) -> RefuseReason;
fn decode_welcome(body: &[u8]) -> Result<(Vec<ControlOp>, Vec<PeerHint>), String>;

/// Ok(None) = not complete yet (my AddMember not seen). Checks, in order:
/// 1 `Roster::replay(ticket.space, ops)` succeeds;
/// 2 the owner has a device whose node == ticket.owner_node;
/// 3 there is a member whose device cert equals `my_cert` exactly;
/// 4 `roster.open_epoch_key(me, roster.epoch)` is Some.
pub fn accept_welcome(ticket: &InviteTicket, me: &SpaceIdentity, my_cert: &DeviceCert, ops: &[ControlOp],
                      hints: Vec<PeerHint>) -> Result<Option<JoinOutcome>, JoinError>;
```

`owner_check` order (implemented in the service's `JoinHost::check` in D3.5, and as a pure function here for tests:
`pub fn owner_check(roster: &Roster, invite: &mut InviteRecord, secret: Option<[u8; 11]>, lockouts: &mut Lockouts,
req: &JoinRequestFrame, joiner: NodeKey, now_ms: u64) -> Result<(), (RefuseReason, u8)>`):

| # | Check | Refuse with |
|---|---|---|
| 1 | `lockouts.busy(now)` | `Busy, 0` |
| 2 | `lockouts.locked(joiner, now)` | `LockedOut, 0` |
| 3 | `req.space == roster.space`, `req.cert.verify()`, `req.cert.space == roster.space`, `req.cert.node == joiner` | `Bad, 0` + `lockouts.strike(joiner)` |
| 4 | `roster.members.len() >= MAX_MEMBERS` | `Full, 0` (no strike, invite not used) |
| 5 | invite record missing or `secret` is `None` | `Gone, 0` + strike |
| 6 | `proof_ok = proof_matches(join_proof(secret, ..., joiner, req.cert.member), req.proof)`; `invite.check(proof_ok, joiner, now)` | `WrongWords{n}` -> `(WrongWords, n)` + strike; `Gone` -> `(Gone, 0)` + strike |
| 7 | member key already in roster | `Bad, 0` (no strike) |
| 8 | Accept: `lockouts.clear(joiner)`; return `Ok(())` | |

`owner_let_in(roster, owner, cert, epoch_key, now) -> Result<ControlOp, RosterError>` builds
`AddMember { cert, epoch: roster.epoch, grant: seal_key_to(cert.device.0, &cert.kem_b64, &epoch_info(..), key) }`
with `roster.next_op`.

### Tests to write (`spaces/join_tests.rs`, all on `memnet`, a `ManualHost` test struct implements `JoinHost` with an
in-memory `InviteRecord`, a `Lockouts`, and a channel to send the owner's answer)
| Test | Input | Expected |
|---|---|---|
| `right_words_and_let_in_joins` | owner opens invite, joiner runs with right secret, test sends LetIn | `Joined`, joiner's roster has 2 members, joiner opens epoch key |
| `owner_says_no` | answer `No` | `Refused(No, 0)`; invite state `Used` |
| `wrong_words_report_tries_left` | wrong secret | `Refused(WrongWords, 4)` |
| `fifth_wrong_try_kills_the_invite_end_to_end` | 5 runs with wrong secret from 5 different joiner nodes, then right secret | runs 1-4 `WrongWords` 4..1, run 5 `Gone`, 6th `Gone` |
| `three_failed_joins_lock_the_node_out` | same node, 3 wrong runs, 4th with right words | 4th `Refused(LockedOut, 0)` |
| `lockout_ends_after_ten_minutes` | lockouts with now + 600_001 | not locked |
| `busy_after_twenty_requests_in_a_minute` | 21 `busy(now)` calls | 21st true |
| `cert_for_another_node_is_bad` | joiner sends a cert whose node != its link peer | `Refused(Bad, 0)` |
| `full_space_refuses_without_using_the_invite` | roster with 50 members | `Refused(Full, 0)`, invite still `Open` |
| `welcome_from_wrong_owner_node_is_rejected` | ticket owner_node = X, welcome log's owner device node = Y | `Err(BadWelcome(_))` |
| `tampered_welcome_op_is_rejected` | flip a byte in op 2 of the welcome | `Err(BadWelcome(_))` |
| `no_answer_in_thirty_minutes` | host returns `NoAnswer` | `Refused(NoAnswer, 0)` |
| plus the 3 tests in `frame.rs` | | |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d26 cargo test --lib spaces::join_tests
# expected: test result: ok. 12 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d26 cargo test --lib spaces::frame
# expected: test result: ok. 3 passed; 0 failed
```

### Done when
- [ ] 15 tests pass.
- [ ] `read_frame` checks the length before allocating (test `oversized_length_is_refused_without_allocating`).
- [ ] A refused join never reveals whether the space exists beyond the reason code.

### Do NOT
- R1-R7.
- Do not send the invite secret or words over the network; only the HMAC proof.
- Do not let `check` run when the request's cert node differs from the link's authenticated peer.
- Do not wait forever: every read has a timeout.

### If it fails
1. Tests hang: `ManualHost::wait_answer` never resolves; send the answer before awaiting `run_joiner`, or use
   `tokio::spawn` for the joiner.
2. `accept_welcome` never completes: the joiner compares certs including `issued_ms`; the owner must put the exact
   cert from the request into `AddMember`.
3. `Lockouts` counts the accepted request as a strike: only steps 3, 5, 6 strike.

---

## D2.7 Commands and `lib/spaces.ts` for create, invite, join, answer
Needs merged first: D3.5 (plan 23).

### Goal
Expose create/invite/join/answer to the UI through Tauri commands gated by `spaces_enabled`, and add the TS types,
invoke wrappers and `useSpaces()` hook.

### Read first
- `packages/app/src-tauri/src/pairing_commands.rs` lines 60-210: the `ready(app)` gate, `FEATURE_OFF`, the `Once`
  notifier and the command shape (facts §4).
- `packages/app/src-tauri/src/spaces/service.rs` (D3.5): `SpacesService` methods named below.
- `packages/app/src/lib/desk-pair.ts` whole file (161 lines): types, wrappers, `pairErrorText`, `usePair` (facts §4).
- `packages/app/src-tauri/src/lib.rs` lines 198-210 and 560-570 (handler list, facts §9).

### Files
- create `packages/app/src-tauri/src/spaces/commands.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src/lib/spaces.ts`
- create `packages/app/src/lib/spaces.test.ts`

### Steps
1. Create `commands.rs` with `ready`, the 7 commands below, and the event wiring.
2. `mod.rs`: `pub mod commands;`.
3. `lib.rs`: after `pairing_commands::trust_remove,` add the 7 lines
   `spaces::commands::spaces_view,` `spaces::commands::space_create,` `spaces::commands::space_invite_create,`
   `spaces::commands::space_invite_cancel,` `spaces::commands::space_join,` `spaces::commands::space_join_answer,`
   `spaces::commands::space_join_cancel,`.
4. `lib/spaces.ts`: add the types, wrappers, `spacesErrorText`, `useSpaces` below (keep D0.1's exports).
5. `spaces.test.ts`: tests below (mock `invoke` and `listen` the way `desk-pair` tests do; search
   `desk-pair.test.ts` or any test that mocks `@tauri-apps/api/core`).

### Code
`spaces/commands.rs`:
```rust
//! Tauri commands for Spaces. Every one is gated by Labs `spaces_enabled`. Agents, MCP and the phone have no route here.
use std::sync::Arc;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use super::service::{self, SpacesService};

pub const OFF: &str = "Spaces is off. Turn it on in Settings, under Labs.";

/// Same shape as pairing_commands::ready: Labs check, lazy service start, notifier attached once.
fn ready(app: &AppHandle) -> Result<Arc<SpacesService>, String> {
    // 1. read the public settings JSON exactly the way pairing_commands::ready does;
    //    if !super::labs_on(&public) { return Err(OFF.into()) }
    // 2. let svc = service::get_or_start(app)?;   (D3.5)
    // 3. static ATTACH: std::sync::Once; ATTACH.call_once(|| { let a = app.clone();
    //      svc.set_notifier(Arc::new(move |v: &Value| { let _ = a.emit("spaces://changed", v); }));
    //      let b = app.clone();
    //      svc.set_message_notifier(Arc::new(move |v: &Value| { let _ = b.emit("spaces://message", v); })); });
    // 4. Ok(svc)
    unimplemented!()
}

#[tauri::command]
pub async fn spaces_view(app: AppHandle) -> Result<Value, String> { Ok(ready(&app)?.view()) }

#[tauri::command]
pub async fn space_create(app: AppHandle, name: String) -> Result<Value, String> {
    let svc = ready(&app)?;
    let id = svc.create_space(&name).await?;
    Ok(json!({ "space": id, "view": svc.view() }))
}

#[tauri::command]
pub async fn space_invite_create(app: AppHandle, space: String) -> Result<Value, String> {
    ready(&app)?.create_invite(&space).await
}

#[tauri::command]
pub async fn space_invite_cancel(app: AppHandle, space: String, invite: String) -> Result<Value, String> {
    let svc = ready(&app)?;
    svc.cancel_invite(&space, &invite)?;
    Ok(svc.view())
}

#[tauri::command]
pub async fn space_join(app: AppHandle, text: String, words: String) -> Result<Value, String> {
    let svc = ready(&app)?;
    svc.start_join(&text, &words).await?;
    Ok(svc.view())
}

#[tauri::command]
pub async fn space_join_answer(app: AppHandle, space: String, request: i64, allow: bool) -> Result<Value, String> {
    let svc = ready(&app)?;
    svc.answer_join(&space, request, allow).await?;
    Ok(svc.view())
}

#[tauri::command]
pub async fn space_join_cancel(app: AppHandle, space: String) -> Result<Value, String> {
    let svc = ready(&app)?;
    svc.cancel_join(&space);
    Ok(svc.view())
}
```
Replace `unimplemented!()` with the 4 numbered steps, copying the settings read from `pairing_commands::ready`.

Error strings returned by the service (exact; the UI shows them as they are):

| Case | Text |
|---|---|
| empty space name | `Give the space a name.` |
| not owner tries to invite | `Only the owner of this space can invite people.` |
| 3 invites open | `You already have 3 open invites. Cancel one first.` |
| bad link | `That doesn't look like a Spaces invite link.` |
| words count | `Type all 8 words. You typed {n}.` |
| unknown word | `Word {n} is not on the list. Check the spelling.` |
| expired link (joiner clock) | `This invite has expired. Ask for a new one.` |
| already a member | `You're already in this space.` |
| network off | `Spaces can't go online right now. Check your internet connection.` |

`spaces_view` JSON (camelCase), the type `SpacesView` below. `create_invite` returns `SpaceInvite`.

`lib/spaces.ts` additions:
```ts
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useState } from 'react';

export type PresenceState = 'online' | 'away' | 'offline';
export type FishMode = 'off' | 'called' | 'helps';

export interface SpaceSummary {
  id: string; name: string; isOwner: boolean; unread: number; members: number; online: number;
}
export interface SpaceInvite {
  id: string; space: string; link: string; words: string[]; qr: string;
  expiresAtMs: number; state: 'open' | 'pending' | 'used' | 'dead' | 'cancelled';
}
export interface SpaceJoinRequest {
  id: number; space: string; spaceName: string; name: string; short: string; receivedAtMs: number; answerByMs: number;
}
export type JoinState =
  | 'connecting' | 'waiting' | 'joined' | 'wrongWords' | 'gone' | 'no'
  | 'lockedOut' | 'busy' | 'full' | 'noAnswer' | 'unreachable' | 'bad';
export interface SpaceJoining {
  space: string; spaceName: string; state: JoinState; triesLeft: number; answerByMs: number | null;
}
export interface SpacesView {
  spaces: SpaceSummary[]; requests: SpaceJoinRequest[]; joining: SpaceJoining[];
  invites: SpaceInvite[]; online: boolean; relayed: boolean;
}

export const spacesView = () => invoke<SpacesView>('spaces_view');
export const spaceCreate = (name: string) => invoke<{ space: string; view: SpacesView }>('space_create', { name });
export const spaceInviteCreate = (space: string) => invoke<SpaceInvite>('space_invite_create', { space });
export const spaceInviteCancel = (space: string, invite: string) =>
  invoke<SpacesView>('space_invite_cancel', { space, invite });
export const spaceJoin = (text: string, words: string) => invoke<SpacesView>('space_join', { text, words });
export const spaceJoinAnswer = (space: string, request: number, allow: boolean) =>
  invoke<SpacesView>('space_join_answer', { space, request, allow });
export const spaceJoinCancel = (space: string) => invoke<SpacesView>('space_join_cancel', { space });

/** Text for an error thrown by a Spaces command. Commands already return plain sentences. */
export function spacesErrorText(e: unknown): string {
  if (typeof e === 'string' && e.trim() !== '') return e;
  if (e instanceof Error && e.message.trim() !== '') return e.message;
  return 'Something went wrong. Try again.';
}

/** The joiner's line for each state (used by JoinSheet). */
export function joinStateText(j: SpaceJoining): string {
  switch (j.state) {
    case 'connecting': return "Reaching the owner's computer…";
    case 'waiting': return 'Waiting for the owner to let you in. Keep Puppet Master open. This can take up to 30 minutes.';
    case 'joined': return `You're in ${j.spaceName}.`;
    case 'wrongWords': return `Those words don't match. ${j.triesLeft} ${j.triesLeft === 1 ? 'try' : 'tries'} left.`;
    case 'gone': return 'This invite no longer works. Ask for a new one.';
    case 'no': return 'The owner said no.';
    case 'lockedOut': return 'Too many wrong tries from this computer. Try again in 10 minutes.';
    case 'busy': return "The owner's computer is busy. Try again in a minute.";
    case 'full': return 'This space is full (50 people).';
    case 'noAnswer': return "The owner didn't answer in time. Ask them for a new invite.";
    case 'unreachable': return "Can't reach the owner's computer. They need to be online with Puppet Master open.";
    case 'bad': return "Something in the invite didn't check out. Ask for a new one.";
  }
}

export interface SpacesApi {
  view: SpacesView | null;
  error: string | null;
  create: (name: string) => Promise<string | null>;
  invite: (space: string) => Promise<SpaceInvite | null>;
  cancelInvite: (space: string, invite: string) => Promise<void>;
  join: (text: string, words: string) => Promise<boolean>;
  answer: (space: string, request: number, allow: boolean) => Promise<void>;
  cancelJoin: (space: string) => Promise<void>;
}

export function useSpaces(enabled: boolean): SpacesApi {
  const [view, setView] = useState<SpacesView | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled) { setView(null); return; }
    let stop: (() => void) | null = null;
    let alive = true;
    void listen<SpacesView>('spaces://changed', (e) => { if (alive) setView(e.payload); }).then((u) => {
      if (alive) stop = u; else u();
    });
    spacesView().then((v) => alive && setView(v)).catch((e) => alive && setError(spacesErrorText(e)));
    return () => { alive = false; stop?.(); };
  }, [enabled]);
  const run = useCallback(async <T,>(f: () => Promise<T>): Promise<T | null> => {
    try { setError(null); return await f(); } catch (e) { setError(spacesErrorText(e)); return null; }
  }, []);
  return {
    view, error,
    create: async (name) => (await run(() => spaceCreate(name)))?.space ?? null,
    invite: (space) => run(() => spaceInviteCreate(space)),
    cancelInvite: async (space, invite) => { const v = await run(() => spaceInviteCancel(space, invite)); if (v) setView(v); },
    join: async (text, words) => { const v = await run(() => spaceJoin(text, words)); if (v) setView(v); return v !== null; },
    answer: async (space, request, allow) => { const v = await run(() => spaceJoinAnswer(space, request, allow)); if (v) setView(v); },
    cancelJoin: async (space) => { const v = await run(() => spaceJoinCancel(space)); if (v) setView(v); },
  };
}
```

### Tests to write (`packages/app/src/lib/spaces.test.ts`)
| Test | Input | Expected |
|---|---|---|
| `spacesEnabled is true only for true` | `{spaces_enabled:true}`, `{}`, `null` | `true`, `false`, `false` |
| `spacesErrorText passes command sentences through` | `"Give the space a name."` | same string |
| `spacesErrorText falls back` | `{}` | `Something went wrong. Try again.` |
| `joinStateText wrongWords singular` | `{state:'wrongWords', triesLeft:1}` | `Those words don't match. 1 try left.` |
| `joinStateText covers every state` | each of the 12 states | non-empty string, none contains `undefined` |
| `useSpaces does nothing when disabled` | `renderHook(() => useSpaces(false))` | `invoke` not called |
| `useSpaces loads the view and listens` | enabled; `invoke` resolves a view | `view` equals it; `listen` called with `spaces://changed` |
| `join returns false and sets error on failure` | `invoke` rejects `"That doesn't look like a Spaces invite link."` | returns `false`, `error` is that text |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d27 cargo build --lib
# expected: Finished, 0 errors
CARGO_TARGET_DIR=D:/pm-night/target-d27 cargo test --lib spaces::guard_tests
# expected: test result: ok. 7 passed; 0 failed
cd ../../..
npx vitest run packages/app/src/lib/spaces.test.ts
# expected: Tests  8 passed (8)
```

### Done when
- [ ] 7 commands registered; each returns `OFF` text when `spaces_enabled` is off.
- [ ] 8 Vitest tests pass; Spaces guard passes (no bridge/MCP/phone file mentions `space_join` or `lib/spaces`).

### Do NOT
- R1-R7.
- Do not add these commands to any MCP tool, the bridge, the CLI or the phone route.
- Do not log invite links, words or QR text anywhere (no `tracing`, no `appLogLines`).
- Do not return the invite words from `spaces_view` after the invite sheet is closed; words are returned only by
  `space_invite_create` (the service does not keep them in the view).

### If it fails
1. Tauri cannot find `space_join_answer` args: JS keys must match Rust parameter names (`space`, `request`,
   `allow`); one-word names avoid camelCase conversion issues.
2. `Emitter` import error: copy the `use tauri::...` line from `pairing_commands.rs`.
3. Vitest `listen` mock missing: copy the mock setup from an existing test that mocks `@tauri-apps/api/event`.

---

## D2.8 `InviteSheet` and `JoinRequestSheet`
Needs merged first: D2.7.

### Goal
Add the owner's two sheets: show an invite (link, 8 words, QR, time left, cancel) and answer "Sam wants to join"
with **Let in** / **No**.

### Read first
- `packages/app/src/components/cockpit/Sheet.tsx` (facts §4: `Sheet({ onClose, role, labelledBy, className,
  children })`).
- `packages/app/src/components/desk/AddComputerSheet.tsx` (template; `timeLeft(ms)` at line 17) and
  `components/desk/InboundPairSheet.tsx` (42 lines, template for the inbound sheet).
- `packages/app/src/styles/cockpit-desk.css` (class style `ck-desk-*`, `is-` modifiers, facts §11).
- Search `packages/app/package.json` for a QR library (`qrcode`, `qrcode.react`, `react-qr-code`). See Open facts
  Q-1.

### Files
- create `packages/app/src/components/spaces/InviteSheet.tsx`
- create `packages/app/src/components/spaces/InviteSheet.test.tsx`
- create `packages/app/src/components/spaces/JoinRequestSheet.tsx`
- create `packages/app/src/components/spaces/JoinRequestSheet.test.tsx`
- create `packages/app/src/styles/cockpit-spaces.css`

### Steps
1. Create `cockpit-spaces.css` with the classes listed in plan 24 section "CSS" (create the file with the sheet
   classes now: `ck-spaces-sheet`, `ck-spaces-sheet-title`, `ck-spaces-sheet-body`, `ck-spaces-field`,
   `ck-spaces-words`, `ck-spaces-word`, `ck-spaces-qr`, `ck-spaces-note`, `ck-spaces-actions`, `ck-spaces-btn`,
   `ck-spaces-btn is-primary`, `ck-spaces-btn is-danger`, `ck-spaces-hint`, `ck-spaces-hint is-error`).
2. Create `InviteSheet.tsx` and `JoinRequestSheet.tsx` exactly as below. Import `timeLeft` from
   `../desk/AddComputerSheet`.
3. QR: if a QR library exists in `packages/app/package.json`, render `invite.qr` with it inside
   `<div className="ck-spaces-qr">` at 192 px. If none exists, do not render the "Show QR code" button at all.
4. Write the tests.

### Code
`InviteSheet.tsx`:
```tsx
import { useId, useState } from 'react';
import { Sheet } from '../cockpit/Sheet';
import { timeLeft } from '../desk/AddComputerSheet';
import type { SpaceInvite } from '../../lib/spaces';
import '../../styles/cockpit-spaces.css';

export interface InviteSheetProps {
  spaceName: string;
  invite: SpaceInvite;
  nowMs: number;
  onCancelInvite: () => void;
  onClose: () => void;
  /** Present only when a QR library exists (see plan 22 D2.8 step 3). */
  renderQr?: (text: string) => JSX.Element;
}

export function InviteSheet({ spaceName, invite, nowMs, onCancelInvite, onClose, renderQr }: InviteSheetProps) {
  const titleId = useId();
  const [copied, setCopied] = useState(false);
  const [showQr, setShowQr] = useState(false);
  const left = Math.max(0, invite.expiresAtMs - nowMs);
  const open = invite.state === 'open' && left > 0;
  const copy = async () => {
    try { await navigator.clipboard.writeText(invite.link); setCopied(true); } catch { setCopied(false); }
  };
  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={onClose} className="ck-spaces-sheet">
      <h2 id={titleId} className="ck-spaces-sheet-title">Invite someone to {spaceName}</h2>
      {open ? (
        <>
          <p className="ck-spaces-sheet-body">
            Send them this link. Then tell them the 8 words another way: say them out loud, or send them in a
            different app.
          </p>
          <label className="ck-spaces-field">
            <span>Invite link</span>
            <input readOnly value={invite.link} onFocus={(e) => e.currentTarget.select()} />
          </label>
          <div className="ck-spaces-actions">
            <button type="button" className="ck-spaces-btn" onClick={() => void copy()}>
              {copied ? 'Copied' : 'Copy link'}
            </button>
            {renderQr && (
              <button type="button" className="ck-spaces-btn" onClick={() => setShowQr((v) => !v)}>
                {showQr ? 'Hide QR code' : 'Show QR code'}
              </button>
            )}
          </div>
          <p className="ck-spaces-note">Invite words</p>
          <ol className="ck-spaces-words" aria-label="Invite words">
            {invite.words.map((w, i) => (
              <li key={i} className="ck-spaces-word">{w}</li>
            ))}
          </ol>
          {renderQr && showQr && (
            <div className="ck-spaces-qr">
              {renderQr(invite.qr)}
              <p className="ck-spaces-hint">
                Scan this with the other computer's camera. The code holds the link and the words.
              </p>
            </div>
          )}
          <p className="ck-spaces-hint">
            Works once. Expires in {timeLeft(left)}. After 5 wrong tries it stops working.
          </p>
          <div className="ck-spaces-actions">
            <button type="button" className="ck-spaces-btn is-danger" onClick={onCancelInvite}>Cancel invite</button>
            <button type="button" className="ck-spaces-btn is-primary" onClick={onClose}>Done</button>
          </div>
        </>
      ) : (
        <>
          <p className="ck-spaces-sheet-body">{inviteEndedText(invite, left)}</p>
          <div className="ck-spaces-actions">
            <button type="button" className="ck-spaces-btn is-primary" onClick={onClose}>Close</button>
          </div>
        </>
      )}
    </Sheet>
  );
}

export function inviteEndedText(invite: SpaceInvite, leftMs: number): string {
  switch (invite.state) {
    case 'pending': return 'Someone used this invite. Answer their request to finish.';
    case 'used': return 'This invite was used.';
    case 'dead': return leftMs <= 0 ? 'This invite expired.' : 'This invite stopped working after 5 wrong tries.';
    case 'cancelled': return 'Invite cancelled.';
    case 'open': return 'This invite expired.';
  }
}
```

`JoinRequestSheet.tsx`:
```tsx
import { useId } from 'react';
import { Sheet } from '../cockpit/Sheet';
import type { SpaceJoinRequest } from '../../lib/spaces';
import '../../styles/cockpit-spaces.css';

export interface JoinRequestSheetProps {
  request: SpaceJoinRequest | null;
  onAnswer: (space: string, request: number, allow: boolean) => void;
}

/** Always mounted; shows only when a request is waiting (like InboundPairSheet). Closing = No. */
export function JoinRequestSheet({ request, onAnswer }: JoinRequestSheetProps) {
  const titleId = useId();
  if (!request) return null;
  const no = () => onAnswer(request.space, request.id, false);
  return (
    <Sheet labelledBy={titleId} onClose={no} className="ck-spaces-sheet">
      <h2 id={titleId} className="ck-spaces-sheet-title">{request.name} wants to join {request.spaceName}</h2>
      <p className="ck-spaces-sheet-body">They used your invite and typed the right words.</p>
      <p className="ck-spaces-note">Their computer: {request.short}</p>
      <p className="ck-spaces-hint">
        Only let in people you know. They will be able to read this space's recent messages.
      </p>
      <div className="ck-spaces-actions">
        <button type="button" className="ck-spaces-btn" onClick={no}>No</button>
        <button type="button" className="ck-spaces-btn is-primary"
          onClick={() => onAnswer(request.space, request.id, true)}>Let in</button>
      </div>
    </Sheet>
  );
}
```

### Tests to write
`InviteSheet.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `shows link, 8 numbered words and the rules` | open invite, words `a..h`, expires in 2 h | title `Invite someone to Book club`; 8 `li`; text `Works once. Expires in` and `After 5 wrong tries it stops working.` |
| `copy link writes only the link` | click `Copy link` (mock clipboard) | clipboard text equals `invite.link`, not containing `#`; button text `Copied` |
| `no QR button without a QR renderer` | `renderQr` undefined | no button `Show QR code` |
| `QR shows the qr text` | `renderQr={(t) => <span data-testid="qr">{t}</span>}`, click `Show QR code` | `qr` testid text equals `invite.qr` |
| `cancel invite calls back` | click `Cancel invite` | `onCancelInvite` called once |
| `dead invite after strikes explains why` | state `dead`, not expired | `This invite stopped working after 5 wrong tries.` |

`JoinRequestSheet.test.tsx`:
| Test | Input | Expected |
|---|---|---|
| `renders nothing without a request` | `request={null}` | container empty |
| `shows who and the short id` | name `Sam`, space `Book club`, short `4F7Q-9K2M` | title `Sam wants to join Book club`; `Their computer: 4F7Q-9K2M` |
| `Let in answers true` | click `Let in` | `onAnswer('s1', 7, true)` |
| `No answers false` | click `No` | `onAnswer('s1', 7, false)` |
| `Escape answers false` | press Escape | `onAnswer('s1', 7, false)` |

### Run
```bash
npx vitest run packages/app/src/components/spaces
# expected: Tests  11 passed (11)
```

### Done when
- [ ] 11 tests pass.
- [ ] The sheets are not mounted anywhere yet; D4.2 mounts them in `SpacesPage` with a page-level test.

### Do NOT
- R1-R7.
- No colour: only the monochrome tokens used in `cockpit-desk.css` (UI rule, facts §11; `docs/design/`).
- Do not put the words in the copied text.
- Do not add a QR npm package in this slice (Open facts Q-1 decides).

### If it fails
1. `timeLeft` is not exported the way the test expects: it is `export function timeLeft(ms: number): string`
   (facts §4); import by name.
2. `Sheet` needs `role` for Escape handling: keep default `alertdialog` for `JoinRequestSheet` and `dialog` for
   `InviteSheet`, as written.
3. `navigator.clipboard` is undefined in the test environment: define it with `Object.assign(navigator,
   { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } })`.

---

## D2.9 `JoinSheet`
Needs merged first: D2.8.

### Goal
Add the joiner's sheet: paste link (or QR text), type 8 words, ask to join, and see each join state in plain words.

### Read first
- `packages/app/src/components/spaces/InviteSheet.tsx` (style), `lib/spaces.ts` (`joinStateText`, `SpaceJoining`).
- `packages/app/src/components/desk/AddComputerSheet.tsx` (code field handling).

### Files
- create `packages/app/src/components/spaces/JoinSheet.tsx`
- create `packages/app/src/components/spaces/JoinSheet.test.tsx`
- edit `packages/app/src/styles/cockpit-spaces.css` (only if a class below is missing)

### Steps
1. Create `JoinSheet.tsx` as below. The local checks (link shape, word count) only guide; the backend decides.
2. Write the tests.

### Code
```tsx
import { useId, useMemo, useState } from 'react';
import { Sheet } from '../cockpit/Sheet';
import { joinStateText, type SpaceJoining } from '../../lib/spaces';
import '../../styles/cockpit-spaces.css';

export interface JoinSheetProps {
  joining: SpaceJoining | null;
  error: string | null;
  onJoin: (text: string, words: string) => void;
  onCancel: (space: string | null) => void;
  onOpenSpace: (space: string) => void;
}

/** Splits like the backend: spaces, dashes, commas, dots, new lines. */
export function countWords(text: string): number {
  return text.toLowerCase().split(/[\s\-,.]+/).filter((w) => w !== '').length;
}

export function JoinSheet({ joining, error, onJoin, onCancel, onOpenSpace }: JoinSheetProps) {
  const titleId = useId();
  const [link, setLink] = useState('');
  const [words, setWords] = useState('');
  const qrWords = useMemo(() => (link.includes('#') ? link.split('#')[1] ?? '' : ''), [link]);
  const effectiveWords = words.trim() === '' ? qrWords : words;
  const n = countWords(effectiveWords);
  const linkOk = link.trim().startsWith('pms1:');
  const busy = joining?.state === 'connecting' || joining?.state === 'waiting';
  const canAsk = linkOk && n === 8 && !busy;
  if (joining?.state === 'joined') {
    return (
      <Sheet labelledBy={titleId} role="dialog" onClose={() => onCancel(null)} className="ck-spaces-sheet">
        <h2 id={titleId} className="ck-spaces-sheet-title">{joinStateText(joining)}</h2>
        <div className="ck-spaces-actions">
          <button type="button" className="ck-spaces-btn is-primary" onClick={() => onOpenSpace(joining.space)}>
            Open space
          </button>
        </div>
      </Sheet>
    );
  }
  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={() => onCancel(joining?.space ?? null)} className="ck-spaces-sheet">
      <h2 id={titleId} className="ck-spaces-sheet-title">Join a space</h2>
      <label className="ck-spaces-field">
        <span>Invite link</span>
        <input value={link} placeholder="pms1:…" disabled={busy} onChange={(e) => setLink(e.target.value)} />
        <small className="ck-spaces-hint">Paste the link you got.</small>
      </label>
      {link.trim() !== '' && !linkOk && (
        <p className="ck-spaces-hint is-error">That doesn't look like a Spaces invite link.</p>
      )}
      <label className="ck-spaces-field">
        <span>Invite words</span>
        <textarea rows={2} value={words} placeholder={qrWords !== '' ? 'Filled in from the QR code' : '8 words, in order'}
          disabled={busy} onChange={(e) => setWords(e.target.value)} />
        <small className="ck-spaces-hint">Type the 8 words you were told. Spaces or dashes between them are fine.</small>
      </label>
      {effectiveWords.trim() !== '' && n !== 8 && (
        <p className="ck-spaces-hint">Type all 8 words. You typed {n}.</p>
      )}
      {joining && <p className={`ck-spaces-hint${joining.state === 'waiting' || joining.state === 'connecting' ? '' : ' is-error'}`}
        role="status">{joinStateText(joining)}</p>}
      {error && <p className="ck-spaces-hint is-error" role="alert">{error}</p>}
      <div className="ck-spaces-actions">
        <button type="button" className="ck-spaces-btn" onClick={() => onCancel(joining?.space ?? null)}>Cancel</button>
        <button type="button" className="ck-spaces-btn is-primary" disabled={!canAsk}
          onClick={() => onJoin(link.trim(), effectiveWords)}>Ask to join</button>
      </div>
    </Sheet>
  );
}
```

### Tests to write (`JoinSheet.test.tsx`)
| Test | Input | Expected |
|---|---|---|
| `ask is disabled until link and 8 words` | empty; then link only; then 7 words; then 8 | disabled, disabled, disabled + `Type all 8 words. You typed 7.`, enabled |
| `qr text fills the words` | link `pms1:abc#w1-w2-w3-w4-w5-w6-w7-w8`, words empty | `Ask to join` enabled; `onJoin` gets words `w1-w2-w3-w4-w5-w6-w7-w8` |
| `bad link shows the plain error` | link `hello` | `That doesn't look like a Spaces invite link.` |
| `waiting state disables fields and explains` | `joining.state='waiting'` | inputs disabled; text starts `Waiting for the owner to let you in.` |
| `wrong words shows tries left` | `state='wrongWords', triesLeft=3` | `Those words don't match. 3 tries left.` |
| `joined shows open space` | `state='joined', spaceName='Book club'` | title `You're in Book club.`; click `Open space` calls `onOpenSpace('s1')` |
| `cancel while waiting cancels that join` | waiting for `s1`, click `Cancel` | `onCancel('s1')` |
| `countWords splits like the backend` | `'a-b, c.d\ne  f g h'` | `8` |

### Run
```bash
npx vitest run packages/app/src/components/spaces/JoinSheet.test.tsx
# expected: Tests  8 passed (8)
```

### Done when
- [ ] 8 tests pass. Not mounted yet (D4.2 mounts it).

### Do NOT
- R1-R7.
- Do not validate words against the word list in TS (the backend does it and names the bad word).
- Do not store the link or words in `localStorage`.

### If it fails
1. `onJoin` receives untrimmed text: the test expects `link.trim()`.
2. Disabled `textarea` cannot be found by label: query by role `textbox` with name `Invite words`.
3. The `is-error` class on waiting: waiting/connecting must not get `is-error`.

---

## D2.R Review of D-2 (Sonnet, read-only)
Needs merged first: D2.9.

### Goal
A Sonnet session reads every D-2 change and runs every D-2 test, and reports pass/fail against the checklist; it
changes no code.

### Read first
All files created or edited in D2.1-D2.9 and D3.1, plus this plan.

### Files
None (read-only). The report goes in the handoff message, not a file.

### Steps
1. Run every `Run` command from D2.1-D2.9.
2. Walk the checklist below; for each item write `PASS` or `FAIL: <file>:<line> <what>`.

### Code
None.

### Tests to write
None.

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d2r cargo test --lib spaces
# expected: 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d2r cargo test --lib p2p::guard_tests
# expected: test result: ok. 9 passed; 0 failed
cd ../../..
npx vitest run packages/app/src/lib/spaces.test.ts packages/app/src/components/spaces
# expected: Tests  27 passed (27)
```

### Done when (the checklist)
- [ ] Invite secret is stored only in the keystore (`set_invite_code`), never in SQLite, logs or the view.
- [ ] `InviteRecord.strikes` is persisted after every check (D3.1 `invite_put` called in `JoinHost::check`).
- [ ] A stranger's first frame other than `JoinRequest` closes the link (D3.5 `serve_first_frame`).
- [ ] `JoinRequestFrame.cert.node` is compared with the authenticated link peer.
- [ ] `read_frame` length check happens before allocation.
- [ ] Every parser rejects trailing bytes.
- [ ] No Spaces file outside `net.rs`/`net_tests.rs`/`spike.rs` names `p2p`.
- [ ] All UI strings match this plan word for word.

### Do NOT
- Edit any file. Report only.

### If it fails
1. A test fails: report the test name and the first assertion message; do not fix.
2. A checklist item fails: report file and line.
3. Vitest count differs: report the actual count and which file is missing.

---

## Open facts

- NEEDS-FACT W-1: the SHA-256 of BIP-39 `english.txt`. Expected value
  `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda` (writer's memory, not verified). Compute with
  `sha256sum packages/app/src-tauri/src/spaces/words/en.txt` after D2.1 step 1 and compare.
- NEEDS-FACT W-2: the licence of the BIP-39 English word list (the `bitcoin/bips` repository's licence for
  `bip-0039`), or of the npm package it is copied from. Read that repository's `README`/`LICENSE` or the package's
  `LICENSE` file.
- NEEDS-FACT N-1: the `CapabilityHandler` trait's methods and how `accept_loop` hands an admitted link to it. Read
  `packages/app/src-tauri/src/p2p/capability.rs` and `p2p/endpoint.rs` line 238 onward.
- NEEDS-FACT N-2: whether `KeyStore::member()`, `device_sign()`, `device_kem()` create keys on first use. Read
  `packages/app/src-tauri/src/team/keystore.rs` lines 85-110.
- NEEDS-FACT N-3: how `pairing_commands::ready` reads the public settings JSON (function name and module). Read
  `packages/app/src-tauri/src/pairing_commands.rs` lines 60-110.
- NEEDS-FACT Q-1: whether `packages/app/package.json` has a QR code library. Search it for `qr`.
- NEEDS-FACT R-1: whether `sha2` is a direct dependency in `packages/app/src-tauri/Cargo.toml` and its version
  (facts §"Could not find" 3).
- NEEDS-FACT T-1: an in-memory `SecretBackend` for tests that need a `KeyStore` (join tests read invite secrets).
  Read `team/keystore.rs` and `team/tests.rs` for an existing test backend.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Guess the invite words online | Send many JoinRequests | 88-bit secret; 5 strikes kill the invite; 3 failures lock a node 10 min; 20/min global | `fifth_wrong_try_kills_the_invite_end_to_end`, `three_failed_joins_lock_the_node_out`, `busy_after_twenty_requests_in_a_minute` |
| Guess offline from a captured link | Link alone | Link has no secret; the proof travels only inside QUIC TLS to the owner's node id | `link_round_trips` (secret is `None`) |
| Replay Sam's proof from another computer | Copy Sam's JoinRequest | Proof binds the joiner's node id, which QUIC authenticates | `proof_binds_the_joiner_node`, `cert_for_another_node_is_bad` |
| Use one invite twice | Second joiner after the first | `Pending(joiner)` and `Used` states | `one_use_other_joiner_gets_gone` |
| Fake owner (MITM) | Answer a joiner with a forged welcome | Joiner dials the owner's node id from the link; replays the log; owner device node must equal the link's | `welcome_from_wrong_owner_node_is_rejected`, `tampered_welcome_op_is_rejected` |
| Forged roster change | Member signs an op | Only owner key signs; chain checked | `op_signed_by_member_is_rejected`, `broken_chain_is_rejected` |
| Hidden name tricks | Bidi override or control chars in a name | `clean_name`; cert must carry a clean name | `unclean_name_in_cert_fails`, `clean_name_rules` |
| Memory exhaustion before checks (cowork M3) | Huge frame length, many strangers | Length checked before allocation; strangers admitted only while an invite is open; lockout map capped at 256 | `oversized_length_is_refused_without_allocating`, `stranger_is_refused_when_no_invite_is_open` |
| Join through pairing or the bridge | Use device pairing to get into a space | Spaces never touches pairing/trust; guard | `spaces_never_uses_pairing_trust_the_bridge_or_team_transports` |
| A member gets desk control | Member node dials desk ALPN | Routed permissions | `space_member_is_refused_on_desk_alpn` (plan 21) |
| Owner tricked into "Let in" | Social engineering | Sheet shows name + computer short id + "Only let in people you know." | `shows who and the short id` |
| The owner's LAN IP leaks via the link | Anyone who sees the link | Accepted cost; documented here; LAN addresses limited to 4 | none (documented) |

## What cannot be verified without real hardware or a second computer

- A real join between two PCs on different networks (direct and relayed), including the 30-minute wait.
- The QR code scanned by a real camera (and whether the base32 link length makes a readable QR at 192 px).
- Keystore behaviour on Windows Credential Manager / macOS Keychain for `space:<hex>` names (colon in the name).
- How long a real person takes to read 8 words aloud and how often they mistype them.
