//! Definition sources and the directory scan (SRC-1..SRC-7, REG-8 ordering).
//!
//! The scan is read-only: it lists `*.toml` files directly inside one directory (never
//! recursively), refuses symlinks that leave it, reads at most `MAX_FILES_PER_DIR` files and at
//! most one byte past `MAX_DEFINITION_BYTES` of each, so `validate` can report an oversize file
//! without the loader ever holding a huge one. Nothing is parsed or registered here.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::validate::{Source, MAX_DEFINITION_BYTES};

pub const MAX_FILES_PER_DIR: usize = 64;

/// A source of definitions: its kind and, for `extra`, the position in the configured list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    pub source: Source,
    /// Index in the settings' `extra` directory list; 0 for every other source.
    pub extra_index: usize,
}

impl Origin {
    pub const fn new(source: Source) -> Self {
        Self { source, extra_index: 0 }
    }

    pub const fn extra(index: usize) -> Self {
        Self { source: Source::Extra, extra_index: index }
    }

    /// REG-8: higher wins. project > extra (earlier in the list wins) > user > bundled.
    pub fn priority(&self) -> (u8, usize) {
        match self.source {
            Source::Bundled => (0, 0),
            Source::User => (1, 0),
            Source::Extra => (2, usize::MAX - self.extra_index),
            Source::Project => (3, 0),
        }
    }
}

pub fn source_name(source: Source) -> &'static str {
    match source {
        Source::Bundled => "bundled",
        Source::User => "user",
        Source::Project => "project",
        Source::Extra => "extra",
    }
}

/// `<project>/.puppet-master/agents` (SRC-3). The caller only scans it for a run or pane of that
/// very project (SRC-7).
pub fn project_agents_dir(project: &Path) -> PathBuf {
    project.join(".puppet-master").join("agents")
}

/// A file that could not be turned into text for the loader (never echoes file content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub code: &'static str,
    pub message: String,
}

/// One definition file's text, with the byte length FMT-6 is checked against (capped at
/// `MAX_DEFINITION_BYTES + 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFile {
    pub path: PathBuf,
    pub text: String,
    pub raw_len: usize,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub files: Vec<RawFile>,
    pub issues: Vec<ScanIssue>,
}

/// SRC-6. A missing directory is an empty scan, not an error.
pub fn scan_dir(dir: &Path) -> Scan {
    let mut scan = Scan::default();
    let Ok(entries) = fs::read_dir(dir) else { return scan };
    let root = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        // `.removed/` holds agents the user removed (removal.rs): never a source.
        .filter(|p| p.file_name().is_none_or(|n| n != super::removal::REMOVED_DIR))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")))
        .collect();
    paths.sort();
    for (index, path) in paths.into_iter().enumerate() {
        let issue = |code: &'static str, message: &str| ScanIssue { path: path.clone(), code, message: message.to_string() };
        let Ok(link_meta) = fs::symlink_metadata(&path) else { continue };
        if link_meta.file_type().is_symlink() {
            let inside = fs::canonicalize(&path).is_ok_and(|real| real.starts_with(&root));
            if !inside {
                scan.issues.push(issue("AGENT_DEF_SYMLINK_REFUSED", "symlink leaves the agents directory"));
                continue;
            }
        }
        if !fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            continue;
        }
        if index >= MAX_FILES_PER_DIR {
            scan.issues.push(issue(
                "AGENT_DEF_TOO_MANY_FILES",
                &format!("only the first {MAX_FILES_PER_DIR} files of a directory are read"),
            ));
            continue;
        }
        match read_capped(&path) {
            Ok(bytes) if bytes.len() > MAX_DEFINITION_BYTES => {
                // oversize: only the length matters (validate reports AGENT_DEF_TOO_LARGE)
                let raw_len = bytes.len();
                let text = String::from_utf8_lossy(&bytes).into_owned();
                scan.files.push(RawFile { path, text, raw_len });
            }
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => {
                    let raw_len = text.len();
                    scan.files.push(RawFile { path, text, raw_len });
                }
                Err(_) => scan.issues.push(issue("AGENT_DEF_INVALID_TOML", "file is not valid UTF-8")),
            },
            Err(_) => scan.issues.push(issue("AGENT_DEF_UNREADABLE", "file could not be read")),
        }
    }
    scan
}

fn read_capped(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(MAX_DEFINITION_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

// ---- source switches and consent (SRC-3..5, SEC-2) -------------------------------------------

use serde::{Deserialize, Serialize};

use crate::standby::key::canonical_project;

/// `<app data>/agent-definitions.json` (SPEC 7.6): the source switches, the project consents and
/// the extra directories. A missing, malformed or wrong-version file means the defaults, which
/// load nothing beyond the bundled and user sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceSettings {
    pub version: u32,
    pub sources: SourceSwitches,
    pub watch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceSwitches {
    pub bundled: Switch,
    pub user: Switch,
    pub project: ProjectSwitch,
    pub extra: ExtraSwitch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Switch {
    pub enabled: bool,
}

impl Default for Switch {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSwitch {
    pub enabled: bool,
    /// Canonical project paths (`canonical_project`) the user allowed. Path only (SPEC 11 Q3).
    pub consented_projects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtraSwitch {
    pub enabled: bool,
    pub dirs: Vec<ExtraDirSetting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraDirSetting {
    pub path: String,
    #[serde(default)]
    pub consented: bool,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self { version: 1, sources: SourceSwitches::default(), watch: true }
    }
}

pub fn settings_path() -> PathBuf {
    crate::app_paths::app_data_dir().join("agent-definitions.json")
}

impl SourceSettings {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else { return Self::default() };
        match serde_json::from_str::<Self>(&text) {
            Ok(settings) if settings.version == 1 => settings,
            _ => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Enable or disable `user`, `project` or `extra` (`bundled` is always on).
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String> {
        match name {
            "user" => self.sources.user.enabled = enabled,
            "project" => self.sources.project.enabled = enabled,
            "extra" => self.sources.extra.enabled = enabled,
            "bundled" => return Err("the bundled source cannot be switched off".into()),
            other => return Err(format!("unknown source: {other}")),
        }
        Ok(())
    }

    /// Record or withdraw consent for one project path (keyed by its canonical spelling, so a
    /// clone at another path asks again).
    pub fn set_project_consent(&mut self, project: &str, consented: bool) {
        let key = canonical_project(project);
        let list = &mut self.sources.project.consented_projects;
        list.retain(|p| *p != key);
        if consented {
            list.push(key);
        }
    }

    pub fn project_consented(&self, project: &str) -> bool {
        let key = canonical_project(project);
        self.sources.project.consented_projects.iter().any(|p| *p == key)
    }

    /// Add an extra directory (not consented yet). A repeat of the same canonical path is a no-op.
    pub fn add_extra(&mut self, path: &str) -> Result<(), String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("a directory path is required".into());
        }
        let key = canonical_project(path);
        if !self.sources.extra.dirs.iter().any(|d| canonical_project(&d.path) == key) {
            self.sources.extra.dirs.push(ExtraDirSetting { path: path.to_string(), consented: false });
        }
        Ok(())
    }

    pub fn remove_extra(&mut self, path: &str) -> bool {
        let key = canonical_project(path);
        let before = self.sources.extra.dirs.len();
        self.sources.extra.dirs.retain(|d| canonical_project(&d.path) != key);
        self.sources.extra.dirs.len() != before
    }

    pub fn set_extra_consent(&mut self, path: &str, consented: bool) -> Result<(), String> {
        let key = canonical_project(path);
        let dir = self
            .sources
            .extra
            .dirs
            .iter_mut()
            .find(|d| canonical_project(&d.path) == key)
            .ok_or_else(|| format!("{path} is not in the extra directory list"))?;
        dir.consented = consented;
        Ok(())
    }

    /// Project directories that may be scanned: the source is on and the path was consented.
    pub fn active_projects(&self) -> Vec<PathBuf> {
        if !self.sources.project.enabled {
            return Vec::new();
        }
        self.sources.project.consented_projects.iter().map(PathBuf::from).collect()
    }

    /// Extra directories that may be scanned, in list order (their index is the REG-8 rank):
    /// the source is on and the directory was consented. Unconsented slots keep their index.
    pub fn active_extra(&self) -> Vec<(usize, PathBuf)> {
        if !self.sources.extra.enabled {
            return Vec::new();
        }
        self.sources
            .extra
            .dirs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.consented)
            .map(|(i, d)| (i, PathBuf::from(&d.path)))
            .collect()
    }
}

/// What the consent banner shows for a project: the `*.toml` file names that sit in
/// `<project>/.puppet-master/agents`, listed without reading or parsing any of them.
pub fn project_candidates(project: &Path) -> Vec<String> {
    let dir = project_agents_dir(project);
    let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names.truncate(MAX_FILES_PER_DIR);
    names
}
