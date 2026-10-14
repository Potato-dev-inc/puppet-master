//! The protected set: which files of a project are tests or check config, found by walking the
//! folder with the rule tables' globs. Files under `node_modules`, `target`, `dist` and the like are
//! never looked at.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::facts::{self, Marker};
use super::model::{CheckKind, Role};
use super::rules::{GlobSet, Rules};

/// A protected file as read from disk.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub role: Role,
    /// The ecosystem whose patterns read it; `None` when none can.
    pub eco: Option<String>,
    pub size: u64,
    pub sha256: String,
    /// The text, when it is UTF-8 and small enough to analyze.
    pub text: Option<String>,
    /// For config files: the checks the matching config rules belong to.
    pub config_checks: Vec<CheckKind>,
}

#[derive(Debug, Default)]
pub struct Walk {
    pub protected: BTreeMap<String, Loaded>,
    /// Markers that hide check errors, in every readable source file.
    pub silencers: BTreeMap<String, Vec<Marker>>,
    /// Checks that some config file in the tree belongs to (used to guess the checks in play).
    pub seen_checks: BTreeSet<CheckKind>,
    /// The walk stopped at the file limit.
    pub truncated: bool,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    Some(parts.join("/"))
}

/// Walk `root` and read every protected file. Source files are also read for markers that hide
/// check errors, but only for the checks in `silence` (`None` means every check, for when the checks
/// in play are not known yet). With no Type check or Lint among them, plain source files are not read.
pub fn walk(root: &Path, rules: &Rules, protect: &GlobSet, ignore: &GlobSet, silence: Option<&[CheckKind]>) -> Walk {
    let mut out = Walk::default();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut files = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !rules.never_dirs.iter().any(|d| *d == name) {
                    stack.push(path);
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            files += 1;
            if files > rules.max_files {
                out.truncated = true;
                return out;
            }
            let Some(rel) = relative(root, &path) else { continue };
            if ignore.is_match(&rel) {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            visit(&mut out, rules, protect, silence, &rel, &path, size);
        }
    }
    out
}

fn visit(out: &mut Walk, rules: &Rules, protect: &GlobSet, silence: Option<&[CheckKind]>, rel: &str, path: &Path, size: u64) {
    let by_ext = rules.ecosystem_for(rel);
    let mut role: Option<Role> = None;
    let mut config_checks: Vec<CheckKind> = Vec::new();
    let mut config_eco: Option<String> = None;
    if rules.snapshot_globs.is_match(rel) {
        role = Some(Role::Snapshot);
    } else {
        for eco in &rules.ecosystems {
            for rule in eco.config.iter().filter(|r| r.globs.is_match(rel)) {
                if !config_checks.contains(&rule.check) {
                    config_checks.push(rule.check);
                }
                config_eco.get_or_insert_with(|| eco.id.clone());
            }
        }
        if !config_checks.is_empty() {
            role = Some(Role::Config);
            out.seen_checks.extend(config_checks.iter().copied());
        } else if protect.is_match(rel) || by_ext.is_some_and(|e| e.test_globs.is_match(rel)) {
            role = Some(Role::Test);
        }
    }

    let wants_silence = |e: &super::rules::Ecosystem| {
        e.silence.iter().any(|p| p.check.is_some_and(|c| silence.map_or(true, |list| list.contains(&c))))
    };
    let readable_source = by_ext.is_some_and(|e| e.scannable && (!e.test_markers.is_empty() || wants_silence(e)));
    if role.is_none() && !(readable_source && size <= rules.max_file_bytes) {
        return;
    }
    let Ok(bytes) = fs::read(path) else { return };
    let text = if size <= rules.max_file_bytes { String::from_utf8(bytes.clone()).ok() } else { None };

    if role.is_none() {
        // A source file with test code inside it (Rust unit tests).
        if let (Some(eco), Some(text)) = (by_ext, &text) {
            if eco.test_markers.iter().any(|m| m.is_match(text)) {
                role = Some(Role::Test);
            }
        }
    }
    if let (Some(eco), Some(text)) = (by_ext, &text) {
        if eco.scannable && wants_silence(eco) {
            let markers = facts::silencers(eco, text);
            if !markers.is_empty() {
                out.silencers.insert(rel.to_string(), markers);
            }
        }
    }
    let Some(role) = role else { return };
    let eco = match role {
        Role::Config => config_eco,
        Role::Test => by_ext.map(|e| e.id.clone()),
        Role::Snapshot => None,
    };
    out.protected.insert(rel.to_string(), Loaded { role, eco, size, sha256: sha256_hex(&bytes), text, config_checks });
}

/// `[tamper] protect = [...]` and `ignore = [...]` from `.puppet-master/project.toml`.
pub fn project_globs(root: &Path) -> (Vec<String>, Vec<String>) {
    let Ok(text) = fs::read_to_string(root.join(".puppet-master").join("project.toml")) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(table) = text.parse::<toml::Table>() else { return (Vec::new(), Vec::new()) };
    let list = |key: &str| -> Vec<String> {
        table
            .get("tamper")
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    (list("protect"), list("ignore"))
}

/// Checks in play: the ones the caller named, or Tests plus every check some config file belongs to.
pub fn effective_checks(named: &[CheckKind], seen: &BTreeSet<CheckKind>) -> Vec<CheckKind> {
    let mut checks: BTreeSet<CheckKind> = if named.is_empty() { seen.clone() } else { named.iter().copied().collect() };
    checks.insert(CheckKind::Tests);
    checks.into_iter().collect()
}

/// A config file is protected when one of its rules' checks is in play.
pub fn config_active(loaded: &Loaded, checks: &[CheckKind]) -> bool {
    loaded.role != Role::Config || loaded.config_checks.iter().any(|c| checks.contains(c))
}
