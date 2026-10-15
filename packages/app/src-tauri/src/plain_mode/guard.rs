//! Which folders Simple mode refuses. Pure string work (no disk), so the same cases can be checked
//! in Rust and in the UI (`components/simple/folder-guard.ts`): both read
//! `components/simple/folder-cases.json`, so the two copies cannot drift apart.
//!
//! The rule (IDEAS-R8-PLAIN-MODE §5.1): a person's own big folders are never a project. That is the
//! user profile, Desktop, Documents, Downloads, a OneDrive root, a drive root, anything above the
//! profile, Windows, Program Files and the folder that holds the Simple projects. Sub folders are
//! fine: `Documents\Quiz` is a project, `Documents` is not.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Ok,
    Empty,
    Home,
    Desktop,
    Documents,
    Downloads,
    Onedrive,
    DriveRoot,
    AboveHome,
    System,
    ProjectsRoot,
}

impl Verdict {
    pub fn is_ok(self) -> bool {
        self == Verdict::Ok
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Empty => "empty",
            Verdict::Home => "home",
            Verdict::Desktop => "desktop",
            Verdict::Documents => "documents",
            Verdict::Downloads => "downloads",
            Verdict::Onedrive => "onedrive",
            Verdict::DriveRoot => "drive_root",
            Verdict::AboveHome => "above_home",
            Verdict::System => "system",
            Verdict::ProjectsRoot => "projects_root",
        }
    }
}

/// The folders this computer treats as "too broad". All paths as the OS spells them.
#[derive(Debug, Clone, Default)]
pub struct KnownDirs {
    pub home: String,
    /// OneDrive roots (`OneDrive`, `OneDriveConsumer`, `OneDriveCommercial`).
    pub onedrive: Vec<String>,
    /// Where Simple projects are made (`Documents\PM`).
    pub projects_root: String,
    /// `C:` (the drive Windows is on).
    pub system_drive: String,
}

impl KnownDirs {
    pub fn from_env() -> Self {
        let home = crate::project_path::home_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let mut onedrive = Vec::new();
        for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
            if let Some(value) = std::env::var_os(var) {
                let value = value.to_string_lossy().into_owned();
                if !value.trim().is_empty() && !onedrive.contains(&value) {
                    onedrive.push(value);
                }
            }
        }
        let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        Self { projects_root: projects_root_for(&home), home, onedrive, system_drive }
    }
}

/// `<home>\Documents\PM`: a new folder for each thing she makes, nothing she owns is in it.
pub fn projects_root_for(home: &str) -> String {
    if home.is_empty() {
        return String::new();
    }
    PathBuf::from(home).join("Documents").join("PM").to_string_lossy().into_owned()
}

/// Lower case, `/` separators, no trailing `/`, no `\?\` prefix.
pub fn normalize(path: &str) -> String {
    let mut text = path.trim().replace('\\', "/");
    if let Some(rest) = text.strip_prefix("//?/") {
        text = rest.to_string();
    }
    let trimmed = text.trim_end_matches('/');
    let out = if trimmed.is_empty() && text.starts_with('/') { "/".to_string() } else { trimmed.to_string() };
    out.to_lowercase()
}

fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    path == "/" || (bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn same(a: &str, b: &str) -> bool {
    !b.is_empty() && a == &normalize(b)
}

/// `path` is `base` or a folder inside it.
fn inside_or_equal(path: &str, base: &str) -> bool {
    path == base || path.strip_prefix(base).is_some_and(|rest| rest.starts_with('/'))
}

pub fn verdict(path: &str, known: &KnownDirs) -> Verdict {
    let p = normalize(path);
    if p.is_empty() {
        return Verdict::Empty;
    }
    if is_drive_root(&p) {
        return Verdict::DriveRoot;
    }
    let drive = normalize(&known.system_drive);
    let system_roots = ["windows", "program files", "program files (x86)", "programdata"];
    if !drive.is_empty() && system_roots.iter().any(|name| inside_or_equal(&p, &format!("{drive}/{name}"))) {
        return Verdict::System;
    }
    let home = normalize(&known.home);
    if !home.is_empty() {
        if inside_or_equal(&p, &format!("{home}/appdata")) {
            return Verdict::System;
        }
        if home.strip_prefix(&p).is_some_and(|rest| rest.starts_with('/')) {
            return Verdict::AboveHome;
        }
    }
    // The personal folders, in the profile and in each OneDrive (it often holds the real Desktop).
    let mut bases: Vec<(String, bool)> = Vec::new();
    if !home.is_empty() {
        bases.push((home.clone(), false));
    }
    for root in &known.onedrive {
        let root = normalize(root);
        if !root.is_empty() {
            bases.push((root, true));
        }
    }
    for (base, is_onedrive) in &bases {
        if &p == base {
            return if *is_onedrive { Verdict::Onedrive } else { Verdict::Home };
        }
        for (name, verdict) in [("desktop", Verdict::Desktop), ("documents", Verdict::Documents), ("downloads", Verdict::Downloads)] {
            if p == format!("{base}/{name}") {
                return verdict;
            }
        }
    }
    if same(&p, &known.projects_root) {
        return Verdict::ProjectsRoot;
    }
    Verdict::Ok
}

/// A folder with more than this many files, or bytes, is not copied (§5.3).
pub const MAX_FILES: usize = 5_000;
pub const MAX_BYTES: u64 = 500 * 1024 * 1024;
const SKIP_DIRS: &[&str] = &["node_modules", ".git", ".puppet-master"];

/// Whether `dir` is small enough to work on a copy of. Skips `node_modules` and `.git`.
pub fn small_enough(dir: &Path) -> bool {
    fn walk(dir: &Path, files: &mut usize, bytes: &mut u64) -> bool {
        let Ok(read) = std::fs::read_dir(dir) else { return true };
        for entry in read.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                if !walk(&entry.path(), files, bytes) {
                    return false;
                }
            } else {
                *files += 1;
                *bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                if *files > MAX_FILES || *bytes > MAX_BYTES {
                    return false;
                }
            }
        }
        true
    }
    let (mut files, mut bytes) = (0usize, 0u64);
    walk(dir, &mut files, &mut bytes)
}
