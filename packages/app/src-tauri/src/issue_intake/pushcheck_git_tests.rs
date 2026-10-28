//! Real-git tests for the push check (spec n32 sections 3.5 and 3.6 C, threats S6 and S7). A diff
//! driver planted in the repo must not run during the push diff, the file list and the patch come
//! back exact, the cap cuts and says so, and the config fingerprint notices a change. Each attack has
//! a control run, so a pass means the guard works and not that the attack never fired. Temp git
//! repositories only.

use super::pushcheck::{changed_files, diff_totals, fingerprint_of, push_diff, scan_added, DIFF_MAX_BYTES};
use crate::worktrees::harden::{config_snapshot, git_env};
use std::path::{Path, PathBuf};
use std::process::Command;

// ------------------------------------------------------------------------------------ helpers

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmpush-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// Removes a temp folder on drop. Git marks pack files read-only, so those bits are cleared first.
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

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Commits everything and returns the new commit.
fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

/// A repo on `main` with `a.txt` committed. Returns the guard, the folder and the base commit.
fn repo() -> (Dir, PathBuf, String) {
    let root = temp_dir("repo");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    write(&root, "a.txt", "alpha\n");
    let base = commit_all(&root, "initial");
    (Dir(root.clone()), root, base)
}

/// The hardened env the issue flow uses, with its empty hooks folder (kept alive by the guard).
fn hardened() -> (Dir, Vec<(String, String)>) {
    let hooks = Dir(temp_dir("hooks"));
    let env = git_env(&hooks.0);
    (hooks, env)
}

/// Plants a diff driver in the repo's own config. The script lives outside the repo and leaves
/// `marker` behind when git runs it.
fn plant_driver(root: &Path, key: &str, marker: &Path, scripts: &Path) {
    let script = scripts.join("driver.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho ran > \"{}\"\nexit 0\n", slash(marker))).unwrap();
    git(root, &["config", key, &format!("sh {}", slash(&script))]);
}

// ------------------------------------------------------------------------------------- tests

#[test]
fn a_planted_textconv_driver_does_not_run_in_the_push_diff() {
    let (_guard, root, base) = repo();
    let scripts = Dir(temp_dir("textconv"));
    let marker = scripts.0.join("ran.txt");
    plant_driver(&root, "diff.evil.textconv", &marker, &scripts.0);
    write(&root, ".gitattributes", "a.txt diff=evil\n");
    write(&root, "a.txt", "beta\n");
    commit_all(&root, "work");
    let (_hooks, env) = hardened();

    let patch = push_diff(&root, &base, &env).unwrap();
    assert!(!marker.exists(), "the hardened push diff must not run the textconv driver");
    assert_eq!(diff_totals(&patch.text), (2, 1), "the patch itself still comes back");

    // Control: a plain diff with the same config does run the driver, so the attack is real.
    git(&root, &["diff", &format!("{base}..HEAD")]);
    assert!(marker.exists(), "control: a plain git diff runs the planted driver");
}

#[test]
fn a_planted_external_diff_program_does_not_run_in_the_push_diff() {
    let (_guard, root, base) = repo();
    let scripts = Dir(temp_dir("external"));
    let marker = scripts.0.join("ran.txt");
    plant_driver(&root, "diff.external", &marker, &scripts.0);
    write(&root, "a.txt", "beta\n");
    commit_all(&root, "work");
    let (_hooks, env) = hardened();

    push_diff(&root, &base, &env).unwrap();
    assert!(!marker.exists(), "the hardened push diff must not run the external diff program");

    git(&root, &["diff", &format!("{base}..HEAD")]);
    assert!(marker.exists(), "control: a plain git diff runs the planted external program");
}

#[test]
fn the_file_list_and_the_patch_come_back_exact_and_the_scan_finds_the_key() {
    let (_guard, root, base) = repo();
    write(&root, "with space.txt", "one\n");
    write(&root, "a.txt", "beta\n");
    write(&root, ".env", "TOKEN=abc\n");
    commit_all(&root, "work");
    let (_hooks, env) = hardened();

    let mut files = changed_files(&root, &base, &env).unwrap();
    files.sort();
    assert_eq!(files, vec![".env", "a.txt", "with space.txt"]);

    let patch = push_diff(&root, &base, &env).unwrap();
    assert!(!patch.over_cap);
    assert_eq!(diff_totals(&patch.text), (3, 1));
    let (secrets, homes) = scan_added(&patch.text, &|line| line.contains("TOKEN="), None);
    assert_eq!(secrets, vec![(".env".to_string(), 1)]);
    assert!(homes.is_empty());
}

#[test]
fn a_patch_over_the_cap_is_cut_and_marked() {
    let (_guard, root, base) = repo();
    write(&root, "big.txt", &"line\n".repeat(60_000));
    commit_all(&root, "big");
    let (_hooks, env) = hardened();

    let patch = push_diff(&root, &base, &env).unwrap();
    assert!(patch.over_cap, "a patch over the cap must say so");
    assert_eq!(patch.text.len(), DIFF_MAX_BYTES, "the text is cut at the cap (ASCII here)");
}

#[test]
fn a_base_that_is_not_a_commit_id_and_an_unhardened_env_are_refused() {
    let (_guard, root, base) = repo();
    let (_hooks, env) = hardened();

    let err = push_diff(&root, "main; rm -rf /", &env).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGUMENT");
    let err = changed_files(&root, "HEAD", &env).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGUMENT");
    let err = push_diff(&root, &base, &[]).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGUMENT", "an empty env is never used for a diff");
}

#[test]
fn the_config_fingerprint_notices_a_changed_remote_or_hook_and_not_a_quiet_run() {
    let (_guard, root, _base) = repo();
    let start = fingerprint_of(&config_snapshot(&root).unwrap());
    let again = fingerprint_of(&config_snapshot(&root).unwrap());
    assert_eq!(again, start, "nothing changed, so the digest is the same");

    git(&root, &["config", "remote.origin.url", "https://github.com/evil/repo.git"]);
    let after_url = fingerprint_of(&config_snapshot(&root).unwrap());
    assert_ne!(after_url, start, "a changed remote url is a change");

    write(&root, ".git/hooks/pre-push", "#!/bin/sh\nexit 0\n");
    let after_hook = fingerprint_of(&config_snapshot(&root).unwrap());
    assert_ne!(after_hook, after_url, "a new hook file is a change");
}
