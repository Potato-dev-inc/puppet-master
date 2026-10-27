//! The secrets this process knows by value, so they can be masked wherever they show up.
//!
//! Sources: secret-named and opaque environment variables, the OS credential store (provider API
//! keys), the toolbelt's stored server credentials, the OpenCode key profiles, and anything a module
//! registers at runtime (`register`). The set is cached for a few seconds: sinks run on busy threads
//! and the credential store is not free to read.

use super::rules::{secret_env_values, RedactContext};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a gathered context is reused. Tests gather every time so a planted secret is seen at once.
#[cfg(not(test))]
const TTL: Duration = Duration::from_secs(10);
#[cfg(test)]
const TTL: Duration = Duration::ZERO;

struct Cache {
    at: Option<Instant>,
    ctx: Arc<RedactContext>,
    /// A context was gathered at least once (`ctx` is real, not the empty default).
    ever: bool,
    /// Bumped by `refresh`: a gather that started before it may have missed the change.
    generation: u64,
    /// `generation` when `ctx` was gathered: the context is complete only while they match.
    ctx_generation: u64,
}

/// The cached context and the lock that serializes gathers. The credential store is read OUTSIDE
/// the cache lock (review R12 4.2): every sink on every thread asks for the context, and a keyring
/// read can block (an access prompt on macOS or Linux), which must not stall them all. While one
/// thread gathers, the others get the last context when it is merely old; a caller waits only when
/// there is none yet, or a `refresh` (a secret saved, a value registered) made it incomplete. With a
/// zero TTL (tests) every caller gets its own fresh gather, as before.
struct Known {
    cache: Mutex<Cache>,
    gathering: Mutex<()>,
}

impl Known {
    fn new() -> Self {
        Known {
            cache: Mutex::new(Cache { at: None, ctx: Arc::new(RedactContext::default()), ever: false, generation: 0, ctx_generation: 0 }),
            gathering: Mutex::new(()),
        }
    }

    fn context(&self, ttl: Duration, gather: impl FnOnce() -> RedactContext) -> Arc<RedactContext> {
        {
            let cache = self.cache.lock();
            if cache.at.is_some_and(|at| at.elapsed() < ttl) {
                return cache.ctx.clone();
            }
        }
        let _gathering = match self.gathering.try_lock() {
            Some(guard) => guard,
            None => {
                let cache = self.cache.lock();
                if cache.ever && !ttl.is_zero() && cache.ctx_generation == cache.generation {
                    // Someone is reading the credential store right now: the last context it is.
                    return cache.ctx.clone();
                }
                drop(cache);
                // Nothing complete to serve: wait for that gather, then use it if it is fresh.
                let guard = self.gathering.lock();
                let cache = self.cache.lock();
                if cache.at.is_some_and(|at| at.elapsed() < ttl) {
                    return cache.ctx.clone();
                }
                drop(cache);
                guard
            }
        };
        let generation = self.cache.lock().generation;
        let ctx = Arc::new(gather());
        let mut cache = self.cache.lock();
        cache.ctx = ctx.clone();
        cache.ever = true;
        cache.ctx_generation = generation;
        // A `refresh` during the gather (a secret saved, a value registered) may be missing from
        // it: leave the cache stale so the next caller gathers again.
        cache.at = (cache.generation == generation).then(Instant::now);
        ctx
    }

    fn refresh(&self) {
        let mut cache = self.cache.lock();
        cache.at = None;
        cache.generation += 1;
    }
}

static KNOWN: Lazy<Known> = Lazy::new(Known::new);
static REGISTERED: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// The context to redact with. Cheap after the first call in each `TTL` window.
pub fn context() -> Arc<RedactContext> {
    KNOWN.context(TTL, gather)
}

/// Forget the cached context (call after a secret was saved or removed).
pub fn refresh() {
    KNOWN.refresh();
}

/// Mask `value` from now on (a pairing token, a one-off credential). Values under 6 characters are
/// ignored: masking a short word everywhere would do more harm than good.
pub fn register(value: &str) {
    if value.trim().len() < 6 {
        return;
    }
    let mut list = REGISTERED.lock();
    if !list.iter().any(|v| v == value) {
        list.push(value.to_string());
    }
    drop(list);
    refresh();
}

#[cfg(test)]
pub(crate) fn clear_registered() {
    REGISTERED.lock().clear();
    refresh();
}

fn gather() -> RedactContext {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let mut values = secret_env_values(&env);
    for name in crate::secrets::PROVIDER_SECRETS {
        if let Some(v) = crate::secrets::read_secret(name) {
            values.push(v);
        }
    }
    // Tests must not read the real user's toolbelt or key profiles.
    #[cfg(not(test))]
    {
        values.extend(crate::toolbelt::store::secret_values());
        values.extend(crate::opencode::keys::secret_values());
    }
    values.extend(REGISTERED.lock().iter().cloned());
    values.retain(|v| v.len() >= 4);
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values.dedup();
    let home = crate::project_path::home_dir().map(|p| p.to_string_lossy().to_string());
    RedactContext { env_values: values, home }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(value: &str) -> RedactContext {
        RedactContext { env_values: vec![value.to_string()], home: None }
    }

    /// Review R12 4.2: the cache lock was held while `gather` read the credential store, so every
    /// sink on every thread waited behind one slow keyring read.
    #[test]
    fn a_slow_gather_does_not_block_callers_that_have_a_context() {
        let known = Arc::new(Known::new());
        let ttl = Duration::from_millis(300);
        known.context(ttl, || ctx_with("first-secret"));
        // The context is merely old (the TTL ran out), not incomplete.
        std::thread::sleep(Duration::from_millis(350));
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (begun, has_begun) = std::sync::mpsc::channel::<()>();
        let slow = {
            let known = known.clone();
            std::thread::spawn(move || {
                known.context(ttl, move || {
                    // from here on this thread holds the gather: the other caller must not be asked before
                    let _ = begun.send(());
                    let _ = wait.recv_timeout(Duration::from_secs(5));
                    ctx_with("second-secret")
                })
            })
        };
        has_begun.recv_timeout(Duration::from_secs(30)).expect("the slow gather begins");
        // The other caller runs on its own thread and reports back. A caller that waited for the gather would
        // only report after the release, so a report before it is the proof it was not blocked. (The wait is
        // bounded, not timed against the clock: a loaded machine can take a while to schedule a thread.)
        let (answer, reported) = std::sync::mpsc::channel();
        let other_caller = {
            let known = known.clone();
            std::thread::spawn(move || {
                let _ = answer.send(known.context(ttl, || panic!("a second gather while one runs")));
            })
        };
        let other = reported.recv_timeout(Duration::from_secs(30)).expect("a caller with a context is not blocked by the slow gather");
        assert_eq!(other.env_values, vec!["first-secret".to_string()], "the last context is served meanwhile");
        other_caller.join().unwrap();
        release.send(()).unwrap();
        assert_eq!(slow.join().unwrap().env_values, vec!["second-secret".to_string()]);
        // The new context is a moment old here. A long TTL makes this a check that it is cached, not a race with
        // the 300 ms TTL on a busy machine.
        assert_eq!(known.context(Duration::from_secs(60), || panic!("fresh")).env_values, vec!["second-secret".to_string()]);
    }

    #[test]
    fn a_refresh_during_a_gather_makes_the_next_caller_gather_again() {
        let known = Known::new();
        let ttl = Duration::from_secs(60);
        known.context(ttl, || {
            known.refresh();
            ctx_with("stale")
        });
        assert_eq!(known.context(ttl, || ctx_with("fresh")).env_values, vec!["fresh".to_string()]);
    }

    /// A value registered (or a secret saved) while a gather runs is never served from the old context.
    #[test]
    fn after_a_refresh_callers_wait_for_a_complete_context() {
        let known = Arc::new(Known::new());
        let ttl = Duration::from_secs(60);
        known.context(ttl, || ctx_with("old"));
        known.refresh();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (begun, has_begun) = std::sync::mpsc::channel::<()>();
        let slow = {
            let known = known.clone();
            std::thread::spawn(move || {
                known.context(ttl, move || {
                    // this thread holds the gather from here on (no guessing how long the spawn takes)
                    let _ = begun.send(());
                    let _ = wait.recv_timeout(Duration::from_secs(5));
                    ctx_with("new")
                })
            })
        };
        has_begun.recv_timeout(Duration::from_secs(30)).expect("the gather begins");
        let waiter = {
            let known = known.clone();
            std::thread::spawn(move || known.context(ttl, || ctx_with("own")))
        };
        // The waiter must still be blocked while the gather runs. A thread that was not blocked would finish at
        // once, so this only ever fails on the bug; a loaded machine can only make it pass more easily.
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiter.is_finished(), "the old context is incomplete after a refresh: it is not served");
        release.send(()).unwrap();
        slow.join().unwrap();
        assert_eq!(waiter.join().unwrap().env_values, vec!["new".to_string()]);
    }
}
