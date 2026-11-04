# Relay plan open facts: answers (NF-1 to NF-39)

Read-only research. No code, plan files, or accounts were touched. Repo paths are relative to `C:\Users\Ren-pc\Desktop\work2\puppet-master`.
External sources read on 2026-11-04.

Totals: 30 answered, 9 NOT FOUND (NF-6 partly, NF-8, NF-9, NF-11, NF-12, NF-19 declaration, NF-21 partly, NF-31 partly, NF-35). Plan contradictions: 2.

---

### NF-1 flag-parsing loop in main.rs and Config fields

Answer: `relay/server/src/main.rs` reads flags with `while let Some(arg) = args.next()` and `match arg.as_str()` (main.rs:28-66). A flag's value is taken with `args.next().expect("--x needs ...")` or, for numbers, `number(&mut args, &arg)`, which exits with code 2 and prints `"<flag> needs a whole number"` (main.rs:14-20). An unknown argument prints `unknown argument ...` and exits 2 (main.rs:61-64). `--trusted-proxy` uses `Cidr::parse` and exits 2 on error (main.rs:34-43). Flags: `--listen` (default `127.0.0.1:8787`), `--db` (default `relay.sqlite`), `--memory`, `--trusted-proxy`, and eight Live Room limit flags (main.rs:31-51). There is no `--max-db-bytes` flag (see contradiction 1).
```rust
pub struct Config {
    pub db: Option<PathBuf>,
    pub limits: Limits,
    pub trusted_proxies: Vec<Cidr>,
}
```
`Limits` defaults (lib.rs:102-124) include `device_per_minute: 60`, `joins_per_invite: 5`, `unauthenticated_per_ip_per_hour: 30`, `max_windows: 500_000`, `max_seen_sigs: 2_000_000`, `team_max_envelopes: 20_000`, `team_max_bytes: 128 MiB`. The limits field is named `limits`, of type `Limits`.
Source: relay/server/src/main.rs:14-66; relay/server/src/lib.rs:59-124, 171-179.

### NF-2 signatures of router, serve, shared state, POST /v1/teams body

Answer:
- `pub fn router(relay: Arc<Relay>) -> Router` (lib.rs:924)
- `pub async fn serve(listener: tokio::net::TcpListener, relay: Arc<Relay>) -> std::io::Result<()>` (lib.rs:938)
- Shared state is `Arc<Relay>` via `.with_state(relay)` (lib.rs:933).
- `POST /v1/teams` is `post(register)` with `DefaultBodyLimit::max(8 * 1024)` (lib.rs:927). `register` takes raw `body: Bytes` and calls `serde_json::from_slice::<TeamRegistration>` itself (lib.rs:597-604). It is not a `Json<TeamRegistration>` extractor.
Source: relay/server/src/lib.rs:597-604, 924-945.

### NF-3 parameter lists of auth_header, verify_request, key_id, is_key_id

Answer (relay/protocol/src/lib.rs):
- `pub fn is_key_id(id: &str) -> bool` (line 108): length 43 and decodes to 32 bytes.
- `pub fn key_id(key: &SigningKey) -> String` (line 129): base64 of the verifying key.
- `pub fn auth_header(device: &SigningKey, method: &str, path_and_query: &str, ts_ms: u64, body: &[u8]) -> String` (line 375). Output format `v1 <key_id> <ts_ms> <sig>`.
- `pub fn verify_request(header: &str, method: &str, path_and_query: &str, body: &[u8], now_ms: u64) -> Result<AuthedRequest, &'static str>` (line 390).
- `AuthedRequest { device: String, ts_ms: u64, sig: String }` (line 382).
Source: relay/protocol/src/lib.rs:108-130, 363-410.

### NF-4 relay/server/Cargo.toml version, axum, test crates

Answer: `version = "0.1.0"` (server/Cargo.toml:3). `axum = { version = "0.8", ... features = ["http1","json","query","tokio","ws"] }` (line 21). `tokio` 1 (line 22). `rusqlite = { version = "0.32", features = ["bundled"] }` (line 24). Dev-dependencies: `ed25519-dalek = "2"`, `ureq = { version = "2", features = ["json"] }`, `tungstenite = { version = "0.24", ... "handshake" }` (lines 30-33). `tokio-util` is NOT a dependency. The tests use ureq 2 (server/tests/protocol.rs:59 onward) and tungstenite 0.24.
Source: relay/server/Cargo.toml:3, 18-33; relay/server/tests/protocol.rs:59.

### NF-5 how Relay holds its SQLite connection

Answer: `db: Mutex<Connection>` (a std/parking_lot-style `Mutex` around `rusqlite::Connection`). Used as `self.db.lock().unwrap()`.
```rust
pub struct Relay {
    db: Mutex<Connection>,
    limits: Limits,
    trusted_proxies: Vec<Cidr>,
    ...
}
```
Source: relay/server/src/lib.rs:267-268 (and lib.rs:525 for use).

### NF-6 Railway public GraphQL schema names and fields

Answer: PARTLY FOUND. The endpoint is `https://backboard.railway.com/graphql/v2`. Auth: `Authorization: Bearer` for account/workspace/OAuth tokens, and `Project-Access-Token` header for project tokens. The public API docs page names the mutation `variableCollectionUpsert` (search result on docs.railway.com/guides/external-secrets-manager, "writes many variables in one call"). The reference page does NOT list `projectCreate`, `serviceCreate`, `serviceInstanceUpdate` input fields (`source.image`, `sleepApplication`, `numReplicas`, `healthcheckPath`, `restartPolicyType`), `volumeCreate`, `serviceDomainCreate` (`targetPort`), `serviceInstanceDeployV2`, `deploymentRestart`, `deploymentRedeploy`, `projectTokenCreate`, or `projectDelete` by name. Those are on the linked pages (Manage Services, Manage Deployments, Manage Projects, Manage Volumes). I did not read those pages, and I did not run schema introspection (no token). Rate limits: 100 RPH free, 1000 Hobby, 10000 Pro; HTTP 429 with `Retry-After`. Errors are HTTP 200 with an `errors` array for authorization denials.
Source: https://docs.railway.com/integrations/api (read 2026-11-04 via fetch-docs); https://docs.railway.com/reference/public-api (read 2026-11-04 via WebFetch).

### NF-7 Railway volume writable by uid 10001 and RAILWAY_RUN_UID

Answer: PARTLY FOUND. Railway's docs say: "Docker images that run as a non-root UID by default will have permissions issues", and the workaround is "you can set `RAILWAY_RUN_UID=0` environment variable in your service." The page does not describe volume file ownership in general, so uid 10001 writability is not confirmed. Plan 30 line 294 still needs a real test deploy.
Source: https://docs.railway.com/reference/volumes (read 2026-11-04, WebFetch).

### NF-8 Railway proxy source address range and X-Forwarded-For

Answer: NOT FOUND. Railway's networking reference (docs.railway.com/reference/public-networking) has no source IP range, no X-Forwarded-For or X-Real-IP statement, no WebSocket statement, and no trusted-proxy text. It points to a Specs & Limits page, which I did not read. Needs a founder-run test deploy that logs the peer address.
Source: https://docs.railway.com/reference/public-networking (read 2026-11-04, WebFetch).

### NF-9 Railway price, trial, and project token mutation rights

Answer: PARTLY FOUND. Price and trial terms: NOT FOUND (pricing page not read). Project token rights: NOT FOUND. Railway's token page says a project token "can only be used to authenticate requests to that environment." It does not list allowed mutations (restart, redeploy, `serviceInstanceUpdate`, `projectDelete`).
Source: https://docs.railway.com/integrations/api, "Project token" section (read 2026-11-04).

### NF-10 Railway OAuth and the Tokens page labels

Answer: YES, Railway offers OAuth: "Login with Railway" (`/integrations/oauth`), which gives "an access token through the OAuth flow" for apps acting on behalf of users. The token page is `https://railway.com/account/tokens`. Its labels, per the docs, are the account "No workspace" option and "Workspace" dropdown on a "New token form". Project tokens are created from "the tokens page in your project settings." The docs call the team token a "workspace token".
Source: https://docs.railway.com/integrations/api, "Creating a token" (read 2026-11-04). Exact button labels not read beyond these terms.

### NF-11 Cloudflare Workers script upload API (DO SQLite migrations, free plan)

Answer: NOT FOUND for the upload API metadata field names (`main_module`, `bindings`, `migrations`, `old_tag`, `new_tag`, `new_sqlite_classes`), and for SQLite-backed Durable Objects on the free plan. The Versions and deployments page has no field names. The limits page does not mention SQLite storage on the Free plan.
What IS in the repo: `relay/worker/wrangler.toml` has `[[migrations]] tag = "v1" new_sqlite_classes = ["Team", "IpGate"]` (line 26-31) and `tag = "v2" new_sqlite_classes = ["Room", "RoomIndex"]` (lines 41-44 area). Plan 34 asks for the update form, which is still unknown.
Source: https://developers.cloudflare.com/workers/platform/limits/ and https://developers.cloudflare.com/workers/configuration/versions-and-deployments/ (both read 2026-11-04, WebFetch); relay/worker/wrangler.toml.

### NF-12 file names from `wrangler deploy --dry-run --outdir dist-bundle`

Answer: NOT FOUND. I did not run it. `relay/worker/node_modules` does not exist, so running it would install packages into the repo and write `dist-bundle` there. Facts from config: `main = "src/index.ts"` (relay/worker/wrangler.toml:4). Wasm is loaded from `relay/worker/src/wasm/` (pm_relay_wasm.js, pm_relay_wasm_bg.wasm, pm_relay_wasm.d.ts; wasm/BUILD.txt). Run it in a scratch copy to answer.

### NF-13 exports of pm_relay_wasm.d.ts

Answer: `relay/worker/src/wasm/pm_relay_wasm.d.ts` exports:
- `authenticate(header: string, method: string, path_and_query: string, body: Uint8Array, now_ms: number): string`. Yes, this is the request-signature check. The worker calls it at `relay/worker/src/index.ts:184` and `:284`, and `relay/worker/src/proto.ts:37`.
- `batch(items: string, next: number, head: number, reset: boolean): string`
- `check_acl(body, team, root)`, `check_blob(body, team, slot, root)`, `check_envelope(body, team, device)`, `check_join(body, team)`, `check_registration(body)`
- `constants(): string`, `is_team_id(id: string): boolean`, `resync(head: number): string`
- `initSync`, `InitOutput`
Source: relay/worker/src/wasm/pm_relay_wasm.d.ts; relay/worker/src/index.ts:184.

### NF-14 WebSocket crate in team/transport/relay.rs

Answer: `tungstenite` 0.24 (`packages/app/src-tauri/Cargo.toml:52`: `tungstenite = { version = "0.24", default-features = false, features = ["handshake", "rustls-tls-webpki-roots"] }`). Connect is `tungstenite::connect(request)` at relay.rs:312, with the request built via `tungstenite::client::IntoClientRequest` (relay.rs:295). Ping is `Message::Text("ping")` (relay.rs:326).
Source: packages/app/src-tauri/src/team/transport/relay.rs:295-312; packages/app/src-tauri/Cargo.toml:52.

### NF-15 where "relay WebSocket unavailable" is produced

Answer: `packages/app/src-tauri/src/team/transport/relay.rs:258`: `WsEnd::Failed(why) => tracing::debug!(%why, "team: relay WebSocket unavailable, polling")`. It is a debug log only. It does not reach the UI. The grep found no other occurrence in `packages/app`.
Source: packages/app/src-tauri/src/team/transport/relay.rs:258.

### NF-16 every call site of RelayTransport::new

Answer:
- `packages/app/src-tauri/src/team/service.rs:208`: `Ok(Arc::new(RelayTransport::new(&config.relay, team, keys.device_sign()?)?))` (production path)
- `packages/app/src-tauri/src/fake_tui/team.rs:194`, `:282` (fake TUI)
- Tests: `team/tests_hardening.rs:29, 134, 175`; `team/tests_v1.rs:539, 568`.
Source: grep `RelayTransport::new` under packages/app/src-tauri/src (read 2026-11-04).

### NF-17 KeyStore::name, get_or_make, and profile default

Answer (packages/app/src-tauri/src/team/keystore.rs):
```rust
fn name(&self, what: &str) -> String { format!("{}{what}", self.prefix) }          // line 66
fn get_or_make(&self, what: &str, make: impl FnOnce() -> [u8; 32]) -> Result<[u8; 32], String>   // line 78
```
`get32(&self, what: &str) -> Result<Option<[u8; 32]>, String>` (line 70) and `set32(&self, what: &str, value: &[u8; 32]) -> Result<(), String>` (line 74). The profile text when unset is `"default"`: `let profile = crate::app_paths::profile().unwrap_or_else(|| "default".into());` and `prefix: format!("team/{profile}/")` (lines 52-53). `get_or_make` reads back and refuses a key the store did not keep (line 84).
Source: packages/app/src-tauri/src/team/keystore.rs:49-84.

### NF-18 how a Tauri command reads a Labs bool

Answer: `ready(app)` in `packages/app/src-tauri/src/pairing_commands.rs:85-100` does NOT read the settings store. It checks a module-level static `LABS: AtomicBool` (declared line 58) and returns `FEATURE_OFF` when it is false. `LABS` is set by `apply_settings(public: &Value)` (pairing_commands.rs:102-104): `LABS.store(crate::p2p::labs_on(public), ...)`. `labs_on` reads the key `LABS_KEY = "desk_enabled"` as a bool with default false (`packages/app/src-tauri/src/p2p/mod.rs:61-65`).
Source: packages/app/src-tauri/src/pairing_commands.rs:58, 85-104; packages/app/src-tauri/src/p2p/mod.rs:61-65.

### NF-19 where `env.settings` declares model_bridge_enabled and desk_enabled

Answer: NOT FOUND as a type declaration. The search for `model_bridge_enabled?:` finds no match. Usages found: `packages/app/src/components/settings/model.ts:79, 97` (inline type `{ potato_bot_enabled?, potato_sandbox_enabled?, acp_chat_enabled?, model_bridge_enabled? }`), and `packages/app/src/components/settings/sections/GeneralSection.tsx:528-545` (`env.settings.desk_enabled`, `env.settings.model_bridge_enabled`). The `env` type is not declared with those keys in the code I searched.
Source: packages/app/src/components/settings/model.ts:79, 97; GeneralSection.tsx:528-545.

### NF-20 TeamEvent kinds declaration and validation

Answer: `packages/app/src-tauri/src/team/event.rs:83-109`: `#[derive(... Serialize, Deserialize)] #[serde(rename_all = "snake_case")] pub enum TeamEventKind { PrOpened, PrReady, PrUpdated, PrMerged, PrClosed, ReviewRequested, Pushed, CiFailed, CiPassed, ConflictForecast, QuestionForwarded, Answer, PleaseReview, Claim, Release, TurnFinished, Presence, Note, Ack, #[serde(other)] Unknown }`. `as_str()` maps each variant (event.rs:112). Inbound validation is `validate_inbound` (event.rs:371), which rejects `Unknown` and `ConflictForecast`. A new kind needs: the enum variant, an `as_str` arm, and any per-kind rules in `validate_inbound` and `dedupe_key` (event.rs:473-490). On the TypeScript side, `packages/app/src/lib/team.ts:10-23` declares the union, with `| string` at the end, so a new kind does not break the type.
Source: packages/app/src-tauri/src/team/event.rs:83-115, 371-490; packages/app/src/lib/team.ts:10-23.

### NF-21 iroh relay server for iroh 1.4.1

Answer: PARTLY FOUND. The iroh docs say to "Run the `iroh-relay` binary or image on a server with a public IP and DNS name", and that self-hosted relays are "configured directly with `RelayMode::Custom`". Automatic TLS via ACME is built in. NOT FOUND: a version match for 1.4.1, config keys for plain HTTP behind a TLS-ending proxy, and node-id access control.
The app uses `RelayMode::Default` or `RelayMode::Disabled` (`packages/app/src-tauri/src/p2p/iroh_net.rs:40`); `iroh = "=1.4.1"` is at `packages/app/src-tauri/Cargo.toml:99`.
Source: https://docs.iroh.computer/add-a-relay.md (read 2026-11-04, WebFetch); iroh_net.rs:16, 40.

### NF-22 CREATE TABLE statements and PRAGMA user_version

Answer (relay/server/src/lib.rs:326-345):
```sql
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS teams (team TEXT PRIMARY KEY, root TEXT NOT NULL, salt TEXT NOT NULL,
    acl_version INTEGER NOT NULL, acl TEXT NOT NULL, created_ms INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS state (team TEXT NOT NULL, slot TEXT NOT NULL, version INTEGER NOT NULL,
    blob TEXT NOT NULL, PRIMARY KEY (team, slot));
CREATE TABLE IF NOT EXISTS envelopes (team TEXT NOT NULL, seq INTEGER NOT NULL, sender TEXT NOT NULL,
    at_ms INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY (team, seq));
CREATE TABLE IF NOT EXISTS joins (id INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL,
    invite TEXT NOT NULL, at_ms INTEGER NOT NULL, body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS devices (team TEXT NOT NULL, device TEXT NOT NULL, from_seq INTEGER NOT NULL,
    PRIMARY KEY (team, device));
```
Columns `next_seq`, `stored_rows`, `stored_bytes` are added later with `ALTER TABLE teams ADD COLUMN ... DEFAULT` after a `has_column` check (lib.rs:348-350). `PRAGMA user_version` is NOT used (grep found none).
Source: relay/server/src/lib.rs:319-356.

### NF-23 InviteView fields and the TeamApi method

Answer: Rust `InviteView { link: String, code: String, expires_at_ms: u64 }` (`packages/app/src-tauri/src/team/service.rs:1490-1496`), built at service.rs:1511 as `link: link.to_url()`, `code: invite::show_code(&code)`. Yes, a separate code is shown. TypeScript `InviteView` is `{ link, code, expiresAtMs }` (`packages/app/src/lib/team.ts:207-211`). The TeamApi method is `invite: () => Promise<InviteView>` (team.ts:236), implemented as `invite: () => call('team_invite')` (team.ts:275). The Tauri command is `team_invite` → `service::create_invite` (commands.rs:75-76).
Source: service.rs:1490-1511; team.ts:207-236, 275; commands.rs:75-76.

### NF-24 team view field carrying transport error text, and view.link

Answer: `TeamConfigView.error: string | null` (`packages/app/src/lib/team.ts:174`), matching Rust `ConfigView.error: Option<String>` (`service.rs:1249`). The error is set on `inner.error` (e.g. service.rs:408). `TeamSettings.tsx` shows it at line 265: `{view?.error && <Note role="alert">{view.error}</Note>}`. The `link` field (`TeamConfigView.link: TeamLink | null`, team.ts:184) is mapped to labels at `TeamSettings.tsx:68-70` (`'live'`, `'polling'`, `'offline'`). The plan said line 74, but the first `view.link` use is at lines 68-70.
Source: packages/app/src/lib/team.ts:161-186; packages/app/src/components/settings/blocks/TeamSettings.tsx:65-70, 265; service.rs:1245-1250.

### NF-25 every logging call in relay/server/src

Answer (from grep of `tracing::|info!|warn!|error!|debug!|println!|eprintln!` in relay/server/src):
- lib.rs:289 `tracing::warn!(error = %e, "relay: internal error")`
- lib.rs:626 `tracing::info!(team = %reg.team, "relay: team registered")`
- lib.rs:946 `tracing::info!(removed = n, "relay: expired mail removed")`
- main.rs:17, 39, 53, 62 are `eprintln!`/`println!` (flag errors and help)
- main.rs:68 `tracing::info!("relay: no --trusted-proxy; ...")`
- main.rs:74 `tracing::info!(%listen, protocol = ..., "relay: listening")`
No logging in `relay/server/src/rooms.rs` or `rooms/`.
Source: as listed.

### NF-26 where the Live Room client sends POST /v1/rooms

Answer: `packages/app/src-tauri/src/live_room/client.rs:85`: `self.send("POST", &format!("{}/v1/rooms", self.prefix), &body, false)`. The room function is at client.rs:79-85 (registration signed inside the body, no `x-pm-auth`). Other room paths: client.rs:111, 272; `live_room/service_tests.rs:134`.
Source: packages/app/src-tauri/src/live_room/client.rs:79-85.

### NF-27 SeenSigs shape, rate-window helper, spawn_local return

Answer:
- `struct SeenSigs` (lib.rs:229-234): two generations (`current`, `previous`, `since_ms`). Insert method is `fn first_use(&mut self, sig: &str, now: u64, cap: usize) -> Result<(), (StatusCode, &'static str)>` (lib.rs:237). It returns 401 `"request replayed"` on a repeat and 503 `"the relay is busy, try again shortly"` at cap.
- Rate-window helper: `fn hit(&self, key: String, span_ms: u64, max: u32) -> bool` (lib.rs:397), keyed `Window { start_ms, span_ms, count }`. Used e.g. `relay.hit(format!("reg:{addr}"), HOUR_MS, ...)` (lib.rs:598).
- No per-invite counter map field exists in `Relay` (fields at lib.rs:267-285). Joins use other limits; this was not traced further.
- `pub fn spawn_local(config: Config) -> (SocketAddr, Arc<Relay>)` (lib.rs:956). It runs `serve` on a thread and returns the bound address and the relay.
- Sweeper: `serve` spawns a task with `tokio::time::interval(600 s)` calling `sweeper.sweep()` (lib.rs:938-950). `sweep()` / `sweep_at(now)` at lib.rs:518-523.
Source: relay/server/src/lib.rs:229-262, 267-285, 397-410, 518-530, 597-604, 938-963.

### NF-28 ureq major version

Answer: `ureq = { version = "2", features = ["json"] }` in `packages/app/src-tauri/Cargo.toml:54`. Also used as `ureq::AgentBuilder::new()` in `packages/app/src-tauri/src/team/transport/relay.rs:112`.
Source: packages/app/src-tauri/Cargo.toml:54; relay.rs:112.

### NF-29 create_team registration, access list, roster and keys blobs, root key

Answer: `packages/app/src-tauri/src/team/membership.rs:55-80` `create_team`:
- `let root = keys.member()?;` (the root signing key is the member key on the creator's machine), `let device = keys.device_sign()?`.
- `Roster::create(...)` makes the roster certified by the root.
- `keys.set_epoch_key(&roster.team, 1, ...)`.
- `TeamRegistration { team, root, salt, acl: SignedAcl::signed(&roster.team, roster.seq() + 1, roster.device_ids(), &root) }` then `transport.register(&registration)`.
- `publish(&roster, keys, transport.as_ref())` (membership.rs:33-45): `transport.set_acl(&acl)`, then `put_state(grant_blob(...))`, then `put_state(roster_blob(...))`. Slots are `"roster"` and `"keys"` (`relay/protocol/src/lib.rs:45`, `SLOTS`). The grant blob is `make_grant`, not `keys` (the plan's `keys` slot naming should be checked).
Source: membership.rs:33-80; relay/protocol/src/lib.rs:45.

### NF-30 CSS class names and spacing in TeamSettings.tsx

Answer: class names used: `st-team`, `st-team-row` (14 uses), `st-team-row is-top`, `st-team-section`, `st-team-h`, `st-team-head`, `st-team-line`, `st-team-member`, `st-team-people`, `st-team-who`, `st-team-you`, `st-team-role`, `st-team-slot`, `st-team-warning`, `st-team-inline`, `st-team-detail`, `st-note st-team-relay-help`, `st-link`, `st-field is-mono`, `font-mono`. Stylesheet: `packages/app/src/styles/settings.css:865-880`. Spacing is literal px, not CSS variables: `.st-team { gap: 8px }`, `.st-team-row { grid-template-columns: 110px minmax(0, 1fr); gap: 10px }`. Colour uses `var(--label-2, inherit)`.
Source: packages/app/src/components/settings/blocks/TeamSettings.tsx; packages/app/src/styles/settings.css:865-880.

### NF-31 Cloudflare free-plan daily limit response

Answer: PARTLY FOUND. The Workers limits page says: "Accounts on the Workers Free plan have a daily request limit of 100,000 requests, resetting at midnight UTC." When exceeded, "Cloudflare returns **Error 1027**." NOT FOUND: a numeric HTTP status and the body. So the plan's `429 → daily_limit` rule is NOT confirmed by this page.
Source: https://developers.cloudflare.com/workers/platform/limits/ (read 2026-11-04, WebFetch).

### NF-32 app code logging request headers of outbound HTTP calls

Answer: None found. The only `ureq::AgentBuilder` in the team transport is relay.rs:112 (timeouts only, no middleware). `grep middleware` in `packages/app/src-tauri/src` hits only unrelated modules (acp/registry.rs, channels/telegram/api.rs, live_room/client.rs, mobile_tunnel.rs, ambient/title_tests.rs). The app-wide subscriber is `packages/app/src-tauri/src/lib.rs:152-155` (`tracing_subscriber::fmt()` with `EnvFilter`), which logs events, not HTTP headers. Caveat: I did not read each of the other `AgentBuilder` sites for header logging.

### NF-33 invoke and listen import paths in desk-pair.ts

Answer: `packages/app/src/lib/desk-pair.ts:3-4`:
```ts
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
```
Source: packages/app/src/lib/desk-pair.ts:1-6.

### NF-34 test that reads PM_RELAY_TEST_URL in team/tests_v1.rs near line 497

Answer: The reader is `fn start() -> Self` on the test helper `TestRelay` (`packages/app/src-tauri/src/team/tests_v1.rs:507-516`): `match std::env::var("PM_RELAY_TEST_URL")`. The doc comment mentioning it is at lines 496-499. It is a helper, not a `#[test]`. The tests that use it are in the same file, e.g. `relay_end_to_end_create_invite_join_push_and_card_with_metadata_only_on_the_relay` (tests_v1.rs:543) and the transport helper `relay_transport` (tests_v1.rs:538).
Source: packages/app/src-tauri/src/team/tests_v1.rs:496-545.

### NF-35 Cloudflare dashboard labels for the "Edit Cloudflare Workers" token template

Answer: NOT FOUND. Not read. Cloudflare's API token docs were not fetched in this pass.

### NF-36 GitHub container package visibility path

Answer: Organization main page → **Packages** tab → click the package → **Package settings** (gear icon, right side) → **Danger Zone** → **Change visibility** → select **Public**. Only an admin of the package can do it. Warning on the page: once public, "you cannot make it private again."
Source: https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility (read 2026-11-04, WebFetch).

### NF-37 how an existing test renders GeneralSection

Answer: `packages/app/src/components/settings/sections/GeneralSection.test.tsx` imports `generalRows` from `./GeneralSection` (line 7) and the helpers `mountRows`, `buttonByText`, `byLabel`, `click`, `pickOption`, `settle`, etc. from `../test-utils` (line 6). `mountRows` is exported from `packages/app/src/components/settings/test-utils.tsx`. The test is a rows-based mount (`generalRows` produces rows), not a full provider tree. Other GeneralSection test files: `blocks/DiagnosticFile.test.tsx`, `sections/PackageCheckRow.test.tsx`, `lib/simple-mode.test.tsx`.
Source: GeneralSection.test.tsx:1-30; components/settings/test-utils.tsx.

### NF-38 function team_config_set uses to save TeamConfig and restart

Answer: `packages/app/src-tauri/src/team/commands.rs:25-27`: `team_config_set(config)` → `blocking(move || service::config_set(config))`. `service::config_set` (service.rs:1423-1433) normalises the config, calls `save_config(svc, next)?`, then `svc.restart()`, and returns `config_get()`.
Source: commands.rs:25-27; service.rs:1423-1433.

### NF-39 handling of reset: true from a batch or Resync frame

Answer: In `packages/app/src-tauri/src/team/transport/relay.rs`:
- Batch: `if batch.reset { self.resync(batch.head.unwrap_or(0)); }` (relay.rs:271-273). Then items are delivered, and the cursor is set to `batch.next` when `batch.reset || batch.next > cursor` (relay.rs:279-281).
- WebSocket Resync frame: `else if let Ok(Resync { reset: true, head }) = serde_json::from_str::<Resync>(&text) { self.resync(head); }` (relay.rs:336-337).
- `deliver` marks `reset` on the first envelope after `pending_reset` (relay.rs:234-238).
- In `team/service.rs:251`, `if inbound.reset || inbound.cursor > store.cursor { store.cursor = inbound.cursor; }`. The service takes the lower position as the saved cursor.
Source: relay.rs:228-240, 265-282, 332-340; service.rs:248-256.

---

## Plan lines that contradict the code

1. `docs/plans/social/31-relay-server-hardening-and-config.md:1504` (Security table, "Fill the disk") says the defence is `--max-db-bytes`, 507. The code has no `--max-db-bytes` flag. `relay/server/src/main.rs:31-51` lists `--listen`, `--db`, `--memory`, `--trusted-proxy`, and eight Live Room limit flags only. The 507 response exists (lib.rs:733, `INSUFFICIENT_STORAGE`) but for team mailbox size, not a database byte cap.
2. `docs/plans/social/32-relay-deploy-flows.md:1694` (Security table, "Token in a file") says the defence is `KeyStore::set_relay_token`. No such function exists: `grep set_relay_token` over `packages/app/src-tauri/src` returns nothing. `keystore.rs` has `member`, `device_sign`, `p2p_node`, `device_kem`, `epoch_key`, `set_epoch_key`, `invite_code`, `set_invite_code`, `forget_invite`, `forget_team` (keystore.rs:89-129).

Not checked: a full sweep of every existing-type or file reference in plans 30 to 35. Only the two items above were verified against the code.

---

Summary: 30 answered, 9 NOT FOUND or partly NOT FOUND (NF-6, NF-8, NF-9, NF-11, NF-12, NF-19 declaration, NF-21, NF-31, NF-35). Contradictions: 2.
