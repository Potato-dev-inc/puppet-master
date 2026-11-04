# 32 — Relay deploy flows: Railway (default) and Cloudflare (fallback)

Status: plan only. Part of the one-click relay family (overview: `30-one-click-relay-overview.md`).

Sources the writer used: `00-FORMAT.md`, `_facts/relay-facts.md` ("relay facts §N"), `_facts/spaces-facts.md`
("spaces facts §N"). No source code was opened by the writer. NEEDS-FACT lines use the shared `NF-n` numbering and are
listed in `Open facts` near the end.

This file covers: the exact click-by-click flow for the person, the exact API calls the app makes, the template files to
add (full contents), how the app learns the relay address, and how it proves the relay works before it says "ready".
The Settings UI that drives it is file 33. Health, fixing and removal are file 34.

New app module: `packages/app/src-tauri/src/relay_host/` (Rust). It is not part of `p2p/` and must not use `p2p::`
(spaces facts §2, `only_known_files_use_the_p2p_module`). Team traffic still goes only through the existing
`RelayTransport` (relay facts §6); `relay_host` only deploys and checks relays, and is not reachable from the bridge,
MCP or the phone (file 35 R35-6).

---

## A. Default: Railway

### A1. What the person does (click by click)

| # | Where | The person does | The app does |
|---|---|---|---|
| 1 | Settings › Labs | Turns on **Your relay** | Saves `relay_host_enabled: true` (file 33 R33-1) |
| 2 | Settings › Your relay | Clicks **Make my relay with Railway** | Opens the Connect Railway sheet |
| 3 | Connect Railway sheet | Clicks **Open Railway** | Calls `relay_host_open_token_page("railway")`, which opens `https://railway.com/account/tokens` in the system browser |
| 4 | Railway's site | Signs in or signs up (Railway's own page; the app never sees the password) | nothing |
| 5 | Railway's site, Tokens page | Clicks the button to make a new token, types the name `Puppet Master`, leaves the workspace choice on the personal account, confirms, copies the token (exact Railway labels: NF-10) | nothing |
| 6 | Connect Railway sheet | Pastes into **Railway token**, clicks **Connect** | `relay_host_connect("railway", token)`: runs the `me` query (A2 call 1); on success stores the token under KeyStore name `relay_railway_token`; never returns it |
| 7 | Your relay | Sees "Railway connected as ren@example.com." Clicks **Make my relay** | `relay_host_deploy("railway")`: starts the runner (A2 calls 2-11) in a background thread and sends `relay://changed` after each step |
| 8 | Your relay | Waits about 2 to 4 minutes, watching the 7 steps | Saves ids after each step to `relay_host.json` so a crash or app restart resumes at the same step |
| 9 | Your relay | Sees **Your relay is ready** with the address | Has checked health, version, owner, WebSocket (section C) |

### A2. Railway API calls, in order

Endpoint: `POST https://backboard.railway.com/graphql/v2`, body `{"query": "...", "variables": {...}}`,
header `Content-Type: application/json`. Auth header: `Authorization: Bearer <token>` for the account token;
`Project-Access-Token: <token>` for the project token. Every query text below must be checked against Railway's public
API schema before R32-3 merges (NF-6); the builder fixes field names to match the schema, and changes nothing else.

| # | Step shown | Call | Variables | Keeps from the reply |
|---|---|---|---|---|
| 1 | Checking your Railway account | `query { me { email name } }` | none | `email`, `name` (memory only, for the "connected as" line) |
| 2 | Making a project called Puppet Master relay | `mutation($input: ProjectCreateInput!) { projectCreate(input: $input) { id environments { edges { node { id name } } } } }` | `{"input":{"name":"Puppet Master relay","description":"Team relay made by Puppet Master"}}` | `id` → `projectId`; the environment named `production` (else the first) → `environmentId` |
| 3 | Adding the relay | `mutation($input: ServiceCreateInput!) { serviceCreate(input: $input) { id } }` | `{"input":{"projectId":"…","name":"pm-relay"}}` | `id` → `serviceId` |
| 3b | (same step) | `mutation($input: VariableCollectionUpsertInput!) { variableCollectionUpsert(input: $input) }` | `{"input":{"projectId":"…","environmentId":"…","serviceId":"…","skipDeploys":true,"variables":{…see A3…}}}` | nothing |
| 3c | (same step) | `mutation($serviceId: String!, $environmentId: String!, $input: ServiceInstanceUpdateInput!) { serviceInstanceUpdate(serviceId: $serviceId, environmentId: $environmentId, input: $input) }` | `{"serviceId":"…","environmentId":"…","input":{"source":{"image":"<RELAY_IMAGE>"},"numReplicas":1,"healthcheckPath":"/v1/health","healthcheckTimeout":120,"restartPolicyType":"ON_FAILURE","restartPolicyMaxRetries":10,"sleepApplication":false}}` | nothing |
| 4 | Adding storage for messages | `mutation($input: VolumeCreateInput!) { volumeCreate(input: $input) { id } }` | `{"input":{"projectId":"…","environmentId":"…","serviceId":"…","mountPath":"/data"}}` | `id` → `volumeId` |
| 5 | Getting a web address | `mutation($input: ServiceDomainCreateInput!) { serviceDomainCreate(input: $input) { domain } }` | `{"input":{"serviceId":"…","environmentId":"…","targetPort":8787}}` | `domain` → `url = "https://" + domain` |
| 6 | Starting the relay | `mutation($serviceId: String!, $environmentId: String!) { serviceInstanceDeployV2(serviceId: $serviceId, environmentId: $environmentId) }` | ids | the reply string → `deploymentId` |
| 6b | (same step, every 5 s, up to 10 min) | `query($input: DeploymentListInput!) { deployments(first: 1, input: $input) { edges { node { id status } } } }` | `{"input":{"projectId":"…","serviceId":"…","environmentId":"…"}}` | `status`: `SUCCESS` → next step; `FAILED`, `CRASHED`, `REMOVED` → stop with "Railway couldn't start the relay." |
| 7 | Checking that it works | none on Railway; section C checks | | `version`, `last_ok_ms` |
| 8 | (after ready, no step shown) | `mutation($input: ProjectTokenCreateInput!) { projectTokenCreate(input: $input) }` | `{"input":{"projectId":"…","environmentId":"…","name":"Puppet Master"}}` | the token → KeyStore `relay_railway_project_token`; then delete `relay_railway_token` (DECISION-OPEN D-2, default yes) |

Later calls (file 34): `deploymentRestart(id)`, `deploymentRedeploy(id)`, `serviceInstanceUpdate` (new image),
`projectDelete(id)`. Which of them a project token may call is NF-9; file 34 handles a refusal by opening
`https://railway.com/project/<projectId>/settings`.

### A3. Variables set on the Railway service

Built by the pure function `railway_vars` (slice R32-5). Exactly these keys:

| Variable | Value | Why |
|---|---|---|
| `PM_RELAY_OWNER_KEY` | `pm_relay_protocol::key_id(&owner)` of the KeyStore key `relay_owner` | Admin routes and owner-only registration (file 31 §2) |
| `PM_RELAY_REGISTRATION` | `owner` | Strangers cannot add teams |
| `RUST_LOG` | `info` | Only 4xx and 5xx access lines (file 31 §7) |
| `RAILWAY_RUN_UID` | `0` | Railway volumes may be owned by root; the image user is uid 10001 (NF-7). Remove this row if NF-7 says volumes are writable by the image user. |

Never set: `TEST_HOOKS` (relay facts §1a), any token, any private key. `PM_RELAY_TRUSTED_PROXY` is added only after NF-8
gives Railway's proxy address range; until then every client counts as one address for per-address limits (file 35 R35-4
explains the effect).

### A4. Railway settings

One replica, health check `/v1/health` with 120 s timeout, restart on failure up to 10 times, app sleeping off
(WebSockets must stay up), volume at `/data`, domain target port 8787. The same settings are in `relay/railway.json`
(D2) for developers who deploy from the repo instead of the image.

## B. Fallback: Cloudflare Worker

### B1. What the person does (click by click)

| # | Where | The person does | The app does |
|---|---|---|---|
| 1 | Settings › Your relay | Clicks **Use Cloudflare instead (free, with daily limits)** | Opens the Connect Cloudflare sheet |
| 2 | Connect Cloudflare sheet | Clicks **Open Cloudflare** | Opens `https://dash.cloudflare.com/profile/api-tokens` |
| 3 | Cloudflare's site | Signs in or signs up | nothing |
| 4 | Cloudflare's site | Clicks **Create Token**, picks the template **Edit Cloudflare Workers**, clicks **Use template**, leaves the account and zone choices on their first values, clicks **Continue to summary**, then **Create Token**, then **Copy** (labels to confirm: NF-35) | nothing |
| 5 | Connect Cloudflare sheet | Pastes into **Cloudflare token**, clicks **Connect** | `relay_host_connect("cloudflare", token)`: B2 calls 1-2; stores `relay_cloudflare_token` |
| 6 | Your relay | Sees "Cloudflare connected: Ren's Account." Clicks **Make my relay** | `relay_host_deploy("cloudflare")`: B2 calls 3-6, then section C |
| 7 | Your relay | Sees **Your relay is ready** | |

### B2. Cloudflare API calls, in order

Base `https://api.cloudflare.com/client/v4`, header `Authorization: Bearer <token>`. Replies are
`{"success":bool,"errors":[{"code":n,"message":"…"}],"result":…}`. Exact multipart metadata fields: NF-11.

| # | Step shown | Call | Body | Keeps |
|---|---|---|---|---|
| 1 | Checking your Cloudflare account | `GET /user/tokens/verify` | none | `result.status == "active"` |
| 2 | (same step) | `GET /accounts?per_page=5` | none | `result[0].id` → `accountId`, `result[0].name` → `accountName` (DECISION-OPEN D-13) |
| 3 | Getting a web address | `GET /accounts/{accountId}/workers/subdomain` | none | `result.subdomain`; when missing, `PUT` the same path with `{"subdomain":"pm-<8 random lowercase letters and digits>"}` |
| 4 | Adding the relay | `PUT /accounts/{accountId}/workers/scripts/{script}` (multipart/form-data) | part `metadata` (JSON below) + one part per module from the bundle (R32-8) | nothing |
| 5 | (same step) | `POST /accounts/{accountId}/workers/scripts/{script}/subdomain` | `{"enabled":true}` | nothing |
| 6 | Checking that it works | section C | | `url = "https://{script}.{subdomain}.workers.dev"` |

`script` = `pm-relay-` + 6 random characters from `abcdefghijkmnpqrstuvwxyz23456789`.

Metadata part (first upload):
```json
{
  "main_module": "index.js",
  "compatibility_date": "2025-09-01",
  "bindings": [
    { "type": "durable_object_namespace", "name": "TEAM", "class_name": "Team" },
    { "type": "durable_object_namespace", "name": "GATE", "class_name": "IpGate" },
    { "type": "durable_object_namespace", "name": "ROOM", "class_name": "Room" },
    { "type": "durable_object_namespace", "name": "ROOM_INDEX", "class_name": "RoomIndex" },
    { "type": "plain_text", "name": "PM_RELAY_OWNER_KEY", "text": "<owner key id>" },
    { "type": "plain_text", "name": "PM_RELAY_REGISTRATION", "text": "owner" }
  ],
  "migrations": {
    "new_tag": "v2",
    "steps": [
      { "new_sqlite_classes": ["Team", "IpGate"] },
      { "new_sqlite_classes": ["Room", "RoomIndex"] }
    ]
  },
  "observability": { "enabled": true }
}
```
Binding names, class names, migration tags and `compatibility_date` come from `relay/worker/wrangler.toml`
(relay facts §3). Never a `TEST_HOOKS` binding.

What Cloudflare cannot do in v1 (the UI says it, file 33): no backups, no admin status (the Worker has no admin routes),
no iroh relay, stops for the rest of the day when the free daily limit is hit.

## C. How the app proves the relay works before it says "ready"

Function `verify::wait_until_ready` (R32-4). In order, with a total time limit of 3 minutes after the provider says the
service is up:

1. `GET {url}/v1/health` every 3 s until the reply body parses to `ok == true` and `protocol == "pm-relay/1"`
   (exactly the reply in relay facts §1a). Errors in this phase are expected while DNS and TLS settle.
2. `GET {url}/v1/version`: `protocol == "pm-relay/1"`, `owner ==` this computer's owner key id,
   `registration == "owner"`, and `version` not older than `RELAY_MIN_VERSION` (R32-5 `image.rs`).
3. WebSocket `wss://{host}/v1/probe/ws`: the first message, within 10 s, is exactly `{"probe":"pm-relay/1"}`.
   This is the check that catches "relay WebSocket unavailable" before any teammate sees it.
4. Railway only: `GET {url}/v1/admin/status` with header `x-pm-owner` signed by the `relay_owner` key → 200 and JSON with
   `registration == "owner"`. This proves the relay at that address holds our owner key and our signature format works
   end to end through the proxy.
5. All pass → save `url`, `last_ok_ms`, step `done`; phase `ready`.

Any failure after the time limit → phase `failed` with the problem text from file 33's table (for example
"The relay answers, but live updates are blocked."), and the buttons **Try again** and **Open Railway**.

The address is learned from the provider's reply (A2 call 5 or B2 call 3), never typed. It passes
`normalize_relay_url` (relay facts §6) before it is saved, so only `https://` reaches `relay_host.json`.

## D. Template files to add

### D1. `.github/workflows/relay-image.yml` (slice R32-1)

```yaml
# Builds the relay Docker image and pushes it to GitHub's container registry.
# Manual only (founder rule: no automatic CI). Start it with:
#   gh workflow run relay-image.yml -f version=0.2.0
name: relay-image

on:
  workflow_dispatch:
    inputs:
      version:
        description: "Relay version tag, the same as relay/server/Cargo.toml (for example 0.2.0)"
        required: true

permissions:
  contents: read
  packages: write

jobs:
  image:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Check the version matches Cargo.toml
        run: |
          want="${{ inputs.version }}"
          have=$(grep -m1 '^version' relay/server/Cargo.toml | sed -E 's/version *= *"([^"]+)"/\1/')
          if [ "$want" != "$have" ]; then echo "Cargo.toml says $have, input says $want"; exit 1; fi
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      - id: build
        uses: docker/build-push-action@v6
        with:
          context: relay
          platforms: linux/amd64
          push: true
          build-args: PM_RELAY_COMMIT=${{ github.sha }}
          tags: |
            ghcr.io/potato-dev-inc/pm-relay:${{ inputs.version }}
            ghcr.io/potato-dev-inc/pm-relay:sha-${{ github.sha }}
          labels: |
            org.opencontainers.image.source=https://github.com/Potato-dev-inc/puppet-master
            org.opencontainers.image.version=${{ inputs.version }}
            org.opencontainers.image.revision=${{ github.sha }}
      - name: Print the digest
        run: |
          echo "Image: ghcr.io/potato-dev-inc/pm-relay:${{ inputs.version }}@${{ steps.build.outputs.digest }}"
          echo "Image: ghcr.io/potato-dev-inc/pm-relay:${{ inputs.version }}@${{ steps.build.outputs.digest }}" >> "$GITHUB_STEP_SUMMARY"
```

### D2. `relay/railway.json` (slice R32-1)

For developers who make a Railway service from the GitHub repo with root directory `relay` (the app does not use this
file; it deploys the image). Keys to confirm against Railway's config schema: NF-6.
```json
{
  "$schema": "https://railway.com/railway.schema.json",
  "build": {
    "builder": "DOCKERFILE",
    "dockerfilePath": "Dockerfile"
  },
  "deploy": {
    "numReplicas": 1,
    "healthcheckPath": "/v1/health",
    "healthcheckTimeout": 120,
    "restartPolicyType": "ON_FAILURE",
    "restartPolicyMaxRetries": 10,
    "sleepApplication": false
  }
}
```

### D3. `relay/worker/scripts/bundle.mjs` (slice R32-8) — full file in R32-8 Code.

### D4. `packages/app/src-tauri/src/relay_host/image.rs` (slice R32-5) — full file in R32-5 Code.

---

## Slices

### R32-1 — Publish the relay image to GHCR

Size: Haiku for the files; the founder starts the workflow. Deps: R31-8.

**Goal**
A public, digest-pinned image `ghcr.io/potato-dev-inc/pm-relay:<version>` exists and its digest is recorded.

**Read first**
- `relay/Dockerfile` after R31-8 — the `PM_RELAY_COMMIT` build arg.
- `relay/server/Cargo.toml` — the version R31-8 set.
- `.github/workflows/ci.yml` lines 66-100 — workflow style used in this repo (relay facts §11).
- `relay/HOSTED-RELAY.md` section 10 — where to record the digest.

**Files**
- create `.github/workflows/relay-image.yml` (D1)
- create `relay/railway.json` (D2)
- edit `relay/README.md` (a "Published image" paragraph)
- edit `relay/HOSTED-RELAY.md` (record the digest after the founder's run)

**Steps**
1. Create the two files with the contents in D1 and D2.
2. In `relay/README.md`, add under the Docker row:
   "A ready image is published as `ghcr.io/potato-dev-inc/pm-relay:<version>`. Run it with
   `docker run -d --restart unless-stopped -p 127.0.0.1:8787:8787 -v pm-relay-data:/data ghcr.io/potato-dev-inc/pm-relay:<version>`."
3. Open a PR, merge it (the workflow must be on `main` before it can be started).
4. Ask the founder in chat: "Publish relay image version `<version>` to GHCR now? (`gh workflow run relay-image.yml -f version=<version>`)".
   Wait for a yes (a publish is a release-like step).
5. Founder or builder after the yes runs Run steps 1-3.
6. Founder makes the package public on GitHub: org Potato-dev-inc → Packages → `pm-relay` → Package settings →
   Change visibility → Public (exact path: NF-36).
7. Builder runs Run steps 4-6 and writes the digest line into `relay/HOSTED-RELAY.md` section 10:
   `Published: ghcr.io/potato-dev-inc/pm-relay:<version>@sha256:<digest> on <date>.`
8. Commit `relay: image <version> published` (doc only).

**Code**
D1 and D2 above, in full.

**Tests to write**
No code tests. The workflow's own "Check the version matches Cargo.toml" step refuses a wrong input.

**Run**
1. `gh workflow run relay-image.yml -f version=<version>` → `✓ Created workflow_dispatch event for relay-image.yml at main`.
2. `gh run list --workflow relay-image.yml --limit 1` → one row; note its id.
3. `gh run watch <id>` → ends `✓ main relay-image · <id>` with all steps green.
4. `docker logout ghcr.io; docker pull ghcr.io/potato-dev-inc/pm-relay:<version>` → `Status: Downloaded newer image` (proves anonymous pull works, so the package is public).
5. `docker inspect --format '{{index .RepoDigests 0}}' ghcr.io/potato-dev-inc/pm-relay:<version>` → `ghcr.io/potato-dev-inc/pm-relay@sha256:<64 hex>`.
6. `$id = docker run -d -p 127.0.0.1:18789:8787 ghcr.io/potato-dev-inc/pm-relay:<version> --memory; curl.exe -s --retry 15 --retry-connrefused --retry-delay 1 http://127.0.0.1:18789/v1/version; docker stop $id; docker rm $id`
   → JSON with `"version":"<version>"` and `"commit"` equal to the merged commit.

**Done when**
- [ ] Anonymous pull works.
- [ ] The digest is in `relay/HOSTED-RELAY.md`.
- [ ] `/v1/version` from the published image shows the right version and commit.

**Do NOT**
- Do not add `push:` or `pull_request:` triggers; manual only.
- Do not publish without the founder's yes in chat.
- Do not push a `latest` tag; the app pins a version and digest.
- Do not put any token in the workflow other than `secrets.GITHUB_TOKEN`.

**If it fails**
1. `denied: permission_denied` on push → the org does not let workflows write packages; the founder enables "Read and
   write permissions" for workflows in the repo settings, or the package's "Manage Actions access" adds the repo.
2. Anonymous pull says `unauthorized` → the package is still private; repeat step 6.
3. The version check step fails → the input does not match `relay/server/Cargo.toml`; start again with the right value.

---

### R32-2 — `relay_host` module: config file and KeyStore names

Size: Haiku. Deps: none.

**Goal**
The app has a `relay_host` module with a secret-free `relay_host.json` and four new KeyStore names for tokens and the
owner key.

**Read first**
- `packages/app/src-tauri/src/team/keystore.rs` — `KeyStore`, `member()` (line 89), `invite_code` /
  `set_invite_code` / `forget_invite` (lines 116-128), private `name`, `get_or_make` (spaces facts §3; exact helper
  signatures NF-17).
- `packages/app/src-tauri/src/team/config.rs` — `save` writes `team.json.tmp` then renames (relay facts §6); copy that.
- `packages/app/src-tauri/src/secrets.rs` — `MemoryBackend::new()` for tests (relay facts §7).
- `packages/app/src-tauri/src/lib.rs` lines 110-114 — where modules are declared (spaces facts §9).

**Files**
- create `packages/app/src-tauri/src/relay_host/mod.rs`
- create `packages/app/src-tauri/src/relay_host/config.rs`
- create `packages/app/src-tauri/src/relay_host/tests.rs`
- edit `packages/app/src-tauri/src/team/keystore.rs`
- edit `packages/app/src-tauri/src/lib.rs` (add `mod relay_host;` after `mod pairing_commands;`)

**Steps**
1. Create `relay_host/mod.rs` with the module list in Code (only `config` and `tests` exist after this slice; later
   slices add the others).
2. Create `config.rs` (Code).
3. In `keystore.rs`, add the four name constants and the four methods (Code). `relay_owner` uses the same body as
   `member()` with the name `RELAY_OWNER`. The token methods copy the pattern of `invite_code` / `set_invite_code` /
   `forget_invite`.
4. Add `mod relay_host;` to `lib.rs`.
5. Write `tests.rs`. Run, commit `relay_host: config file and credential names`.

**Code**
`relay_host/mod.rs` (after this slice):
```rust
//! Your relay: make, check and look after a team relay on the person's own Railway or Cloudflare account.
//! Secrets (provider tokens, the owner key) live only in the credential store, through `team::keystore::KeyStore`.
//! This module never carries team traffic; `team::transport::relay::RelayTransport` does.
//! Not reachable from the bridge, MCP or the phone (guard: `relay_host::guard_tests`).

pub mod config;

#[cfg(test)]
mod tests;
```
`relay_host/config.rs`:
```rust
//! `relay_host.json`: the relay this computer made. Ids and the address only. No token, no key.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "relay_host.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider { Railway, Cloudflare }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeployStep { CheckAccount, CreateProject, AddRelay, AddStorage, GetAddress, Start, Check, Done }

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct RailwayIds {
    pub project_id: String,
    pub environment_id: String,
    pub service_id: String,
    pub volume_id: String,
    pub domain: String,
    pub deployment_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudflareIds {
    pub account_id: String,
    pub account_name: String,
    pub script: String,
    pub subdomain: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct HostConfig {
    pub provider: Option<Provider>,
    pub url: String,
    pub image: String,
    pub previous_image: String,
    pub railway: Option<RailwayIds>,
    pub cloudflare: Option<CloudflareIds>,
    pub step: Option<DeployStep>,
    pub created_at_ms: u64,
    pub last_ok_ms: u64,
    pub last_backup_ms: u64,
    pub last_backup_path: String,
    pub removed_url: String,
}

pub fn path_in(dir: &Path) -> PathBuf { dir.join(FILE_NAME) }

/// Missing or unreadable file → empty config (nothing made yet).
pub fn load(path: &Path) -> HostConfig {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Writes `relay_host.json.tmp`, then renames over the real file.
pub fn save(path: &Path, config: &HostConfig) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(config).map_err(|_| "Couldn't save your relay settings.".to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|_| "Couldn't save your relay settings.".to_string())?;
    std::fs::rename(&tmp, path).map_err(|_| "Couldn't save your relay settings.".to_string())
}
```
`team/keystore.rs` additions:
```rust
/// Credential-store names for Your relay (full entry: service `com.puppetmaster.app`, name `team/<profile>/<this>`).
pub const RELAY_OWNER: &str = "relay_owner";
pub const RELAY_RAILWAY_TOKEN: &str = "relay_railway_token";
pub const RELAY_RAILWAY_PROJECT_TOKEN: &str = "relay_railway_project_token";
pub const RELAY_CLOUDFLARE_TOKEN: &str = "relay_cloudflare_token";
const RELAY_TOKENS: [&str; 3] = [RELAY_RAILWAY_TOKEN, RELAY_RAILWAY_PROJECT_TOKEN, RELAY_CLOUDFLARE_TOKEN];

impl KeyStore {
    /// The owner key of the relay this computer made. Made on first use, then stable.
    pub fn relay_owner(&self) -> Result<SigningKey, String> {
        // Same body as `member()`, with RELAY_OWNER as the name.
        self.get_or_make(RELAY_OWNER)
    }

    pub fn relay_token(&self, which: &str) -> Result<Option<String>, String> {
        if !RELAY_TOKENS.contains(&which) { return Err("unknown relay token name".into()); }
        self.backend.get(&self.name(which)).map(|v| v.filter(|s| !s.is_empty()))
    }

    pub fn set_relay_token(&self, which: &str, token: &str) -> Result<(), String> {
        if !RELAY_TOKENS.contains(&which) { return Err("unknown relay token name".into()); }
        if token.is_empty() { return Err("empty token".into()); }
        self.backend.set(&self.name(which), token)
    }

    pub fn forget_relay_token(&self, which: &str) -> Result<(), String> {
        if !RELAY_TOKENS.contains(&which) { return Err("unknown relay token name".into()); }
        self.backend.delete(&self.name(which))
    }
}
```
If `get_or_make` or `name` has another signature (NF-17), write `relay_owner` and the three token methods exactly the way
`member()` and `invite_code` are written, changing only the name string.

**Tests to write**
In `relay_host/tests.rs`, keystore `KeyStore::with_backend(Arc::new(MemoryBackend::new()), "team/test/")`:
- `relay_token_round_trip`: set `relay_railway_token` to `"t1"`, get → `Some("t1")`; forget; get → `None`.
- `relay_token_unknown_name_refused`: `set_relay_token("anthropic_api_key", "x")` → `Err("unknown relay token name")`.
- `relay_owner_is_stable`: two calls give the same public key.
- `relay_owner_differs_from_member`: `relay_owner()` public key != `member()` public key.
- `host_config_round_trip`: fill every field, `save`, `load` → equal.
- `host_config_missing_file_is_default`: `load` of a missing path → `HostConfig::default()`.
- `host_config_has_no_secret_fields`: serialize a filled config to JSON; the set of top-level keys is exactly
  `provider,url,image,previousImage,railway,cloudflare,step,createdAtMs,lastOkMs,lastBackupMs,lastBackupPath,removedUrl`;
  no key contains `token`, `secret` or `password`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-2'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host`
→ `test result: ok. 7 passed; 0 failed`.
Then `cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib p2p::guard_tests` → `ok.` (unchanged).

**Done when**
- [ ] 7 tests pass.
- [ ] `relay_host.json` never has a token field (test).
- [ ] p2p guard tests still pass.

**Do NOT**
- Do not add these names to `PROVIDER_SECRETS` or `SIDECAR_SECRETS` in `secrets.rs`; the UI must not read or set them
  through `secret_get_masked` / `secret_set` (relay facts §7).
- Do not store any token in `relay_host.json`, `team.json` or settings.
- Do not use `p2p::` from `relay_host`.

**If it fails**
1. `self.backend` or `self.name` is private to another impl block → the methods must be in `keystore.rs` itself, not in
   `relay_host`.
2. `relay_owner_is_stable` fails → the method made a new key each time; it must use the same get-or-make helper as
   `member()`.
3. `MemoryBackend` not visible in tests → import it as `crate::secrets::MemoryBackend`.

---

### R32-3 — Railway API client

Size: Sonnet (an external protocol, error mapping, secret handling). Deps: R32-2.

**Goal**
A tested Railway GraphQL client behind a trait, that never lets a token into an error, a log or `Debug` output.

**Read first**
- `packages/app/src-tauri/src/team/transport/relay.rs` — how `ureq::Agent` is built and errors mapped (relay facts §6;
  ureq major version NF-28).
- This file, section A2 — the calls.
- Railway public API schema (NF-6), to fix field names before merge.

**Files**
- create `packages/app/src-tauri/src/relay_host/railway.rs`
- create `packages/app/src-tauri/src/relay_host/railway_fake.rs` (`#[cfg(test)]`)
- create `packages/app/src-tauri/src/relay_host/railway_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs` (`pub mod railway;` and the two test modules)

**Steps**
1. Write `railway.rs` (Code): types, `RailwayApi` trait, `Post` seam, `HttpRailway`.
2. Write `railway_fake.rs`: `FakeRailway` implementing `RailwayApi` with a `calls: Mutex<Vec<String>>` log, a
   `fail_at: Mutex<Option<(&'static str, ApiError)>>`, and a `statuses: Mutex<VecDeque<DeployStatus>>` for
   `latest_deployment`. Every method pushes its name to `calls` first.
3. Write `railway_tests.rs` using a `RecordingPost` that stores the last url, headers and body and returns a canned
   `(status, body)`.
4. Run, commit `relay_host: Railway API client`.

**Code**
`relay_host/railway.rs` (signatures in full; bodies for the non-obvious parts):
```rust
//! Railway's public GraphQL API. Tokens never appear in errors, logs or Debug output.

use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const RAILWAY_API: &str = "https://backboard.railway.com/graphql/v2";
pub const RAILWAY_TOKENS_PAGE: &str = "https://railway.com/account/tokens";

pub fn project_page(project_id: &str) -> String { format!("https://railway.com/project/{project_id}") }
pub fn project_settings_page(project_id: &str) -> String { format!("https://railway.com/project/{project_id}/settings") }

#[derive(Clone)]
pub enum RailwayAuth { Account(String), Project(String) }

impl std::fmt::Debug for RailwayAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Account(_) => "RailwayAuth::Account([hidden])", Self::Project(_) => "RailwayAuth::Project([hidden])" })
    }
}

impl RailwayAuth {
    fn secret(&self) -> &str { match self { Self::Account(t) | Self::Project(t) => t } }
    fn header(&self) -> (&'static str, String) {
        match self {
            Self::Account(t) => ("Authorization", format!("Bearer {t}")),
            Self::Project(t) => ("Project-Access-Token", t.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError { BadToken, NotAllowed, Network, Api(String) }

impl ApiError {
    /// Words for the person.
    pub fn plain(&self) -> String {
        match self {
            Self::BadToken => "That token didn't work. Make a new one in Railway and paste it again.".into(),
            Self::NotAllowed => "This token can't do that. Make an account token in Railway, not a project token.".into(),
            Self::Network => "Railway didn't answer. Check your internet and try again.".into(),
            Self::Api(m) => format!("Railway said: {m}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailwayMe { pub email: String, pub name: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployStatus { Queued, Waiting, Initializing, Building, Deploying, Success, Failed, Crashed, Removed, Removing, Sleeping, Skipped, Other(String) }

impl DeployStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "QUEUED" => Self::Queued, "WAITING" => Self::Waiting, "INITIALIZING" => Self::Initializing,
            "BUILDING" => Self::Building, "DEPLOYING" => Self::Deploying, "SUCCESS" => Self::Success,
            "FAILED" => Self::Failed, "CRASHED" => Self::Crashed, "REMOVED" => Self::Removed,
            "REMOVING" => Self::Removing, "SLEEPING" => Self::Sleeping, "SKIPPED" => Self::Skipped,
            other => Self::Other(other.to_owned()),
        }
    }
    pub fn is_running(&self) -> bool { matches!(self, Self::Success) }
    pub fn is_dead(&self) -> bool { matches!(self, Self::Failed | Self::Crashed | Self::Removed | Self::Skipped) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailwayDeployment { pub id: String, pub status: DeployStatus }

pub struct ServiceSettings<'a> { pub image: &'a str }

pub trait RailwayApi: Send + Sync {
    fn me(&self, auth: &RailwayAuth) -> Result<RailwayMe, ApiError>;
    /// Returns (project id, environment id).
    fn create_project(&self, auth: &RailwayAuth, name: &str) -> Result<(String, String), ApiError>;
    fn create_service(&self, auth: &RailwayAuth, project_id: &str, name: &str) -> Result<String, ApiError>;
    fn set_variables(&self, auth: &RailwayAuth, project_id: &str, environment_id: &str, service_id: &str, vars: &BTreeMap<String, String>) -> Result<(), ApiError>;
    fn configure_service(&self, auth: &RailwayAuth, service_id: &str, environment_id: &str, settings: &ServiceSettings) -> Result<(), ApiError>;
    fn create_volume(&self, auth: &RailwayAuth, project_id: &str, environment_id: &str, service_id: &str, mount_path: &str) -> Result<String, ApiError>;
    fn create_domain(&self, auth: &RailwayAuth, service_id: &str, environment_id: &str, target_port: u16) -> Result<String, ApiError>;
    fn deploy(&self, auth: &RailwayAuth, service_id: &str, environment_id: &str) -> Result<String, ApiError>;
    fn latest_deployment(&self, auth: &RailwayAuth, project_id: &str, service_id: &str, environment_id: &str) -> Result<Option<RailwayDeployment>, ApiError>;
    fn restart(&self, auth: &RailwayAuth, deployment_id: &str) -> Result<(), ApiError>;
    fn redeploy(&self, auth: &RailwayAuth, deployment_id: &str) -> Result<(), ApiError>;
    fn create_project_token(&self, auth: &RailwayAuth, project_id: &str, environment_id: &str, name: &str) -> Result<String, ApiError>;
    fn delete_project(&self, auth: &RailwayAuth, project_id: &str) -> Result<(), ApiError>;
}

/// The one HTTP call the client needs. Tests replace it.
pub trait Post: Send + Sync {
    /// Ok((status, body)); Err(()) when the network failed.
    fn post_json(&self, url: &str, headers: &[(&'static str, String)], body: &str) -> Result<(u16, String), ()>;
}

pub struct UreqPost { agent: ureq::Agent }

impl UreqPost {
    pub fn new() -> Self {
        // ureq 2 style (NF-28): 10 s connect, 30 s overall.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build();
        Self { agent }
    }
}

impl Post for UreqPost {
    fn post_json(&self, url: &str, headers: &[(&'static str, String)], body: &str) -> Result<(u16, String), ()> {
        let mut req = self.agent.post(url).set("Content-Type", "application/json");
        for (k, v) in headers { req = req.set(k, v); }
        match req.send_string(body) {
            Ok(r) => Ok((r.status(), r.into_string().unwrap_or_default())),
            Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().unwrap_or_default())),
            Err(_) => Err(()),
        }
    }
}

pub struct HttpRailway { post: Box<dyn Post>, endpoint: String }

impl HttpRailway {
    pub fn new() -> Self { Self { post: Box::new(UreqPost::new()), endpoint: RAILWAY_API.into() } }
    pub fn with_post(post: Box<dyn Post>) -> Self { Self { post, endpoint: RAILWAY_API.into() } }

    fn gql(&self, auth: &RailwayAuth, query: &str, variables: Value) -> Result<Value, ApiError> {
        let body = json!({ "query": query, "variables": variables }).to_string();
        let (status, text) = self.post.post_json(&self.endpoint, &[auth.header()], &body).map_err(|_| ApiError::Network)?;
        if status == 401 || status == 403 { return Err(ApiError::BadToken); }
        let v: Value = serde_json::from_str(&text).map_err(|_| ApiError::Api(format!("unexpected reply ({status})")))?;
        if let Some(msg) = v.pointer("/errors/0/message").and_then(Value::as_str) {
            let msg = redact(msg, auth.secret());
            let low = msg.to_lowercase();
            if low.contains("not authorized") || low.contains("unauthorized") || low.contains("invalid token") {
                return Err(if matches!(auth, RailwayAuth::Project(_)) { ApiError::NotAllowed } else { ApiError::BadToken });
            }
            return Err(ApiError::Api(msg.chars().take(200).collect()));
        }
        if !(200..300).contains(&status) { return Err(ApiError::Api(format!("error {status}"))); }
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    }
}

/// Replaces every copy of `secret` in `text` with `[hidden]`. Empty or very short secrets are not searched.
pub fn redact(text: &str, secret: &str) -> String {
    if secret.len() < 8 { return text.to_owned(); }
    text.replace(secret, "[hidden]")
}
```
Each trait method on `HttpRailway` sends the matching document from section A2 and reads the reply fields listed there.
The document texts are module constants:
```rust
const Q_ME: &str = "query { me { email name } }";
const M_PROJECT: &str = "mutation($input: ProjectCreateInput!) { projectCreate(input: $input) { id environments { edges { node { id name } } } } }";
const M_SERVICE: &str = "mutation($input: ServiceCreateInput!) { serviceCreate(input: $input) { id } }";
const M_VARS: &str = "mutation($input: VariableCollectionUpsertInput!) { variableCollectionUpsert(input: $input) }";
const M_INSTANCE: &str = "mutation($serviceId: String!, $environmentId: String!, $input: ServiceInstanceUpdateInput!) { serviceInstanceUpdate(serviceId: $serviceId, environmentId: $environmentId, input: $input) }";
const M_VOLUME: &str = "mutation($input: VolumeCreateInput!) { volumeCreate(input: $input) { id } }";
const M_DOMAIN: &str = "mutation($input: ServiceDomainCreateInput!) { serviceDomainCreate(input: $input) { domain } }";
const M_DEPLOY: &str = "mutation($serviceId: String!, $environmentId: String!) { serviceInstanceDeployV2(serviceId: $serviceId, environmentId: $environmentId) }";
const Q_DEPLOYS: &str = "query($input: DeploymentListInput!) { deployments(first: 1, input: $input) { edges { node { id status } } } }";
const M_RESTART: &str = "mutation($id: String!) { deploymentRestart(id: $id) }";
const M_REDEPLOY: &str = "mutation($id: String!) { deploymentRedeploy(id: $id) { id } }";
const M_PROJECT_TOKEN: &str = "mutation($input: ProjectTokenCreateInput!) { projectTokenCreate(input: $input) }";
const M_DELETE: &str = "mutation($id: String!) { projectDelete(id: $id) }";
```
`create_project` picks the environment whose `name == "production"`, else the first edge; no edge →
`Err(ApiError::Api("Railway made a project with no environment".into()))`.

**Tests to write**
In `railway_tests.rs` with `RecordingPost`:
- `account_token_uses_bearer_header`: `me(&RailwayAuth::Account("acct_token_123456".into()))` → recorded header `("Authorization", "Bearer acct_token_123456")`.
- `project_token_uses_project_access_header`: `Project("proj_token_123456")` → header `("Project-Access-Token", "proj_token_123456")`.
- `me_parses_email`: reply `{"data":{"me":{"email":"ren@example.com","name":"Ren"}}}` → `RailwayMe { email: "ren@example.com", name: "Ren" }`.
- `http_401_is_bad_token`: status 401 → `Err(BadToken)`.
- `graphql_not_authorized_with_project_token_is_not_allowed`: `{"errors":[{"message":"Not Authorized"}]}` with a project token → `Err(NotAllowed)`.
- `graphql_error_message_is_redacted`: `{"errors":[{"message":"bad input acct_token_123456 here"}]}` → `Err(Api("bad input [hidden] here"))`.
- `network_error_is_network`: post returns `Err(())` → `Err(Network)`; `plain()` is `Railway didn't answer. Check your internet and try again.`
- `create_project_picks_production`: two environments `staging`, `production` → returns the production id.
- `deployment_status_parsing`: `"CRASHED"` → `Crashed`; `"NEW_THING"` → `Other("NEW_THING")`; `Crashed.is_dead()` true; `Success.is_running()` true.
- `debug_of_auth_hides_token`: `format!("{:?}", RailwayAuth::Account("acct_token_123456".into()))` does not contain `acct_token_123456`.
- `create_service_sends_project_and_name`: recorded body JSON has `variables.input.projectId == "p1"` and `variables.input.name == "pm-relay"`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-3'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::railway_tests`
→ `test result: ok. 11 passed; 0 failed`.

**Done when**
- [ ] 11 tests pass.
- [ ] Every query text was checked against Railway's schema (NF-6) and the check is noted in the PR description.

**Do NOT**
- Do not derive `Debug` on any type that holds a token.
- Do not include the HTTP error text from ureq in `ApiError` (it can carry the URL and headers in some versions).
- Do not call Railway from tests; tests use `RecordingPost` or `FakeRailway`.
- Do not log anything in this file.

**If it fails**
1. `ureq::AgentBuilder` missing → the app uses ureq 3; build the agent the way `relay.rs` builds it and map
   `ureq::Error::StatusCode(code)` for the status arm.
2. Railway rejects a field name in a manual check → fix only that document constant to the schema's name.
3. `redact` misses a token wrapped in quotes → the replace is on the raw message, so quotes do not matter; check the
   test token is at least 8 characters.

---

### R32-4 — Relay checks: health, version, WebSocket probe, owner proof

Size: Sonnet (signing, WebSocket client, a pure judge). Deps: R32-2, R31-5, R31-7.

**Goal**
One function decides, from real replies, whether a relay is ours and working, and names the problem when it is not.

**Read first**
- `packages/app/src-tauri/src/team/transport/relay.rs` — how it opens a WebSocket (`wss://{host}{prefix}/…`) and
  which crate it uses (relay facts §6; crate NF-14), and `normalize_relay_url`.
- `relay/protocol/src/lib.rs` — `auth_header`, `key_id` (NF-3).
- File 31 §4 — the reply shapes.

**Files**
- create `packages/app/src-tauri/src/relay_host/verify.rs`
- create `packages/app/src-tauri/src/relay_host/verify_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs`

**Steps**
1. Write `verify.rs` (Code): types, `RelayProbe` trait, `HttpProbe`, `judge`, `check`, `wait_until_ready`,
   `version_older`.
2. Write tests with a `FakeProbe` that returns queued replies.
3. Run, commit `relay_host: relay checks`.

**Code**
```rust
//! Checks that a relay is ours and works: health, version, WebSocket probe, owner-signed status.

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROBE_FRAME: &str = r#"{"probe":"pm-relay/1"}"#;
pub const PROTOCOL: &str = "pm-relay/1";

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthReply { pub ok: bool, pub protocol: String }

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RelayVersion {
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub registration: String,
    pub owner: Option<String>,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError { Unreachable, Status(u16), BadReply, WebSocket }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Problem { Down, Unreachable, Websocket, WrongRelay, OldVersion, Full, Account, DailyLimit }

pub trait RelayProbe: Send + Sync {
    fn health(&self, base: &str) -> Result<HealthReply, ProbeError>;
    fn version(&self, base: &str) -> Result<RelayVersion, ProbeError>;
    fn ws_probe(&self, base: &str) -> Result<(), ProbeError>;
    /// GET /v1/admin/status signed with `owner`. Ok(json) on 200.
    fn owner_status(&self, base: &str, owner: &SigningKey, now_ms: u64) -> Result<Value, ProbeError>;
}

pub struct HttpProbe { agent: ureq::Agent }

impl HttpProbe {
    pub fn new() -> Self; // 5 s connect, 15 s overall, like RelayTransport's connect timeout (relay facts §6)
}
// HttpProbe::owner_status sets header pm_relay_protocol::OWNER_HEADER to
// pm_relay_protocol::auth_header(owner, "GET", "/v1/admin/status", now_ms, b"") (argument types: NF-3).
// HttpProbe::ws_probe turns https://host into wss://host/v1/probe/ws, connects with the same WebSocket crate
// relay.rs uses (NF-14), reads one message within 10 s, and returns Ok only if it is text equal to PROBE_FRAME.
// HTTP status 0/connect error/timeout → Unreachable; non-2xx → Status(code); JSON that does not parse → BadReply.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckInput<'a> {
    pub health: &'a Result<HealthReply, ProbeError>,
    pub version: Option<&'a Result<RelayVersion, ProbeError>>,
    pub ws: Option<&'a Result<(), ProbeError>>,
    /// Some(Ok(json)) / Some(Err) when the owner check ran; None when it did not (Cloudflare or a light check).
    pub owner: Option<&'a Result<Value, ProbeError>>,
    pub my_owner_id: &'a str,
    pub min_version: &'a str,
}

/// Pure. The first matching rule wins, in this order.
pub fn judge(i: &CheckInput) -> Option<Problem> {
    match i.health {
        Err(ProbeError::Unreachable) => return Some(Problem::Unreachable),
        Err(ProbeError::Status(429)) => return Some(Problem::DailyLimit),
        Err(ProbeError::Status(404)) | Err(ProbeError::BadReply) => return Some(Problem::WrongRelay),
        Err(_) => return Some(Problem::Down),
        Ok(h) if !h.ok || h.protocol != PROTOCOL => return Some(Problem::WrongRelay),
        Ok(_) => {}
    }
    if let Some(v) = i.version {
        match v {
            Err(ProbeError::Status(404)) => return Some(Problem::OldVersion),
            Err(ProbeError::Unreachable) => return Some(Problem::Unreachable),
            Err(_) => return Some(Problem::Down),
            Ok(v) => {
                if v.protocol != PROTOCOL { return Some(Problem::WrongRelay); }
                if v.owner.as_deref() != Some(i.my_owner_id) { return Some(Problem::WrongRelay); }
                if version_older(&v.version, i.min_version) { return Some(Problem::OldVersion); }
            }
        }
    }
    if let Some(o) = i.owner {
        match o {
            Err(ProbeError::Status(401)) | Err(ProbeError::Status(403)) | Err(ProbeError::Status(404)) => return Some(Problem::WrongRelay),
            Err(ProbeError::Unreachable) => return Some(Problem::Unreachable),
            Err(_) => return Some(Problem::Down),
            Ok(json) => {
                let used = json.get("dbBytes").and_then(Value::as_u64).unwrap_or(0);
                let max = json.get("maxDbBytes").and_then(Value::as_u64).unwrap_or(u64::MAX);
                if used >= max { return Some(Problem::Full); }
            }
        }
    }
    if let Some(Err(_)) = i.ws { return Some(Problem::Websocket); }
    None
}

/// True when `a` < `b`, comparing up to three dot-separated whole numbers. Text that does not parse counts as 0.
pub fn version_older(a: &str, b: &str) -> bool {
    let parts = |s: &str| -> [u64; 3] {
        let mut out = [0u64; 3];
        for (i, p) in s.split('.').take(3).enumerate() { out[i] = p.trim().parse().unwrap_or(0); }
        out
    };
    parts(a) < parts(b)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult { pub problem: Option<Problem>, pub version: Option<String>, pub checked_ms: u64 }

/// `deep` adds the WebSocket probe and (when `with_owner`) the owner status call.
pub fn check(probe: &dyn RelayProbe, base: &str, owner: &SigningKey, my_owner_id: &str, deep: bool, with_owner: bool, min_version: &str, now_ms: u64) -> CheckResult;

/// Calls `check(deep = true)` every 3 s until it returns no problem or `timeout_ms` passes.
/// Ok(result) on success; Err(last problem) on timeout or when `cancelled()` turns true (Err(Problem::Down) then).
pub fn wait_until_ready(
    probe: &dyn RelayProbe, base: &str, owner: &SigningKey, my_owner_id: &str, with_owner: bool, min_version: &str,
    now: &dyn Fn() -> u64, sleep: &dyn Fn(u64), cancelled: &dyn Fn() -> bool, timeout_ms: u64,
) -> Result<CheckResult, Problem>;
```

**Tests to write**
`verify_tests.rs`, `judge` table (each test builds a `CheckInput` and asserts the result):
- `judge_all_good_is_none` → health ok, version owner = mine and version `0.2.0`, min `0.2.0`, ws ok, owner json `{"dbBytes":10,"maxDbBytes":100}` → `None`.
- `judge_unreachable` → health `Err(Unreachable)` → `Some(Unreachable)`.
- `judge_502_is_down` → `Err(Status(502))` → `Some(Down)`.
- `judge_429_is_daily_limit` → `Err(Status(429))` → `Some(DailyLimit)`.
- `judge_404_health_is_wrong_relay` → `Some(WrongRelay)`.
- `judge_other_protocol_is_wrong_relay` → health ok with protocol `x/1` → `Some(WrongRelay)`.
- `judge_owner_mismatch_is_wrong_relay` → version owner `other` → `Some(WrongRelay)`.
- `judge_no_version_route_is_old_version` → version `Err(Status(404))` → `Some(OldVersion)`.
- `judge_old_version` → version `0.1.9`, min `0.2.0` → `Some(OldVersion)`.
- `judge_owner_status_403_is_wrong_relay` → `Some(WrongRelay)`.
- `judge_full` → owner json `{"dbBytes":100,"maxDbBytes":100}` → `Some(Full)`.
- `judge_ws_blocked` → ws `Err(WebSocket)` → `Some(Websocket)`.
- `version_older_cases` → `("0.1.9","0.2.0")` true; `("0.2.0","0.2.0")` false; `("1.0.0","0.9.9")` false; `("x","0.0.1")` true.
- `wait_until_ready_after_three_failures` → FakeProbe: health `Err(Unreachable)` 3 times then ok, everything else ok → `Ok`, the fake `sleep` was called 3 times with 3000.
- `wait_until_ready_times_out` → health always `Err(Status(502))`, timeout 9000 with a manual clock that adds 3000 per sleep → `Err(Down)`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-4'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::verify_tests`
→ `test result: ok. 15 passed; 0 failed`.

**Done when**
- [ ] 15 tests pass.
- [ ] `HttpProbe` uses the same WebSocket crate as `relay.rs` (no new dependency).

**Do NOT**
- Do not add a WebSocket or HTTP crate; reuse what `relay.rs` uses.
- Do not accept `http://` for a non-loopback address (run `normalize_relay_url` on `base` first).
- Do not log replies.

**If it fails**
1. `auth_header` wants a different key type → pass the type `relay.rs` passes for `x-pm-auth` (it signs with a
   `SigningKey`, relay facts §6 `device: SigningKey`).
2. WebSocket handshake to `wss://` fails in a manual run with a TLS error → the crate needs its TLS feature; it is already
   enabled for `relay.rs`, so call the same connect function.
3. `judge_owner_mismatch_is_wrong_relay` gives `None` → the version arm compares `owner` after the version check; keep
   the order in Code.

---

### R32-5 — Railway deploy runner

Size: Sonnet (a resumable state machine). Deps: R32-1, R32-3, R32-4.

**Goal**
One call turns a Railway token into a working, verified relay, saving progress after every step so it can resume.

**Read first**
- `relay_host/railway.rs`, `relay_host/verify.rs`, `relay_host/config.rs` (R32-2 to R32-4).
- `relay/HOSTED-RELAY.md` section 10 — the published digest from R32-1.
- `packages/app/src-tauri/src/team/transport/relay.rs` — `normalize_relay_url` (relay facts §6).

**Files**
- create `packages/app/src-tauri/src/relay_host/image.rs`
- create `packages/app/src-tauri/src/relay_host/deploy.rs`
- create `packages/app/src-tauri/src/relay_host/deploy_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs`

**Steps**
1. Create `image.rs` with the digest from `relay/HOSTED-RELAY.md` (Code).
2. Create `deploy.rs` (Code).
3. Write tests with `FakeRailway` (R32-3) and `FakeProbe` (R32-4).
4. Run, commit `relay_host: Railway deploy runner`.

**Code**
`relay_host/image.rs`, whole file:
```rust
//! The relay image the app deploys. Pinned by digest, so a moved tag cannot change what runs.
//! Update all three together when a new image is published (file 32 slice R32-1).

/// The relay version inside RELAY_IMAGE (relay/server/Cargo.toml at publish time).
pub const RELAY_VERSION: &str = "0.2.0";

/// Tag plus digest. The digest is the one printed by the relay-image workflow and recorded in relay/HOSTED-RELAY.md.
pub const RELAY_IMAGE: &str = "ghcr.io/potato-dev-inc/pm-relay:0.2.0@sha256:<64 hex digest from relay/HOSTED-RELAY.md>";

/// The oldest relay this app can look after (has /v1/version, /v1/probe/ws and owner routes).
pub const RELAY_MIN_VERSION: &str = "0.2.0";
```
(The builder replaces `0.2.0` with the real version R31-8 set and the placeholder with the real digest. A guard test in
R35-6 fails if the placeholder text remains.)

`relay_host/deploy.rs`:
```rust
//! The Railway deploy runner. Each step is skipped when its result is already saved, so a crash resumes.

use super::config::{DeployStep, HostConfig, Provider, RailwayIds};
use super::image::{RELAY_IMAGE, RELAY_MIN_VERSION};
use super::railway::{ApiError, RailwayApi, RailwayAuth, ServiceSettings};
use super::verify::{self, Problem, RelayProbe};
use crate::team::keystore::{KeyStore, RELAY_RAILWAY_PROJECT_TOKEN, RELAY_RAILWAY_TOKEN};
use std::collections::BTreeMap;

pub const PROJECT_NAME: &str = "Puppet Master relay";
pub const SERVICE_NAME: &str = "pm-relay";
pub const TOKEN_NAME: &str = "Puppet Master";
pub const START_TIMEOUT_MS: u64 = 10 * 60_000;
pub const READY_TIMEOUT_MS: u64 = 3 * 60_000;
pub const POLL_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployError {
    Cancelled,
    NoToken,
    Api(ApiError),
    StartFailed,
    NotReady(Problem),
    Save(String),
    Keys(String),
}

impl DeployError {
    pub fn plain(&self) -> String {
        match self {
            Self::Cancelled => "Stopped.".into(),
            Self::NoToken => "Connect Railway first.".into(),
            Self::Api(e) => e.plain(),
            Self::StartFailed => "Railway couldn't start the relay. Open Railway to see why.".into(),
            Self::NotReady(p) => super::problem_text(*p).into(),
            Self::Save(m) | Self::Keys(m) => m.clone(),
        }
    }
}

pub struct DeployDeps<'a> {
    pub railway: &'a dyn RailwayApi,
    pub probe: &'a dyn RelayProbe,
    pub keys: &'a KeyStore,
    pub now: &'a dyn Fn() -> u64,
    pub sleep: &'a dyn Fn(u64),
    pub cancelled: &'a dyn Fn() -> bool,
    pub progress: &'a dyn Fn(DeployStep),
    pub save: &'a dyn Fn(&HostConfig) -> Result<(), String>,
}

/// The variables put on the Railway service. Exactly these keys (file 32 §A3).
pub fn railway_vars(owner_id: &str) -> BTreeMap<String, String> {
    let mut v = BTreeMap::new();
    v.insert("PM_RELAY_OWNER_KEY".into(), owner_id.into());
    v.insert("PM_RELAY_REGISTRATION".into(), "owner".into());
    v.insert("RUST_LOG".into(), "info".into());
    v.insert("RAILWAY_RUN_UID".into(), "0".into()); // remove if NF-7 says the volume is writable by uid 10001
    v
}

fn auth(keys: &KeyStore) -> Result<RailwayAuth, DeployError> {
    if let Some(t) = keys.relay_token(RELAY_RAILWAY_TOKEN).map_err(DeployError::Keys)? { return Ok(RailwayAuth::Account(t)); }
    if let Some(t) = keys.relay_token(RELAY_RAILWAY_PROJECT_TOKEN).map_err(DeployError::Keys)? { return Ok(RailwayAuth::Project(t)); }
    Err(DeployError::NoToken)
}

pub fn deploy_railway(d: &DeployDeps, c: &mut HostConfig) -> Result<(), DeployError> {
    let a = auth(d.keys)?;
    let owner = d.keys.relay_owner().map_err(DeployError::Keys)?;
    let owner_id = pm_relay_protocol::key_id(&owner); // argument type: NF-3
    c.provider = Some(Provider::Railway);
    let mut ids = c.railway.clone().unwrap_or_default();
    let save = |c: &mut HostConfig, ids: &RailwayIds, step: DeployStep| -> Result<(), DeployError> {
        c.railway = Some(ids.clone());
        c.step = Some(step);
        (d.save)(c).map_err(DeployError::Save)
    };
    let gate = |step: DeployStep| -> Result<(), DeployError> {
        if (d.cancelled)() { return Err(DeployError::Cancelled); }
        (d.progress)(step);
        Ok(())
    };

    gate(DeployStep::CheckAccount)?;
    if matches!(a, RailwayAuth::Account(_)) { d.railway.me(&a).map_err(DeployError::Api)?; }

    gate(DeployStep::CreateProject)?;
    if ids.project_id.is_empty() {
        let (p, e) = d.railway.create_project(&a, PROJECT_NAME).map_err(DeployError::Api)?;
        ids.project_id = p;
        ids.environment_id = e;
        save(c, &ids, DeployStep::CreateProject)?;
    }

    gate(DeployStep::AddRelay)?;
    if ids.service_id.is_empty() {
        ids.service_id = d.railway.create_service(&a, &ids.project_id, SERVICE_NAME).map_err(DeployError::Api)?;
        save(c, &ids, DeployStep::AddRelay)?;
    }
    d.railway.set_variables(&a, &ids.project_id, &ids.environment_id, &ids.service_id, &railway_vars(&owner_id)).map_err(DeployError::Api)?;
    d.railway.configure_service(&a, &ids.service_id, &ids.environment_id, &ServiceSettings { image: RELAY_IMAGE }).map_err(DeployError::Api)?;
    c.image = RELAY_IMAGE.into();

    gate(DeployStep::AddStorage)?;
    if ids.volume_id.is_empty() {
        ids.volume_id = d.railway.create_volume(&a, &ids.project_id, &ids.environment_id, &ids.service_id, "/data").map_err(DeployError::Api)?;
        save(c, &ids, DeployStep::AddStorage)?;
    }

    gate(DeployStep::GetAddress)?;
    if ids.domain.is_empty() {
        ids.domain = d.railway.create_domain(&a, &ids.service_id, &ids.environment_id, 8787).map_err(DeployError::Api)?;
    }
    let url = crate::team::transport::relay::normalize_relay_url(&format!("https://{}", ids.domain))
        .map_err(|_| DeployError::Api(ApiError::Api("Railway gave an address we can't use".into())))?;
    c.url = url.clone();
    save(c, &ids, DeployStep::GetAddress)?;

    gate(DeployStep::Start)?;
    ids.deployment_id = d.railway.deploy(&a, &ids.service_id, &ids.environment_id).map_err(DeployError::Api)?;
    save(c, &ids, DeployStep::Start)?;
    let started = (d.now)();
    loop {
        if (d.cancelled)() { return Err(DeployError::Cancelled); }
        match d.railway.latest_deployment(&a, &ids.project_id, &ids.service_id, &ids.environment_id).map_err(DeployError::Api)? {
            Some(dep) if dep.status.is_running() => break,
            Some(dep) if dep.status.is_dead() => return Err(DeployError::StartFailed),
            _ => {}
        }
        if (d.now)() - started > START_TIMEOUT_MS { return Err(DeployError::StartFailed); }
        (d.sleep)(POLL_MS);
    }

    gate(DeployStep::Check)?;
    let result = verify::wait_until_ready(d.probe, &url, &owner, &owner_id, true, RELAY_MIN_VERSION, d.now, d.sleep, d.cancelled, READY_TIMEOUT_MS)
        .map_err(DeployError::NotReady)?;

    // Swap the account token for a token that reaches only this project (DECISION-OPEN D-2, default yes).
    if let RailwayAuth::Account(_) = a {
        let pt = d.railway.create_project_token(&a, &ids.project_id, &ids.environment_id, TOKEN_NAME).map_err(DeployError::Api)?;
        d.keys.set_relay_token(RELAY_RAILWAY_PROJECT_TOKEN, &pt).map_err(DeployError::Keys)?;
        d.keys.forget_relay_token(RELAY_RAILWAY_TOKEN).map_err(DeployError::Keys)?;
    }
    c.created_at_ms = (d.now)();
    c.last_ok_ms = result.checked_ms;
    save(c, &ids, DeployStep::Done)?;
    (d.progress)(DeployStep::Done);
    Ok(())
}
```
`super::problem_text` is defined in `mod.rs` in this slice:
```rust
pub fn problem_text(p: verify::Problem) -> &'static str {
    use verify::Problem::*;
    match p {
        Down => "The relay stopped.",
        Unreachable => "We can't reach the relay's address.",
        Websocket => "The relay answers, but live updates are blocked.",
        WrongRelay => "Something else answers at this address. It isn't your relay.",
        OldVersion => "Your relay is out of date.",
        Full => "The relay's storage is full.",
        Account => "Puppet Master can't reach your account any more.",
        DailyLimit => "Cloudflare's free daily limit is used up.",
    }
}
```
(File 33 shows longer UI text for each problem; these short lines are what `DeployError::plain` returns.)

**Tests to write**
`deploy_tests.rs` (KeyStore over `MemoryBackend`; account token `"acct_token_123456"` set first; FakeRailway returns
project `p1`, env `e1`, service `s1`, volume `v1`, domain `pm-relay-production-1a2b.up.railway.app`, deployment `d1`,
statuses `[Building, Deploying, Success]`, project token `"proj_token_654321"`; FakeProbe all good with owner = the
keystore owner id; manual clock):
- `deploy_happy_path_makes_everything_in_order` → `Ok`; calls == `["me","create_project","create_service","set_variables","configure_service","create_volume","create_domain","deploy","latest_deployment","latest_deployment","latest_deployment","create_project_token"]`; `c.url == "https://pm-relay-production-1a2b.up.railway.app"`; `c.step == Some(Done)`; `relay_token(RELAY_RAILWAY_TOKEN) == None`; `relay_token(RELAY_RAILWAY_PROJECT_TOKEN) == Some("proj_token_654321")`.
- `deploy_resumes_after_a_failure` → first run: FakeRailway fails `create_volume` with `Api("boom")` → `Err(Api(Api("boom")))`, saved config has `project_id == "p1"` and `service_id == "s1"`; second run with the failure cleared → `Ok`, and the second run's calls contain no `create_project` and no `create_service`.
- `deploy_stops_when_cancelled` → `cancelled` returns true after `create_project` → `Err(Cancelled)`; no `create_service` call.
- `deploy_reports_crashed_start` → statuses `[Building, Crashed]` → `Err(StartFailed)`; `plain()` == `Railway couldn't start the relay. Open Railway to see why.`
- `deploy_not_ready_when_websocket_blocked` → FakeProbe ws always `Err(WebSocket)` → `Err(NotReady(Websocket))`; account token still present (no swap on failure).
- `deploy_without_token` → no token → `Err(NoToken)`, no calls.
- `railway_vars_are_exactly_these` → keys == `["PM_RELAY_OWNER_KEY","PM_RELAY_REGISTRATION","RAILWAY_RUN_UID","RUST_LOG"]`; no key contains `TEST_HOOKS` or `TOKEN`.
- `progress_reports_every_step_once_in_order` → recorded steps == `[CheckAccount, CreateProject, AddRelay, AddStorage, GetAddress, Start, Check, Done]`.
- `saved_config_never_has_the_token` → every config passed to `save` serialized to JSON contains neither `acct_token_123456` nor `proj_token_654321`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-5'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host`
→ `test result: ok.` with `0 failed`; the 9 tests above listed as `ok`.

**Done when**
- [ ] 9 tests pass, earlier `relay_host` tests still pass.
- [ ] `RELAY_IMAGE` holds the real digest.

**Do NOT**
- Do not call the real Railway API in tests.
- Do not swap the token before the relay is verified.
- Do not set any variable not in `railway_vars`.
- Do not print or log anything.

**If it fails**
1. The second resume run calls `create_project` again → the first run did not `save` after that step; every step that
   gets an id must call `save` before the next call.
2. Borrow errors with the `save` closure capturing `c` → make `save` a plain `fn save(d: &DeployDeps, c: &mut HostConfig, ids: &RailwayIds, step: DeployStep)`.
3. `normalize_relay_url` is private to `relay.rs` → it is `pub fn` (relay facts §6); import the full path
   `crate::team::transport::relay::normalize_relay_url`.

---

### R32-6 — App signs team registration with the owner key

Size: Haiku. Deps: R31-5, R32-2.

**Goal**
When the team's relay is the relay this computer made, team registration carries `x-pm-owner`, so owner-only relays
accept the team.

**Read first**
- `packages/app/src-tauri/src/team/transport/relay.rs` — `RelayTransport` fields, `new`, `register` (relay facts §6).
- `packages/app/src-tauri/src/team/service.rs` — where `RelayTransport::new` is called (NF-16).
- `relay_host/config.rs` (R32-2).

**Files**
- edit `packages/app/src-tauri/src/team/transport/relay.rs`
- edit `packages/app/src-tauri/src/team/service.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs`
- edit `packages/app/src-tauri/src/relay_host/tests.rs`

**Steps**
1. In `RelayTransport`, add field `owner: Option<SigningKey>` (set to `None` in `new`) and method `with_owner`.
2. Add a private pure helper `owner_header(&self, method, path_and_query, body, ts_ms) -> Option<(String, String)>`.
3. In `register`, add the header when `owner_header` returns `Some`.
4. In `relay_host/mod.rs`, add `pub fn owner_for(relay_url: &str) -> Option<SigningKey>`.
5. In `service.rs`, right after each `RelayTransport::new(...)` call, add
   `let transport = match crate::relay_host::owner_for(&config.relay) { Some(k) => transport.with_owner(k), None => transport };`
6. Tests, run, commit `team: sign team registration on our own relay`.

**Code**
```rust
// relay.rs
pub fn with_owner(mut self, owner: SigningKey) -> Self {
    self.owner = Some(owner);
    self
}

fn owner_header(&self, method: &str, path_and_query: &str, body: &[u8], ts_ms: u64) -> Option<(String, String)> {
    let owner = self.owner.as_ref()?;
    Some((
        pm_relay_protocol::OWNER_HEADER.to_string(),
        pm_relay_protocol::auth_header(owner, method, path_and_query, ts_ms, body),
    ))
}
```
In `register`, where the request for `POST {prefix}/v1/teams` is built, before sending:
```rust
if let Some((k, v)) = self.owner_header("POST", &format!("{}/v1/teams", self.prefix), &body, now_ms()) {
    req = req.set(&k, &v);
}
```
(`body` is the exact bytes sent; `now_ms` is `transport::now_ms`, spaces facts §3.)

`relay_host/mod.rs`:
```rust
/// The owner key, when `relay_url` is the relay this computer made. Compares normalized addresses.
pub fn owner_for(relay_url: &str) -> Option<ed25519_dalek::SigningKey> {
    let want = crate::team::transport::relay::normalize_relay_url(relay_url).ok()?;
    let dir = crate::app_paths::app_data_dir();
    let saved = config::load(&config::path_in(&dir));
    if saved.url.is_empty() || saved.url != want { return None; }
    crate::team::keystore::KeyStore::app().relay_owner().ok()
}
```

**Tests to write**
- In `relay.rs` (new `#[cfg(test)] mod owner_header_tests`): `owner_header_is_none_without_owner`; `owner_header_verifies` → build a transport with `with_owner(SigningKey::from_bytes(&[7;32]))`, get the header for `POST /v1/teams` and body `b"{}"`, and check `pm_relay_protocol::verify_request` accepts it for `key_id` of that key (argument order NF-3).
- In `relay_host/tests.rs`: `owner_for_other_url_is_none` → save a config with url `https://a.example`, call the pure part with `https://b.example` → `None`. To test without the real app dir, split `owner_for` into `owner_for_in(dir: &Path, keys: &KeyStore, relay_url: &str)` and test that.
- `owner_for_same_url_with_trailing_slash` → saved `https://a.example`, asked `https://a.example/` → `Some`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-6'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host team::transport`
→ `test result: ok.`, `0 failed`. Then `cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib team` → `ok.`.

**Done when**
- [ ] 4 new tests pass; all team tests pass.
- [ ] Registration on a relay not made by this computer sends no `x-pm-owner` header.

**Do NOT**
- Do not add the owner header to any request other than `POST /v1/teams`.
- Do not change `normalize_relay_url`.
- Do not read the keystore when the URLs differ.

**If it fails**
1. `RelayTransport` is built in several places → apply step 5 to each call in `service.rs`; list them in the PR.
2. Signature fails at the relay → the signed path must include the base prefix (`{prefix}/v1/teams`), exactly what is
   sent on the wire.
3. Team tests break because `owner_for` reads the real app folder → use `owner_for_in` in tests; only the app path calls
   `owner_for`.

---

### R32-7 — Worker: version, probe, owner-only registration

Size: Sonnet (TypeScript + WASM signature check + replay). Deps: R31-5.

**Goal**
The Cloudflare Worker answers `/v1/version` and `/v1/probe/ws`, and refuses team registrations not signed by its owner
when `PM_RELAY_OWNER_KEY` is set.

**Read first**
- `relay/worker/src/index.ts` header comment lines 6-15 and the route sort (relay facts §3).
- `relay/worker/src/env.ts` — `Env`, `DEFAULT_LIMITS`, `limitsFrom`.
- `relay/worker/src/proto.ts` and `relay/worker/src/wasm/pm_relay_wasm.d.ts` — which checks the WASM exports (NF-13).
- `relay/worker/src/gate.ts` — `IpGate` replay store (relay facts §3).
- `relay/worker/test/worker.test.mjs` — test style.

**Files**
- edit `relay/worker/src/env.ts`
- edit `relay/worker/src/index.ts`
- create `relay/worker/src/owner.ts`
- edit `relay/worker/test/worker.test.mjs`
- edit `relay/worker/README.md`

**Steps**
1. Add `PM_RELAY_OWNER_KEY?: string; PM_RELAY_REGISTRATION?: string;` to `Env`.
2. Create `owner.ts` (Code). Signature check calls the WASM export that verifies a signed request (NF-13). If no such
   export exists, add one to `relay/worker/wasm` that wraps `pm_relay_protocol::verify_request`, rebuild with
   `npm run build:wasm` (needs `wasm32-unknown-unknown` and `wasm-bindgen-cli --version 0.2.129`, relay facts §1a), and
   commit the rebuilt `src/wasm/` files.
3. In `index.ts`: route `GET /v1/version` and `GET /v1/probe/ws` before the team routes; in `POST /v1/teams`, call
   `ownerGate` first (Code).
4. Tests, README rows, run, commit `worker: version, probe, owner-only registration`.

**Code**
`relay/worker/src/owner.ts`:
```ts
// Relay owner checks for the Worker. Same rules as the Rust server (relay/server/src/owner.rs).
import type { Env } from './env';
import { verifyRequest } from './proto'; // WASM-backed check; exact export name per NF-13

export const OWNER_HEADER = 'x-pm-owner';
export const NOT_OWNER = 'this relay only takes teams from its owner';

export type Registration = 'open' | 'owner';

export function registration(env: Env): Registration {
  if (env.PM_RELAY_REGISTRATION === 'open') return 'open';
  if (env.PM_RELAY_REGISTRATION === 'owner') return 'owner';
  return env.PM_RELAY_OWNER_KEY ? 'owner' : 'open';
}

export type OwnerResult = 'missing' | 'bad' | 'replayed' | 'ok';

/** Checks x-pm-owner. `seenOnce` stores the header value and returns false if it was seen before. */
export async function checkOwner(
  env: Env,
  request: Request,
  body: Uint8Array,
  seenOnce: (value: string) => Promise<boolean>,
): Promise<OwnerResult> {
  const key = env.PM_RELAY_OWNER_KEY;
  const value = request.headers.get(OWNER_HEADER);
  if (!key || !value) return 'missing';
  const url = new URL(request.url);
  const ok = verifyRequest(key, value, request.method, url.pathname + url.search, Date.now(), body);
  if (!ok) return 'bad';
  return (await seenOnce(value)) ? 'ok' : 'replayed';
}

export function versionReply(env: Env, version: string) {
  const features = ['teams', 'rooms', 'probe'];
  return {
    name: 'pm-relay-worker',
    version,
    protocol: 'pm-relay/1',
    commit: 'unknown',
    registration: registration(env),
    owner: env.PM_RELAY_OWNER_KEY ?? null,
    features,
  };
}

export const PROBE_FRAME = '{"probe":"pm-relay/1"}';

export function probeSocket(): Response {
  const pair = new WebSocketPair();
  const [client, server] = Object.values(pair);
  server.accept();
  server.send(PROBE_FRAME);
  server.close(1000, 'done');
  return new Response(null, { status: 101, webSocket: client });
}
```
`index.ts`, `POST /v1/teams` start:
```ts
const body = new Uint8Array(await request.arrayBuffer());
const result = await checkOwner(env, request, body, (v) => gateSeenOnce(env, request, v));
if (result === 'replayed') return refuse(401, 'request replayed');
if (result === 'bad') return refuse(403, NOT_OWNER);
if (registration(env) === 'owner' && result !== 'ok') return refuse(403, NOT_OWNER);
// existing per-address limit runs only when result !== 'ok'
```
`gateSeenOnce` uses the `IpGate` object's replay store the same way the front door already does for device signatures
(relay facts §3, `gate.ts`). The version string is the `version` from `relay/worker/package.json`, imported at build time.

**Tests to write**
In `worker.test.mjs` (the suite runs under `wrangler dev`; start a second dev instance with
`--var PM_RELAY_OWNER_KEY:<id>` for owner tests):
- `version route answers` → `GET /v1/version` 200, `protocol` `pm-relay/1`, `features` includes `probe`.
- `probe sends one frame` → WebSocket to `/v1/probe/ws`, first message exactly `{"probe":"pm-relay/1"}`.
- `owner worker refuses unsigned registration` → 403 body `this relay only takes teams from its owner`.
- `owner worker accepts signed registration` → 201 (sign with the protocol test helper in `test/proto.mjs`).
- `open worker still accepts unsigned registration` → 201 on the default dev instance.

**Run**
`npm ci --prefix relay/worker` → no errors. Then `npm --prefix relay/worker run test:worker` → all tests pass, including
the 5 new ones. Then `npm --prefix relay/worker run typecheck` → no errors.

**Done when**
- [ ] 5 new tests pass; old Worker tests pass.
- [ ] `TEST_HOOKS` is not referenced by the new code.

**Do NOT**
- Do not set `TEST_HOOKS` in `wrangler.toml` or in any test instance used for owner tests.
- Do not write a second signature check in TypeScript; use the WASM one so the byte layout matches Rust.
- Do not change existing status codes or messages.

**If it fails**
1. `WebSocketPair` is not a type in tests → the test file is plain `.mjs`; only `owner.ts` uses it, typed by
   `@cloudflare/workers-types`.
2. Signed registration gets 403 → the signed path must be `pathname + search` exactly as the client signed it; compare
   with `auth_message` in `relay/protocol/src/lib.rs`.
3. `npm run build:wasm` fails → install `wasm-bindgen-cli` version `0.2.129` exactly (relay facts §1a) into a separate
   cargo root; do not change the version.

---

### R32-8 — Worker bundle for upload by the app

Size: Haiku. Deps: R32-7.

**Goal**
A committed, ready-to-upload Worker bundle and a generated Rust file that embeds it in the app.

**Read first**
- `relay/worker/package.json` scripts (relay facts §3).
- `relay/worker/wrangler.toml` — `main`, rules for text modules (relay facts §3).
- Output of `npx wrangler deploy --dry-run --outdir dist-bundle` (NF-12).

**Files**
- create `relay/worker/scripts/bundle.mjs`
- edit `relay/worker/package.json` (script `"bundle": "node scripts/bundle.mjs"`)
- create `relay/worker/test/bundle.test.mjs`
- create (generated, committed) `relay/worker/dist-bundle/` and `relay/worker/dist-bundle/manifest.json`
- create (generated, committed) `packages/app/src-tauri/src/relay_host/cf_bundle_gen.rs`

**Steps**
1. Write `bundle.mjs` (Code).
2. Run `npm --prefix relay/worker run bundle`.
3. Write `bundle.test.mjs` and add it to `scripts/run-tests.mjs`'s list the same way other test files are listed.
4. Commit the generated files with `relay: worker bundle for the app`.

**Code**
`relay/worker/scripts/bundle.mjs`:
```js
// Builds the Worker into dist-bundle/ for the app's Cloudflare upload, writes manifest.json and the Rust file
// that embeds the modules (packages/app/src-tauri/src/relay_host/cf_bundle_gen.rs). Never deploys.
import { execFileSync } from 'node:child_process';
import { readdirSync, writeFileSync, rmSync, statSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const worker = join(here, '..');
const out = join(worker, 'dist-bundle');
const rust = join(worker, '..', '..', 'packages', 'app', 'src-tauri', 'src', 'relay_host', 'cf_bundle_gen.rs');

rmSync(out, { recursive: true, force: true });
const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
execFileSync(npx, ['wrangler', 'deploy', '--dry-run', '--outdir', 'dist-bundle'], { cwd: worker, stdio: 'inherit' });

const kind = (name) => {
  const ext = extname(name);
  if (ext === '.js' || ext === '.mjs') return 'esm';
  if (ext === '.wasm') return 'wasm';
  if (ext === '.map') return null;
  return 'text';
};
const files = readdirSync(out).filter((n) => statSync(join(out, n)).isFile() && kind(n));
files.sort((a, b) => (a === 'index.js' ? -1 : b === 'index.js' ? 1 : a.localeCompare(b)));
if (files[0] !== 'index.js') throw new Error('dist-bundle has no index.js');
const manifest = files.map((name) => ({ name, type: kind(name) }));
writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

const lines = [
  '// Generated by relay/worker/scripts/bundle.mjs. Do not edit.',
  '//! The Cloudflare Worker bundle the app uploads (file 32 slice R32-9).',
  '',
  'pub struct Module { pub name: &\'static str, pub kind: &\'static str, pub bytes: &\'static [u8] }',
  '',
  'pub const MODULES: &[Module] = &[',
  ...manifest.map(
    (m) => `    Module { name: "${m.name}", kind: "${m.type}", bytes: include_bytes!("../../../../../relay/worker/dist-bundle/${m.name}") },`,
  ),
  '];',
  '',
];
writeFileSync(rust, lines.join('\n'));
console.log(`bundle: ${manifest.length} modules`);
```
The `include_bytes!` path is relative to `packages/app/src-tauri/src/relay_host/`: five `..` reach the repo root.

**Tests to write**
`relay/worker/test/bundle.test.mjs`:
- `manifest starts with index.js as esm` → `manifest[0]` is `{ name: 'index.js', type: 'esm' }`.
- `manifest has a wasm module` → one entry with `type: 'wasm'`.
- `manifest has the three viewer text files` → names ending `viewer.html`, `viewer.js`, `viewer.css` with `type: 'text'` (the text rules in `wrangler.toml`, relay facts §3; exact output names NF-12).
- `rust file lists every manifest entry` → each `name` appears in `cf_bundle_gen.rs`.

**Run**
`npm --prefix relay/worker run bundle` → `bundle: <n> modules`. Then `npm --prefix relay/worker test` → all pass.
Then `$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-8'; cargo build --manifest-path packages/app/src-tauri/Cargo.toml`
after adding `pub mod cf_bundle_gen;` to `relay_host/mod.rs` → builds.

**Done when**
- [ ] `dist-bundle/` and `cf_bundle_gen.rs` are committed and match a fresh `npm run bundle` (run it twice; `git status` clean).
- [ ] The app builds with the embedded bundle.

**Do NOT**
- Do not run `wrangler deploy` without `--dry-run`.
- Do not hand-edit `cf_bundle_gen.rs`.
- Do not include `.map` files.

**If it fails**
1. Dry run writes a different main name (for example `index.mjs`) → change the `index.js` checks and the metadata
   `main_module` in R32-9 to that name; record it in NF-12.
2. Viewer files are inlined into `index.js` instead of separate modules → drop the third test and note it; the upload
   then has only the JS and WASM parts.
3. `npx.cmd` not found on Windows → run from a shell where `npm` works; `execFileSync` needs the `.cmd` name on Windows.

---

### R32-9 — Cloudflare API client and deploy runner

Size: Sonnet. Deps: R32-8, R32-4, R33-2.

**Goal**
One call turns a Cloudflare token into a working Worker relay, verified the same way as Railway (minus owner status).

**Read first**
- Section B of this file.
- `relay_host/deploy.rs` (R32-5) — same runner shape.
- `relay_host/cf_bundle_gen.rs` (R32-8).
- `relay_host/mod.rs` — the provider dispatch added in R33-2.

**Files**
- create `packages/app/src-tauri/src/relay_host/cloudflare.rs`
- create `packages/app/src-tauri/src/relay_host/cloudflare_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/deploy.rs` (add `deploy_cloudflare`)
- edit `packages/app/src-tauri/src/relay_host/mod.rs` (route provider `cloudflare` to it)

**Steps**
1. Write `cloudflare.rs` (Code).
2. Add `deploy_cloudflare` to `deploy.rs` (Code).
3. In `mod.rs`, replace the R33-2 placeholder error `Cloudflare isn't ready yet.` with a call to `deploy_cloudflare`.
4. Tests, run, commit `relay_host: Cloudflare deploy`.

**Code**
```rust
//! Cloudflare API: token check, account, workers.dev name, Worker upload. Tokens never in errors or Debug.

pub const CF_API: &str = "https://api.cloudflare.com/client/v4";
pub const CF_TOKENS_PAGE: &str = "https://dash.cloudflare.com/profile/api-tokens";
pub fn worker_page(account_id: &str, script: &str) -> String {
    format!("https://dash.cloudflare.com/{account_id}/workers/services/view/{script}/production")
}

#[derive(Clone)]
pub struct CfAuth(pub String);
impl std::fmt::Debug for CfAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("CfAuth([hidden])") }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfError { BadToken, NotAllowed, Network, Api(String) }
impl CfError {
    pub fn plain(&self) -> String {
        match self {
            Self::BadToken => "That token didn't work. Make a new one in Cloudflare and paste it again.".into(),
            Self::NotAllowed => "This token can't change Workers. Make it with the Edit Cloudflare Workers template.".into(),
            Self::Network => "Cloudflare didn't answer. Check your internet and try again.".into(),
            Self::Api(m) => format!("Cloudflare said: {m}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfAccount { pub id: String, pub name: String }

pub trait CloudflareApi: Send + Sync {
    fn verify_token(&self, auth: &CfAuth) -> Result<(), CfError>;
    fn first_account(&self, auth: &CfAuth) -> Result<CfAccount, CfError>;
    fn subdomain(&self, auth: &CfAuth, account_id: &str) -> Result<Option<String>, CfError>;
    fn create_subdomain(&self, auth: &CfAuth, account_id: &str, name: &str) -> Result<String, CfError>;
    /// `first` = true sends the migrations block of section B2; false sends the update form (NF-11).
    fn upload_script(&self, auth: &CfAuth, account_id: &str, script: &str, owner_id: &str, first: bool) -> Result<(), CfError>;
    fn enable_workers_dev(&self, auth: &CfAuth, account_id: &str, script: &str) -> Result<(), CfError>;
    fn delete_script(&self, auth: &CfAuth, account_id: &str, script: &str) -> Result<(), CfError>;
}

/// Pure: the metadata JSON of section B2. Never adds TEST_HOOKS.
pub fn metadata(owner_id: &str, first: bool) -> serde_json::Value;

/// Pure: a multipart/form-data body. Part "metadata" (application/json) first, then each module from
/// `cf_bundle_gen::MODULES` with content type: esm → "application/javascript+module", wasm → "application/wasm",
/// text → "text/plain".
pub fn multipart(boundary: &str, metadata: &serde_json::Value, modules: &[(&str, &str, &[u8])]) -> Vec<u8>;

pub fn random_name(prefix: &str, len: usize) -> String; // alphabet "abcdefghijkmnpqrstuvwxyz23456789", rand crate

pub struct HttpCloudflare { /* ureq agent, same timeouts as HttpRailway */ }
impl HttpCloudflare { pub fn new() -> Self; }
```
`deploy_cloudflare` (in `deploy.rs`):
```rust
pub struct CfDeps<'a> {
    pub cf: &'a dyn super::cloudflare::CloudflareApi,
    pub probe: &'a dyn RelayProbe,
    pub keys: &'a KeyStore,
    pub now: &'a dyn Fn() -> u64,
    pub sleep: &'a dyn Fn(u64),
    pub cancelled: &'a dyn Fn() -> bool,
    pub progress: &'a dyn Fn(DeployStep),
    pub save: &'a dyn Fn(&HostConfig) -> Result<(), String>,
}

/// Steps shown: CheckAccount, GetAddress, AddRelay, Check, Done.
pub fn deploy_cloudflare(d: &CfDeps, c: &mut HostConfig) -> Result<(), DeployError>;
```
Order inside: verify token → first account (save `accountId`, `accountName`) → subdomain or create (save) →
script name (save before upload) → `upload_script(first = true)` → `enable_workers_dev` → `url =
normalize_relay_url("https://{script}.{subdomain}.workers.dev")` → `wait_until_ready(with_owner = false)` → step `Done`.
The Cloudflare token stays (Cloudflare has no per-project token). `DeployError` gains the variant `Cf(CfError)`, whose
`plain()` returns `CfError::plain()`.

**Tests to write**
`cloudflare_tests.rs`:
- `metadata_first_upload_has_migrations` → `migrations.new_tag == "v2"`, two steps, `new_sqlite_classes` `["Team","IpGate"]` then `["Room","RoomIndex"]`.
- `metadata_has_owner_and_registration` → bindings include `PM_RELAY_OWNER_KEY` = id and `PM_RELAY_REGISTRATION` = `owner`.
- `metadata_never_has_test_hooks` → serialized JSON does not contain `TEST_HOOKS`.
- `multipart_puts_metadata_first` → body starts with `--<boundary>\r\nContent-Disposition: form-data; name="metadata"`.
- `multipart_types_by_kind` → the wasm part has `Content-Type: application/wasm`.
- `random_name_shape` → `random_name("pm-relay-", 6)` matches `^pm-relay-[abcdefghijkmnpqrstuvwxyz23456789]{6}$`.
- `deploy_cloudflare_happy_path` → FakeCloudflare: subdomain `None` then created `pm-abc12345`; calls == `["verify_token","first_account","subdomain","create_subdomain","upload_script","enable_workers_dev"]`; `c.url` matches `^https://pm-relay-[a-z2-9]{6}\.pm-abc12345\.workers\.dev$`.
- `deploy_cloudflare_bad_token` → `verify_token` → `Err(BadToken)`; `plain()` == `That token didn't work. Make a new one in Cloudflare and paste it again.`
- `debug_of_cf_auth_hides_token` → `format!("{:?}", CfAuth("cf_token_123456".into()))` has no `cf_token_123456`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r32-9'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::cloudflare_tests`
→ `test result: ok. 9 passed; 0 failed`.

**Done when**
- [ ] 9 tests pass, all `relay_host` tests pass.
- [ ] Choosing Cloudflare in the UI no longer shows `Cloudflare isn't ready yet.`

**Do NOT**
- Do not call Cloudflare in tests.
- Do not add a `TEST_HOOKS` binding.
- Do not reuse a script name that exists; always a new random one for a new relay.

**If it fails**
1. Cloudflare answers `10021` / migration error on a first upload → the metadata migration form differs from NF-11;
   fix `metadata()` only.
2. The relay answers on workers.dev only after a minute → the 3-minute `wait_until_ready` covers it; do not shorten it.
3. `first_account` returns none → the token has no account access; return `CfError::NotAllowed`.

---

## Open facts

- NEEDS-FACT NF-3: parameter types of `pm_relay_protocol::auth_header`, `verify_request` and `key_id`. Read
  `relay/protocol/src/lib.rs`.
- NEEDS-FACT NF-6: Railway public GraphQL schema: exact names and input fields of `projectCreate`, `serviceCreate`,
  `variableCollectionUpsert`, `serviceInstanceUpdate` (does it take `source.image`, `sleepApplication`,
  `numReplicas`, `healthcheckPath`, `restartPolicyType`), `volumeCreate`, `serviceDomainCreate` (`targetPort`),
  `serviceInstanceDeployV2`, `deployments`, `deploymentRestart`, `deploymentRedeploy`, `projectTokenCreate`,
  `projectDelete`; whether an image reference with both tag and `@sha256:` digest is accepted; and the
  `railway.json` keys. Read `https://docs.railway.com/reference/public-api` and run a schema introspection with a
  test token.
- NEEDS-FACT NF-7: whether a Railway volume mounted at `/data` is writable by a non-root image user (uid 10001), and
  whether the variable `RAILWAY_RUN_UID=0` is the supported fix. Read Railway's volumes docs.
- NEEDS-FACT NF-8: the source address range of Railway's proxy as seen by the container, and whether it sets
  `X-Forwarded-For`, so `PM_RELAY_TRUSTED_PROXY` can be set. Read Railway's networking docs or log the peer address in
  a test deploy.
- NEEDS-FACT NF-9: Railway's current price and trial terms for one small always-on service with a volume, and which
  mutations a project token may call (restart, redeploy, `serviceInstanceUpdate`, `projectDelete`). Read Railway's
  pricing and public API token docs.
- NEEDS-FACT NF-10: whether Railway offers OAuth for third-party apps (would replace token paste), and the exact labels
  on Railway's Tokens page. Read Railway's docs and the page at `https://railway.com/account/tokens`.
- NEEDS-FACT NF-11: the Cloudflare Workers script upload API for module workers with Durable Object SQLite migrations
  (metadata field names for the first upload and for later updates, `old_tag` rules), and whether Durable Objects with
  SQLite storage are on the free plan. Read Cloudflare's Workers script upload API docs.
- NEEDS-FACT NF-12: the file names `npx wrangler deploy --dry-run --outdir dist-bundle` writes for `relay/worker`
  (main module name, wasm name, whether viewer files are separate). Run it in `relay/worker`.
- NEEDS-FACT NF-13: which functions `relay/worker/src/wasm/pm_relay_wasm.d.ts` exports (is there a request signature
  check usable for the owner header). Read that file and `relay/worker/src/proto.ts`.
- NEEDS-FACT NF-14: which WebSocket crate `packages/app/src-tauri/src/team/transport/relay.rs` uses and its connect
  function. Read that file and `packages/app/src-tauri/Cargo.toml`.
- NEEDS-FACT NF-16: every place `RelayTransport::new` is called. Search `packages/app/src-tauri/src` for
  `RelayTransport::new`.
- NEEDS-FACT NF-17: exact signatures of the private `KeyStore::name` and `get_or_make`, and the profile text used when
  no profile is set. Read `packages/app/src-tauri/src/team/keystore.rs`.
- NEEDS-FACT NF-28: the `ureq` major version in `packages/app/src-tauri/Cargo.toml`.
- NEEDS-FACT NF-35: the exact Cloudflare dashboard labels for making a token from the "Edit Cloudflare Workers" template,
  and the permissions that template grants. Read Cloudflare's API token docs.
- NEEDS-FACT NF-36: the exact GitHub page path to make an organization container package public. Read GitHub's
  "Configuring a package's access control and visibility" docs.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Token in a file | Anyone reading app data gets the account | Tokens only via `KeyStore::set_relay_token`; `relay_host.json` has no token fields | `host_config_has_no_secret_fields`, `saved_config_never_has_the_token` |
| Token echoed in an API error | Token shown on screen, in a screenshot or bug report | `redact` on every GraphQL message; ureq error text dropped | `graphql_error_message_is_redacted`, `network_error_is_network` |
| Token in Debug output or panics | Token in a crash log | Manual `Debug` on `RailwayAuth` and `CfAuth` | `debug_of_auth_hides_token`, `debug_of_cf_auth_hides_token` |
| Account token left after setup | A thief can spend on any project | Swap to a project token after the relay is verified | `deploy_happy_path_makes_everything_in_order` |
| Wrong or hostile address saved | Team traffic goes to a stranger | Address comes from the provider reply, passes `normalize_relay_url`, and must show our owner key and pass owner status | `judge_owner_mismatch_is_wrong_relay`, `judge_owner_status_403_is_wrong_relay` |
| Moved image tag | Railway runs code we did not publish | Digest pin in `RELAY_IMAGE` | R35-6 `relay_image_is_pinned_by_digest` |
| Test hooks on a real relay | `/v1/_test` routes open on the internet | Never in `railway_vars` or Worker metadata | `railway_vars_are_exactly_these`, `metadata_never_has_test_hooks` |
| Strangers use the relay | Bill and storage | Owner-only registration on both providers | file 31 R31-5 tests, R32-7 `owner worker refuses unsigned registration` |
| Published image tampered on GHCR | Code change for every new relay | Manual publish, founder yes, digest recorded in the repo | R32-1 Run steps 4-6 |

## What cannot be verified without real hardware or a second computer

- Every Railway call against the real API, the real domain, TLS, and WebSocket through Railway's proxy (needs the
  founder's Railway account and billing).
- The project-token swap working for later restart and redeploy (NF-9).
- A real Cloudflare upload with Durable Object migrations and the workers.dev name (needs a Cloudflare account).
- The anonymous GHCR pull from Railway's builders.
- A second computer joining a team through the new relay with an invite link.
