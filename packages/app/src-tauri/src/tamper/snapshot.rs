//! Taking a snapshot: walk the project, record each protected file (hash, counts, text) and the
//! silencing markers of every source file.

use std::collections::BTreeMap;
use std::path::Path;

use super::facts::{self, collapse_ws};
use super::model::{CheckKind, FileSnap, Role, RunCounts, Snapshot, SNAPSHOT_VERSION};
use super::protect::{self, config_active};
use super::rules::{GlobSet, Rules};
use super::store::{valid_id, TamperError};

#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions {
    /// Use this id; otherwise one is made up.
    pub id: Option<String>,
    /// Checks in play. Empty means Tests plus the checks the project's config files belong to.
    pub checks: Vec<CheckKind>,
    pub check_argvs: Vec<Vec<String>>,
    pub counts: Option<RunCounts>,
    /// Extra globs to protect, besides `[tamper] protect` in `.puppet-master/project.toml`.
    pub protect: Vec<String>,
    /// Globs to leave alone (generated tests), besides `[tamper] ignore`.
    pub ignore: Vec<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn new_id() -> String {
    format!("t{}-{:04x}", now_ms(), rand::random::<u16>())
}

/// Take a snapshot of `root` as it is now. Nothing is written; see `save`.
pub fn take(root: &Path, opts: &SnapshotOptions) -> Result<Snapshot, TamperError> {
    if !root.is_dir() {
        return Err(TamperError::BadProject(root.display().to_string()));
    }
    let id = match &opts.id {
        Some(id) if valid_id(id) => id.clone(),
        Some(id) => return Err(TamperError::BadId(id.clone())),
        None => new_id(),
    };
    let rules = Rules::builtin();
    let (project_protect, project_ignore) = protect::project_globs(root);
    let mut protect_globs = opts.protect.clone();
    protect_globs.extend(project_protect);
    let mut ignore_globs = opts.ignore.clone();
    ignore_globs.extend(project_ignore);

    let named: Option<Vec<CheckKind>> = (!opts.checks.is_empty()).then(|| protect::effective_checks(&opts.checks, &Default::default()));
    let walked = protect::walk(root, rules, &GlobSet::new(&protect_globs), &GlobSet::new(&ignore_globs), named.as_deref());
    let checks = protect::effective_checks(&opts.checks, &walked.seen_checks);

    let mut files = BTreeMap::new();
    let mut partial = walked.truncated;
    let mut content_bytes: u64 = 0;
    for (path, loaded) in walked.protected.iter().filter(|(_, l)| config_active(l, &checks)) {
        let (mut tests, mut assertions, mut skips, mut focus) = (0, 0, 0, 0);
        if loaded.role == Role::Test {
            if let (Some(eco), Some(text)) = (loaded.eco.as_deref().and_then(|e| rules.ecosystem(e)), &loaded.text) {
                if eco.scannable {
                    let f = facts::analyze(eco, text, facts::region_only(eco, path));
                    (tests, assertions, skips, focus) = (f.tests, f.assertions, f.skips.len() as u32, f.focus.len() as u32);
                }
            }
        }
        let mut content = loaded.text.clone();
        if let Some(text) = &content {
            if content_bytes + text.len() as u64 > rules.content_cap_bytes {
                content = None;
            } else {
                content_bytes += text.len() as u64;
            }
        }
        if content.is_none() {
            partial = true;
        }
        files.insert(
            path.clone(),
            FileSnap {
                role: loaded.role,
                sha256: loaded.sha256.clone(),
                bytes: loaded.size,
                tests,
                assertions,
                skips,
                focus,
                ecosystem: loaded.eco.clone(),
                content,
            },
        );
    }

    let mut silencers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, markers) in &walked.silencers {
        let kept: Vec<String> = markers
            .iter()
            .filter(|m| m.check.is_some_and(|c| checks.contains(&c)))
            .map(|m| collapse_ws(&m.text))
            .collect();
        if !kept.is_empty() {
            silencers.insert(path.clone(), kept);
        }
    }

    Ok(Snapshot {
        version: SNAPSHOT_VERSION,
        id,
        created_at_ms: now_ms(),
        checks,
        check_argvs: opts.check_argvs.clone(),
        counts: opts.counts,
        protect: protect_globs,
        ignore: ignore_globs,
        files,
        silencers,
        partial,
    })
}
