//! Tests for the start commit of a branch copy (`commit_start`, spec n33 §5 BF-3). Real temp git
//! repositories and the real `git` CLI; the copies are made by [`create_with`] like a branch's are.

use super::harden::git_env;
use super::*;

const MESSAGE: &str = "Puppet Master: start from save point (after turn 5, 14:02)";

// ------------------------------------------------------------------------------------ helpers

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmstart-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
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

fn git_run(dir: &Path, args: &[&str]) -> std::process::Output {
    crate::proc_util::hidden_command("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_run(dir, args);
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A repo on `main` with `a.txt` and `src/lib.rs` committed. Identity and signing are pinned.
fn repo() -> (Dir, PathBuf) {
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
    (Dir(root.clone()), root)
}

/// A copy of `root` on a new branch, the way a branch from here gets one.
fn copy_of(root: &Path, handle: &str) -> WorktreeRef {
    create_with(&root.to_string_lossy(), handle, &CreateOptions::default()).unwrap()
}

// ------------------------------------------------------------------------------------- tests

#[test]
fn the_start_commit_runs_no_hooks_so_a_failing_pre_commit_hook_does_not_block_it() {
    let (_guard, root) = repo();
    let copy = copy_of(&root, "start-hooks-1");
    let cwd = PathBuf::from(&copy.path);
    // A pre-commit hook that always fails. The copy shares the origin's hooks folder.
    write(&root.join(".git").join("hooks"), "pre-commit", "#!/bin/sh\nexit 1\n");
    write(&cwd, "agent.txt", "work\n");
    let empty = Dir(temp_dir("empty-hooks"));
    let made = commit_start(&copy, MESSAGE, &git_env(&empty.0)).unwrap();
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    assert_eq!(made.as_deref(), Some(head.as_str()), "the new HEAD is returned");
    assert_eq!(git(&cwd, &["log", "-1", "--format=%s"]), MESSAGE);
    // Control: the same hook does block a plain commit, so the hardened one really skipped it.
    write(&cwd, "more.txt", "more\n");
    git(&cwd, &["add", "-A"]);
    assert!(!git_run(&cwd, &["commit", "-q", "-m", "plain"]).status.success(), "the control commit must be blocked");
}

#[test]
fn the_start_commit_never_takes_in_puppet_masters_own_data() {
    let (_guard, root) = repo();
    let copy = copy_of(&root, "start-data-1");
    let cwd = PathBuf::from(&copy.path);
    write(&cwd, ".puppet-master/checkpoints/secret.txt", "private\n");
    write(&cwd, "agent.txt", "work\n");
    let empty = Dir(temp_dir("empty-hooks"));
    assert!(commit_start(&copy, MESSAGE, &git_env(&empty.0)).unwrap().is_some());
    let files = git(&cwd, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.lines().any(|line| line == "agent.txt"), "{files}");
    assert!(!files.contains(".puppet-master"), "{files}");
    assert!(cwd.join(".puppet-master/checkpoints/secret.txt").is_file(), "the data stays on disk");
}

#[test]
fn nothing_staged_gives_none_and_leaves_head_alone() {
    let (_guard, root) = repo();
    let copy = copy_of(&root, "start-none-1");
    let cwd = PathBuf::from(&copy.path);
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    assert_eq!(commit_start(&copy, MESSAGE, &[]).unwrap(), None);
    // Only Puppet Master's own data changed: still nothing to commit.
    write(&cwd, ".puppet-master/notes.txt", "x\n");
    assert_eq!(commit_start(&copy, MESSAGE, &[]).unwrap(), None);
    assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
}

#[test]
fn without_a_user_email_the_start_commit_still_lands_with_a_fallback_identity() {
    let (_guard, root) = repo();
    git(&root, &["config", "--unset", "user.name"]);
    git(&root, &["config", "--unset", "user.email"]);
    let copy = copy_of(&root, "start-identity-1");
    let cwd = PathBuf::from(&copy.path);
    write(&cwd, "agent.txt", "work\n");
    let empty = Dir(temp_dir("empty-hooks"));
    let made = commit_start(&copy, MESSAGE, &git_env(&empty.0)).unwrap();
    assert!(made.is_some(), "the commit lands even though the repo has no identity");
    let author_email = git(&cwd, &["log", "-1", "--format=%ae"]);
    // The fallback applies only when no identity is set globally on this computer.
    let global = git_run(&cwd, &["config", "--global", "--get", "user.email"]);
    let global_email = String::from_utf8_lossy(&global.stdout).trim().to_string();
    if global_email.is_empty() {
        assert_eq!(author_email, "puppet-master@localhost");
        assert_eq!(git(&cwd, &["log", "-1", "--format=%an"]), "Puppet Master");
    } else {
        assert_eq!(author_email, global_email, "the global identity is kept");
    }
}

#[test]
fn a_start_message_that_is_not_one_plain_line_is_refused() {
    let (_guard, root) = repo();
    let copy = copy_of(&root, "start-message-1");
    let cwd = PathBuf::from(&copy.path);
    write(&cwd, "agent.txt", "work\n");
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    let too_long = "x".repeat(121);
    for bad in ["", "two\nlines", "tab\there", "bell\u{7}here", "carriage\rreturn", too_long.as_str()] {
        let error = commit_start(&copy, bad, &[]).unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT", "{bad:?}");
    }
    assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head, "a refused message commits nothing");
    // 120 characters is the most that is allowed.
    let longest = "y".repeat(120);
    assert!(commit_start(&copy, &longest, &[]).unwrap().is_some());
}
