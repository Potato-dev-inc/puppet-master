//! The standby pool's state machine. Pure bookkeeping: every side effect (spawn, health probe,
//! kill) goes through the `Host` trait and every method takes the current time, so the rules
//! (what to warm, when to give up, who may claim) are tested without a PTY or a clock.
//!
//! Rules in one place:
//! - target size 1 per `PoolKey`; keys come from runs seen recently (`note_run`), only the most
//!   recent `recent_projects` projects inside `recent_hours`, only for enabled backends, never
//!   more than `max_total` standbys at once;
//! - a standby is claimable only once it has been observed `Ready` twice; claiming removes it from
//!   the pool under the lock, so two callers can never get the same one, then re-checks its health;
//! - a standby unused for `idle_timeout_secs` is shut down and its key stays cold until the next run
//!   of that key (`note_run`), so an idle app does not keep CLIs alive or respawn them forever;
//! - a standby that shows a dialog / login / update, dies, or never becomes ready is discarded and
//!   its key backs off (and goes cold after 3 failures), so an untrusted folder is simply not pooled.

use super::config::{self, RecentUse, StandbyConfig, MAX_RECENT};
use super::key::PoolKey;
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// What a standby is, as far as the pool cares: a TUI pane or a headless process (the host maps
/// the token back to the real child).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Handle {
    Pane(String),
    Process(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ready,
    Starting,
    /// A trust or approval dialog is up (never answered by the pool).
    Dialog,
    /// A login, update or first-run screen.
    NeedsAttention,
    Dead,
}

impl Health {
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Ready => "ready",
            Health::Starting => "starting",
            Health::Dialog => "dialog",
            Health::NeedsAttention => "needs_attention",
            Health::Dead => "dead",
        }
    }
}

/// The side effects the pool needs. Implemented by `host::LiveHost` and by test fakes.
pub trait Host: Send + Sync {
    /// Start a standby for `key` in `cwd` (hidden pane or waiting process).
    fn spawn(&self, key: &PoolKey, cwd: &str) -> Result<Handle, String>;
    /// Observe a standby. `age_ms` is how long ago it was spawned.
    fn check(&self, key: &PoolKey, handle: &Handle, age_ms: i64) -> Health;
    /// Shut a standby down. Must not block for long (called under no lock, but on hot paths).
    fn kill(&self, handle: &Handle, reason: &str);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    Starting,
    Ready,
}

struct Entry {
    id: u64,
    key: PoolKey,
    cwd: String,
    handle: Handle,
    state: EntryState,
    created_ms: i64,
    ready_since_ms: Option<i64>,
    /// Consecutive `Ready` observations while `Starting`.
    ready_checks: u32,
    last_observed_ms: i64,
    health: Health,
}

struct Backoff {
    fails: u32,
    retry_at_ms: i64,
    last: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Stats {
    pub spawned: u64,
    pub spawn_failed: u64,
    pub claimed: u64,
    /// A run asked and no matching ready standby existed.
    pub missed: u64,
    /// A standby failed its claim-time health check (or a periodic one) and was thrown away.
    pub discarded: u64,
    pub expired: u64,
    /// Claimed and then given back unused (the run was cancelled or failed during the claim).
    pub abandoned: u64,
}

/// A startup that never reaches a ready screen is given up after this long.
pub const STARTUP_TIMEOUT_MS: i64 = 90_000;
/// Two `Ready` observations at least this far apart promote a standby: a screen that is still
/// being drawn is not stable.
pub const READY_RECHECK_MS: i64 = 400;
const BACKOFF_BASE_MS: i64 = 15_000;
const BACKOFF_MAX_MS: i64 = 300_000;
/// After this many failures in a row a key stays cold until its next run.
const MAX_FAILS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissReason {
    Disabled,
    NoStandby,
    NotReady,
    Unhealthy(Health),
}

pub enum ClaimOutcome {
    Hit(Claim),
    Miss(MissReason),
}

/// A standby taken out of the pool for one run. Dropping it without `commit` shuts the standby
/// down, so a run cancelled (or failed) between claim and hand-over never leaks a worker.
pub struct Claim {
    host: Arc<dyn Host>,
    handle: Handle,
    age_ms: i64,
    committed: bool,
    pool: Arc<PoolShared>,
}

impl Claim {
    pub fn handle(&self) -> &Handle {
        &self.handle
    }
    /// How long the standby had been warm.
    pub fn age_ms(&self) -> i64 {
        self.age_ms
    }
    /// The run owns the worker now.
    pub fn commit(mut self) -> Handle {
        self.committed = true;
        self.handle.clone()
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if !self.committed {
            self.pool.inner.lock().stats.abandoned += 1;
            self.host.kill(&self.handle, "claim_abandoned");
            self.pool.wake.notify();
        }
    }
}

#[derive(Default)]
pub struct Wake {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl Wake {
    pub fn notify(&self) {
        *self.flag.lock() = true;
        self.cv.notify_all();
    }
    /// Wait until notified or `timeout` passes. Returns whether it was notified.
    pub fn wait(&self, timeout: std::time::Duration) -> bool {
        let mut flag = self.flag.lock();
        if !*flag {
            let _ = self.cv.wait_for(&mut flag, timeout);
        }
        std::mem::take(&mut *flag)
    }
}

struct Inner {
    config: StandbyConfig,
    recent: Vec<RecentUse>,
    entries: Vec<Entry>,
    next_id: u64,
    backoff: HashMap<PoolKey, Backoff>,
    /// Keys left cold until their next run (idle expiry, repeated failure).
    suppressed: HashSet<PoolKey>,
    spawning: HashSet<PoolKey>,
    stats: Stats,
    shut_down: bool,
}

struct PoolShared {
    inner: Mutex<Inner>,
    wake: Wake,
}

pub struct Pool {
    host: Arc<dyn Host>,
    shared: Arc<PoolShared>,
    store: Option<PathBuf>,
    /// No spawning before this time (the app is still starting).
    spawn_not_before_ms: i64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Counts {
    pub total: usize,
    pub ready: usize,
    pub starting: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StandbyView {
    pub id: u64,
    pub agent_type: String,
    pub project: String,
    pub headless: bool,
    pub read_only: bool,
    pub model: Option<String>,
    pub state: EntryState,
    /// Last health observation.
    pub health: &'static str,
    pub age_ms: i64,
    /// How long it has been ready (the idle timer runs from here).
    pub ready_for_ms: Option<i64>,
    pub idle_expires_in_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockedView {
    pub key: String,
    pub reason: String,
    pub fails: u32,
    pub retry_in_ms: i64,
    /// Cold until the next run of this key.
    pub cold: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusView {
    pub enabled: bool,
    pub config: StandbyConfig,
    pub counts: Counts,
    pub standbys: Vec<StandbyView>,
    /// Keys the pool wants to warm but is holding back (untrusted folder, failures, idle expiry).
    pub blocked: Vec<BlockedView>,
    pub recent: Vec<RecentView>,
    pub stats: Stats,
    /// Which backends can be pooled, and in which kinds (what the settings tab offers).
    pub supported: Vec<SupportedBackend>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportedBackend {
    pub agent_type: String,
    pub visible: bool,
    pub headless: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentView {
    pub key: String,
    pub age_ms: i64,
}

/// Counts only, for the bridge health summary.
#[derive(Debug, Clone, Serialize)]
pub struct HealthSummary {
    pub enabled: bool,
    pub total: usize,
    pub ready: usize,
    pub starting: usize,
}

/// What one `reconcile` pass did (for tests and logs).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reconciled {
    pub spawned: Vec<PoolKey>,
    pub killed: usize,
}

impl Pool {
    pub fn new(
        host: Arc<dyn Host>,
        store: Option<PathBuf>,
        config: StandbyConfig,
        recent: Vec<RecentUse>,
        spawn_not_before_ms: i64,
    ) -> Self {
        Self {
            host,
            store,
            spawn_not_before_ms,
            shared: Arc::new(PoolShared {
                wake: Wake::default(),
                inner: Mutex::new(Inner {
                    config: config.clamped(),
                    recent,
                    entries: Vec::new(),
                    next_id: 1,
                    backoff: HashMap::new(),
                    suppressed: HashSet::new(),
                    spawning: HashSet::new(),
                    stats: Stats::default(),
                    shut_down: false,
                }),
            }),
        }
    }

    pub fn wake_handle(&self) -> &Wake {
        &self.shared.wake
    }

    pub fn config(&self) -> StandbyConfig {
        self.shared.inner.lock().config.clone()
    }

    /// Replace the settings. Standbys the new settings no longer allow (pool off, backend off,
    /// smaller cap) are shut down right away.
    pub fn set_config(&self, config: StandbyConfig) -> StandbyConfig {
        let config = config.clamped();
        let (doomed, snapshot) = {
            let mut inner = self.shared.inner.lock();
            inner.config = config.clone();
            let mut doomed = Vec::new();
            let mut keep = Vec::new();
            for entry in std::mem::take(&mut inner.entries) {
                if inner.config.allows(&entry.key) {
                    keep.push(entry);
                } else {
                    doomed.push(entry);
                }
            }
            // Over the new cap: shut the youngest down first.
            keep.sort_by_key(|entry| entry.created_ms);
            while keep.len() > inner.config.max_total {
                if let Some(entry) = keep.pop() {
                    doomed.push(entry);
                }
            }
            inner.entries = keep;
            (doomed, (inner.config.clone(), inner.recent.clone()))
        };
        for entry in &doomed {
            self.host.kill(&entry.handle, "pool_config_changed");
        }
        self.persist(snapshot);
        self.shared.wake.notify();
        config
    }

    /// A run of `key` started (claimed a standby or spawned its own worker). Remembers it so the
    /// next run can find a warm one, and lifts any cold/backoff state on the key. A disabled pool
    /// ignores it entirely.
    pub fn note_run(&self, key: &PoolKey, cwd: &str, now_ms: i64) {
        let snapshot = {
            let mut inner = self.shared.inner.lock();
            if !inner.config.enabled {
                // A disabled pool keeps no history and writes nothing.
                return;
            }
            inner.recent.retain(|recent| recent.key != *key);
            inner.recent.insert(
                0,
                RecentUse {
                    key: key.clone(),
                    cwd: cwd.to_string(),
                    at_ms: now_ms,
                },
            );
            inner.recent.truncate(MAX_RECENT);
            inner.suppressed.remove(key);
            inner.backoff.remove(key);
            (inner.config.clone(), inner.recent.clone())
        };
        self.persist(snapshot);
        self.shared.wake.notify();
    }

    fn persist(&self, (config, recent): (StandbyConfig, Vec<RecentUse>)) {
        if let Some(path) = &self.store {
            if let Err(error) = config::save(path, &config, &recent) {
                tracing::warn!(%error, "could not save the standby pool config");
            }
        }
    }

    /// Try to take a ready standby for `key`. The standby leaves the pool before anything else
    /// happens, so concurrent callers cannot share one; its health is re-checked after that.
    pub fn claim(&self, key: &PoolKey, now_ms: i64) -> ClaimOutcome {
        let entry = {
            let mut inner = self.shared.inner.lock();
            if inner.shut_down || !inner.config.allows(key) {
                return ClaimOutcome::Miss(MissReason::Disabled);
            }
            let Some(index) = inner.entries.iter().position(|entry| entry.key == *key) else {
                inner.stats.missed += 1;
                return ClaimOutcome::Miss(MissReason::NoStandby);
            };
            if inner.entries[index].state != EntryState::Ready {
                inner.stats.missed += 1;
                return ClaimOutcome::Miss(MissReason::NotReady);
            }
            inner.stats.claimed += 1;
            inner.entries.remove(index)
        };
        let age_ms = now_ms - entry.created_ms;
        let health = self.host.check(&entry.key, &entry.handle, age_ms);
        if health != Health::Ready {
            self.host.kill(&entry.handle, "claim_health_check_failed");
            {
                let mut inner = self.shared.inner.lock();
                inner.stats.claimed -= 1;
                inner.stats.discarded += 1;
                fail(&mut inner, &entry.key, health.as_str(), now_ms);
            }
            self.shared.wake.notify();
            return ClaimOutcome::Miss(MissReason::Unhealthy(health));
        }
        // The slot is empty now: have the replacement spawned.
        self.shared.wake.notify();
        ClaimOutcome::Hit(Claim {
            host: self.host.clone(),
            handle: entry.handle,
            age_ms,
            committed: false,
            pool: self.shared.clone(),
        })
    }

    /// One maintenance pass: shut down what is no longer allowed or idle too long, observe every
    /// standby (promote the ready, discard the broken), then spawn what is missing.
    pub fn reconcile(&self, now_ms: i64) -> Reconciled {
        let mut result = Reconciled::default();
        // A: expiry and disallowed entries.
        let (doomed, observe) = {
            let mut inner = self.shared.inner.lock();
            if inner.shut_down {
                return result;
            }
            let idle_ms = inner.config.idle_timeout_secs as i64 * 1000;
            let mut doomed: Vec<(Entry, &'static str)> = Vec::new();
            let mut keep = Vec::new();
            for entry in std::mem::take(&mut inner.entries) {
                if !inner.config.allows(&entry.key) {
                    doomed.push((entry, "pool_config_changed"));
                } else if entry
                    .ready_since_ms
                    .is_some_and(|since| now_ms - since >= idle_ms)
                {
                    inner.suppressed.insert(entry.key.clone());
                    inner.stats.expired += 1;
                    doomed.push((entry, "idle_timeout"));
                } else {
                    keep.push(entry);
                }
            }
            inner.entries = keep;
            let observe: Vec<(u64, PoolKey, Handle, i64)> = inner
                .entries
                .iter()
                .map(|entry| (entry.id, entry.key.clone(), entry.handle.clone(), entry.created_ms))
                .collect();
            (doomed, observe)
        };
        for (entry, reason) in &doomed {
            self.host.kill(&entry.handle, reason);
            result.killed += 1;
        }
        // B: observe (no lock held: the host reads screens and process state).
        let observed: Vec<(u64, Health)> = observe
            .iter()
            .map(|(id, key, handle, created)| (*id, self.host.check(key, handle, now_ms - created)))
            .collect();
        let mut broken: Vec<Entry> = Vec::new();
        {
            let mut inner = self.shared.inner.lock();
            for (id, health) in observed {
                let Some(index) = inner.entries.iter().position(|entry| entry.id == id) else {
                    continue; // claimed while we were looking
                };
                let age = now_ms - inner.entries[index].created_ms;
                let entry = &mut inner.entries[index];
                let consecutive = now_ms - entry.last_observed_ms >= READY_RECHECK_MS;
                entry.health = health;
                match health {
                    Health::Ready => {
                        if entry.state == EntryState::Starting {
                            if consecutive || entry.ready_checks == 0 {
                                entry.ready_checks += 1;
                                entry.last_observed_ms = now_ms;
                            }
                            if entry.ready_checks >= 2 {
                                entry.state = EntryState::Ready;
                                entry.ready_since_ms = Some(now_ms);
                                let key = entry.key.clone();
                                inner.backoff.remove(&key);
                            }
                        }
                    }
                    Health::Starting => {
                        entry.ready_checks = 0;
                        if entry.state == EntryState::Ready {
                            entry.state = EntryState::Starting;
                            entry.ready_since_ms = None;
                        }
                        if age >= STARTUP_TIMEOUT_MS {
                            let entry = inner.entries.remove(index);
                            fail(&mut inner, &entry.key, "startup_timeout", now_ms);
                            inner.stats.discarded += 1;
                            broken.push(entry);
                        }
                    }
                    Health::Dialog | Health::NeedsAttention | Health::Dead => {
                        let entry = inner.entries.remove(index);
                        fail(&mut inner, &entry.key, health.as_str(), now_ms);
                        inner.stats.discarded += 1;
                        broken.push(entry);
                    }
                }
            }
        }
        for entry in &broken {
            self.host.kill(&entry.handle, "unhealthy");
            result.killed += 1;
        }
        // C: spawn what is missing, one at a time.
        if now_ms < self.spawn_not_before_ms {
            return result;
        }
        loop {
            let next = {
                let mut inner = self.shared.inner.lock();
                if inner.shut_down {
                    break;
                }
                match wanted(&inner, now_ms) {
                    Some((key, cwd)) => {
                        inner.spawning.insert(key.clone());
                        Some((key, cwd))
                    }
                    None => None,
                }
            };
            let Some((key, cwd)) = next else {
                break;
            };
            let spawned = self.host.spawn(&key, &cwd);
            let mut inner = self.shared.inner.lock();
            inner.spawning.remove(&key);
            match spawned {
                Ok(handle) => {
                    if inner.shut_down || !inner.config.allows(&key) {
                        drop(inner);
                        self.host.kill(&handle, "pool_config_changed");
                        result.killed += 1;
                        continue;
                    }
                    let id = inner.next_id;
                    inner.next_id += 1;
                    inner.stats.spawned += 1;
                    inner.entries.push(Entry {
                        id,
                        key: key.clone(),
                        cwd,
                        handle,
                        state: EntryState::Starting,
                        created_ms: now_ms,
                        ready_since_ms: None,
                        ready_checks: 0,
                        last_observed_ms: i64::MIN / 2,
                        health: Health::Starting,
                    });
                    result.spawned.push(key);
                }
                Err(error) => {
                    inner.stats.spawn_failed += 1;
                    fail(&mut inner, &key, &format!("spawn_failed: {error}"), now_ms);
                }
            }
        }
        result
    }

    /// Shut down every standby of `agent_type` (its definition changed or went away, so a warm
    /// worker would run the old one). Returns how many were discarded; their keys stay allowed
    /// and the next `reconcile` spawns fresh ones.
    pub fn discard_agent(&self, agent_type: &str, reason: &str) -> usize {
        let doomed: Vec<Entry> = {
            let mut inner = self.shared.inner.lock();
            let (doomed, keep): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut inner.entries)
                .into_iter()
                .partition(|entry| entry.key.agent_type.eq_ignore_ascii_case(agent_type));
            inner.entries = keep;
            inner.stats.discarded += doomed.len() as u64;
            doomed
        };
        for entry in &doomed {
            self.host.kill(&entry.handle, reason);
        }
        if !doomed.is_empty() {
            self.shared.wake.notify();
        }
        doomed.len()
    }

    /// Shut every standby down and refuse new ones (app exit).
    pub fn shutdown(&self) {
        let entries = {
            let mut inner = self.shared.inner.lock();
            inner.shut_down = true;
            std::mem::take(&mut inner.entries)
        };
        for entry in &entries {
            self.host.kill(&entry.handle, "app_shutdown");
        }
        self.shared.wake.notify();
    }

    pub fn is_shut_down(&self) -> bool {
        self.shared.inner.lock().shut_down
    }

    pub fn health_summary(&self) -> HealthSummary {
        let inner = self.shared.inner.lock();
        let counts = counts(&inner);
        HealthSummary {
            enabled: inner.config.enabled,
            total: counts.total,
            ready: counts.ready,
            starting: counts.starting,
        }
    }

    pub fn status(&self, now_ms: i64) -> StatusView {
        let inner = self.shared.inner.lock();
        let idle_ms = inner.config.idle_timeout_secs as i64 * 1000;
        let standbys = inner
            .entries
            .iter()
            .map(|entry| StandbyView {
                id: entry.id,
                agent_type: entry.key.agent_type.clone(),
                project: entry.cwd.clone(),
                headless: entry.key.headless,
                read_only: entry.key.read_only,
                model: entry.key.model.clone(),
                state: entry.state,
                health: entry.health.as_str(),
                age_ms: now_ms - entry.created_ms,
                ready_for_ms: entry.ready_since_ms.map(|since| now_ms - since),
                idle_expires_in_ms: entry.ready_since_ms.map(|since| (since + idle_ms) - now_ms),
                pane_id: match &entry.handle {
                    Handle::Pane(id) => Some(id.clone()),
                    Handle::Process(_) => None,
                },
            })
            .collect();
        let mut blocked: Vec<BlockedView> = inner
            .backoff
            .iter()
            .map(|(key, backoff)| BlockedView {
                key: key.label(),
                reason: backoff.last.clone(),
                fails: backoff.fails,
                retry_in_ms: (backoff.retry_at_ms - now_ms).max(0),
                cold: inner.suppressed.contains(key),
            })
            .collect();
        for key in &inner.suppressed {
            if !inner.backoff.contains_key(key) {
                blocked.push(BlockedView {
                    key: key.label(),
                    reason: "idle_timeout".into(),
                    fails: 0,
                    retry_in_ms: 0,
                    cold: true,
                });
            }
        }
        blocked.sort_by(|a, b| a.key.cmp(&b.key));
        StatusView {
            enabled: inner.config.enabled,
            config: inner.config.clone(),
            counts: counts(&inner),
            standbys,
            blocked,
            recent: inner
                .recent
                .iter()
                .map(|recent| RecentView {
                    key: recent.key.label(),
                    age_ms: now_ms - recent.at_ms,
                })
                .collect(),
            stats: inner.stats.clone(),
            supported: crate::tui::standby_backends()
                .into_iter()
                .map(|(agent_type, support)| SupportedBackend {
                    agent_type: agent_type.to_string(),
                    visible: support.visible,
                    headless: support.headless,
                })
                .collect(),
        }
    }
}

fn counts(inner: &Inner) -> Counts {
    Counts {
        total: inner.entries.len(),
        ready: inner
            .entries
            .iter()
            .filter(|entry| entry.state == EntryState::Ready)
            .count(),
        starting: inner
            .entries
            .iter()
            .filter(|entry| entry.state == EntryState::Starting)
            .count(),
    }
}

fn fail(inner: &mut Inner, key: &PoolKey, reason: &str, now_ms: i64) {
    let backoff = inner.backoff.entry(key.clone()).or_insert(Backoff {
        fails: 0,
        retry_at_ms: now_ms,
        last: String::new(),
    });
    backoff.fails += 1;
    let delay = (BACKOFF_BASE_MS << (backoff.fails - 1).min(8)).min(BACKOFF_MAX_MS);
    backoff.retry_at_ms = now_ms + delay;
    backoff.last = reason.to_string();
    if backoff.fails >= MAX_FAILS {
        inner.suppressed.insert(key.clone());
    }
}

/// The next key worth warming, if any: a recent key of a recent project, allowed by the config,
/// not already warm or being spawned, not backing off or cold, while the global cap has room.
fn wanted(inner: &Inner, now_ms: i64) -> Option<(PoolKey, String)> {
    if !inner.config.enabled || inner.entries.len() + inner.spawning.len() >= inner.config.max_total {
        return None;
    }
    let horizon = inner.config.recent_hours as i64 * 3_600_000;
    let mut projects: Vec<&str> = Vec::new();
    let mut recent: Vec<&RecentUse> = inner.recent.iter().collect();
    recent.sort_by_key(|entry| std::cmp::Reverse(entry.at_ms));
    for entry in &recent {
        if now_ms - entry.at_ms > horizon {
            continue;
        }
        if !projects.contains(&entry.key.project.as_str()) {
            projects.push(&entry.key.project);
        }
    }
    projects.truncate(inner.config.recent_projects);
    recent.into_iter().find_map(|entry| {
        let key = &entry.key;
        let eligible = projects.contains(&key.project.as_str())
            && now_ms - entry.at_ms <= horizon
            && inner.config.allows(key)
            && !inner.suppressed.contains(key)
            && !inner.spawning.contains(key)
            && !inner.entries.iter().any(|existing| existing.key == *key)
            && inner
                .backoff
                .get(key)
                .map_or(true, |backoff| backoff.retry_at_ms <= now_ms);
        eligible.then(|| (key.clone(), entry.cwd.clone()))
    })
}
