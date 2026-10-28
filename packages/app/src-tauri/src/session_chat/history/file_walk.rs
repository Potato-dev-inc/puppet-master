//! Finds the chat files under the roots of one store: folders walked to a depth, names checked, symlinks
//! kept inside the roots, and the entry and time caps honoured.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::file::FileHistorySpec;
use crate::session_chat::declared::path::{checked_file, glob_matches};

/// The deepest folder level a spec may walk (the TOML table allows at most 4).
const MAX_DEPTH: usize = 4;

/// The chat files found, real paths, and whether a cap stopped the walk.
#[derive(Debug, Default)]
pub(super) struct Walked {
    pub files: Vec<PathBuf>,
    pub cut_short: bool,
}

/// Whether a file name is a chat file of this spec: `.jsonl` or `.jsonl.zst`, the glob, and not a skipped name.
pub(super) fn is_candidate(name: &str, spec: &FileHistorySpec) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.ends_with(".jsonl") || lower.ends_with(".jsonl.zst"))
        && glob_matches(spec.glob, name)
        && !spec.skip.iter().any(|skip| glob_matches(skip, name))
}

/// Every chat file under `roots` (already real folders), down to the spec's depth. A symlink to a file counts only
/// when its real place is under a root; a symlinked folder is never followed.
pub(super) fn collect(spec: &FileHistorySpec, roots: &[PathBuf], max_entries: usize, budget: Duration) -> Walked {
    let mut walker = Walker { spec, roots, max_entries, budget, started: Instant::now(), seen: 0, out: Walked::default() };
    for root in roots {
        walker.dir(root, 0);
    }
    walker.out.files.sort();
    walker.out.files.dedup();
    walker.out
}

struct Walker<'a> {
    spec: &'a FileHistorySpec,
    roots: &'a [PathBuf],
    max_entries: usize,
    budget: Duration,
    started: Instant,
    /// Folder entries looked at so far, across all roots.
    seen: usize,
    out: Walked,
}

impl Walker<'_> {
    fn dir(&mut self, dir: &Path, level: usize) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries {
            if self.seen >= self.max_entries || self.started.elapsed() >= self.budget {
                self.out.cut_short = true;
                return;
            }
            self.seen += 1;
            let Ok(entry) = entry else { continue };
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if level < self.spec.depth.min(MAX_DEPTH) {
                    self.dir(&path, level + 1);
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_candidate(&name, self.spec) {
                continue;
            }
            if let Some(real) = checked_file(&path, self.roots) {
                self.out.files.push(real);
            }
        }
    }
}
