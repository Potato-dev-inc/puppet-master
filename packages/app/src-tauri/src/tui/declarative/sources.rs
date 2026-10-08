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
