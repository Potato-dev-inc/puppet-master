# 33 — Your relay: app UI, settings and Tauri commands

Status: plan only. Part of the one-click relay family (overview: `30-one-click-relay-overview.md`).

Sources the writer used: `00-FORMAT.md`, `_facts/relay-facts.md` ("relay facts §N"), `_facts/spaces-facts.md`
("spaces facts §N"). No source code was opened by the writer. NEEDS-FACT lines use the shared `NF-n` numbering and are
listed in `Open facts` near the end.

UI rules for every slice here: monochrome (no coloured glass, `docs/design/`), simple English, no jargon in UI text, no
window on the primary screen while testing (use Vitest and the preview tools). Strings below are final; components use
them exactly.

---

## 1. States

| Phase (`RelayPhase`) | When | What the row shows |
|---|---|---|
| `idle` | Nothing made, or a make was never started | Explanation, provider buttons, connected line when a token is stored |
| `deploying` | The runner (file 32) is working | The step list, Stop |
| `ready` | Made and last check passed | Address, status, version, team hand-off, More actions |
| `unhealthy` | Made, last check found a problem | Problem text, Check again, Open Railway (file 34 adds Fix it) |
| `fixing` | File 34 Fix, Restart or Update is running | A short step list (file 34) |
| `removing` | File 34 Remove is running | "Removing your relay" |
| `removed` | Removed by the person | Removed text, Make a new relay |
| `failed` | The runner stopped with an error, or the app restarted mid-make | Error text, Try again, Open Railway |

## 2. Every string

### 2a. Settings list entry and Labs

| Key | Text |
|---|---|
| Row label | `Your relay` |
| Row hint | `Make a relay for your team on your own Railway or Cloudflare account.` |
| Row keywords (search) | `relay railway cloudflare server host team heads-up spaces` |
| Labs name | `Your relay` |
| Labs about | `Make your own relay for your team in a few clicks.` |
| Labs label in setup checks | `Your relay` |
| Command refused while Labs is off | `Turn on Your relay in Settings › Labs first.` |
| Command refused while a job runs | `Wait for the current step to finish.` |

### 2b. Idle

| Key | Text |
|---|---|
| Body | `Your team needs a relay so your computers can reach each other. Make your own in about 3 minutes. It runs on your own Railway account, and Railway charges you about 5 US dollars a month. Your messages are sealed: the relay can't read them.` |
| Primary button | `Make my relay with Railway` |
| Link | `Use Cloudflare instead (free, with daily limits)` |
| Link | `I already have a relay` |
| Text after that link | `Type its address in Team above.` |
| Cloudflare note (shown when Cloudflare is connected or chosen) | `Cloudflare is free. If your team sends a lot in one day, the relay stops until 00:00 UTC. Backups aren't available on Cloudflare.` |
| Connected, Railway, with email | `Railway connected as {email}.` |
| Connected, Railway, project token only | `Railway connected.` |
| Connected, Cloudflare | `Cloudflare connected: {account}.` |
| Button when connected | `Make my relay` |
| Disconnect link | `Disconnect Railway` / `Disconnect Cloudflare` |

### 2c. Connect sheets

Railway:

| Key | Text |
|---|---|
| Title | `Connect Railway` |
| Line 1 | `Open Railway and sign in, or make a free account.` |
| Button | `Open Railway` |
| Line 2 | `Make a new token. Name it Puppet Master. Copy it.` |
| Line 3 | `Paste the token here.` |
| Field label | `Railway token` |
| Placeholder | `Paste token` |
| Footer | `The token is kept in this computer's password store, never in a file. Puppet Master uses it only to set up and look after your relay.` |
| Buttons | `Connect` (busy: `Checking`), `Cancel` |
| Error, empty | `Paste the token first.` |
| Error, has spaces | `Paste only the token, with no spaces.` |
| Error, bad token | `That token didn't work. Make a new one in Railway and paste it again.` |
| Error, project token pasted | `This token can't do that. Make an account token in Railway, not a project token.` |
| Error, network | `Railway didn't answer. Check your internet and try again.` |

Cloudflare:

| Key | Text |
|---|---|
| Title | `Connect Cloudflare` |
| Line 1 | `Open Cloudflare and sign in, or make a free account.` |
| Button | `Open Cloudflare` |
| Line 2 | `Make a token with the template Edit Cloudflare Workers. Copy it.` |
| Line 3 | `Paste the token here.` |
| Field label | `Cloudflare token` |
| Placeholder, footer, buttons, empty/space errors | same as Railway |
| Error, bad token | `That token didn't work. Make a new one in Cloudflare and paste it again.` |
| Error, wrong template | `This token can't change Workers. Make it with the Edit Cloudflare Workers template.` |
| Error, network | `Cloudflare didn't answer. Check your internet and try again.` |

### 2d. Deploying

| Key | Text |
|---|---|
| Title | `Making your relay` |
| Railway steps | `Checking your Railway account` · `Making a project called Puppet Master relay` · `Adding the relay` · `Adding storage for messages` · `Getting a web address` · `Starting the relay (this can take 2 minutes)` · `Checking that it works` |
| Cloudflare steps | `Checking your Cloudflare account` · `Getting a web address` · `Adding the relay` · `Checking that it works` |
| Note | `You can close Settings. We'll keep going.` |
| Button | `Stop` (after press: `Stopping after this step`) |

### 2e. Ready

| Key | Text |
|---|---|
| Title | `Your relay is ready` |
| Address line | `Address: {url}` with button `Copy` (after press: `Copied`) |
| Status line | `Working · checked {ago}` where `{ago}` is `just now` (under 60 s), `1 minute ago`, `{n} minutes ago` (under 60), `1 hour ago`, `{n} hours ago` |
| Version line | `Version {version}` |
| Provider line | `On Railway` / `On Cloudflare` |
| Button, team not using it | `Use it for my team` |
| After it, no team | `Go to Team above, type your team's name and press Create team. Your relay is already filled in.` |
| After it, team uses it | `Your team uses this relay.` |
| After it, team uses another | `Your team uses another relay.` (file 34 adds `Move my team here`) |
| Button, team uses it | `Invite people` |
| Invite text | `Send this link to the people you want on your team. It stops working after 10 minutes. When they open it, you'll see a request here to let them in.` |
| Invite buttons | `Copy link` (after: `Copied`) |
| More actions (this file) | `Check now`, `Open Railway` / `Open Cloudflare` |

### 2f. Unhealthy

| Key | Text |
|---|---|
| Title | `Your relay isn't answering` |
| `down` | `The relay stopped. Your team isn't getting updates.` |
| `unreachable` | `We can't reach the relay's address. If your internet works, the relay may have been removed on Railway.` |
| `websocket` | `The relay answers, but live updates are blocked. Your team still gets updates every 20 seconds.` |
| `wrong_relay` | `Something else answers at your relay's address. It isn't your relay.` |
| `old_version` | `Your relay is out of date. Update it to keep it working with this app.` |
| `full` | `Your relay's storage is full. It frees space as messages older than 7 days are removed.` |
| `account` | `Puppet Master can't reach your Railway account any more. Connect Railway again.` |
| `daily_limit` | `Cloudflare's free daily limit is used up. Your relay starts again at {time}.` where `{time}` is the next 00:00 UTC shown in local time, for example `1:00 AM` |
| Buttons (this file) | `Check again`, `Open Railway` / `Open Cloudflare`; for `account`: `Connect Railway` |

(The 20 seconds in `websocket` is `POLL_WAIT_S = 20`, relay facts §6.)

### 2g. Failed and removed

| Key | Text |
|---|---|
| Failed title | `We couldn't finish making your relay` |
| Failed line | the error's plain text (file 32 `DeployError::plain`) |
| Failed after app restart | `We stopped while making your relay. Press Try again to finish.` |
| Failed buttons | `Try again`, `Open Railway` (only when a Railway project id is saved) |
| Removed title | `Your relay was removed.` |
| Removed, team used it | `Your team won't get updates until it uses another relay.` |
| Removed button | `Make a new relay` |

## 3. Data shapes

Rust (`relay_host/service.rs`, R33-2):
```rust
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase { Idle, Deploying, Ready, Unhealthy, Fixing, Removing, Removed, Failed }

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayHostView {
    pub phase: Phase,
    pub provider: Option<Provider>,
    pub railway_connected: bool,
    pub cloudflare_connected: bool,
    /// Railway email or Cloudflare account name, from this session only (not saved).
    pub account: Option<String>,
    pub step: Option<DeployStep>,
    pub stopping: bool,
    pub url: Option<String>,
    pub version: Option<String>,
    pub update_available: bool,
    pub problem: Option<Problem>,
    pub last_ok_ms: Option<u64>,
    pub last_backup_ms: Option<u64>,
    pub used_by_team: bool,
    pub has_project: bool,
    pub can_backup: bool,
    pub error: Option<String>,
}
```
TypeScript (`packages/app/src/lib/relay-host.ts`, R33-3):
```ts
export type RelayProvider = 'railway' | 'cloudflare';
export type RelayPhase = 'idle' | 'deploying' | 'ready' | 'unhealthy' | 'fixing' | 'removing' | 'removed' | 'failed';
export type RelayStep =
  | 'check_account' | 'create_project' | 'add_relay' | 'add_storage' | 'get_address' | 'start' | 'check' | 'done';
export type RelayProblem =
  | 'down' | 'unreachable' | 'websocket' | 'wrong_relay' | 'old_version' | 'full' | 'account' | 'daily_limit';

export interface RelayHostView {
  phase: RelayPhase;
  provider: RelayProvider | null;
  railwayConnected: boolean;
  cloudflareConnected: boolean;
  account: string | null;
  step: RelayStep | null;
  stopping: boolean;
  url: string | null;
  version: string | null;
  updateAvailable: boolean;
  problem: RelayProblem | null;
  lastOkMs: number | null;
  lastBackupMs: number | null;
  usedByTeam: boolean;
  hasProject: boolean;
  canBackup: boolean;
  error: string | null;
}

export type TeamFit = 'create' | 'uses' | 'move';
```
Event: `relay://changed`, payload `RelayHostView` (same pattern as `pair://changed`, spaces facts §4).

## 4. Tauri commands (this file; file 34 adds fix, restart, backup, update, remove, move)

All in `packages/app/src-tauri/src/relay_host/commands.rs`, registered in `packages/app/src-tauri/src/lib.rs` inside
`tauri::generate_handler![…]` (relay facts §11; spaces facts §9 shows the one-path-per-line style).

```rust
#[tauri::command] pub async fn relay_host_view(app: AppHandle) -> Result<Value, String>
#[tauri::command] pub async fn relay_host_connect(app: AppHandle, provider: String, token: String) -> Result<Value, String>
#[tauri::command] pub async fn relay_host_disconnect(app: AppHandle, provider: String) -> Result<Value, String>
#[tauri::command] pub async fn relay_host_deploy(app: AppHandle, provider: String) -> Result<Value, String>
#[tauri::command] pub async fn relay_host_cancel(app: AppHandle) -> Result<Value, String>
#[tauri::command] pub async fn relay_host_check(app: AppHandle) -> Result<Value, String>
#[tauri::command] pub async fn relay_host_team_fit(app: AppHandle) -> Result<Value, String>   // {"next":"create"|"uses"|"move"}
#[tauri::command] pub async fn relay_host_open_dashboard(app: AppHandle) -> Result<(), String>
#[tauri::command] pub async fn relay_host_open_token_page(app: AppHandle, provider: String) -> Result<(), String>
```
Every one returns `FEATURE_OFF` = `Turn on Your relay in Settings › Labs first.` when the Labs key is off. None returns
a token. `relay_host_connect` returns the view, never the token or a masked form of it.

## 5. Components

Folder `packages/app/src/components/settings/relay/` (same idea as `components/settings/sandbox/`, which exists in the
repo per the git status of this session).

| Component | Props | Slice |
|---|---|---|
| `RelayHostSettings` | `{ api?: RelayHostApi; teamApi?: TeamApi; onSaved?: () => void }` | R33-4, R33-5 |
| `RelayConnectSheet` | `{ provider: RelayProvider; busy: boolean; error: string \| null; onConnect: (token: string) => void; onOpenTokenPage: () => void; onClose: () => void }` | R33-4 |
| `RelayDeployProgress` | `{ provider: RelayProvider; step: RelayStep \| null; stopping: boolean; onStop: () => void }` | R33-5 |
| `RelayReadyCard` | `{ view: RelayHostView; now: number; teamFit: TeamFit \| null; invite: string \| null; onCheck: () => void; onOpenDashboard: () => void; onUseForTeam: () => void; onInvite: () => void }` | R33-5 |
| `RelayProblemCard` | `{ view: RelayHostView; busy: boolean; onCheck: () => void; onOpenDashboard: () => void; onReconnect: () => void }` | R33-5 |
| `RelayHostRow` | `{ enabled: boolean; onSaved?: () => void }` | R33-6 |
| `TeamWithRelay` | `{ relayOn: boolean; onSaved?: () => void }` | R33-6 |

---

## Slices

### R33-1 — Labs flag `relay_host_enabled`

Size: Haiku. Deps: none.

**Goal**
A Labs key `relay_host_enabled` exists in Rust and TypeScript, is on in dev builds, and has a label.

**Read first**
- `packages/app/src-tauri/src/settings_store.rs` lines 13-21 — `DEV_LABS_KEYS` (relay facts §11).
- `packages/app/src/lib/dev-labs.ts` lines 8-16 — the TS mirror.
- `packages/app/src/lib/setup-checks/labs.ts` line 11 — the `LABEL` map (spaces facts §6).
- `packages/app/src-tauri/src/relay_host/mod.rs` (R32-2).

**Files**
- edit `packages/app/src-tauri/src/settings_store.rs`
- edit `packages/app/src/lib/dev-labs.ts`
- edit `packages/app/src/lib/setup-checks/labs.ts`
- edit `packages/app/src-tauri/src/relay_host/mod.rs`
- edit `packages/app/src-tauri/src/relay_host/tests.rs`

**Steps**
1. In `DEV_LABS_KEYS` (Rust), add the line `"relay_host_enabled",` after `"model_bridge_enabled",`.
2. In `dev-labs.ts`, add `'relay_host_enabled',` at the same position.
3. In `setup-checks/labs.ts` `LABEL`, add `relay_host_enabled: 'Your relay',`.
4. In `relay_host/mod.rs`, add `pub const LABS_KEY: &str = "relay_host_enabled";` and
   `pub const FEATURE_OFF: &str = "Turn on Your relay in Settings › Labs first.";`.
5. Add the Rust test. Run, commit `labs: Your relay flag`.

**Code**
Rust, after the change:
```rust
pub const DEV_LABS_KEYS: &[&str] = &[
    "missions_enabled",
    "night_shift_enabled",
    "potato_bot_enabled",
    "potato_sandbox_enabled",
    "acp_chat_enabled",
    "desk_enabled",
    "model_bridge_enabled",
    "relay_host_enabled",
];
```
TypeScript, after the change (same order):
```ts
export const DEV_LABS_KEYS = [
  'missions_enabled',
  'night_shift_enabled',
  'potato_bot_enabled',
  'potato_sandbox_enabled',
  'acp_chat_enabled',
  'desk_enabled',
  'model_bridge_enabled',
  'relay_host_enabled',
] as const;
```
If either list has gained other keys since the facts were taken, keep them and add `relay_host_enabled` at the end.

**Tests to write**
- Rust, in `relay_host/tests.rs`: `labs_key_is_a_dev_lab` → `crate::settings_store::DEV_LABS_KEYS.contains(&super::LABS_KEY)`.
- TypeScript: if a test already compares the two lists (search `DEV_LABS_KEYS` in `*.test.ts`), run it; otherwise add
  `packages/app/src/lib/dev-labs.relay.test.ts`: reads `packages/app/src-tauri/src/settings_store.rs` as text with
  `fs.readFileSync` and asserts every quoted key in `DEV_LABS_KEYS` there is in the TS list, and the TS list includes
  `'relay_host_enabled'`.

**Run**
- `$env:CARGO_TARGET_DIR='D:/pm-night/target-r33-1'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::tests::labs_key_is_a_dev_lab` → `test result: ok. 1 passed`.
- `npx vitest run packages/app/src/lib/dev-labs.relay.test.ts` (from the repo root) → `1 passed`.

**Done when**
- [ ] Both lists contain the key; the label exists; tests pass.

**Do NOT**
- Do not reorder existing keys.
- Do not add the key to any non-dev default.

**If it fails**
1. A settings type check fails because `relay_host_enabled` is unknown → add it to the settings type where
   `model_bridge_enabled` is typed (NF-19).
2. The TS test cannot find the Rust file → build the path from `process.cwd()` (Vitest runs from the repo root).
3. Setup-check tests snapshot the label list → update that snapshot to include `Your relay`.

---

### R33-2 — `RelayHost` service, Tauri commands, `relay://changed`

Size: Sonnet (runtime wiring, background thread, events). Deps: R32-5, R33-1.

**Goal**
The app has one `RelayHost` service with the commands in §4, emitting `relay://changed` on every change.

**Read first**
- `packages/app/src-tauri/src/pairing_commands.rs` lines 85-100 and 159-213 — the `ready(app)` Labs gate, the
  once-only notifier `ATTACH`, and `app.emit("pair://changed", view)` (spaces facts §4). Copy that shape.
- How a command reads a Labs bool from settings (inside `ready`) (NF-18).
- `packages/app/src-tauri/src/app_lifecycle.rs` lines 36-42 — `open_external_url` (relay facts §9).
- `packages/app/src-tauri/src/team/config.rs` — `load_checked`, `path_in`, `has_team` (relay facts §6).
- `relay_host/deploy.rs`, `verify.rs`, `config.rs`.

**Files**
- create `packages/app/src-tauri/src/relay_host/service.rs`
- create `packages/app/src-tauri/src/relay_host/commands.rs`
- create `packages/app/src-tauri/src/relay_host/service_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs`
- edit `packages/app/src-tauri/src/lib.rs`

**Steps**
1. Write `service.rs` (Code).
2. Write `commands.rs` (Code).
3. In `mod.rs` add `pub mod service; pub mod commands;` and `#[cfg(test)] mod service_tests;`.
4. In `lib.rs` `generate_handler!`, after `pairing_commands::trust_remove,` add the 9 lines
   `relay_host::commands::relay_host_view,` … `relay_host::commands::relay_host_open_token_page,`.
5. Tests, run, commit `relay_host: service and commands`.

**Code**
`relay_host/service.rs`:
```rust
//! The one RelayHost per app: state, background jobs, view, notifier.

use super::config::{self, DeployStep, HostConfig, Provider};
use super::deploy::{self, DeployDeps, DeployError};
use super::railway::{RailwayApi, RailwayAuth};
use super::verify::{self, Problem, RelayProbe};
use crate::team::keystore::{KeyStore, RELAY_CLOUDFLARE_TOKEN, RELAY_RAILWAY_PROJECT_TOKEN, RELAY_RAILWAY_TOKEN};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const BUSY: &str = "Wait for the current step to finish.";
pub const RESUME: &str = "We stopped while making your relay. Press Try again to finish.";

pub type Notifier = Arc<dyn Fn(&RelayHostView) + Send + Sync>;

pub struct HostDeps {
    pub dir: PathBuf,
    pub keys: KeyStore,
    pub railway: Arc<dyn RailwayApi>,
    pub cloudflare: Arc<dyn super::cloudflare::CloudflareApi>, // until R32-9 merges: a stub that returns CfError::Api("Cloudflare isn't ready yet.")
    pub probe: Arc<dyn RelayProbe>,
    pub now: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub sleep: Arc<dyn Fn(u64) + Send + Sync>,
    /// The team's relay address, or None when there is no team.
    pub team_relay: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    /// true runs jobs on the calling thread (tests); false spawns a thread.
    pub inline_jobs: bool,
}

#[derive(Default)]
struct Live {
    phase_override: Option<Phase>, // Deploying, Fixing, Removing while a job runs
    step: Option<DeployStep>,
    problem: Option<Problem>,
    error: Option<String>,
    account: Option<String>,
    version: Option<String>,
}

pub struct RelayHost {
    deps: HostDeps,
    live: Mutex<Live>,
    busy: AtomicBool,
    cancel: AtomicBool,
    notifier: Mutex<Option<Notifier>>,
}

impl RelayHost {
    pub fn new(deps: HostDeps) -> Arc<Self>;
    pub fn set_notifier(&self, n: Notifier);
    pub fn config(&self) -> HostConfig;                       // config::load(config::path_in(&deps.dir))
    pub fn save(&self, c: &HostConfig) -> Result<(), String>; // config::save
    pub fn view(&self) -> RelayHostView;                      // rules below
    pub fn connect(&self, provider: Provider, token: &str) -> Result<RelayHostView, String>;
    pub fn disconnect(&self, provider: Provider) -> Result<RelayHostView, String>;
    pub fn start_deploy(self: &Arc<Self>, provider: Provider) -> Result<RelayHostView, String>;
    pub fn cancel(&self) -> RelayHostView;
    pub fn check_now(&self, deep: bool) -> RelayHostView;
    pub fn team_fit(&self) -> Option<&'static str>;           // Some("create" | "uses" | "move"); None when no relay made
    pub fn dashboard_url(&self) -> Option<String>;
    fn changed(&self);                                         // calls the notifier with self.view()
    fn run_job(self: &Arc<Self>, phase: Phase, job: impl FnOnce(&Arc<Self>) + Send + 'static) -> Result<(), String>;
}
```
`view()` rules, in order:
1. `phase_override` set → that phase.
2. `config.url` non-empty and `config.step == Some(Done)` → `Unhealthy` if `live.problem` is `Some`, else `Ready`.
3. `live.error` set, or `config.step` is `Some` and not `Done` → `Failed` (error = `live.error` or `RESUME`).
4. `config.removed_url` non-empty → `Removed`.
5. Otherwise `Idle`.
Other fields: `railway_connected` = either Railway token present; `cloudflare_connected` = Cloudflare token present;
`used_by_team` = `team_relay()` normalized == `config.url`; `has_project` = `config.railway` has a non-empty
`project_id`; `can_backup` = provider is Railway; `update_available` = `version` older than `image::RELAY_VERSION`;
`last_ok_ms` and `last_backup_ms` = `None` when 0.

`connect()`:
```rust
pub fn connect(&self, provider: Provider, token: &str) -> Result<RelayHostView, String> {
    let token = token.trim();
    if token.is_empty() { return Err("Paste the token first.".into()); }
    if token.chars().any(char::is_whitespace) { return Err("Paste only the token, with no spaces.".into()); }
    match provider {
        Provider::Railway => {
            let me = self.deps.railway.me(&RailwayAuth::Account(token.to_owned())).map_err(|e| e.plain())?;
            self.deps.keys.set_relay_token(RELAY_RAILWAY_TOKEN, token)?;
            self.live.lock().unwrap().account = Some(me.email);
        }
        Provider::Cloudflare => {
            let auth = super::cloudflare::CfAuth(token.to_owned());
            self.deps.cloudflare.verify_token(&auth).map_err(|e| e.plain())?;
            let acct = self.deps.cloudflare.first_account(&auth).map_err(|e| e.plain())?;
            self.deps.keys.set_relay_token(RELAY_CLOUDFLARE_TOKEN, token)?;
            self.live.lock().unwrap().account = Some(acct.name);
        }
    }
    self.changed();
    Ok(self.view())
}
```
`start_deploy()`: refuse with `BUSY` when `busy` is set; clear `live.error` and `live.problem`; set `phase_override =
Deploying`; run `deploy::deploy_railway` (or `deploy_cloudflare` after R32-9) with `progress` writing `live.step` and
calling `changed()`, `cancelled` reading `cancel`, `save` calling `self.save`; on `Err(e)` set `live.error =
Some(e.plain())` (for `Cancelled`: `"Stopped."`); always clear `phase_override`, `busy`, `cancel` and call `changed()`.

`check_now(deep)`: when a relay is made, run `verify::check(probe, url, owner, owner_id, deep, with_owner = provider ==
Railway, RELAY_MIN_VERSION, now)`; store `problem` and `version`; when `problem` is `None`, set `config.last_ok_ms` and
save at most once per hour (compare with the saved value).

`relay_host/commands.rs`:
```rust
//! Tauri commands for Your relay. Gated by the Labs key; never return a token.

use super::service::{HostDeps, RelayHost};
use super::{config::Provider, FEATURE_OFF, LABS_KEY};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter};

static HOST: OnceLock<Arc<RelayHost>> = OnceLock::new();

fn labs_on(app: &AppHandle) -> bool {
    // Read the public settings exactly as pairing_commands::ready does, with LABS_KEY (NF-18).
    unimplemented_by_plan_see_nf_18(app, LABS_KEY)
}

fn host(app: &AppHandle) -> Result<Arc<RelayHost>, String> {
    if !labs_on(app) { return Err(FEATURE_OFF.into()); }
    let h = HOST.get_or_init(|| {
        let h = RelayHost::new(HostDeps::real());
        let app2 = app.clone();
        h.set_notifier(Arc::new(move |view| { let _ = app2.emit("relay://changed", view); }));
        h
    });
    Ok(h.clone())
}

fn provider(raw: &str) -> Result<Provider, String> {
    match raw { "railway" => Ok(Provider::Railway), "cloudflare" => Ok(Provider::Cloudflare), _ => Err("Unknown provider.".into()) }
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value, String> { serde_json::to_value(v).map_err(|e| e.to_string()) }

#[tauri::command]
pub async fn relay_host_view(app: AppHandle) -> Result<Value, String> { to_value(&host(&app)?.view()) }

#[tauri::command]
pub async fn relay_host_connect(app: AppHandle, provider: String, token: String) -> Result<Value, String> {
    let h = host(&app)?;
    let p = self::provider(&provider)?;
    let view = tauri::async_runtime::spawn_blocking(move || h.connect(p, &token)).await.map_err(|e| e.to_string())??;
    to_value(&view)
}

#[tauri::command]
pub async fn relay_host_disconnect(app: AppHandle, provider: String) -> Result<Value, String> {
    to_value(&host(&app)?.disconnect(self::provider(&provider)?)?)
}

#[tauri::command]
pub async fn relay_host_deploy(app: AppHandle, provider: String) -> Result<Value, String> {
    to_value(&host(&app)?.start_deploy(self::provider(&provider)?)?)
}

#[tauri::command]
pub async fn relay_host_cancel(app: AppHandle) -> Result<Value, String> { to_value(&host(&app)?.cancel()) }

#[tauri::command]
pub async fn relay_host_check(app: AppHandle) -> Result<Value, String> {
    let h = host(&app)?;
    let view = tauri::async_runtime::spawn_blocking(move || h.check_now(true)).await.map_err(|e| e.to_string())?;
    to_value(&view)
}

#[tauri::command]
pub async fn relay_host_team_fit(app: AppHandle) -> Result<Value, String> {
    Ok(json!({ "next": host(&app)?.team_fit() }))
}

#[tauri::command]
pub async fn relay_host_open_dashboard(app: AppHandle) -> Result<(), String> {
    let url = host(&app)?.dashboard_url().ok_or("There is no relay to open.")?;
    crate::app_lifecycle::open_external_url(url)
}

#[tauri::command]
pub async fn relay_host_open_token_page(app: AppHandle, provider: String) -> Result<(), String> {
    host(&app)?;
    let url = match self::provider(&provider)? {
        Provider::Railway => super::railway::RAILWAY_TOKENS_PAGE,
        Provider::Cloudflare => super::cloudflare::CF_TOKENS_PAGE,
    };
    crate::app_lifecycle::open_external_url(url.to_owned())
}
```
`labs_on` is written by copying the settings read from `pairing_commands::ready` (NF-18); the placeholder name above is
replaced by that real code. `HostDeps::real()` builds `HttpRailway::new()`, `HttpCloudflare::new()` (or the stub),
`HttpProbe::new()`, `KeyStore::app()`, `crate::app_paths::app_data_dir()`, a system clock, `std::thread::sleep`, and
`team_relay` reading `team::config::load_checked(&team::config::path_in(&app_data_dir()))` → `Some(relay)` when
`has_team()`.

`dashboard_url()`: Railway → `railway::project_page(project_id)`; Cloudflare → `cloudflare::worker_page(account_id, script)`.

**Tests to write**
`service_tests.rs` (FakeRailway, FakeProbe, MemoryBackend KeyStore, temp dir, `inline_jobs: true`, manual clock):
- `fresh_host_is_idle` → `view().phase == Idle`, `railway_connected == false`.
- `connect_stores_token_and_returns_no_token` → `connect(Railway, "acct_token_123456")` → `railway_connected == true`, `account == Some("ren@example.com")`; `serde_json::to_string(&view)` has no `acct_token_123456`.
- `connect_refuses_empty` → `Err("Paste the token first.")`.
- `connect_refuses_spaces` → `"acct token"` → `Err("Paste only the token, with no spaces.")`.
- `connect_bad_token_stores_nothing` → FakeRailway `me` → `BadToken` → `Err("That token didn't work. Make a new one in Railway and paste it again.")`, token absent.
- `deploy_reaches_ready` → connect then `start_deploy(Railway)` → `view().phase == Ready`, `url == Some("https://pm-relay-production-1a2b.up.railway.app")`.
- `notifier_sees_every_step` → collected views' `step` values include all 7 Railway steps in order.
- `deploy_twice_while_busy_is_refused` → with a FakeRailway that blocks on a channel, a second `start_deploy` → `Err("Wait for the current step to finish.")`. (Use `inline_jobs: false` for this one test and release the channel at the end.)
- `half_made_relay_shows_resume` → save a config with `step: Some(AddStorage)` → `view().phase == Failed`, `error == Some(RESUME)`.
- `cancel_sets_stopped` → `cancelled` true at `CreateProject` → `phase == Failed`, `error == Some("Stopped.")`.
- `check_marks_unhealthy` → ready host, FakeProbe health `Err(Status(502))` → `check_now(false).phase == Unhealthy`, `problem == Some(Down)`.
- `team_fit_cases` → no team → `Some("create")`; team relay equal → `Some("uses")`; team relay other → `Some("move")`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r33-2'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host`
→ `test result: ok.`, `0 failed`, the 12 tests above listed. Then
`cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib p2p::guard_tests` and `--lib desk` → `ok.`.

**Done when**
- [ ] 12 tests pass; p2p and desk guard tests pass.
- [ ] `cargo build --manifest-path packages/app/src-tauri/Cargo.toml` succeeds with the 9 commands registered.

**Do NOT**
- Do not add these commands to any phone, bridge, MCP or CLI route.
- Do not return a token, a masked token or token length from any command.
- Do not run network calls on the async runtime thread; use `spawn_blocking` or the job thread.
- Do not start the app on the primary screen to try it.

**If it fails**
1. `Emitter` trait not found → the app's Tauri version uses `Manager::emit`; copy the import from `pairing_commands.rs`.
2. Deadlock in `changed()` → the notifier is called while `live` is locked; build the view after dropping the lock.
3. `open_external_url` signature differs → it is `pub fn open_external_url(url: String) -> Result<(), String>`
   (relay facts §9); pass an owned `String`.

---

### R33-3 — `lib/relay-host.ts` client and `useRelayHost` hook

Size: Haiku. Deps: R33-2.

**Goal**
Typed wrappers for the commands, the `relay://changed` listener hook, and the string tables used by the components.

**Read first**
- `packages/app/src/lib/desk-pair.ts` — `invoke` wrappers, `usePair`, `listen('pair://changed', …)`,
  `pairErrorText` (spaces facts §4). Copy its import lines (NF-33).
- `packages/app/src/lib/team.ts` — `TeamApi` and the invite call (relay facts §6; field names NF-23).

**Files**
- create `packages/app/src/lib/relay-host.ts`
- create `packages/app/src/lib/relay-host.test.ts`

**Steps**
1. Write `relay-host.ts` (Code).
2. Write tests. Run, commit `ui: relay host client`.

**Code**
```ts
// Your relay: typed calls into the relay_host Tauri commands, the change listener, and the UI strings.
import { invoke } from '@tauri-apps/api/core'; // same import as desk-pair.ts (NF-33)
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useState } from 'react';

// (types from section 3 of the plan go here, exactly)

export interface RelayHostApi {
  view: () => Promise<RelayHostView>;
  connect: (provider: RelayProvider, token: string) => Promise<RelayHostView>;
  disconnect: (provider: RelayProvider) => Promise<RelayHostView>;
  deploy: (provider: RelayProvider) => Promise<RelayHostView>;
  cancel: () => Promise<RelayHostView>;
  check: () => Promise<RelayHostView>;
  teamFit: () => Promise<{ next: TeamFit | null }>;
  openDashboard: () => Promise<void>;
  openTokenPage: (provider: RelayProvider) => Promise<void>;
  onChange: (cb: (v: RelayHostView) => void) => Promise<() => void>;
}

export const relayHostApi: RelayHostApi = {
  view: () => invoke<RelayHostView>('relay_host_view'),
  connect: (provider, token) => invoke<RelayHostView>('relay_host_connect', { provider, token }),
  disconnect: (provider) => invoke<RelayHostView>('relay_host_disconnect', { provider }),
  deploy: (provider) => invoke<RelayHostView>('relay_host_deploy', { provider }),
  cancel: () => invoke<RelayHostView>('relay_host_cancel'),
  check: () => invoke<RelayHostView>('relay_host_check'),
  teamFit: () => invoke<{ next: TeamFit | null }>('relay_host_team_fit'),
  openDashboard: () => invoke<void>('relay_host_open_dashboard'),
  openTokenPage: (provider) => invoke<void>('relay_host_open_token_page', { provider }),
  onChange: (cb) => listen<RelayHostView>('relay://changed', (e) => cb(e.payload)),
};

export function relayErrorText(e: unknown): string {
  if (typeof e === 'string' && e.trim()) return e;
  if (e instanceof Error && e.message.trim()) return e.message;
  return 'Something went wrong. Try again.';
}

export const RELAY_STEPS: Record<RelayProvider, { step: RelayStep; text: string }[]> = {
  railway: [
    { step: 'check_account', text: 'Checking your Railway account' },
    { step: 'create_project', text: 'Making a project called Puppet Master relay' },
    { step: 'add_relay', text: 'Adding the relay' },
    { step: 'add_storage', text: 'Adding storage for messages' },
    { step: 'get_address', text: 'Getting a web address' },
    { step: 'start', text: 'Starting the relay (this can take 2 minutes)' },
    { step: 'check', text: 'Checking that it works' },
  ],
  cloudflare: [
    { step: 'check_account', text: 'Checking your Cloudflare account' },
    { step: 'get_address', text: 'Getting a web address' },
    { step: 'add_relay', text: 'Adding the relay' },
    { step: 'check', text: 'Checking that it works' },
  ],
};

export function problemText(p: RelayProblem, provider: RelayProvider | null, now = new Date()): string {
  const site = provider === 'cloudflare' ? 'Cloudflare' : 'Railway';
  switch (p) {
    case 'down': return 'The relay stopped. Your team isn\u2019t getting updates.';
    case 'unreachable': return `We can\u2019t reach the relay\u2019s address. If your internet works, the relay may have been removed on ${site}.`;
    case 'websocket': return 'The relay answers, but live updates are blocked. Your team still gets updates every 20 seconds.';
    case 'wrong_relay': return 'Something else answers at your relay\u2019s address. It isn\u2019t your relay.';
    case 'old_version': return 'Your relay is out of date. Update it to keep it working with this app.';
    case 'full': return 'Your relay\u2019s storage is full. It frees space as messages older than 7 days are removed.';
    case 'account': return `Puppet Master can\u2019t reach your ${site} account any more. Connect ${site} again.`;
    case 'daily_limit': return `Cloudflare\u2019s free daily limit is used up. Your relay starts again at ${nextUtcMidnight(now)}.`;
  }
}

/** The next 00:00 UTC in local time, for example "1:00 AM". */
export function nextUtcMidnight(now: Date): string {
  const next = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1));
  return next.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
}

export function checkedAgo(lastOkMs: number | null, now: number): string {
  if (!lastOkMs) return 'just now';
  const s = Math.max(0, Math.floor((now - lastOkMs) / 1000));
  if (s < 60) return 'just now';
  const m = Math.floor(s / 60);
  if (m < 60) return m === 1 ? '1 minute ago' : `${m} minutes ago`;
  const h = Math.floor(m / 60);
  return h === 1 ? '1 hour ago' : `${h} hours ago`;
}

export function siteName(provider: RelayProvider | null): string {
  return provider === 'cloudflare' ? 'Cloudflare' : 'Railway';
}

export interface UseRelayHost {
  view: RelayHostView | null;
  error: string | null;
  busy: boolean;
  run: (fn: () => Promise<RelayHostView | void>) => Promise<boolean>;
  clearError: () => void;
}

export function useRelayHost(enabled: boolean, api: RelayHostApi = relayHostApi): UseRelayHost {
  const [view, setView] = useState<RelayHostView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!enabled) { setView(null); return; }
    let stop: (() => void) | null = null;
    let live = true;
    void api.view().then((v) => live && setView(v)).catch((e) => live && setError(relayErrorText(e)));
    void api.onChange((v) => live && setView(v)).then((u) => { stop = u; });
    return () => { live = false; stop?.(); };
  }, [enabled, api]);
  const run = useCallback(async (fn: () => Promise<RelayHostView | void>) => {
    setBusy(true);
    setError(null);
    try {
      const v = await fn();
      if (v) setView(v);
      return true;
    } catch (e) {
      setError(relayErrorText(e));
      return false;
    } finally {
      setBusy(false);
    }
  }, []);
  return { view, error, busy, run, clearError: () => setError(null) };
}
```

**Tests to write**
`relay-host.test.ts`:
- `relayErrorText passes strings through` → `relayErrorText('Paste the token first.')` → same.
- `relayErrorText falls back` → `relayErrorText({})` → `'Something went wrong. Try again.'`.
- `railway has 7 steps, cloudflare 4` → lengths 7 and 4; first Railway text `'Checking your Railway account'`.
- `checkedAgo` → `(null, 0)` → `'just now'`; `(0+1, 61_000)` → `'1 minute ago'`; `(1, 3_600_001)` → `'1 hour ago'`; `(1, 7_200_001)` → `'2 hours ago'`.
- `problemText names the site` → `problemText('account', 'cloudflare')` → `'Puppet Master can’t reach your Cloudflare account any more. Connect Cloudflare again.'`.
- `nextUtcMidnight is a time` → returns a non-empty string containing `:`.

**Run**
`npx vitest run packages/app/src/lib/relay-host.test.ts` → `6 passed`.

**Done when**
- [ ] 6 tests pass; `npx tsc --noEmit -p packages/app` (or the repo's typecheck script) shows no new errors.

**Do NOT**
- Do not cache or store any token in the frontend.
- Do not call `secret_set` or `secret_get_masked` for relay tokens.

**If it fails**
1. `@tauri-apps/api/core` not found → the app uses another import path; copy the one in `desk-pair.ts`.
2. Typographic apostrophes break a string test → compare with the same `\u2019` escapes the code uses.
3. `toLocaleTimeString` differs by machine → only assert the string contains `:`.

---

### R33-4 — Idle state and Connect sheet

Size: Haiku. Deps: R33-3.

**Goal**
`RelayHostSettings` renders the idle state, and the Connect sheet connects Railway or Cloudflare.

**Read first**
- `packages/app/src/components/cockpit/Sheet.tsx` — `Sheet` props (spaces facts §4).
- `packages/app/src/components/desk/AddComputerSheet.tsx` — a `Sheet` with a form (template, spaces facts §4).
- `packages/app/src/components/settings/blocks/TeamSettings.tsx` — the settings block style and class names (NF-30).
- `packages/app/src/lib/relay-host.ts` (R33-3).

**Files**
- create `packages/app/src/components/settings/relay/RelayHostSettings.tsx`
- create `packages/app/src/components/settings/relay/RelayConnectSheet.tsx`
- create `packages/app/src/components/settings/relay/RelayHostSettings.test.tsx`
- create `packages/app/src/components/settings/relay/RelayConnectSheet.test.tsx`
- create `packages/app/src/styles/relay-host.css`

**Steps**
1. Write `RelayConnectSheet.tsx` (Code).
2. Write `RelayHostSettings.tsx` with the idle state and the sheet; other phases render a placeholder `null` until
   R33-5.
3. Write `relay-host.css` with classes `ck-relay`, `ck-relay-body`, `ck-relay-actions`, `ck-relay-link`,
   `ck-relay-note`, `ck-relay-steps`, `ck-relay-step`, `is-done`, `is-now`, `ck-relay-error` in greys only, reusing the
   settings block's spacing variables (NF-30).
4. Tests, run, commit `ui: Your relay idle and connect`.

**Code**
`RelayConnectSheet.tsx`:
```tsx
import { useId, useState } from 'react';
import { Sheet } from '../../cockpit/Sheet';
import type { RelayProvider } from '../../../lib/relay-host';
import { siteName } from '../../../lib/relay-host';

export function RelayConnectSheet({
  provider, busy, error, onConnect, onOpenTokenPage, onClose,
}: {
  provider: RelayProvider;
  busy: boolean;
  error: string | null;
  onConnect: (token: string) => void;
  onOpenTokenPage: () => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const [token, setToken] = useState('');
  const site = siteName(provider);
  const line2 = provider === 'railway'
    ? 'Make a new token. Name it Puppet Master. Copy it.'
    : 'Make a token with the template Edit Cloudflare Workers. Copy it.';
  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={onClose} className="ck-relay-sheet">
      <h2 id={titleId}>Connect {site}</h2>
      <ol className="ck-relay-body">
        <li>
          Open {site} and sign in, or make a free account.{' '}
          <button type="button" onClick={onOpenTokenPage}>Open {site}</button>
        </li>
        <li>{line2}</li>
        <li>
          <label>
            <span>{site} token</span>
            <input
              type="password"
              autoComplete="off"
              spellCheck={false}
              aria-label={`${site} token`}
              placeholder="Paste token"
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
          </label>
          Paste the token here.
        </li>
      </ol>
      {error && <p className="ck-relay-error" role="alert">{error}</p>}
      <p className="ck-relay-note">
        The token is kept in this computer&rsquo;s password store, never in a file. Puppet Master uses it only to set up
        and look after your relay.
      </p>
      <div className="ck-relay-actions">
        <button type="button" disabled={busy} onClick={() => onConnect(token)}>{busy ? 'Checking' : 'Connect'}</button>
        <button type="button" onClick={onClose}>Cancel</button>
      </div>
    </Sheet>
  );
}
```
(Line 3's visible text "Paste the token here." sits next to the field; keep the order of the three lines as in §2c.)

`RelayHostSettings.tsx` (idle part; signature final):
```tsx
import '../../../styles/relay-host.css';
import { useState } from 'react';
import { relayHostApi, siteName, useRelayHost, type RelayHostApi, type RelayProvider } from '../../../lib/relay-host';
import { RelayConnectSheet } from './RelayConnectSheet';

export function RelayHostSettings({ api = relayHostApi, onSaved }: { api?: RelayHostApi; onSaved?: () => void }) {
  const { view, error, busy, run, clearError } = useRelayHost(true, api);
  const [sheet, setSheet] = useState<RelayProvider | null>(null);
  const [haveOne, setHaveOne] = useState(false);
  if (!view) return null;
  if (view.phase !== 'idle') return null; // R33-5 fills the other phases
  const connected: RelayProvider | null = view.railwayConnected ? 'railway' : view.cloudflareConnected ? 'cloudflare' : null;
  return (
    <section className="ck-relay" aria-label="Your relay">
      {!connected && (
        <>
          <p className="ck-relay-body">
            Your team needs a relay so your computers can reach each other. Make your own in about 3 minutes. It runs on
            your own Railway account, and Railway charges you about 5 US dollars a month. Your messages are sealed: the
            relay can&rsquo;t read them.
          </p>
          <div className="ck-relay-actions">
            <button type="button" onClick={() => { clearError(); setSheet('railway'); }}>Make my relay with Railway</button>
          </div>
          <button type="button" className="ck-relay-link" onClick={() => { clearError(); setSheet('cloudflare'); }}>
            Use Cloudflare instead (free, with daily limits)
          </button>
          <button type="button" className="ck-relay-link" onClick={() => setHaveOne(true)}>I already have a relay</button>
          {haveOne && <p className="ck-relay-note">Type its address in Team above.</p>}
        </>
      )}
      {connected && (
        <>
          <p className="ck-relay-body">
            {connected === 'railway'
              ? view.account ? `Railway connected as ${view.account}.` : 'Railway connected.'
              : `Cloudflare connected: ${view.account ?? 'your account'}.`}
          </p>
          {connected === 'cloudflare' && (
            <p className="ck-relay-note">
              Cloudflare is free. If your team sends a lot in one day, the relay stops until 00:00 UTC. Backups
              aren&rsquo;t available on Cloudflare.
            </p>
          )}
          <div className="ck-relay-actions">
            <button type="button" disabled={busy} onClick={() => void run(() => api.deploy(connected))}>Make my relay</button>
          </div>
          <button type="button" className="ck-relay-link" onClick={() => void run(() => api.disconnect(connected))}>
            Disconnect {siteName(connected)}
          </button>
        </>
      )}
      {error && !sheet && <p className="ck-relay-error" role="alert">{error}</p>}
      {sheet && (
        <RelayConnectSheet
          provider={sheet}
          busy={busy}
          error={error}
          onOpenTokenPage={() => void api.openTokenPage(sheet)}
          onClose={() => { setSheet(null); clearError(); }}
          onConnect={(token) => void run(() => api.connect(sheet, token)).then((ok) => { if (ok) { setSheet(null); onSaved?.(); } })}
        />
      )}
    </section>
  );
}
```

**Tests to write**
With a fake `RelayHostApi` (every method a `vi.fn`, `onChange` resolves to a no-op unsubscribe):
- `RelayHostSettings.test.tsx`:
  - `idle shows the Railway pitch` → text `Make my relay with Railway` and the body sentence starting `Your team needs a relay`.
  - `Cloudflare link opens the Cloudflare sheet` → click → heading `Connect Cloudflare`.
  - `I already have a relay shows the hint` → click → `Type its address in Team above.`
  - `connected Railway shows email and Make my relay` → view `railwayConnected: true, account: 'ren@example.com'` → `Railway connected as ren@example.com.`; click `Make my relay` → `api.deploy` called with `'railway'`.
  - `connected Cloudflare shows the free note` → `cloudflareConnected: true, account: "Ren's Account"` → `Cloudflare connected: Ren's Account.` and the note.
- `RelayConnectSheet.test.tsx`:
  - `Open Railway calls the token page` → click `Open Railway` → `onOpenTokenPage` called once.
  - `Connect sends the pasted token` → type `abc123` into the field labelled `Railway token`, click `Connect` → `onConnect('abc123')`.
  - `field hides characters` → the input has `type="password"`.
  - `error is shown` → `error="That token didn't work. Make a new one in Railway and paste it again."` → visible with `role="alert"`.
  - `busy shows Checking` → `busy` → button text `Checking`, disabled.

**Run**
`npx vitest run packages/app/src/components/settings/relay` → `10 passed`.

**Done when**
- [ ] 10 tests pass.
- [ ] Every string matches §2b and §2c.

**Do NOT**
- Do not keep the token in state after a successful connect (the sheet unmounts, which drops it).
- Do not log the token or put it in a URL.
- Do not use colour other than greys.

**If it fails**
1. `Sheet` traps focus and tests cannot find the field → query by label inside `screen.getByRole('dialog')`.
2. `onChange` promise warning in tests → make the fake return `Promise.resolve(() => {})`.
3. CSS import fails in Vitest → the repo's Vitest config handles CSS for `DeskPage`; if not, mock it like other component
   tests do.

---

### R33-5 — Deploying, ready, unhealthy, failed and removed states

Size: Haiku. Deps: R33-4.

**Goal**
`RelayHostSettings` shows every other phase with the exact strings in §2d to §2g.

**Read first**
- `RelayHostSettings.tsx` (R33-4).
- `packages/app/src/lib/team.ts` — the invite call and its return fields (NF-23).

**Files**
- create `packages/app/src/components/settings/relay/RelayDeployProgress.tsx`
- create `packages/app/src/components/settings/relay/RelayReadyCard.tsx`
- create `packages/app/src/components/settings/relay/RelayProblemCard.tsx`
- edit `packages/app/src/components/settings/relay/RelayHostSettings.tsx`
- edit `packages/app/src/components/settings/relay/RelayHostSettings.test.tsx`

**Steps**
1. Write the three components (Code).
2. In `RelayHostSettings`, replace `if (view.phase !== 'idle') return null;` with a `switch` on `view.phase` rendering
   the matching component; `failed` and `removed` are rendered inline (Code).
3. Add `teamApi?: TeamApi` to `RelayHostSettings` props (default the app's `teamApi`), used by **Invite people**.
4. Tests, run, commit `ui: Your relay states`.

**Code**
`RelayDeployProgress.tsx`:
```tsx
import { RELAY_STEPS, type RelayProvider, type RelayStep } from '../../../lib/relay-host';

export function RelayDeployProgress({
  provider, step, stopping, onStop,
}: { provider: RelayProvider; step: RelayStep | null; stopping: boolean; onStop: () => void }) {
  const steps = RELAY_STEPS[provider];
  const at = step ? steps.findIndex((s) => s.step === step) : 0;
  return (
    <section className="ck-relay" aria-label="Making your relay">
      <h3>Making your relay</h3>
      <ol className="ck-relay-steps">
        {steps.map((s, i) => (
          <li key={s.step} className={`ck-relay-step${i < at ? ' is-done' : i === at ? ' is-now' : ''}`} aria-current={i === at ? 'step' : undefined}>
            {s.text}
          </li>
        ))}
      </ol>
      <p className="ck-relay-note">You can close Settings. We&rsquo;ll keep going.</p>
      <div className="ck-relay-actions">
        <button type="button" disabled={stopping} onClick={onStop}>{stopping ? 'Stopping after this step' : 'Stop'}</button>
      </div>
    </section>
  );
}
```
`RelayReadyCard.tsx`:
```tsx
import { useState } from 'react';
import { checkedAgo, siteName, type RelayHostView, type TeamFit } from '../../../lib/relay-host';

export function RelayReadyCard({
  view, now, teamFit, invite, onCheck, onOpenDashboard, onUseForTeam, onInvite,
}: {
  view: RelayHostView; now: number; teamFit: TeamFit | null; invite: string | null;
  onCheck: () => void; onOpenDashboard: () => void; onUseForTeam: () => void; onInvite: () => void;
}) {
  const [copied, setCopied] = useState<'url' | 'invite' | null>(null);
  const copy = (text: string, what: 'url' | 'invite') => void navigator.clipboard.writeText(text).then(() => setCopied(what));
  const site = siteName(view.provider);
  return (
    <section className="ck-relay" aria-label="Your relay">
      <h3>Your relay is ready</h3>
      <p>Address: {view.url} <button type="button" onClick={() => copy(view.url ?? '', 'url')}>{copied === 'url' ? 'Copied' : 'Copy'}</button></p>
      <p>Working · checked {checkedAgo(view.lastOkMs, now)}</p>
      {view.version && <p>Version {view.version}</p>}
      <p>On {site}</p>
      {view.usedByTeam ? (
        <>
          <p>Your team uses this relay.</p>
          <div className="ck-relay-actions"><button type="button" onClick={onInvite}>Invite people</button></div>
          {invite && (
            <div className="ck-relay-invite">
              <p>Send this link to the people you want on your team. It stops working after 10 minutes. When they open it, you&rsquo;ll see a request here to let them in.</p>
              <code>{invite}</code>
              <button type="button" onClick={() => copy(invite, 'invite')}>{copied === 'invite' ? 'Copied' : 'Copy link'}</button>
            </div>
          )}
        </>
      ) : (
        <>
          <div className="ck-relay-actions"><button type="button" onClick={onUseForTeam}>Use it for my team</button></div>
          {teamFit === 'create' && <p className="ck-relay-note">Go to Team above, type your team&rsquo;s name and press Create team. Your relay is already filled in.</p>}
          {teamFit === 'move' && <p className="ck-relay-note">Your team uses another relay.</p>}
        </>
      )}
      <div className="ck-relay-actions">
        <button type="button" onClick={onCheck}>Check now</button>
        <button type="button" onClick={onOpenDashboard}>Open {site}</button>
      </div>
    </section>
  );
}
```
`RelayProblemCard.tsx`:
```tsx
import { problemText, siteName, type RelayHostView } from '../../../lib/relay-host';

export function RelayProblemCard({
  view, busy, onCheck, onOpenDashboard, onReconnect,
}: { view: RelayHostView; busy: boolean; onCheck: () => void; onOpenDashboard: () => void; onReconnect: () => void }) {
  const site = siteName(view.provider);
  return (
    <section className="ck-relay" aria-label="Your relay">
      <h3>Your relay isn&rsquo;t answering</h3>
      {view.problem && <p role="alert">{problemText(view.problem, view.provider)}</p>}
      <div className="ck-relay-actions">
        {view.problem === 'account'
          ? <button type="button" onClick={onReconnect}>Connect {site}</button>
          : <button type="button" disabled={busy} onClick={onCheck}>Check again</button>}
        <button type="button" onClick={onOpenDashboard}>Open {site}</button>
      </div>
    </section>
  );
}
```
`RelayHostSettings` switch (the rest of the component from R33-4 stays):
```tsx
const [teamFit, setTeamFit] = useState<TeamFit | null>(null);
const [invite, setInvite] = useState<string | null>(null);
switch (view.phase) {
  case 'deploying':
    return <RelayDeployProgress provider={view.provider ?? 'railway'} step={view.step} stopping={view.stopping} onStop={() => void run(() => api.cancel())} />;
  case 'ready':
    return (
      <RelayReadyCard
        view={view} now={Date.now()} teamFit={teamFit} invite={invite}
        onCheck={() => void run(() => api.check())}
        onOpenDashboard={() => void api.openDashboard()}
        onUseForTeam={() => void api.teamFit().then((r) => setTeamFit(r.next))}
        onInvite={() => void teamApi.invite().then((r) => setInvite(r.link))} // field names per NF-23
      />
    );
  case 'unhealthy':
    return <RelayProblemCard view={view} busy={busy} onCheck={() => void run(() => api.check())} onOpenDashboard={() => void api.openDashboard()} onReconnect={() => setSheet(view.provider ?? 'railway')} />;
  case 'failed':
    return (
      <section className="ck-relay" aria-label="Your relay">
        <h3>We couldn&rsquo;t finish making your relay</h3>
        {view.error && <p role="alert">{view.error}</p>}
        <div className="ck-relay-actions">
          <button type="button" disabled={busy} onClick={() => void run(() => api.deploy(view.provider ?? 'railway'))}>Try again</button>
          {view.hasProject && <button type="button" onClick={() => void api.openDashboard()}>Open Railway</button>}
        </div>
      </section>
    );
  case 'removed':
    return (
      <section className="ck-relay" aria-label="Your relay">
        <h3>Your relay was removed.</h3>
        {view.usedByTeam && <p>Your team won&rsquo;t get updates until it uses another relay.</p>}
        <div className="ck-relay-actions"><button type="button" onClick={() => setSheet('railway')}>Make a new relay</button></div>
      </section>
    );
  case 'fixing':
  case 'removing':
    return null; // file 34 fills these
  case 'idle':
    break; // the R33-4 idle markup follows
}
```
(The `sheet` render from R33-4 must also be reachable from `unhealthy` and `removed`; move the
`{sheet && <RelayConnectSheet … />}` block so it renders in every phase.)

**Tests to write**
In `RelayHostSettings.test.tsx`, a `view(overrides)` helper with every field set:
- `deploying shows the step list with the current step` → `phase: 'deploying', provider: 'railway', step: 'add_storage'` → 7 items; the item `Adding storage for messages` has `aria-current="step"`.
- `stop calls cancel` → click `Stop` → `api.cancel` called.
- `ready shows address and Use it for my team` → `Address: https://pm-relay-production-1a2b.up.railway.app`, `Use it for my team`.
- `use it for my team with no team shows the hint` → `teamFit` resolves `{ next: 'create' }` → hint text from §2e.
- `team uses it shows Invite people and the link` → `usedByTeam: true`; fake `teamApi.invite` resolves `{ link: 'https://example.invalid/join#x' }` → after click, the link text and `Copy link`.
- `unhealthy down shows the plain problem` → `phase: 'unhealthy', problem: 'down'` → `The relay stopped. Your team isn’t getting updates.`
- `unhealthy account shows Connect Railway` → `problem: 'account'` → button `Connect Railway`.
- `failed shows Try again` → `phase: 'failed', error: 'Railway couldn\'t start the relay. Open Railway to see why.'` → that text and `Try again`.
- `removed shows Make a new relay` → `phase: 'removed', usedByTeam: true` → both texts of §2g.

**Run**
`npx vitest run packages/app/src/components/settings/relay` → `19 passed` (10 + 9).

**Done when**
- [ ] 19 tests pass; every string matches §2.

**Do NOT**
- Do not show the token, project id or account id on screen.
- Do not add Fix, Restart, Backup, Update or Remove buttons here (file 34).

**If it fails**
1. `navigator.clipboard` is undefined in jsdom → stub it in the test with `Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } })`.
2. The invite field is not `link` → use the field NF-23 names; change the fake to match.
3. Two elements match `Open Railway` → scope queries to the section with `within(screen.getByRole('region', { name: 'Your relay' }))`.

---

### R33-6 — Settings row, Team form hand-off, screen-level test

Size: Haiku. Deps: R33-5, R32-6.

**Goal**
The row appears in Settings under Team when Labs is on, the Team create form is pre-filled with the new relay, and a
screen-level test proves both.

**Read first**
- `packages/app/src/components/settings/sections/GeneralSection.tsx` lines 528-550 — the desk Labs toggle pattern and
  the Team and Live Room entries (relay facts §11, spaces facts §6).
- `packages/app/src/components/settings/blocks/TeamSettings.tsx` — `TeamSettings`, `CreateForm` (line 298),
  `RelayHelp` (relay facts §6).
- `packages/app/src/components/settings/blocks/TeamSettings.test.tsx` line 82 — the help-text assertion.
- How `GeneralSection` is rendered in an existing test (NF-37).

**Files**
- create `packages/app/src/components/settings/relay/RelayHostRow.tsx`
- create `packages/app/src/components/settings/relay/RelayHostRow.test.tsx`
- edit `packages/app/src/components/settings/sections/GeneralSection.tsx`
- edit `packages/app/src/components/settings/blocks/TeamSettings.tsx`
- edit `packages/app/src/components/settings/blocks/TeamSettings.test.tsx`

**Steps**
1. Write `RelayHostRow.tsx` (Code): `RelayHostRow` and `TeamWithRelay`.
2. In `GeneralSection.tsx`, change the Team entry's `render` to
   `render: (env) => <TeamWithRelay relayOn={env.settings.relay_host_enabled ?? false} onSaved={env.markSaved} />`.
3. Add the new entry right after the Team entry (Code).
4. In `TeamSettings.tsx`: add prop `suggestedRelay?: string` to `TeamSettings` and pass it to `CreateForm`; in
   `CreateForm`, start the relay field with `suggestedRelay` when the field is empty, and show the line
   `This is the relay you made in Your relay.` under the field while its value equals `suggestedRelay`.
5. Tests, run, commit `ui: Your relay in Settings`.

**Code**
`RelayHostRow.tsx`:
```tsx
import { relayHostApi, useRelayHost, type RelayHostApi } from '../../../lib/relay-host';
import { TeamSettings } from '../blocks/TeamSettings';
import { RelayHostSettings } from './RelayHostSettings';

/** The Settings row body. When the Labs key is off, the row shows nothing (the Labs list has the switch). */
export function RelayHostRow({ enabled, onSaved, api = relayHostApi }: { enabled: boolean; onSaved?: () => void; api?: RelayHostApi }) {
  if (!enabled) return <p className="ck-relay-note">Turn on Your relay in Settings › Labs first.</p>;
  return <RelayHostSettings api={api} onSaved={onSaved} />;
}

/** Team settings, with the address of a ready relay passed in for the create form. */
export function TeamWithRelay({ relayOn, onSaved, api = relayHostApi }: { relayOn: boolean; onSaved?: () => void; api?: RelayHostApi }) {
  const { view } = useRelayHost(relayOn, api);
  const suggested = view && view.phase === 'ready' && view.url ? view.url : undefined;
  return <TeamSettings onSaved={onSaved} suggestedRelay={suggested} />;
}
```
`GeneralSection.tsx` new entry (after Team):
```tsx
{
  id: 'your-relay',
  label: 'Your relay',
  hint: 'Make a relay for your team on your own Railway or Cloudflare account.',
  keywords: 'relay railway cloudflare server host team heads-up spaces',
  group: 'Labs',
  labs: { name: 'Your relay', about: 'Make your own relay for your team in a few clicks.' },
  render: (env) => <RelayHostRow enabled={env.settings.relay_host_enabled ?? false} onSaved={env.markSaved} />,
},
```
The Labs on/off switch for `relay_host_enabled` follows the desk pattern at lines 528-529:
`checked={env.settings.relay_host_enabled ?? false}` and
`onChange={(value) => void env.patch({ relay_host_enabled: value })}`, in the same place the desk switch lives.

**Tests to write**
- `RelayHostRow.test.tsx`:
  - `off shows the Labs line` → `enabled={false}` → `Turn on Your relay in Settings › Labs first.`
  - `on shows the idle pitch` → `enabled` with a fake api in `idle` → `Make my relay with Railway`.
  - `TeamWithRelay passes a ready address` → fake api view `phase: 'ready', url: 'https://r.example'`; with no team, the field labelled `Relay address` has value `https://r.example` and the line `This is the relay you made in Your relay.` is shown. (Use `TeamSettings`'s `api` fake the existing test uses.)
- `TeamSettings.test.tsx`:
  - keep the line-82 help-text test unchanged (the help text does not change);
  - add `suggested relay fills an empty field` → `<TeamSettings suggestedRelay="https://r.example" api={fake} />` → field value `https://r.example`.
  - add `typed relay is not replaced` → type `https://mine.example`, rerender with a new `suggestedRelay` → value stays `https://mine.example`.
- Screen-level (wire-in rule): `packages/app/src/components/settings/sections/GeneralSection.relay.test.tsx` renders
  `GeneralSection` the way its existing test does (NF-37) with settings `{ relay_host_enabled: true }` and asserts the
  row label `Your relay` and the text `Make my relay with Railway` appear; with `relay_host_enabled: false` the
  `Make my relay with Railway` text is absent.

**Run**
`npx vitest run packages/app/src/components/settings` → all pass, including 3 + 2 + 2 new tests.
Then `npx vitest run packages/e2e/tests/labs/team-forms-need-input.test.mjs` if it runs under Vitest in this repo
(relay facts §6 lists it); otherwise run it the way its folder's README says.

**Done when**
- [ ] Row is visible only with the Labs key on (screen test).
- [ ] Team create form is pre-filled with a ready relay and keeps a typed value.
- [ ] Existing Team tests pass unchanged.

**Do NOT**
- Do not change `RELAY_HELP_URL` or the existing `RelayHelp` text (the line-82 test pins it).
- Do not change the Join form (the address comes from the invite link, relay facts §6).
- Do not start the desktop app on the primary screen to look at it; use the preview tools if a picture is needed.

**If it fails**
1. `env.settings.relay_host_enabled` is a type error → add the key to the settings type (NF-19).
2. The create form overwrites a typed value → only apply `suggestedRelay` when the field is empty and untouched (track a
   `touched` flag on first input).
3. The screen test cannot render `GeneralSection` → render it the same way the nearest existing settings section test
   does; if none exists, render the Team and Your relay entries' `render(env)` with a fake `env` and note that in NF-37.

---

## Open facts

- NEEDS-FACT NF-18: how a Tauri command reads a Labs bool from the settings store. Read `ready(app)` in
  `packages/app/src-tauri/src/pairing_commands.rs` lines 85-100.
- NEEDS-FACT NF-19: where the TypeScript type of `env.settings` declares `model_bridge_enabled` and `desk_enabled`.
  Search `packages/app/src` for `model_bridge_enabled?:`.
- NEEDS-FACT NF-23: the `InviteView` fields returned by `team_invite` (the link field name, whether a separate code is
  shown) and the `TeamApi` method name that calls it. Read `packages/app/src-tauri/src/team/service.rs` near line 1498
  and `packages/app/src/lib/team.ts`.
- NEEDS-FACT NF-30: the CSS class names and spacing variables the settings blocks use (`TeamSettings.tsx`). Read that
  file and its stylesheet.
- NEEDS-FACT NF-33: the `invoke` and `listen` import paths. Read the top of `packages/app/src/lib/desk-pair.ts`.
- NEEDS-FACT NF-37: how an existing test renders `GeneralSection` (props, providers). Search `packages/app/src` for
  `GeneralSection` in `*.test.tsx`.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Token shown back in the UI | Shoulder-surfing, screenshots, screen share in a Live Room | No command returns the token or a mask of it; field is `type="password"`; the sheet unmounts after connect | `connect_stores_token_and_returns_no_token`, `field hides characters` |
| Token kept in frontend memory | A later XSS bug reads it | Token lives only in the sheet's local state until submit | Code review rule in R33-4 `Do NOT`; R35-5 `token_never_leaves_the_credential_store` |
| Relay tokens set through generic secret commands | Agents or other UI read them masked | Names are KeyStore names, not `is_ui_secret` names | R35-6 `relay_tokens_are_not_ui_secrets` |
| Commands used while Labs is off | Hidden feature reachable | Every command checks the Labs key | `FEATURE_OFF` path in `host()`; R35-6 guard |
| Phone, bridge or MCP presses Make my relay or Disconnect | Spend money, break the team | Commands registered only in the desktop handler; guard test | R35-6 `relay_host_is_not_reachable_from_bridge_mcp_or_phone` |
| Prefilled address swapped | Team created on someone else's relay | The suggestion comes only from `relay_host.json` via the service, not from a link or page | `TeamWithRelay passes a ready address` |
| Clickjacking the Connect button | n/a in a desktop app | Sheet is app-owned | none needed |

## What cannot be verified without real hardware or a second computer

- The whole flow on screen with a real Railway token (needs the founder's account).
- The system browser opening Railway's and Cloudflare's token pages on macOS and Linux.
- The credential store prompt behaviour on macOS (Keychain may ask the person to allow access).
- A teammate on a second computer opening the invite link from **Invite people** and being let in.
