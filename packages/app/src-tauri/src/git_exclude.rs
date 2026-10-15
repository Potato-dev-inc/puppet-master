//! Keep `<project>/.puppet-master/` out of the user's git status.
//!
//! Puppet Master stores checkpoints, isolated worktrees, the operations store, turn summaries and
//! the event log under `<project>/.puppet-master/`. In a git repository that would show up as
//! untracked files and could be committed by accident (`events.jsonl` would leak to GitHub). The
//! `.puppet-master/` also holds files meant to be shared through git: `memory.md` (project memory)
//! and `agents/*.toml` (agent definitions). So the first time PM creates that folder inside a git
//! work tree, this writes a managed block to the repository's `info/exclude` that hides everything
//! except those:
//!
//! ```text
//! # Puppet Master runtime data (local, added automatically)
//! .puppet-master/*
//! !.puppet-master/memory.md
//! !.puppet-master/agents/
//! ```
//!
//! An older version wrote the plain line `.puppet-master/`; that line is replaced by the block. No
//! other line of the file is touched.
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

/// The comment line that starts the managed block in `info/exclude`.
pub const BLOCK_HEADER: &str = "# Puppet Master runtime data (local, added automatically)";

/// The managed block written to `info/exclude`, for a project at `rel` below the repository root
/// (`rel` uses `/`, and is empty when the project IS the root). Runtime data is hidden; the
/// team-shared files are re-included. Negations only work because the directory is matched by
/// `.puppet-master/*`, not by `.puppet-master/` (git cannot re-include a file inside an excluded
/// directory). A pattern with a slash in it is anchored to the root, so a nested project gets its
/// own path prefix. At the root the block is exactly the three lines of the spec.
fn block_lines(rel: &str) -> [String; 4] {
    let dir = if rel.is_empty() { ".puppet-master".to_string() } else { format!("{rel}/.puppet-master") };
    [
        BLOCK_HEADER.to_string(),
        format!("{dir}/*"),
        format!("!{dir}/memory.md"),
        format!("!{dir}/agents/"),
    ]
}

/// The plain line an earlier version wrote. It hides the shared files too, so it is migrated.
const LEGACY_LINE: &str = ".puppet-master/";

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
    let rel = project
        .strip_prefix(&repo.work_tree)
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default();
    let block = block_lines(&rel);

    let existing = std::fs::read_to_string(&exclude_file).unwrap_or_default();
    if has_block(&existing, &block) {
        return Outcome::AlreadyCovered;
    }
    // A legacy plain line is always migrated, even if some other ignore file also covers it.
    if !has_legacy_line(&existing) {
        if covers(&existing, at_root) {
            return Outcome::AlreadyCovered;
        }
        // The work tree's own .gitignore may already ignore it. We only ever read that file.
        if let Ok(gitignore) = std::fs::read_to_string(repo.work_tree.join(".gitignore")) {
            if covers(&gitignore, at_root) {
                return Outcome::AlreadyCovered;
            }
        }
    }

    if let Some(parent) = exclude_file.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return Outcome::Failed(format!("create {}: {error}", parent.display()));
        }
    }
    match std::fs::write(&exclude_file, with_block(&existing, &block)) {
        Ok(()) => Outcome::Added(exclude_file),
        Err(error) => Outcome::Failed(format!("write {}: {error}", exclude_file.display())),
    }
}

/// Is the managed block already present, exactly as written by `with_block`?
fn has_block(exclude: &str, block: &[String]) -> bool {
    let lines: Vec<&str> = exclude.lines().map(str::trim_end).collect();
    lines.windows(block.len()).any(|window| window.iter().zip(block).all(|(a, b)| *a == b.as_str()))
}

/// Is the old plain `.puppet-master/` line present (at least once)?
fn has_legacy_line(exclude: &str) -> bool {
    exclude.lines().any(|line| line.trim() == LEGACY_LINE)
}

/// The exclude file with the managed block in place. Every legacy line becomes the block (once);
/// all other lines are copied byte for byte, including their line endings. With no legacy line the
/// block is appended. A file that already has the block is returned unchanged.
fn with_block(existing: &str, block: &[String]) -> String {
    if has_block(existing, block) {
        return existing.to_string();
    }
    let eol = if existing.contains("\r\n") { "\r\n" } else { "\n" };
    let text: String = block.iter().map(|line| format!("{line}{eol}")).collect();

    let mut out = String::with_capacity(existing.len() + text.len());
    let mut placed = false;
    for raw in existing.split_inclusive('\n') {
        if raw.trim() == LEGACY_LINE {
            if !placed {
                out.push_str(&text);
                placed = true;
            }
            continue;
        }
        out.push_str(raw);
    }
    if !placed {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&text);
    }
    out
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

/// The anchored `info/exclude` line for one file PM created at `rel_file` inside `project`.
fn file_line(project: &Path, repo: &Repo, rel_file: &str) -> String {
    let base = project
        .strip_prefix(&repo.work_tree)
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default();
    let file = rel_file.replace('\\', "/");
    if base.is_empty() { format!("/{file}") } else { format!("/{base}/{file}") }
}

/// Hide ONE file Puppet Master created (Hook Lane: `.codex/hooks.json`) in `info/exclude`. Only
/// ever called for a file PM created, never for a file the user had. Best effort, like the block.
pub fn exclude_file(project: &Path, rel_file: &str) -> bool {
    let Some(repo) = find_repo(project) else { return false };
    let exclude = repo.common_dir.join("info").join("exclude");
    let line = file_line(project, &repo, rel_file);
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|l| l.trim_end() == line) {
        return true;
    }
    let _ = std::fs::create_dir_all(exclude.parent().unwrap_or(&repo.common_dir));
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    std::fs::write(&exclude, format!("{existing}{sep}# Puppet Master hook file (local, added automatically)\n{line}\n")).is_ok()
}

/// Undo [`exclude_file`]: its two lines go, every other line stays byte for byte.
pub fn unexclude_file(project: &Path, rel_file: &str) {
    let Some(repo) = find_repo(project) else { return };
    let exclude = repo.common_dir.join("info").join("exclude");
    let Ok(existing) = std::fs::read_to_string(&exclude) else { return };
    let line = file_line(project, &repo, rel_file);
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let here = lines[i].trim_end();
        if here == "# Puppet Master hook file (local, added automatically)" && lines.get(i + 1).map(|l| l.trim_end()) == Some(line.as_str()) {
            i += 2;
            continue;
        }
        if here == line {
            i += 1;
            continue;
        }
        out.push_str(lines[i]);
        i += 1;
    }
    if out != existing {
        let _ = std::fs::write(&exclude, out);
    }
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
        assert!(exclude.lines().any(|l| l == ".puppet-master/*"), "{exclude}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_lists_memory_and_agents_but_not_runtime_data() {
        let dir = repo("shareable");
        let pm = dir.join(".puppet-master");
        std::fs::create_dir_all(pm.join("checkpoints.git")).unwrap();
        std::fs::create_dir_all(pm.join("agents")).unwrap();
        std::fs::write(pm.join("events.jsonl"), "{}\n").unwrap();
        std::fs::write(pm.join("checkpoints.git/x"), "x").unwrap();
        std::fs::write(pm.join("memory.md"), "# memory\n").unwrap();
        std::fs::write(pm.join("agents/a.toml"), "name = \"a\"\n").unwrap();
        let status = || git(&dir, &["status", "--porcelain", "--untracked-files=all"]);
        assert!(status().contains(".puppet-master/events.jsonl"), "control: visible first");

        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        let after = status();
        assert!(after.contains("?? .puppet-master/memory.md"), "{after}");
        assert!(after.contains("?? .puppet-master/agents/a.toml"), "{after}");
        assert!(!after.contains("events.jsonl"), "{after}");
        assert!(!after.contains("checkpoints.git"), "{after}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_old_single_line_is_migrated_to_the_block_and_other_lines_survive() {
        let dir = repo("migrate");
        let exclude_file = dir.join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
        std::fs::write(&exclude_file, "*.log\n.puppet-master/\n# mine\nbuild/\n").unwrap();
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        let text = std::fs::read_to_string(&exclude_file).unwrap();
        let expected = format!("*.log\n{}\n# mine\nbuild/\n", block_lines("").join("\n"));
        assert_eq!(text, expected);
        assert!(!text.lines().any(|l| l == ".puppet-master/"), "legacy line gone");
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered, "migration is done once");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_legacy_line_at_the_end_without_newline_is_migrated() {
        let dir = repo("migrate-eof");
        let exclude_file = dir.join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
        std::fs::write(&exclude_file, "*.log\n.puppet-master/").unwrap();
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        assert_eq!(
            std::fs::read_to_string(&exclude_file).unwrap(),
            format!("*.log\n{}\n", block_lines("").join("\n"))
        );
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
        assert_eq!(first, format!("*.log\n{}\n", block_lines("").join("\n")));
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered);
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered);
        assert_eq!(std::fs::read_to_string(&exclude_file).unwrap(), first, "no second block");
        assert_eq!(first.matches(BLOCK_HEADER).count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn crlf_line_endings_are_kept_when_the_block_is_written() {
        let dir = repo("crlf");
        let exclude_file = dir.join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
        std::fs::write(&exclude_file, "*.log\r\n.puppet-master/\r\n").unwrap();
        assert!(matches!(ensure_in(&dir), Outcome::Added(_)));
        let text = std::fs::read_to_string(&exclude_file).unwrap();
        assert_eq!(text, format!("*.log\r\n{}\r\n", block_lines("").join("\r\n")));
        assert_eq!(ensure_in(&dir), Outcome::AlreadyCovered);
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
        std::fs::write(sub.join(".puppet-master/memory.md"), "# memory\n").unwrap();
        assert!(matches!(ensure_in(&sub), Outcome::Added(_)));
        let status = git(&dir, &["status", "--porcelain", "--untracked-files=all"]);
        assert!(status.contains("?? packages/app/.puppet-master/memory.md"), "{status}");
        assert!(!status.contains("events.jsonl") && !status.contains("worktrees"), "{status}");
        // The block is anchored to the nested project, so it does not hide other folders' storage.
        let exclude = std::fs::read_to_string(dir.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == "packages/app/.puppet-master/*"), "{exclude}");
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
        assert!(exclude.lines().any(|l| l == ".puppet-master/*"), "{exclude}");
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
