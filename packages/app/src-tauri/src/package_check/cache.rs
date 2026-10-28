//! In-memory cache of registry outcomes, per (ecosystem, name), with TTL per outcome kind and a cap
//! (SPEC-n33 section 3.8). Nothing is written to disk.
//!
//! TTL: Found 6 h, NotFound 2 min, Failed 30 s. An entry is fresh while `now - fetched < ttl`.
//! Past the cap the oldest entry (first stored) goes. `get_or_fetch` runs one fetch per key at a
//! time: a second caller for the same key waits for the first result instead of asking the registry.

use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::model::{
    CacheHit, Ecosystem, FetchError, Outcome, CACHE_CAP, TTL_FAILED_MS, TTL_FOUND_MS, TTL_NOT_FOUND_MS,
};

type Key = (Ecosystem, String);

struct Entry {
    hit: CacheHit,
    /// Store order: the smallest is the oldest.
    seq: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Entry>,
    /// Keys with a fetch running now.
    in_flight: HashSet<Key>,
    next_seq: u64,
}

pub struct Cache {
    cap: usize,
    state: Mutex<State>,
    /// Woken when a fetch finishes, so waiters can look again.
    done: Condvar,
}

fn ttl_ms(outcome: &Outcome) -> u64 {
    match outcome {
        Outcome::Found(_) => TTL_FOUND_MS,
        Outcome::NotFound => TTL_NOT_FOUND_MS,
        Outcome::Failed(_) => TTL_FAILED_MS,
    }
}

fn is_fresh(hit: &CacheHit, now_ms: u64) -> bool {
    now_ms.saturating_sub(hit.fetched_ms) < ttl_ms(&hit.outcome)
}

fn fresh_in(state: &State, key: &Key, now_ms: u64) -> Option<CacheHit> {
    state
        .entries
        .get(key)
        .filter(|entry| is_fresh(&entry.hit, now_ms))
        .map(|entry| entry.hit.clone())
}

fn store(state: &mut State, cap: usize, key: Key, hit: CacheHit) {
    if cap == 0 {
        return;
    }
    if !state.entries.contains_key(&key) && state.entries.len() >= cap {
        let oldest = state
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.seq)
            .map(|(k, _)| k.clone());
        if let Some(oldest) = oldest {
            state.entries.remove(&oldest);
        }
    }
    let seq = state.next_seq;
    state.next_seq += 1;
    state.entries.insert(key, Entry { hit, seq });
}

/// Clears the in-flight mark and wakes waiters when a fetch ends, also when the fetch panics.
struct Flight<'a> {
    cache: &'a Cache,
    key: Key,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        let mut state = self.cache.lock();
        state.in_flight.remove(&self.key);
        drop(state);
        self.cache.done.notify_all();
    }
}

impl Cache {
    pub fn new(cap: usize) -> Self {
        Self { cap, state: Mutex::new(State::default()), done: Condvar::new() }
    }

    /// The one cache the desktop app uses (cap `CACHE_CAP`).
    pub fn global() -> &'static Cache {
        static GLOBAL: OnceLock<Cache> = OnceLock::new();
        GLOBAL.get_or_init(|| Cache::new(CACHE_CAP))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The stored outcome, if it is still fresh at `now_ms`.
    pub fn get(&self, eco: Ecosystem, norm: &str, now_ms: u64) -> Option<CacheHit> {
        let state = self.lock();
        fresh_in(&state, &(eco, norm.to_string()), now_ms)
    }

    /// Store an outcome fetched at `now_ms`. Past the cap, the oldest entry goes.
    pub fn put(&self, eco: Ecosystem, norm: &str, outcome: Outcome, now_ms: u64) {
        let mut state = self.lock();
        store(&mut state, self.cap, (eco, norm.to_string()), CacheHit { outcome, fetched_ms: now_ms });
    }

    /// "Check again": drop a stored NotFound or Failed outcome. A Found outcome stays.
    pub fn forget_unsettled(&self, eco: Ecosystem, norm: &str) {
        let key = (eco, norm.to_string());
        let mut state = self.lock();
        let unsettled = state
            .entries
            .get(&key)
            .is_some_and(|entry| !matches!(entry.hit.outcome, Outcome::Found(_)));
        if unsettled {
            state.entries.remove(&key);
        }
    }

    /// Fresh stored outcome, or run `fetch` once for this key. A second caller for the same key
    /// waits for the first result, at most `wait_ms`; after that it gets `Failed(Timeout)`.
    pub fn get_or_fetch(
        &self,
        eco: Ecosystem,
        norm: &str,
        now_ms: u64,
        wait_ms: u64,
        fetch: &dyn Fn() -> Outcome,
    ) -> CacheHit {
        let key = (eco, norm.to_string());
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        let mut state = self.lock();
        loop {
            if let Some(hit) = fresh_in(&state, &key, now_ms) {
                return hit;
            }
            if !state.in_flight.contains(&key) {
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                return CacheHit { outcome: Outcome::Failed(FetchError::Timeout), fetched_ms: now_ms };
            }
            let (next, _) = self
                .done
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
        state.in_flight.insert(key.clone());
        drop(state);

        let flight = Flight { cache: self, key: key.clone() };
        let hit = CacheHit { outcome: fetch(), fetched_ms: now_ms };
        {
            let mut state = self.lock();
            store(&mut state, self.cap, key, hit.clone());
        }
        drop(flight);
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_check::model::Facts;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    const NPM: Ecosystem = Ecosystem::Npm;

    fn found() -> Outcome {
        Outcome::Found(Facts {
            ecosystem: NPM,
            created_ms: Some(1),
            newest: Some("1.0.0".to_string()),
            tags: BTreeMap::new(),
            versions: BTreeMap::new(),
            downloads: None,
            maintainers: None,
            source_link: None,
            big: false,
        })
    }

    #[test]
    fn found_is_fresh_for_six_hours_then_gone() {
        let cache = Cache::new(10);
        cache.put(NPM, "lodash", found(), 1_000);
        assert!(cache.get(NPM, "lodash", 1_000 + TTL_FOUND_MS - 1).is_some());
        assert!(cache.get(NPM, "lodash", 1_000 + TTL_FOUND_MS).is_none());
        assert_eq!(TTL_FOUND_MS, 6 * 3_600_000);
    }

    #[test]
    fn not_found_is_fresh_for_two_minutes_then_gone() {
        let cache = Cache::new(10);
        cache.put(NPM, "made-up", Outcome::NotFound, 0);
        let hit = cache.get(NPM, "made-up", TTL_NOT_FOUND_MS - 1).unwrap();
        assert_eq!(hit.outcome, Outcome::NotFound);
        assert_eq!(hit.fetched_ms, 0);
        assert!(cache.get(NPM, "made-up", TTL_NOT_FOUND_MS).is_none());
        assert_eq!(TTL_NOT_FOUND_MS, 120_000);
    }

    #[test]
    fn failed_is_fresh_for_thirty_seconds_then_gone() {
        let cache = Cache::new(10);
        cache.put(NPM, "lodash", Outcome::Failed(FetchError::Unreachable), 500);
        assert!(cache.get(NPM, "lodash", 500 + 29_999).is_some());
        assert!(cache.get(NPM, "lodash", 500 + 30_000).is_none());
        assert_eq!(TTL_FAILED_MS, 30_000);
    }

    #[test]
    fn get_is_per_ecosystem_and_name() {
        let cache = Cache::new(10);
        cache.put(NPM, "lodash", found(), 0);
        assert!(cache.get(Ecosystem::Pypi, "lodash", 0).is_none());
        assert!(cache.get(NPM, "lodash-es", 0).is_none());
    }

    #[test]
    fn cap_evicts_the_oldest_entry() {
        let cache = Cache::new(2);
        cache.put(NPM, "a", found(), 0);
        cache.put(NPM, "b", found(), 0);
        cache.put(NPM, "c", found(), 0);
        assert!(cache.get(NPM, "a", 0).is_none());
        assert!(cache.get(NPM, "b", 0).is_some());
        assert!(cache.get(NPM, "c", 0).is_some());
    }

    #[test]
    fn re_putting_a_key_does_not_evict_others() {
        let cache = Cache::new(2);
        cache.put(NPM, "a", found(), 0);
        cache.put(NPM, "b", found(), 0);
        cache.put(NPM, "a", Outcome::NotFound, 5);
        assert!(cache.get(NPM, "a", 5).is_some());
        assert!(cache.get(NPM, "b", 5).is_some());
    }

    #[test]
    fn forget_unsettled_keeps_found() {
        let cache = Cache::new(10);
        cache.put(NPM, "lodash", found(), 0);
        cache.put(NPM, "gone", Outcome::NotFound, 0);
        cache.put(NPM, "slow", Outcome::Failed(FetchError::Timeout), 0);
        cache.forget_unsettled(NPM, "lodash");
        cache.forget_unsettled(NPM, "gone");
        cache.forget_unsettled(NPM, "slow");
        assert!(cache.get(NPM, "lodash", 0).is_some());
        assert!(cache.get(NPM, "gone", 0).is_none());
        assert!(cache.get(NPM, "slow", 0).is_none());
    }

    #[test]
    fn get_or_fetch_uses_a_fresh_entry_without_fetching() {
        let cache = Cache::new(10);
        cache.put(NPM, "lodash", found(), 0);
        let calls = AtomicUsize::new(0);
        let hit = cache.get_or_fetch(NPM, "lodash", 10, 1_000, &|| {
            calls.fetch_add(1, Ordering::SeqCst);
            Outcome::NotFound
        });
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(hit.outcome, Outcome::Found(_)));
    }

    #[test]
    fn get_or_fetch_stores_the_fetched_outcome() {
        let cache = Cache::new(10);
        let hit = cache.get_or_fetch(NPM, "made-up", 42, 1_000, &|| Outcome::NotFound);
        assert_eq!(hit, CacheHit { outcome: Outcome::NotFound, fetched_ms: 42 });
        assert_eq!(cache.get(NPM, "made-up", 42), Some(hit));
    }

    #[test]
    fn two_threads_on_one_key_fetch_once() {
        let cache = Cache::new(10);
        let calls = AtomicUsize::new(0);
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                cache.get_or_fetch(NPM, "lodash", 0, 5_000, &|| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let _ = started_tx.send(());
                    let _ = release_rx.lock().unwrap().recv();
                    found()
                })
            });
            started_rx.recv().unwrap();
            let second = scope.spawn(|| {
                cache.get_or_fetch(NPM, "lodash", 0, 5_000, &|| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    found()
                })
            });
            std::thread::sleep(Duration::from_millis(100));
            let _ = release_tx.send(());
            let a = first.join().unwrap();
            let b = second.join().unwrap();
            assert_eq!(a.outcome, found());
            assert_eq!(b.outcome, found());
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn waiter_gives_up_at_wait_ms_with_timeout() {
        let cache = Cache::new(10);
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                cache.get_or_fetch(NPM, "slow", 7, 5_000, &|| {
                    let _ = started_tx.send(());
                    let _ = release_rx.lock().unwrap().recv();
                    Outcome::NotFound
                })
            });
            started_rx.recv().unwrap();
            let waited = cache.get_or_fetch(NPM, "slow", 9, 50, &|| -> Outcome { panic!("a waiter must not fetch") });
            assert_eq!(waited, CacheHit { outcome: Outcome::Failed(FetchError::Timeout), fetched_ms: 9 });
            let _ = release_tx.send(());
            assert_eq!(first.join().unwrap().outcome, Outcome::NotFound);
        });
        // The timed-out answer is not stored: the first fetch's answer is.
        assert_eq!(cache.get(NPM, "slow", 7).unwrap().outcome, Outcome::NotFound);
    }

    #[test]
    fn a_panicking_fetch_releases_the_key() {
        let cache = Cache::new(10);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.get_or_fetch(NPM, "boom", 0, 1_000, &|| panic!("registry exploded"));
        }));
        assert!(result.is_err());
        let hit = cache.get_or_fetch(NPM, "boom", 0, 1_000, &|| Outcome::NotFound);
        assert_eq!(hit.outcome, Outcome::NotFound);
    }
}
