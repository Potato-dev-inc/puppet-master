//! Tests for [`read`] (spec n33 CA-2, threats C1 and C2). Real temp repositories and the real git
//! CLI. The hostile settings are planted the way an agent could plant them, and each attack gets a
//! control run with plain git, so a pass means the guard works and not that the attack never fired.

use super::*;
use crate::proc_util::hidden_command;
use crate::worktrees::git::run_capped;
use std::time::{Duration, Instant};

// ------------------------------------------------------------------------------------ helpers

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
    let dir = std::env::temp_dir().join(format!("pmread-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
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

fn strs(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// A repo on `main` with `a.txt` and `b.txt` committed.
fn repo(tag: &str) -> Dir {
    let dir = temp(tag);
    git(&dir.0, &["init", "-q", "-b", "main"]);
    git(&dir.0, &["config", "user.name", "Test"]);
    git(&dir.0, &["config", "user.email", "test@example.com"]);
    git(&dir.0, &["config", "commit.gpgsign", "false"]);
    git(&dir.0, &["config", "core.autocrlf", "false"]);
    write(&dir.0, "a.txt", "alpha\n");
    write(&dir.0, "b.txt", "bravo\n");
    git(&dir.0, &["add", "-A"]);
    git(&dir.0, &["commit", "-q", "-m", "initial"]);
    dir
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

// ------------------------------------------------------------------------------- pure parts

#[test]
fn the_argv_adds_only_the_flags_each_read_needs() {
    assert_eq!(read_argv(&["rev-parse", "HEAD"]).unwrap(), strs(&["rev-parse", "HEAD"]));
    assert_eq!(read_argv(&["ls-files", "-z"]).unwrap(), strs(&["ls-files", "-z"]));
    assert_eq!(
        read_argv(&["diff-index", "-z", "HEAD"]).unwrap(),
        strs(&["diff-index", "--no-textconv", "-z", "HEAD"])
    );
    assert_eq!(
        read_argv(&["diff", "-U0", "HEAD", "--", "a.txt"]).unwrap(),
        strs(&["diff", "--no-textconv", "--no-ext-diff", "--no-color", "--no-renames", "-U0", "HEAD", "--", "a.txt"])
    );
    assert_eq!(read_argv(&[]).unwrap_err().code, "INVALID_ARGUMENT");
}

// ------------------------------------------------------------------------------ real repos

#[test]
fn the_plumbing_reads_and_a_diff_work_on_a_copy_with_changes() {
    let root = repo("reads");
    let empty = temp("empty-hooks");
    let env = git_env(&empty.0);
    write(&root.0, "a.txt", "alpha\nbeta\n");
    write(&root.0, "new.txt", "new\n");

    let head = read(&root.0, &["rev-parse", "HEAD"], &env, 1024, 20_000).unwrap();
    assert_eq!(head.text.trim(), git(&root.0, &["rev-parse", "HEAD"]));
    assert!(!head.truncated);
    let tracked = read(&root.0, &["ls-files"], &env, 4096, 20_000).unwrap();
    assert_eq!(tracked.text, "a.txt\nb.txt\n");
    let untracked = read(&root.0, &["ls-files", "-z", "--others", "--exclude-standard"], &env, 4096, 20_000).unwrap();
    assert_eq!(untracked.text, "new.txt\0");
    let changed = read(&root.0, &["diff-index", "--name-status", "-z", "HEAD"], &env, 4096, 20_000).unwrap();
    assert_eq!(changed.text, "M\0a.txt\0");
    let patch = read(&root.0, &["diff", "-U0", "HEAD", "--", "a.txt"], &env, 65_536, 20_000).unwrap();
    assert!(patch.text.contains("@@ -1,0 +2 @@"), "{}", patch.text);
    assert!(patch.text.contains("+beta"), "{}", patch.text);

    // A git that fails is an error, not empty text.
    let missing = read(&root.0, &["rev-parse", "--verify", "no-such-ref"], &env, 1024, 20_000).unwrap_err();
    assert_eq!(missing.code, "GIT_FAILED");
}

#[test]
fn a_hostile_config_runs_nothing_in_a_read_while_plain_git_runs_every_planted_attack() {
    let markers = temp("markers");
    let empty = temp("empty-hooks");
    let hardened = repo("hardened");
    let control = repo("control");
    let hooks_h = temp("hooks-h");
    let hooks_c = temp("hooks-c");
    plant(&hardened.0, "hardened", &markers.0, &hooks_h.0);
    plant(&control.0, "control", &markers.0, &hooks_c.0);
    let marker = |tag: &str, name: &str| markers.0.join(format!("{tag}-{name}.txt"));
    let attacks = ["fsm", "tc", "clean", "hook"];

    // PM's reads on the hostile copy succeed and run none of the planted commands.
    let env = git_env(&empty.0);
    let changed = read(&hardened.0, &["diff-index", "--name-status", "-z", "HEAD"], &env, 4096, 20_000).unwrap();
    assert_eq!(changed.text, "M\0a.txt\0");
    let patch = read(&hardened.0, &["diff", "-U0", "HEAD", "--", "a.txt"], &env, 65_536, 20_000).unwrap();
    assert!(patch.text.contains("+changed"), "{}", patch.text);
    read(&hardened.0, &["ls-files", "-z", "--others", "--exclude-standard"], &env, 4096, 20_000).unwrap();
    read(&hardened.0, &["rev-parse", "HEAD"], &env, 1024, 20_000).unwrap();
    for name in attacks {
        assert!(!marker("hardened", name).exists(), "PM's read ran the planted {name}");
    }

    // Control: plain git on the same planted copy fires each attack, so the empty markers above mean something.
    git(&control.0, &["diff", "-U0", "HEAD", "--", "a.txt"]);
    git(&control.0, &["add", "-A"]);
    git(&control.0, &["commit", "-q", "-m", "control"]);
    for name in attacks {
        assert!(marker("control", name).exists(), "the control run did not fire the planted {name}");
    }
}

#[test]
fn read_refuses_every_command_and_option_it_does_not_run() {
    let root = repo("refuse");
    let empty = temp("empty-hooks");
    let env = git_env(&empty.0);
    let refused: &[&[&str]] = &[
        &["status"],
        &["add", "a.txt"],
        &["config", "core.fsmonitor", "x"],
        &["checkout", "-q", "main"],
        &["commit", "-m", "x"],
        &["reset", "--hard"],
        &["diff", "--no-index", "a.txt", "b.txt"],
        &["diff", "--output=out.txt"],
        &["diff-index", "--output", "out.txt", "HEAD"],
        &["ls-files", "--output=out.txt"],
        &["diff", "--textconv"],
        &["diff", "--ext-diff"],
        &["diff", "-Oorder.txt"],
        &["diff", "--open-files-in-pager=more"],
        &[""],
        &[],
    ];
    for args in refused {
        let err = read(&root.0, args, &env, 1024, 5_000).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGUMENT", "{args:?}");
    }
    assert!(!root.0.join("out.txt").exists(), "a refused --output still wrote a file");
}

#[test]
fn an_empty_env_and_a_nul_byte_are_refused() {
    let root = repo("env");
    let no_env = read(&root.0, &["rev-parse", "HEAD"], &[], 1024, 5_000).unwrap_err();
    assert_eq!(no_env.code, "INVALID_ARGUMENT");
    let empty = temp("empty-hooks");
    let env = git_env(&empty.0);
    let nul = read(&root.0, &["ls-files", "a\0b"], &env, 1024, 5_000).unwrap_err();
    assert_eq!(nul.code, "INVALID_ARGUMENT");
}

#[test]
fn an_index_lock_never_fails_a_read_and_the_index_bytes_do_not_change() {
    let root = repo("lock");
    let empty = temp("empty-hooks");
    let env = git_env(&empty.0);
    // A changed file makes the index stale: a read that refreshed it would want to write it.
    write(&root.0, "a.txt", "alpha\nchanged\n");
    write(&root.0, "new.txt", "new\n");
    let git_dir = root.0.join(".git");
    let index = git_dir.join("index");
    let lock = git_dir.join("index.lock");
    std::fs::write(&lock, "held\n").unwrap();
    let before = std::fs::read(&index).unwrap();

    let reads: [&[&str]; 4] = [
        &["rev-parse", "HEAD"],
        &["ls-files", "-z", "--others", "--exclude-standard"],
        &["diff-index", "--name-status", "-z", "HEAD"],
        &["diff", "-U0", "HEAD", "--", "a.txt"],
    ];
    for args in reads {
        if let Err(error) = read(&root.0, args, &env, 65_536, 20_000) {
            panic!("a read with index.lock present failed: {args:?}: {}", error.message);
        }
    }
    assert_eq!(std::fs::read(&index).unwrap(), before, "a read rewrote .git/index");
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), "held\n", "a read touched index.lock");
}

#[test]
fn the_output_cap_truncates_and_says_so() {
    let root = repo("cap");
    let empty = temp("empty-hooks");
    let env = git_env(&empty.0);
    let capped = read(&root.0, &["ls-files"], &env, 8, 20_000).unwrap();
    assert!(capped.truncated);
    assert!(capped.text.len() <= 8, "{:?}", capped.text);
    let full = read(&root.0, &["ls-files"], &env, 65_536, 20_000).unwrap();
    assert!(!full.truncated);
    assert_eq!(full.text, "a.txt\nb.txt\n");
}

#[test]
fn a_helper_that_hangs_is_killed_at_the_deadline() {
    let mut cmd = hidden_command(if cfg!(windows) { "cmd" } else { "sh" });
    cmd.args(if cfg!(windows) { vec!["/C", "ping -n 6 127.0.0.1 >NUL"] } else { vec!["-c", "sleep 5"] });
    let started = Instant::now();
    let (text, truncated, timed_out) = run_capped(cmd, 1024, 300).unwrap();
    assert!(timed_out, "a helper past its deadline must come back timed out");
    assert!(text.is_empty() && !truncated);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the wait must end at the deadline, not when the helper ends"
    );
}
