//! Collision Alert: the git reads behind the engine, hardened (SPEC-n33 sections 3.3, 3.7 and
//! section 4 C1-C5).
//!
//! Every git call goes through `worktrees::harden::read`: a fixed list of read-only commands, the
//! repo's own config, hooks and filter drivers blanked, `GIT_OPTIONAL_LOCKS=0`, an output cap and a
//! deadline. `GitProbe::sample` makes three plumbing calls (`rev-parse HEAD`, `diff-index`,
//! `ls-files --others`). `GitProbe::hunks` makes one `diff -U0` for one shared file. Only names,
//! modified times and line numbers come back. No file content is read, and nothing here writes,
//! locks or types into anything.
//!
//! `FakeProbe` (tests only) is the scripted probe the engine tests use. It counts every call.

#[cfg(test)]
use std::cell::{Cell, RefCell};
#[cfg(test)]
use std::collections::{HashMap, VecDeque};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use crate::operations::OperationError;
use crate::worktrees::harden;

use super::hunks;
use super::model::{
    FileHunks, RepoFact, Sample, TouchKind, Touched, MAX_PATCH_BYTES, MAX_SAMPLE_FILES, MAX_STAT_FILES,
    PROBE_TIMEOUT_MS,
};
use super::noise;
use super::repo;

/// Output cap for `rev-parse HEAD`: one commit id is 40 or 64 characters.
const HEAD_MAX_BYTES: usize = 1024;
/// Output cap for the two file lists. Well above `MAX_STAT_FILES` paths of `MAX_PATH` bytes each.
const LIST_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Why a probe failed (SPEC-n33 section 3.2). Shared with `compare` (CA-8): keep these names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// The folder is not a git checkout (git says so).
    NotGit,
    /// The start commit is gone or is not a commit id (for example after a history rewrite).
    StartLost,
    /// One git process hit `PROBE_TIMEOUT_MS` and was killed.
    TooSlow,
    /// Anything else. The text is for the log only, never for a card.
    Failed(String),
}

/// The git reads of the watch. The engine owns one probe per step.
pub trait Probe {
    /// The repo a pane's folder belongs to. No process.
    fn repo_of(&self, cwd: &str) -> Option<RepoFact>;
    /// A copy's start commit (its `base_commit`). `None` when unknown.
    fn start_of(&self, repo: &RepoFact) -> Option<String>;
    /// The changed files of a checkout since `start` (`""` means HEAD).
    fn sample(&self, root: &str, start: &str) -> Result<Sample, ProbeError>;
    /// The changed lines of one file since `start` (`""` means HEAD). `rel` is a clean relative path.
    fn hunks(&self, root: &str, start: &str, rel: &str) -> Result<FileHunks, ProbeError>;
}

/// The real probe. `hooks` is PM's empty private folder (`<app data>/collision/empty-hooks`), which
/// `harden::git_env` points git at, so a repo's own hooks never run.
pub struct GitProbe {
    pub hooks: PathBuf,
}

impl GitProbe {
    fn env(&self) -> Vec<(String, String)> {
        harden::git_env(&self.hooks)
    }
}

impl Probe for GitProbe {
    fn repo_of(&self, cwd: &str) -> Option<RepoFact> {
        repo::repo_of(Path::new(cwd))
    }

    /// Only the live layer knows a copy's `base_commit` (from `worktrees::list`), so this is `None`.
    fn start_of(&self, _repo: &RepoFact) -> Option<String> {
        None
    }

    fn sample(&self, root: &str, start: &str) -> Result<Sample, ProbeError> {
        let began = Instant::now();
        // A start that is not a commit id is never handed to git.
        if !start.is_empty() && !is_commit_id(start) {
            return Err(ProbeError::StartLost);
        }
        let dir = Path::new(root);
        let env = self.env();
        let head = harden::read(dir, &["rev-parse", "HEAD"], &env, HEAD_MAX_BYTES, PROBE_TIMEOUT_MS)
            .map_err(not_git)?
            .text
            .trim()
            .to_string();
        // `diff-index` compares the start commit with the working tree.
        let base = if start.is_empty() { head.as_str() } else { start };
        let changed = harden::read(
            dir,
            &["diff-index", "--name-status", "-z", "--no-renames", base],
            &env,
            LIST_MAX_BYTES,
            PROBE_TIMEOUT_MS,
        )
        .map_err(|error| start_lost(error, start))?;
        let untracked = harden::read(
            dir,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            &env,
            LIST_MAX_BYTES,
            PROBE_TIMEOUT_MS,
        )
        .map_err(timed)?;

        let mut truncated = changed.truncated || untracked.truncated;
        // One entry per clean, non-noise path. A path that two lists name counts once.
        let mut found: BTreeMap<String, TouchKind> = BTreeMap::new();
        let listed = parse_changed(&changed.text)
            .into_iter()
            .chain(parse_untracked(&untracked.text).into_iter().map(|raw| (raw, TouchKind::Added)));
        for (raw, kind) in listed {
            let Some(rel) = noise::clean_rel(&raw) else { continue };
            if !noise::is_noise(&rel) {
                found.entry(rel).or_insert(kind);
            }
        }
        if found.len() > MAX_STAT_FILES {
            truncated = true;
        }

        let mut files: Vec<Touched> = Vec::new();
        for (rel, kind) in found.into_iter().take(MAX_STAT_FILES) {
            let mtime_ms = match kind {
                TouchKind::Deleted => None,
                _ => match modified_ms(&dir.join(&rel)) {
                    Some(ms) => Some(ms),
                    // Gone since the list was read, or a folder: not a file change.
                    None => continue,
                },
            };
            files.push(Touched { path: rel, kind, mtime_ms });
        }
        // Newest first. A deletion has no time and counts as newest. Ties keep path order.
        files.sort_by(|a, b| {
            b.mtime_ms
                .unwrap_or(u64::MAX)
                .cmp(&a.mtime_ms.unwrap_or(u64::MAX))
                .then_with(|| a.path.cmp(&b.path))
        });
        if files.len() > MAX_SAMPLE_FILES {
            files.truncate(MAX_SAMPLE_FILES);
            truncated = true;
        }
        Ok(Sample {
            head,
            files,
            truncated,
            cost_ms: began.elapsed().as_millis() as u64,
        })
    }

    fn hunks(&self, root: &str, start: &str, rel: &str) -> Result<FileHunks, ProbeError> {
        if !start.is_empty() && !is_commit_id(start) {
            return Err(ProbeError::StartLost);
        }
        if noise::clean_rel(rel).as_deref() != Some(rel) {
            return Err(ProbeError::Failed("not a clean relative path".to_string()));
        }
        let base = if start.is_empty() { "HEAD" } else { start };
        // `--` ends the revisions, so `rel` can only be a path.
        let patch = harden::read(
            Path::new(root),
            &["diff", "-U0", base, "--", rel],
            &self.env(),
            MAX_PATCH_BYTES,
            PROBE_TIMEOUT_MS,
        )
        .map_err(|error| start_lost(error, start))?;
        Ok(hunks::parse_u0(&patch.text, patch.truncated))
    }
}

/// A timeout is `TooSlow`; anything else is `Failed` with git's message.
fn timed(error: OperationError) -> ProbeError {
    if error.code == "GIT_TIMEOUT" {
        ProbeError::TooSlow
    } else {
        ProbeError::Failed(error.message)
    }
}

/// `rev-parse HEAD` that git refuses (not a timeout): the folder is not a checkout.
fn not_git(error: OperationError) -> ProbeError {
    if error.code == "GIT_FAILED" {
        ProbeError::NotGit
    } else {
        timed(error)
    }
}

/// A diff against a start commit that git refuses: the start is gone. Without a start, a refusal
/// is a plain failure (HEAD is known to exist by then).
fn start_lost(error: OperationError, start: &str) -> ProbeError {
    if error.code == "GIT_FAILED" && !start.is_empty() {
        ProbeError::StartLost
    } else {
        timed(error)
    }
}

/// `diff-index --name-status -z`: `status NUL path NUL` records. The text after the last NUL is
/// always an unfinished record (empty when the listing is whole), so it is dropped.
fn parse_changed(text: &str) -> Vec<(String, TouchKind)> {
    let fields: Vec<&str> = text.split('\0').collect();
    fields[..fields.len().saturating_sub(1)]
        .chunks_exact(2)
        .map(|pair| (pair[1].to_string(), kind_of(pair[0])))
        .collect()
}

/// `A` is added, `D` is deleted, anything else (`M`, `T`, unmerged) is modified.
fn kind_of(status: &str) -> TouchKind {
    match status.chars().next() {
        Some('A') => TouchKind::Added,
        Some('D') => TouchKind::Deleted,
        _ => TouchKind::Modified,
    }
}

/// `ls-files --others -z`: one path per NUL record. The unfinished tail is dropped.
fn parse_untracked(text: &str) -> Vec<String> {
    let fields: Vec<&str> = text.split('\0').collect();
    fields[..fields.len().saturating_sub(1)]
        .iter()
        .filter(|path| !path.is_empty())
        .map(|path| path.to_string())
        .collect()
}

/// A commit id as git prints it (4 to 64 hex digits). Anything else never reaches git.
fn is_commit_id(text: &str) -> bool {
    (4..=64).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Modified time in milliseconds since the epoch. `None` for a folder or a missing path. A link is
/// read as a link (`symlink_metadata`), never followed.
fn modified_ms(path: &Path) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.is_dir() {
        return None;
    }
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(since.as_millis() as u64)
}

/// How many times each probe method ran (tests only).
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Calls {
    pub repo_of: u32,
    pub start_of: u32,
    pub sample: u32,
    pub hunks: u32,
}

/// Scripted probe for the engine and alert tests. Every method counts its call. Scripts are set
/// through `&self` so a test can change them between two engine steps. A root with no scripted
/// sample fails with `Failed`; a cwd with no scripted repo has none.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeProbe {
    repos: RefCell<HashMap<String, RepoFact>>,
    starts: RefCell<HashMap<String, String>>,
    sample_script: RefCell<HashMap<String, VecDeque<Result<Sample, ProbeError>>>>,
    hunk_script: RefCell<HashMap<(String, String), Result<FileHunks, ProbeError>>>,
    calls: Cell<Calls>,
}

#[cfg(test)]
impl FakeProbe {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The repo that `cwd` belongs to.
    pub(crate) fn set_repo(&self, cwd: &str, repo: RepoFact) {
        self.repos.borrow_mut().insert(cwd.to_string(), repo);
    }

    /// The start commit of a checkout (what `start_of` returns for a repo with this root).
    pub(crate) fn set_start(&self, root: &str, start: &str) {
        self.starts.borrow_mut().insert(root.to_string(), start.to_string());
    }

    /// Queue the next sample for a root. Each call takes the next queued value; the last one repeats.
    pub(crate) fn push_sample(&self, root: &str, sample: Result<Sample, ProbeError>) {
        self.sample_script.borrow_mut().entry(root.to_string()).or_default().push_back(sample);
    }

    /// The hunks of one file of a checkout.
    pub(crate) fn set_hunks(&self, root: &str, rel: &str, hunks: Result<FileHunks, ProbeError>) {
        self.hunk_script.borrow_mut().insert((root.to_string(), rel.to_string()), hunks);
    }

    pub(crate) fn calls(&self) -> Calls {
        self.calls.get()
    }

    fn count(&self, bump: impl FnOnce(&mut Calls)) {
        let mut calls = self.calls.get();
        bump(&mut calls);
        self.calls.set(calls);
    }
}

#[cfg(test)]
impl Probe for FakeProbe {
    fn repo_of(&self, cwd: &str) -> Option<RepoFact> {
        self.count(|c| c.repo_of += 1);
        self.repos.borrow().get(cwd).cloned()
    }

    fn start_of(&self, repo: &RepoFact) -> Option<String> {
        self.count(|c| c.start_of += 1);
        self.starts.borrow().get(&repo.root).cloned()
    }

    fn sample(&self, root: &str, _start: &str) -> Result<Sample, ProbeError> {
        self.count(|c| c.sample += 1);
        let mut scripts = self.sample_script.borrow_mut();
        let Some(queue) = scripts.get_mut(root) else {
            return Err(ProbeError::Failed(format!("no scripted sample for {root}")));
        };
        let next = if queue.len() > 1 { queue.pop_front() } else { queue.front().cloned() };
        next.unwrap_or_else(|| Err(ProbeError::Failed("no scripted sample".to_string())))
    }

    fn hunks(&self, root: &str, _start: &str, rel: &str) -> Result<FileHunks, ProbeError> {
        self.count(|c| c.hunks += 1);
        self.hunk_script
            .borrow()
            .get(&(root.to_string(), rel.to_string()))
            .cloned()
            .unwrap_or_else(|| Err(ProbeError::Failed(format!("no scripted hunks for {rel}"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::model::Span;
    use crate::proc_util::hidden_command;

    // ------------------------------------------------------------------------------ helpers

    /// A temp folder, removed when the test ends (read-only git objects are cleared first).
    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            clear_readonly(&self.0);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn clear_readonly(dir: &Path) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                let mut perms = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&path, perms);
                if meta.is_dir() {
                    clear_readonly(&path);
                }
            }
        }
    }

    fn temp(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("pmprobe-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn git_available() -> bool {
        hidden_command("git").arg("--version").output().map(|out| out.status.success()).unwrap_or(false)
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = hidden_command("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Forward slashes, so a path is safe inside a `sh -c` string.
    fn sh_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn text(dir: &Path) -> String {
        dir.to_string_lossy().into_owned()
    }

    /// `src/auth.ts` as 100 numbered lines, so a line number in a test is easy to read.
    fn numbered() -> String {
        (1..=100).map(|n| format!("line {n}\n")).collect()
    }

    /// `src/auth.ts` with line 50 changed (old side line 50).
    fn edit_line_50() -> String {
        numbered().replace("line 50\n", "line 50 edited\n")
    }

    /// A repo on `main` with `a.txt`, `b.txt`, `db.ts` and `src/auth.ts`, committed.
    fn repo(tag: &str) -> Dir {
        let dir = temp(tag);
        let root = &dir.0;
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["config", "core.autocrlf", "false"]);
        write(root, "a.txt", "alpha\n");
        write(root, "b.txt", "bravo\n");
        write(root, "db.ts", "export const db = 1;\n");
        write(root, "src/auth.ts", &numbered());
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "initial"]);
        dir
    }

    /// The probe as PM builds it: PM's own empty hooks folder (kept alive by the caller).
    fn probe_with(hooks: &Dir) -> GitProbe {
        GitProbe { hooks: hooks.0.clone() }
    }

    fn paths(files: &[Touched]) -> Vec<&str> {
        files.iter().map(|file| file.path.as_str()).collect()
    }

    fn touched(files: &[Touched], path: &str) -> Option<Touched> {
        files.iter().find(|file| file.path == path).cloned()
    }

    /// Plant what an agent could write into a copy: a fsmonitor command, a hooks folder, a textconv
    /// and a clean filter driver, and an attribute that uses them. Each one writes its own marker.
    fn plant(root: &Path, tag: &str, markers: &Path, hooks: &Path) {
        let marker = |name: &str| sh_path(&markers.join(format!("{tag}-{name}.txt")));
        write(hooks, "pre-commit", &format!("#!/bin/sh\necho hook > \"{}\"\nexit 0\n", marker("hook")));
        git(root, &["config", "core.fsmonitor", &format!("sh -c 'echo fsm > \"{}\"'", marker("fsm"))]);
        git(root, &["config", "core.hooksPath", &sh_path(hooks)]);
        git(root, &["config", "diff.x.textconv", &format!("sh -c 'echo tc > \"{}\"; cat'", marker("tc"))]);
        git(root, &["config", "filter.x.clean", &format!("sh -c 'echo clean > \"{}\"; cat'", marker("clean"))]);
        write(root, ".gitattributes", "*.txt diff=x filter=x\n");
        write(root, "a.txt", "alpha\nchanged\n");
    }

    fn empty_sample() -> Sample {
        Sample { head: "h".to_string(), files: Vec::new(), truncated: false, cost_ms: 0 }
    }

    // ----------------------------------------------------------------------- pure parts

    #[test]
    fn a_changed_listing_gives_the_kind_of_each_record() {
        assert_eq!(
            parse_changed("M\0a.txt\0A\0b.txt\0D\0c.txt\0T\0d.txt\0"),
            vec![
                ("a.txt".to_string(), TouchKind::Modified),
                ("b.txt".to_string(), TouchKind::Added),
                ("c.txt".to_string(), TouchKind::Deleted),
                ("d.txt".to_string(), TouchKind::Modified),
            ]
        );
        assert!(parse_changed("").is_empty());
    }

    #[test]
    fn a_cut_listing_drops_its_unfinished_record() {
        // Cut after the status of the second record, and in the middle of its path.
        assert_eq!(parse_changed("M\0a.txt\0A\0b.t").len(), 1);
        assert_eq!(parse_changed("M\0a.txt\0A").len(), 1);
        assert_eq!(parse_untracked("new.ts\0ne"), vec!["new.ts"]);
        assert_eq!(parse_untracked("x.ts\0\0"), vec!["x.ts"]);
    }

    #[test]
    fn git_errors_map_to_probe_errors() {
        assert_eq!(timed(OperationError::new("GIT_TIMEOUT", "slow", true)), ProbeError::TooSlow);
        assert_eq!(
            timed(OperationError::new("GIT_FAILED", "git diff failed", false)),
            ProbeError::Failed("git diff failed".to_string())
        );
        assert_eq!(not_git(OperationError::new("GIT_FAILED", "fatal", false)), ProbeError::NotGit);
        assert_eq!(not_git(OperationError::new("GIT_TIMEOUT", "slow", true)), ProbeError::TooSlow);
        assert_eq!(start_lost(OperationError::new("GIT_FAILED", "bad", false), "abcd"), ProbeError::StartLost);
        assert!(matches!(
            start_lost(OperationError::new("GIT_FAILED", "bad", false), ""),
            ProbeError::Failed(_)
        ));
        assert_eq!(start_lost(OperationError::new("GIT_TIMEOUT", "slow", true), "abcd"), ProbeError::TooSlow);
    }

    #[test]
    fn commit_ids_are_hex_of_four_to_sixty_four_digits() {
        assert!(is_commit_id("abcd"));
        assert!(is_commit_id(&"f".repeat(64)));
        assert!(is_commit_id("0123456789abcdef0123456789ABCDEF01234567"));
        assert!(!is_commit_id(""));
        assert!(!is_commit_id("abc"));
        assert!(!is_commit_id(&"f".repeat(65)));
        assert!(!is_commit_id("main"));
        assert!(!is_commit_id("abcd -x"));
        assert!(!is_commit_id("abcd\0"));
    }

    #[test]
    fn fake_probe_repeats_its_last_sample_and_counts_every_call() {
        let fake = FakeProbe::new();
        fake.push_sample("/r", Ok(empty_sample()));
        fake.push_sample("/r", Err(ProbeError::TooSlow));
        assert!(fake.sample("/r", "").is_ok());
        assert_eq!(fake.sample("/r", "").unwrap_err(), ProbeError::TooSlow);
        assert_eq!(fake.sample("/r", "").unwrap_err(), ProbeError::TooSlow);
        assert!(fake.sample("/unscripted", "").is_err());
        assert_eq!(fake.repo_of("/x"), None);
        assert_eq!(fake.calls().sample, 4);
        assert_eq!(fake.calls().repo_of, 1);
        assert_eq!(fake.calls().hunks, 0);
    }

    // --------------------------------------------------------------------- real repos

    #[test]
    fn an_edited_an_added_and_a_deleted_file_are_reported_with_their_kinds() {
        if !git_available() {
            return;
        }
        let dir = repo("kinds");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        write(&root, "src/auth.ts", &edit_line_50());
        write(&root, "new.ts", "export const fresh = true;\n");
        std::fs::remove_file(root.join("db.ts")).unwrap();

        let sample = probe_with(&hooks).sample(&text(&root), "").expect("a sample");

        assert_eq!(sample.head, git(&root, &["rev-parse", "HEAD"]));
        assert!(!sample.truncated);
        assert_eq!(sample.files.len(), 3, "{:?}", sample.files);
        let edited = touched(&sample.files, "src/auth.ts").expect("the edited file");
        assert_eq!(edited.kind, TouchKind::Modified);
        assert!(edited.mtime_ms.is_some());
        assert_eq!(touched(&sample.files, "new.ts").expect("the added file").kind, TouchKind::Added);
        let gone = touched(&sample.files, "db.ts").expect("the deleted file");
        assert_eq!(gone.kind, TouchKind::Deleted);
        assert_eq!(gone.mtime_ms, None);
    }

    #[test]
    fn lock_files_and_dependency_folders_are_dropped() {
        if !git_available() {
            return;
        }
        let dir = repo("noise");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        write(&root, "src/auth.ts", &edit_line_50());
        write(&root, "package-lock.json", "{}\n");
        write(&root, "node_modules/left-pad/index.js", "module.exports = 1;\n");
        write(&root, "build.log", "noise\n");

        let sample = probe_with(&hooks).sample(&text(&root), "").expect("a sample");

        assert_eq!(paths(&sample.files), vec!["src/auth.ts"], "{:?}", sample.files);
    }

    #[test]
    fn a_committed_change_since_the_start_commit_is_reported() {
        if !git_available() {
            return;
        }
        let dir = repo("committed");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        let start = git(&root, &["rev-parse", "HEAD"]);
        write(&root, "src/auth.ts", &edit_line_50());
        git(&root, &["commit", "-q", "-am", "agent edit"]);
        let probe = probe_with(&hooks);

        let since = probe.sample(&text(&root), &start).expect("a sample from the start");

        assert_eq!(paths(&since.files), vec!["src/auth.ts"]);
        assert_eq!(since.files[0].kind, TouchKind::Modified);
        assert_ne!(since.head, start);
        // From the new HEAD there is nothing left to report.
        assert!(probe.sample(&text(&root), "").expect("a sample").files.is_empty());
    }

    #[test]
    fn a_stat_dirty_file_is_listed_and_its_hunks_are_empty() {
        if !git_available() {
            return;
        }
        let dir = repo("stat");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        // Same bytes, new modified time: only git's stat record is stale.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&root, "src/auth.ts", &numbered());
        let probe = probe_with(&hooks);

        let sample = probe.sample(&text(&root), "").expect("a sample");
        assert_eq!(touched(&sample.files, "src/auth.ts").map(|file| file.kind), Some(TouchKind::Modified));

        // The hunk read finds nothing: the file is dropped by the engine's compare step.
        let hunks = probe.hunks(&text(&root), "", "src/auth.ts").expect("hunks");
        assert!(hunks.spans.is_empty(), "{:?}", hunks.spans);
        assert!(!hunks.binary && !hunks.cut);
    }

    #[test]
    fn a_start_commit_that_is_gone_is_start_lost() {
        if !git_available() {
            return;
        }
        let dir = repo("lost");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        let probe = probe_with(&hooks);
        let missing = "0123456789abcdef0123456789abcdef01234567";

        assert_eq!(probe.sample(&text(&root), missing).unwrap_err(), ProbeError::StartLost);
        // Not a commit id at all: refused before any git call.
        assert_eq!(probe.sample(&text(&root), "not a commit").unwrap_err(), ProbeError::StartLost);
        assert_eq!(probe.hunks(&text(&root), missing, "a.txt").unwrap_err(), ProbeError::StartLost);
    }

    #[test]
    fn a_folder_that_is_not_a_checkout_is_not_git() {
        if !git_available() {
            return;
        }
        let dir = temp("plain");
        let hooks = temp("hooks");
        let probe = probe_with(&hooks);

        assert_eq!(probe.sample(&text(&dir.0), "").unwrap_err(), ProbeError::NotGit);
        assert!(probe.repo_of(&text(&dir.0)).is_none());
    }

    #[test]
    fn repo_of_finds_the_checkout_and_start_of_is_none() {
        if !git_available() {
            return;
        }
        let dir = repo("repo-of");
        let hooks = temp("hooks");
        let probe = probe_with(&hooks);

        let fact = probe.repo_of(&text(&dir.0)).expect("a repo");
        assert_eq!(fact.root, text(&dir.0));
        assert!(!fact.linked);
        assert_eq!(probe.start_of(&fact), None);
    }

    #[test]
    fn a_hostile_config_runs_nothing_in_the_probe() {
        if !git_available() {
            return;
        }
        let dir = repo("hostile");
        let root = dir.0.clone();
        let markers = temp("markers");
        let planted_hooks = temp("planted-hooks");
        plant(&root, "hostile", &markers.0, &planted_hooks.0);
        let hooks = temp("pm-hooks");
        let probe = probe_with(&hooks);

        let sample = probe.sample(&text(&root), "").expect("the sample still works");
        assert!(touched(&sample.files, "a.txt").is_some(), "{:?}", sample.files);
        probe.hunks(&text(&root), "", "a.txt").expect("the hunk read still works");
        for name in ["fsm", "tc", "clean", "hook"] {
            let marker = markers.0.join(format!("hostile-{name}.txt"));
            assert!(!marker.exists(), "the probe ran the planted {name}");
        }

        // Control: plain git runs the planted fsmonitor on this same copy, so the empty markers
        // above mean something.
        let _ = hidden_command("git").args(["status", "--porcelain"]).current_dir(&root).output();
        assert!(markers.0.join("hostile-fsm.txt").exists(), "the control did not fire the planted fsmonitor");
    }

    #[test]
    fn a_stale_index_lock_does_not_stop_a_read_and_the_index_bytes_do_not_change() {
        if !git_available() {
            return;
        }
        let dir = repo("lock");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        write(&root, "src/auth.ts", &edit_line_50());
        // An agent's own git left its lock behind.
        write(&root, ".git/index.lock", "");
        let index = root.join(".git").join("index");
        let before = std::fs::read(&index).unwrap();
        let probe = probe_with(&hooks);

        let sample = probe.sample(&text(&root), "").expect("the read works with the lock present");
        assert_eq!(paths(&sample.files), vec!["src/auth.ts"]);
        probe.hunks(&text(&root), "", "src/auth.ts").expect("the hunk read works too");

        assert_eq!(std::fs::read(&index).unwrap(), before, "the index was rewritten");
    }

    #[test]
    fn more_than_400_changed_files_are_cut_to_the_newest_400() {
        if !git_available() {
            return;
        }
        let dir = repo("many");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        for n in 0..500 {
            write(&root, &format!("gen/f{n:03}.txt"), "x\n");
        }

        let sample = probe_with(&hooks).sample(&text(&root), "").expect("a sample");

        assert_eq!(sample.files.len(), MAX_SAMPLE_FILES);
        assert!(sample.truncated);
        assert!(sample.files.windows(2).all(|pair| pair[0].mtime_ms >= pair[1].mtime_ms));
    }

    #[test]
    fn hunks_give_the_changed_old_side_lines_of_one_file() {
        if !git_available() {
            return;
        }
        let dir = repo("hunks");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        let edited = edit_line_50().replace("line 90\n", "line 90 edited\n");
        write(&root, "src/auth.ts", &edited);

        let found = probe_with(&hooks).hunks(&text(&root), "", "src/auth.ts").expect("hunks");

        assert_eq!(found.spans, vec![Span { start: 50, end: 50 }, Span { start: 90, end: 90 }]);
        assert!(!found.binary && !found.cut);
    }

    #[test]
    fn a_binary_file_is_flagged_and_has_no_lines() {
        if !git_available() {
            return;
        }
        let dir = repo("binary");
        let hooks = temp("hooks");
        let root = dir.0.clone();
        std::fs::write(root.join("logo.bin"), [0u8, 1, 2, 3]).unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "logo"]);
        std::fs::write(root.join("logo.bin"), [0u8, 9, 9, 9, 9]).unwrap();

        let found = probe_with(&hooks).hunks(&text(&root), "", "logo.bin").expect("hunks");

        assert!(found.binary);
        assert!(found.spans.is_empty());
    }

    #[test]
    fn a_hunk_path_must_be_clean_and_relative() {
        let hooks = temp("hooks");
        let probe = probe_with(&hooks);
        assert!(matches!(
            probe.hunks("/anywhere", "", "../escape.txt"),
            Err(ProbeError::Failed(_))
        ));
        assert!(matches!(probe.hunks("/anywhere", "", "/etc/passwd"), Err(ProbeError::Failed(_))));
    }
}
