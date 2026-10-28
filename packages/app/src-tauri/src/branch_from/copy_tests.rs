//! Tests for `copy::make` and `copy::start_point_for` (SPEC n33 section 5, BF-7). Real temp git
//! repositories and the real `git` CLI; save points come from the real checkpoint engine. Nothing
//! outside a temp folder is touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::{make, start_point_for, Made};
use crate::checkpoints::{test_snapshot, TestKind, TestLimits};
use crate::worktrees::{self, CreateOptions};

const PANE: &str = "pane-1";
const MESSAGE: &str = "Puppet Master: start from save point (after turn 1, 11:02)";
/// Numbering of a pane's save points: the first one is "1", the next "2" (engine `n = max + 1`).
const FIRST: &str = "1";
const SECOND: &str = "2";

// ------------------------------------------------------------------------------------ helpers

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmcopy-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
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

/// Removes its folder when the test ends. Git's read-only objects are made writable first.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        clear_readonly(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git_run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
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

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

fn git_available() -> bool {
    if !crate::checkpoints::git_available() {
        eprintln!("git is not installed: copy tests skipped");
    }
    crate::checkpoints::git_available()
}

/// A repo on `main` with `a.txt` ("alpha") and `src/lib.rs` committed. Identity and signing are pinned.
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

/// Take a save point of `root` for [`PANE`] (the way the chat's turn ends do).
fn save_point(root: &Path, at: u64) {
    test_snapshot(root, PANE, TestKind::After, None, at, &TestLimits::default()).expect("save point");
}

/// `make` with the test's usual inputs: the save point is read from `root`, the copy starts from HEAD.
fn make_from(root: &Path, handle: &str, id: &str) -> Result<Made, crate::operations::OperationError> {
    make(&root.to_string_lossy(), None, handle, root, PANE, id, MESSAGE, &[])
}

/// Sha-256 of every file under `root`, by relative path. `.git` and `.puppet-master` (git's and PM's
/// own data) are left out: the test is about the files a person sees.
fn file_hashes(root: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == ".git" || name == ".puppet-master" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, format!("{:x}", Sha256::digest(bytes)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

// ------------------------------------------------------------------------------------- tests

#[test]
fn the_copy_has_the_save_point_files_and_one_start_commit() {
    if !git_available() {
        return;
    }
    let (_guard, root) = repo();
    let base = git(&root, &["rev-parse", "HEAD"]);
    // Save point 1: a.txt says "one". Save point 2: a.txt says "two" and b.txt is new.
    write(&root, "a.txt", "one\n");
    save_point(&root, 1_000);
    write(&root, "a.txt", "two\n");
    write(&root, "b.txt", "later\n");
    save_point(&root, 2_000);

    let made = make_from(&root, "bf-000000000001", FIRST).expect("the copy is made");
    let cwd = PathBuf::from(&made.worktree.cwd);
    // The copy holds save point 1, not the newer work and not the original's current files.
    assert_eq!(read(&cwd, "a.txt").as_deref(), Some("one\n"));
    assert!(!cwd.join("b.txt").exists(), "a file from a later save point is not in the copy");
    assert_eq!(made.export.removed, 0);
    assert!(made.export.failed.is_empty(), "{:?}", made.export.failed);
    // Exactly one commit on top of the copy's starting commit: PM's start commit.
    assert_eq!(made.worktree.base_commit, base);
    assert_eq!(git(&cwd, &["rev-list", "--count", &format!("{base}..HEAD")]), "1");
    assert_eq!(git(&cwd, &["log", "-1", "--format=%s"]), MESSAGE);
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    assert_eq!(made.start_commit.as_deref(), Some(head.as_str()));
    let files = git(&cwd, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.lines().any(|line| line == "a.txt"), "{files}");
    assert!(!files.contains(".puppet-master"), "{files}");
}

#[test]
fn the_original_folder_has_the_same_files_before_and_after_a_copy_is_made() {
    if !git_available() {
        return;
    }
    let (_guard, root) = repo();
    // Uncommitted work in the original, saved as a save point, plus more that is not saved.
    write(&root, "a.txt", "edited\n");
    save_point(&root, 1_000);
    write(&root, "notes.txt", "not saved\n");
    let files_before = file_hashes(&root);
    let status_before = git(&root, &["status", "--porcelain=v1", "--untracked-files=all"]);
    let head_before = git(&root, &["rev-parse", "HEAD"]);

    let made = make_from(&root, "bf-000000000002", FIRST).expect("the copy is made");
    assert!(Path::new(&made.worktree.path).is_dir());

    assert_eq!(file_hashes(&root), files_before, "every file of the original folder is unchanged");
    assert_eq!(git(&root, &["status", "--porcelain=v1", "--untracked-files=all"]), status_before);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(read(&root, "notes.txt").as_deref(), Some("not saved\n"));
}

#[test]
fn a_copy_of_a_copy_starts_from_that_copys_branch() {
    if !git_available() {
        return;
    }
    let (_guard, root) = repo();
    let origin = root.to_string_lossy().into_owned();
    // The source pane works in copy 1, which has one commit of its own.
    let source = worktrees::create_with(&origin, "bf-000000000003", &CreateOptions::default()).expect("copy 1");
    let source_cwd = PathBuf::from(&source.cwd);
    write(&source_cwd, "from-copy.txt", "made in copy 1\n");
    git(&source_cwd, &["add", "-A"]);
    git(&source_cwd, &["commit", "-q", "-m", "work in copy 1"]);
    let source_head = git(&source_cwd, &["rev-parse", "HEAD"]);

    // The start point of a branch made from there is copy 1's branch. The original folder has none.
    assert_eq!(start_point_for(&source.cwd), Some(source.branch.clone()));
    assert_eq!(start_point_for(&root.to_string_lossy()), None);
    let elsewhere = Dir(temp_dir("elsewhere"));
    assert_eq!(start_point_for(&elsewhere.0.to_string_lossy()), None);

    // A branch made from copy 1 starts from copy 1's commit, not from the original's HEAD.
    save_point(&root, 1_000);
    let made = make(&origin, Some(source.branch.as_str()), "bf-000000000004", &root, PANE, FIRST, MESSAGE, &[])
        .expect("the branch copy is made");
    assert_eq!(made.worktree.base_commit, source_head, "the new copy starts from copy 1's commit");
    assert!(made.worktree.branch.starts_with("pm/"));
    assert_ne!(made.worktree.branch, source.branch);
}

#[test]
fn an_export_failure_discards_the_copy_and_its_branch_and_leaves_the_original_alone() {
    if !git_available() {
        return;
    }
    let (_guard, root) = repo();
    save_point(&root, 1_000);
    let files_before = file_hashes(&root);
    let status_before = git(&root, &["status", "--porcelain=v1", "--untracked-files=all"]);

    // Save point 99 does not exist: the export fails after the copy was made.
    let error = make_from(&root, "bf-000000000005", "99").expect_err("the copy is not kept");
    assert_eq!(error.code, "BRANCH_COPY_FAILED");
    assert!(error.message.contains("Your folder wasn't changed."), "{}", error.message);

    // The half-made copy is gone: no description, no checkout, no branch of its own.
    let origin = root.to_string_lossy().into_owned();
    assert!(worktrees::list(&origin).is_empty(), "no copy is left behind");
    assert!(worktrees::find_by_handle(&origin, "bf-000000000005").is_none());
    assert_eq!(git(&root, &["branch", "--list", "pm/*"]), "", "the copy's branch is gone");
    let listed = git(&root, &["worktree", "list", "--porcelain"]);
    assert_eq!(listed.lines().filter(|l| l.starts_with("worktree ")).count(), 1, "{listed}");
    let copies = root.join(".puppet-master").join("worktrees");
    let left = std::fs::read_dir(&copies).map(|rd| rd.count()).unwrap_or(0);
    assert_eq!(left, 0, "nothing is left in the worktrees folder");

    // The original is untouched.
    assert_eq!(file_hashes(&root), files_before);
    assert_eq!(git(&root, &["status", "--porcelain=v1", "--untracked-files=all"]), status_before);
}

#[test]
fn uncommitted_changes_in_the_original_stay_out_of_the_copy_unless_the_save_point_has_them() {
    if !git_available() {
        return;
    }
    let (_guard, root) = repo();
    // Save point 1 is the clean state. Save point 2 has uncommitted edits and a new file.
    save_point(&root, 1_000);
    write(&root, "a.txt", "edited\n");
    write(&root, "dirty.txt", "not yet committed\n");
    save_point(&root, 2_000);
    // More uncommitted work after the last save point: it is in no copy.
    write(&root, "after.txt", "after the last save point\n");

    let clean_copy = make_from(&root, "bf-000000000006", FIRST).expect("copy from save point 1");
    let clean = PathBuf::from(&clean_copy.worktree.cwd);
    assert_eq!(read(&clean, "a.txt").as_deref(), Some("alpha\n"));
    assert!(!clean.join("dirty.txt").exists(), "work after save point 1 is not in the copy");
    assert!(!clean.join("after.txt").exists(), "work after the last save point is not in the copy");

    let with_edits = make_from(&root, "bf-000000000007", SECOND).expect("copy from save point 2");
    let edited = PathBuf::from(&with_edits.worktree.cwd);
    assert_eq!(read(&edited, "a.txt").as_deref(), Some("edited\n"));
    assert_eq!(read(&edited, "dirty.txt").as_deref(), Some("not yet committed\n"));
    assert!(!edited.join("after.txt").exists(), "work after the last save point is not in the copy");

    // The original keeps all of its own uncommitted work.
    assert_eq!(read(&root, "dirty.txt").as_deref(), Some("not yet committed\n"));
    assert_eq!(read(&root, "after.txt").as_deref(), Some("after the last save point\n"));
}

#[test]
fn a_branch_of_a_branch_reads_the_source_copys_own_save_points_and_writes_a_new_copy_under_the_origin() {
    if !git_available() {
        return;
    }
    let (_guard, root) = repo();
    let origin = root.to_string_lossy().into_owned();
    // The source pane works in copy 1. Its save points live in that copy's own shadow repository.
    let source = worktrees::create_with(&origin, "bf-0000000000a1", &CreateOptions::default()).expect("copy 1");
    let source_cwd = PathBuf::from(&source.cwd);
    write(&source_cwd, "from-copy.txt", "made in copy 1\n");
    git(&source_cwd, &["add", "-A"]);
    git(&source_cwd, &["commit", "-q", "-m", "work in copy 1"]);
    let source_head = git(&source_cwd, &["rev-parse", "HEAD"]);
    write(&source_cwd, "a.txt", "edited in copy 1\n");
    save_point(&source_cwd, 1_000);
    write(&source_cwd, "a.txt", "edited again\n");
    write(&source_cwd, "later.txt", "later\n");
    save_point(&source_cwd, 2_000);
    let source_files = file_hashes(&source_cwd);
    let source_status = git(&source_cwd, &["status", "--porcelain=v1", "--untracked-files=all"]);
    let origin_files = file_hashes(&root);

    let start = start_point_for(&source.cwd);
    assert_eq!(start.as_deref(), Some(source.branch.as_str()));
    let made = make(&origin, start.as_deref(), "bf-0000000000a2", &source_cwd, PANE, FIRST, MESSAGE, &[])
        .expect("a branch of a branch is made");
    let cwd = PathBuf::from(&made.worktree.cwd);
    // A new copy under the ORIGIN's worktrees folder, next to copy 1, not inside it.
    let worktrees_dir = root.join(".puppet-master").join("worktrees");
    assert_eq!(cwd.parent().map(Path::to_path_buf), Some(worktrees_dir.clone()), "{}", cwd.display());
    assert_ne!(cwd, source_cwd);
    // It holds save point 1 of copy 1 on top of copy 1's own commit.
    assert_eq!(read(&cwd, "a.txt").as_deref(), Some("edited in copy 1\n"));
    assert_eq!(read(&cwd, "from-copy.txt").as_deref(), Some("made in copy 1\n"));
    assert!(!cwd.join("later.txt").exists(), "a file from a later save point is not in the copy");
    assert_eq!(made.worktree.base_commit, source_head);
    assert!(made.export.failed.is_empty(), "{:?}", made.export.failed);
    assert_eq!(git(&cwd, &["log", "-1", "--format=%s"]), MESSAGE);
    // Neither the source copy nor the original folder changed.
    assert_eq!(file_hashes(&source_cwd), source_files, "the source copy is untouched");
    assert_eq!(git(&source_cwd, &["status", "--porcelain=v1", "--untracked-files=all"]), source_status);
    assert_eq!(read(&source_cwd, "a.txt").as_deref(), Some("edited again\n"));
    assert_eq!(file_hashes(&root), origin_files, "the original folder is untouched");
    assert_eq!(read(&root, "a.txt").as_deref(), Some("alpha\n"));
}

#[test]
fn a_project_inside_a_repo_gets_its_prefixed_folder_written_and_nothing_outside_it() {
    if !git_available() {
        return;
    }
    // The project is `<repo>/src`: the copy is a whole checkout and the agent runs in its `src` folder.
    let (_guard, root) = repo();
    let project = root.join("src");
    let origin = project.to_string_lossy().into_owned();
    write(&project, "lib.rs", "pub fn saved() {}\n");
    save_point(&project, 1_000);
    write(&project, "lib.rs", "pub fn newer() {}\n");
    let files_before = file_hashes(&root);

    let made = make(&origin, None, "bf-0000000000b1", &project, PANE, FIRST, MESSAGE, &[]).expect("the copy is made");
    let copy_root = PathBuf::from(&made.worktree.path);
    let cwd = PathBuf::from(&made.worktree.cwd);
    assert_ne!(cwd, copy_root, "the copy's cwd is below the checkout");
    assert!(cwd.starts_with(&copy_root));
    assert_eq!(read(&cwd, "lib.rs").as_deref(), Some("pub fn saved() {}\n"));
    // Files outside the project's folder are the checkout's own.
    assert_eq!(read(&copy_root, "a.txt").as_deref(), Some("alpha\n"));
    assert!(made.export.failed.is_empty(), "{:?}", made.export.failed);
    let committed = git(&cwd, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(committed.lines().any(|line| line == "src/lib.rs"), "{committed}");
    assert_eq!(file_hashes(&root), files_before, "the original folder is untouched");
    assert_eq!(read(&project, "lib.rs").as_deref(), Some("pub fn newer() {}\n"));
}
