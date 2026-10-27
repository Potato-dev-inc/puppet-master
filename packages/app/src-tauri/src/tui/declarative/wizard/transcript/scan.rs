//! Where the wizard may look for the transcript a CLI wrote during the test turn, and what it may read there.
//!
//! The folders are a fixed, short list: the test pane's own temporary folder, and under the person's home only
//! `~/.{name}`, `~/.config/{name}`, `%APPDATA%/{name}` and `%LOCALAPPDATA%/{name}` where `{name}` is the program's
//! or the definition's name. Nothing else is listed or opened. A walk is bounded in depth and in entries; links
//! are never followed; only `*.jsonl` files count; and only the bytes appended during the turn are read.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Most folder entries looked at in one walk (all folders together).
pub const MAX_ENTRIES: usize = 4000;
/// Deepest folder below a root that is walked.
pub const MAX_DEPTH: usize = 6;
/// Most bytes read from one file: the part that was appended during the turn.
pub const MAX_APPEND_BYTES: u64 = 1024 * 1024;
/// Most changed files read.
pub const MAX_CHANGED_FILES: usize = 8;
/// Most lines kept from one file.
pub const MAX_LINES: usize = 2000;
/// Folders a walk never enters.
const SKIPPED: [&str; 3] = [".git", "node_modules", "target"];

/// The person's folders the home patterns start from. Empty by default, so a test never looks at a real one.
#[derive(Debug, Clone, Default)]
pub struct Homes {
    pub home: Option<PathBuf>,
    /// `%APPDATA%`.
    pub appdata: Option<PathBuf>,
    /// `%LOCALAPPDATA%`.
    pub localappdata: Option<PathBuf>,
}

impl Homes {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        Homes { home: crate::project_path::home_dir(), appdata: var("APPDATA"), localappdata: var("LOCALAPPDATA") }
    }
}

/// What a name may be: letters, digits, `_` and `-`, at most 40 characters. Anything else (a dot, a separator) is not
/// a name, so a pattern can never leave its folder.
pub fn clean_name(raw: &str) -> Option<String> {
    let stem = Path::new(raw.trim()).file_stem()?.to_string_lossy().to_lowercase();
    let ok = !stem.is_empty() && stem.len() <= 40 && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
    ok.then_some(stem)
}

/// The folders to look in.
#[derive(Debug, Clone)]
pub struct Roots {
    /// The test pane's own folder.
    pub cwd: PathBuf,
    pub homes: Homes,
    /// Already cleaned (`clean_name`).
    pub names: Vec<String>,
}

impl Roots {
    /// The folders that exist, the pane's own first.
    pub fn folders(&self) -> Vec<PathBuf> {
        let mut out = vec![self.cwd.clone()];
        for name in &self.names {
            let candidates = [
                self.homes.home.as_ref().map(|h| h.join(format!(".{name}"))),
                self.homes.home.as_ref().map(|h| h.join(".config").join(name)),
                self.homes.appdata.as_ref().map(|d| d.join(name)),
                self.homes.localappdata.as_ref().map(|d| d.join(name)),
            ];
            for path in candidates.into_iter().flatten() {
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        out.into_iter().filter(|p| is_plain_dir(p)).collect()
    }
}

/// A folder that is not a link.
fn is_plain_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}

/// The `*.jsonl` files under the roots and their sizes at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    files: HashMap<PathBuf, u64>,
    /// The walk stopped at `MAX_ENTRIES`: a file may have been missed.
    pub cut_short: bool,
}

impl Snapshot {
    pub fn take(roots: &Roots) -> Snapshot {
        let mut snap = Snapshot::default();
        let mut seen = 0usize;
        for root in roots.folders() {
            let mut queue = VecDeque::from([(root, 0usize)]);
            while let Some((dir, depth)) = queue.pop_front() {
                let Ok(read) = std::fs::read_dir(&dir) else { continue };
                let mut subs: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
                for entry in read.filter_map(Result::ok) {
                    seen += 1;
                    if seen > MAX_ENTRIES {
                        snap.cut_short = true;
                        return snap;
                    }
                    let Ok(kind) = entry.file_type() else { continue };
                    let path = entry.path();
                    if kind.is_symlink() {
                        continue;
                    }
                    if kind.is_dir() {
                        let name = entry.file_name().to_string_lossy().to_lowercase();
                        if depth < MAX_DEPTH && !SKIPPED.contains(&name.as_str()) {
                            let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                            subs.push((modified, path));
                        }
                    } else if kind.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jsonl")) {
                        if let Ok(meta) = entry.metadata() {
                            snap.files.insert(path, meta.len());
                        }
                    }
                }
                // the folders touched last first: a transcript folder made a moment ago is looked at before the old ones
                subs.sort_by(|a, b| b.0.cmp(&a.0));
                queue.extend(subs.into_iter().map(|(_, path)| (path, depth + 1)));
            }
        }
        snap
    }

    /// The lines appended between `self` and `after`, per changed file (the file that grew most recently first is not
    /// known, so by size of the growth, biggest first), at most `MAX_CHANGED_FILES` files.
    pub fn appended(&self, after: &Snapshot) -> Vec<Appended> {
        let mut changed: Vec<(&PathBuf, u64, u64)> = after
            .files
            .iter()
            .filter_map(|(path, &len)| {
                let was = self.files.get(path).copied().unwrap_or(0);
                // a file that got shorter was rewritten: all of it is new
                let from = if len < was { 0 } else { was };
                (len > from).then_some((path, from, len))
            })
            .collect();
        changed.sort_by(|a, b| (b.2 - b.1).cmp(&(a.2 - a.1)).then_with(|| a.0.cmp(b.0)));
        changed
            .into_iter()
            .take(MAX_CHANGED_FILES)
            .filter_map(|(path, from, len)| {
                let lines = read_appended(path, from, len)?;
                Some(Appended { file: path.clone(), lines })
            })
            .collect()
    }
}

/// The lines one file gained.
#[derive(Debug, Clone)]
pub struct Appended {
    pub file: PathBuf,
    pub lines: Vec<String>,
}

/// The bytes `from..len` of `path` (at most `MAX_APPEND_BYTES`) as lines. The first line is dropped when `from` is in
/// the middle of one; a last line with no end yet is kept.
fn read_appended(path: &Path, from: u64, len: u64) -> Option<Vec<String>> {
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.take((len - from).min(MAX_APPEND_BYTES)).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<String> = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(MAX_LINES).collect();
    Some(lines)
}
