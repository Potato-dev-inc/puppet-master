//! Tests for the git and push calls Red to Green needs (spec n33 §3.4, threats S4, S5, S7, S9):
//! a fix is pushed only as a fast-forward of the copy's own `pm/` branch, only while the remote is
//! still the failing commit, never to the default branch or the PR base, and never with a hook or a
//! config the agent could have planted. Real temp git repositories and a bare remote only; each
//! hook attack has a control run without the hardened env, so a pass means the guard works.

use super::harden::{self, git_env};
use super::pr;
use super::proc::ToolEnv;
use super::*;
use std::process::Command;

// ------------------------------------------------------------------------------------ helpers

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmcifix-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn clear_readonly(dir: &Path) {
    if let Ok(read) = std::fs::read_dir(dir) {
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
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        clear_readonly(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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

/// A repo on `main` with `a.txt` and `src/lib.rs`, a bare `origin` it can push to, and `main`
/// pushed. Line endings and signing are pinned so the tests do not depend on the machine.
fn repo_with_origin() -> (Dir, PathBuf, Dir, PathBuf) {
    let root = temp_dir("repo");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    write(&root, "a.txt", "alpha\n");
    write(&root, "src/lib.rs", "pub fn one() {}\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    let bare = temp_dir("origin");
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(&root, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&root, &["push", "-q", "origin", "main"]);
    (Dir(root.clone()), root, Dir(bare.clone()), bare)
}

fn project_str(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

/// A marker path with forward slashes, safe inside a `sh` hook.
fn marker_in(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().replace('\\', "/")
}

/// An empty private hooks folder, the way the issue flow makes one.
fn empty_hooks() -> Dir {
    Dir(temp_dir("empty-hooks"))
}

/// A hook file body that leaves a marker and exits 0.
fn marker_hook(marker: &str) -> String {
    format!("#!/bin/sh\necho ran > \"{marker}\"\nexit 0\n")
}

/// A copy of `root` on a new branch `pm/<handle>`, the way Red to Green gets its copy.
fn copy_of(root: &Path, handle: &str) -> WorktreeRef {
    create_with(&project_str(root), handle, &CreateOptions::default()).unwrap()
}

/// Write `name` and commit only that file (untracked helper files stay out). Returns the new head.
fn commit_file(cwd: &Path, name: &str, text: &str) -> String {
    write(cwd, name, text);
    git(cwd, &["add", "--", name]);
    git(cwd, &["commit", "-q", "--no-verify", "-m", name]);
    git(cwd, &["rev-parse", "HEAD"])
}

/// The commit a branch points to on the bare remote.
fn remote_sha(bare: &Path, branch: &str) -> String {
    git(bare, &["rev-parse", &format!("refs/heads/{branch}")])
}

/// Another person's clone pushes a commit to `branch` on the remote. Returns that commit.
fn push_from_another_clone(bare: &Path, branch: &str) -> String {
    let other = Dir(temp_dir("other-clone"));
    git(&other.0, &["clone", "-q", &bare.to_string_lossy(), "clone"]);
    let clone = other.0.join("clone");
    git(&clone, &["config", "user.name", "Other"]);
    git(&clone, &["config", "user.email", "other@example.com"]);
    git(&clone, &["config", "commit.gpgsign", "false"]);
    git(&clone, &["checkout", "-q", branch]);
    let sha = commit_file(&clone, "other.txt", "someone else\n");
    git(&clone, &["push", "-q", "origin", branch]);
    sha
}

/// The body of the top-level function whose signature starts with `signature` in `src`.
fn fn_body<'a>(src: &'a str, signature: &str) -> &'a str {
    let start = src.find(signature).unwrap_or_else(|| panic!("{signature} is in the source"));
    let rest = &src[start..];
    let end = rest.find("\n}\n").unwrap_or_else(|| panic!("{signature} ends"));
    &rest[..end]
}

// ------------------------------------------------------------------------------ pure parts

#[test]
fn a_sha_is_forty_lowercase_hex_characters() {
    assert!(harden::is_sha(&"a".repeat(40)));
    assert!(harden::is_sha("0123456789abcdef0123456789abcdef01234567"));
    assert!(!harden::is_sha(&"A".repeat(40)), "upper case is not what git writes");
    assert!(!harden::is_sha("abc"));
    assert!(!harden::is_sha(&"g".repeat(40)));
    assert!(!harden::is_sha(&format!("{} ", "a".repeat(40))));
}

#[test]
fn the_push_paths_have_no_force_flag_and_no_wide_refspec_in_argv() {
    // Source scan of the three functions that can send a fix: no force, lease, mirror, delete or
    // a `+` refspec in what they pass to git. The scan reads only each function's own body.
    // The files may be checked out with CRLF line endings: compare with LF only.
    let pr_src = include_str!("pr.rs").replace("\r\n", "\n");
    let service_src = include_str!("service.rs").replace("\r\n", "\n");
    for (src, signature) in [
        (pr_src.as_str(), "pub fn push_update("),
        (pr_src.as_str(), "fn push_branch("),
        (service_src.as_str(), "pub fn pr_push_fix("),
    ] {
        let body = fn_body(src, signature);
        for banned in ["--force", "force-with-lease", "--mirror", "--delete", "\"-f\"", "\"+", "\"-f "] {
            assert!(!body.contains(banned), "{signature} must not contain {banned}");
        }
    }
}

// ----------------------------------------------------------------------------- fix pushes

#[test]
fn a_fix_commit_on_the_pm_branch_fast_forwards_the_remote() {
    let (_guard, root, _bare, bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-ff-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let failing = commit_file(&cwd, "failing.txt", "oops\n");
    let env = git_env(&empty_hooks().0);
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    assert_eq!(remote_sha(&bare_path, &worktree.branch), failing);
    let fix = commit_file(&cwd, "fix.txt", "fixed\n");
    let head = pr::push_update(&worktree, "main", Some("main"), &failing, &ToolEnv::default(), &env)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(head, fix, "push_update returns the new head");
    assert_eq!(remote_sha(&bare_path, &worktree.branch), fix, "the remote moved to the fix");
    assert!(harden::is_ancestor(&cwd, &failing, &fix, &env).unwrap(), "a fast-forward, never a rewrite");
}

#[test]
fn a_remote_that_moved_on_is_refused_and_left_alone() {
    let (_guard, root, _bare, bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-moved-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let failing = commit_file(&cwd, "failing.txt", "oops\n");
    let env = git_env(&empty_hooks().0);
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    let other = push_from_another_clone(&bare_path, &worktree.branch);
    commit_file(&cwd, "fix.txt", "fixed\n");
    let err = pr::push_update(&worktree, "main", Some("main"), &failing, &ToolEnv::default(), &env)
        .expect_err("the remote moved, so nothing may be pushed");
    assert_eq!(err.code, "CI_FIX_REMOTE_MOVED", "{err:?}");
    assert_eq!(remote_sha(&bare_path, &worktree.branch), other, "the other person's commit is untouched");
}

#[test]
fn the_default_branch_the_pr_base_and_a_non_pm_branch_are_refused_and_nothing_is_pushed() {
    let (_guard, root, _bare, bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-refuse-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let failing = commit_file(&cwd, "failing.txt", "oops\n");
    let env = git_env(&empty_hooks().0);
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    let before = remote_sha(&bare_path, &worktree.branch);
    commit_file(&cwd, "fix.txt", "fixed\n");
    let tool = ToolEnv::default();

    // The copy's own branch is the PR base.
    let as_base = pr::push_update(&worktree, &worktree.branch, None, &failing, &tool, &env).unwrap_err();
    assert_eq!(as_base.code, "PR_BRANCH_REFUSED", "{as_base:?}");
    // The branch is the repository's default branch.
    let as_default = pr::push_update(&worktree, "main", Some(&worktree.branch), &failing, &tool, &env).unwrap_err();
    assert_eq!(as_default.code, "PR_BRANCH_REFUSED", "{as_default:?}");
    // A copy whose branch is `main` (not a pm/ branch) is refused before any git call.
    let mut on_main = worktree.clone();
    on_main.branch = "main".into();
    let not_pm = pr::push_update(&on_main, "develop", Some("trunk"), &failing, &tool, &env).unwrap_err();
    assert_eq!(not_pm.code, "PR_BRANCH_REFUSED", "{not_pm:?}");
    // The failing commit must be a full commit id.
    let short = pr::push_update(&worktree, "main", None, "abc123", &tool, &env).unwrap_err();
    assert_eq!(short.code, "INVALID_ARGUMENT", "{short:?}");

    assert_eq!(remote_sha(&bare_path, &worktree.branch), before, "no refused push reached the remote");
}

#[test]
fn a_planted_pre_push_hook_does_not_run_under_the_hardened_push_but_does_without_it() {
    let (_guard, root, _bare, bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-hook-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let failing = commit_file(&cwd, "failing.txt", "oops\n");
    let env = git_env(&empty_hooks().0);
    // The failing commit is pushed first (PM's own push, before the agent could plant anything).
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    // The agent then plants a husky-style hook and points core.hooksPath at it, then commits a fix.
    let marker_dir = Dir(temp_dir("marker"));
    let marker = marker_in(&marker_dir.0, "pre-push.txt");
    write(&cwd, ".husky/pre-push", &marker_hook(&marker));
    git(&cwd, &["config", "core.hooksPath", ".husky"]);
    let fix = commit_file(&cwd, "fix.txt", "fixed\n");
    // The hardened push succeeds and does not run the hook.
    let head = pr::push_update(&worktree, "main", Some("main"), &failing, &ToolEnv::default(), &env)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(head, fix);
    assert_eq!(remote_sha(&bare_path, &worktree.branch), fix);
    assert!(!Path::new(&marker).exists(), "the hardened push must not run pre-push");
    // Control: a plain push of one more commit runs the same hook.
    commit_file(&cwd, "more.txt", "more\n");
    pr::push_branch(&worktree, &ToolEnv::default(), &[]).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(Path::new(&marker).exists(), "the control push must show the hook fires without the env");
}

#[test]
fn the_hardened_calls_refuse_an_empty_env() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-empty-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let sha = git(&cwd, &["rev-parse", "HEAD"]);
    assert!(harden::head_sha(&cwd, &[]).is_err());
    assert!(harden::remote_branch_sha(&cwd, "origin", &worktree.branch, &[]).is_err());
    assert!(harden::current_branch(&cwd, &[]).is_err());
    assert!(harden::is_ancestor(&cwd, &sha, &sha, &[]).is_err());
    assert!(harden::name_status(&cwd, &sha, &[]).is_err());
    let push = pr::push_update(&worktree, "main", None, &sha, &ToolEnv::default(), &[]).unwrap_err();
    assert_eq!(push.code, "INVALID_ARGUMENT", "{push:?}");
}

// --------------------------------------------------------------------------- harden reads

#[test]
fn remote_branch_sha_is_none_until_the_branch_is_pushed_and_matches_only_its_exact_name() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-remote-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let failing = commit_file(&cwd, "failing.txt", "oops\n");
    let env = git_env(&empty_hooks().0);
    assert_eq!(harden::remote_branch_sha(&cwd, "origin", &worktree.branch, &env).unwrap(), None);
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    assert_eq!(
        harden::remote_branch_sha(&cwd, "origin", &worktree.branch, &env).unwrap(),
        Some(failing)
    );
    assert_eq!(harden::remote_branch_sha(&cwd, "origin", "pm/other", &env).unwrap(), None);
    // The tail of the branch name is not a branch on the remote.
    let tail = worktree.branch.trim_start_matches("pm/").to_string();
    assert_eq!(harden::remote_branch_sha(&cwd, "origin", &tail, &env).unwrap(), None);
}

#[test]
fn head_sha_is_the_commit_head_points_to() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-head-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let env = git_env(&empty_hooks().0);
    let head = commit_file(&cwd, "one.txt", "1\n");
    let read = harden::head_sha(&cwd, &env).unwrap();
    assert_eq!(read, head);
    assert!(harden::is_sha(&read));
}

#[test]
fn current_branch_is_none_on_a_detached_head() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-branch-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let env = git_env(&empty_hooks().0);
    assert_eq!(harden::current_branch(&cwd, &env).unwrap(), Some(worktree.branch.clone()));
    git(&cwd, &["checkout", "-q", "--detach"]);
    assert_eq!(harden::current_branch(&cwd, &env).unwrap(), None);
}

#[test]
fn is_ancestor_is_true_and_false_after_a_history_rewrite() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-anc-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let env = git_env(&empty_hooks().0);
    let first = commit_file(&cwd, "one.txt", "1\n");
    let second = commit_file(&cwd, "two.txt", "2\n");
    assert!(harden::is_ancestor(&cwd, &first, &second, &env).unwrap());
    assert!(!harden::is_ancestor(&cwd, &second, &first, &env).unwrap());
    // A soft reset moves HEAD back and keeps the work: the later commit is no longer in history.
    git(&cwd, &["reset", "-q", "--soft", &first]);
    let head = harden::head_sha(&cwd, &env).unwrap();
    assert_eq!(head, first);
    assert!(harden::is_ancestor(&cwd, &first, &head, &env).unwrap(), "a commit is its own ancestor");
    assert!(!harden::is_ancestor(&cwd, &second, &head, &env).unwrap(), "the rewritten commit is gone");
    // A commit git does not know is an error, not `false`.
    assert!(harden::is_ancestor(&cwd, &"f".repeat(40), &head, &env).is_err());
}

#[test]
fn name_status_shows_d_for_a_deleted_file_and_a_and_m_for_the_others() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-names-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let env = git_env(&empty_hooks().0);
    let base = worktree.base_commit.clone();
    git(&cwd, &["rm", "-q", "a.txt"]);
    write(&cwd, "src/lib.rs", "pub fn two() {}\n");
    write(&cwd, "new.txt", "new\n");
    git(&cwd, &["add", "-A"]);
    git(&cwd, &["commit", "-q", "--no-verify", "-m", "change"]);
    let listed = harden::name_status(&cwd, &base, &env).unwrap();
    assert!(listed.contains(&("D".to_string(), "a.txt".to_string())), "{listed:?}");
    assert!(listed.contains(&("M".to_string(), "src/lib.rs".to_string())), "{listed:?}");
    assert!(listed.contains(&("A".to_string(), "new.txt".to_string())), "{listed:?}");
    assert_eq!(listed.len(), 3, "an unchanged file is not listed: {listed:?}");
}

#[test]
fn commit_count_counts_the_commits_after_the_failing_commit_and_refuses_a_name() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "cf-count-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let env = git_env(&empty_hooks().0);
    let failing = harden::head_sha(&cwd, &env).unwrap();
    assert_eq!(harden::commit_count(&cwd, &failing, &env).unwrap(), 0);
    commit_file(&cwd, "one.txt", "1\n");
    commit_file(&cwd, "two.txt", "2\n");
    assert_eq!(harden::commit_count(&cwd, &failing, &env).unwrap(), 2);
    assert_eq!(harden::commit_count(&cwd, "main", &env).unwrap_err().code, "INVALID_ARGUMENT");
    assert!(harden::commit_count(&cwd, &failing, &[]).is_err(), "the hardened env is required");
}
