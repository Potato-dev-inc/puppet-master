# Spaces D-0 and D-1: Labs flag, transport seams, spike, secure core

Read `20-spaces-overview.md` first (architecture, merge order, limits, glossary).

Writer's sources: `00-FORMAT.md` and `_facts/spaces-facts.md` only (founder instruction). Facts are cited as
`facts §N`. Every builder MUST open the files in `Read first` and stop if they contradict this plan.

Slices in this file: D0.1, D0.2, D0.3, D0.4, D1.1, D1.2, D1.3 (7 slices).

Shared rules for every slice below (copied into each `Do NOT`, listed once here to keep slices short; they apply in
full to every slice):

- R1. Never run a dev server or the app on the primary screen. No `npm run dev`, no `cargo tauri dev`.
- R2. Always set a separate `CARGO_TARGET_DIR`, exactly the one named in the slice's `Run`. Never the default.
- R3. Never kill a process by image name. Stop only a PID you started.
- R4. `p2p/guard_tests.rs` must stay green: only `p2p/iroh_net.rs` touches the network in `p2p/`; shipped p2p code
  prints nothing, reads no env, writes no file, uses no `Serialize` or `serde_json::to_`; the four core files
  (`endpoint.rs`, `iroh_net.rs`, `memory.rs`, `identity.rs`) never contain `claude codex agent pane mouse keyboard
  clipboard desk` (any case) (facts §2).
- R5. Space traffic never uses the bridge, MCP, the phone route, device pairing (`PairManager`, `TrustList`), or the
  team transports.
- R6. Commit finished work on a branch (never on `main`), one commit per slice, message `spaces: <slice id> <goal>`.
  The handoff names a fresh build id.
- R7. File count: adding `pub mod x;` / `#[cfg(test)] mod x_tests;` lines to `spaces/mod.rs` does not count toward a
  slice's 5-file limit (it is only the module list). Every other file does.

All cargo commands run from `packages/app/src-tauri` (facts §10). All Vitest commands run from the repo root.

---

## D0.1 Labs flag `spaces_enabled`
Needs merged first: none.

### Goal
Add the Labs key `spaces_enabled` to the Rust and TypeScript dev lists, the Labs label table, the General settings
toggle, and a frontend gate `useSpacesEnabled()`.

### Read first
- `packages/app/src-tauri/src/settings_store.rs` lines 1-40: the `DEV_LABS_KEYS` list (facts §6), and whether the
  file already has a `#[cfg(test)] mod` at the end.
- `packages/app/src/lib/dev-labs.ts` lines 1-20: the TS mirror (facts §6).
- `packages/app/src/lib/setup-checks/labs.ts` lines 1-30: the `LABEL` table (facts §6).
- `packages/app/src/components/settings/sections/GeneralSection.tsx` lines 500-560: the `desk_enabled` toggle block
  to copy (lines 528-529, facts §6). Copy the whole JSX element that contains those two lines.
- `packages/app/src/lib/desk.ts` lines 1-40: `deskEnabled` and `useDeskEnabled` to copy (facts §6).

### Files
- edit `packages/app/src-tauri/src/settings_store.rs`
- edit `packages/app/src/lib/dev-labs.ts`
- edit `packages/app/src/lib/setup-checks/labs.ts`
- edit `packages/app/src/components/settings/sections/GeneralSection.tsx`
- create `packages/app/src/lib/spaces.ts`

### Steps
1. In `settings_store.rs`, in `DEV_LABS_KEYS`, add the line `    "spaces_enabled",` directly after the line
   `    "desk_enabled",`.
2. In `dev-labs.ts`, in `DEV_LABS_KEYS`, add `  'spaces_enabled',` directly after `  'desk_enabled',`.
3. In `setup-checks/labs.ts`, in `LABEL`, add `  spaces_enabled: 'Spaces',` directly after `  desk_enabled: 'Desk',`.
4. In `GeneralSection.tsx`, copy the JSX element that holds `checked={env.settings.desk_enabled ?? false}` and paste
   the copy directly below it. In the copy change: `desk_enabled` to `spaces_enabled` (both places), the label text to
   `Spaces`, the help text to `Chat with friends in small groups, computer to computer. No server keeps your messages.`
   If the settings type does not have `spaces_enabled`, add `spaces_enabled?: boolean;` next to `desk_enabled` in the
   same type (find it with a search for `desk_enabled?:` in `packages/app/src`), and count that file as a 6th file
   only in this case; say so in the commit message.
5. Create `packages/app/src/lib/spaces.ts` with the code below. Copy the import of the settings hook from `desk.ts`
   exactly (same module path, same hook name).
6. Append the Rust test module below at the end of `settings_store.rs` (if a `#[cfg(test)] mod tests` exists, add the
   test function inside it instead and keep its name).

### Code
`packages/app/src/lib/spaces.ts` (first version; D2.7 and D4.1 extend it):
```ts
// Spaces (Labs `spaces_enabled`): small-group chat between people's own computers.
// The settings hook import below MUST be copied from lib/desk.ts (same path, same name).
import { useSettingsForLabs } from './desk-settings-hook-placeholder';

export const SPACES_LABS_KEY = 'spaces_enabled' as const;

export function spacesEnabled(settings: { spaces_enabled?: boolean } | null | undefined): boolean {
  return settings?.spaces_enabled === true;
}

export function useSpacesEnabled(): boolean {
  const settings = useSettingsForLabs();
  return spacesEnabled(settings);
}
```
Replace the placeholder import line and the body of `useSpacesEnabled` with the exact pattern `useDeskEnabled` uses
in `desk.ts`, changing only `deskEnabled` to `spacesEnabled`. The file must not keep the word `placeholder`.

Rust test (end of `settings_store.rs`):
```rust
#[cfg(test)]
mod spaces_labs_tests {
    use super::DEV_LABS_KEYS;

    #[test]
    fn dev_labs_keys_include_spaces_enabled() {
        assert!(DEV_LABS_KEYS.contains(&"spaces_enabled"));
        assert_eq!(DEV_LABS_KEYS.iter().filter(|k| **k == "spaces_enabled").count(), 1);
    }
}
```

### Tests to write
- `dev_labs_keys_include_spaces_enabled` (Rust): input the const; expected: contains `spaces_enabled` exactly once.
- The TS mirror is checked by the existing tests that compare the two lists if any exist (search
  `DEV_LABS_KEYS` in `packages/app/src/**/*.test.ts`); they must stay green.

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d01 cargo test --lib spaces_labs_tests
# expected: test result: ok. 1 passed; 0 failed
cd ../../..
npx vitest run packages/app/src/lib packages/app/src/components/settings
# expected: no failed tests
```

### Done when
- [ ] `DEV_LABS_KEYS` has 8 entries in Rust and in TS, the new one is `spaces_enabled` in both.
- [ ] Labs label `Spaces` shows in the setup check list code.
- [ ] General settings has a `Spaces` toggle bound to `spaces_enabled`.
- [ ] `spacesEnabled({ spaces_enabled: true })` is `true`; `spacesEnabled({})` is `false`.
- [ ] Commit on a branch; fresh build id in the handoff.

### Do NOT
- R1-R7.
- Do not reuse `desk_enabled` for anything in Spaces.
- Do not turn the flag on by default anywhere.

### If it fails
1. Vitest fails on a list-length snapshot: update only that snapshot's expected list to include `spaces_enabled`.
2. TS error `Property 'spaces_enabled' does not exist`: do step 4's type addition.
3. Rust test module name clash: rename the new module to `spaces_labs_tests_2`.

---

## D0.2 p2p seams for Spaces
Needs merged first: D0.1.

### Goal
Let the single `P2p` service carry capability `spaces` under its own Labs key `spaces_enabled`, with its own
permission rule (`RoutedPermissions`), and let a dial carry the peer's home relay URL.

### Read first
- `packages/app/src-tauri/src/p2p/mod.rs` whole file: `LABS_KEY`, `labs_on`, `P2p` fields, `apply_settings`, `want`,
  `unwant`, `dial`, `stop` (facts §1). You need the bodies, which the facts do not have.
- `packages/app/src-tauri/src/p2p/capability.rs` whole file: `Permissions`, `MemoryPermissions`, `RelayPolicy`
  (facts §1).
- `packages/app/src-tauri/src/p2p/endpoint.rs` lines 20-150: `NodeId`, `PeerAddr`, `Transport`, `BindSpec`.
- `packages/app/src-tauri/src/p2p/iroh_net.rs` whole file: how `dial` builds `EndpointAddr` and how relays are set.
- `packages/app/src-tauri/src/p2p/tests.rs` top 80 lines: the helpers used to build a `P2p` on `MemoryNet`.
- `packages/app/src-tauri/src/p2p/guard_tests.rs` whole file (rules R4).
- Find where the app constructs its one `P2p` (search `P2p::new(` in `packages/app/src-tauri/src`). Write down the
  file and line in the commit message.

### Files
- edit `packages/app/src-tauri/src/p2p/capability.rs`
- edit `packages/app/src-tauri/src/p2p/mod.rs`
- edit `packages/app/src-tauri/src/p2p/endpoint.rs`
- edit `packages/app/src-tauri/src/p2p/iroh_net.rs`
- edit `packages/app/src-tauri/src/p2p/tests.rs`

### Steps
1. `capability.rs`: add `RoutedPermissions` (code below) after `MemoryPermissions`.
2. `mod.rs`: add `pub const SPACES_LABS_KEY`, `labs_key_for`, `labs_on_for` next to `LABS_KEY`/`labs_on`.
3. `mod.rs`: store both Labs flags. Wherever `apply_settings` stores the result of `labs_on(public)`, also store
   `labs_on_for(public, capability::SPACES)` in a new field `spaces_labs: AtomicBool` (same type style as the
   existing flag; if the existing flag is a `Mutex<bool>`, use `Mutex<bool>`). Add `pub fn labs_for(&self,
   capability: &str) -> bool`.
4. `mod.rs`: in `want(capability)`, replace the check of the desk Labs flag with `self.labs_for(capability)`. In
   `apply_settings`, where the service stops because Labs is off, stop only when BOTH flags are off; when only one
   flag is off, call `self.unwant(cap).await` for each wanted capability whose `labs_for` is now false (if
   `apply_settings` is not `async`, spawn this on the existing runtime handle the file already uses; if it uses none,
   record `NEEDS-FACT` in the commit message and leave the stop-both rule only).
5. `endpoint.rs`: add `pub relay: Option<String>` to `PeerAddr`, set it to `None` in `PeerAddr::new`, add
   `pub fn with_relay(mut self, url: &str) -> Self`. Add `fn home_relay(&self) -> Option<String> { None }` as a
   default method on `Transport`. Fix every `PeerAddr { .. }` struct literal in `p2p/` (search `PeerAddr {`).
6. `iroh_net.rs`: in `IrohTransport::dial`, when `peer.relay` is `Some(url)` and the bind spec had `relays: true`,
   add that relay URL to the `EndpointAddr`. Implement `home_relay` by reading the endpoint's current home relay.
   Use the iroh 1.4.1 API that `iroh_net.rs` already imports; if no relay-url method is visible, see Open facts F-3.
7. `tests.rs`: add the tests below.

### Code
`capability.rs` addition:
```rust
/// One capability may bring its own permission rule; everything else asks `base`.
/// Generic on purpose: it knows only capability ids, never what they do.
pub struct RoutedPermissions {
    base: Arc<dyn Permissions>,
    routes: std::sync::RwLock<std::collections::BTreeMap<String, Arc<dyn Permissions>>>,
}

impl RoutedPermissions {
    pub fn new(base: Arc<dyn Permissions>) -> Arc<Self> {
        Arc::new(Self { base, routes: std::sync::RwLock::new(Default::default()) })
    }

    /// Give `capability` its own rule. Replaces an earlier route for the same id.
    pub fn route(&self, capability: &str, perms: Arc<dyn Permissions>) {
        if let Ok(mut r) = self.routes.write() {
            r.insert(capability.to_string(), perms);
        }
    }

    fn pick(&self, capability: &str) -> Arc<dyn Permissions> {
        self.routes
            .read()
            .ok()
            .and_then(|r| r.get(capability).cloned())
            .unwrap_or_else(|| self.base.clone())
    }
}

impl Permissions for RoutedPermissions {
    fn allowed(&self, peer: &NodeId, capability: &str) -> bool {
        self.pick(capability).allowed(peer, capability)
    }
    fn set(&self, peer: &NodeId, capability: &str, on: bool) {
        self.pick(capability).set(peer, capability, on)
    }
}
```
(Use the `NodeId` import the file already has.)

`mod.rs` additions:
```rust
/// Spaces has its own Labs switch; every other capability uses `LABS_KEY`.
pub const SPACES_LABS_KEY: &str = "spaces_enabled";

pub fn labs_key_for(capability: &str) -> &'static str {
    if capability == capability::SPACES { SPACES_LABS_KEY } else { LABS_KEY }
}

pub fn labs_on_for(public: &Value, capability: &str) -> bool {
    public.get(labs_key_for(capability)).and_then(Value::as_bool).unwrap_or(false)
}

impl P2p {
    pub fn labs_for(&self, capability: &str) -> bool {
        if capability == capability::SPACES {
            self.spaces_labs.load(std::sync::atomic::Ordering::SeqCst)
        } else {
            self.labs()
        }
    }
}
```

`endpoint.rs` additions:
```rust
// in struct PeerAddr:
    /// Home relay URL of the peer, when known. Only used if the bind allows relays.
    pub relay: Option<String>,

impl PeerAddr {
    pub fn with_relay(mut self, url: &str) -> Self {
        let url = url.trim();
        self.relay = if url.is_empty() { None } else { Some(url.to_string()) };
        self
    }
}

// in trait Transport:
    /// The relay this endpoint is reachable through, when relays are on.
    fn home_relay(&self) -> Option<String> { None }
```

Wiring note (do it in this slice only if the `P2p::new(` call site is one of the 5 files above; otherwise D2.5 does
it): the app's `P2p::new(keys, factory, registry, perms)` gets `perms = RoutedPermissions::new(old_perms)` and keeps
the `Arc<RoutedPermissions>` so Spaces can call `route("spaces", ...)` later.

### Tests to write (all in `p2p/tests.rs`, all on `MemoryNet`)
| Test | Input | Expected |
|---|---|---|
| `routed_permissions_use_the_route_for_its_capability` | base = `MemoryPermissions::new()` (nobody), route `spaces` = a test struct that allows everyone | `allowed(peer, "spaces") == true` |
| `routed_permissions_fall_back_to_base_for_other_capabilities` | same | `allowed(peer, "desk/input") == false` |
| `routed_set_goes_to_the_routed_rule` | route `spaces` = `MemoryPermissions`; `set(peer, "spaces", true)` | route's `allowed(peer,"spaces") == true`, base's `allowed(peer,"spaces") == false` |
| `space_member_is_refused_on_desk_alpn` | `P2p` on `MemoryNet` with routed perms (spaces allows peer B), a capability registered on `alpn::DESK` with any handler the existing tests use; B dials A on `alpn::DESK` | the link is closed with `CLOSE_NOT_ALLOWED` (2) and A's `refusals()` has one entry |
| `spaces_labs_key_is_spaces_enabled` | `labs_key_for("spaces")`, `labs_key_for("desk/input")` | `"spaces_enabled"`, `"desk_enabled"` |
| `want_spaces_fails_with_only_desk_enabled` | `apply_settings(json!({"desk_enabled": true}))`, `want("spaces")` | `Err(_)` |
| `want_spaces_works_with_spaces_enabled` | `apply_settings(json!({"spaces_enabled": true}))` after registering a `spaces` capability | `Ok(_)`, `is_running() == true` |
| `want_desk_still_needs_desk_enabled` | `apply_settings(json!({"spaces_enabled": true}))`, `want("desk/input")` | `Err(_)` |
| `peer_addr_keeps_relay_hint` | `PeerAddr::new(id).with_relay(" https://r.example/ ")` | `relay == Some("https://r.example/")`; `with_relay("")` gives `None` |

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d02 cargo test --lib p2p
# expected: test result: ok. ... 0 failed   (the 9 new tests are in the list)
CARGO_TARGET_DIR=D:/pm-night/target-d02 cargo test --lib p2p::guard_tests
# expected: test result: ok. 9 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d02 cargo test --lib desk
# expected: 0 failed
```

### Done when
- [ ] The 9 tests above pass; all existing p2p and desk tests pass.
- [ ] `p2p/guard_tests.rs` is unchanged and green.
- [ ] `desk_enabled` alone never starts Spaces; `spaces_enabled` alone never allows desk.

### Do NOT
- R1-R7.
- Do not add `spaces` to `PERMISSIONABLE` (facts §1): a space member must never become a "My computers" device.
- Do not write the words `desk`, `agent`, `pane` in `endpoint.rs` or `iroh_net.rs` (R4). The word `spaces` is
  allowed but keep it out of those two files anyway; `relay` is the only new word there.
- Do not change `pairing.rs` (its `set_permission(` count is guarded, facts §2).

### If it fails
1. `the_core_knows_no_agent_or_capability_behaviour` fails: a forbidden word slipped into a doc comment of
   `endpoint.rs`/`iroh_net.rs`; reword it.
2. Compile errors `missing field relay` in `PeerAddr { .. }` literals: add `relay: None` to each.
3. `want_spaces_works_with_spaces_enabled` fails because `want` needs a registered capability: register one in the
   test with `Capability::new(capability::SPACES, alpn::SPACES, <relay-allowed variant>, handler)` using the handler
   type the existing tests use.

---

## D0.3 `spaces/` module skeleton and guard tests
Needs merged first: D0.2.

### Goal
Create the `spaces` Rust module with `limits.rs` and its own guard tests, and allow-list the three Spaces files that
may use `p2p` in the existing p2p guard.

### Read first
- `packages/app/src-tauri/src/p2p/guard_tests.rs` whole file: helpers `tauri_src`, `packages`, `files`, `read`,
  `code_only`, and the allowed list in `only_known_files_use_the_p2p_module` (facts §2).
- `packages/app/src-tauri/src/lib.rs` lines 100-120 (module list, facts §9).

### Files
- create `packages/app/src-tauri/src/spaces/mod.rs`
- create `packages/app/src-tauri/src/spaces/limits.rs`
- create `packages/app/src-tauri/src/spaces/guard_tests.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src-tauri/src/p2p/guard_tests.rs`

### Steps
1. Create `spaces/mod.rs` and `spaces/limits.rs` exactly as below.
2. Create `spaces/guard_tests.rs` as below. Copy the helper functions `tauri_src`, `packages`, `files`, `read`,
   `code_only` from `p2p/guard_tests.rs` verbatim into it (they are private there).
3. `lib.rs`: add `mod spaces;` directly after `mod review_desk;` (line 114 area, facts §9).
4. `p2p/guard_tests.rs`: in the allowed list of `only_known_files_use_the_p2p_module`, add the three paths
   `spaces/net.rs`, `spaces/net_tests.rs`, `spaces/spike.rs`, in the same path format the list already uses, with
   the comment `// Spaces: only these three may name p2p (spaces/guard_tests.rs checks the rest)`.
5. In `p2p/guard_tests.rs`, check that the `src/spaces` directory is NOT in the roots of
   `p2p_is_not_reachable_from_the_bridge_mcp_or_the_phone`. It must not be added there.

### Code
`packages/app/src-tauri/src/spaces/mod.rs`:
```rust
//! Spaces (Labs `spaces_enabled`): small-group chat between people's own computers.
//! No server keeps messages. Only `net.rs` talks to the link layer (`crate::p2p`).
//! See docs/plans/social/20-spaces-overview.md.

pub mod limits;

pub const LABS_KEY: &str = "spaces_enabled";

pub fn labs_on(public: &serde_json::Value) -> bool {
    public.get(LABS_KEY).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod guard_tests;
```

`packages/app/src-tauri/src/spaces/limits.rs`:
```rust
//! Every Spaces limit in one place. Changing a number here is a product decision.

pub const MAX_MEMBERS: usize = 50;
pub const MAX_CHANNELS: usize = 20;
pub const MAX_CHANNELS_EVER: usize = 100;
pub const KEEP_MS: u64 = 90 * 24 * 60 * 60 * 1000;
pub const KEEP_BYTES: u64 = 200 * 1024 * 1024;
pub const TRIM_TO_BYTES: u64 = 180 * 1024 * 1024;
pub const INVITE_TTL_MS: u64 = 24 * 60 * 60 * 1000;
pub const INVITE_MAX_STRIKES: u32 = 5;
pub const MAX_OPEN_INVITES: usize = 3;
pub const JOIN_LOCKOUT_STRIKES: u32 = 3;
pub const JOIN_LOCKOUT_MS: u64 = 600_000;
pub const JOIN_WAIT_MS: u64 = 30 * 60 * 1000;
pub const JOINS_PER_MINUTE: u32 = 20;
pub const NAME_MAX_CHARS: usize = 40;
pub const CHANNEL_NAME_MAX_CHARS: usize = 32;
pub const MAX_TEXT_CHARS: usize = 4000;
pub const MAX_ENTRY_BYTES: usize = 65_536;
pub const MAX_FRAME_BYTES: usize = 1_048_576;
pub const MAX_LINKS: usize = 12;
pub const RATE_BURST: u32 = 10;
pub const RATE_BURST_WINDOW_MS: u64 = 10_000;
pub const RATE_HOUR: u32 = 600;
pub const MAX_WANT_PER_ROUND: usize = 500;
pub const FILE_MAX_BYTES: u64 = 25 * 1024 * 1024;
pub const FILE_CHUNK: usize = 65_536;
pub const FISH_DAY_CAP: u32 = 30;
pub const FISH_PERSON_HOUR_CAP: u32 = 5;
pub const FISH_UNPROMPTED_CAP: u32 = 4;
pub const FISH_UNPROMPTED_WINDOW_MS: u64 = 30 * 60 * 1000;
```

`packages/app/src-tauri/src/spaces/guard_tests.rs`:
```rust
//! Source guards for Spaces. They read source files, so a change that breaks a rule fails here.
use std::path::{Path, PathBuf};

// --- helpers copied verbatim from p2p/guard_tests.rs: tauri_src, packages, files, read, code_only ---

fn spaces_dir() -> PathBuf { tauri_src().join("spaces") }

/// Every .rs file in spaces/ with its file name.
fn spaces_files() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in files(&spaces_dir(), &["rs"]) {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        out.push((name, code_only(&read(&p))));
    }
    out
}

const MAY_NAME_P2P: [&str; 3] = ["net.rs", "net_tests.rs", "spike.rs"];
const NOT_SHIPPED: [&str; 4] = ["guard_tests.rs", "spike.rs", "net_tests.rs", "memnet.rs"];
const PURE: [&str; 9] = [
    "ids.rs", "wire.rs", "words.rs", "identity.rs", "roster.rs", "invite.rs", "log.rs", "sync.rs", "limits.rs",
];

#[test]
fn spaces_is_not_reachable_from_the_bridge_mcp_or_the_phone() {
    let src = tauri_src();
    let pk = packages();
    let roots: Vec<PathBuf> = vec![
        src.join("bridge"), src.join("tool_registry"), src.join("mcp_shim"), src.join("companion"),
        src.join("mobile_pairing"), src.join("channels"), src.join("bin"),
        pk.join("mcp-server/src"), pk.join("cli/src"), pk.join("app/src/pwa"),
        pk.join("app/src/PwaApp.tsx"), pk.join("app/src/PwaLegacyApp.tsx"),
    ];
    let words = ["spaces::", "space_send", "spaces_view", "space_join", "SpaceNet", "fish_bridge", "lib/spaces"];
    for root in roots {
        for f in files(&root, &["rs", "ts", "tsx"]) {
            let code = code_only(&read(&f));
            for w in words {
                assert!(!code.contains(w), "{} mentions {w}", f.display());
            }
        }
    }
}

#[test]
fn only_net_rs_names_the_p2p_module() {
    for (name, code) in spaces_files() {
        if MAY_NAME_P2P.contains(&name.as_str()) || name == "guard_tests.rs" { continue; }
        assert!(!code.contains("p2p::") && !code.contains("crate::p2p"), "{name} names p2p");
    }
}

#[test]
fn spaces_never_uses_pairing_trust_the_bridge_or_team_transports() {
    let words = [
        "PairManager", "TrustList", "pairing", "bridge::", "mobile_pairing", "RelayTransport",
        "FolderTransport", "TeamTransport", "pair_answer", "trust_set_permission",
    ];
    for (name, code) in spaces_files() {
        if name == "guard_tests.rs" { continue; }
        for w in words { assert!(!code.contains(w), "{name} uses {w}"); }
    }
}

#[test]
fn spaces_code_prints_nothing_and_reads_no_env() {
    let words = ["println!", "eprintln!", "dbg!", "env::var", "env::args"];
    for (name, code) in spaces_files() {
        if NOT_SHIPPED.contains(&name.as_str()) { continue; }
        for w in words { assert!(!code.contains(w), "{name} uses {w}"); }
    }
}

#[test]
fn pure_spaces_files_open_no_files_or_sockets() {
    let words = ["std::fs", "tokio::fs", "File::", "tokio::net", "std::net::Tcp", "UdpSocket", "reqwest", "ureq"];
    for (name, code) in spaces_files() {
        if !PURE.contains(&name.as_str()) { continue; }
        for w in words { assert!(!code.contains(w), "{name} uses {w}"); }
    }
}

#[test]
fn fish_bridge_has_no_route_to_tools() {
    let p = spaces_dir().join("fish_bridge.rs");
    if !p.exists() { return; }
    let code = code_only(&read(&p));
    for w in ["tool_registry", "Command::new", "std::fs", "mcp", "shell", "computer", "spawn_agent"] {
        assert!(!code.contains(w), "fish_bridge.rs uses {w}");
    }
}

#[test]
fn limits_match_the_founder_defaults() {
    use super::limits::*;
    assert_eq!(MAX_MEMBERS, 50);
    assert_eq!(MAX_CHANNELS, 20);
    assert_eq!(KEEP_MS, 7_776_000_000);
    assert_eq!(KEEP_BYTES, 209_715_200);
    assert_eq!(INVITE_TTL_MS, 86_400_000);
    assert_eq!(INVITE_MAX_STRIKES, 5);
    assert_eq!(FISH_DAY_CAP, 30);
    assert_eq!(FISH_PERSON_HOUR_CAP, 5);
    assert_eq!(FISH_UNPROMPTED_CAP, 4);
}
```
If the copied `files()` helper takes a file path root and returns nothing for a single file, the two `.tsx` roots
above still work only if `files()` accepts a file; if it does not, handle them with a direct `read` in the same test.

### Tests to write
The 7 tests in `spaces/guard_tests.rs` above. Expected: all pass on the current tree (no `spaces` words exist
outside `spaces/`; `fish_bridge.rs` does not exist yet, so its test returns early).

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d03 cargo test --lib spaces::guard_tests
# expected: test result: ok. 7 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d03 cargo test --lib p2p::guard_tests
# expected: test result: ok. 9 passed; 0 failed
```

### Done when
- [ ] `spaces` module compiles with `mod spaces;` in `lib.rs`.
- [ ] 7 Spaces guard tests pass; 9 p2p guard tests pass.
- [ ] p2p allow-list has exactly the three new Spaces paths.

### Do NOT
- R1-R7.
- Do not allow-list `spaces/service.rs` or any other Spaces file in the p2p guard.
- Do not add `#[allow(dead_code)]` to the whole module; `limits.rs` consts unused for now give warnings only, which
  is fine.

### If it fails
1. Unused-const warnings fail the build because of `#![deny(warnings)]`: add `#[allow(dead_code)]` on the line above
   `pub mod limits;` only.
2. `files()` signature differs (for example takes `&Path, &[&str]` vs `&[&str; N]`): adapt the calls, not the helper.
3. A p2p guard path format mismatch (`src/spaces/net.rs` vs `spaces/net.rs`): copy the exact format of the entries
   already in the list.

---

## D0.4 iroh spike: two PCs talk, direct and through a relay
Needs merged first: D0.3, D1.3.

### Goal
Measure, with two test identities on two computers, whether a `pm/spaces/1` link comes up direct, through a public
relay, how long it takes, and the round-trip time; record the numbers in this plan.

### Read first
- `packages/app/src-tauri/src/p2p/iroh_net.rs` (how `IrohFactory::bind` works).
- `packages/app/src-tauri/src/p2p/identity.rs` (`DeviceIdentity::load(keys)`).
- `packages/app/src-tauri/src/team/keystore.rs` lines 1-90 (`KeyStore::with_backend`, `SecretBackend`).
- `packages/app/src-tauri/Cargo.toml` `[features]` block (facts §5).

### Files
- create `packages/app/src-tauri/src/spaces/spike.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`
- edit `packages/app/src-tauri/Cargo.toml`
- edit `docs/plans/social/21-spaces-d0-d1-transport-spike-and-secure-core.md` (fill the Results table below)

### Steps
1. `Cargo.toml` `[features]`: add the line `spaces-spike = []` directly after `dev-fake-tui = []`.
2. `spaces/mod.rs`: add at the end:
   ```rust
   #[cfg(all(test, feature = "spaces-spike"))]
   mod spike;
   ```
3. Create `spaces/spike.rs` (code below). The `SpikeSecrets` type implements the keystore's `SecretBackend` trait;
   copy the trait's method list from `keystore.rs` and store bytes in a `Mutex<HashMap<String, Vec<u8>>>`.
4. Build once on PC A and PC B (two different networks, for example home Wi-Fi and a phone hotspot).
5. PC A: run the listen command. Copy the `SPIKE ticket` line.
6. PC B: run the dial command with that ticket, three runs with relays on, then three runs with
   `PM_SPIKE_RELAYS=0`. Then repeat both on the same LAN.
7. Paste every `SPIKE result` line into the Results table.

### Code
`packages/app/src-tauri/src/spaces/spike.rs`:
```rust
//! D0.4 spike. Test-only, feature `spaces-spike`, ignored by default. Prints results on purpose.
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::p2p::alpn;
use crate::p2p::endpoint::{BindSpec, NodeId, PeerAddr, TransportFactory};
use crate::p2p::identity::DeviceIdentity;
use crate::p2p::iroh_net::IrohFactory;
use crate::team::keystore::KeyStore;

struct SpikeSecrets(Mutex<HashMap<String, Vec<u8>>>);
// impl crate::team::keystore::SecretBackend for SpikeSecrets { ...copy the trait's methods; store in the map... }

fn keys(role: &str) -> KeyStore {
    KeyStore::with_backend(Arc::new(SpikeSecrets(Mutex::new(HashMap::new()))), &format!("spike-{role}"))
}

fn spec(relays: bool) -> BindSpec {
    let mut alpns = BTreeSet::new();
    alpns.insert(alpn::SPACES.to_vec());
    BindSpec { alpns, relays, loopback_only: false }
}

fn ticket(node: &NodeId, relay: Option<String>, addrs: &[std::net::SocketAddr]) -> String {
    let a: Vec<String> = addrs.iter().map(|x| x.to_string()).collect();
    format!("{}|{}|{}", node.to_hex(), relay.unwrap_or_default(), a.join(","))
}

fn parse_ticket(t: &str) -> PeerAddr {
    let parts: Vec<&str> = t.trim().split('|').collect();
    let mut id = [0u8; 32];
    for i in 0..32 { id[i] = u8::from_str_radix(&parts[0][i * 2..i * 2 + 2], 16).unwrap(); }
    let mut addr = PeerAddr::new(NodeId(id)).with_relay(parts.get(1).copied().unwrap_or(""));
    // LAN addresses: copy how PeerAddr stores addresses (see p2p::pairing::parse_ticket) and add parts[2] split by ','.
    let _ = &mut addr;
    addr
}

fn env(name: &str, default: &str) -> String { std::env::var(name).unwrap_or_else(|_| default.to_string()) }

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn spaces_spike() {
    let role = env("PM_SPIKE_ROLE", "listen");
    let relays = env("PM_SPIKE_RELAYS", "1") == "1";
    let seconds: u64 = env("PM_SPIKE_SECONDS", "120").parse().unwrap();
    let identity = DeviceIdentity::load(&keys(&role)).expect("identity");
    let t = IrohFactory.bind(&identity, &spec(relays)).await.expect("bind");
    tokio::time::sleep(Duration::from_secs(3)).await; // let the relay home settle
    if role == "listen" {
        println!("SPIKE ticket {}", ticket(&t.id(), t.home_relay(), &t.local_addrs()));
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            let Some(link) = t.accept().await else { break };
            println!("SPIKE result accepted peer={} path={:?}", link.peer().short(), link.path());
            tokio::spawn(async move {
                while let Ok(mut s) = link.accept_bi().await {
                    let mut buf = vec![0u8; 65536];
                    loop {
                        let n = match s.recv.read(&mut buf).await { Ok(0) | Err(_) => break, Ok(n) => n };
                        if s.send.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            });
        }
    } else {
        let peer = parse_ticket(&env("PM_SPIKE_TICKET", ""));
        let start = Instant::now();
        let link = t.dial(&peer, alpn::SPACES).await.expect("dial");
        println!("SPIKE result connect_ms={} first_path={:?}", start.elapsed().as_millis(), link.path());
        let first = link.path();
        let change = tokio::time::timeout(Duration::from_secs(15), link.wait_path_change(first)).await;
        println!("SPIKE result path_after_15s={:?}", change.ok().flatten().unwrap_or(first));
        let mut s = link.open_bi().await.expect("stream");
        let mut rtts = Vec::new();
        for _ in 0..50 {
            let t0 = Instant::now();
            s.send.write_all(&[7u8; 32]).await.unwrap();
            let mut back = [0u8; 32];
            s.recv.read_exact(&mut back).await.unwrap();
            rtts.push(t0.elapsed().as_micros() as u64);
        }
        rtts.sort();
        println!("SPIKE result rtt_p50_us={} rtt_p95_us={}", rtts[25], rtts[47]);
        let blob = vec![1u8; 1_048_576];
        let t1 = Instant::now();
        s.send.write_all(&blob).await.unwrap();
        let mut got = vec![0u8; blob.len()];
        s.recv.read_exact(&mut got).await.unwrap();
        println!("SPIKE result echo_1mib_ms={}", t1.elapsed().as_millis());
        link.close(0, "done");
    }
    t.close().await;
}
```

### Tests to write
- `spaces_spike` (ignored, manual): input env vars; expected output lines `SPIKE ticket ...` (listen) and four
  `SPIKE result` lines (dial). It is a measurement, not a pass/fail test; it fails only if dial or bind fails.

### Run
PC A (listen):
```bash
cd packages/app/src-tauri
PM_SPIKE_ROLE=listen PM_SPIKE_RELAYS=1 CARGO_TARGET_DIR=D:/pm-night/target-d04 \
  cargo test --lib --features spaces-spike spaces::spike -- --ignored --nocapture
# expected: a line "SPIKE ticket <64 hex>|https://...|<addrs>"
```
PC B (dial):
```bash
PM_SPIKE_ROLE=dial PM_SPIKE_RELAYS=1 PM_SPIKE_TICKET='<ticket from A>' CARGO_TARGET_DIR=D:/pm-night/target-d04 \
  cargo test --lib --features spaces-spike spaces::spike -- --ignored --nocapture
# expected: 4 "SPIKE result" lines, then "test result: ok. 1 passed"
```
Default `cargo test --lib` must not run it:
```bash
CARGO_TARGET_DIR=D:/pm-night/target-d04 cargo test --lib spaces
# expected: the spike is not listed (feature off)
```

### Results (filled by the builder of D0.4)
| Run | Networks | Relays | connect_ms | first_path | path_after_15s | rtt_p50_us | rtt_p95_us | echo_1mib_ms |
|---|---|---|---|---|---|---|---|---|
| 1 | different | on | | | | | | |
| 2 | different | on | | | | | | |
| 3 | different | on | | | | | | |
| 4 | different | off | | | | | | |
| 5 | different | off | | | | | | |
| 6 | different | off | | | | | | |
| 7 | same LAN | on | | | | | | |
| 8 | same LAN | off | | | | | | |

Verdict line to write under the table, one of: `Direct works across NAT in N of 3 runs.` and
`Relay fallback worked in N of 3 runs.` (both lines, with numbers).

### Done when
- [ ] Results table has 8 filled rows from two real computers.
- [ ] `cargo test --lib spaces` without the feature does not compile or run `spike.rs`.
- [ ] p2p and Spaces guard tests still pass (`spike.rs` is allow-listed and in `NOT_SHIPPED`).

### Do NOT
- R1-R7.
- Do not commit any ticket, node id or IP address from the runs into the repo (the Results table holds only
  numbers and path kinds).
- Do not use the real `KeyStore::app()` in the spike: it would create real node keys in the OS keystore.
- Do not move the spike into `p2p/` (it prints; p2p shipped code must not, R4).

### If it fails
1. `home_relay` is `None` on A with relays on: wait longer (raise the 3 s sleep to 10 s); if still `None`, see Open
   facts F-3.
2. Dial with relays off times out across networks: expected for some NATs; record `timeout` in the row.
3. `SecretBackend` trait shape differs from what you copied: copy it again from `keystore.rs`; it is the only
   type you implement.

---

## D1.1 `secure_core/crypto.rs` move with golden tests
Needs merged first: none.

### Goal
Move `team/crypto.rs` to `secure_core/crypto.rs` without changing a single output byte, proven by golden tests
written before the move, and keep every team caller working through a re-export.

### Read first
- `packages/app/src-tauri/src/team/crypto.rs` whole file (facts §3: all public signatures).
- `packages/app/src-tauri/src/team/mod.rs`: how `crypto` is declared (`pub mod crypto;` or `mod crypto;`).
- Search `pub(super)` and `pub(crate)` in `team/crypto.rs`. Write down the result.

### Files
- create `packages/app/src-tauri/src/secure_core/mod.rs`
- create `packages/app/src-tauri/src/secure_core/golden_tests.rs`
- create `packages/app/src-tauri/src/secure_core/crypto.rs` (by `git mv` from `team/crypto.rs`)
- create `packages/app/src-tauri/src/team/crypto.rs` (new one-line shim, after the move)
- edit `packages/app/src-tauri/src/lib.rs`

### Steps
1. Create `secure_core/mod.rs` with only `#[cfg(test)] mod golden_tests;` and add `mod secure_core;` to `lib.rs`
   directly after `mod spaces;`.
2. Create `golden_tests.rs` with `use crate::team::crypto as c;` (BEFORE the move) and the tests below, with the
   constants set to `"FILL"`.
3. Run `golden_print` (below) with `--nocapture`; paste the printed hex into the constants. Run all golden tests:
   green.
4. `git mv packages/app/src-tauri/src/team/crypto.rs packages/app/src-tauri/src/secure_core/crypto.rs`.
5. Create the new `team/crypto.rs` shim (code below). If step "Read first" found `pub(super)`/`pub(crate)` items,
   change them to `pub` in `secure_core/crypto.rs` (only those, nothing else).
6. In `secure_core/mod.rs` add `pub mod crypto;` above the test line. In `golden_tests.rs` change the import to
   `use super::crypto as c;`.
7. Run golden tests and all team tests.

### Code
`secure_core/mod.rs` (final):
```rust
//! Crypto shared by teams and Spaces. Moved from team/crypto.rs in Spaces D1.1; bytes are unchanged
//! (see golden_tests.rs).
pub mod crypto;

#[cfg(test)]
mod golden_tests;
```

`team/crypto.rs` (final shim):
```rust
//! Moved to `crate::secure_core::crypto` (Spaces D1.1). This re-export keeps team code unchanged.
pub use crate::secure_core::crypto::*;
```

`secure_core/golden_tests.rs`:
```rust
//! Golden bytes, recorded before the move from team/crypto.rs. They must never change.
use super::crypto as c; // before the move: use crate::team::crypto as c;

const KEY: [u8; 32] = [7u8; 32];
const NONCE: [u8; 24] = [9u8; 24];
const HEADER: &[u8] = b"pm/golden/header";
const PLAIN: &[u8] = b"hello spaces";

const SEALED_HEX: &str = "FILL";
const HMAC_HEX: &str = "FILL";

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

#[test]
#[ignore]
fn golden_print() {
    let ct = c::seal_with_key_and_nonce(&KEY, HEADER, PLAIN, NONCE).unwrap();
    println!("SEALED_HEX={}", hex(&ct));
    println!("HMAC_HEX={}", hex(&c::hmac(b"golden-key", b"golden-message")));
}

#[test]
fn sealed_bytes_are_unchanged() {
    let ct = c::seal_with_key_and_nonce(&KEY, HEADER, PLAIN, NONCE).unwrap();
    assert_eq!(hex(&ct), SEALED_HEX);
}

#[test]
fn golden_ciphertext_still_opens() {
    let ct = c::seal_with_key_and_nonce(&KEY, HEADER, PLAIN, NONCE).unwrap();
    assert_eq!(c::open_with_key(&KEY, &NONCE, HEADER, &ct).unwrap(), PLAIN);
}

#[test]
fn hmac_is_unchanged() {
    assert_eq!(hex(&c::hmac(b"golden-key", b"golden-message")), HMAC_HEX);
    assert!(c::same_mac(&c::hmac(b"a", b"b"), &c::hmac(b"a", b"b")));
    assert!(!c::same_mac(&c::hmac(b"a", b"b"), &c::hmac(b"a", b"c")));
}

#[test]
fn pad_buckets_are_unchanged() {
    assert_eq!(c::PAD_BUCKETS, [1024, 4096, 16384]);
}

#[test]
fn hpke_round_trip_with_fixed_secret() {
    let kem = c::KemSecret::from_bytes([5u8; 32]);
    let (enc, ct) = c::hpke_seal(&kem.public_b64(), b"pm/golden/info", b"key material").unwrap();
    assert_eq!(c::hpke_open(&kem, &enc, b"pm/golden/info", &ct).unwrap(), b"key material");
    assert!(c::hpke_open(&kem, &enc, b"pm/golden/other", &ct).is_err());
}

#[test]
fn kem_public_is_stable_for_fixed_secret() {
    let a = c::KemSecret::from_bytes([5u8; 32]).public_b64();
    let b = c::KemSecret::from_bytes([5u8; 32]).public_b64();
    assert_eq!(a, b);
}
```
If `seal_with_key_and_nonce` pads (facts §3 says only `seal_with_key` "pads"), the golden hex is just longer; it is
still a fixed value.

### Tests to write
The 6 non-ignored tests above plus the ignored `golden_print`. Expected after the move: 6 passed.

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d11 cargo test --lib secure_core::golden_tests::golden_print -- --ignored --nocapture
# expected (before step 4): two lines SEALED_HEX=<hex> and HMAC_HEX=<64 hex>
CARGO_TARGET_DIR=D:/pm-night/target-d11 cargo test --lib secure_core
# expected: test result: ok. 6 passed; 0 failed; 1 ignored  (plus any tests that moved inside crypto.rs)
CARGO_TARGET_DIR=D:/pm-night/target-d11 cargo test --lib team
# expected: 0 failed
```

### Done when
- [ ] `git log --follow packages/app/src-tauri/src/secure_core/crypto.rs` shows the old team history (it was a move).
- [ ] Golden hex constants were recorded BEFORE the move (two commits: "golden tests" then "move").
- [ ] All team tests pass with no other team file edited.

### Do NOT
- R1-R7.
- Do not change any function body in `crypto.rs` during the move.
- Do not edit any other `team/*.rs` file; the shim is the only team change.
- Do not regenerate the golden values after the move.

### If it fails
1. `sealed_bytes_are_unchanged` fails after the move: you changed bytes. `git diff` the moved file against
   `git show HEAD~1:packages/app/src-tauri/src/team/crypto.rs`; undo every non-visibility change.
2. Team compile error "private item": a `pub(super)` item; make it `pub` in `secure_core/crypto.rs`.
3. Tests inside the moved file use `crate::team::...` paths: they still resolve (team still exists); leave them.

---

## D1.2 `secure_core/sign.rs` and `secure_core/grant.rs`
Needs merged first: D1.1.

### Goal
Add domain-separated Ed25519 sign/verify and "seal one 32-byte key to one or many devices" helpers, used by every
Spaces format.

### Read first
- `packages/app/src-tauri/src/secure_core/crypto.rs` (`hpke_seal`, `hpke_open`, `KemSecret`, `CryptoError`).
- `packages/app/src-tauri/Cargo.toml` line 48 (`ed25519-dalek = 2`, facts §5).

### Files
- create `packages/app/src-tauri/src/secure_core/sign.rs`
- create `packages/app/src-tauri/src/secure_core/grant.rs`
- edit `packages/app/src-tauri/src/secure_core/mod.rs`

### Steps
1. Create both files as below.
2. `secure_core/mod.rs`: add `pub mod grant;` and `pub mod sign;` under `pub mod crypto;`.

### Code
`secure_core/sign.rs`:
```rust
//! Ed25519 with a domain string, so a signature for one format never verifies as another.
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// The signed bytes: len(domain) as one byte, the domain, then the message.
fn framed(domain: &[u8], msg: &[u8]) -> Vec<u8> {
    assert!(domain.starts_with(b"pm/") && domain.len() < 256, "bad signing domain");
    let mut v = Vec::with_capacity(1 + domain.len() + msg.len());
    v.push(domain.len() as u8);
    v.extend_from_slice(domain);
    v.extend_from_slice(msg);
    v
}

pub fn sign(key: &SigningKey, domain: &[u8], msg: &[u8]) -> [u8; 64] {
    key.sign(&framed(domain, msg)).to_bytes()
}

pub fn verify(public: &[u8; 32], domain: &[u8], msg: &[u8], sig: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(public) else { return false };
    vk.verify(&framed(domain, msg), &Signature::from_bytes(sig)).is_ok()
}

pub fn public_of(key: &SigningKey) -> [u8; 32] {
    key.verifying_key().to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> SigningKey { SigningKey::from_bytes(&[3u8; 32]) }

    #[test]
    fn signature_verifies() {
        let s = sign(&key(), b"pm/test/v1", b"hello");
        assert!(verify(&public_of(&key()), b"pm/test/v1", b"hello", &s));
    }
    #[test]
    fn other_domain_fails() {
        let s = sign(&key(), b"pm/test/v1", b"hello");
        assert!(!verify(&public_of(&key()), b"pm/test/v2", b"hello", &s));
    }
    #[test]
    fn changed_message_fails() {
        let s = sign(&key(), b"pm/test/v1", b"hello");
        assert!(!verify(&public_of(&key()), b"pm/test/v1", b"hellp", &s));
    }
    #[test]
    fn bad_public_key_fails_without_panic() {
        let s = sign(&key(), b"pm/test/v1", b"hello");
        assert!(!verify(&[0xffu8; 32], b"pm/test/v1", b"hello", &s));
    }
    #[test]
    #[should_panic(expected = "bad signing domain")]
    fn domain_must_start_with_pm() {
        let _ = sign(&key(), b"x/test", b"hello");
    }
}
```

`secure_core/grant.rs`:
```rust
//! Seal a 32-byte key (for example a space epoch key) to devices by their KEM public key (HPKE).
use super::crypto::{hpke_open, hpke_seal, CryptoError, KemSecret};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedKey {
    /// Which device can open it (its signing public key; used only to find the right grant).
    pub device: [u8; 32],
    pub enc: Vec<u8>,
    pub ct: Vec<u8>,
}

pub fn seal_key_to(device: [u8; 32], kem_public_b64: &str, info: &[u8], key: &[u8; 32])
    -> Result<SealedKey, CryptoError>
{
    let (enc, ct) = hpke_seal(kem_public_b64, info, key)?;
    Ok(SealedKey { device, enc, ct })
}

pub fn seal_key_to_many(recipients: &[([u8; 32], String)], info: &[u8], key: &[u8; 32])
    -> Result<Vec<SealedKey>, CryptoError>
{
    recipients.iter().map(|(d, kem)| seal_key_to(*d, kem, info, key)).collect()
}

pub fn open_sealed_key(sealed: &SealedKey, kem: &KemSecret, info: &[u8]) -> Option<[u8; 32]> {
    let plain = hpke_open(kem, &sealed.enc, info, &sealed.ct).ok()?;
    plain.as_slice().try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_key_opens_with_the_right_secret() {
        let kem = KemSecret::from_bytes([1u8; 32]);
        let s = seal_key_to([9u8; 32], &kem.public_b64(), b"pm/spaces/epoch/v1", &[4u8; 32]).unwrap();
        assert_eq!(open_sealed_key(&s, &kem, b"pm/spaces/epoch/v1"), Some([4u8; 32]));
    }
    #[test]
    fn wrong_info_does_not_open() {
        let kem = KemSecret::from_bytes([1u8; 32]);
        let s = seal_key_to([9u8; 32], &kem.public_b64(), b"pm/a", &[4u8; 32]).unwrap();
        assert_eq!(open_sealed_key(&s, &kem, b"pm/b"), None);
    }
    #[test]
    fn wrong_secret_does_not_open() {
        let kem = KemSecret::from_bytes([1u8; 32]);
        let other = KemSecret::from_bytes([2u8; 32]);
        let s = seal_key_to([9u8; 32], &kem.public_b64(), b"pm/a", &[4u8; 32]).unwrap();
        assert_eq!(open_sealed_key(&s, &other, b"pm/a"), None);
    }
    #[test]
    fn many_gives_one_grant_per_device_in_order() {
        let a = KemSecret::from_bytes([1u8; 32]);
        let b = KemSecret::from_bytes([2u8; 32]);
        let list = vec![([1u8; 32], a.public_b64()), ([2u8; 32], b.public_b64())];
        let g = seal_key_to_many(&list, b"pm/a", &[4u8; 32]).unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].device, [1u8; 32]);
        assert_eq!(open_sealed_key(&g[1], &b, b"pm/a"), Some([4u8; 32]));
    }
}
```

### Tests to write
The 5 tests in `sign.rs` and 4 tests in `grant.rs` above (names as written). Expected: all pass.

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d12 cargo test --lib secure_core
# expected: test result: ok. 15 passed; 0 failed; 1 ignored
```

### Done when
- [ ] 9 new tests pass, 6 golden tests still pass.
- [ ] No other file changed.

### Do NOT
- R1-R7.
- Do not use `rand` or `SigningKey::generate` in these files; tests use fixed bytes.
- Do not add a new crate.

### If it fails
1. `SigningKey::from_bytes` takes `&[u8; 32]` in ed25519-dalek 2 (it does); if the compiler says otherwise, use
   `SigningKey::from_bytes(&bytes)` exactly as `team/` code does (search `SigningKey::from_bytes` in `team/`).
2. `hpke_seal` error type is not `CryptoError`: use the type it returns, in all three signatures.
3. Count is not 15: `crypto.rs` brought its own tests; the expected count is 15 plus those. Only `0 failed` matters.

---

## D1.3 `spaces/wire.rs` and `spaces/ids.rs`
Needs merged first: D0.3, D1.1.

### Goal
Add the byte writer/reader (big-endian, length-prefixed) and base32 used by every Spaces format, and the id types.

### Read first
- `packages/app/src-tauri/src/secure_core/crypto.rs` (`random_bytes`).
- `packages/app/src-tauri/src/spaces/mod.rs`.

### Files
- create `packages/app/src-tauri/src/spaces/wire.rs`
- create `packages/app/src-tauri/src/spaces/ids.rs`
- edit `packages/app/src-tauri/src/spaces/mod.rs`

### Steps
1. Create `wire.rs` and `ids.rs` as below.
2. `spaces/mod.rs`: add `pub mod ids;` and `pub mod wire;` under `pub mod limits;`.

### Code
`spaces/wire.rs`:
```rust
//! Byte format used by every Spaces structure. Integers are big-endian.
//! str = u16 length + UTF-8. blob = u32 length + bytes. Nothing else.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub &'static str);

#[derive(Default)]
pub struct Writer { buf: Vec<u8> }

impl Writer {
    pub fn new() -> Self { Self { buf: Vec::new() } }
    pub fn u8(&mut self, v: u8) -> &mut Self { self.buf.push(v); self }
    pub fn u16(&mut self, v: u16) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn u32(&mut self, v: u32) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn u64(&mut self, v: u64) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn fixed(&mut self, b: &[u8]) -> &mut Self { self.buf.extend_from_slice(b); self }
    /// Panics if longer than 65535 bytes; callers clean and cap text first.
    pub fn str(&mut self, s: &str) -> &mut Self {
        assert!(s.len() <= u16::MAX as usize, "str too long");
        self.u16(s.len() as u16).fixed(s.as_bytes())
    }
    pub fn blob(&mut self, b: &[u8]) -> &mut Self {
        assert!(b.len() <= u32::MAX as usize, "blob too long");
        self.u32(b.len() as u32).fixed(b)
    }
    pub fn len(&self) -> usize { self.buf.len() }
    pub fn bytes(&self) -> &[u8] { &self.buf }
    pub fn finish(self) -> Vec<u8> { self.buf }
}

pub struct Reader<'a> { buf: &'a [u8], pos: usize }

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self { Self { buf, pos: 0 } }
    pub fn pos(&self) -> usize { self.pos }
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(n).ok_or(WireError("overflow"))?;
        if end > self.buf.len() { return Err(WireError("short")); }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, WireError> { Ok(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Result<u16, WireError> { Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap())) }
    pub fn u32(&mut self) -> Result<u32, WireError> { Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap())) }
    pub fn u64(&mut self) -> Result<u64, WireError> { Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap())) }
    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N], WireError> { Ok(self.take(N)?.try_into().unwrap()) }
    pub fn str(&mut self, max_bytes: usize) -> Result<String, WireError> {
        let n = self.u16()? as usize;
        if n > max_bytes { return Err(WireError("str too long")); }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| WireError("not utf-8"))
    }
    pub fn blob(&mut self, max: usize) -> Result<&'a [u8], WireError> {
        let n = self.u32()? as usize;
        if n > max { return Err(WireError("blob too long")); }
        self.take(n)
    }
    pub fn done(&self) -> Result<(), WireError> {
        if self.pos == self.buf.len() { Ok(()) } else { Err(WireError("trailing bytes")) }
    }
}

const B32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// RFC 4648 base32, lowercase, no padding.
pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() * 8 + 4) / 5);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in data {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 { out.push(B32[((acc << (5 - bits)) & 31) as usize] as char); }
    out
}

/// Accepts upper or lower case; rejects any other character and non-zero leftover bits.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = B32.iter().position(|&x| x == c.to_ascii_lowercase())? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    if bits >= 5 || (acc & ((1 << bits) - 1)) != 0 { return None; }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_reader_round_trip() {
        let mut w = Writer::new();
        w.u8(1).u16(2).u32(3).u64(4).fixed(&[5u8; 4]).str("héllo").blob(&[6, 7]);
        let b = w.finish();
        let mut r = Reader::new(&b);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.u16().unwrap(), 2);
        assert_eq!(r.u32().unwrap(), 3);
        assert_eq!(r.u64().unwrap(), 4);
        assert_eq!(r.fixed::<4>().unwrap(), [5u8; 4]);
        assert_eq!(r.str(100).unwrap(), "héllo");
        assert_eq!(r.blob(10).unwrap(), &[6, 7]);
        assert!(r.done().is_ok());
    }
    #[test]
    fn short_input_is_an_error_not_a_panic() {
        assert_eq!(Reader::new(&[0, 1]).u32(), Err(WireError("short")));
    }
    #[test]
    fn too_long_str_is_rejected() {
        let mut w = Writer::new();
        w.str("abcdef");
        assert_eq!(Reader::new(&w.finish()).str(3), Err(WireError("str too long")));
    }
    #[test]
    fn trailing_bytes_are_rejected() {
        let mut r = Reader::new(&[1, 2]);
        r.u8().unwrap();
        assert_eq!(r.done(), Err(WireError("trailing bytes")));
    }
    #[test]
    fn base32_matches_rfc4648_vectors() {
        assert_eq!(base32_encode(b""), "");
        assert_eq!(base32_encode(b"f"), "my");
        assert_eq!(base32_encode(b"fo"), "mzxq");
        assert_eq!(base32_encode(b"foo"), "mzxw6");
        assert_eq!(base32_encode(b"foob"), "mzxw6yq");
        assert_eq!(base32_encode(b"fooba"), "mzxw6ytb");
        assert_eq!(base32_encode(b"foobar"), "mzxw6ytboi");
    }
    #[test]
    fn base32_round_trip_and_rejects_junk() {
        let data: Vec<u8> = (0u8..=200).collect();
        assert_eq!(base32_decode(&base32_encode(&data)).unwrap(), data);
        assert_eq!(base32_decode("MZXW6YTBOI").unwrap(), b"foobar");
        assert_eq!(base32_decode("mzxw6ytb0i"), None); // '0' is not in the alphabet
        assert_eq!(base32_decode("mz"), Some(b"f".to_vec()));
        assert_eq!(base32_decode("mz7"), None); // leftover non-zero bits
    }
}
```

`spaces/ids.rs`:
```rust
//! Id types. All are random bytes or public keys, shown as lowercase hex.
use crate::secure_core::crypto::random_bytes;

pub fn to_hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

pub fn from_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.trim();
    if s.len() != N * 2 || !s.bytes().all(|c| c.is_ascii_hexdigit()) { return None; }
    let mut out = [0u8; N];
    for i in 0..N { out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?; }
    Some(out)
}

macro_rules! id_type {
    ($name:ident, $n:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub [u8; $n]);
        impl $name {
            pub fn to_hex(&self) -> String { to_hex(&self.0) }
            pub fn from_hex(s: &str) -> Option<Self> { from_hex::<$n>(s).map(Self) }
        }
    };
}

id_type!(SpaceId, 16);
id_type!(ChannelId, 8);
id_type!(InviteId, 8);
id_type!(EntryId, 32);
id_type!(NodeKey, 32);
id_type!(MemberKey, 32);
id_type!(DeviceKey, 32);

impl SpaceId { pub fn random() -> Self { Self(random_bytes::<16>()) } }
impl ChannelId { pub fn random() -> Self { Self(random_bytes::<8>()) } }
impl InviteId { pub fn random() -> Self { Self(random_bytes::<8>()) } }

impl NodeKey {
    /// First 8 hex chars, shown as XXXX-XXXX in the UI (like pairing's display_code style).
    pub fn short(&self) -> String {
        let h = self.to_hex().to_uppercase();
        format!("{}-{}", &h[0..4], &h[4..8])
    }
}

/// Keystore "team" name for a space: epoch keys and invite secrets live under it.
pub fn space_tag(space: &SpaceId) -> String { format!("space:{}", space.to_hex()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_round_trip() {
        let s = SpaceId([0xab; 16]);
        assert_eq!(s.to_hex(), "ab".repeat(16));
        assert_eq!(SpaceId::from_hex(&s.to_hex()), Some(s));
    }
    #[test]
    fn bad_hex_is_none() {
        assert_eq!(ChannelId::from_hex("zz00000000000000"), None);
        assert_eq!(ChannelId::from_hex("00"), None);
    }
    #[test]
    fn random_ids_differ() {
        assert_ne!(SpaceId::random(), SpaceId::random());
    }
    #[test]
    fn space_tag_format() {
        assert_eq!(space_tag(&SpaceId([1; 16])), format!("space:{}", "01".repeat(16)));
    }
    #[test]
    fn node_short_is_two_groups_of_four() {
        assert_eq!(NodeKey([0x4f; 32]).short(), "4F4F-4F4F");
    }
}
```

### Tests to write
6 tests in `wire.rs`, 5 in `ids.rs` (names above). Expected: all pass.

### Run
```bash
cd packages/app/src-tauri
CARGO_TARGET_DIR=D:/pm-night/target-d13 cargo test --lib spaces::wire spaces::ids
# cargo takes one filter; run twice:
CARGO_TARGET_DIR=D:/pm-night/target-d13 cargo test --lib spaces::wire
# expected: test result: ok. 6 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d13 cargo test --lib spaces::ids
# expected: test result: ok. 5 passed; 0 failed
CARGO_TARGET_DIR=D:/pm-night/target-d13 cargo test --lib spaces::guard_tests
# expected: test result: ok. 7 passed; 0 failed
```

### Done when
- [ ] 11 new tests pass; Spaces guard tests pass (both files are in `PURE` and use no fs/net).

### Do NOT
- R1-R7.
- Do not add the `hex` or `base32` crates.
- Do not let `Reader` panic on any input (every read returns `Result`).

### If it fails
1. `random_bytes` is not `pub`: it is (facts §3 line 36); check the import path `crate::secure_core::crypto`.
2. RFC vector mismatch: the leftover-bits branch is wrong; compare with the code above character by character.
3. Macro type errors with `$n`: write the seven structs by hand instead of the macro.

---

## Open facts

- NEEDS-FACT F-1: the bodies of `P2p::apply_settings`, `P2p::want`, `P2p::unwant`, and the type of the stored Labs
  flag. Read `packages/app/src-tauri/src/p2p/mod.rs` in full (D0.2 step 3-4 depends on it).
- NEEDS-FACT F-2: where the app constructs its single `P2p` (file and line) and where it keeps the `Arc`. Search
  `P2p::new(` in `packages/app/src-tauri/src` (D0.2 wiring note, D2.5).
- NEEDS-FACT F-3: the iroh 1.4.1 API to (a) add a relay URL to an `EndpointAddr` when dialing and (b) read the
  endpoint's current home relay URL. Read `packages/app/src-tauri/src/p2p/iroh_net.rs` and the `iroh` 1.4.1 crate
  docs in `~/.cargo/registry/src/*/iroh-1.4.1/src/endpoint.rs`.
- NEEDS-FACT F-4: the variants of `P2pError` and `RelayPolicy` (facts §"Could not find" 4). Read
  `p2p/endpoint.rs` line 70 and `p2p/capability.rs`.
- NEEDS-FACT F-5: the `SecretBackend` trait's methods (for the spike's in-memory backend). Read
  `packages/app/src-tauri/src/team/keystore.rs` lines 1-90.
- NEEDS-FACT F-6: how `PeerAddr` stores LAN addresses (field name and type). Read `p2p/endpoint.rs` line 50 and
  `p2p/pairing.rs` `parse_ticket`.
- NEEDS-FACT F-7: whether `team/crypto.rs` has `pub(super)`/`pub(crate)` items and whether `team/mod.rs` declares
  `pub mod crypto`. Read both files (D1.1).
- NEEDS-FACT F-8: the exact import used by `useDeskEnabled` in `packages/app/src/lib/desk.ts` (D0.1 step 5).
- NEEDS-FACT F-9: the settings TypeScript type that holds `desk_enabled?: boolean` (D0.1 step 4). Search
  `desk_enabled?` in `packages/app/src`.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test that proves it |
|---|---|---|---|
| A space member (or anyone allowed on `spaces`) tries desk input | Dial `pm/desk/1` with a node id that Spaces allows | `RoutedPermissions` answers `spaces` only; desk asks its own base rule | `space_member_is_refused_on_desk_alpn` |
| Turning on Spaces silently enables My computers | User flips `spaces_enabled` only | Per-capability Labs key | `want_desk_still_needs_desk_enabled` |
| Turning on My computers silently enables Spaces | User flips `desk_enabled` only | Same | `want_spaces_fails_with_only_desk_enabled` |
| A new Spaces file reaches the network directly | Code in `spaces/` opens a socket or names `p2p` | Spaces guard tests + p2p allow-list | `only_net_rs_names_the_p2p_module`, `pure_spaces_files_open_no_files_or_sockets`, p2p `only_one_file_touches_the_network_and_no_test_binds_it` |
| Agents or the phone drive Spaces | Bridge/MCP/phone code calls a Spaces command | Guard scans the same roots as the p2p guard | `spaces_is_not_reachable_from_the_bridge_mcp_or_the_phone` |
| Crypto move changes bytes, breaking teams' stored data | A refactor slip in `crypto.rs` | Golden tests recorded before the move | `sealed_bytes_are_unchanged`, `hmac_is_unchanged` |
| Signature reuse across formats | A control-op signature replayed as a message signature | Domain-separated signing | `other_domain_fails` |
| Parser crash from hostile bytes | Short or oversized frames | `Reader` returns errors, caps lengths | `short_input_is_an_error_not_a_panic`, `too_long_str_is_rejected` |
| Relay reads messages | Relay sees packets | QUIC TLS end to end (iroh); relays forward ciphertext only | not testable locally; see below |

## What cannot be verified without real hardware or a second computer

- D0.4 as a whole: hole punching across real NATs, relay fallback, connect time, RTT and 1 MiB echo time need two
  computers on different networks.
- That the public iroh relays are reachable from the founder's network and from a typical home router.
- Firewall prompts on Windows and macOS when the endpoint first binds with relays on.
- `home_relay()` returning a real URL (MemoryNet always returns `None`).
