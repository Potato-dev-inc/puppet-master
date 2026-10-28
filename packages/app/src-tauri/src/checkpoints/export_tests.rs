//! Tests for `export::export_into`: real `git`, real temp folders. The source folder must never change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::engine::{self, CheckpointError, Kind, Limits, Snapshot};
use super::export::{export_into, ExportReport};
use super::git::git_available;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-export-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    fn p(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        // git object files are read-only on Windows.
        fn unlock(dir: &Path) {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        unlock(&p);
                    } else if let Ok(m) = std::fs::metadata(&p) {
                        let mut perms = m.permissions();
                        #[allow(clippy::permissions_set_readonly_false)]
                        perms.set_readonly(false);
                        let _ = std::fs::set_permissions(&p, perms);
                    }
                }
            }
        }
        unlock(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A folder link (a junction on Windows, a symlink elsewhere). Removed when the test ends, before
/// the temp folder is cleaned up, so the cleanup never walks through it. Removes the link only.
struct Link(PathBuf);

impl Drop for Link {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0).or_else(|_| std::fs::remove_file(&self.0));
    }
}

fn link_dir(target: &Path, link: &Path) -> Link {
    #[cfg(windows)]
    let made = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(target, link).is_ok();
    assert!(made, "could not link {} to {}", link.display(), target.display());
    Link(link.to_path_buf())
}

fn needs_git() -> bool {
    if !git_available() {
        eprintln!("git is not installed: export tests skipped");
    }
    git_available()
}

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn get(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

fn repo_in(t: &Tmp) -> PathBuf {
    let repo = t.p().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    repo
}

/// A copy folder in the place Puppet Master makes copies: `<project>/.puppet-master/worktrees/<name>`.
fn copy_in(repo: &Path, name: &str) -> PathBuf {
    let copy = repo.join(".puppet-master").join("worktrees").join(name);
    std::fs::create_dir_all(&copy).unwrap();
    copy
}

fn snap(project: &Path, pane: &str, kind: Kind, at: u64, limits: &Limits) -> engine::Checkpoint {
    match engine::snapshot(project, pane, kind, None, at, limits).expect("snapshot") {
        Snapshot::Created(c) => c,
        Snapshot::Unchanged(_) => panic!("expected a new save point"),
    }
}

fn assert_refused(result: Result<ExportReport, CheckpointError>) {
    match result {
        Err(CheckpointError::Other(m)) => assert_eq!(m, "not a separate copy"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Sha-256 of every file under `root`, by relative path. The `.puppet-master` store is left out: it is
/// the app's own data, and its refs and index have their own test.
fn tree_hashes(root: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if rel == ".puppet-master" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.insert(rel, format!("{:x}", Sha256::digest(bytes)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn git_out(git_dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn no_temp_index(store: &Path) -> bool {
    !std::fs::read_dir(store)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("pm-export-"))
}

#[test]
fn export_writes_an_old_save_point_into_a_copy() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("old");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let first = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "b.txt", "bee");
    snap(&repo, "p1", Kind::After, 2_000, &lim);
    put(&repo, "a.txt", "3");
    snap(&repo, "p1", Kind::After, 3_000, &lim);

    // The copy holds the newest files.
    let copy = copy_in(&repo, "x");
    put(&copy, "a.txt", "3");
    put(&copy, "b.txt", "bee");
    let report = export_into(&repo, "p1", &first.id, &copy, &lim).expect("export");
    assert_eq!(get(&copy, "a.txt").as_deref(), Some("1"));
    assert!(!copy.join("b.txt").exists(), "files made after the save point are removed");
    assert_eq!((report.written, report.removed), (1, 1), "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(report.tree, first.tree);
    assert_eq!(get(&repo, "a.txt").as_deref(), Some("3"), "the source folder is untouched");
}

#[test]
fn ignored_and_oversized_files_in_the_copy_are_left_alone() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("keep");
    let repo = repo_in(&t);
    let lim = Limits { max_file_bytes: 16, ..Limits::default() };
    put(&repo, ".gitignore", "ignored.log\n");
    put(&repo, "a.txt", "one");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);

    let copy = copy_in(&repo, "x");
    put(&copy, ".gitignore", "ignored.log\n");
    put(&copy, "a.txt", "two");
    put(&copy, "ignored.log", "keep");
    put(&copy, "big.bin", "0123456789012345678901234567890123456789");
    let report = export_into(&repo, "p1", &save.id, &copy, &lim).expect("export");
    assert_eq!(get(&copy, "a.txt").as_deref(), Some("one"));
    assert_eq!(get(&copy, "ignored.log").as_deref(), Some("keep"), "ignored files are never touched");
    assert_eq!(get(&copy, "big.bin").map(|s| s.len()), Some(40), "files over the size limit are never touched");
    assert_eq!((report.written, report.removed), (1, 0), "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
}

#[test]
fn refuses_the_original_folder() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("same");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "a.txt", "2");
    assert_refused(export_into(&repo, "p1", &save.id, &repo, &lim));
    assert_eq!(get(&repo, "a.txt").as_deref(), Some("2"));
}

#[test]
fn refuses_an_ancestor_of_the_original_folder() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("ancestor");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "a.txt", "2");
    assert_refused(export_into(&repo, "p1", &save.id, t.p(), &lim));
    assert_refused(export_into(&repo, "p1", &save.id, &repo.join(".puppet-master"), &lim));
    assert_eq!(get(&repo, "a.txt").as_deref(), Some("2"));
}

#[test]
fn refuses_a_folder_without_the_worktrees_part() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("parts");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "elsewhere/keep.txt", "keep");
    put(&repo, ".puppet-master/x/keep.txt", "keep");
    put(&t.p().join("other"), "keep.txt", "keep");

    assert_refused(export_into(&repo, "p1", &save.id, &repo.join("elsewhere"), &lim));
    assert_refused(export_into(&repo, "p1", &save.id, &repo.join(".puppet-master").join("x"), &lim));
    assert_refused(export_into(&repo, "p1", &save.id, &repo.join(".puppet-master").join("checkpoints.git"), &lim));
    assert_refused(export_into(&repo, "p1", &save.id, &t.p().join("other"), &lim));
    assert_eq!(get(&repo, "elsewhere/keep.txt").as_deref(), Some("keep"));
    assert_eq!(get(&repo, ".puppet-master/x/keep.txt").as_deref(), Some("keep"));
    assert_eq!(get(&t.p().join("other"), "keep.txt").as_deref(), Some("keep"));
}

#[test]
fn refuses_a_link_to_the_original_folder() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("link");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "a.txt", "2");
    let worktrees = repo.join(".puppet-master").join("worktrees");
    std::fs::create_dir_all(&worktrees).unwrap();
    let _link = link_dir(&repo, &worktrees.join("link"));
    assert_refused(export_into(&repo, "p1", &save.id, &worktrees.join("link"), &lim));
    assert_eq!(get(&repo, "a.txt").as_deref(), Some("2"));
}

#[test]
fn project_files_are_unchanged_by_an_export() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("hashes");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    put(&repo, "src/main.rs", "fn main() {}");
    put(&repo, ".gitignore", "target/\n");
    let first = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "a.txt", "2");
    put(&repo, "src/lib.rs", "pub fn x() {}");
    snap(&repo, "p1", Kind::After, 2_000, &lim);
    let copy = copy_in(&repo, "x");
    put(&copy, "a.txt", "9");
    let before = tree_hashes(&repo);

    let report = export_into(&repo, "p1", &first.id, &copy, &lim).expect("export");
    assert_eq!(tree_hashes(&repo), before, "every file of the project is unchanged");
    assert!(report.failed.is_empty(), "{report:?}");
}

#[test]
fn refs_and_the_per_pane_index_of_the_source_are_unchanged() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("refs");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let first = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    put(&repo, "a.txt", "2");
    snap(&repo, "p1", Kind::After, 2_000, &lim);
    put(&repo, "a.txt", "3");
    snap(&repo, "p1", Kind::After, 3_000, &lim);

    let store = repo.join(".puppet-master").join("checkpoints.git");
    let refs_before = git_out(&store, &["for-each-ref", "refs/pm"]);
    let index_before = std::fs::read(store.join("pm-index-p1")).expect("the per-pane index");
    assert!(!refs_before.is_empty());

    let copy = copy_in(&repo, "x");
    export_into(&repo, "p1", &first.id, &copy, &lim).expect("export");
    assert_eq!(git_out(&store, &["for-each-ref", "refs/pm"]), refs_before);
    assert_eq!(std::fs::read(store.join("pm-index-p1")).unwrap(), index_before);
}

#[test]
fn the_temp_index_is_removed_on_success_and_on_error() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("tmpindex");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    let store = repo.join(".puppet-master").join("checkpoints.git");

    let copy = copy_in(&repo, "x");
    put(&copy, "a.txt", "2");
    export_into(&repo, "p1", &save.id, &copy, &lim).expect("export");
    assert!(no_temp_index(&store), "the temp index is left behind after success");

    // Too much data in the copy: the export stops in the size check.
    put(&copy, "a.txt", "12345");
    let tight = Limits { max_total_bytes: 2, ..Limits::default() };
    let err = export_into(&repo, "p1", &save.id, &copy, &tight);
    assert!(matches!(err, Err(CheckpointError::TooLarge(_))), "{err:?}");
    assert!(no_temp_index(&store), "the temp index is left behind after an error");
}

#[test]
fn an_unknown_save_point_is_an_error_and_writes_nothing() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("unknown");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);
    let copy = copy_in(&repo, "x");
    put(&copy, "keep.txt", "keep");

    let err = export_into(&repo, "p1", "999", &copy, &lim);
    assert!(matches!(err, Err(CheckpointError::Other(ref m)) if m == "That save point is gone."), "{err:?}");
    // A pane with no save points gives the same answer.
    let err = export_into(&repo, "nobody", &save.id, &copy, &lim);
    assert!(matches!(err, Err(CheckpointError::Other(ref m)) if m == "That save point is gone."), "{err:?}");
    assert_eq!(get(&copy, "keep.txt").as_deref(), Some("keep"));
    assert_eq!(get(&copy, "a.txt"), None);
}

#[test]
fn links_inside_the_copy_are_never_written_or_deleted() {
    if !needs_git() {
        return;
    }
    let t = Tmp::new("escape");
    let repo = repo_in(&t);
    let lim = Limits::default();
    put(&repo, "a.txt", "1");
    put(&repo, "sub/keep.txt", "from the save point");
    let save = snap(&repo, "p1", Kind::Before, 1_000, &lim);

    // A folder outside the copy, reachable from inside it through a link.
    put(&t.p().join("outside"), "victim.txt", "outside");
    put(&t.p().join("outside"), "keep.txt", "outside keep");
    let outside = t.p().join("outside");
    let before = tree_hashes(&outside);

    let copy = copy_in(&repo, "x");
    put(&copy, "a.txt", "9");
    let _link = link_dir(&outside, &copy.join("sub"));
    let report = export_into(&repo, "p1", &save.id, &copy, &lim).expect("export");
    assert_eq!(tree_hashes(&outside), before, "nothing outside the copy changed");
    assert_eq!(get(&copy, "a.txt").as_deref(), Some("1"));
    assert!(!report.failed.is_empty(), "paths through the link are reported: {report:?}");
    assert!(report.failed.iter().all(|p| p.starts_with("sub/")), "{report:?}");
}
