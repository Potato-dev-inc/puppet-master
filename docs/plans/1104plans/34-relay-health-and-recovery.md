# 34 — Relay health and recovery

Status: plan only. Part of the one-click relay family (overview: `30-one-click-relay-overview.md`).

Sources the writer used: `00-FORMAT.md`, `_facts/relay-facts.md` ("relay facts §N"), `_facts/spaces-facts.md`
("spaces facts §N"). No source code was opened by the writer. NEEDS-FACT lines use the shared `NF-n` numbering and are
listed in `Open facts` near the end.

Covers: health checks, the plain "isn't answering" message with a Fix button (for the person who made the relay and for
teammates), restart and redeploy, SQLite backup and restore, updates, removing the relay, and what happens to members
when a team moves to a new relay.

---

## 1. What the person sees today and after

| Today (relay facts §6) | After |
|---|---|
| `The relay said 502` (the "other" arm of `relay_error`) | Team: `The relay isn't answering right now (error 502). It may be restarting.` plus, on the relay maker's computer, the notice `Your relay isn't answering, so your team isn't getting updates.` and **Fix it** |
| `relay WebSocket unavailable` (source: NF-15) | `Live updates are blocked on the way to the relay, so updates come every 20 seconds.` (status `polling`), plus **Fix it** on the maker's computer |
| Nothing tells the maker that the relay died | Health checks every 5 minutes; the Your relay row turns to `Your relay isn't answering` with the problem in plain words |
| No backup | Weekly automatic backup on Railway, and **Save a backup** |
| No update path | **Update relay** with automatic roll back |
| No way to remove | **Remove relay** with an optional backup first |
| A new relay address means every teammate must re-join | **Move my team here**: teammates follow an admin-signed "team moved" message |

## 2. Health checks

| Check | How often | What it calls | Problem it can find |
|---|---|---|---|
| Light | every 5 minutes while the relay is `ready` | `GET /v1/health`, `GET /v1/version` | `down`, `unreachable`, `wrong_relay`, `old_version`, `daily_limit` |
| Deep | every 30 minutes, at every retry, and on **Check now** | light + WebSocket probe + owner-signed `GET /v1/admin/status` (Railway) | also `websocket`, `full` |
| Retry | every 1 minute for the first 10 minutes after a failure, then back to every 5 minutes | deep | |
| On demand | when Team settings shows the team relay as offline or polling and the relay is ours | deep | |

The classifier is `verify::judge` (file 32 R32-4). Checks run in a background thread owned by the `RelayHost` (file 33
R33-2), never on the UI thread, and only while the Labs key is on.

## 3. Fix button: what it does

Pure function `fix::plan_fix(provider, problem, deployment_status, tried_restart) -> FixAction`. The table is the
function.

Railway:

| Problem | Latest deployment status | Action |
|---|---|---|
| `down`, `unreachable`, `websocket` | none found | Redeploy (`serviceInstanceDeployV2`) |
| same | `SUCCESS` and no restart tried yet | Restart (`deploymentRestart`) |
| same | `SUCCESS` and a restart was tried | Redeploy (`deploymentRedeploy`) |
| same | `FAILED`, `CRASHED`, `REMOVED`, `SKIPPED` | Redeploy |
| same | `SLEEPING` | Set `sleepApplication: false` again (`serviceInstanceUpdate`), then Redeploy |
| same | `QUEUED`, `WAITING`, `INITIALIZING`, `BUILDING`, `DEPLOYING`, `REMOVING`, other | Wait (poll up to 10 minutes) |
| `old_version` | any | Update (section 5) |
| `account` | any | Reconnect (open the Connect Railway sheet) |
| `wrong_relay` | any | Open the dashboard |
| `full` | any | Explain (no action) |
| `daily_limit` | any | Explain (no action) |

Cloudflare: `down`, `unreachable`, `websocket`, `old_version` → re-upload the Worker (`upload_script(first = false)`);
`account` → Reconnect; `wrong_relay` → Open the dashboard; `full`, `daily_limit` → Explain.

A Railway call that answers `BadToken` or `NotAllowed` during a fix turns the problem into `account`.

Fixing steps shown: `Checking what's wrong` · `Restarting the relay` (or `Starting the relay again`) ·
`Waiting for it to start` · `Checking that it works`.
Results: success notice `Fixed. Your relay is working again.`; failure text
`It still isn't working. Open Railway to see what's wrong, or remove it and make a new one.`

## 4. Backups

- Server: owner-signed `GET /v1/admin/backup` returns a consistent copy made with `VACUUM INTO` (R34-4).
- App: saves to `<app data>/relay-backups/relay-YYYY-MM-DD-HHMM.sqlite` (UTC), checks the SQLite header, keeps the
  newest 4 (DECISION-OPEN D-8, default weekly and keep 4).
- What is inside: team ids, device key ids, access lists, root-signed state blobs, sealed envelopes and sealed join
  boxes (relay facts §2 tables `teams`, `state`, `envelopes`, `joins`, `devices`). No message text: envelopes are sealed
  by the team key before they reach the relay (spaces facts §3 `secure.rs`).
- Cloudflare: no backups (no admin routes on the Worker). The row says `Backups aren't available on Cloudflare.`
- Restore: owner-signed `PUT /v1/admin/restore`, accepted only by a relay with no teams (R34-8). Used by
  **Move my team here** when a backup exists.

## 5. Updates

- The app carries `RELAY_VERSION` and `RELAY_IMAGE` (file 32 R32-5). When the relay's `/v1/version` is older,
  `update_available` is true and the row shows **Update relay**.
- Steps: `Saving a backup` · `Switching to the new version` · `Starting the relay` · `Checking that it works`.
- Railway: backup, remember `previous_image`, `serviceInstanceUpdate` with the new image, deploy, wait, check the version.
  On any failure after the switch: set the previous image, deploy, wait, and show
  `The update didn't work, so your relay is back on the old version.`
- Success notice: `Your relay is up to date.`
- The database moves forward with the relay's own migrations inside `Relay::open` (relay facts §2). Going back to an
  older image after a migration is not supported; the roll back is only for an update whose new version never reached
  "ready", and the backup taken first is the safety net.

## 6. Removing the relay

Remove sheet: title `Remove your relay?`; body `Your team stops getting updates until it uses another relay. Messages
still waiting on the relay are deleted. Railway stops charging for it.` (Cloudflare: the last sentence is left out);
when the team uses it, an extra line `Your team uses this relay right now.`; buttons `Save a backup, then remove`
(Railway only), `Remove`, `Cancel`. Working text: `Removing your relay`.

Railway: `projectDelete`. If the project token may not delete (NF-9), the app opens
`https://railway.com/project/<projectId>/settings` and shows
`Railway needs you to delete it on its website. We opened the page. Press I deleted it when it's gone.` with the button
`I deleted it`. That button runs a check: unreachable or 404 → removed; still answering →
`Your relay still answers. Delete the project on Railway first.`
Cloudflare: delete the Worker script.
After removal: both provider tokens are deleted from the credential store, `relay_host.json` is reset with
`removedUrl` set, the owner key is kept (it is reused by the next relay; R35-7 rotates it on request).

## 7. When the relay moves: what happens to members

Members save the relay address in `team.json` (`TeamConfig.relay`, relay facts §6) and learned it from the invite link.

**Move my team here** (shown when the person's relay is ready, the team uses another relay, and this member is a team
admin):
1. `Checking your new relay` — deep check of the new relay.
2. `Copying your team to it` — if a backup of the old relay exists and the new relay has no teams, restore it (messages
   up to 7 days old come back). Otherwise register the team, put the access list, and put the `roster` and `keys`
   state blobs on the new relay (the same calls team creation makes, NF-29). The owner key signs the registration
   (file 32 R32-6).
3. `Telling your team` — send a team event `relay_moved { url }` through the **old** relay, sealed and signed like every
   team event (spaces facts §3 `Session::seal`).
4. `Switching this computer` — set `TeamConfig.relay` to the new address and restart the team transport.

Members' apps, on a verified `relay_moved` event:
- ignore it unless the sender is a team admin (`Roster::is_admin`, spaces facts §3) — DECISION-OPEN D-15, default yes;
- ignore it unless the address passes `normalize_relay_url` (so `https://` only for other computers);
- check the new relay: signed `GET /v1/teams/{team}/state/roster` with this device's key must answer 200 (proves the team
  is there and this device is on its access list);
- then switch `TeamConfig.relay` and show `Your team moved to a new relay. Updates come from there now.`;
- if the check fails, try again every 10 minutes for 7 days and show
  `Your team's admin moved the team to a new relay, but this computer can't reach it yet. We'll keep trying.`

A member offline longer than 7 days misses the event (the old mailbox keeps envelopes for 7 days, `MAILBOX_TTL_MS`).
When they come back and the old relay is gone, Team settings shows the "isn't answering" notice with the link
`The relay moved? Paste the new address`; pasting it runs the same check as above (`team_relay_switch`).

Messages a member posts to the old relay after the admin switched are not copied to the new relay. The admin's app keeps
no second subscription to the old relay (DECISION-OPEN D-9 keeps the old relay up 7 days so late members still receive
the move event). After 7 days the maker's Your relay row shows
`Your team moved off {old address} on {date}. If you run that relay, you can turn it off now.` with `Dismiss`.

---

## Slices

### R34-1 — Plain words for 500/502/503/504 and blocked WebSocket

Size: Haiku. Deps: none.

**Goal**
The team transport never shows `The relay said 502` or `relay WebSocket unavailable`.

**Read first**
- `packages/app/src-tauri/src/team/transport/relay.rs` — `fn relay_error(code: u16, body: &str) -> String` (private) and
  the WebSocket fallback path (relay facts §6).
- Where the text `relay WebSocket unavailable` is made (NF-15):
  `rg -n "WebSocket unavailable" packages/app/src-tauri/src packages/app/src`.

**Files**
- edit `packages/app/src-tauri/src/team/transport/relay.rs`
- edit the file NF-15 finds, if it is not `relay.rs` (at most one more file)

**Steps**
1. In `relay_error`, add arms before the "other" arm (Code).
2. Replace the user-visible `relay WebSocket unavailable` text with the new constant `WS_BLOCKED` (Code). If the text is
   only a log line or an internal error that never reaches the UI, leave it and write that finding in the PR.
3. Add a test module at the end of `relay.rs`. Run, commit `team: plain words for relay outages`.

**Code**
```rust
pub const RELAY_DOWN: &str = "The relay isn't answering right now";
pub const WS_BLOCKED: &str = "Live updates are blocked on the way to the relay, so updates come every 20 seconds.";

// inside relay_error, before the final `_ =>` arm:
502 | 503 | 504 => format!("{RELAY_DOWN} (error {code}). It may be restarting."),
500 => "The relay had a problem (error 500). Try again in a minute.".to_string(),
```
If `relay_error` matches on a different shape (for example `if`/`else`), add the same two cases in the same style.

**Tests to write**
`#[cfg(test)] mod relay_error_tests { use super::*; … }`:
- `relay_error_502_is_plain` → `relay_error(502, "")` == `The relay isn't answering right now (error 502). It may be restarting.`
- `relay_error_503_and_504` → same pattern with 503 and 504.
- `relay_error_500_is_plain` → `The relay had a problem (error 500). Try again in a minute.`
- `relay_error_429_unchanged` → `The relay asks to slow down. Try again in a minute.` (relay facts §6).
- `relay_error_418_still_says_code` → starts with `The relay said 418`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-1'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_error_tests`
→ `test result: ok. 5 passed`. Then `--lib team` → `ok.`.

**Done when**
- [ ] 5 tests pass; team tests pass.
- [ ] `rg -n "The relay said 50" packages/app/src` finds no UI test expecting the old text (update any that do).

**Do NOT**
- Do not change the 401, 403, 404, 409, 413, 429, 507 texts.
- Do not change `DENIED` (relay facts §6, `team/transport/mod.rs`).

**If it fails**
1. A frontend test expects `The relay said 502` → update it to the new text.
2. `relay_error` is in another module → add the arms where the function is; keep the tests next to it.
3. The WebSocket text is built with `format!` → replace the whole format string with `WS_BLOCKED`.

---

### R34-2 — Health monitor and schedule

Size: Sonnet. Deps: R33-2.

**Goal**
A background monitor checks the person's relay on the schedule in section 2 and keeps `phase` and `problem` current.

**Read first**
- `relay_host/service.rs` (R33-2) — `RelayHost`, `HostDeps`, `check_now`, `changed`.
- `relay_host/verify.rs` (R32-4) — `check`, `Problem`.
- `relay_host/commands.rs` — where `HOST` is created.

**Files**
- create `packages/app/src-tauri/src/relay_host/monitor.rs`
- create `packages/app/src-tauri/src/relay_host/monitor_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/relay_host/mod.rs`

**Steps**
1. Write `monitor.rs` (Code).
2. In `service.rs`, add `labs: Arc<dyn Fn() -> bool + Send + Sync>` to `HostDeps`, a `schedule: Mutex<Schedule>` and a
   `wake: (Mutex<bool>, Condvar)` to `RelayHost`, and the methods `tick`, `wake`, `spawn_monitor` (Code).
3. In `commands.rs`, right after the host is created inside `get_or_init`, call `h.spawn_monitor()`.
4. Tests, run, commit `relay_host: health monitor`.

**Code**
`relay_host/monitor.rs`:
```rust
//! When to check the relay. Pure; the thread lives in service.rs.

pub const CHECK_EVERY_MS: u64 = 5 * 60_000;
pub const DEEP_EVERY_MS: u64 = 30 * 60_000;
pub const RETRY_EVERY_MS: u64 = 60_000;
pub const RETRY_WINDOW_MS: u64 = 10 * 60_000;
pub const TICK_MS: u64 = 15_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schedule {
    pub last_check_ms: u64,
    pub last_deep_ms: u64,
    pub failing_since_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due { Nothing, Light, Deep }

pub fn due(s: &Schedule, now_ms: u64) -> Due {
    let retrying = s.failing_since_ms.map_or(false, |since| now_ms.saturating_sub(since) < RETRY_WINDOW_MS);
    let every = if retrying { RETRY_EVERY_MS } else { CHECK_EVERY_MS };
    if s.last_check_ms != 0 && now_ms.saturating_sub(s.last_check_ms) < every { return Due::Nothing; }
    if s.failing_since_ms.is_some() || now_ms.saturating_sub(s.last_deep_ms) >= DEEP_EVERY_MS { Due::Deep } else { Due::Light }
}

pub fn after_check(s: &mut Schedule, deep: bool, ok: bool, now_ms: u64) {
    s.last_check_ms = now_ms;
    if deep { s.last_deep_ms = now_ms; }
    s.failing_since_ms = if ok { None } else { Some(s.failing_since_ms.unwrap_or(now_ms)) };
}
```
`service.rs` additions:
```rust
impl RelayHost {
    /// One monitor step. Safe to call from tests with a manual clock.
    pub fn tick(&self, now_ms: u64) {
        if !(self.deps.labs)() || self.busy.load(Ordering::SeqCst) { return; }
        let made = { let c = self.config(); !c.url.is_empty() && c.step == Some(DeployStep::Done) };
        if !made { return; }
        let due = monitor::due(&self.schedule.lock().unwrap(), now_ms);
        let deep = match due { monitor::Due::Nothing => return, monitor::Due::Light => false, monitor::Due::Deep => true };
        let before = self.view();
        let after = self.check_now(deep);
        monitor::after_check(&mut self.schedule.lock().unwrap(), deep, after.problem.is_none(), now_ms);
        if before.phase != after.phase || before.problem != after.problem { self.changed(); }
    }

    /// Wakes the monitor at once (used by the Team notice and by Check now).
    pub fn wake(&self) {
        let (lock, cv) = &self.wake;
        *lock.lock().unwrap() = true;
        cv.notify_one();
        self.schedule.lock().unwrap().last_check_ms = 0;
    }

    pub fn spawn_monitor(self: &Arc<Self>) {
        let me = Arc::clone(self);
        let _ = std::thread::Builder::new().name("relay-host-monitor".into()).spawn(move || loop {
            me.tick((me.deps.now)());
            let (lock, cv) = &me.wake;
            let woke = lock.lock().unwrap();
            let (mut woke, _) = cv.wait_timeout(woke, std::time::Duration::from_millis(monitor::TICK_MS)).unwrap();
            *woke = false;
        });
    }
}
```
`spawn_monitor` must run once: guard it with a `std::sync::Once` in `commands.rs` like `ATTACH` in `pairing_commands.rs`
(spaces facts §4).

**Tests to write**
`monitor_tests.rs`:
- `first_tick_is_deep` → `due(&Schedule::default(), 1_000)` → `Deep` (last_deep 0, so 30 minutes have "passed").
- `light_after_five_minutes` → after a deep check at t=0 (`after_check(deep=true, ok=true, 0)`), `due(t=5*60_000)` → `Light`; `due(t=4*60_000)` → `Nothing`.
- `deep_after_thirty_minutes` → `due(t=30*60_000)` with last deep at 0 and last check at 25 minutes → `Deep`.
- `retry_every_minute_for_ten_minutes` → failure at t=0 → `due(60_000)` → `Deep`; `due(30_000)` → `Nothing`.
- `back_to_five_minutes_after_window` → failing since 0, last check at 11 minutes → `due(12*60_000)` → `Nothing`; `due(16*60_000)` → `Deep`.
- `tick_marks_unhealthy_then_ready` → host made (config step Done), FakeProbe 502 → `tick(1_000)` → `phase == Unhealthy`, `problem == Some(Down)`; probe fixed → `tick(62_000)` → `phase == Ready`.
- `tick_does_nothing_when_labs_off` → `labs` false → probe never called.
- `tick_does_nothing_while_busy` → `busy` set → probe never called.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-2'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::monitor_tests`
→ `test result: ok. 8 passed`.

**Done when**
- [ ] 8 tests pass; all `relay_host` tests pass.
- [ ] Only one monitor thread starts per app run (Once guard).

**Do NOT**
- Do not check more often than the schedule.
- Do not write `relay_host.json` on every check (R33-2 saves `last_ok_ms` at most once per hour).
- Do not show an OS notification.

**If it fails**
1. Deadlock between `schedule` and `live` → never hold `schedule` while calling `check_now`; the Code takes and drops it.
2. Test thread hangs → tests call `tick` directly; never call `spawn_monitor` in tests.
3. `wait_timeout` poisoned → use `unwrap_or_else(|e| e.into_inner())` on both locks.

---

### R34-3 — Fix and restart (Rust)

Size: Sonnet. Deps: R34-1, R34-2.

**Goal**
`relay_host_fix` and `relay_host_restart` follow the table in section 3 and end in `ready` or a plain error.

**Read first**
- Section 3 of this file.
- `relay_host/railway.rs` — `restart`, `redeploy`, `deploy`, `latest_deployment`, `configure_service` (R32-3).
- `relay_host/deploy.rs` — `START_TIMEOUT_MS`, `READY_TIMEOUT_MS`, polling loop (R32-5).
- `relay_host/service.rs` and `commands.rs`.

**Files**
- create `packages/app/src-tauri/src/relay_host/fix.rs`
- create `packages/app/src-tauri/src/relay_host/fix_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

**Steps**
1. Write `fix.rs` (Code).
2. In `service.rs`: add `FixStep` to `Live` and `fix_step: Option<FixStep>` and `notice: Option<String>` to
   `RelayHostView`; add `pub fn fix(self: &Arc<Self>) -> Result<RelayHostView, String>` and
   `pub fn restart(self: &Arc<Self>) -> Result<RelayHostView, String>`, both run with `run_job(Phase::Fixing, …)`.
3. In `commands.rs`: `relay_host_fix` and `relay_host_restart` (Code). When the outcome is `OpenDashboard`, the command
   opens the dashboard URL.
4. Register both in `lib.rs` after `relay_host::commands::relay_host_open_token_page,`.
5. Tests, run, commit `relay_host: fix and restart`.

**Code**
```rust
//! What Fix does, decided by a pure table (file 34 section 3), and the runner that does it.

use super::config::Provider;
use super::railway::DeployStatus;
use super::verify::Problem;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixAction { Wait, Restart, Redeploy, WakeAndRedeploy, Reupload, Update, Reconnect, OpenDashboard, Explain }

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FixStep { Diagnose, Restart, Start, Check }

pub const FIXED: &str = "Fixed. Your relay is working again.";
pub const STILL_BROKEN: &str = "It still isn't working. Open Railway to see what's wrong, or remove it and make a new one.";

pub fn plan_fix(provider: Provider, problem: Problem, deployment: Option<&DeployStatus>, tried_restart: bool) -> FixAction {
    use FixAction::*;
    use Problem::*;
    match (provider, problem) {
        (_, Account) => Reconnect,
        (_, WrongRelay) => OpenDashboard,
        (_, Full) | (_, DailyLimit) => Explain,
        (Provider::Cloudflare, _) => Reupload,
        (Provider::Railway, OldVersion) => Update,
        (Provider::Railway, Down | Unreachable | Websocket) => match deployment {
            None => Redeploy,
            Some(DeployStatus::Success) if !tried_restart => Restart,
            Some(DeployStatus::Success) => Redeploy,
            Some(DeployStatus::Failed | DeployStatus::Crashed | DeployStatus::Removed | DeployStatus::Skipped) => Redeploy,
            Some(DeployStatus::Sleeping) => WakeAndRedeploy,
            Some(_) => Wait,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixOutcome { Fixed, StillBroken(Problem), Explained(Problem), OpenDashboard(String), Reconnect, NeedsUpdate }
```
`RelayHost::fix` body, in order: `Diagnose` (deep check; no problem → `Fixed`); read the latest deployment (Railway);
`plan_fix`; for `Restart` call `restart(deployment_id)`, for `Redeploy` call `redeploy(deployment_id)` when an id exists
else `deploy(service, env)`, for `WakeAndRedeploy` call `configure_service` then the same as Redeploy, for `Reupload`
call `upload_script(first = false)`; `Start` polls `latest_deployment` every 5 s up to 10 minutes; `Check` runs
`wait_until_ready` up to 3 minutes; if still broken after a `Restart`, run the table once more with
`tried_restart = true`. Set `live.notice = Some(FIXED)` on success, `live.error = Some(STILL_BROKEN)` on failure. Any
`ApiError::BadToken` or `NotAllowed` → `live.problem = Some(Problem::Account)` and outcome `Reconnect`.

Commands:
```rust
#[tauri::command]
pub async fn relay_host_fix(app: AppHandle) -> Result<Value, String> {
    let h = host(&app)?;
    let (view, open) = tauri::async_runtime::spawn_blocking(move || h.fix()).await.map_err(|e| e.to_string())??;
    if let Some(url) = open { let _ = crate::app_lifecycle::open_external_url(url); }
    to_value(&view)
}

#[tauri::command]
pub async fn relay_host_restart(app: AppHandle) -> Result<Value, String> {
    let h = host(&app)?;
    to_value(&tauri::async_runtime::spawn_blocking(move || h.restart()).await.map_err(|e| e.to_string())??)
}
```
(`fix()` returns `Result<(RelayHostView, Option<String>), String>`; the `Option<String>` is the dashboard URL to open.)

**Tests to write**
`fix_tests.rs`:
- `plan_table_railway` → one assert per row of the Railway table in section 3 (11 asserts in one test).
- `plan_table_cloudflare` → `Down` → `Reupload`; `WrongRelay` → `OpenDashboard`; `DailyLimit` → `Explain`; `Account` → `Reconnect`.
- `fix_restarts_a_running_but_broken_relay` → FakeProbe 502 then good after restart; FakeRailway latest `Success` → calls include `restart`, not `redeploy`; `view.notice == Some(FIXED)`; `phase == Ready`.
- `fix_redeploys_a_crashed_relay` → latest `Crashed` → calls include `redeploy`.
- `fix_tries_redeploy_after_restart_fails` → probe stays bad after restart, good after redeploy → calls `restart` then `redeploy`; `phase == Ready`.
- `fix_gives_up_with_plain_words` → probe always 502 → `phase == Unhealthy`, `error == Some(STILL_BROKEN)`.
- `fix_with_revoked_token_asks_to_reconnect` → FakeRailway `latest_deployment` → `BadToken` → `problem == Some(Account)`.
- `fix_wrong_relay_opens_dashboard` → probe owner mismatch → returned URL `https://railway.com/project/p1`.
- `fix_when_healthy_says_fixed` → probe good → no Railway calls other than `latest_deployment` is not even called; notice `FIXED`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-3'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::fix_tests`
→ `test result: ok. 9 passed`.

**Done when**
- [ ] 9 tests pass; the two commands are registered.

**Do NOT**
- Do not delete or recreate the volume in a fix.
- Do not create a new project in a fix.
- Do not loop more than one restart and one redeploy per press.

**If it fails**
1. `deploymentRestart` is not allowed with a project token (NF-9) → the call gives `NotAllowed`; for restart only,
   fall back to `Redeploy` before turning the problem into `account`.
2. The fix leaves `phase_override` set after an error → clear it in a `finally`-style guard struct in `run_job`.
3. `plan_table_railway` and the section 3 table disagree → the table in section 3 is the spec; fix the code.

---

### R34-4 — Owner-only backup endpoint (server)

Size: Sonnet. Deps: R31-5.

**Goal**
`GET /v1/admin/backup` streams a consistent SQLite copy to the owner, and only to the owner.

**Read first**
- `relay/server/src/owner.rs` (R31-5) — `check_owner`, `admin_status`.
- `relay/server/src/lib.rs` — `Relay::open` (line 326) and how the connection is held (NF-5).
- `relay/server/Cargo.toml` — axum version; whether `tokio-util` is present (NF-4).

**Files**
- edit `relay/server/src/owner.rs`
- edit `relay/server/src/lib.rs`
- edit `relay/server/Cargo.toml`
- create `relay/server/tests/backup.rs`
- edit `relay/README.md`

**Steps**
1. Add `tokio-util = { version = "0.7", features = ["io"] }` to `[dependencies]` if missing.
2. In `lib.rs`: `Relay::backup_to(&self, path: &Path) -> Result<(), ()>` and `Relay::clean_backups(dir: &Path)`
   (Code); call `clean_backups` at the end of `Relay::open` when the database is a file.
3. In `owner.rs`: `admin_backup` handler (Code) and a `static BACKUP_RUNNING: AtomicBool`.
4. Route `.route("/v1/admin/backup", get(owner::admin_backup))`.
5. README row. Tests, run, commit `relay: owner backup endpoint`.

**Code**
```rust
// lib.rs
impl Relay {
    /// A consistent copy of the whole database at `path` (VACUUM INTO). `path` must not exist.
    pub fn backup_to(&self, path: &std::path::Path) -> Result<(), ()> {
        let conn = self.conn.lock().map_err(|_| ())?;
        let p = path.to_str().ok_or(())?;
        conn.execute("VACUUM INTO ?1", [p]).map(|_| ()).map_err(|_| ())
    }

    /// Deletes leftover `backup-*.sqlite` files in the data folder.
    pub fn clean_backups(dir: &std::path::Path) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("backup-") && name.ends_with(".sqlite") { let _ = std::fs::remove_file(e.path()); }
            }
        }
    }
}
```
```rust
// owner.rs
static BACKUP_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub async fn admin_backup(State(state): State<crate::SharedState>, headers: HeaderMap, uri: Uri) -> Response {
    use std::sync::atomic::Ordering;
    let config = state.config();
    let Some(key) = config.owner_key.as_deref() else { return (StatusCode::NOT_FOUND, "not found").into_response() };
    let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/v1/admin/backup");
    match check_owner(key, &headers, "GET", pq, b"", crate::now_ms(), state.seen()) {
        OwnerCheck::Ok => {}
        OwnerCheck::Missing => return (StatusCode::UNAUTHORIZED, "signed by the owner required").into_response(),
        OwnerCheck::Replayed => return (StatusCode::UNAUTHORIZED, "request replayed").into_response(),
        OwnerCheck::Bad => return (StatusCode::FORBIDDEN, "not signed by the relay's owner").into_response(),
    }
    let Some(db) = config.db.as_ref() else {
        return (StatusCode::CONFLICT, "this relay keeps nothing on disk").into_response();
    };
    if BACKUP_RUNNING.swap(true, Ordering::SeqCst) {
        return (StatusCode::TOO_MANY_REQUESTS, "a backup is already running").into_response();
    }
    let dir = std::path::Path::new(db).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new(".")).to_path_buf();
    crate::Relay::clean_backups(&dir);
    let path = dir.join(format!("backup-{}.sqlite", crate::now_ms()));
    let made = state.relay().backup_to(&path);
    BACKUP_RUNNING.store(false, Ordering::SeqCst);
    if made.is_err() {
        let _ = std::fs::remove_file(&path);
        return (StatusCode::INSUFFICIENT_STORAGE, "not enough room on the relay's disk for a backup").into_response();
    }
    let Ok(file) = tokio::fs::File::open(&path).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "the backup could not be read").into_response();
    };
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/vnd.sqlite3"),
            (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"relay-backup.sqlite\""),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
    // The file is removed by clean_backups on the next backup or the next start.
}
```
(`config.db` is `Option<…>` per relay facts §2; use its real type, NF-1.)

**Tests to write**
`tests/backup.rs` (owner relay with a file database in a temp dir):
- `backup_needs_owner` → no header → 401 `signed by the owner required`; other key → 403.
- `backup_is_404_without_owner_key` → 404 `not found`.
- `backup_returns_a_sqlite_file` → 200; `content-type` `application/vnd.sqlite3`; first 16 bytes == `b"SQLite format 3\0"`.
- `backup_holds_the_registered_team` → register a team, back up, open the bytes (write to a temp file) with `rusqlite` → `SELECT COUNT(*) FROM teams` == 1.
- `backup_on_memory_relay_is_409` → memory relay → 409 `this relay keeps nothing on disk`.
- `old_backup_files_are_cleaned_at_start` → put `backup-1.sqlite` in the data folder, start the relay → the file is gone.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-4'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test backup`
→ `test result: ok. 6 passed`. Then the whole relay workspace → `ok.`.

**Done when**
- [ ] 6 tests pass.
- [ ] The backup reply has `cache-control: no-store`.

**Do NOT**
- Do not log the backup path or size.
- Do not hold the database lock while streaming (only during `VACUUM INTO`).
- Do not offer backups without an owner key.

**If it fails**
1. `VACUUM INTO` fails with "cannot VACUUM from within a transaction" → another statement left a transaction open on
   the shared connection; run the backup on a second connection opened read-only to the same file.
2. `Body::from_stream` missing → axum 0.6; use `axum::body::StreamBody::new(stream)`.
3. Windows test cannot delete the file while open → the cleanup only runs on the next call; tests check cleanup at start
   with a file the relay never opened.

---

### R34-5 — Backups in the app

Size: Haiku. Deps: R34-4, R34-2.

**Goal**
**Save a backup** and a weekly automatic backup save the relay's database to app data, keeping the newest 4.

**Read first**
- `relay_host/verify.rs` — `HttpProbe`, owner header building (R32-4).
- `relay_host/service.rs` — `tick` (R34-2).
- `packages/app/src-tauri/src/app_paths.rs` — `app_data_dir()` (spaces facts §9).

**Files**
- create `packages/app/src-tauri/src/relay_host/backup.rs`
- create `packages/app/src-tauri/src/relay_host/backup_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

**Steps**
1. In `backup.rs`, write the network part as one free function
   `pub fn fetch_backup(base: &str, owner: &SigningKey, now_ms: u64, out: &mut dyn std::io::Write) -> Result<u64, ()>`:
   GET `{base}/v1/admin/backup` with header `x-pm-owner` = `auth_header(owner, "GET", "/v1/admin/backup", now_ms, b"")`,
   status 200 only, copy the body into `out` with `std::io::copy`, return the byte count. It uses its own `ureq::Agent`
   (10 s connect, 10 minutes overall). `verify.rs` is not changed. The service passes
   `|out| fetch_backup(&url, &owner, now, out)` as the `fetch` closure; tests pass their own closure.
2. Write the rest of `backup.rs` (Code).
3. In `service.rs`: `pub fn backup(&self) -> Result<RelayHostView, String>`; in `tick`, after a good check, if the
   provider is Railway and `now - config.last_backup_ms >= BACKUP_EVERY_MS`, run the backup (errors only set
   `live.notice = Some("The weekly backup didn't work. Press Save a backup to try again.")`).
4. Command `relay_host_backup(app: AppHandle) -> Result<Value, String>`; register it.
5. Tests, run, commit `relay_host: backups`.

**Code**
```rust
//! Relay backups in app data: relay-backups/relay-YYYY-MM-DD-HHMM.sqlite (UTC), newest 4 kept.

use std::path::{Path, PathBuf};

pub const KEEP: usize = 4;
pub const BACKUP_EVERY_MS: u64 = 7 * 24 * 60 * 60_000;
pub const SAVED: &str = "Backup saved.";
pub const NO_BACKUP: &str = "Couldn't save a backup: the relay didn't send one.";
pub const NO_DISK: &str = "Couldn't save a backup on this computer.";
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

pub fn backups_dir(app_dir: &Path) -> PathBuf { app_dir.join("relay-backups") }

/// (year, month, day, hour, minute) in UTC, without a date crate.
pub fn utc_parts(ms: u64) -> (i64, u32, u32, u32, u32) {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32)
}

pub fn file_name(now_ms: u64) -> String {
    let (y, m, d, h, min) = utc_parts(now_ms);
    format!("relay-{y:04}-{m:02}-{d:02}-{h:02}{min:02}.sqlite")
}

/// Downloads to a .part file, checks the SQLite header, renames, prunes. Returns the final path.
pub fn backup_now(
    fetch: &dyn Fn(&mut dyn std::io::Write) -> Result<u64, ()>,
    app_dir: &Path,
    now_ms: u64,
) -> Result<PathBuf, String>;

/// Deletes the oldest `relay-*.sqlite` files beyond `keep`. Returns how many were deleted.
pub fn prune(dir: &Path, keep: usize) -> Result<usize, String>;
```
`backup_now` body: create `backups_dir`; open `<name>.part`; call `fetch`; on `Err` delete the part and return
`NO_BACKUP`; read the first 16 bytes; if not `SQLITE_MAGIC` delete and return `NO_BACKUP`; rename to `<name>`; `prune`;
any file-system error returns `NO_DISK`.

**Tests to write**
`backup_tests.rs`:
- `utc_parts_known_dates` → `utc_parts(0)` == `(1970,1,1,0,0)`; `utc_parts(1_793_580_994_000)` == the date `2026-11-01` at the right hour (compute the expected value once with an online epoch converter and write it in the test).
- `file_name_shape` → matches `^relay-\d{4}-\d{2}-\d{2}-\d{4}\.sqlite$`.
- `backup_now_writes_a_checked_file` → fetch writes `SQLITE_MAGIC` + 100 bytes → file exists, size 116.
- `backup_now_refuses_non_sqlite` → fetch writes `hello` → `Err(NO_BACKUP)`; no file left (also no `.part`).
- `prune_keeps_newest_four` → six files named for six dates → after prune, the four newest remain.
- `service_backup_sets_last_backup` → `HostDeps` gets a test `fetch` closure (new field `backup_fetch: Arc<dyn Fn(&str, &SigningKey, u64, &mut dyn std::io::Write) -> Result<u64, ()> + Send + Sync>`, real value `fetch_backup`) that writes a good body → `view().last_backup_ms == Some(now)`.
- `weekly_backup_runs_once_a_week` → `tick` at t0 (no backup before) → one backup; `tick` at t0+1 day → no new backup; at t0+7 days → second backup (count calls of the test `backup_fetch`).
- `no_backup_on_cloudflare` → provider Cloudflare → `backup()` → `Err("Backups aren't available on Cloudflare.")`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-5'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::backup_tests`
→ `test result: ok. 8 passed`.

**Done when**
- [ ] 8 tests pass; `relay_host_backup` registered.

**Do NOT**
- Do not save backups outside app data.
- Do not keep more than 4.
- Do not delete files that do not match `relay-*.sqlite`.

**If it fails**
1. Rename fails on Windows because the file is open → close the writer before renaming (drop it in an inner scope).
2. `prune` sorts wrongly → sort by file name; the name is a UTC timestamp, so name order is time order.
3. Weekly test runs the backup twice → `last_backup_ms` must be saved to `relay_host.json` before `tick` returns.

---

### R34-6 — Update the relay, with roll back

Size: Sonnet. Deps: R34-5.

**Goal**
**Update relay** moves the relay to `RELAY_IMAGE` and goes back to the previous image if the new one never becomes ready.

**Read first**
- Section 5 of this file.
- `relay_host/deploy.rs` (R32-5), `relay_host/fix.rs` (R34-3), `relay_host/backup.rs` (R34-5).

**Files**
- create `packages/app/src-tauri/src/relay_host/update.rs`
- create `packages/app/src-tauri/src/relay_host/update_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

**Steps**
1. Write `update.rs` (Code).
2. `service.rs`: `pub fn update(self: &Arc<Self>) -> Result<RelayHostView, String>` with `run_job(Phase::Fixing, …)`,
   `UpdateStep` in the view field `update_step`.
3. Command `relay_host_update`; register it.
4. Tests, run, commit `relay_host: update with roll back`.

**Code**
```rust
//! Update the relay image; roll back if the new one never becomes ready.

use super::config::HostConfig;
use super::deploy::{DeployDeps, DeployError};
use serde::Serialize;

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStep { Backup, Switch, Start, Check }

pub const UPDATED: &str = "Your relay is up to date.";
pub const ROLLED_BACK: &str = "The update didn't work, so your relay is back on the old version.";
pub const BACKUP_FIRST_FAILED: &str = "We couldn't save a backup, so we didn't update. Try again later.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome { Updated, RolledBack, NotStarted(String) }

/// Railway: backup → previous_image = image → configure_service(new) → deploy → wait SUCCESS → wait_until_ready
/// with version == RELAY_VERSION. On failure after the switch: configure_service(previous) → deploy → wait → RolledBack.
pub fn update_railway(
    d: &DeployDeps,
    c: &mut HostConfig,
    backup: &dyn Fn() -> Result<(), String>,
    step: &dyn Fn(UpdateStep),
) -> Result<UpdateOutcome, DeployError>;

/// Cloudflare: upload_script(first = false) with the embedded bundle, then wait_until_ready.
pub fn update_cloudflare(d: &super::deploy::CfDeps, c: &mut HostConfig, step: &dyn Fn(UpdateStep)) -> Result<UpdateOutcome, DeployError>;
```
The "ready" check after an update also requires `RelayVersion.version == image::RELAY_VERSION`; a relay that answers
with the old version counts as not updated.

**Tests to write**
- `update_happy_path` → FakeRailway statuses `[Deploying, Success]`, FakeProbe version `RELAY_VERSION` → `Ok(Updated)`; calls in order `configure_service`, `deploy`, `latest_deployment…`; `c.image == RELAY_IMAGE`; `c.previous_image` == the old image.
- `update_rolls_back_on_crash` → statuses `[Crashed]` after the switch, then `[Success]` after the roll back → `Ok(RolledBack)`; the last `configure_service` used the old image.
- `update_rolls_back_when_version_stays_old` → Success but version `0.1.0` → `Ok(RolledBack)`.
- `update_does_not_start_without_backup` → backup closure returns `Err` → `Ok(NotStarted(BACKUP_FIRST_FAILED))`; no `configure_service` call.
- `update_steps_in_order` → recorded steps `[Backup, Switch, Start, Check]`.
- `update_cloudflare_reuploads` → FakeCloudflare records `upload_script` with `first == false`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-6'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::update_tests`
→ `test result: ok. 6 passed`.

**Done when**
- [ ] 6 tests pass; `relay_host_update` registered.

**Do NOT**
- Do not update without a backup on Railway.
- Do not roll back after the new version was reported ready (the database may have moved forward).
- Do not update Cloudflare with `first = true` (it would try to create the Durable Object classes again).

**If it fails**
1. Roll back starts after a good update → the success check must stop the function before the roll back branch.
2. Cloudflare re-upload errors on migrations → the update form of the metadata is NF-11; fix `metadata(first = false)`.
3. `previous_image` empty for relays made before this slice → fill it from `c.image` at the start of the update.

---

### R34-7 — Remove the relay

Size: Haiku. Deps: R34-5.

**Goal**
**Remove relay** deletes the provider project or Worker (after an optional backup), forgets the tokens, and leaves the row
in `removed`.

**Read first**
- Section 6 of this file.
- `relay_host/railway.rs` — `delete_project`, `project_settings_page` (R32-3).
- `relay_host/cloudflare.rs` — `delete_script` (R32-9).

**Files**
- create `packages/app/src-tauri/src/relay_host/remove.rs`
- create `packages/app/src-tauri/src/relay_host/remove_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`

**Steps**
1. Write `remove.rs` (Code).
2. `service.rs`: `pub fn remove(self: &Arc<Self>, backup_first: bool) -> Result<(RelayHostView, Option<String>), String>`
   and `pub fn confirm_removed(&self) -> Result<RelayHostView, String>`; `RelayHostView` gains
   `waiting_for_web_delete: bool`.
3. Commands (Code); register both.
4. Tests, run, commit `relay_host: remove`.

**Code**
```rust
//! Removing the relay.

pub const WEB_DELETE: &str = "Railway needs you to delete it on its website. We opened the page. Press I deleted it when it's gone.";
pub const STILL_THERE: &str = "Your relay still answers. Delete the project on Railway first.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome { Removed, DeleteOnWebsite(String) }

/// Railway: delete_project with the stored token. NotAllowed → DeleteOnWebsite(project settings URL).
/// Cloudflare: delete_script. Then forget tokens and reset the config (keep removed_url).
pub fn remove(
    railway: &dyn super::railway::RailwayApi,
    cloudflare: &dyn super::cloudflare::CloudflareApi,
    keys: &crate::team::keystore::KeyStore,
    c: &mut super::config::HostConfig,
) -> Result<RemoveOutcome, String>;

/// After a website delete: unreachable or 404 health → removed; anything else → Err(STILL_THERE).
pub fn confirm_gone(probe: &dyn super::verify::RelayProbe, url: &str) -> Result<(), String>;

/// Reset after removal.
pub fn forget(keys: &crate::team::keystore::KeyStore, c: &mut super::config::HostConfig) {
    use crate::team::keystore::{RELAY_CLOUDFLARE_TOKEN, RELAY_RAILWAY_PROJECT_TOKEN, RELAY_RAILWAY_TOKEN};
    for n in [RELAY_RAILWAY_TOKEN, RELAY_RAILWAY_PROJECT_TOKEN, RELAY_CLOUDFLARE_TOKEN] { let _ = keys.forget_relay_token(n); }
    let removed_url = c.url.clone();
    *c = super::config::HostConfig { removed_url, ..Default::default() };
}
```
Commands:
```rust
#[tauri::command]
pub async fn relay_host_remove(app: AppHandle, backup_first: bool) -> Result<Value, String>  // opens the URL in DeleteOnWebsite
#[tauri::command]
pub async fn relay_host_confirm_removed(app: AppHandle) -> Result<Value, String>
```
"Remove what was made" on a failed deploy calls the same `relay_host_remove(false)`.

**Tests to write**
- `remove_railway_deletes_and_forgets` → FakeRailway records `delete_project("p1")`; tokens all gone; `c.url` empty; `c.removed_url` == old url; view `phase == Removed`.
- `remove_with_backup_first_backs_up` → `backup_first: true` → the test `backup_fetch` closure (R34-5) is called before `delete_project`.
- `remove_backup_failure_stops` → backup fails → `Err(NO_BACKUP)`, no delete call.
- `remove_not_allowed_asks_for_website` → `delete_project` → `NotAllowed` → `DeleteOnWebsite("https://railway.com/project/p1/settings")`; tokens still present; `waiting_for_web_delete == true`.
- `confirm_removed_when_gone` → FakeProbe health `Unreachable` → phase `Removed`.
- `confirm_removed_when_still_there` → health ok → `Err(STILL_THERE)`.
- `remove_cloudflare_deletes_script` → FakeCloudflare records `delete_script(acct, script)`.
- `owner_key_kept_after_remove` → `relay_owner()` before == after.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-7'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::remove_tests`
→ `test result: ok. 8 passed`.

**Done when**
- [ ] 8 tests pass; both commands registered.

**Do NOT**
- Do not remove without the person's press in the sheet (the command is only called from the sheet, R34-12).
- Do not delete backups on remove.
- Do not change `team.json`; the team keeps its relay address until it moves.

**If it fails**
1. `delete_project` succeeds on Railway but the probe still answers for a minute → `remove` does not probe; only
   `confirm_removed` probes.
2. Tokens survive → `forget` ran before an early return; call it only after a successful delete, and in
   `confirm_removed` after a good check.
3. Cloudflare delete says "script has bound Durable Objects" → add `?force=true` (B2) to the delete URL.

---

### R34-8 — Owner-only restore endpoint (server)

Size: Sonnet. Deps: R34-4, R35-1.

**Goal**
`PUT /v1/admin/restore` loads a backup into a relay that has no teams yet.

**Read first**
- `relay/server/src/owner.rs` — `admin_backup` (R34-4).
- `relay/server/src/lib.rs` — table definitions in `Relay::open`, including the M1 column from R35-1 (NF-22).

**Files**
- edit `relay/server/src/owner.rs`
- edit `relay/server/src/lib.rs`
- create `relay/server/tests/restore.rs`
- edit `relay/README.md`

**Steps**
1. `Relay::restore_from(&self, path: &Path) -> Result<RestoreCounts, RestoreError>` (Code).
2. `admin_restore` handler: owner check over the body; body limit = `config.max_db_bytes`; write to
   `<data dir>/restore-<ms>.sqlite`; call `restore_from`; delete the file; reply.
3. Route `.route("/v1/admin/restore", put(owner::admin_restore).layer(DefaultBodyLimit::max(<max_db_bytes as usize>)))`.
4. Tests, README, run, commit `relay: owner restore endpoint`.

**Code**
```rust
pub const TABLES: [&str; 5] = ["teams", "devices", "state", "joins", "envelopes"];

#[derive(Debug, serde::Serialize)]
pub struct RestoreCounts { pub teams: u64, pub envelopes: u64 }

#[derive(Debug, PartialEq, Eq)]
pub enum RestoreError { NotEmpty, BadFile, OtherVersion, Busy }

impl Relay {
    pub fn restore_from(&self, path: &std::path::Path) -> Result<RestoreCounts, RestoreError> {
        let conn = self.conn.lock().map_err(|_| RestoreError::Busy)?;
        let teams: i64 = conn.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0)).map_err(|_| RestoreError::Busy)?;
        if teams > 0 { return Err(RestoreError::NotEmpty); }
        let p = path.to_str().ok_or(RestoreError::BadFile)?;
        conn.execute("ATTACH DATABASE ?1 AS src", [p]).map_err(|_| RestoreError::BadFile)?;
        let result = (|| {
            let ok: String = conn.query_row("PRAGMA src.integrity_check", [], |r| r.get(0)).map_err(|_| RestoreError::BadFile)?;
            if ok != "ok" { return Err(RestoreError::BadFile); }
            for t in TABLES {
                let main_sql: Option<String> = conn.query_row("SELECT sql FROM main.sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0)).ok();
                let src_sql: Option<String> = conn.query_row("SELECT sql FROM src.sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0)).ok();
                if main_sql.is_none() || main_sql != src_sql { return Err(RestoreError::OtherVersion); }
            }
            conn.execute_batch("BEGIN IMMEDIATE").map_err(|_| RestoreError::Busy)?;
            for t in TABLES {
                if conn.execute(&format!("INSERT INTO main.{t} SELECT * FROM src.{t}"), []).is_err() {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(RestoreError::BadFile);
                }
            }
            conn.execute_batch("COMMIT").map_err(|_| RestoreError::Busy)?;
            let teams: i64 = conn.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0)).unwrap_or(0);
            let envelopes: i64 = conn.query_row("SELECT COUNT(*) FROM envelopes", [], |r| r.get(0)).unwrap_or(0);
            Ok(RestoreCounts { teams: teams as u64, envelopes: envelopes as u64 })
        })();
        let _ = conn.execute("DETACH DATABASE src", []);
        result
    }
}
```
Replies: 200 `{"teams":N,"envelopes":M}`; `NotEmpty` → 409 `this relay already has teams`; `BadFile` → 400
`that isn't a relay backup`; `OtherVersion` → 409 `the backup is from a different relay version`; `Busy` → 503
`the relay is busy, try again shortly`; owner failures as in `admin_status`; memory relay → 409
`this relay keeps nothing on disk`.

**Tests to write**
`tests/restore.rs`:
- `restore_into_empty_relay_brings_back_teams` → relay A: register a team and post one envelope; back up A; relay B (empty, same owner) → restore → 200 `{"teams":1,"envelopes":1}`.
- `restored_device_can_read_its_mailbox` → after the restore, a signed `GET /v1/teams/{team}/envelopes?after=0` on B by the team's device → 200 with 1 item.
- `restore_refused_when_teams_exist` → B already has a team → 409 `this relay already has teams`.
- `restore_refused_for_non_sqlite` → body `hello` → 400 `that isn't a relay backup`.
- `restore_needs_owner` → no header → 401.
- `restore_refused_for_other_schema` → a SQLite file with a `teams` table of different columns → 409 `the backup is from a different relay version`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-8'; cargo test --manifest-path relay/Cargo.toml -p pm-relay --test restore`
→ `test result: ok. 6 passed`.

**Done when**
- [ ] 6 tests pass; the restore temp file is deleted after every call (test checks the data folder).

**Do NOT**
- Do not restore into a relay that has any team.
- Do not replace the database file under a running connection.
- Do not skip the integrity check.

**If it fails**
1. `ATTACH` fails inside a transaction → make sure no `BEGIN` is open on the shared connection before `ATTACH`.
2. `INSERT … SELECT *` column order differs → the schema comparison should have caught it; if it passes but inserts
   fail, compare `PRAGMA table_info` column lists instead of `sql` text.
3. The owner signature check over a large body is slow → acceptable; the body limit caps it at `max_db_bytes`.

---

### R34-9 — Team side: `relay_moved` event and putting a team on a new relay

Size: Sonnet (team protocol). Deps: R32-6.

**Goal**
The team module can put the team on another relay and announce the move with an admin-signed `relay_moved` event.

**Read first**
- `packages/app/src-tauri/src/team/event.rs` — `TeamEvent`, its kinds, `validate_inbound` (spaces facts §3; kinds NF-20).
- `packages/app/src-tauri/src/team/membership.rs` — `create_team` (line 55) and how it registers, sets the access list,
  and puts state (NF-29).
- `packages/app/src-tauri/src/team/secure.rs` — `roster_blob`, `grant_blob`, `make_grant` (spaces facts §3).
- `packages/app/src-tauri/src/team/service.rs` — how an event is sealed and sent, and where the transport is rebuilt
  after `team_config_set` (NF-38).

**Files**
- edit `packages/app/src-tauri/src/team/event.rs`
- edit `packages/app/src-tauri/src/team/service.rs`
- edit `packages/app/src-tauri/src/team/mod.rs` (add `#[cfg(test)] mod tests_relay_move;`)
- create `packages/app/src-tauri/src/team/tests_relay_move.rs`

**Steps**
1. Add the event kind `relay_moved` with one field `url: String` (Code), following the style of the existing kinds. Its
   inbound check: `url` passes `normalize_relay_url`, at most 512 characters.
2. In `service.rs`, add `pub fn put_team_on(url: &str, owner: Option<SigningKey>) -> Result<(), String>` (Code).
3. In `service.rs`, add `pub fn announce_relay_move(url: &str) -> Result<(), String>`: only when this member is an admin
   (`roster.is_admin(&my_member)`), else `Err("Only a team admin can move the team.")`; seals and sends the event
   through the current transport.
4. Tests, run, commit `team: relay_moved event and moving a team`.

**Code**
Event kind (shape; the enum or tag style follows `event.rs`, NF-20):
```rust
/// The team now uses another relay. Only events from a team admin are followed.
RelayMoved { url: String },
```
```rust
/// Puts this team on the relay at `url`: registration (owner-signed when `owner` is set), access list, and the
/// roster and keys state blobs. A 409 "team already registered" counts as done (a restored relay).
pub fn put_team_on(url: &str, owner: Option<SigningKey>) -> Result<(), String> {
    // 1. normalize_relay_url(url)
    // 2. let transport = RelayTransport::new(&url, &team_id, device_key)?, plus .with_owner(owner) when Some
    // 3. transport.register(&TeamRegistration { team, root, salt, acl }) — the same values create_team used (NF-29);
    //    Err with "The relay already has a newer copy." or a 409 → continue
    // 4. transport.set_acl(&current_signed_acl)
    // 5. transport.put_state(&roster_blob(&roster, &epoch_key, &root_key)?)
    // 6. transport.put_state(&grant_blob(&make_grant(&roster, &epoch_key)?, &root_key))
    // 7. transport.stop()
}
```
The root signing key and the current access list are read the way `create_team` and `rotate` read them (NF-29). If
this computer does not hold the root key, return `Err("Only the person who made the team can move it.")`.

**Tests to write**
`tests_relay_move.rs` (use the in-memory or folder transport the other team tests use; for `put_team_on`, inject the
transport through a test-only `put_team_on_with(transport: &dyn TeamTransport, …)` split):
- `relay_moved_event_round_trips` → build, seal, open, validate → kind `RelayMoved` with the same url.
- `relay_moved_with_http_url_is_rejected` → `http://relay.example.com` → `validate_inbound` rejects.
- `put_team_on_registers_acl_and_state` → a recording transport sees `register`, `set_acl`, `put_state("roster")`, `put_state("keys")` in that order.
- `put_team_on_accepts_already_registered` → `register` returns the 409 error → still `Ok`.
- `announce_needs_admin` → a non-admin member → `Err("Only a team admin can move the team.")`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-9'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib team::tests_relay_move`
→ `test result: ok. 5 passed`. Then `--lib team` → `ok.`.

**Done when**
- [ ] 5 tests pass; all team tests pass.
- [ ] `SCHEMA_VERSION` handling matches what `event.rs` does for a new kind (if adding a kind needs a version bump,
      the existing code says so; follow it).

**Do NOT**
- Do not send team traffic through the bridge or device pairing.
- Do not let a non-admin event change anything.
- Do not put the root key anywhere but the keystore.

**If it fails**
1. Old app versions reject the unknown kind loudly → `validate_inbound` must treat unknown kinds the way it does today;
   do not change that rule here.
2. `put_state` returns 409 "older state" → the new relay already has newer blobs (restored); treat 409 as done.
3. No root key on this computer → expected for non-creators; the UI (R34-12) only offers Move to the creator.

---

### R34-10 — Move my team here (relay maker side)

Size: Sonnet. Deps: R34-8, R34-9, R33-6.

**Goal**
**Move my team here** runs the four steps of section 7 and leaves this computer's team on the new relay.

**Read first**
- Section 7 of this file.
- `team::service::put_team_on`, `announce_relay_move` (R34-9).
- `relay_host/backup.rs` (R34-5) and the restore endpoint (R34-8).

**Files**
- edit `packages/app/src-tauri/src/relay_host/service.rs`
- edit `packages/app/src-tauri/src/relay_host/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- create `packages/app/src-tauri/src/relay_host/move_tests.rs`
- edit `packages/app/src-tauri/src/relay_host/config.rs` (fields `moved_from_url`, `moved_at_ms`)

**Steps**
1. Add `moved_from_url: String`, `moved_at_ms: u64` to `HostConfig` (camelCase, default).
2. Add `MoveStep { CheckNew, Copy, Tell, Switch }` and `pub fn move_team(self: &Arc<Self>) -> Result<RelayHostView, String>`
   to `service.rs`, run with `run_job(Phase::Fixing, …)`. Its team calls go through a new `HostDeps` field
   `team: Arc<dyn TeamOps>` (Code) so tests can fake them.
3. Command `relay_host_move_team(app: AppHandle) -> Result<Value, String>`; register it.
4. View field `moved_notice: Option<String>`: when `moved_at_ms` is older than 7 days, the text
   `Your team moved off {moved_from_url} on {date}. If you run that relay, you can turn it off now.`; command
   `relay_host_dismiss_moved` clears `moved_from_url`.
5. Tests, run, commit `relay_host: move my team here`.

**Code**
```rust
pub trait TeamOps: Send + Sync {
    fn team_relay(&self) -> Option<String>;
    fn is_admin(&self) -> bool;
    fn holds_root(&self) -> bool;
    fn put_team_on(&self, url: &str, owner: Option<ed25519_dalek::SigningKey>) -> Result<(), String>;
    fn announce_relay_move(&self, url: &str) -> Result<(), String>;
    /// Sets TeamConfig.relay and restarts the transport (the same path team_config_set uses, NF-38).
    fn switch_relay(&self, url: &str) -> Result<(), String>;
}

pub const MOVE_NOT_ADMIN: &str = "Only the person who made the team can move it.";
pub const MOVE_TELL_FAILED: &str = "We couldn't tell your team through the old relay. Send each teammate the new address: {url}. They paste it in Team › Relay address.";
pub const MOVE_DONE: &str = "Your team now uses your relay.";
```
The restore upload is one more `HostDeps` field,
`restore_put: Arc<dyn Fn(&str, &SigningKey, u64, &std::path::Path) -> Result<(), String> + Send + Sync>`, whose real
value (written in `service.rs`) sends `PUT {url}/v1/admin/restore` with the file as the body and `x-pm-owner` signed
over those exact bytes; tests pass a recording closure.

`move_team` order: `holds_root()` and `is_admin()` else `Err(MOVE_NOT_ADMIN)`; `CheckNew` deep check of own relay;
`Copy`: if a backup file exists in `relay-backups/` **and** owner status shows `teams == 0`, PUT the newest backup to
`/v1/admin/restore` (owner-signed); then always `put_team_on(url, Some(owner))`; `Tell`: `announce_relay_move(url)`,
on error remember `MOVE_TELL_FAILED` with `{url}` filled; `Switch`: `switch_relay(url)`; save `moved_from_url` (the old
team relay) and `moved_at_ms`; notice = `MOVE_DONE` or the tell-failed text.

**Tests to write**
`move_tests.rs` (FakeTeamOps records calls):
- `move_runs_four_steps_in_order` → calls `put_team_on`, `announce_relay_move`, `switch_relay`; notice `Your team now uses your relay.`
- `move_refused_for_non_creator` → `holds_root` false → `Err(MOVE_NOT_ADMIN)`; no calls.
- `move_restores_backup_into_empty_relay` → a backup file present, FakeProbe owner status `teams: 0` → restore called before `put_team_on`.
- `move_skips_restore_when_relay_has_teams` → status `teams: 1` → no restore call.
- `move_when_old_relay_is_dead_still_switches` → `announce_relay_move` errs → `switch_relay` still called; notice contains the new url and `They paste it in Team › Relay address.`
- `moved_notice_after_seven_days` → `moved_at_ms` 8 days ago → view `moved_notice` starts with `Your team moved off`.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-10'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib relay_host::move_tests`
→ `test result: ok. 6 passed`.

**Done when**
- [ ] 6 tests pass; commands registered.

**Do NOT**
- Do not switch this computer before the team is on the new relay.
- Do not delete or touch the old relay.
- Do not offer Move to a member who does not hold the root key.

**If it fails**
1. `switch_relay` restarts the transport while the announce is still sending → announce first, wait for its `Ok`/`Err`,
   then switch (the order in Code).
2. The restore is attempted on a relay that just got the team registered → do `Copy`'s restore before `put_team_on`.
3. `{url}` stays in the text → fill it with `replace("{url}", &url)`.

---

### R34-11 — Members follow a moved team

Size: Sonnet. Deps: R34-9.

**Goal**
A member's app switches to the new relay after a verified admin `relay_moved` event, and a member can paste a new
address by hand with the same checks.

**Read first**
- `packages/app/src-tauri/src/team/store.rs` — `ingest` (line 194) and `Verified` (spaces facts §3).
- `packages/app/src-tauri/src/team/roster.rs` — `is_admin` (spaces facts §3).
- `packages/app/src-tauri/src/team/commands.rs` — command style (relay facts §6).
- The transport rebuild path after a config change (NF-38).

**Files**
- edit `packages/app/src-tauri/src/team/service.rs`
- edit `packages/app/src-tauri/src/team/commands.rs`
- edit `packages/app/src-tauri/src/lib.rs`
- edit `packages/app/src-tauri/src/team/tests_relay_move.rs`

**Steps**
1. In `service.rs`, add the pure decision `pub fn follow_move(event_from_admin: bool, url: &str, check: &dyn Fn(&str) -> Result<(), String>) -> Follow` (Code).
2. Where verified events are handled (after `ingest`), on `RelayMoved`: compute `event_from_admin` with
   `roster.is_admin(verified.member())`; call `follow_move` with a `check` that builds a `RelayTransport` for the new URL
   and calls `get_state("roster")` (Ok(Some) = pass); on `Switch` call the transport-rebuild path and set the notice
   `Your team moved to a new relay. Updates come from there now.`; on `Retry` store the URL and try again every 10
   minutes for 7 days with the notice `Your team's admin moved the team to a new relay, but this computer can't reach it yet. We'll keep trying.`
3. Add command `team_relay_switch(url: String) -> Result<Value, String>` that runs `follow_move(true, url, check)` (the
   person typed it) and returns the team view.
4. Register the command after `team::commands::team_config_set,`.
5. Tests, run, commit `team: follow a moved relay`.

**Code**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow { Ignore, Switch(String), Retry(String), Refuse(String) }

pub const MOVED_NOTICE: &str = "Your team moved to a new relay. Updates come from there now.";
pub const MOVE_WAITING: &str = "Your team's admin moved the team to a new relay, but this computer can't reach it yet. We'll keep trying.";
pub const NOT_ON_RELAY: &str = "That relay doesn't have your team. Check the address with your team's admin.";

pub fn follow_move(event_from_admin: bool, url: &str, check: &dyn Fn(&str) -> Result<(), String>) -> Follow {
    if !event_from_admin { return Follow::Ignore; }
    let Ok(url) = crate::team::transport::relay::normalize_relay_url(url) else {
        return Follow::Refuse("Use https:// for a relay on another computer.".into());
    };
    match check(&url) {
        Ok(()) => Follow::Switch(url),
        Err(_) => Follow::Retry(url),
    }
}
```
`team_relay_switch` maps `Retry` to `Err(NOT_ON_RELAY)` (a typed address is not retried in the background) and `Refuse`
to its message.

**Tests to write**
In `tests_relay_move.rs`:
- `relay_moved_from_admin_switches` → `follow_move(true, "https://new.example", ok)` → `Switch("https://new.example")`.
- `relay_moved_from_non_admin_is_ignored` → `follow_move(false, …)` → `Ignore`, and `check` is never called.
- `relay_moved_to_http_is_refused` → `http://new.example` → `Refuse("Use https:// for a relay on another computer.")`.
- `relay_moved_waits_when_new_relay_lacks_team` → check errs → `Retry`.
- `manual_switch_refuses_relay_without_team` → `team_relay_switch` path with a failing check → `Err(NOT_ON_RELAY)`.
- `ingest_of_admin_move_updates_config` → end-to-end inside the team test harness: an admin's `RelayMoved` event ingested by a member with a passing check → that member's saved `TeamConfig.relay` equals the new URL.

**Run**
`$env:CARGO_TARGET_DIR='D:/pm-night/target-r34-11'; cargo test --manifest-path packages/app/src-tauri/Cargo.toml --lib team::tests_relay_move`
→ `test result: ok. 11 passed` (5 + 6). Then `--lib team` → `ok.`.

**Done when**
- [ ] 6 new tests pass; all team tests pass.

**Do NOT**
- Do not follow a move from a non-admin.
- Do not switch before the new relay proves it holds the team for this device.
- Do not run the check on the UI thread.

**If it fails**
1. `verified.member()` gives a device id, not a member id → map it with `roster.device(id)` to the member first.
2. Retry timer keeps running after a switch → clear the stored retry URL on switch and on team leave.
3. The check uses the old relay's transport → build a fresh `RelayTransport` for the new URL; never reuse the running one.

---

### R34-12 — Recovery buttons in the Your relay row (UI)

Size: Haiku. Deps: R34-3, R34-5, R34-6, R34-7, R34-10.

**Goal**
The row shows Fix it, Restart, Save a backup, Update relay, Remove relay (with its sheet), Move my team here, and the
fixing and removing progress, with the strings of sections 3 to 7.

**Read first**
- `packages/app/src/lib/relay-host.ts`, `RelayHostSettings.tsx`, `RelayReadyCard.tsx`, `RelayProblemCard.tsx` (file 33).

**Files**
- edit `packages/app/src/lib/relay-host.ts`
- edit `packages/app/src/components/settings/relay/RelayProblemCard.tsx`
- edit `packages/app/src/components/settings/relay/RelayReadyCard.tsx`
- edit `packages/app/src/components/settings/relay/RelayHostSettings.tsx`
- edit `packages/app/src/components/settings/relay/RelayHostSettings.test.tsx`

**Steps**
1. `relay-host.ts`: add to `RelayHostView` the fields `notice: string | null`, `fixStep: 'diagnose' | 'restart' | 'start' | 'check' | null`,
   `updateStep: 'backup' | 'switch' | 'start' | 'check' | null`, `waitingForWebDelete: boolean`,
   `movedNotice: string | null`; add API methods `fix`, `restart`, `backup`, `update`, `remove(backupFirst)`,
   `confirmRemoved`, `moveTeam`, `dismissMoved` calling `relay_host_fix`, `relay_host_restart`, `relay_host_backup`,
   `relay_host_update`, `relay_host_remove` (`{ backupFirst }`), `relay_host_confirm_removed`, `relay_host_move_team`,
   `relay_host_dismiss_moved`; add the string tables `FIX_STEPS`, `UPDATE_STEPS` (Code).
2. `RelayProblemCard.tsx`: add **Fix it** before **Check again** (not for `account`, `full`, `daily_limit`); export
   `RelayWorking` (Code) for `fixing` and `removing`.
3. `RelayReadyCard.tsx`: add the More actions and `RelayRemoveSheet` (Code); show **Move my team here** when
   `teamFit === 'move'`; show `notice` and `movedNotice` (with **Dismiss**).
4. `RelayHostSettings.tsx`: render `RelayWorking` for `fixing` and `removing`; show `notice` in every phase; when
   `waitingForWebDelete`, show the `WEB_DELETE` text and **I deleted it**; add **Remove what was made** to `failed`
   when `hasProject`.
5. Tests, run, commit `ui: relay recovery buttons`.

**Code**
```ts
export const FIX_STEPS = [
  { step: 'diagnose', text: 'Checking what\u2019s wrong' },
  { step: 'restart', text: 'Restarting the relay' },
  { step: 'start', text: 'Waiting for it to start' },
  { step: 'check', text: 'Checking that it works' },
] as const;

export const UPDATE_STEPS = [
  { step: 'backup', text: 'Saving a backup' },
  { step: 'switch', text: 'Switching to the new version' },
  { step: 'start', text: 'Starting the relay' },
  { step: 'check', text: 'Checking that it works' },
] as const;

export const WEB_DELETE = 'Railway needs you to delete it on its website. We opened the page. Press I deleted it when it\u2019s gone.';
```
```tsx
export function RelayWorking({ title, steps, current }: { title: string; steps: readonly { step: string; text: string }[]; current: string | null }) {
  const at = current ? steps.findIndex((s) => s.step === current) : 0;
  return (
    <section className="ck-relay" aria-label={title}>
      <h3>{title}</h3>
      <ol className="ck-relay-steps">
        {steps.map((s, i) => (
          <li key={s.step} className={`ck-relay-step${i < at ? ' is-done' : i === at ? ' is-now' : ''}`} aria-current={i === at ? 'step' : undefined}>{s.text}</li>
        ))}
      </ol>
    </section>
  );
}
```
Titles: `Fixing your relay` (fixStep set), `Updating your relay` (updateStep set), `Moving your team` (move steps:
`Checking your new relay`, `Copying your team to it`, `Telling your team`, `Switching this computer`),
`Removing your relay` (removing, no list).

`RelayRemoveSheet` (in `RelayReadyCard.tsx`):
```tsx
export function RelayRemoveSheet({ provider, usedByTeam, busy, onRemove, onClose }: {
  provider: RelayProvider; usedByTeam: boolean; busy: boolean; onRemove: (backupFirst: boolean) => void; onClose: () => void;
}) {
  const titleId = useId();
  return (
    <Sheet labelledBy={titleId} role="alertdialog" onClose={onClose}>
      <h2 id={titleId}>Remove your relay?</h2>
      <p>
        Your team stops getting updates until it uses another relay. Messages still waiting on the relay are deleted.
        {provider === 'railway' ? ' Railway stops charging for it.' : ''}
      </p>
      {usedByTeam && <p>Your team uses this relay right now.</p>}
      <div className="ck-relay-actions">
        {provider === 'railway' && <button type="button" disabled={busy} onClick={() => onRemove(true)}>Save a backup, then remove</button>}
        <button type="button" disabled={busy} onClick={() => onRemove(false)}>Remove</button>
        <button type="button" onClick={onClose}>Cancel</button>
      </div>
    </Sheet>
  );
}
```
More actions in the ready card: `Restart`, `Save a backup` (only when `canBackup`; else the line
`Backups aren't available on Cloudflare.`), `Update relay` (only when `updateAvailable`), `Remove relay`.
Last backup line when `lastBackupMs`: `Last backup: {date}` with the local date.

**Tests to write**
In `RelayHostSettings.test.tsx`:
- `unhealthy shows Fix it and calls fix` → `problem: 'down'` → click `Fix it` → `api.fix` called.
- `no Fix it for full` → `problem: 'full'` → no `Fix it` button.
- `fixing shows the fix steps` → `phase: 'fixing', fixStep: 'start'` → heading `Fixing your relay`, current item `Waiting for it to start`.
- `ready shows Update relay only when available` → `updateAvailable: false` → absent; `true` → present.
- `remove sheet offers backup on Railway` → click `Remove relay` → `Save a backup, then remove`, `Remove`, `Cancel`; click `Save a backup, then remove` → `api.remove(true)`.
- `remove sheet on Cloudflare has no backup button` → provider `cloudflare` → no `Save a backup, then remove`.
- `web delete shows I deleted it` → `waitingForWebDelete: true` → the `WEB_DELETE` text and `I deleted it` → click → `api.confirmRemoved`.
- `move button when team uses another relay` → `teamFit` `move` → `Move my team here` → click → `api.moveTeam`.
- `notice is shown` → `notice: 'Fixed. Your relay is working again.'` → visible.

**Run**
`npx vitest run packages/app/src/components/settings/relay` → all pass (19 earlier + 9).

**Done when**
- [ ] 9 new tests pass; strings match sections 3 to 7.

**Do NOT**
- Do not call remove without the sheet.
- Do not show the Move button to a member without the root key (the command refuses; the button shows only when `teamFit === 'move'`, and the refusal text appears as the error).

**If it fails**
1. `Sheet` with `role="alertdialog"` needs a description id in this repo → pass the body paragraph's id if `Sheet` asks.
2. Date text differs across machines → assert only that `Last backup:` is present.
3. Two `Remove` buttons match → query inside the sheet with `within(screen.getByRole('alertdialog'))`.

---

### R34-13 — Relay-down notice in Team settings (UI)

Size: Haiku. Deps: R34-1, R34-11, R34-12.

**Goal**
Team settings shows the plain relay-down notice, with **Fix it** for the relay maker and a paste-new-address path for
everyone.

**Read first**
- `packages/app/src/components/settings/blocks/TeamSettings.tsx` — how `view.transport`, `view.link` and errors are
  shown (relay facts §6; error field NF-24).
- `packages/app/src/lib/team.ts` — `TeamApi` (add `relaySwitch`).
- `packages/app/src/lib/relay-host.ts`.

**Files**
- create `packages/app/src/components/settings/relay/RelayDownNotice.tsx`
- create `packages/app/src/components/settings/relay/RelayDownNotice.test.tsx`
- edit `packages/app/src/components/settings/blocks/TeamSettings.tsx`
- edit `packages/app/src/lib/team.ts`

**Steps**
1. In `team.ts`, add `relaySwitch: (url: string) => call('team_relay_switch', { url })` to `TeamApi` and `teamApi`.
2. Write `RelayDownNotice.tsx` (Code).
3. In `TeamSettings.tsx`, render `<RelayDownNotice … />` above the connection text when `view.transport === 'relay'`
   and `view.link` is `offline` or `polling`. `mine` = the relay host view is `ready`/`unhealthy` and its `url` equals
   `view.relay` without a trailing `/`. When the notice mounts with `mine`, call `relayHostApi.check()` once.
4. Tests, run, commit `ui: relay down notice in Team`.

**Code**
```tsx
import { useState } from 'react';

export function RelayDownNotice({
  link, mine, relayOn, busy, error, onFix, onRetry, onSwitch,
}: {
  link: 'offline' | 'polling';
  mine: boolean;
  relayOn: boolean;
  busy: boolean;
  error: string | null;
  onFix: () => void;
  onRetry: () => void;
  onSwitch: (url: string) => void;
}) {
  const [pasting, setPasting] = useState(false);
  const [url, setUrl] = useState('');
  if (link === 'polling') {
    return (
      <div className="ck-relay-note" role="status">
        <p>Live updates are blocked on the way to the relay, so updates come every 20 seconds.</p>
        {mine && relayOn && <button type="button" disabled={busy} onClick={onFix}>Fix it</button>}
      </div>
    );
  }
  return (
    <div className="ck-relay-note" role="alert">
      {mine ? (
        <>
          <p>Your relay isn&rsquo;t answering, so your team isn&rsquo;t getting updates.</p>
          {relayOn
            ? <button type="button" disabled={busy} onClick={onFix}>Fix it</button>
            : <p>Turn on Your relay in Settings › Labs to fix it here.</p>}
        </>
      ) : (
        <>
          <p>Your team&rsquo;s relay isn&rsquo;t answering. The person who made it can fix it in their Settings › Your relay.</p>
          <button type="button" disabled={busy} onClick={onRetry}>Try again</button>
          <button type="button" className="ck-relay-link" onClick={() => setPasting(true)}>The relay moved? Paste the new address</button>
          {pasting && (
            <label>
              <span>New relay address</span>
              <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://relay.example.com" aria-label="New relay address" />
              <button type="button" disabled={busy || !url.trim()} onClick={() => onSwitch(url.trim())}>Use this address</button>
            </label>
          )}
        </>
      )}
      {error && <p className="ck-relay-error">{error}</p>}
    </div>
  );
}
```

**Tests to write**
`RelayDownNotice.test.tsx`:
- `mine and on shows Fix it` → `link: 'offline', mine, relayOn` → `Your relay isn’t answering, so your team isn’t getting updates.` and `Fix it` → click → `onFix`.
- `mine and Labs off says how to turn it on` → `relayOn: false` → `Turn on Your relay in Settings › Labs to fix it here.`
- `not mine shows Try again and paste link` → text `Your team’s relay isn’t answering. The person who made it can fix it in their Settings › Your relay.`; click the paste link; type `https://new.example`; click `Use this address` → `onSwitch('https://new.example')`.
- `polling text` → `link: 'polling'` → `Live updates are blocked on the way to the relay, so updates come every 20 seconds.`
- `error shown` → `error="That relay doesn't have your team. Check the address with your team's admin."` → visible.
- In `TeamSettings.test.tsx` is not edited; the wiring is covered by a new case in `RelayDownNotice.test.tsx`:
  `TeamSettings shows the notice when offline` → render `TeamSettings` with a fake `api` whose view is
  `{ transport: 'relay', link: 'offline', relay: 'https://r.example', … }` → the not-mine text is visible.

**Run**
`npx vitest run packages/app/src/components/settings` → all pass, including 6 new tests.

**Done when**
- [ ] 6 tests pass; the old `TeamSettings` tests pass unchanged.
- [ ] No screen shows `The relay said 502` for a 502 (R34-1 plus this notice).

**Do NOT**
- Do not show Fix it on a computer that did not make the relay.
- Do not call `team_relay_switch` with an empty address.

**If it fails**
1. `view.link` has other values (`live`, `denied`) → render nothing for them.
2. The relay address in the view has a trailing slash → compare after removing one trailing `/` on both sides.
3. The check call loops on every render → call it in a `useEffect` with `[mine]` as the only dependency.

---

## Open facts

- NEEDS-FACT NF-1: the type of `Config.db` and the shared state accessors (needed by R34-4, R34-8). Read
  `relay/server/src/lib.rs` `Config` and `router`.
- NEEDS-FACT NF-4: axum version and whether `tokio-util` is a dependency. Read `relay/server/Cargo.toml`.
- NEEDS-FACT NF-5: how `Relay` holds its connection. Read `relay/server/src/lib.rs` near line 326.
- NEEDS-FACT NF-9: which Railway mutations a project token may call (restart, redeploy, update, delete). Read Railway's
  public API token docs.
- NEEDS-FACT NF-11: the Cloudflare upload form for updating an existing Worker with Durable Objects (migration tags).
  Read Cloudflare's Workers script upload API docs.
- NEEDS-FACT NF-15: where the text `relay WebSocket unavailable` is produced and whether it reaches the UI. Run
  `rg -n "WebSocket unavailable" packages/app`.
- NEEDS-FACT NF-20: how `TeamEvent` kinds are declared and validated, and whether a new kind needs a schema change. Read
  `packages/app/src-tauri/src/team/event.rs`.
- NEEDS-FACT NF-22: the exact `CREATE TABLE` statements for `teams`, `devices`, `state`, `joins`, `envelopes`, and
  whether `PRAGMA user_version` is used. Read `Relay::open` in `relay/server/src/lib.rs`.
- NEEDS-FACT NF-24: the field in the team view that carries the transport error text, and how `TeamSettings.tsx`
  shows `view.link`. Read `packages/app/src/lib/team.ts` `TeamConfigView` and `TeamSettings.tsx` around line 74.
- NEEDS-FACT NF-29: how `create_team` registers the team, signs the access list and puts the `roster` and `keys` blobs,
  and which key is the root signing key on the creator's computer. Read
  `packages/app/src-tauri/src/team/membership.rs` lines 55-120 and 228-270.
- NEEDS-FACT NF-31: what the Cloudflare free plan returns when the daily limit is hit (status code and body), to confirm
  the `429 → daily_limit` rule. Read Cloudflare's Workers limits docs.
- NEEDS-FACT NF-38: the function `team_config_set` uses to save `TeamConfig` and restart the transport. Read
  `packages/app/src-tauri/src/team/commands.rs` line 25 and the `service.rs` function it calls.

## Security

| Threat | What an attacker or broken peer can do | Defence | Test |
|---|---|---|---|
| Fake "team moved" event | Send members to an attacker's relay | Only admin-signed events are followed; the new relay must hold the team for this device; `https://` only | `relay_moved_from_non_admin_is_ignored`, `relay_moved_waits_when_new_relay_lacks_team`, `relay_moved_to_http_is_refused` |
| A member pastes a stranger's address | Their device talks to a stranger | The same check: the relay must already hold this team and accept this device's signature | `manual_switch_refuses_relay_without_team` |
| Backup stolen from app data | Metadata (ids, sizes, timing) and ciphertext | No plaintext on the relay by design; backups stay in app data; newest 4 only | `backup_holds_the_registered_team` shows what is inside; team crypto tests show envelopes are sealed (spaces facts §3) |
| Backup download by a stranger | Same metadata | Owner signature, replay guard, 404 without owner, `no-store` | `backup_needs_owner`, `backup_is_404_without_owner_key` |
| Restore used to plant data | Fake teams on the owner's relay | Owner signature; empty relay only; integrity and schema checks | `restore_needs_owner`, `restore_refused_when_teams_exist`, `restore_refused_for_other_schema` |
| Bad update | Relay down for everyone | Backup first; roll back when the new version never gets ready | `update_rolls_back_on_crash`, `update_does_not_start_without_backup` |
| Fix loops spend money | Many redeploys | One restart and one redeploy per press | `fix_tries_redeploy_after_restart_fails` |
| Revoked token | Fix silently fails | Turns into `account` with Connect again | `fix_with_revoked_token_asks_to_reconnect` |
| Accidental remove | Team loses its relay | Sheet with plain consequences; backup offer; command only from the sheet | `remove sheet offers backup on Railway` |
| Old relay keeps data after a move | The old operator keeps metadata | The maker gets a reminder after 7 days; the app never deletes a relay it did not make | `moved_notice_after_seven_days` |

## What cannot be verified without real hardware or a second computer

- Fix against a real Railway crash (needs a real deploy and a way to crash it, for example a bad variable).
- Railway's real behaviour for `SLEEPING` and for a project token calling restart or delete (NF-9).
- Backup and restore speed and memory for a large database through Railway's proxy.
- A team move with three computers: the admin, an online member, and a member who is offline for a day and for 8 days.
- The Cloudflare daily-limit reply (NF-31).
