//! Pool rules, tested against a fake host and a hand-driven clock.

use super::config::{BackendToggle, StandbyConfig};
use super::key::PoolKey;
use super::pool::{ClaimOutcome, EntryState, Handle, Health, Host, MissReason, Pool};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct FakeHost {
    spawned: Mutex<Vec<(PoolKey, Handle)>>,
    killed: Mutex<Vec<(Handle, String)>>,
    health: Mutex<HashMap<Handle, Health>>,
    default_health: Mutex<Option<Health>>,
    fail_spawn: Mutex<bool>,
    counter: Mutex<u32>,
}

impl FakeHost {
    fn set_health(&self, handle: &Handle, health: Health) {
        self.health.lock().insert(handle.clone(), health);
    }
    fn spawn_count(&self) -> usize {
        self.spawned.lock().len()
    }
    fn kills(&self) -> Vec<(Handle, String)> {
        self.killed.lock().clone()
    }
    fn last_handle(&self) -> Handle {
        self.spawned.lock().last().unwrap().1.clone()
    }
}

impl Host for FakeHost {
    fn spawn(&self, key: &PoolKey, _cwd: &str) -> Result<Handle, String> {
        if *self.fail_spawn.lock() {
            return Err("boom".into());
        }
        let mut counter = self.counter.lock();
        *counter += 1;
        let handle = Handle::Pane(format!("fake-{}", *counter));
        self.spawned.lock().push((key.clone(), handle.clone()));
        Ok(handle)
    }
    fn check(&self, _key: &PoolKey, handle: &Handle, _age_ms: i64) -> Health {
        if let Some(health) = self.health.lock().get(handle) {
            return *health;
        }
        self.default_health.lock().unwrap_or(Health::Ready)
    }
    fn kill(&self, handle: &Handle, reason: &str) {
        self.killed.lock().push((handle.clone(), reason.to_string()));
    }
}

fn config(backends: &[(&str, bool, bool)]) -> StandbyConfig {
    let mut config = StandbyConfig {
        enabled: true,
        idle_timeout_secs: 60,
        ..Default::default()
    };
    for (agent, visible, headless) in backends {
        config.backends.insert(
            (*agent).into(),
            BackendToggle {
                visible: *visible,
                headless: *headless,
            },
        );
    }
    config.clamped()
}

fn key(agent: &str, project: &str) -> PoolKey {
    PoolKey::new(agent, project, None, false, false, false)
}

fn pool(host: &Arc<FakeHost>, config: StandbyConfig) -> Pool {
    Pool::new(host.clone(), None, config, Vec::new(), 0)
}

/// Runs reconcile passes a second apart until the pool settles; returns the time reached.
fn warm_up(pool: &Pool, mut now: i64) -> i64 {
    for _ in 0..4 {
        pool.reconcile(now);
        now += 1_000;
    }
    now
}

fn hit(outcome: ClaimOutcome) -> super::pool::Claim {
    match outcome {
        ClaimOutcome::Hit(claim) => claim,
        ClaimOutcome::Miss(reason) => panic!("expected a standby, got a miss: {reason:?}"),
    }
}

fn miss(outcome: ClaimOutcome) -> MissReason {
    match outcome {
        ClaimOutcome::Miss(reason) => reason,
        ClaimOutcome::Hit(_) => panic!("expected a miss"),
    }
}

#[test]
fn a_matching_key_claims_the_warm_standby() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("cursor_agent", true, false)]));
    let key = key("cursor_agent", "/work/a");
    pool.note_run(&key, "/work/a", 0);
    let now = warm_up(&pool, 0);
    assert_eq!(pool.status(now).counts.ready, 1);
    let claim = hit(pool.claim(&key, now));
    assert_eq!(claim.handle(), &host.last_handle());
    assert!(claim.age_ms() > 0);
    assert_eq!(claim.commit(), host.last_handle());
    assert!(host.kills().is_empty(), "a committed claim keeps the worker");
    let status = pool.status(now);
    assert_eq!(status.counts.total, 0, "claiming removes it from the pool");
    assert_eq!(status.stats.claimed, 1);
}

#[test]
fn a_standby_is_not_claimable_until_seen_ready_twice() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false)]));
    let key = key("claude", "/w");
    pool.note_run(&key, "/w", 0);
    pool.reconcile(0); // spawns, still Starting
    assert_eq!(miss(pool.claim(&key, 10)), MissReason::NotReady);
    pool.reconcile(1_000); // first Ready observation
    assert_eq!(miss(pool.claim(&key, 1_001)), MissReason::NotReady);
    pool.reconcile(2_000); // second, 1s later: promoted
    hit(pool.claim(&key, 2_001)).commit();
}

#[test]
fn a_run_with_different_options_never_gets_the_standby() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, true)]));
    let key = key("claude", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    for other in [
        PoolKey::new("claude", "/w", Some("opus"), false, false, false),
        PoolKey::new("claude", "/w", None, true, false, false),
        PoolKey::new("claude", "/w", None, false, true, false),
        PoolKey::new("claude", "/w", None, false, false, true),
        PoolKey::new("claude", "/other", None, false, false, false),
        PoolKey::new("cursor_agent", "/w", None, false, false, false),
    ] {
        assert!(matches!(
            miss(pool.claim(&other, now)),
            MissReason::NoStandby | MissReason::Disabled
        ));
    }
    assert_eq!(pool.status(now).counts.ready, 1, "the standby is untouched");
    hit(pool.claim(&key, now)).commit();
}

#[test]
fn a_standby_that_fails_its_health_check_is_discarded_and_the_run_falls_back() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("cursor_agent", true, false)]));
    let key = key("cursor_agent", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    let handle = host.last_handle();
    host.set_health(&handle, Health::Dialog);
    assert_eq!(miss(pool.claim(&key, now)), MissReason::Unhealthy(Health::Dialog));
    assert_eq!(host.kills(), vec![(handle, "claim_health_check_failed".to_string())]);
    let status = pool.status(now);
    assert_eq!((status.counts.total, status.stats.discarded, status.stats.claimed), (0, 1, 0));
    assert_eq!(status.blocked[0].reason, "dialog");
    // The key backs off instead of respawning at once into the same problem.
    assert!(pool.reconcile(now).spawned.is_empty());
    assert_eq!(pool.reconcile(now + 16_000).spawned, vec![key]);
}

#[test]
fn only_one_of_many_concurrent_claims_wins() {
    let host = Arc::new(FakeHost::default());
    let pool = Arc::new(pool(&host, config(&[("cursor_agent", true, false)])));
    let key = key("cursor_agent", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (pool, key, barrier) = (pool.clone(), key.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                match pool.claim(&key, now) {
                    ClaimOutcome::Hit(claim) => Some(claim.commit()),
                    ClaimOutcome::Miss(_) => None,
                }
            })
        })
        .collect();
    let winners: Vec<Handle> = threads
        .into_iter()
        .filter_map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(winners.len(), 1, "exactly one caller gets the standby: {winners:?}");
    assert!(host.kills().is_empty());
}

#[test]
fn a_claimed_standby_is_owned_by_exactly_one_session() {
    // The pool hands out an unowned pane; the dispatcher registers the claimant with the same
    // ownership call every run-created pane uses, and a second session is refused.
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false)]));
    let key = key("claude", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    let Handle::Pane(pane_id) = hit(pool.claim(&key, now)).commit() else {
        panic!("a visible standby is a pane");
    };
    let (a, b) = (
        format!("standby-test-a-{pane_id}"),
        format!("standby-test-b-{pane_id}"),
    );
    assert!(!crate::mcp_sessions::session_controls_pane(&a, &pane_id), "nobody owns a standby");
    crate::mcp_sessions::register_owned_pane(&a, &pane_id).unwrap();
    assert!(crate::mcp_sessions::session_controls_pane(&a, &pane_id));
    assert!(crate::mcp_sessions::register_owned_pane(&b, &pane_id).is_err());
    assert!(!crate::mcp_sessions::session_controls_pane(&b, &pane_id));
}

#[test]
fn claiming_schedules_a_replacement() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("cursor_agent", true, false)]));
    let key = key("cursor_agent", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    assert_eq!(host.spawn_count(), 1);
    // The claim wakes the maintenance loop...
    assert!(pool.wake_handle().wait(std::time::Duration::from_millis(1)));
    hit(pool.claim(&key, now)).commit();
    assert!(pool.wake_handle().wait(std::time::Duration::from_millis(1)), "claim must wake the loop");
    // ...and the next pass spawns exactly one replacement.
    assert_eq!(pool.reconcile(now).spawned, vec![key.clone()]);
    assert_eq!(host.spawn_count(), 2);
    assert!(pool.reconcile(now).spawned.is_empty(), "target size is one");
}

#[test]
fn only_the_most_recent_projects_inside_the_window_are_warmed() {
    let host = Arc::new(FakeHost::default());
    let mut cfg = config(&[("claude", true, false)]);
    cfg.recent_projects = 2;
    cfg.recent_hours = 1;
    cfg.max_total = 8;
    let pool = pool(&host, cfg);
    let hour = 3_600_000;
    let now = 10 * hour;
    pool.note_run(&key("claude", "/old"), "/old", now - 2 * hour); // outside the window
    pool.note_run(&key("claude", "/third"), "/third", now - 3_000);
    pool.note_run(&key("claude", "/second"), "/second", now - 2_000);
    pool.note_run(&key("claude", "/first"), "/first", now - 1_000);
    pool.reconcile(now);
    let mut warmed: Vec<String> = host.spawned.lock().iter().map(|(key, _)| key.project.clone()).collect();
    warmed.sort();
    assert_eq!(warmed, vec!["/first".to_string(), "/second".to_string()]);
}

#[test]
fn the_global_cap_bounds_standbys_across_keys() {
    let host = Arc::new(FakeHost::default());
    let mut cfg = config(&[("claude", true, false), ("cursor_agent", true, false)]);
    cfg.max_total = 2;
    cfg.recent_projects = 2;
    let pool = pool(&host, cfg);
    for (agent, project, at) in [
        ("claude", "/a", 1),
        ("cursor_agent", "/a", 2),
        ("claude", "/b", 3),
        ("cursor_agent", "/b", 4),
    ] {
        pool.note_run(&key(agent, project), project, at);
    }
    pool.reconcile(10);
    pool.reconcile(10);
    assert_eq!(host.spawn_count(), 2);
    assert_eq!(pool.status(10).counts.total, 2);
}

#[test]
fn an_idle_standby_expires_and_stays_cold_until_the_next_run() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("cursor_agent", true, false)])); // idle timeout 60s
    let key = key("cursor_agent", "/w");
    pool.note_run(&key, "/w", 0);
    let ready_at = warm_up(&pool, 0);
    let first = host.last_handle();
    let outcome = pool.reconcile(ready_at + 30_000);
    assert_eq!(outcome.killed, 0, "not idle long enough yet");
    let outcome = pool.reconcile(ready_at + 61_000);
    assert_eq!(outcome.killed, 1);
    assert_eq!(host.kills(), vec![(first, "idle_timeout".to_string())]);
    assert!(outcome.spawned.is_empty(), "an idle app does not respawn what it just expired");
    assert!(pool.reconcile(ready_at + 120_000).spawned.is_empty());
    let status = pool.status(ready_at + 120_000);
    assert!(status.blocked.iter().any(|blocked| blocked.cold && blocked.reason == "idle_timeout"));
    assert_eq!(status.stats.expired, 1);
    // The next run of that key warms it again.
    pool.note_run(&key, "/w", ready_at + 130_000);
    assert_eq!(pool.reconcile(ready_at + 130_000).spawned, vec![key]);
}

#[test]
fn a_disabled_pool_does_nothing_and_never_hands_out_a_worker() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, StandbyConfig::default());
    let key = key("cursor_agent", "/w");
    pool.note_run(&key, "/w", 0);
    assert!(pool.reconcile(0).spawned.is_empty());
    assert!(pool.reconcile(10_000).spawned.is_empty());
    assert_eq!(miss(pool.claim(&key, 10_000)), MissReason::Disabled);
    assert_eq!(host.spawn_count(), 0);
    assert_eq!(pool.status(0).stats.missed, 0, "a disabled pool does not even count misses");
}

#[test]
fn disabling_the_pool_or_a_backend_kills_its_standbys() {
    let host = Arc::new(FakeHost::default());
    let cfg = config(&[("cursor_agent", true, false), ("claude", true, false)]);
    let pool = pool(&host, cfg.clone());
    pool.note_run(&key("cursor_agent", "/w"), "/w", 1);
    pool.note_run(&key("claude", "/w"), "/w", 2);
    warm_up(&pool, 10);
    assert_eq!(pool.status(20).counts.total, 2);
    let mut only_claude = cfg.clone();
    only_claude.backends.get_mut("cursor_agent").unwrap().visible = false;
    pool.set_config(only_claude);
    assert_eq!(pool.status(20).counts.total, 1);
    assert_eq!(host.kills().len(), 1);
    let mut off = cfg;
    off.enabled = false;
    pool.set_config(off);
    assert_eq!(pool.status(20).counts.total, 0);
    assert_eq!(host.kills().len(), 2);
    assert!(pool.reconcile(30_000).spawned.is_empty());
}

#[test]
fn a_claim_given_back_unused_shuts_the_worker_down() {
    // Cancel (or any failure) between claim and hand-over must not leak the worker.
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false)]));
    let key = key("claude", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    let claim = hit(pool.claim(&key, now));
    let handle = claim.handle().clone();
    drop(claim);
    assert_eq!(host.kills(), vec![(handle, "claim_abandoned".to_string())]);
    assert_eq!(pool.status(now).stats.abandoned, 1);
}

#[test]
fn a_folder_that_shows_a_dialog_is_not_pooled_and_goes_cold_after_repeated_failures() {
    let host = Arc::new(FakeHost::default());
    *host.default_health.lock() = Some(Health::Dialog);
    let pool = pool(&host, config(&[("cursor_agent", true, false)]));
    let key = key("cursor_agent", "/untrusted");
    pool.note_run(&key, "/untrusted", 0);
    let mut now = 0;
    for _ in 0..3 {
        pool.reconcile(now); // spawn
        pool.reconcile(now + 1); // observe the dialog: discarded
        now += 301_000; // past any backoff
    }
    let spawned = host.spawn_count();
    assert_eq!(spawned, 3);
    assert!(pool.reconcile(now + 1_000_000).spawned.is_empty(), "cold after 3 failures");
    assert_eq!(host.kills().len(), 3, "every dialog standby was shut down");
    // The user trusted the folder meanwhile; their next run revives the key.
    *host.default_health.lock() = None;
    pool.note_run(&key, "/untrusted", now + 2_000_000);
    assert_eq!(pool.reconcile(now + 2_000_000).spawned, vec![key]);
}

#[test]
fn a_spawn_failure_backs_off() {
    let host = Arc::new(FakeHost::default());
    *host.fail_spawn.lock() = true;
    let pool = pool(&host, config(&[("claude", true, false)]));
    let key = key("claude", "/w");
    pool.note_run(&key, "/w", 0);
    assert!(pool.reconcile(0).spawned.is_empty());
    assert_eq!(pool.status(0).stats.spawn_failed, 1);
    *host.fail_spawn.lock() = false;
    assert!(pool.reconcile(1_000).spawned.is_empty(), "backing off");
    assert_eq!(pool.reconcile(16_000).spawned, vec![key]);
}

#[test]
fn a_standby_that_dies_is_replaced() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false)]));
    let key = key("claude", "/w");
    pool.note_run(&key, "/w", 0);
    let now = warm_up(&pool, 0);
    host.set_health(&host.last_handle(), Health::Dead);
    pool.reconcile(now);
    assert_eq!(pool.status(now).counts.total, 0);
    assert_eq!(pool.reconcile(now + 16_000).spawned, vec![key]);
}

#[test]
fn shutdown_kills_everything_and_refuses_new_work() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("cursor_agent", true, false), ("claude", true, false)]));
    pool.note_run(&key("cursor_agent", "/w"), "/w", 1);
    pool.note_run(&key("claude", "/w"), "/w", 2);
    let now = warm_up(&pool, 10);
    assert_eq!(pool.status(now).counts.total, 2);
    pool.shutdown();
    assert_eq!(host.kills().len(), 2);
    assert!(host.kills().iter().all(|(_, reason)| reason == "app_shutdown"));
    assert!(pool.reconcile(now + 100_000).spawned.is_empty());
    assert_eq!(miss(pool.claim(&key("claude", "/w"), now)), MissReason::Disabled);
}

#[test]
fn the_status_view_describes_each_standby_and_the_health_summary_counts_them() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false)]));
    pool.note_run(&key("claude", "/w"), "/w", 0);
    pool.reconcile(0);
    let starting = pool.status(500);
    assert_eq!(starting.standbys[0].state, EntryState::Starting);
    assert_eq!(starting.standbys[0].agent_type, "claude");
    let summary = pool.health_summary();
    assert_eq!((summary.enabled, summary.total, summary.ready, summary.starting), (true, 1, 0, 1));
    let now = warm_up(&pool, 1_000);
    let view = pool.status(now);
    assert_eq!(view.standbys[0].state, EntryState::Ready);
    assert_eq!(view.standbys[0].health, "ready");
    assert!(view.standbys[0].idle_expires_in_ms.unwrap() > 0);
    assert!(view.standbys[0].pane_id.is_some());
    assert_eq!(pool.health_summary().ready, 1);
}

/// `standby-pool.json` exactly as the previous build wrote it (`config::save` output).
const LEGACY_POOL_FILE: &str = r#"{
  "config": {
    "enabled": true,
    "backends": {
      "claude": {
        "visible": true,
        "headless": true
      },
      "cursor_agent": {
        "visible": true,
        "headless": false
      }
    },
    "max_total": 3,
    "recent_projects": 2,
    "recent_hours": 6,
    "idle_timeout_secs": 1200
  },
  "recent": [
    {
      "key": {
        "agent_type": "claude",
        "project": "/p",
        "model": null,
        "read_only": false,
        "allow_shell": false,
        "headless": true
      },
      "cwd": "/p",
      "at_ms": 5
    },
    {
      "key": {
        "agent_type": "cursor_agent",
        "project": "/q",
        "model": null,
        "read_only": true,
        "allow_shell": false,
        "headless": false
      },
      "cwd": "/q",
      "at_ms": 7
    }
  ]
}"#;

#[test]
fn loads_legacy_pool_file() {
    let dir = std::env::temp_dir().join(format!("pm-standby-legacy-{}", uuid::Uuid::new_v4()));
    let path = dir.join("standby-pool.json");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, LEGACY_POOL_FILE).unwrap();

    let (loaded, recent) = super::config::load(&path);
    assert!(loaded.enabled);
    assert_eq!(
        loaded.backends.keys().map(String::as_str).collect::<Vec<_>>(),
        ["claude", "cursor_agent"]
    );
    assert_eq!(loaded.backends["claude"], BackendToggle { visible: true, headless: true });
    assert_eq!(loaded.backends["cursor_agent"], BackendToggle { visible: true, headless: false });
    assert_eq!(
        recent.iter().map(|r| r.key.clone()).collect::<Vec<_>>(),
        [
            PoolKey::new("claude", "/p", None, false, false, true),
            PoolKey::new("cursor_agent", "/q", None, true, false, false),
        ]
    );
    assert!(loaded.allows(&recent[0].key));

    // Saving what was loaded writes the same file back, byte for byte.
    super::config::save(&path, &loaded, &recent).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LEGACY_POOL_FILE);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}
