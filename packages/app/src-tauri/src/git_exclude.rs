//! Keep `<project>/.puppet-master/` out of the user's git status.
//!
//! Puppet Master stores checkpoints, isolated worktrees, the operations store, turn summaries and
//! the event log under `<project>/.puppet-master/`. In a git repository that would show up as
//! untracked files and could be committed by accident (`events.jsonl` would leak to GitHub). The
//! first time PM creates that folder inside a git work tree, this appends `.puppet-master/` to the
//! repository's `info/exclude`:
//!
//! - local only: `info/exclude` is never committed, and the user's `.gitignore` is never touched;
//! - idempotent: nothing is written when the folder is already covered (by `info/exclude` or by the
//!   work tree's `.gitignore`), and the result is remembered for the rest of the process;
//! - no `git` process: the repository is found by walking up from the project, so a worktree or
//!   submodule `.git` FILE (`gitdir: <path>`) and its `commondir` are followed by hand;
//! - best effort: a read-only repository only makes `git status` noisier, it never blocks PM.
//!
//! Outside a git work tree nothing at all is created or written.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The line written to `info/exclude`.
pub const EXCLUDE_LINE: &str = ".puppet-master/";

/// What `ensure_in` did, for tests and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not inside a git work tree: nothing was touched.
    NotARepo,
    /// Already covered by `info/exclude` or `.gitignore`: nothing was written.
    AlreadyCovered,
    /// The line was appended to this file.
    Added(PathBuf),
    /// Wanted to add the line but could not write the file.
    Failed(String),
}

/// Call this right before PM first writes under `<project>/.puppet-master/`. Cheap after the first
/// call for a project (one set lookup). Never fails and never panics.
pub fn ensure_project_storage_excluded(project: &Path) {
    static DONE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let done = DONE.get_or_init(|| Mutex::new(HashSet::new()));
    {
        let Ok(mut seen) = done.lock() else { return };
        if !seen.insert(project.to_path_buf()) {
            return;
        }
    }
    if let Outcome::Failed(error) = ensure_in(project) {
        tracing::warn!(%error, project = %project.display(), "could not exclude .puppet-master from git");
    }
}

/// The uncached work. `project` is the folder that holds (or will hold) `.puppet-master/`.
pub fn ensure_in(project: &Path) -> Outcome {
    let Some(repo) = find_repo(project) else { return Outcome::NotARepo };
    let exclude_file = repo.common_dir.join("info").join("exclude");
    let at_root = same_dir(project, &repo.work_tree);

    let existing = std::fs::read_to_string(&exclude_file).unwrap_or_default();
    if covers(&existing, at_root) {
        return Outcome::AlreadyCovered;
    }
    // The work tree's own .gitignore may already ignore it. We only ever read that file.
    if let Ok(gitignore) = std::fs::read_to_string(repo.work_tree.join(".gitignore")) {
        if covers(&gitignore, at_root) {
            return Outcome::AlreadyCovered;
        }
    }

    if let Some(parent) = exclude_file.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return Outcome::Failed(format!("create {}: {error}", parent.display()));
        }
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(EXCLUDE_LINE);
    next.push('\n');
    match std::fs::write(&exclude_file, next) {
        Ok(()) => Outcome::Added(exclude_file),
        Err(error) => Outcome::Failed(format!("write {}: {error}", exclude_file.display())),
    }
}

/// Does an ignore file already ignore `.puppet-master` wherever PM puts it? An anchored pattern
/// (`/.puppet-master`) only counts when the project is the repository root.
fn covers(ignore_file: &str, project_is_root: bool) -> bool {
    ignore_file.lines().map(str::trim).any(|line| match line {
        ".puppet-master" | ".puppet-master/" | "**/.puppet-master" | "**/.puppet-master/" => true,
        "/.puppet-master" | "/.puppet-master/" => project_is_root,
        _ => false,
    })
}

struct Repo {
    work_tree: PathBuf,
    /// Where git reads `info/exclude` from: the main repository's git dir, also for linked worktrees.
    common_dir: PathBuf,
}

fn find_repo(project: &Path) -> Option<Repo> {
    for dir in project.ancestors() {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            return Some(Repo { work_tree: dir.to_path_buf(), common_dir: dot_git });
        }
        if dot_git.is_file() {
            // A linked worktree or a submodule: `.git` is a file that says `gitdir: <path>`.
            let text = std::fs::read_to_string(&dot_git).ok()?;
            let target = text.lines().find_map(|l| l.trim().strip_prefix("gitdir:"))?.trim();
            let git_dir = resolve(dir, target);
            if !git_dir.is_dir() {
                return None;
            }
            // A linked worktree's git dir has `commondir` (usually `../..`) pointing at the main
            // repository's git dir, which is where `info/exclude` lives.
            let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
                Ok(text) if !text.trim().is_empty() => resolve(&git_dir, text.trim()),
                _ => git_dir,
            };
            return Some(Repo { work_tree: dir.to_path_buf(), common_dir });
        }
    }
    None
}

fn resolve(base: &Path, target: &str) -> PathBuf {
    let path = Path::new(target);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-git-exclude-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git is installed");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn repo(name: &str) -> PathBuf {
        let dir = temp(name);
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        dir
    }

    fn fake_pm_storage(project: &Path) {
        let storage = project.join(".puppet-master");
        std::fs::create_dir_all(storage.join("worktrees/h-1")).unwrap();
        std::fs::write(storage.join("events.jsonl"), "{}\n").unwrap();
        std::fs::write(storage.join("worktrees/h-1/file"), "x").unwrap();
    }

    #[test]
    fn a_repo_shows_untracked_puppet_master_before_and_nothing_after() {
        let dir = repo("untracked");
        fake_pm_storage(&dir);
        assert!(git(&dir, &["status", "--porcelain"]).contains(".puppet-master"), "control: it is visible first");
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        assert_eq!(git(&dir, &["status", "--porcelain"]).trim(), "", "nothing untracked after");
        let exclude = std::fs::read_to_string(dir.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == ".puppet-master/"), "{exclude}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn it_is_idempotent_and_keeps_what_was_already_in_the_exclude_file() {
        let dir = repo("idempotent");
        let exclude_file = dir.join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
        std::fs::write(&exclude_file, "*.log").unwrap(); // no trailing newline on purpose
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        let first = std::fs::read_to_string(&exclude_file).unwrap();
        assert_eq!(first, "*.log\n.puppet-master/\n");
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered);
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered);
        assert_eq!(std::fs::read_to_string(&exclude_file).unwrap(), first, "no second line");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_users_gitignore_is_never_written_and_counts_as_covering() {
        let dir = repo("gitignore");
        std::fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        assert_eq!(std::fs::read_to_string(dir.join(".gitignore")).unwrap(), "node_modules/\n", "untouched");

        let covered = repo("gitignore-covered");
        std::fs::write(covered.join(".gitignore"), "# mine\n.puppet-master\n").unwrap();
        let before = std::fs::read_to_string(covered.join(".git/info/exclude")).unwrap_or_default();
        assert_eq!(ensure_in(&covered), Outcome::AlreadyCovered);
        assert_eq!(std::fs::read_to_string(covered.join(".git/info/exclude")).unwrap_or_default(), before);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&covered).ok();
    }

    #[test]
    fn a_missing_info_directory_is_created() {
        let dir = repo("noinfo");
        std::fs::remove_dir_all(dir.join(".git/info")).ok();
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        assert!(dir.join(".git/info/exclude").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_that_is_not_a_repo_is_left_alone() {
        let dir = temp("plain");
        assert_eq!(ensure_in(&dir), Outcome::NotARepo);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing was created");
        // A fake .git FILE that points nowhere is also not a repo.
        std::fs::write(dir.join(".git"), "gitdir: /does/not/exist\n").unwrap();
        assert_eq!(ensure_in(&dir), Outcome::NotARepo);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_subfolder_of_a_repo_uses_the_repos_exclude() {
        let dir = repo("sub");
        let sub = dir.join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        fake_pm_storage(&sub);
        assert!(matches!(ensure_in(&sub), Outcome::Added(_)));
        assert_eq!(git(&dir, &["status", "--porcelain"]).trim(), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_linked_worktree_writes_to_the_main_repos_exclude() {
        let main = repo("linked-main");
        let linked = temp("linked-wt").join("wt");
        git(&main, &["worktree", "add", "-q", linked.to_str().unwrap(), "-b", "side"]);
        assert!(linked.join(".git").is_file(), "control: a worktree's .git is a file");
        fake_pm_storage(&linked);
        assert!(matches!(ensure_in(&linked), Outcome::Added(_)));
        assert_eq!(git(&linked, &["status", "--porcelain"]).trim(), "", "clean in the worktree");
        let exclude = std::fs::read_to_string(main.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == ".puppet-master/"), "{exclude}");
        assert_eq!(ensure_in(&linked), Outcome::AlreadyCovered);
        std::fs::remove_dir_all(&main).ok();
        std::fs::remove_dir_all(linked.parent().unwrap()).ok();
    }

    #[test]
    fn an_anchored_pattern_only_counts_at_the_repo_root() {
        let dir = repo("anchored");
        let exclude_file = dir.join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
        std::fs::write(&exclude_file, "/.puppet-master/\n").unwrap();
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered, "at the root it is enough");
        let sub = dir.join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(matches!(ensure_in(&sub), Outcome::Added(_)), "below the root it is not");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_cached_entry_point_never_panics_and_only_acts_once_per_project() {
        let dir = repo("cached");
        ensure_project_storage_excluded(&dir);
        let first = std::fs::read_to_string(dir.join(".git/info/exclude")).unwrap();
        ensure_project_storage_excluded(&dir);
        assert_eq!(std::fs::read_to_string(dir.join(".git/info/exclude")).unwrap(), first);
        ensure_project_storage_excluded(Path::new("Z:/definitely/not/here"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
