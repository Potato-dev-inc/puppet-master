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
    project: Mutex<Option<String>>,
}

impl FakeHost {
    fn set_project(&self, project: &str) {
        *self.project.lock() = Some(project.to_string());
    }
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
    fn default_project(&self) -> Option<String> {
        Some(self.project.lock().clone().unwrap_or_else(|| "/w".to_string()))
    }
}

fn config(backends: &[(&str, bool, bool)]) -> StandbyConfig {
    let mut config = StandbyConfig {
        enabled: true,
        idle_timeout_secs: 60,
        ..Default::default()
    };
    config.preload.enabled = true;
    for (agent, visible, headless) in backends {
        if *visible {
            config.preload.providers.insert((*agent).into(), 1);
        }
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
    let key = key("cursor_agent", "/w");
    pool.note_run(&key, "/w", 0);
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
fn headless_standbys_warm_only_the_most_recent_projects_inside_the_window() {
    let host = Arc::new(FakeHost::default());
    let mut cfg = config(&[("claude", false, true)]);
    cfg.recent_projects = 2;
    cfg.recent_hours = 1;
    cfg.max_total = 8;
    let pool = pool(&host, cfg);
    let hour = 3_600_000;
    let now = 10 * hour;
    let hl = |project: &str| PoolKey::new("claude", project, None, false, false, true);
    pool.note_run(&hl("/old"), "/old", now - 2 * hour); // outside the window
    pool.note_run(&hl("/third"), "/third", now - 3_000);
    pool.note_run(&hl("/second"), "/second", now - 2_000);
    pool.note_run(&hl("/first"), "/first", now - 1_000);
    pool.reconcile(now);
    let mut warmed: Vec<String> = host.spawned.lock().iter().map(|(key, _)| key.project.clone()).collect();
    warmed.sort();
    assert_eq!(warmed, vec!["/first".to_string(), "/second".to_string()]);
}

#[test]
fn the_global_cap_bounds_standbys_across_keys() {
    let host = Arc::new(FakeHost::default());
    let mut cfg = config(&[("claude", false, true)]);
    cfg.max_total = 2;
    cfg.recent_projects = 3;
    let pool = pool(&host, cfg);
    for (project, at) in [("/a", 1), ("/b", 2), ("/c", 3)] {
        pool.note_run(&PoolKey::new("claude", project, None, false, false, true), project, at);
    }
    pool.reconcile(10);
    pool.reconcile(10);
    assert_eq!(host.spawn_count(), 2);
    assert_eq!(pool.status(10).counts.total, 2);
}

#[test]
fn preload_warms_each_ticked_provider_in_the_default_project_without_any_run() {
    let host = Arc::new(FakeHost::default());
    host.set_project("/home/proj");
    let pool = pool(&host, config(&[("claude", true, false), ("cursor_agent", true, false)]));
    let spawned = pool.reconcile(0).spawned;
    assert_eq!(spawned.len(), 2);
    assert!(spawned.iter().all(|k| k.project == "/home/proj" && !k.headless));
    assert!(pool.reconcile(0).spawned.is_empty(), "one each");
    assert!(pool.ready_agents().is_empty(), "still starting");
    let now = warm_up(&pool, 1_000);
    assert_eq!(pool.ready_agents(), vec!["claude".to_string(), "cursor_agent".to_string()]);
    hit(pool.claim(&key("claude", "/home/proj"), now)).commit();
    assert_eq!(pool.ready_agents(), vec!["cursor_agent".to_string()]);
}

#[test]
fn preload_keeps_the_requested_count_per_provider_and_replaces_after_a_claim() {
    let host = Arc::new(FakeHost::default());
    let mut cfg = config(&[("claude", true, false)]);
    cfg.preload.providers.insert("claude".into(), 2);
    let pool = pool(&host, cfg.clamped());
    pool.reconcile(0);
    assert_eq!(host.spawn_count(), 2, "two warm workers for count 2");
    let now = warm_up(&pool, 1_000);
    assert_eq!(pool.status(now).counts.ready, 2);
    hit(pool.claim(&key("claude", "/w"), now)).commit();
    // Replacement scheduling: the claim woke the loop and the next pass tops it back up to 2.
    assert!(pool.wake_handle().wait(std::time::Duration::from_millis(1)));
    assert_eq!(pool.reconcile(now).spawned.len(), 1);
    assert_eq!(host.spawn_count(), 3);
    assert!(pool.reconcile(now).spawned.is_empty());
}

#[test]
fn a_claim_takes_the_ready_worker_even_when_a_sibling_is_still_starting() {
    let host = Arc::new(FakeHost::default());
    let mut cfg = config(&[("claude", true, false)]);
    cfg.preload.providers.insert("claude".into(), 2);
    let pool = pool(&host, cfg.clamped());
    pool.reconcile(0); // two spawned
    let first = host.spawned.lock()[0].1.clone();
    let second = host.spawned.lock()[1].1.clone();
    host.set_health(&second, Health::Starting);
    let now = warm_up(&pool, 1_000);
    let claim = hit(pool.claim(&key("claude", "/w"), now));
    assert_eq!(claim.handle(), &first);
    claim.commit();
}

#[test]
fn preload_off_or_without_a_default_project_warms_nothing() {
    struct NoProject;
    impl Host for NoProject {
        fn spawn(&self, _: &PoolKey, _: &str) -> Result<Handle, String> {
            panic!("must not spawn")
        }
        fn check(&self, _: &PoolKey, _: &Handle, _: i64) -> Health {
            Health::Dead
        }
        fn kill(&self, _: &Handle, _: &str) {}
    }
    let pool = Pool::new(Arc::new(NoProject), None, config(&[("claude", true, false)]), Vec::new(), 0);
    assert!(pool.reconcile(0).spawned.is_empty(), "no default project");
    let host = Arc::new(FakeHost::default());
    let mut off = config(&[("claude", true, false)]);
    off.preload.enabled = false;
    let pool = Pool::new(host.clone(), None, off, Vec::new(), 0);
    assert!(pool.reconcile(0).spawned.is_empty());
    assert_eq!(host.spawn_count(), 0);
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
    only_claude.preload.providers.remove("cursor_agent");
    pool.set_config(only_claude);
    assert_eq!(pool.status(20).counts.total, 1);
    assert_eq!(host.kills().len(), 1);
    let mut off = cfg;
    off.preload.enabled = false;
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
    host.set_project("/untrusted");
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
    assert!(loaded.preload.enabled, "the old master switch becomes Preload's");
    assert_eq!(loaded.preload.providers.get("claude"), Some(&1));
    assert_eq!(loaded.preload.providers.get("cursor_agent"), Some(&1));
    assert_eq!(
        loaded.backends.keys().map(String::as_str).collect::<Vec<_>>(),
        ["claude", "cursor_agent"]
    );
    assert_eq!(loaded.backends["claude"], BackendToggle { visible: false, headless: true });
    assert_eq!(loaded.backends["cursor_agent"], BackendToggle { visible: false, headless: false });
    assert_eq!(
        recent.iter().map(|r| r.key.clone()).collect::<Vec<_>>(),
        [
            PoolKey::new("claude", "/p", None, false, false, true),
            PoolKey::new("cursor_agent", "/q", None, true, false, false),
        ]
    );
    assert!(loaded.allows(&recent[0].key));

    // The migration is stable: what was loaded saves and loads back equal (no second migration).
    super::config::save(&path, &loaded, &recent).unwrap();
    assert_eq!(super::config::load(&path), (loaded, recent));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn fingerprint_change_discards_workers() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false), ("cursor_agent", true, false)]));
    let claude = key("claude", "/w");
    let cursor = key("cursor_agent", "/w");
    pool.note_run(&claude, "/w", 0);
    pool.note_run(&cursor, "/w", 0);
    let now = warm_up(&pool, 0);
    assert_eq!(pool.status(now).counts.ready, 2);

    // The definition of one agent changed: only its workers go.
    assert_eq!(pool.discard_agent("CLAUDE", "definition_changed"), 1);
    let kills = host.kills();
    assert_eq!(kills.len(), 1);
    assert_eq!(kills[0].1, "definition_changed");
    assert_eq!(pool.status(now).counts.total, 1);
    assert!(matches!(pool.claim(&claude, now), ClaimOutcome::Miss(MissReason::NoStandby)));
    hit(pool.claim(&cursor, now)).commit();
    assert_eq!(pool.discard_agent("nothing_here", "definition_changed"), 0);

    // The key stays allowed: the next pass warms a replacement.
    let later = warm_up(&pool, now + 1_000);
    hit(pool.claim(&claude, later)).commit();
}

#[test]
fn a_warm_worker_whose_project_is_no_longer_the_default_is_replaced() {
    let host = Arc::new(FakeHost::default());
    host.set_project("/old");
    let pool = pool(&host, config(&[("claude", true, false)]));
    pool.reconcile(0);
    let stranded = host.last_handle();
    host.set_project("/new");
    let outcome = pool.reconcile(10);
    assert_eq!(outcome.killed, 1);
    assert_eq!(host.kills(), vec![(stranded, "project_changed".to_string())]);
    assert_eq!(outcome.spawned.len(), 1);
    assert_eq!(outcome.spawned[0].project, "/new");
}

// ---- Bug #12: warm-standby churn. A worker that is unhealthy right after it starts must not be
// respawned in a loop, however the pool is nudged (a new project folder, a settings save, a run).

/// One spawn-then-observe cycle at `now`; returns how many workers were spawned in it.
fn cycle(pool: &Pool, host: &Arc<FakeHost>, now: i64) -> usize {
    let before = host.spawn_count();
    pool.reconcile(now); // observe what is there, then spawn what is missing
    pool.reconcile(now + 1); // observe the new worker: unhealthy
    host.spawn_count() - before
}

#[test]
fn an_unhealthy_agent_backs_off_then_stops_and_says_why() {
    let host = Arc::new(FakeHost::default());
    *host.default_health.lock() = Some(Health::NeedsAttention);
    let pool = pool(&host, config(&[("codex", true, false)]));
    let mut now = 0;
    assert_eq!(cycle(&pool, &host, now), 1, "first spawn");
    let blocked = &pool.status(now + 1).agents_blocked;
    assert_eq!(blocked.len(), 1);
    assert_eq!((blocked[0].fails, blocked[0].cold), (1, false));
    assert_eq!(blocked[0].reason, "needs_attention");
    // Schedule: 15s after the first failure, 30s after the second.
    now = 14_000;
    assert_eq!(cycle(&pool, &host, now), 0, "still inside the 15s backoff");
    now = 16_000;
    assert_eq!(cycle(&pool, &host, now), 1, "second spawn after 15s");
    let retry = pool.status(now + 1).agents_blocked[0].retry_in_ms;
    assert!((29_000..=30_000).contains(&retry), "30s backoff, got {retry}");
    now = 40_000;
    assert_eq!(cycle(&pool, &host, now), 0, "inside the 30s backoff");
    now = 47_000;
    assert_eq!(cycle(&pool, &host, now), 1, "third spawn after 30s");
    // Cap: three unhealthy workers in a row and the agent stops being warmed.
    let blocked = pool.status(now + 1).agents_blocked[0].clone();
    assert!(blocked.cold, "gave up: {blocked:?}");
    assert!(blocked.message.contains("login or update"), "{}", blocked.message);
    for hours in 1..=5 {
        assert_eq!(cycle(&pool, &host, now + hours * 3_600_000), 0, "cold agents are not respawned");
    }
    assert_eq!(host.spawn_count(), 3);
}

#[test]
fn a_new_project_folder_does_not_restart_the_agent_schedule() {
    // The live loop: the default project moved, each folder was a fresh key with a fresh backoff.
    let host = Arc::new(FakeHost::default());
    *host.default_health.lock() = Some(Health::Dialog);
    let pool = pool(&host, config(&[("cursor_agent", true, false)]));
    let mut now = 0;
    let mut spawned = 0;
    for folder in 0..40 {
        host.set_project(&format!("/folder-{folder}"));
        spawned += cycle(&pool, &host, now);
        now += 31_000;
    }
    assert_eq!(spawned, 3, "three tries across all folders, then cold");
    assert!(pool.status(now).agents_blocked[0].cold);
}

#[test]
fn a_run_or_changed_settings_allow_one_more_try_but_never_before_the_backoff() {
    let host = Arc::new(FakeHost::default());
    *host.default_health.lock() = Some(Health::Dialog);
    let cfg = config(&[("codex", true, false)]);
    let pool = pool(&host, cfg.clone());
    let mut now = 0;
    for _ in 0..3 {
        cycle(&pool, &host, now);
        now += 301_000;
    }
    assert_eq!(host.spawn_count(), 3);
    assert!(pool.status(now).agents_blocked[0].cold);
    // Saving identical settings (the Settings tab does this often) changes nothing.
    pool.set_config(cfg.clone());
    assert_eq!(cycle(&pool, &host, now), 0, "an unchanged save does not revive it");
    // A run of the agent lifts the cold flag for ONE more try; failing again goes straight back.
    pool.note_run(&key("codex", "/w"), "/w", now);
    assert_eq!(cycle(&pool, &host, now), 1);
    assert!(pool.status(now + 1).agents_blocked[0].cold);
    assert_eq!(cycle(&pool, &host, now + 100_000_000), 0);
    // Right after a failure, even a run and a changed config cannot respawn it before the delay.
    let mut changed = cfg;
    changed.max_total += 1;
    pool.set_config(changed);
    pool.note_run(&key("codex", "/w"), "/w", now + 100_000_000);
    let t = now + 200_000_000;
    assert_eq!(cycle(&pool, &host, t), 1);
    pool.note_run(&key("codex", "/w"), "/w", t + 2_000);
    assert_eq!(cycle(&pool, &host, t + 3_000), 0, "inside the backoff set by the failure at t+1ms");
}

#[test]
fn reaching_ready_clears_the_agent_failures() {
    let host = Arc::new(FakeHost::default());
    *host.default_health.lock() = Some(Health::Dialog);
    let pool = pool(&host, config(&[("codex", true, false)]));
    cycle(&pool, &host, 0);
    assert_eq!(pool.status(2).agents_blocked.len(), 1);
    *host.default_health.lock() = None; // the user dealt with the prompt
    let now = warm_up(&pool, 20_000);
    assert_eq!(pool.status(now).counts.ready, 1);
    assert!(pool.status(now).agents_blocked.is_empty(), "a healthy worker clears the record");
}

// ---- strict mode (an untested CLI version, `crate::canary`) ---------------------------------

#[test]
fn an_agent_in_strict_mode_is_not_warmed_and_its_standby_is_shut_down() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false), ("cursor_agent", true, false)]));
    let strict_key = key("claude", "/w");
    let other_key = key("cursor_agent", "/w");
    pool.note_run(&strict_key, "/w", 0);
    pool.note_run(&other_key, "/w", 0);
    let now = warm_up(&pool, 0);
    assert_eq!(pool.status(now).counts.ready, 2);

    pool.set_strict_agents(&["Claude".to_string()]);
    let kills = host.kills();
    assert_eq!(kills.len(), 1, "only the strict agent's standby is shut down: {kills:?}");
    assert_eq!(kills[0].1, "untested_version");
    assert_eq!(miss(pool.claim(&strict_key, now)), MissReason::Disabled, "a claim finds nothing for it");

    let spawned = host.spawn_count();
    let now = warm_up(&pool, now);
    let status = pool.status(now);
    assert_eq!(host.spawn_count(), spawned, "nothing new is warmed for it");
    let blocked = status.agents_blocked.iter().find(|a| a.agent_type == "claude").expect("listed as held back");
    assert!(blocked.cold);
    assert_eq!(blocked.reason, "untested_version");
    assert!(blocked.message.contains("not been tested"), "{}", blocked.message);
    // The other agent keeps its warm worker.
    let _ = hit(pool.claim(&other_key, now));
}

#[test]
fn leaving_strict_mode_warms_the_agent_again_and_a_run_does_not_lift_it_early() {
    let host = Arc::new(FakeHost::default());
    let pool = pool(&host, config(&[("claude", true, false)]));
    let k = key("claude", "/w");
    pool.set_strict_agents(&["claude".to_string()]);
    pool.note_run(&k, "/w", 0);
    let mut now = warm_up(&pool, 0);
    assert_eq!(host.spawn_count(), 0, "a run of the agent does not make it warm while the version is untested");
    pool.set_strict_agents(&[]);
    now = warm_up(&pool, now);
    assert_eq!(pool.status(now).counts.ready, 1);
    let _ = hit(pool.claim(&k, now));
}

#[test]
fn a_faked_version_bump_turns_the_chip_on_and_the_standby_cold() {
    use crate::canary::engine::{Deps, Engine, Source};
    use crate::canary::model::{Facts, StrictReason};
    let _lock = crate::canary::test_lock();

    let host = Arc::new(FakeHost::default());
    let pool = Arc::new(pool(&host, config(&[("claude", true, false)])));
    let k = key("claude", "/w");
    pool.note_run(&k, "/w", 0);
    let now = warm_up(&pool, 0);
    assert_eq!(pool.status(now).counts.ready, 1, "a tested version keeps its warm worker");

    // The machine: Claude Code's version is read from a cell the test bumps.
    let version = Arc::new(Mutex::new("2.1.299 (Claude Code)".to_string()));
    let read = version.clone();
    let tested = crate::tui::profile_for("claude").tested_versions().map(|r| r.as_str().to_string());
    let engine = Engine::new(
        None,
        Deps {
            sources: Box::new(move || {
                vec![Source {
                    facts: Facts { id: "claude".into(), label: "Claude Code".into(), installed: true, tested: tested.clone() },
                    version_command: None,
                }]
            }),
            read_version: Box::new(move |_| Some(read.lock().clone())),
            run_check: Box::new(|_, _| unreachable!("this test only reads versions")),
            captures: Box::new(|_| 0),
            now_ms: Box::new(|| 0),
        },
    );
    let wired = pool.clone();
    engine.on_change(Box::new(move |_, strict| {
        crate::canary::strict::replace(strict.iter().cloned());
        wired.set_strict_agents(strict);
    }));

    engine.refresh();
    assert!(!engine.status().agents[0].strict);
    assert!(host.kills().is_empty());

    // The CLI updates to 2.4.0, outside the tested range.
    *version.lock() = "2.4.0 (Claude Code)".to_string();
    engine.refresh();
    let view = &engine.status().agents[0];
    assert_eq!(view.chip.as_deref(), Some("Untested version"));
    assert_eq!(view.strict_reason, Some(StrictReason::UntestedVersion));
    assert_eq!(host.kills().len(), 1, "its warm worker was shut down");
    assert_eq!(host.kills()[0].1, "untested_version");
    assert_eq!(miss(pool.claim(&k, now)), MissReason::Disabled);
    let status = pool.status(now);
    assert!(status.agents_blocked.iter().any(|a| a.agent_type == "claude" && a.cold));

    // Back to a tested version: the chip goes and the agent can be warmed again.
    *version.lock() = "2.1.300 (Claude Code)".to_string();
    engine.refresh();
    assert!(engine.status().agents[0].chip.is_none());
    let now = warm_up(&pool, now);
    assert_eq!(pool.status(now).counts.ready, 1);
    crate::canary::strict::replace(Vec::<String>::new());
}
