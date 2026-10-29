//! The card's lookup, end to end (SPEC-n33 section 3.11 C): re-read the live prompt by its id, check
//! settings and private servers, run the lookups on a pool with a deadline, build the view.
//!
//! Everything the lookup touches outside this module comes through `LiveIo`, so tests run with a fake
//! screen, a fake clock and a fake registry. The real `RegistryIo` reads the pane registry (the lock is
//! taken and released inside `prompt`, never held while a request is made), calls the public registries
//! and reads the environment and the config files `private.rs` names.
//!
//! Names come only from the pane's live on-screen prompt (S9), never from the caller. A name on a
//! project's own package server gets no request at all (S3). Nothing here writes to a pane (S1).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex as PlMutex;
use serde_json::Value;

use super::cache::Cache;
use super::http::UreqFetcher;
use super::lines;
use super::model::{
    CacheHit, Ecosystem, FetchError, Fetcher, Outcome, PackageCheckView, PackageList, PackageRow, Settings,
    DEADLINE_MS, LOOKUP_THREADS, MAX_LOOKUPS,
};
use super::private::{self, Look};
use super::registries;
use super::settings;
use crate::pty::PaneRegistry;

/// Largest config file read for the private server check (SPEC-n33 section 3.9).
const MAX_CONFIG_READ: u64 = 64 * 1024;

/// Everything `lookup` needs from outside.
pub trait LiveIo: Send + Sync {
    /// The prompt the pane shows now, and the pane's folder. `None` when the pane has no prompt.
    fn prompt(&self, pane_id: &str) -> Option<(Value, String)>;
    /// The network seam. An `Arc` (the spec said `&dyn Fetcher`) because a request that outlives the card's
    /// deadline keeps running on its own thread, which needs its own handle.
    fn fetcher(&self) -> Arc<dyn Fetcher>;
    fn now_ms(&self) -> u64;
    fn settings(&self) -> Settings;
    /// Text of one config file (capped), or `None` when missing.
    fn read_text(&self, p: &Path) -> Option<String>;
    fn env(&self, k: &str) -> Option<String>;
    fn home(&self) -> Option<PathBuf>;
}

/// The real `LiveIo` for the desktop.
pub struct RegistryIo {
    registry: Arc<PlMutex<PaneRegistry>>,
    fetcher: Arc<UreqFetcher>,
}

impl RegistryIo {
    pub fn new(registry: Arc<PlMutex<PaneRegistry>>) -> Self {
        Self { registry, fetcher: Arc::new(UreqFetcher::new()) }
    }
}

impl LiveIo for RegistryIo {
    fn prompt(&self, pane_id: &str) -> Option<(Value, String)> {
        let prompt = crate::pane_prompt::detect(&self.registry, pane_id, false)?;
        // Taken and released here; `detect` has released its own lock already.
        let cwd = self.registry.lock().panes.get(pane_id)?.info.cwd.clone();
        Some((prompt, cwd))
    }

    fn fetcher(&self) -> Arc<dyn Fetcher> {
        self.fetcher.clone()
    }

    fn now_ms(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }

    fn settings(&self) -> Settings {
        settings::load(&settings::live_root())
    }

    fn read_text(&self, p: &Path) -> Option<String> {
        let file = std::fs::File::open(p).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_READ).read_to_end(&mut bytes).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn env(&self, k: &str) -> Option<String> {
        std::env::var(k).ok()
    }

    fn home(&self) -> Option<PathBuf> {
        crate::project_path::home_dir()
    }
}

/// The card's lookup for the prompt `prompt_id` of `pane_id`. `again` drops unsettled cache entries first
/// ("Check again"). Errors: `STALE_PROMPT` when the pane no longer shows that prompt.
pub fn lookup(io: &dyn LiveIo, cache: &Cache, pane_id: &str, prompt_id: &str, again: bool) -> Result<PackageCheckView, String> {
    lookup_within(io, cache, pane_id, prompt_id, again, DEADLINE_MS)
}

/// [`lookup`] with the card's deadline given, so a test does not wait the real `DEADLINE_MS`.
fn lookup_within(
    io: &dyn LiveIo,
    cache: &Cache,
    pane_id: &str,
    prompt_id: &str,
    again: bool,
    deadline_ms: u64,
) -> Result<PackageCheckView, String> {
    let started = Instant::now();
    // 1. The names come from the live prompt, and only while it is still the one the card shows.
    let (prompt, cwd) = io.prompt(pane_id).ok_or_else(|| "STALE_PROMPT".to_string())?;
    if prompt.get("prompt_id").and_then(Value::as_str) != Some(prompt_id) {
        return Err("STALE_PROMPT".to_string());
    }
    let list: PackageList = prompt
        .get("packages")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let now = io.now_ms();
    // 2. Lookups off: every row says so, nothing is requested.
    let off = !io.settings().lookups;
    if off || list.items.is_empty() {
        let rows = list.items.iter().map(|item| lines::row(item, None, false, off, now)).collect();
        return Ok(lines::view(prompt_id, &list, rows, off, None, now));
    }

    // 3. Which items are looked up: a project's own server means no request (S3); the first MAX_LOOKUPS only.
    let workspace = (!cwd.trim().is_empty()).then(|| PathBuf::from(&cwd));
    let home = io.home();
    let read = |p: &Path| io.read_text(p);
    let env = |k: &str| io.env(k);
    let look = Look { workspace: workspace.as_deref(), home: home.as_deref(), read: &read, env: &env };
    let mut plan: Vec<Plan> = Vec::with_capacity(list.items.len());
    let mut jobs: Vec<Job> = Vec::new();
    let mut counted = 0usize;
    for item in &list.items {
        if !item.lookup {
            plan.push(Plan::NotChecked);
            continue;
        }
        counted += 1;
        if counted > MAX_LOOKUPS {
            plan.push(Plan::TooMany);
        } else if private::uses_own_server(item.ecosystem, &item.name, &look) {
            plan.push(Plan::OwnServer);
        } else {
            let norm = item.ecosystem.normalize(&item.name);
            if again {
                cache.forget_unsettled(item.ecosystem, &norm);
            }
            plan.push(Plan::Job(jobs.len()));
            jobs.push(Job { ecosystem: item.ecosystem, name: item.name.clone(), norm });
        }
    }

    // 4. The lookups, on a small pool, all finished or timed out by the deadline.
    let deadline = started + Duration::from_millis(deadline_ms);
    let hits = run_jobs(&jobs, io.fetcher(), cache, now, deadline);
    let failed = hits.iter().filter(|h| matches!(h.outcome, Outcome::Failed(_))).count();
    tracing::debug!("package_check: {} lookups, {} failed", jobs.len(), failed);

    // 5. Rows and view.
    let now = io.now_ms();
    let mut rows: Vec<PackageRow> = Vec::with_capacity(list.items.len());
    for (item, plan) in list.items.iter().zip(&plan) {
        rows.push(match plan {
            Plan::NotChecked => lines::row(item, None, false, false, now),
            Plan::TooMany => lines::too_many_row(item),
            Plan::OwnServer => lines::row(item, None, true, false, now),
            Plan::Job(i) => lines::row(item, Some(&hits[*i]), false, false, now),
        });
    }
    let oldest = hits
        .iter()
        .filter(|h| matches!(h.outcome, Outcome::Found(_) | Outcome::NotFound))
        .map(|h| h.fetched_ms)
        .min();
    Ok(lines::view(prompt_id, &list, rows, false, oldest, now))
}

/// What one item of the card turns into.
enum Plan {
    /// Never looked up (a link, a folder, a cut name, a tool without a registry API).
    NotChecked,
    /// Past `MAX_LOOKUPS`.
    TooMany,
    /// The project's own package server: no request.
    OwnServer,
    /// Index into the job list.
    Job(usize),
}

struct Job {
    ecosystem: Ecosystem,
    name: String,
    norm: String,
}

/// Run every job: at most `LOOKUP_THREADS` at once, each through the cache so two cards (or two calls)
/// asking for the same name make one request. Returns one hit per job, in job order. A job that has not
/// answered at `deadline` is `Failed(Timeout)`; its request may finish later on its own thread.
fn run_jobs(jobs: &[Job], fetcher: Arc<dyn Fetcher>, cache: &Cache, now_ms: u64, deadline: Instant) -> Vec<CacheHit> {
    let results: Mutex<HashMap<usize, CacheHit>> = Mutex::new(HashMap::new());
    let next = AtomicUsize::new(0);
    let workers = LOOKUP_THREADS.min(jobs.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(i) else { break };
                let left = deadline.saturating_duration_since(Instant::now());
                let hit = if left.is_zero() {
                    // The card's time is up before this one started: it is not asked at all.
                    CacheHit { outcome: Outcome::Failed(FetchError::Timeout), fetched_ms: now_ms }
                } else {
                    let wait_ms = left.as_millis() as u64;
                    cache.get_or_fetch(job.ecosystem, &job.norm, now_ms, wait_ms, &|| {
                        fetch_by(&fetcher, job.ecosystem, &job.name, deadline)
                    })
                };
                results.lock().unwrap_or_else(|p| p.into_inner()).insert(i, hit);
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|p| p.into_inner());
    (0..jobs.len())
        .map(|i| results.remove(&i).unwrap_or(CacheHit { outcome: Outcome::Failed(FetchError::Timeout), fetched_ms: now_ms }))
        .collect()
}

/// One registry lookup on its own thread, waited for until `deadline`. The thread is not stopped at the
/// deadline (a blocked request cannot be cancelled); it only loses its reader.
fn fetch_by(fetcher: &Arc<dyn Fetcher>, eco: Ecosystem, name: &str, deadline: Instant) -> Outcome {
    let (tx, rx) = mpsc::channel();
    let (fetcher, name) = (Arc::clone(fetcher), name.to_string());
    let spawned = std::thread::Builder::new().name("package-check-fetch".into()).spawn(move || {
        let _ = tx.send(by_ecosystem(&*fetcher, eco, &name));
    });
    if spawned.is_err() {
        return Outcome::Failed(FetchError::Unreachable);
    }
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or(Outcome::Failed(FetchError::Timeout))
}

/// The registry that answers for an ecosystem.
fn by_ecosystem(f: &dyn Fetcher, eco: Ecosystem, name: &str) -> Outcome {
    match eco {
        Ecosystem::Npm => registries::npm::lookup(f, name),
        Ecosystem::Pypi => registries::pypi::lookup(f, name),
        Ecosystem::Crates => registries::crates::lookup(f, name),
        Ecosystem::Gems => registries::gems::lookup(f, name),
        Ecosystem::Go => registries::go::lookup(f, name),
        // These have no public API we call; `item.lookup` is false for them, so this is not reached.
        Ecosystem::Winget | Ecosystem::Choco | Ecosystem::Scoop => Outcome::Failed(FetchError::NotAllowed),
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
