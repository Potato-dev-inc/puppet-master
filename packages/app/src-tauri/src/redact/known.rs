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
}

static CACHE: Lazy<Mutex<Cache>> = Lazy::new(|| Mutex::new(Cache { at: None, ctx: Arc::new(RedactContext::default()) }));
static REGISTERED: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// The context to redact with. Cheap after the first call in each `TTL` window.
pub fn context() -> Arc<RedactContext> {
    let mut cache = CACHE.lock();
    if let Some(at) = cache.at {
        if at.elapsed() < TTL {
            return cache.ctx.clone();
        }
    }
    let ctx = Arc::new(gather());
    cache.at = Some(Instant::now());
    cache.ctx = ctx.clone();
    ctx
}

/// Forget the cached context (call after a secret was saved or removed).
pub fn refresh() {
    CACHE.lock().at = None;
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
