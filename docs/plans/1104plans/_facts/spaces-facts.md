# Spaces facts (code reader output)

Repo root: `C:\Users\Ren-pc\Desktop\work2\puppet-master`. Every item below was seen in the file named. Line numbers are from the files as they are on disk (branch `main`, 2026-11-04). Nothing here is a plan; nothing was changed.

Not found or not read (see the end too): `docs/security/` does not exist. `potato-brain/src/rooms/transport.ts` was not checked. `gossip`/`blobs` crates are not in Cargo.toml.

---

## 1. `packages/app/src-tauri/src/p2p/`

Module list in `p2p/mod.rs` (lines 26-43): `pub mod alpn; capability; endpoint; identity; iroh_net; memory` (memory is `#[cfg(test)]`), `discovery; pairing; session; trust`. Test-only: `guard_tests`, `pair_tests`, `tests` (all `#[cfg(test)]`).

| File | Purpose (from its `//!` header) | Public items (exact) |
|---|---|---|
| `alpn.rs` | Protocol names (ALPN) the link layer accepts | `pub const SPACES: &[u8] = b"pm/spaces/1";` (line 7, comment "Teams and spaces (Spaces D-0 uses this one)") · `pub const DESK: &[u8] = b"pm/desk/1";` · `pub const DESK_CLIPBOARD: &[u8] = b"pm/desk-clipboard/1";` · `pub const FILES: &[u8] = b"pm/files/1";` · `pub const PREFIX: &[u8] = b"pm/";` · `pub const MAX_LEN: usize = 48;` · `pub const PAIR: &[u8] = b"pm/pair/1";` · `pub fn is_valid(alpn: &[u8]) -> bool` · `pub fn relay_possible(alpn: &[u8]) -> bool` |
| `capability.rs` | The registry of capabilities and the per-device permission trait | `pub const DESK_INPUT: &str = "desk/input";` · `DESK_CLIPBOARD: &str = "desk/clipboard"` · `FILES: &str = "files"` · `SPACES: &str = "spaces"` (line 16) · `PAIR: &str = "pair"` · `pub const PERMISSIONABLE: [&str; 3] = [DESK_INPUT, DESK_CLIPBOARD, FILES];` (line 21; **SPACES is not in it**) · `pub enum RelayPolicy` (variant `Never` is used by pairing; others not read) · `pub trait CapabilityHandler: Send + Sync` · `pub struct Capability` with `pub fn new(id: &str, alpn: &[u8], relay: RelayPolicy, handler: Arc<dyn CapabilityHandler>) -> Self` and `pub fn relay_allowed(&self) -> bool` · `pub struct Registry` with `pub fn new() -> Self`, `pub fn register(&self, cap: Capability) -> Result<(), P2pError>`, `pub fn get(&self, id: &str) -> Option<Arc<Capability>>`, `pub fn by_alpn(&self, alpn: &[u8]) -> Option<Arc<Capability>>`, `pub fn ids(&self) -> Vec<String>` · `pub trait Permissions: Send + Sync { fn allowed(&self, peer: &NodeId, capability: &str) -> bool; fn set(&self, peer: &NodeId, capability: &str, on: bool); }` (line 101-104; "Default: nobody, nothing") · `pub struct MemoryPermissions` with `new()` |
| `endpoint.rs` | The two seams (Transport/Link) plus `admit`. "Nothing here knows what a capability does." | `pub struct NodeId(pub [u8; 32]);` (line 24, Ed25519 public key) with `pub fn to_hex(&self) -> String`, `pub fn short(&self) -> String` · `pub struct PeerAddr` (line 50) with `pub fn new(id: NodeId) -> Self` · `pub enum PathKind` (line 62) · `pub enum P2pError` (line 70) · `pub struct BiStream { pub send: Box<dyn AsyncWrite + Send + Unpin>, pub recv: Box<dyn AsyncRead + Send + Unpin> }` (line 90) · **`#[async_trait] pub trait Link: Send + Sync`** (line 97): `fn peer(&self) -> NodeId; fn alpn(&self) -> Vec<u8>; fn path(&self) -> PathKind; async fn wait_path_change(&self, from: PathKind) -> Option<PathKind>; async fn open_bi(&self) -> Result<BiStream, P2pError>; async fn accept_bi(&self) -> Result<BiStream, P2pError>; async fn send_datagram(&self, data: Vec<u8>) -> Result<(), P2pError>; async fn recv_datagram(&self) -> Result<Vec<u8>, P2pError>; fn close(&self, code: u32, reason: &str); async fn closed(&self) -> u32;` · **`#[async_trait] pub trait Transport: Send + Sync`** (line 114): `fn id(&self) -> NodeId; async fn set_alpns(&self, alpns: Vec<Vec<u8>>); async fn dial(&self, peer: &PeerAddr, alpn: &[u8]) -> Result<Arc<dyn Link>, P2pError>; async fn accept(&self) -> Option<Arc<dyn Link>>; async fn close(&self); fn local_addrs(&self) -> Vec<SocketAddr> {Vec::new()}` · `pub struct BindSpec { pub alpns: BTreeSet<Vec<u8>>, pub relays: bool, pub loopback_only: bool }` (line 131) · **`#[async_trait] pub trait TransportFactory: Send + Sync`** (line 140): `async fn bind(&self, identity: &DeviceIdentity, spec: &BindSpec) -> Result<Arc<dyn Transport>, P2pError>;` · consts `CLOSE_UNKNOWN_ALPN: u32 = 1`, `CLOSE_NOT_ALLOWED = 2`, `CLOSE_RELAY = 3`, `CLOSE_OFF = 4` · `pub enum RefusalKind` (line 152) · `pub struct Refusal` (line 161) · `pub type RefusalLog = Arc<Mutex<Vec<Refusal>>>;` · `pub fn spawn_path_guard(link: Arc<dyn Link>)` (line 188) · `pub fn admit(link: &Arc<dyn Link>, registry: &Registry, perms: &dyn Permissions, log: &RefusalLog) -> Option<Arc<Capability>>` (line 206) · `pub fn check_dialed(link: &Arc<dyn Link>, cap: &Capability) -> Result<(), P2pError>` (line 226) · `pub async fn accept_loop(` (line 238, signature continues; not read in full) · `pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);` (line 260) |
| `identity.rs` | This device's node identity, from the team keystore. Secret never leaves memory. | `pub const BINDING_PREFIX: &[u8] = b"pm/p2p/node-binding/v1";` · `pub struct NodeBinding` (line 20) with `pub fn verify(&self) -> bool` · `pub struct DeviceIdentity` (line 46) with `pub fn load(keys: &KeyStore) -> Result<Self, P2pError>`, `pub fn id(&self) -> NodeId`, `pub fn binding(&self) -> NodeBinding`. (`secret_bytes` is `pub(super)`, per guard test.) |
| `iroh_net.rs` | The real network: iroh endpoint (QUIC, dial by public key, hole punching, optional relay). The only real transport. | `pub struct IrohFactory;` (line 31) with `impl TransportFactory`. Private `IrohTransport` (impl Transport, line 70), `IrohLink` (impl Link, line 131). Uses `iroh::endpoint::{presets, Connection}`, `iroh::{EndpointAddr, RelayMode, SecretKey}`. No address-lookup service configured; relays only when `BindSpec.relays`. No test binds it. |
| `memory.rs` (test only) | In-memory network for tests, no sockets, no clock | `pub struct MemoryNet` (line 42) with `pub fn new() -> Self`, `pub fn factory(&self) -> Arc<dyn TransportFactory>`, `pub fn bind_count(&self) -> usize`, `pub fn spec_of(&self, id: &NodeId) -> Option<BindSpec>`, `pub fn default_to_relayed(&self, on: bool)`, `pub fn blackhole(&self, a: NodeId, b: NodeId, on: bool)`, `pub fn set_path(&self, a: NodeId, b: NodeId, kind: PathKind)`, `pub fn bind_raw(&self, id: NodeId, alpns: &[&[u8]]) -> Arc<dyn Transport>` |
| `discovery.rs` | "Found nearby": LAN announcements of computers showing a pairing code. Seam, not radio. | `pub struct Announcement` (line 17) · `pub trait Discovery: Send + Sync { fn announce(&self, a: Announcement); fn withdraw(&self, node: &NodeId); fn nearby(&self) -> Vec<Announcement>; }` (line 23) · `pub struct NoDiscovery;` (ships by default: "Add by address" always works) · `pub struct MemoryLan` with `pub fn new() -> Self` · `pub struct MdnsLan` with `pub fn new() -> Result<Self, String>` (line 80; uses `mdns_sd::ServiceInfo`) |
| `pairing.rs` | Pairing two computers with a short code. Result: a trust-list entry with every capability off. | See section below ("pairing.rs detail"). |
| `session.rs` | Link session rules: which of two links wins, dial retries, heartbeat/silence | Constants: `pub const HEARTBEAT_MS: u64 = 100;` · `STALL_MS: u64 = 400` · `SETTLE_MS: u64 = 2_000` · `DEAD_MS: u64 = 3_000` · `RETRY_FIRST_MS: u64 = 1_000` · `RETRY_MAX_MS: u64 = 10_000`. Fns: `pub fn is_preferred(dialed: bool, local: &NodeId, remote: &NodeId) -> bool` · `pub fn decide_install(existing: Option<Installed>, new_dialed: bool, local: &NodeId, remote: &NodeId, now_ms: u64) -> InstallAction`. Types: `pub enum InstallAction` · `pub struct Installed` · `pub struct DialLedger` (`new`, `begin(&mut self, peer, now_ms) -> bool`, `in_flight`, `waiting`, `succeeded`, `failed`, `forget`) · `pub enum Reach` · `pub struct Sessions` (`new`, `new_link`, `handshake_started`, `handshake_ended`, `current`, `install(&mut self, peer, id, dialed, local, now_ms) -> (InstallAction, Option<u64>)`, `link_ended`, `is_current`, `reach`) · `pub struct Silence` (`new(now_ms)`, `heard`, `check(&mut self, now_ms) -> SilenceVerdict`, `is_stalled`, `quiet_for`) · `pub enum SilenceVerdict` |
| `trust.rs` | The signed list of paired computers and what each may do. Public halves only. | `pub trait TrustStorage: Send + Sync` (line 28) · `pub trait TrustSigner: Send + Sync` (line 34) · `pub struct KeyStoreSigner` with `pub fn new(keys: KeyStore) -> Self` · `pub struct MemoryTrustStorage` with `new() -> Arc<Self>`, `put(&self, bytes: Vec<u8>)` · `pub struct TrustedDevice` (line 98) · `pub fn clean_name(raw: &str) -> String` (line 106) · `pub enum TrustError` (line 193) · `pub struct TrustList` with `pub fn open(storage: Arc<dyn TrustStorage>, signer: Arc<dyn TrustSigner>) -> Arc<Self>`, `tampered(&self) -> bool`, `list(&self) -> Vec<TrustedDevice>`, `get(&self, node: &NodeId) -> Option<TrustedDevice>`, `is_trusted(&self, node: &NodeId) -> bool`, `set_pairing_open(&self, open: bool)`, `pairing_open(&self) -> bool`, `add(&self, node: NodeId, device_pub: [u8; 32], name: &str, now_secs: u64) -> Result<(), TrustError>`, `remove(&self, node: &NodeId) -> Result<(), TrustError>`, `set_permission(&self, node: &NodeId, capability: &str, on: bool) -> Result<(), TrustError>`. A list that fails the signature check on load is ignored and `tampered()` is true. Its file is written by `pairing_commands.rs`, not here. |
| `mod.rs` | The `P2p` service: registry, want/dial, Labs check | `pub const LABS_KEY: &str = "desk_enabled";` (line 61) · `pub fn labs_on(public: &Value) -> bool` (line 63) · `pub struct AddrBook` (`remember`, `peer`) · `pub struct P2p` with `pub fn new(keys: KeyStore, factory: Arc<dyn TransportFactory>, registry: Arc<Registry>, perms: Arc<dyn Permissions>) -> Self`, `loopback_only(mut self, on: bool) -> Self`, `addrs()`, `registry()`, `permissions()`, `labs()`, `apply_settings(&self, public: &Value)`, `is_running()`, `node_id() -> Option<NodeId>`, `local_addrs()`, `bound_spec()`, `refusals() -> Vec<Refusal>`, `pub async fn want(&self, capability: &str) -> Result<Arc<dyn Transport>, P2pError>`, `pub async fn unwant(&self, capability: &str)`, `pub async fn dial(&self, peer: &PeerAddr, capability: &str) -> Result<Arc<dyn Link>, P2pError>`, `stop(&self)` |

### pairing.rs detail

- File header (lines 1-50): hosts show a code, the joiner enters it (or picks from "found nearby"). Joiner dials `pm/pair/1` (`alpn::PAIR`). Host admits only while a code is on screen and only on a direct path. Five-step exchange: hello, challenge, proof, accept, then both people confirm on screen with `decision` (HMAC-tagged). Nothing is trusted until both say yes. No PAKE.
- Constants (exact): `pub const CODE_TTL_MS: u64 = 120_000;` (line 59, code lives 2 minutes) · `pub const LOCKOUT_MS: u64 = 600_000;` (line 60, 10 minutes) · `pub const MAX_STRIKES: u32 = 3;` (line 61) · private `const CODE_LEN: usize = 8;` (line 62) · private `const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";` (line 64, 32 chars, no 0/O/1/I) · private `const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);` (line 67) · private `const TICKET_PREFIX: &str = "pm1:";` (line 69).
- Code format: `pub fn normalize_code(raw: &str) -> Option<String>` (strips spaces and `-`, uppercases, 8 chars from ALPHABET). `pub fn display_code(code: &str) -> String` returns `format!("{}-{}", &code[..4], &code[4..])`, e.g. `4F7Q-9K2M`.
- Ticket: `pub fn make_ticket(node: &NodeId, addrs: &[SocketAddr]) -> String` gives `pm1:` + 64 hex chars of the node id, then for each LAN address `@<addr>` (e.g. `pm1:<hex>@192.168.1.5:4000`). `pub fn parse_ticket(raw: &str) -> Option<PeerAddr>` reverses it.
- Other pub items: `pub enum PairError` (line 109; variants not read), `pub trait Clock: Send + Sync` (line 75), `pub struct SystemClock` (79), `pub struct ManualClock` with `pub fn new(start_ms: u64) -> Arc<Self>` and `advance(&self, ms: u64)` (88-95), `pub enum Role { Host, Joiner }` (line 313), `pub type PairNotifier = Arc<dyn Fn(&Value) + Send + Sync>;` (line 355), `pub fn permissionable() -> BTreeSet<&'static str>` (line 830).
- `pub struct PairManager` (line 357). Fields are private: `p2p: Arc<P2p>, trust: Arc<TrustList>, keys: KeyStore, clock: Arc<dyn Clock>, discovery: Arc<dyn Discovery>, name: String, state: Mutex<State>, notifier: Mutex<Option<PairNotifier>>`. Methods:
  - `pub fn new(p2p: Arc<P2p>, trust: Arc<TrustList>, keys: KeyStore, clock: Arc<dyn Clock>, discovery: Arc<dyn Discovery>, name: &str) -> Result<Arc<Self>, P2pError>` (line 392). Registers the `capability::PAIR` capability (`RelayPolicy::Never`) on `p2p.registry()`. Starts nothing.
  - `pub fn set_notifier(&self, n: PairNotifier)` (414)
  - `pub fn trust(&self) -> &Arc<TrustList>` (418)
  - `pub fn view(&self) -> Value` (477) · `pub fn devices_view(&self) -> Value` (501) · `pub fn nearby(&self) -> Value` (521)
  - `pub async fn start_hosting(self: &Arc<Self>) -> Result<Value, PairError>` (536) · `pub fn cancel_hosting(&self)` (555)
  - `pub async fn join(self: &Arc<Self>, target: &str, code: &str) -> Result<String, PairError>` (636)
  - `pub fn answer(&self, id: u64, allow: bool) -> Result<(), PairError>` (807)
  - `pub fn remove(&self, node: &NodeId) -> Result<(), PairError>` (816)
  - `pub fn set_permission(&self, node: &NodeId, capability: &str, on: bool) -> Result<(), PairError>` (822)
- `notify`/change: private `fn changed(&self)` (line 426) calls the notifier.
- Fixed in the code: `pub const CODE_TTL_MS` is 120 s. The **Spaces plan's invite TTL is in `team/invite.rs`, not here** (see section 3).

### Notes that matter for Spaces

- `admit` refuses an unknown ALPN, a relay on a direct-only capability, and a peer without the permission. A new capability is only reachable if registered and permitted.
- `PERMISSIONABLE` lists 3 ids. `spaces` and `pair` are NOT permissionable through `set_permission`. Spaces would need its own rule if it uses `Permissions`.
- Crates: `iroh = "=1.4.1"` with `default-features = false, features = ["tls-ring"]`; `mdns-sd = "=0.13.11"`. Only these two networking crates. No gossip or blobs crate.

---

## 2. `p2p/guard_tests.rs`

9 `#[test]` functions (file header line 1: "Source guards for the link layer. They read the source files, so a change that breaks a rule fails here."). Helpers: `tauri_src()` = `<crate>/src`, `packages()` = `<crate>/../..`, `files(root, exts)` (skips `node_modules`, `target`, `dist`), `read`, `code_only` (drops `//` comment lines and trailing `//` comments), `shipped()` (all `p2p/*.rs` except `tests.rs`, `guard_tests.rs`, `memory.rs`, `pair_tests.rs`).

| Test | Rule it enforces (plain words) |
|---|---|
| `p2p_is_not_reachable_from_the_bridge_mcp_or_the_phone` | Agents, MCP tools, the phone, the bridge, tool registry, mobile pairing, channels and the CLI may not mention the p2p module or the network crate. Scans roots like `src/bridge`, `src/tool_registry`, `src/mcp_shim`, `src/companion`, `src/mobile_pairing`, `src/channels`, `src/bin`, `packages/mcp-server/src`, `packages/cli/src`, `packages/app/src/pwa`, `PwaApp.tsx`, `PwaLegacyApp.tsx`. |
| `only_known_files_use_the_p2p_module` | Only the p2p module itself, `lib.rs`'s module list, and listed files (desk, "My computers" DL-7/DL-9) may use `p2p`. A new user of the module must add itself to the allowed list in this test, on purpose. |
| `p2p_code_writes_no_file_and_prints_nothing` | Shipped p2p code has none of: `std::fs`, `tokio::fs`, `File::`, `OpenOptions`, `fs::write`, `fs::read`, `env::args`, `env::var`, `println!`, `eprintln!`, `dbg!`, `tracing::`, `info!(`, `debug!(`, `warn!(`, `error!(`, `Serialize`, `serde_json::to_`. The node key never reaches a file, log or argument. |
| `the_secret_is_asked_for_only_by_the_iroh_transport` | `identity.rs` has `pub(super) fn secret_bytes`, no `pub fn secret`; `identity.rs` does not use `keyring`, `SigningKey::generate` or `new_signing_key` (it uses the keystore). |
| `identity_uses_the_existing_keystore` | Node key comes from the team keystore helpers (`KeyStore::p2p_node`), not a second implementation. |
| `only_one_file_touches_the_network_and_no_test_binds_it` | Only `iroh_net.rs` may use `iroh::`, `tokio::net`, `std::net::Tcp`, `std::net::Udp`, `UdpSocket`, `TcpStream`, `TcpListener`, `reqwest`, `ureq`. `tests.rs`, `guard_tests.rs`, `memory.rs` may not name `Iroh`/`iroh_net` or `IrohFactory`. |
| `the_core_knows_no_agent_or_capability_behaviour` | `endpoint.rs`, `iroh_net.rs`, `memory.rs`, `identity.rs` may not contain (case-insensitive) `claude`, `codex`, `agent`, `pane`, `mouse`, `keyboard`, `clipboard`, `desk`. **A Spaces module must not put its words into these four files.** |
| `pairing_and_its_sheet_have_no_route_from_agents_mcp_or_the_phone` | The pairing words `PairManager`, `pairing_commands`, `pair_answer`, `pair_join`, `pair_host_start`, `trust_set_permission`, `trust_remove`, `TrustList`, `p2p::pairing`, `p2p::trust` may not appear in bridge/MCP/phone/CLI roots. |
| `the_sheet_is_answered_in_one_place_and_a_peer_never_sets_permissions` | Only `pairing_commands.rs` (and `pairing*` files) may call `mgr.answer(`. `pairing.rs` must not call `.set(&` or mention `Permissions`. the test asserts that `set_permission(` appears exactly 2 times in `pairing.rs` (the manager method plus its call into the trust list). I did not count it myself. |

Words to avoid in new Spaces code, by these rules: in the 4 core files, the words `claude codex agent pane mouse keyboard clipboard desk`. In any shipped p2p file: `println!`, `tracing::`, `serde_json::to_`, `std::fs`, `env::var`. Any file outside `p2p/` must not use `p2p::` unless listed in the allowed-module test.

---

## 3. `packages/app/src-tauri/src/team/`

`team/mod.rs` header lists the parts: `event` (TeamEvent schema), `crypto` and `keystore` (Ed25519, XChaCha20-Poly1305, HPKE; keys in the credential store), `roster` (signed member list), `secure` (sealing and checks before ingest), `invite` and `membership` (invites, join, approve, remove with key rotation), `transport` (`TeamTransport`, `FolderTransport`, `RelayTransport`), `config` (`team.json`), `sources` (pushes, PRs, CI through `gh`), `store`, `actions` and `merge_green`, `mcp`, `service`/`commands`/`phone`.

Files (all under `team/`): `actions.rs` (git/GitHub actions with safety rules), `commands.rs` (Tauri commands), `config.rs` (`team.json`), `crypto.rs`, `event.rs`, `git.rs`, `invite.rs`, `keystore.rs`, `leakcheck.rs` (test/dev-fake-tui only), `mcp.rs` (agent tools, read-only + drafts), `membership.rs`, `merge_green.rs`, `mod.rs`, `phone.rs` (phone route), `project_key.rs`, `roster.rs`, `secure.rs`, `service.rs` (2051 lines, live wiring), `store.rs`, `transport/{mod.rs, folder.rs, relay.rs}`, `sources/`, `fixtures/`, and test files `tests.rs`, `tests_v1.rs`, `tests_hardening.rs`, `tests_mcp.rs`, `tests_mcp_service.rs`.

### keystore.rs (exact)
- `pub struct KeyStore` (line 17). `pub fn app() -> Self` (51). `pub fn with_backend(backend: Arc<dyn SecretBackend>, prefix: &str) -> Self` (58). `pub fn label(&self) -> &'static str` (62).
- `pub fn member(&self) -> Result<SigningKey, String>` (89) · `pub fn device_sign(&self) -> Result<SigningKey, String>` (94) · `pub fn p2p_node(&self) -> Result<SigningKey, String>` (99) · `pub fn device_kem(&self) -> Result<KemSecret, String>` (104)
- `pub fn epoch_key(&self, team: &str, epoch: u64) -> Result<Option<[u8; 32]>, String>` (108) · `pub fn set_epoch_key(&self, team: &str, epoch: u64, key: &[u8; 32]) -> Result<(), String>` (112)
- `pub fn invite_code(&self, team: &str, invite: &str) -> Result<Option<String>, String>` (116) · `pub fn set_invite_code(&self, team: &str, invite: &str, code: &str) -> Result<(), String>` (120) · `pub fn forget_invite(&self, team: &str, invite: &str) -> Result<(), String>` (124) · `pub fn forget_team(&self, team: &str, last_epoch: u64)` (129)

### crypto.rs (exact)
- `pub const PAD_BUCKETS: [usize; 3] = [1024, 4096, 16384];` (25) · `pub struct CryptoError(pub &'static str);` (28)
- `pub fn random_bytes<const N: usize>() -> [u8; N]` (36) · `pub fn new_signing_key() -> SigningKey` (42)
- `pub struct KemSecret([u8; 32]);` (50) with `pub fn generate() -> Self`, `pub fn from_bytes(bytes: [u8; 32]) -> Self`, `pub fn to_bytes(&self) -> [u8; 32]`, `pub fn public_b64(&self) -> String`
- `pub fn hpke_seal(recipient_b64: &str, info: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError>` (83) · `pub fn hpke_open(secret: &KemSecret, enc: &[u8], info: &[u8], ct: &[u8]) -> Result<Vec<u8>, CryptoError>` (99)
- `pub fn seal_with_key_and_nonce(key: &[u8; 32], header: &[u8], plaintext: &[u8], nonce: [u8; 24]) -> Result<Vec<u8>, CryptoError>` (131) · `pub fn seal_with_key(key: &[u8; 32], header: &[u8], plaintext: &[u8]) -> Result<([u8; 24], Vec<u8>), CryptoError>` (139, pads) · `pub fn open_with_key(key: &[u8; 32], nonce: &[u8], header: &[u8], ct: &[u8]) -> Result<Vec<u8>, CryptoError>` (154) · `pub fn seal_unpadded(...)` (159) · `pub fn open_unpadded(...)` (167)
- `pub fn hmac(key: &[u8], message: &[u8]) -> [u8; 32]` (174) · `pub fn same_mac(a: &[u8], b: &[u8]) -> bool` (182)

### secure.rs (exact)
- `pub enum Payload` (34) · `pub struct Verified` (42; `event()`, `member()`, `into_event()`, `local_fact(event) -> Option<Self>`, `for_test`) · `pub enum Opened` (75) · `pub enum Reject` (83; `as_str`)
- `pub struct DeviceSeq` (118) · `pub struct SeqTracker` (133) with `pub fn observe(&mut self, device: &str, seq: u64, envelope_id: &str, now_ms: u64) -> Result<(), Reject>`, `missing(&self) -> u64`, `clear_gaps(&mut self)`
- `pub struct Session` (190) with `pub fn new(roster: Roster, keys: KeyStore) -> Self`, `team(&self) -> &str`, `my_device(&self) -> Result<String, String>`, `my_member(&self) -> Result<String, String>`, `pub fn seal(&self, payload: &Payload, seq: u64, now_ms: u64) -> Result<Envelope, String>` (213), `pub fn open(&self, env: &Envelope, tracker: &mut SeqTracker, now_ms: u64) -> Result<Opened, Reject>` (243)
- `pub struct SealedKey` (302) · `pub struct KeyGrant` (311) · `pub fn make_grant(roster: &Roster, key: &[u8; 32]) -> Result<KeyGrant, String>` (323) · `pub fn open_grant(grant: &KeyGrant, device_id: &str, kem: &KemSecret) -> Option<[u8; 32]>` (333) · `pub fn grant_blob(grant: &KeyGrant, root: &SigningKey) -> SignedBlob` (340) · `pub fn read_grant(blob: &SignedBlob, team: &str, root: &str) -> Option<KeyGrant>` (346) · `pub fn roster_blob(roster: &Roster, key: &[u8; 32], root: &SigningKey) -> Result<SignedBlob, String>` (367) · `pub fn read_roster_blob(blob: &SignedBlob, team: &str, root: &str, keys: &KeyStore) -> Option<Vec<SignedOp>>` (377)

### roster.rs (exact)
- `pub enum Role` (19) · `pub struct DeviceEntry` (30) · `pub struct MemberEntry` (44) · `pub enum OpBody` (66) · `pub struct RosterOp` (81) · `pub struct SignedOp` (93; `hash(&self) -> String`) · `pub fn device_cert_bytes(team: &str, member: &str, device: &str, kem: &str) -> Vec<u8>` (112) · `pub fn certify_device(team: &str, member_key: &SigningKey, device_key: &SigningKey, kem_public: &str, name: &str) -> DeviceEntry` (117) · `pub struct RosterError(pub String)` (125)
- `pub struct Roster` (140): `pub fn verify(expected_team: &str, ops: &[SignedOp]) -> Result<Roster, RosterError>` (181) · `pub fn create(name: &str, root: &SigningKey, display: &str, device: impl FnOnce(&str) -> DeviceEntry, now_ms: u64) -> Result<Roster, RosterError>` (295) · `pub fn append(&mut self, op: OpBody, root: &SigningKey, now_ms: u64) -> Result<SignedOp, RosterError>` (318) · `pub fn update_from(&mut self, ops: &[SignedOp]) -> Result<bool, RosterError>` (337) · `seq(&self) -> u64` · `member(&self, id: &str) -> Option<&MemberEntry>` · `device(&self, id: &str) -> Option<(&MemberEntry, &DeviceEntry)>` · `all_devices(&self) -> Vec<&DeviceEntry>` · `device_ids(&self) -> Vec<String>` · `is_admin(&self, member: &str) -> bool` · `by_github(&self, login: &str) -> Option<&MemberEntry>`

### invite.rs (exact)
- `pub const CODE_LEN: usize = 8;` (18) · `pub const INVITE_TTL_MS: u64 = 10 * 60 * 1000;` (20, **10 minutes**) · `pub const MAX_ATTEMPTS: u32 = 5;` (22)
- `pub struct Invite` (29; `is_open(&self, now_ms: u64) -> bool`) · `pub fn new_invite(now_ms: u64) -> (Invite, String)` (49) · `pub fn show_code(code: &str) -> String` (59) · `pub fn normalize_code(typed: &str) -> String` (68)
- `pub struct InviteLink` (75; `to_url(&self) -> String` (126), `parse(text: &str) -> Result<Self, String>` (146))
- `pub fn is_network_path(folder: &str) -> bool` (192) · `pub struct JoinRequest` (201) · `pub fn seal_join(link: &InviteLink, code: &str, mut req: JoinRequest) -> Result<JoinBox, String>` (223) · `pub fn open_join(kem: &KemSecret, join: &JoinBox) -> Option<JoinRequest>` (232) · `pub fn check_join(team: &str, req: &JoinRequest, code: &str) -> Result<(), &'static str>` (239)

### membership.rs (exact)
- `pub fn create_team(` (55) · `pub fn invite_link(roster: &Roster, keys: &KeyStore, relay: Option<&str>, folder: Option<&str>, invite_id: &str) -> Result<InviteLink, String>` (81) · `pub fn send_join(` (95) · `pub fn try_finish_join(link: &InviteLink, keys: &KeyStore, transport: &dyn TeamTransport) -> Result<Option<Roster>, String>` (121) · `pub fn refresh(roster: &mut Roster, keys: &KeyStore, transport: &dyn TeamTransport) -> Result<Refreshed, String>` (151) · `pub fn approve(roster: &mut Roster, request: &JoinRequest, keys: &KeyStore, transport: &dyn TeamTransport, now_ms: u64) -> Result<SignedOp, String>` (184) · `pub fn remove(roster: &mut Roster, member: &str, keys: &KeyStore, transport: &dyn TeamTransport, now_ms: u64) -> Result<u64, String>` (228) · `pub const ROTATE_EVERY_MS: u64 = 30 * 24 * 60 * 60 * 1000;` (241) · `pub fn rotate(roster: &mut Roster, keys: &KeyStore, transport: &dyn TeamTransport, now_ms: u64) -> Result<u64, String>` (255)

### transport/
- `transport/mod.rs`: `pub struct TransportError(pub String)` (28) with `new(text: impl Into<String>)`, `is_denied()`. `pub const DENIED: &str = "The relay doesn't let this computer in";` (37). `pub struct Inbound { envelope: Envelope, received_at_ms: u64, cursor: u64, reset: bool }` (52). **`pub trait TeamTransport: Send + Sync`** (62):
  - `fn kind(&self) -> &'static str;`
  - `fn send(&self, envelope: &Envelope) -> Result<(), TransportError>;`
  - `fn subscribe(&self, after: u64) -> Result<Receiver<Inbound>, TransportError>;`
  - `fn put_state(&self, blob: &SignedBlob) -> Result<(), TransportError>;`
  - `fn get_state(&self, slot: &str) -> Result<Option<SignedBlob>, TransportError>;`
  - `fn post_join(&self, join: &JoinBox) -> Result<(), TransportError>;`
  - `fn joins(&self) -> Result<Vec<JoinBox>, TransportError>;`
  - default `fn register(&self, _registration: &TeamRegistration) -> Result<(), TransportError>`, default `fn set_acl(&self, _acl: &SignedAcl) -> Result<(), TransportError>`, default `fn status(&self) -> &'static str { "live" }`, default `fn lost_access(&self) -> bool { false }`, `fn stop(&self);`
  - `pub fn now_ms() -> u64` (100)
- `transport/folder.rs`: `pub struct FolderTransport` (36), `pub fn new(folder: impl Into<PathBuf>) -> Self` (42). Implements `TeamTransport`.
- `transport/relay.rs`: `pub struct RelayTransport` (39), `pub fn normalize_relay_url(raw: &str) -> Result<String, String>` (62), `pub fn new(base: &str, team: &str, device: SigningKey) -> Result<Self, String>` (107), `pub fn poll_only(mut self) -> Self` (131).

### Other team items
- `event.rs`: `pub const SCHEMA_VERSION: u8 = 1;` · `MAX_NOTE_CHARS: usize = 280` · `MAX_FUTURE_SKEW_MS: u64 = 10 * 60 * 1000` · `DEFAULT_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000`. `pub struct TeamEvent` (23). `pub fn validate_inbound(mut event: TeamEvent, now_ms: u64) -> Result<TeamEvent, Rejected>` (371). `pub fn clean_text(text: &str, max: usize) -> String` (339). `pub fn new_event_id(now_ms: u64) -> String` (268).
- `store.rs`: `pub const MAX_SEEN: usize = 20_000;` (29). `pub fn ingest(store: &mut TeamStore, config: &TeamConfig, verified: Verified, now_ms: u64) -> Ingested` (194). `TeamStore::load(path: &Path) -> Self`, `save(&self, path: &Path) -> Result<(), String>`.
- `config.rs`: `pub const FILE_NAME: &str = "team.json";` (17). `pub fn path_in(dir: &Path) -> PathBuf` (127). `pub fn load_checked(path: &Path) -> Result<TeamConfig, String>` (132). `pub fn save(path: &Path, config: &TeamConfig) -> Result<(), String>` (143).
- `commands.rs`: `#[tauri::command]` functions, e.g. `pub async fn team_state() -> Result<Value, String>`, `pub async fn team_join(input: service::JoinInput) -> Result<Value, String>`, `pub async fn team_invite() -> Result<Value, String>`, `pub async fn team_merge_when_green(card_id: String, head_sha: String) -> Result<Value, String>` (full list in section 9).
- `service.rs`: `pub fn view() -> Result<TeamView, String>` (1341), `pub fn create_invite() -> Result<InviteView, String>` (1498), `pub fn join(input: JoinInput) -> Result<ConfigView, String>` (1537), `pub fn approve_join(join_id: &str)` (1627), `pub fn remove_member(member_id: &str)` (1680). Emit pattern: `let _ = app.emit("team://changed", ());` (line 337) and `app.emit("team://draft", draft)` (346).

---

## 4. Pairing UI (the template for an invite/join flow)

### Tauri commands: `packages/app/src-tauri/src/pairing_commands.rs`
All `#[tauri::command] pub async fn ...(app: AppHandle, ...) -> Result<Value, String>` unless noted. Each one is gated by `ready(app)` which returns `FEATURE_OFF` when the Labs flag is off.
- `pair_view(app: AppHandle) -> Result<Value, String>` (line 159)
- `pair_host_start(app: AppHandle) -> Result<Value, String>` (164) · `pair_host_cancel(app: AppHandle) -> Result<Value, String>` (169)
- `pair_join(app: AppHandle, target: String, code: String) -> Result<Value, String>` (176)
- `pair_answer(app: AppHandle, id: u64, allow: bool) -> Result<Value, String>` (184)
- `pair_nearby(app: AppHandle) -> Result<Value, String>` (191)
- `trust_set_permission(app: AppHandle, node: String, capability: String, on: bool) -> Result<Value, String>` (196)
- `trust_remove(app: AppHandle, node: String) -> Result<Value, String>` (205)
- `fn mdns_or_none() -> Arc<dyn Discovery>` (213)

### The event: `pair://changed`
- Emitted in `ready()` (pairing_commands.rs lines 85-100): `mgr.set_notifier(Arc::new(move |view: &Value| { let _ = app.emit("pair://changed", view); }));`, attached once with `static ATTACH: std::sync::Once`.
- `PairManager::changed()` calls the notifier whenever state changes.
- Frontend listens in `packages/app/src/lib/desk-pair.ts` `usePair(enabled)` line 113: `listen<PairView>('pair://changed', (e) => apply(e.payload))`. It also calls `pairView()` once on start.
- Other events in the same file: `desk://link` (line 133), `desk://layout` (136). Team uses `team://changed`.

### Frontend: `packages/app/src/lib/desk-pair.ts` (161 lines)
- Types: `export interface PairHosting` (7), `PairPending` (17; has `id`), `PairDevice` (26), `PairView` (36), `PairNearby` (50), `DESK_CAPABILITIES: { id: string; label: string }[]` (56).
- `invoke` wrappers: `pairView = () => invoke<PairView>('pair_view')` (62); `pairHostStart` (63); `pairHostCancel` (64); `pairJoin = (target, code) => invoke<PairView>('pair_join', { target, code })` (65); `pairAnswer = (id, allow) => invoke<PairView>('pair_answer', { id, allow })` (66); `pairNearby = () => invoke<PairNearby[]>('pair_nearby')` (67); `trustSetPermission(node, capability, on)` (68); `trustRemove = (node) => invoke<PairView>('trust_remove', { node })` (70).
- `export function pairErrorText(e: unknown): string` (73)
- `export interface PairApi` (82): `view`, `error`, `nearby`, `refreshNearby`, `hostStart: () => Promise<string | null>`, `hostCancel`, `join(target, code) => Promise<string | null>`, `answer(id, allow)`, `setPermission`, `remove`.
- `export function usePair(enabled: boolean): PairApi` (98).

### How the sheets are shown
- `components/desk/DeskPage.tsx` (266 lines): `const pair = usePair(snapshot !== null);` (line 33). The add sheet: `{adding && !pairView?.pending && (<AddComputerSheet ... />)}` (line 223). The inbound sheet is always mounted: `<InboundPairSheet pending={pairView?.pending ?? null} onAnswer={(id, allow) => void pair.answer(id, allow)} />` (line 253). It shows only when `pending` is set. So the sheet appears when the backend pushes a `pending` in `pair://changed`.
- `components/desk/InboundPairSheet.tsx` (42 lines): `export function InboundPairSheet({ pending, onAnswer })`, uses `<Sheet labelledBy={titleId} onClose={() => onAnswer(pending.id, false)}>`.
- `components/desk/AddComputerSheet.tsx` (253 lines): `export function AddComputerSheet({...})` (line 28), `export function timeLeft(ms: number): string` (17). Uses `<Sheet labelledBy={titleId} role="dialog" onClose={close} className="ck-desk-add">` (line 104). Imports `PairHosting, PairNearby` types.
- `components/desk/DeskPage.tsx` header comment: "My computers (Labs `desk_enabled`): the page."

### Sheet component: `packages/app/src/components/cockpit/Sheet.tsx`
- `export function Sheet({ onClose, role = 'alertdialog', labelledBy, className, children })` (line 57). `onClose` optional (omit to make it unclosable). `role?: 'alertdialog' | 'dialog'`. `labelledBy: string` required (the id of the title). `className?: string`. `children: ReactNode`.
- Also `export function useSheetKeys(onClose, paused = false)` (line 9) for Esc/keys.

---

## 5. Cargo.toml (`packages/app/src-tauri/Cargo.toml`, version 0.2.2, edition 2021)

- Networking: `iroh = { version = "=1.4.1", default-features = false, features = ["tls-ring"] }` (line 99), comment: MIT OR Apache-2.0, pinned exactly. `mdns-sd = "=0.13.11"` (line 100).
- Crypto: `ed25519-dalek = { version = "2", features = ["rand_core"] }` (line 48) · `rand = "0.8"` (49) · `chacha20poly1305 = "0.10"` (85) · `hpke = { version = "0.12", default-features = false, features = ["alloc", "x25519"] }` (88). Also `keyring = "3.6"` (66) with per-OS features (Windows `windows-native`, macOS `apple-native`, Linux `linux-native`, lines 104-118).
- `serde = { version = "1", features = ["derive"] }` (29), `serde_json = "1"` (30), `tokio = { version = "1", features = [...] }` (34) with `rt-multi-thread, sync, macros, fs, process, io-util, time`.
- Used by pairing.rs but not listed in my grep of Cargo.toml: `hmac`, `sha2`, `subtle`, `async-trait`, `bytes`, `regex` (imports seen in source). Confirm the exact versions in Cargo.toml before use.
- **SQLite: `rusqlite = { version = "0.32", features = ["bundled"] }`** (line 74). Not in a `[target]` block.
- No gossip or blobs crate (no `iroh-gossip`, `iroh-blobs`).

### SQLite example in use
- `packages/app/src-tauri/src/chat_index/store.rs`: `use rusqlite::{params, Connection, OptionalExtension};` (line 10), `pub fn open(path: &Path, cap: usize) -> rusqlite::Result<Store>` (line 120), `pub fn open_memory(cap: usize) -> rusqlite::Result<Store>` (127). Also used in `src/tui/opencode/history.rs`. This is the one to copy.

### Dev/test features in Cargo.toml
- `[features] default = ["custom-protocol"]`, `dev-fake-tui = []` (dev only).
- `[[test]]` entries: `fake_tui_e2e` (`tests/fake_tui_e2e.rs`, requires `dev-fake-tui`), `what_broke_it_e2e` (`tests/what_broke_it_e2e.rs`, requires `dev-fake-tui`).
- `[profile.dev] debug = "line-tables-only"`.

---

## 6. Labs flags

Current Labs key for My computers: **`desk_enabled`**.
- Rust definition: `packages/app/src-tauri/src/p2p/mod.rs` line 61: `pub const LABS_KEY: &str = "desk_enabled";` and line 63: `pub fn labs_on(public: &Value) -> bool { public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false) }`. Also `desk/mod.rs` line 48 has its own `pub const LABS_KEY: &str = "desk_enabled";`.
- `P2p::apply_settings(&self, public: &Value)` (mod.rs line 150) reads Labs from the settings JSON. Test: `desk_soak.rs` line 128 calls `p2p.apply_settings(&json!({ "desk_enabled": true }))`.
- **Dev-build list:** `packages/app/src-tauri/src/settings_store.rs` lines 13-21:
  ```
  pub const DEV_LABS_KEYS: &[&str] = &[
      "missions_enabled", "night_shift_enabled", "potato_bot_enabled", "potato_sandbox_enabled",
      "acp_chat_enabled", "desk_enabled", "model_bridge_enabled",
  ];
  ```
  One key per line in the file, 7 entries. Add `"spaces_enabled",` after line 19 (`"desk_enabled",`) or at the end.
- **Frontend mirror (must match):** `packages/app/src/lib/dev-labs.ts` lines 8-16 `export const DEV_LABS_KEYS = [ ... ] as const;` (same 7 strings, line 14 is `'desk_enabled',`). The file says it "Mirrors Rust `settings_store::DEV_LABS_KEYS`".
- **Labels:** `packages/app/src/lib/setup-checks/labs.ts` line 11: `desk_enabled: 'Desk',` inside `LABEL` (used for the setup check list).
- **Settings toggle:** `packages/app/src/components/settings/sections/GeneralSection.tsx` lines 528-529: `checked={env.settings.desk_enabled ?? false}` and `onChange={(value) => void env.patch({ desk_enabled: value })}`. Copy this pattern for `spaces_enabled`.
- **Frontend gate:** `packages/app/src/lib/desk.ts` line 9: `export function deskEnabled(settings) { return settings?.desk_enabled === true; }`, and a `useDeskEnabled()` hook (used in CockpitApp).
- Note: `spaces_enabled` does not exist anywhere in the repo yet (grep found none in the app).

---

## 7. The existing plan: `D:/pm-night/PLAN-R17-TEAM-SOCIAL.md`

Copied from the file (sections 5, 6, 8). Section 5 is the **D — Spaces** plan (lines 184-300); sections 6 and 8 are below. The condensed text is faithful to the file. Tables are copied as-is.

### 7.1 Section 5 summary (D — Spaces)
- UI: left bar "Spaces"; "New space" creates `#general`. Invite: an 8-word code or QR, **valid 24 h, one use**. Owner gets "Sam wants to join · Let in · No". Channels, roles (Owner · Member), online dots. Message menu: Reply · React · Delete. Owner: Remove from space. Anyone: Block. Channel settings: "Bots here: Fish (Ren's) · talks only when called · Mute". Fish chip: "Fish here only talks. No tools, no files, no memory of your private chats." Settings line: "Messages live on members' PCs. If everyone is offline, new messages wait."
- Transport: **iroh** picked (QUIC, node id = Ed25519 key, hole punching, relay forwards ciphertext only, gossip + blobs crates, self-hostable relay). libp2p and WebRTC rejected for now.
- `SpaceNet` trait (like `TeamTransport`) so iroh is one implementation; in-memory net for tests.
- Identity: reuse the team keystore; member key + device keys (Ed25519 + X25519); iroh node key is a device key certified by the member key. Pairing: invite carries space id, owner node id, one-time secret, expiry; join request sealed to owner device (reuse `team\invite.rs`); owner approves; roster entry. Member list: owner-signed hash-linked roster log (reuse `team\roster.rs`). Optional 6-word "safety words".
- Storage: signed append-only log (not CRDT). Entry: `{space, channel, author_device, seq (per author), lamport, kind, sealed_body, sig}`. Body sealed with space epoch key (XChaCha20, reuse `team\crypto.rs` + `secure.rs`); epoch key sealed to each device by HPKE. Sync: swap "latest seq per author", send what is missing; any member can serve any entry. Edits/deletes are tombstones. Store: one SQLite per space in app data; keep 90 days or 200 MB per space. Files via iroh blobs, ≤ 25 MB, on click. Optional mailbox (off by default).
- Roles v1: owner, member. Remove: roster entry + key rotation. Block: local only. Delete: tombstone with honest text. Abuse limits from cowork review M1-M6.
- Fish in channels: new `packages/potato-brain/src/rooms/transport.ts` (`RoomTransport`), inbound `room_message` kind, room id `space:ch:<space>:<channel>`. Port fishd modules into `potato-brain/src/rooms/` (same work as P-6). App side `spaces/fish_bridge.rs`.
- Social mode (decision 10-12): new tools value `social` (no tools, no helpers, no computer, no web, no MCP, no file share library). Whole turn is social in a space. Own memory at `vault/spaces/<space_id>/`. Caps: 30 answers/day per space, 5 per person per hour, 4 unprompted per 30 min. Output `render::clean`, leak check.
- Areas: `packages/app/src-tauri/src/spaces/` (`mod, ids, net/{mod,iroh,memory}, log, sync, store, invite, roster (wraps team::roster), moderation, files, fish_bridge, commands, config (Labs spaces_enabled)`). Move `team\crypto.rs` + `secure.rs` helpers to `secure_core/`. UI: `packages/app/src/components/spaces/` (SpaceList, ChannelView, MemberList, InviteSheet, JoinSheet, ModerationMenu).
- Phases table D-0..D-9 (copied in 7.2).

### 7.2 Section 5 phases table (exact)

| Id | Piece | Who |
|---|---|---|
| D-0 | Spike: two test profiles talk over iroh (direct + relay), measure hole punch. A small CLI, no UI. | Sonnet |
| D-1 | `secure_core/` move with golden tests. | Sonnet |
| D-2 | Identity, invite code/QR, owner approve, roster. | Haiku ×3, Sonnet review |
| D-3 | Log, store, sync, catch-up from any member. | Sonnet |
| D-4 | Channels + chat UI (list, view, composer, member list). | Haiku ×4 |
| D-5 | Moderation: remove + rotate, block, delete, rate limits. | Haiku ×3, Sonnet for rotate |
| D-6 | `RoomTransport` + `room_message` + rooms modules (shared with P-6). | Sonnet + Haiku ×3 |
| D-7 | Social mode (tools `social`, per-space memory, caps) + tests that prove no tool runs. | Sonnet |
| D-8 | Files via blobs; presence (T-1). | Haiku ×2 |
| D-9 | Mailbox fallback (only after decision 9). | Haiku |

Risks (section 5, copied in short): NAT (relay fallback, say so when relayed). Offline gaps (honest line, optional mailbox, T-21 home node). Lost device = lost keys (T-23, T-24). A modified client can ignore deletes (say so). Fish leaking owner info (no memory of owner, leak check, caps). Cost (heuristic gate default). Security debt: cowork M1–M6 must close before Spaces ships.

### 7.3 Section 6 table (E — team + social), exact

| Id | Name | Who | What they see | Net | Size | Depends |
|---|---|---|---|---|---|---|
| T-1 | Presence | both | Dots: online · busy · "agent working". Hover: "Codex fixing tests, 4 min". | P2P | S | D-3 |
| T-2 | Threads + reactions | both | Reply in a thread, emoji reactions, unread line. | P2P | M | D-4 |
| T-3 | Hand a task to a teammate's agent | dev | "Ask Sam's agent: fix flaky test". Sam gets a card **Accept** · **No**; it runs on Sam's PC; result comes back. | P2P | M | D-3, Heads-up |
| T-4 | Share a live agent chat into a channel | dev | "Share to #dev (read-only)". Members watch it live; redacted like Live Room. | P2P | M | D-3, P-36 |
| T-5 | Team Fish | both | A bot the space owns, in social mode, with the space's memory. "@Fish what did we decide Monday?" | P2P | M | D-7 |
| T-6 | Shared prompts + skills library | dev | `#library` items: a skill or prompt. **Install** with preview into your skills folder. | P2P | M | P-34, D-3 |
| T-7 | Shared routines | both | Share a routine template (no keys). Others **Copy** and edit. | P2P | S | Wave 4 routines 2 |
| T-8 | Team mind map | dev | The Project Map shared in a space; members see it and its age. | P2P | S | C-5 |
| T-9 | Team spending | both | Each member may share daily totals per agent. A small table; no prompts shared. | P2P | M | F-6 |
| T-10 | Voice rooms | both | "Join voice" in a channel, up to 6 people; Fish can listen only if allowed. | P2P | L | D-0 |
| T-11 | Watch a screen | both | Share an agent pane or Fish's computer view-only into a call. | P2P | L | T-10 |
| T-12 | Pins + channel notes | both | Pin messages; a short "about this channel" note. | P2P | S | D-4 |
| T-13 | Quick polls | both | "Ship Friday? Yes / No". Live count. | P2P | S | D-4 |
| T-14 | Standup by Fish | dev | Each morning Fish posts "my agents did: 2 PRs, 1 stuck" to #standup (opt-in, from turn summaries). | P2P | M | F-3, D-7 |
| T-15 | Review request | dev | "Ask Sam to review" sends a diff bundle; Sam leaves line notes; you see them in Review Desk. | P2P | M | D-3 |
| T-16 | Shared board | both | The P-22 board shared in a space; cards show owner and agent. | P2P | L | P-22, D-3 |
| T-17 | Search a space | both | Search messages of all your spaces, local index. | P2P | S | D-3 |
| T-18 | Phone push for mentions | both | "@you in #dev" pushes to your phone. | Server (push service) | S | D-4, push |
| T-19 | Team cards in a channel | dev | Heads-up cards (PR opened, CI red) also post in a chosen channel. | P2P | S | Heads-up, D-4 |
| T-20 | Public directory | both | Find public spaces and personas. | Server | M | decision 4 |
| T-21 | Home node | both | Run PM on a spare PC so the space is always reachable. Still your machine, not ours. | P2P | M | D-3 |
| T-22 | Phone as a full member | non-dev | Join spaces from the phone app itself, not only via your PC. | P2P (WebRTC/native) | L | native app |
| T-23 | One person, many devices | both | Add your laptop to your identity with a code; same spaces, same name. | P2P | M | D-2 |
| T-24 | Space backup | both | "Save an encrypted backup" file; restore on a new PC. | P2P (local file) | S | D-3 |
| T-25 | Fish meets Fish | both | Two members' bots can talk in a channel, with a hop limit and a daily cap. | P2P | M | D-7, Wave 2 hop limit |

Section 6 footer (copied): Counts: 25 items. Server needed: T-18, T-20, and the optional mailbox / hosted relay (paid SaaS idea). T-22 needs a native app first.

### 7.4 Section 8: the 20 decisions and their defaults (exact text, condensed only in layout)

1. **Service terms for Model Bridge (OpenCode Go etc.):** ship that adapter? — *Default: ship the generic bridge; add a provider's TOML only after its terms say yes.*
2. **Write into Codex's settings file:** OK on click? — *Default: yes, with a backup and Undo, never without a click.*
3. **Model Bridge faces:** serve only to apps on this PC? — *Default: yes, loopback only, forever.*
4. **Hosted catalog for personas (paid):** build it? — *Default: no in v1; files, links and Spaces only.*
5. **Share the default Fish persona:** allowed? — *Default: no until the Fish licence check passes.*
6. **Project Map spend:** which agent and cap? — *Default: cheapest installed agent, estimate first, cap $0.50 per map.*
7. **Project Map cache place:** — *Default: app data; "Save to repo" button for `docs/map/`.*
8. **Spaces relays:** public iroh relays or ours? — *Default: public relays in Labs; our relay/server can run one for self-hosting and the paid plan.*
9. **Offline mailbox** (sealed messages on a relay, 7 days): — *Default: off; members catch up from each other.*
10. **Fish social mode without the sandbox:** allow? — *Default: yes, with: no tools, no computer, no helpers, per-space memory, fenced input, daily caps.*
11. **Web search in social mode:** — *Default: no.*
12. **Owner's own lines in a space also social mode:** — *Default: yes; full Fish only in the app DM.*
13. **History kept per space:** — *Default: 90 days or 200 MB, whichever first.*
14. **Space size v1:** — *Default: up to 50 members, 20 channels.*
15. **Fish ambient talk in spaces:** — *Default: "only when called"; "when it helps" is an owner choice.*
16. **Hand a task to a teammate's agent (T-3):** — *Default: always needs the receiver's Accept; never automatic; runs in a worktree.*
17. **Team spending sharing (T-9):** — *Default: opt-in per member, daily totals only.*
18. **Voice rooms (T-10):** build them? — *Default: later (R25), P2P only, up to 6 people.*
19. **Phone push for mentions (T-18):** uses a push service. — *Default: yes, message text not in the push, only "New mention in #dev".*
20. **UI name for D:** — *Default: "Spaces".*

Count of decisions: 20. Decisions 1-3 are the Model Bridge ones. Decisions 8-13 and 14-15 are Spaces. The plan's roadmap (section 7) puts D-2..D-8 in R23 and T-items in R24/R25.

---

## 8. Cowork security review (M1-M6)

File: **`D:/pm-night/SECURITY-REVIEW-COWORK.md`** (the summary table is at line 31-36; detail sections "### M1." at line 51, "### M3." 92, "### M4." 109, "### M5." 134, "### M6." 155). Searching `docs/security` in the repo returned nothing. The review is outside the repo, in D:.
- **M1** (Medium, Confirmed): the relay's mailbox counter restarts after the mailbox empties, and clients never see new mail. `relay/server/src/lib.rs:351-361` (`SELECT COALESCE(MAX(seq), 0) + 1 FROM envelopes WHERE team = ?1`), `:202-208` (`sweep` deletes envelopes older than 7 days), client `team/transport/relay.rs:190-194, 224-226` (cursor only moves forward), `team/service.rs:170-172`.
- **M2** (Medium, Confirmed): Merge when green arms whatever repo and PR a teammate's card names. The button shows on teammate-sent cards. `team/merge_green.rs:68-77` `plan()` takes repo from `event.project.key` and number/title/state from the event; only `repos/{repo}` is fetched from GitHub. Frontend `packages/app/src/lib/team.ts:329-331, 364`.
- **M3** (Medium, Confirmed): relay memory and CPU exhaustion before auth. `seen_sigs` is filled before the device check (`relay/server/src/lib.rs:172-182`, in `authorize`). The map is never pruned well and is scanned per request under a global mutex. Fix: rate-limit auth failures per IP.
- **M4** (Medium, Confirmed): WebSocket wake amplification. Every post wakes every socket which queries SQLite. `relay/server/src/lib.rs:394-438` (especially 419-425). Authenticated devices can send many small frames, no rate limit.
- **M5** (Medium, Confirmed): relay rate limits break behind a reverse proxy (per-IP sees only the proxy's IP, lines 253 and 442 use `ConnectInfo`), per-invite counter never resets (keyed by invite id), limits are in memory only, and no storage quota (lines 344-350, per-device and per-team limits).
- **M6** (Medium, Confirmed; impact on Windows Codex is a hypothesis): the queued-merge agent prompt. Conflict file names sit outside the fence, quoting breaks, and the Codex profile auto-approves. `team/actions.rs:308-326` (`merge_task`), `team/actions.rs:301-303` (`quote_untrusted`).

Also in the review (not M-items but Spaces-relevant): I7 "merge when green confirmation is a UI sheet only" (line 321); H2 "Codex sandbox may allow network" (line 350). Fix order (line 403-406): 1 = M1, 2 = M2, 3 = M6 then verify H2, 4 = M3, M4, M5 before any hosted relay goes public.

Rate-limit grep ("rate limit") in the same file gave M3's fix line and M5's "Rate limits are in memory only" (line 146).

---

## 9. Tauri command registration and event/app data

### Registration: `packages/app/src-tauri/src/lib.rs`
- Modules: line 110 `mod desk;`, 111 `pub mod desk_soak;`, 112 `mod p2p;`, 113 `mod pairing_commands;`, 114 `mod review_desk;`. `pub mod app_paths;` line 20. the `team::commands::...` paths are used in the handler. (The `mod team;` line itself was not seen in the grep.)
- `.invoke_handler(tauri::generate_handler![` starts line 198. Commands are listed as paths, one per line. Real examples:
  - line 265-266: `team::commands::team_state,`, `team::commands::team_config_get,`
  - line 562-569: `pairing_commands::pair_view,` `pairing_commands::pair_host_start,` `pairing_commands::pair_host_cancel,` `pairing_commands::pair_join,` `pairing_commands::pair_answer,` `pairing_commands::pair_nearby,` `pairing_commands::trust_set_permission,` `pairing_commands::trust_remove,`
  - line 550-551: `desk::commands::desk_snapshot,` `desk::commands::desk_command,`
- To add Spaces: add `mod spaces;` near line 110-114, and add `spaces::commands::space_xxx,` lines in the `generate_handler!` list (e.g. after line 569).
- Line 166-167: `if app_paths::profile().is_some() { context.config_mut().identifier = app_paths::app_id(); }`.
- Line 775 comment: "Before the first window: an older version's desktop service worker would serve its UI."

### Event emit
- Pattern: `app.emit("<name>://changed", payload)` with `let _ = ...`. Examples: `pairing_commands.rs` line 95 `app.emit("pair://changed", view)`; `team/service.rs` line 337 `app.emit("team://changed", ())`, line 346 `app.emit("team://draft", draft)`; `desk://link` line 133; `desk://layout` line 136.

### App data dir: `packages/app/src-tauri/src/app_paths.rs`
- `pub fn profile_from(raw: Option<&str>) -> Option<String>` (17) · `pub fn profile() -> Option<String>` (28) · `pub fn app_id_for(profile: Option<&str>) -> String` (38) · `pub fn app_id() -> String` (45) · `pub fn app_data_dir_for(profile: Option<&str>) -> PathBuf` (68) · `pub fn app_data_dir() -> PathBuf` (104; `app_data_dir_for(profile().as_deref())`) · `pub fn ensure_app_data_dir() -> Result<PathBuf, String>` (131) · `pub fn agents_dir() -> PathBuf` (139) · `pub fn bridge_port_file() -> PathBuf` (143) · `pub fn pairing_file() -> PathBuf` (147) · `pub fn set_data_root_for_e2e(root: PathBuf)` (80)
- Usage in team: `config.rs` `path_in(dir: &Path) -> PathBuf` takes the dir (team.json). Use `crate::app_paths::app_data_dir()` (or `ensure_app_data_dir()`) and then `join("spaces")`.

---

## 10. Test commands

Run from `packages/app/src-tauri` (this is where `Cargo.toml` is; lib target at Cargo.toml line 10). Test modules: `p2p` has `#[cfg(test)] mod guard_tests; pair_tests; tests` (mod.rs 38-43); `team` has `tests`, `tests_v1`, `tests_hardening`, `tests_mcp`, `tests_mcp_service` (team/mod.rs). `desk` has its own.

- Whole p2p: `cargo test --lib p2p`
- Only the guard rules: `cargo test --lib p2p::guard_tests`
- Only pairing: `cargo test --lib p2p::pair_tests` (and `p2p::pairing`)
- Only the p2p core tests: `cargo test --lib p2p::tests`
- Team: `cargo test --lib team`
- Desk: `cargo test --lib desk`
- Feature-gated e2e (dev only): `cargo test --features dev-fake-tui --test fake_tui_e2e` (Cargo.toml comment line 1-6).

Frontend (Vitest) runs from the **repo root** (root `package.json` line 13: `"test": "vitest run"`, line 14 `"test:watch": "vitest"`). Examples: `npx vitest run packages/app/src/components/desk` or `npx vitest run packages/app/src/components/desk/DeskPage.test.tsx`. Test files sit next to the component (e.g. `DeskPage.test.tsx`, `AddComputerSheet.test.tsx`).

I did NOT run any of these commands (the task said not needed).

---

## 11. Frontend navigation and conventions

### How "My computers" (desk) is registered: `packages/app/src/CockpitApp.tsx`
- Import: line 50 `import { DeskPage } from './components/desk/DeskPage';`, line 51 `import { useDeskEnabled } from './lib/desk';`
- Gate: line 167-168 `// My computers (Labs desk_enabled): off, the row is gone and the page is unreachable.` `const deskOn = useDeskEnabled();`
- View state: line 86 `const [view, setView] = useState<MainView>(() => homeView(c.spawn !== null, c.selectedId));` (the `MainView` type is defined elsewhere; not read). Desk is the string `'desk'` (`view === 'desk'`).
- Sidebar props: line 432 `desk={{ enabled: deskOn }}`, line 433 `deskOpen={view === 'desk' && deskOn}`, line 434 `onOpenDesk={() => setView('desk')}`.
- Render: line 533-534 `) : view === 'desk' && deskOn ? (` / `<DeskPage />`.

### Sidebar: `packages/app/src/components/cockpit/Sidebar.tsx`
- Props: line 66-67 `deskOpen = false,` `onOpenDesk,`; line 110-111 types `deskOpen?: boolean;` `onOpenDesk?: () => void;`. The desk row is at line 466: `{desk?.enabled && onOpenDesk && (`. Active check line 246 includes `&& !deskOpen`.
- To add a Spaces row: same pattern (a new prop, a `view` value, a row rendered next to line 466).

### Component folder conventions
- Folders: `packages/app/src/components/desk/`, `components/cockpit/`, `components/settings/sections/`, `components/chat/`. Each feature folder has `XxxPage.tsx`, `XxxSheet.tsx`, and `Xxx.test.tsx` next to each file.
- Lib files: `packages/app/src/lib/desk.ts`, `desk-pair.ts`, `desk-link.ts`, `desk-permissions.ts`, `dev-labs.ts`, `tauri.ts` (`tauri.appLogLines(...)`). Plan says `components/spaces/` (not yet existing; `ls` on `components/` for spaces was not run).
- Stylesheets: `packages/app/src/styles/cockpit-desk.css` (imported by DeskPage line 3 `import '../../styles/cockpit-desk.css';`), `styles/potato-sandbox.css`. A Spaces page would add `styles/cockpit-spaces.css` the same way.

### CSS class prefix: `ck-`
- Seen throughout DeskPage.tsx and AddComputerSheet: `ck-desk`, `ck-desk-head`, `ck-desk-title`, `ck-desk-status`, `ck-desk-hint is-error`, `ck-desk-section`, `ck-desk-btn`, `ck-desk-switch is-on`, `ck-desk-add` (the sheet class). Modifiers use `is-` (e.g. `is-on`, `is-error`).
- Cockpit sheets also use `ck-` (e.g. `ck-desk-add`).

### One existing sheet using `Sheet`
- `components/desk/InboundPairSheet.tsx` (42 lines): `<Sheet labelledBy={titleId} onClose={() => onAnswer(pending.id, false)}>`.
- `components/desk/AddComputerSheet.tsx`: `<Sheet labelledBy={titleId} role="dialog" onClose={close} className="ck-desk-add">`. Use this as the template for an InviteSheet/JoinSheet.

---

## Could not find / did not verify

1. `docs/security/` does not exist in the repo. The cowork review is `D:/pm-night/SECURITY-REVIEW-COWORK.md`.
2. `packages/potato-brain/src/rooms/transport.ts` and the `potato-brain` tree were not opened.
3. `hmac`, `sha2`, `subtle`, `async-trait`, `bytes` are imported by p2p source but their exact versions in Cargo.toml were not read.
4. `PairError` variants, `RelayPolicy` variants other than `Never`, `TrustedDevice` fields, `session.rs` `Installed` fields, and the full `accept_loop` signature were not read.
5. `spaces_enabled` Labs key, `packages/app/src/components/spaces/`, `spaces/` Rust module, and `secure_core/` do not exist yet (checked by listing only).
6. The Spaces plan (section 5) says **invite valid 24 h, one use**. The existing team invite (`team/invite.rs`) uses `INVITE_TTL_MS = 10 * 60 * 1000` (10 minutes) and `MAX_ATTEMPTS = 5`. The Spaces invite needs its own TTL. Mismatch to decide.
7. The plan says iroh "gossip + blobs crates". Only `iroh = "=1.4.1"` (default features off) is in Cargo.toml. Adding gossip or blobs would be a new dependency.
8. The plan says `Labs spaces_enabled` and a `SpaceNet` trait. Neither exists in the code yet. `spaces` is already reserved as a capability id (`capability::SPACES`) and ALPN (`alpn::SPACES = b"pm/spaces/1"`), but not in `PERMISSIONABLE`.
9. The guard test `the_core_knows_no_agent_or_capability_behaviour` makes `endpoint.rs`, `iroh_net.rs`, `memory.rs`, `identity.rs` unable to contain the word `desk`. The `p2p_is_not_reachable...` test also blocks the words `PairManager`, `TrustList` and similar outside p2p.
10. Tests were not run; only source was read.
